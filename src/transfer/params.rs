//! Request parameters of objects mapped from the command line options, in the
//! same way as `RequestParamsMapper` of the AWS CLI.

use std::collections::HashMap;

use aws_sdk_s3::primitives::DateTime;
use aws_sdk_s3::types::{
    ChecksumAlgorithm, ChecksumMode, MetadataDirective, ObjectCannedAcl, RequestPayer,
    ServerSideEncryption, StorageClass,
};
use aws_smithy_types::base64;
use md5::{Digest, Md5};

use crate::cli::{self, BinaryFormat, GlobalArgs, TransferArgs};

/// The customer-provided key of server-side encryption.
#[derive(Debug, Clone)]
pub struct SseCustomerKey {
    pub algorithm: String,
    pub key: String,
    pub key_md5: String,
}

#[derive(Debug, Clone, Default)]
pub struct RequestParams {
    pub acl: Option<ObjectCannedAcl>,
    pub storage_class: Option<StorageClass>,
    pub website_redirect: Option<String>,
    pub content_type: Option<String>,
    pub cache_control: Option<String>,
    pub content_disposition: Option<String>,
    pub content_encoding: Option<String>,
    pub content_language: Option<String>,
    pub expires: Option<DateTime>,
    pub grant_read: Option<String>,
    pub grant_full_control: Option<String>,
    pub grant_read_acp: Option<String>,
    pub grant_write_acp: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
    pub metadata_directive: Option<MetadataDirective>,
    pub sse: Option<ServerSideEncryption>,
    pub sse_kms_key_id: Option<String>,
    pub sse_c: Option<SseCustomerKey>,
    pub sse_c_copy_source: Option<SseCustomerKey>,
    pub request_payer: Option<RequestPayer>,
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
    pub checksum_mode: Option<ChecksumMode>,
    pub no_overwrite: bool,
}

impl RequestParams {
    /// Creates the request parameters, where errors are reported as fatal
    /// errors like the AWS CLI does on submitting transfers.
    pub fn new(args: &TransferArgs, global: &GlobalArgs) -> Result<Self, String> {
        let mut params = Self {
            acl: args
                .acl
                .map(|acl| ObjectCannedAcl::from(value_name(acl).as_str())),
            storage_class: args
                .storage_class
                .map(|class| StorageClass::from(value_name(class).as_str())),
            website_redirect: args.website_redirect.clone(),
            content_type: args.content_type.clone(),
            cache_control: args.cache_control.clone(),
            content_disposition: args.content_disposition.clone(),
            content_encoding: args.content_encoding.clone(),
            content_language: args.content_language.clone(),
            expires: args.expires.as_deref().map(parse_timestamp).transpose()?,
            metadata: args.metadata.as_deref().map(parse_map).transpose()?,
            metadata_directive: args
                .metadata_directive
                .map(|directive| MetadataDirective::from(value_name(directive).as_str())),
            sse: args
                .sse
                .map(|sse| ServerSideEncryption::from(value_name(sse).as_str())),
            sse_kms_key_id: args.sse_kms_key_id.clone(),
            sse_c: sse_customer_key(args.sse_c, args.sse_c_key.as_deref(), global)?,
            sse_c_copy_source: sse_customer_key(
                args.sse_c_copy_source,
                args.sse_c_copy_source_key.as_deref(),
                global,
            )?,
            request_payer: request_payer(args.request_payer.request_payer),
            checksum_algorithm: args
                .checksum_algorithm
                .map(|algorithm| ChecksumAlgorithm::from(value_name(algorithm).as_str())),
            checksum_mode: args.checksum_mode.map(|_| ChecksumMode::Enabled),
            no_overwrite: args.no_overwrite,
            ..Self::default()
        };
        for grant in &args.grants {
            let (permission, grantee) = grant
                .split_once('=')
                .ok_or("grants should be of the form permission=principal")?;
            let field = match permission {
                "read" => &mut params.grant_read,
                "full" => &mut params.grant_full_control,
                "readacl" => &mut params.grant_read_acp,
                "writeacl" => &mut params.grant_write_acp,
                _ => return Err("permission must be one of: read|readacl|writeacl|full".into()),
            };
            *field = Some(grantee.to_owned());
        }
        Ok(params)
    }

