//! The put-object operation.

use std::io::Read;

use aws_sdk_s3::Client;
use aws_sdk_s3::config::RequestChecksumCalculation;
use aws_sdk_s3::operation::put_object::builders::PutObjectFluentBuilder;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{
    ChecksumAlgorithm, ObjectCannedAcl, ObjectLockEventHold, ObjectLockLegalHoldStatus,
    ObjectLockMode, RequestPayer, ServerSideEncryption, StorageClass,
};
use aws_smithy_checksums::ChecksumAlgorithm as Checksums;
use aws_smithy_types::base64;
use aws_smithy_types::body::SdkBody;

use super::output::Response;
use super::{parse_int, parse_metadata, parse_timestamp, sse_customer_key};
use crate::cli::PutObjectArgs;
use crate::client::Config;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::transfer::merge_content_encodings;
use crate::value::Value;

pub async fn run(config: &Config, client: &Client, args: &PutObjectArgs) -> Result<Value, Error> {
    let body = body(args.body.as_deref()).await?;
    let metadata = parse_metadata(args.metadata.as_deref())?;
    let expires = parse_timestamp("Expires", args.expires.as_deref())?;
    let object_lock_retain_until_date = parse_timestamp(
        "ObjectLockRetainUntilDate",
        args.object_lock_retain_until_date.as_deref(),
    )?;
    let object_lock_event_hold_duration_days =
        parse_int(args.object_lock_event_hold_duration_days.as_deref())?;
    let object_lock_event_hold_duration_years =
        parse_int(args.object_lock_event_hold_duration_years.as_deref())?;
    let (sse_customer_key, sse_customer_key_md5) = sse_customer_key(
        "--sse-customer-key",
        args.sse_customer_key.as_deref(),
        args.sse_customer_key_md5.as_deref(),
    )?;
    let checksum_algorithm = checksum_algorithm(client, args);
    let checksum = header_checksum(config, args, checksum_algorithm.as_ref()).await?;
    let checksums = &args.checksums;
    let has_content_type = args.content_type.is_some();
    let output = Redirector::default()
        .send(client, &args.bucket, "PutObject", |client| {
            client
                .put_object()
                .set_acl(args.acl.as_deref().map(ObjectCannedAcl::from))
                .body(ByteStream::new(
                    body.try_clone().unwrap_or_else(SdkBody::taken),
                ))
                .bucket(&args.bucket)
                .set_cache_control(args.cache_control.clone())
                .set_content_disposition(args.content_disposition.clone())
                .set_content_encoding(args.content_encoding.clone())
                .set_content_language(args.content_language.clone())
                .set_content_length(args.content_length)
                .set_content_md5(args.content_md5.clone())
                .set_content_type(args.content_type.clone())
                .set_checksum_algorithm(checksum_algorithm.clone())
                .set_checksum_crc32(checksums.checksum_crc32.clone())
                .set_checksum_crc32_c(checksums.checksum_crc32_c.clone())
                .set_checksum_crc64_nvme(checksums.checksum_crc64_nvme.clone())
                .set_checksum_sha1(checksums.checksum_sha1.clone())
                .set_checksum_sha256(checksums.checksum_sha256.clone())
                .set_checksum_sha512(checksums.checksum_sha512.clone())
                .set_checksum_md5(checksums.checksum_md5.clone())
                .set_checksum_xxhash64(checksums.checksum_xxhash64.clone())
                .set_checksum_xxhash3(checksums.checksum_xxhash3.clone())
                .set_checksum_xxhash128(checksums.checksum_xxhash128.clone())
                .set_checksum(checksum_algorithm.as_ref(), checksum.clone())
                .set_expires(expires)
                .set_if_match(args.if_match.clone())
                .set_if_none_match(args.if_none_match.clone())
                .set_grant_full_control(args.grant_full_control.clone())
                .set_grant_read(args.grant_read.clone())
                .set_grant_read_acp(args.grant_read_acp.clone())
                .set_grant_write_acp(args.grant_write_acp.clone())
                .key(&args.key)
                .set_write_offset_bytes(args.write_offset_bytes)
                .set_metadata(metadata.clone())
                .set_server_side_encryption(
                    args.server_side_encryption
                        .as_deref()
                        .map(ServerSideEncryption::from),
                )
                .set_storage_class(args.storage_class.as_deref().map(StorageClass::from))
                .set_website_redirect_location(args.website_redirect_location.clone())
                .set_sse_customer_algorithm(args.sse_customer_algorithm.clone())
                .set_sse_customer_key(sse_customer_key.clone())
                .set_sse_customer_key_md5(sse_customer_key_md5.clone())
                .set_ssekms_key_id(args.ssekms_key_id.clone())
                .set_ssekms_encryption_context(args.ssekms_encryption_context.clone())
                .set_bucket_key_enabled(args.bucket_key_enabled.value())
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
                .set_tagging(args.tagging.clone())
                .set_object_lock_mode(args.object_lock_mode.as_deref().map(ObjectLockMode::from))
                .set_object_lock_retain_until_date(object_lock_retain_until_date)
                .set_object_lock_legal_hold_status(
                    args.object_lock_legal_hold_status
                        .as_deref()
                        .map(ObjectLockLegalHoldStatus::from),
                )
                .set_object_lock_event_hold(
                    args.object_lock_event_hold
                        .as_deref()
                        .map(ObjectLockEventHold::from),
                )
                .set_object_lock_event_hold_duration_days(object_lock_event_hold_duration_days)
                .set_object_lock_event_hold_duration_years(object_lock_event_hold_duration_years)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .customize()
                .mutate_request(move |request| {
                    // The SDK sends the default content type of the body,
                    // whereas the AWS CLI leaves it to S3, which defaults to
                    // binary/octet-stream.
                    if !has_content_type {
                        request.headers_mut().remove("content-type");
                    }
                    merge_content_encodings(request.headers_mut());
                })
                .send()
        })
        .await
        .map_err(|err| Error::sdk("PutObject", err))?;
    let mut response = Response::default();
    response.str("Expiration", output.expiration());
    response.str("ETag", output.e_tag());
    response.str("ChecksumCRC32", output.checksum_crc32());
    response.str("ChecksumCRC32C", output.checksum_crc32_c());
    response.str("ChecksumCRC64NVME", output.checksum_crc64_nvme());
    response.str("ChecksumSHA1", output.checksum_sha1());
    response.str("ChecksumSHA256", output.checksum_sha256());
    response.str("ChecksumSHA512", output.checksum_sha512());
    response.str("ChecksumMD5", output.checksum_md5());
    response.str("ChecksumXXHASH64", output.checksum_xxhash64());
    response.str("ChecksumXXHASH3", output.checksum_xxhash3());
    response.str("ChecksumXXHASH128", output.checksum_xxhash128());
    response.str("ChecksumType", output.checksum_type());
    response.str("ServerSideEncryption", output.server_side_encryption());
    response.str("VersionId", output.version_id());
    response.str("SSECustomerAlgorithm", output.sse_customer_algorithm());
    response.str("SSECustomerKeyMD5", output.sse_customer_key_md5());
    response.str("SSEKMSKeyId", output.ssekms_key_id());
    response.str(
        "SSEKMSEncryptionContext",
        output.ssekms_encryption_context(),
    );
    response.bool("BucketKeyEnabled", output.bucket_key_enabled());
    response.int("Size", output.size());
    response.str("RequestCharged", output.request_charged());
    Ok(response.build())
}

