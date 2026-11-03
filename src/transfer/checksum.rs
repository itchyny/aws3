//! The validation of the full object checksums of multipart downloads, by
//! combining the checksums of the parts in the same way as
//! `FullObjectChecksumCombiner` of s3transfer.

use aws_sdk_s3::operation::head_object::HeadObjectOutput;
use aws_sdk_s3::types::ChecksumType;
use aws_smithy_types::base64;
use crc_fast::CrcAlgorithm;

/// The checksum of the whole object, which is available on HeadObject with
/// the checksum mode enabled.
#[derive(Debug, Clone)]
pub struct FullObjectChecksum {
    name: &'static str,
    algorithm: CrcAlgorithm,
    width: usize,
    expected: String,
}

/// The checksum of a part, which is combined in the order of the offsets.
#[derive(Debug, Clone, Copy)]
pub struct PartChecksum {
    pub offset: u64,
    pub checksum: u64,
    pub length: u64,
}

/// The digest of a part being downloaded.
pub struct PartDigest {
    offset: u64,
    digest: crc_fast::Digest,
    length: u64,
}

impl PartDigest {
    pub fn update(&mut self, data: &[u8]) {
        self.digest.update(data);
        self.length += data.len() as u64;
    }

    pub fn finish(self) -> PartChecksum {
        PartChecksum {
            offset: self.offset,
            checksum: self.digest.finalize(),
            length: self.length,
        }
    }
}

impl FullObjectChecksum {
    /// Returns the full object checksum of the object, in the same way as
    /// `resolve_full_object_checksum` of s3transfer.
    pub fn from_head(head: &HeadObjectOutput) -> Option<Self> {
        if head.checksum_type() != Some(&ChecksumType::FullObject) {
            return None;
        }
        let checksums = [
            (
                "crc32",
                CrcAlgorithm::Crc32IsoHdlc,
                4,
                head.checksum_crc32(),
            ),
            (
                "crc32c",
                CrcAlgorithm::Crc32Iscsi,
                4,
                head.checksum_crc32_c(),
            ),
            (
                "crc64nvme",
                CrcAlgorithm::Crc64Nvme,
                8,
                head.checksum_crc64_nvme(),
            ),
        ];
        let (name, algorithm, width, expected) =
            checksums
                .into_iter()
                .find_map(|(name, algorithm, width, expected)| {
                    Some((
                        name,
                        algorithm,
                        width,
                        expected.filter(|value| !value.is_empty())?,
                    ))
                })?;
        Some(Self {
            name,
            algorithm,
            width,
            expected: expected.to_owned(),
        })
    }

    /// Returns the digest of the part at the offset, which is updated with
    /// the data of the part in order.
    pub fn part(&self, offset: u64) -> PartDigest {
        PartDigest {
            offset,
            digest: crc_fast::Digest::new(self.algorithm),
            length: 0,
        }
    }

    /// Combines the checksums of the parts, and returns the error message if
    /// it does not match the full object checksum.
    pub fn validate(&self, mut parts: Vec<PartChecksum>) -> Result<(), String> {
        parts.sort_by_key(|part| part.offset);
        let Some((first, rest)) = parts.split_first() else {
            return Ok(());
        };
        let combined = rest.iter().fold(first.checksum, |checksum, part| {
            crc_fast::checksum_combine(self.algorithm, checksum, part.checksum, part.length)
        });
        let combined = &combined.to_be_bytes()[8 - self.width..];
        if base64::decode(&self.expected).is_ok_and(|expected| expected == combined) {
            return Ok(());
        }
        Err(format!(
            "Expected full object checksum ({}) {} did not match combined checksum: {}",
            self.name,
            self.expected,
            base64::encode(combined)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate() {
        let data: Vec<u8> = (0..10_000u32).map(|i| (i * 7 % 251) as u8).collect();
        for (algorithm, width) in [
            (CrcAlgorithm::Crc32IsoHdlc, 4),
            (CrcAlgorithm::Crc32Iscsi, 4),
            (CrcAlgorithm::Crc64Nvme, 8),
        ] {
            let whole = crc_fast::checksum(algorithm, &data);
            let checksum = FullObjectChecksum {
                name: "crc",
                algorithm,
                width,
                expected: base64::encode(&whole.to_be_bytes()[8 - width..]),
            };
            // The parts are updated in pieces, and combined in the order of
            // the offsets.
            let parts: Vec<_> = data
                .chunks(3000)
                .enumerate()
                .map(|(i, chunk)| {
                    let mut part = checksum.part(i as u64 * 3000);
                    chunk.chunks(700).for_each(|piece| part.update(piece));
                    part.finish()
                })
                .rev()
                .collect();
            assert_eq!(checksum.validate(parts.clone()), Ok(()));
            let mut broken = parts;
            broken[0].checksum ^= 1;
            assert!(
                checksum
                    .validate(broken)
                    .unwrap_err()
                    .starts_with("Expected full object checksum (crc) ")
            );
        }
    }
}