    /// Returns whether any of the properties that CopyObject replaces with
    /// the REPLACE metadata directive is specified.
    pub fn has_metadata_props(&self) -> bool {
        self.cache_control.is_some()
            || self.content_disposition.is_some()
            || self.content_encoding.is_some()
            || self.content_language.is_some()
            || self.content_type.is_some()
            || self.expires.is_some()
            || self.metadata.is_some()
    }
}

/// Sets the general object parameters, i.e. ACL, storage class, website
/// redirect, content headers, grants, metadata and server-side encryption.
macro_rules! set_object_params {
    ($builder:expr, $params:expr) => {
        $builder
            .set_acl($params.acl.clone())
            .set_storage_class($params.storage_class.clone())
            .set_website_redirect_location($params.website_redirect.clone())
            .set_content_type($params.content_type.clone())
            .set_cache_control($params.cache_control.clone())
            .set_content_disposition($params.content_disposition.clone())
            .set_content_encoding($params.content_encoding.clone())
            .set_content_language($params.content_language.clone())
            .set_expires($params.expires)
            .set_grant_read($params.grant_read.clone())
            .set_grant_full_control($params.grant_full_control.clone())
            .set_grant_read_acp($params.grant_read_acp.clone())
            .set_grant_write_acp($params.grant_write_acp.clone())
            .set_metadata($params.metadata.clone())
            .set_server_side_encryption($params.sse.clone())
            .set_ssekms_key_id($params.sse_kms_key_id.clone())
            .set_request_payer($params.request_payer.clone())
    };
}
pub(crate) use set_object_params;

/// Sets the customer-provided key of server-side encryption.
macro_rules! set_sse_c {
    ($builder:expr, $key:expr) => {{
        let key: &Option<$crate::transfer::params::SseCustomerKey> = $key;
        $builder
            .set_sse_customer_algorithm(key.as_ref().map(|key| key.algorithm.clone()))
            .set_sse_customer_key(key.as_ref().map(|key| key.key.clone()))
            .set_sse_customer_key_md5(key.as_ref().map(|key| key.key_md5.clone()))
    }};
}
pub(crate) use set_sse_c;

/// Sets the customer-provided key of server-side encryption of the source.
macro_rules! set_copy_source_sse_c {
    ($builder:expr, $key:expr) => {{
        let key: &Option<$crate::transfer::params::SseCustomerKey> = $key;
        $builder
            .set_copy_source_sse_customer_algorithm(key.as_ref().map(|key| key.algorithm.clone()))
            .set_copy_source_sse_customer_key(key.as_ref().map(|key| key.key.clone()))
            .set_copy_source_sse_customer_key_md5(key.as_ref().map(|key| key.key_md5.clone()))
    }};
}
pub(crate) use set_copy_source_sse_c;

/// Returns the name of the value on the command line.
fn value_name(value: impl clap::ValueEnum) -> String {
    value
        .to_possible_value()
        .map(|value| value.get_name().to_owned())
        .unwrap_or_default()
}

fn request_payer(payer: Option<cli::RequestPayer>) -> Option<RequestPayer> {
    payer.map(|cli::RequestPayer::Requester| RequestPayer::Requester)
}

fn sse_customer_key(
    algorithm: Option<cli::SseC>,
    key: Option<&str>,
    global: &GlobalArgs,
) -> Result<Option<SseCustomerKey>, String> {
    let (Some(algorithm), Some(key)) = (algorithm, key) else {
        return Ok(None);
    };
    let key = parse_blob(key, global)?;
    Ok(Some(SseCustomerKey {
        algorithm: value_name(algorithm),
        key: base64::encode(&key),
        key_md5: base64::encode(Md5::digest(&key)),
    }))
}

/// Parses the blob argument, which is base64 encoded unless the binary
/// format is `raw-in-base64-out`, or the content of a `fileb://` file.
fn parse_blob(value: &str, global: &GlobalArgs) -> Result<Vec<u8>, String> {
    if let Some(path) = value.strip_prefix("fileb://") {
        return std::fs::read(path)
            .map_err(|err| format!("Unable to load paramfile {value}: {err}"));
    }
    if global.cli_binary_format == Some(BinaryFormat::RawInBase64Out) {
        return Ok(value.as_bytes().to_vec());
    }
    base64::decode(value).map_err(|_| format!("Invalid base64: \"{value}\""))
}