/// Returns the body of the file, which is empty without the path. The file
/// is read on sending, and again on redirects.
async fn body(path: Option<&str>) -> Result<SdkBody, Error> {
    let Some(path) = path else {
        return Ok(SdkBody::empty());
    };
    if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        return Err(Error::param_validation(
            "Error parsing parameter '--body': Blob values must be a path to a file.",
        ));
    }
    let stream = ByteStream::from_path(path)
        .await
        .map_err(|err| Error::new(err.to_string()))?;
    Ok(stream.into_inner())
}

/// Returns the checksum of the file computed before sending over HTTP, which
/// is sent in the header instead of the trailer of the aws-chunked encoding,
/// since botocore sends the trailers only over TLS.
async fn header_checksum(
    config: &Config,
    args: &PutObjectArgs,
    algorithm: Option<&ChecksumAlgorithm>,
) -> Result<Option<String>, Error> {
    let (Some(algorithm), Some(path)) = (algorithm, &args.body) else {
        return Ok(None);
    };
    let is_http = config.endpoint_url.as_deref().is_some_and(|url| {
        url.get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
    });
    if !is_http || !args.checksums.is_empty() {
        return Ok(None);
    }
    // The MD5 of the SDK is an alias of CRC32, which is not computed here.
    let Some(checksum) = (*algorithm != ChecksumAlgorithm::Md5)
        .then(|| algorithm.as_str().parse::<Checksums>().ok())
        .flatten()
    else {
        return Ok(None);
    };
    let path = path.clone();
    tokio::task::spawn_blocking(move || {
        let mut checksum = checksum.into_impl();
        let mut file = std::fs::File::open(path)?;
        let mut buffer = vec![0; 1 << 20];
        loop {
            match file.read(&mut buffer)? {
                0 => break,
                n => checksum.update(&buffer[..n]),
            }
        }
        Ok(Some(base64::encode(checksum.finalize())))
    })
    .await
    .map_err(|err| Error::new(err.to_string()))?
}

/// Sets the checksum of the algorithm computed before sending.
trait SetChecksum {
    fn set_checksum(self, algorithm: Option<&ChecksumAlgorithm>, value: Option<String>) -> Self;
}

impl SetChecksum for PutObjectFluentBuilder {
    fn set_checksum(self, algorithm: Option<&ChecksumAlgorithm>, value: Option<String>) -> Self {
        let Some(value) = value else {
            return self;
        };
        match algorithm {
            Some(ChecksumAlgorithm::Crc32) => self.checksum_crc32(value),
            Some(ChecksumAlgorithm::Crc32C) => self.checksum_crc32_c(value),
            Some(ChecksumAlgorithm::Crc64Nvme) => self.checksum_crc64_nvme(value),
            Some(ChecksumAlgorithm::Sha1) => self.checksum_sha1(value),
            Some(ChecksumAlgorithm::Sha256) => self.checksum_sha256(value),
            _ => self,
        }
    }
}

/// Returns the checksum algorithm, which defaults to CRC64NVME unless the
/// checksum is given or `request_checksum_calculation` is `when_required`,
/// like the AWS CLI.
fn checksum_algorithm(client: &Client, args: &PutObjectArgs) -> Option<ChecksumAlgorithm> {
    args.checksum_algorithm
        .as_deref()
        .map(ChecksumAlgorithm::from)
        .or_else(|| {
            let calculation = client.config().request_checksum_calculation();
            (args.checksums.is_empty()
                && calculation != Some(&RequestChecksumCalculation::WhenRequired))
            .then_some(ChecksumAlgorithm::Crc64Nvme)
        })
}
