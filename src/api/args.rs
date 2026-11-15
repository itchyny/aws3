//! The parsers of the arguments shared by the operations.

use std::collections::HashMap;

use aws_sdk_s3::primitives::DateTime;
use aws_sdk_s3::types::ChecksumAlgorithm;
use aws_smithy_types::base64;
use md5::{Digest, Md5};

use super::shape;
use crate::error::Error;
use crate::param;
use crate::value::{Value, python_str};

/// Parses the integer argument.
pub fn parse_int(value: Option<&str>) -> Result<Option<i32>, Error> {
    value
        .map(|value| {
            i32::try_from(shape::python_int(value)?)
                .map_err(|_| Error::new(format!("Integer value out of range: {}", value.trim())))
        })
        .transpose()
}

/// Returns the checksum algorithm of the requests whose checksums are
/// required, which defaults to CRC64NVME like the AWS CLI.
pub fn required_checksum_algorithm(value: Option<&str>) -> ChecksumAlgorithm {
    value.map_or(ChecksumAlgorithm::Crc64Nvme, ChecksumAlgorithm::from)
}

/// Parses the timestamp argument of the member, failing with the same message
/// as the parameter validation of botocore.
pub fn parse_timestamp(member: &str, value: Option<&str>) -> Result<Option<DateTime>, Error> {
    value
        .map(|value| {
            param::parse_timestamp(value).map_err(|_| {
                Error::param_validation(format!(
                    "Parameter validation failed:\nInvalid type for parameter {member}, \
                     value: {value}, type: <class 'str'>, \
                     valid types: <class 'datetime.datetime'>, timestamp-string"
                ))
            })
        })
        .transpose()
}

/// Parses the `--metadata` argument, whose values must be ASCII strings like
/// `validate_ascii_metadata` of botocore.
pub fn parse_metadata(value: Option<&str>) -> Result<Option<HashMap<String, String>>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    let (pairs, _) = param::parse_map_argument("--metadata", value)?;
    let mut metadata = HashMap::new();
    for (key, value) in pairs {
        let Value::Str(value) = value else {
            return Err(Error::param_validation(format!(
                "Parameter validation failed:\nInvalid type for parameter Metadata.{key}, \
                 value: {}, type: <class '{}'>, valid types: <class 'str'>",
                python_str(&value),
                shape::type_name(&value)
            )));
        };
        if !key.is_ascii() || !value.is_ascii() {
            return Err(Error::param_validation(format!(
                "Parameter validation failed:\nNon ascii characters found in S3 metadata for \
                 key \"{key}\", value: \"{value}\".  \nS3 metadata can only contain ASCII \
                 characters. "
            )));
        }
        metadata.insert(key, value);
    }
    Ok(Some(metadata))
}

/// Returns the customer-provided key and its MD5 digest, where the key is
/// base64 encoded and the digest is calculated unless the digest is given,
/// like `sse_md5` of botocore.
pub fn sse_customer_key(
    option: &str,
    key: Option<&str>,
    key_md5: Option<&str>,
) -> Result<(Option<String>, Option<String>), Error> {
    let key_md5 = key_md5.map(str::to_owned);
    let Some(key) = param::load_key(option, key)? else {
        return Ok((None, key_md5));
    };
    Ok(match key_md5 {
        Some(key_md5) => (
            Some(String::from_utf8_lossy(&key).into_owned()),
            Some(key_md5),
        ),
        None => (
            Some(base64::encode(&key)),
            Some(base64::encode(Md5::digest(&key))),
        ),
    })
}