/// Parses the timestamp argument, which is an ISO 8601 date time, an RFC 2822
/// date time or the epoch seconds. Date times without offsets are in UTC.
fn parse_timestamp(value: &str) -> Result<DateTime, String> {
    let date_time = chrono::DateTime::parse_from_rfc3339(value)
        .or_else(|_| chrono::DateTime::parse_from_rfc2822(value))
        .map(|date_time| date_time.to_utc())
        .or_else(|_| {
            [
                "%Y-%m-%dT%H:%M:%S%.f",
                "%Y-%m-%d %H:%M:%S%.f",
                "%Y-%m-%dT%H:%M:%S%.fZ",
            ]
            .iter()
            .find_map(|format| chrono::NaiveDateTime::parse_from_str(value, format).ok())
            .or_else(|| {
                chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                    .ok()
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
            })
            .map(|date_time| date_time.and_utc())
            .ok_or(())
        })
        .or_else(|_| {
            value
                .parse::<f64>()
                .ok()
                .and_then(|secs| chrono::DateTime::from_timestamp_millis((secs * 1000.0) as i64))
                .ok_or(())
        })
        .map_err(|_| format!("Invalid timestamp value: {value}"))?;
    Ok(DateTime::from_millis(date_time.timestamp_millis()))
}

/// Parses the map argument in the JSON syntax or the shorthand syntax, i.e.
/// `KeyName1=string,KeyName2=string`.
fn parse_map(value: &str) -> Result<HashMap<String, String>, String> {
    if value.trim_start().starts_with('{') {
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(value)
            .map_err(|err| format!("Invalid JSON: {err}\nJSON received: {value}"))?;
        return map
            .into_iter()
            .map(|(key, value)| match value {
                serde_json::Value::String(value) => Ok((key, value)),
                value => Ok((key, value.to_string())),
            })
            .collect();
    }
    let mut map = HashMap::new();
    for pair in split_shorthand(value) {
        let (key, value) = pair.split_once('=').ok_or_else(|| {
            format!(
                "Error parsing parameter '--metadata': Expected: '=', received: 'EOF' for input:\n{value}"
            )
        })?;
        map.insert(key.trim().to_owned(), unquote(value.trim()));
    }
    Ok(map)
}

/// Splits the shorthand syntax by commas outside of quotes.
fn split_shorthand(value: &str) -> Vec<String> {
    let mut pairs = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        match (c, quote) {
            ('\\', _) => {
                current.push(c);
                current.extend(chars.next());
            }
            ('\'' | '"', None) => {
                quote = Some(c);
                current.push(c);
            }
            (c, Some(q)) if c == q => {
                quote = None;
                current.push(c);
            }
            (',', None) => pairs.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        pairs.push(current);
    }
    pairs
}

fn unquote(value: &str) -> String {
    let inner = ['\'', '"'].iter().find_map(|&q| {
        value
            .strip_prefix(q)
            .and_then(|value| value.strip_suffix(q))
    });
    let mut result = String::new();
    let mut chars = inner.unwrap_or(value).chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            result.extend(chars.next());
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map() {
        let map = parse_map("a=1,b='x,y',c=\"z\"").unwrap();
        assert_eq!(map["a"], "1");
        assert_eq!(map["b"], "x,y");
        assert_eq!(map["c"], "z");
        let map = parse_map(r#"{"k": "v"}"#).unwrap();
        assert_eq!(map["k"], "v");
        assert!(parse_map("novalue").is_err());
    }

    #[test]
    fn timestamp() {
        let expected = DateTime::from_secs(1412195400);
        assert_eq!(parse_timestamp("2014-10-01T20:30:00Z").unwrap(), expected);
        assert_eq!(parse_timestamp("2014-10-01T20:30:00").unwrap(), expected);
        assert_eq!(parse_timestamp("1412195400").unwrap(), expected);
        assert!(parse_timestamp("tomorrow").is_err());
    }
}
