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

use crate::cli::{self, TransferArgs};
use crate::error::Error;
use crate::param::{load_key, parse_map_argument, parse_timestamp};
use crate::path::url_encode;
use crate::value::{Value, invalid_type};

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
    /// The tags of the objects uploaded from local paths, in the format of
    /// the tagging header.
    pub tagging: Option<String>,
    pub grant_read: Option<String>,
    pub grant_full_control: Option<String>,
    pub grant_read_acp: Option<String>,
    pub grant_write_acp: Option<String>,
    pub metadata: Option<HashMap<String, String>>,
    /// The first pair of the metadata with non-ASCII characters, in the
    /// order of the argument.
    pub non_ascii_metadata: Option<(String, String)>,
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
    pub fn new(args: &TransferArgs, unpacked: &Unpacked) -> Result<Self, String> {
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
            metadata: unpacked
                .metadata
                .as_ref()
                .map(|metadata| metadata.iter().cloned().collect()),
            non_ascii_metadata: unpacked.metadata.as_ref().and_then(|metadata| {
                metadata
                    .iter()
                    .find(|(key, value)| !key.is_ascii() || !value.is_ascii())
                    .cloned()
            }),
            metadata_directive: args
                .metadata_directive
                .map(|directive| MetadataDirective::from(value_name(directive).as_str())),
            sse: args
                .sse
                .map(|sse| ServerSideEncryption::from(value_name(sse).as_str())),
            sse_kms_key_id: args.sse_kms_key_id.clone(),
            sse_c: sse_customer_key(args.sse_c, unpacked.sse_c.as_deref()),
            sse_c_copy_source: sse_customer_key(
                args.sse_c_copy_source,
                unpacked.sse_c_copy_source.as_deref(),
            ),
            request_payer: request_payer(args.request_payer.request_payer),
            checksum_algorithm: args
                .checksum_algorithm
                .map(|algorithm| ChecksumAlgorithm::from(value_name(algorithm).as_str())),
            checksum_mode: args.checksum_mode.map(|_| ChecksumMode::Enabled),
            no_overwrite: args.no_overwrite,
            tagging: (!args.tags.is_empty()).then(|| {
                let tags: Vec<_> = args
                    .tags
                    .chunks(2)
                    .map(|tag| {
                        format!(
                            "{}={}",
                            url_encode(&tag[0], false),
                            url_encode(&tag[1], false)
                        )
                    })
                    .collect();
                tags.join("&")
            }),
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

    /// Creates the request parameters of commands which only take the
    /// request payer, e.g. rm.
    pub fn with_request_payer(payer: Option<cli::RequestPayer>) -> Self {
        Self {
            request_payer: request_payer(payer),
            ..Self::default()
        }
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

    /// Validates that the metadata contains only ASCII characters, like
    /// `validate_ascii_metadata` of botocore on PutObject, CopyObject, and
    /// CreateMultipartUpload.
    pub fn validate_ascii_metadata(&self) -> Result<(), String> {
        match &self.non_ascii_metadata {
            Some((key, value)) => Err(format!(
                "Parameter validation failed:\nNon ascii characters found in S3 metadata for \
                 key \"{key}\", value: \"{value}\".  \nS3 metadata can only contain ASCII \
                 characters. "
            )),
            None => Ok(()),
        }
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

fn sse_customer_key(algorithm: Option<cli::SseC>, key: Option<&[u8]>) -> Option<SseCustomerKey> {
    let (Some(algorithm), Some(key)) = (algorithm, key) else {
        return None;
    };
    Some(SseCustomerKey {
        algorithm: value_name(algorithm),
        key: base64::encode(key),
        key_md5: base64::encode(Md5::digest(key)),
    })
}

/// The arguments unpacked before the other arguments are validated, like
/// `unpack_argument` of the AWS CLI, which loads the parameter files and
/// parses the maps in the order of the arguments.
#[derive(Debug, Default)]
pub struct Unpacked {
    sse_c: Option<Vec<u8>>,
    sse_c_copy_source: Option<Vec<u8>>,
    metadata: Option<Vec<(String, String)>>,
}

impl Unpacked {
    pub fn load(args: &TransferArgs) -> Result<Self, Error> {
        Ok(Self {
            sse_c: load_key("--sse-c-key", args.sse_c_key.as_deref())?,
            sse_c_copy_source: load_key(
                "--sse-c-copy-source-key",
                args.sse_c_copy_source_key.as_deref(),
            )?,
            metadata: args.metadata.as_deref().map(parse_metadata).transpose()?,
        })
    }
}

/// Parses the `--metadata` argument in the JSON syntax or the shorthand
/// syntax, i.e. `KeyName1=string,KeyName2=string`, and validates that the
/// values are strings like `unpack_argument` of the AWS CLI, which reports
/// the invalid values in the representations of Python instead of JSON.
fn parse_metadata(value: &str) -> Result<Vec<(String, String)>, Error> {
    let (pairs, _) = parse_map_argument("--metadata", value)?;
    let mut metadata = Vec::new();
    let mut errors = Vec::new();
    for (key, value) in pairs {
        match value {
            Value::Str(value) => {
                metadata.push((key, value));
            }
            value => errors.push(invalid_type(&key, &value, "string")),
        }
    }
    if !errors.is_empty() {
        return Err(Error::param_validation(format!(
            "Parameter validation failed:\n{}",
            errors.join("\n")
        )));
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::{Cli, Command};

    fn parse(args: &[&str]) -> Result<RequestParams, String> {
        let args = ["aws3", "cp", "a", "s3://bucket/a"].iter().chain(args);
        let cli = Cli::try_parse_from(args).unwrap();
        let Command::Cp(cp) = &cli.command else {
            unreachable!()
        };
        let unpacked = Unpacked::load(&cp.transfer).map_err(|err| err.to_string())?;
        RequestParams::new(&cp.transfer, &unpacked)
    }

    #[test]
    fn tags() {
        let params = parse(&["--tags", "k1", "v 1", "--tags", "a&b=c", "\u{fc}/~"]).unwrap();
        assert_eq!(
            params.tagging.as_deref(),
            Some("k1=v%201&a%26b%3Dc=%C3%BC%2F~")
        );
        assert_eq!(parse(&[]).unwrap().tagging, None);
    }

    #[test]
    fn grants() {
        let params = parse(&[
            "--grants",
            "read=uri=http://acs.amazonaws.com/groups/global/AllUsers",
            "full=id=abc",
            "readacl=id=x",
            "writeacl=emailaddress=a@example.com",
        ])
        .unwrap();
        assert_eq!(
            params.grant_read.as_deref(),
            Some("uri=http://acs.amazonaws.com/groups/global/AllUsers")
        );
        assert_eq!(params.grant_full_control.as_deref(), Some("id=abc"));
        assert_eq!(params.grant_read_acp.as_deref(), Some("id=x"));
        assert_eq!(
            params.grant_write_acp.as_deref(),
            Some("emailaddress=a@example.com")
        );
        assert_eq!(
            parse(&["--grants", "foo"]).unwrap_err(),
            "grants should be of the form permission=principal"
        );
        assert_eq!(
            parse(&["--grants", "write=id=x"]).unwrap_err(),
            "permission must be one of: read|readacl|writeacl|full"
        );
    }

    #[test]
    fn sse_customer_key() {
        // The key is not base64 encoded regardless of the binary format.
        let key = b"0123456789abcdef0123456789abcdef";
        let encoded = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=";
        for args in [
            &["--sse-c", "--sse-c-key", "0123456789abcdef0123456789abcdef"][..],
            &[
                "--sse-c",
                "AES256",
                "--sse-c-key",
                "0123456789abcdef0123456789abcdef",
                "--cli-binary-format",
                "base64",
            ],
        ] {
            let sse_c = parse(args).unwrap().sse_c.unwrap();
            assert_eq!(sse_c.algorithm, "AES256");
            assert_eq!(sse_c.key, encoded);
            assert_eq!(sse_c.key_md5, base64::encode(Md5::digest(key)));
        }

        let path = std::env::temp_dir().join(format!("aws3-test-key-{}", std::process::id()));
        std::fs::write(&path, key).unwrap();
        for scheme in ["fileb", "file"] {
            let arg = format!("{scheme}://{}", path.display());
            let result = parse(&["--sse-c-copy-source", "--sse-c-copy-source-key", &arg]);
            assert_eq!(result.unwrap().sse_c_copy_source.unwrap().key, encoded);
        }
        std::fs::remove_file(&path).unwrap();

        assert!(parse(&["--sse-c", "--sse-c-key", "fileb:///nonexistent"]).is_err());
    }

    #[test]
    fn object_params() {
        let params = parse(&[
            "--acl",
            "bucket-owner-full-control",
            "--storage-class",
            "GLACIER_IR",
            "--sse",
            "aws:kms",
            "--sse-kms-key-id",
            "key",
            "--checksum-algorithm",
            "CRC64NVME",
            "--request-payer",
            "--expires",
            "2030-01-01T00:00:00Z",
            "--metadata-directive",
            "REPLACE",
            "--no-overwrite",
        ])
        .unwrap();
        assert_eq!(params.acl, Some(ObjectCannedAcl::BucketOwnerFullControl));
        assert_eq!(params.storage_class, Some(StorageClass::GlacierIr));
        assert_eq!(params.sse, Some(ServerSideEncryption::AwsKms));
        assert_eq!(
            params.checksum_algorithm,
            Some(ChecksumAlgorithm::Crc64Nvme)
        );
        assert_eq!(params.request_payer, Some(RequestPayer::Requester));
        assert_eq!(params.expires, Some(DateTime::from_secs(1893456000)));
        assert_eq!(params.metadata_directive, Some(MetadataDirective::Replace));
        assert!(params.no_overwrite);
        assert!(params.has_metadata_props());
        assert!(parse(&["--expires", "invalid"]).is_err());
        assert!(parse(&["--metadata", "{invalid"]).is_err());
    }

    #[test]
    fn metadata() {
        let pair = |key: &str, value: &str| (key.to_owned(), value.to_owned());
        let metadata = parse_metadata("a=1,b='x,y',c=\"z\"").unwrap();
        assert_eq!(metadata, [pair("a", "1"), pair("b", "x,y"), pair("c", "z")]);
        let metadata = parse_metadata(r#" {"k": "v", "l": "x", "k": "w"} "#).unwrap();
        assert_eq!(metadata, [pair("k", "w"), pair("l", "x")]);
        let params = parse(&["--metadata", "a=x,b=\u{e9},c=\u{fc}"]).unwrap();
        assert_eq!(
            params.validate_ascii_metadata().unwrap_err(),
            "Parameter validation failed:\nNon ascii characters found in S3 metadata for key \
             \"b\", value: \"\u{e9}\".  \nS3 metadata can only contain ASCII characters. "
        );
        let params = parse(&["--metadata", "a=x"]).unwrap();
        assert_eq!(params.validate_ascii_metadata(), Ok(()));
        let mut errors = vec![
            (
                "fileb://a.json",
                "Error parsing parameter '--metadata': \
                 Binary file is not supported, use file:// instead",
            ),
            (
                "a",
                "Error parsing parameter '--metadata': Expected: '=', received: 'EOF' for input:\n a\n ^",
            ),
            (
                "[1]",
                "Error parsing parameter '--metadata': Invalid JSON:\n[1]",
            ),
            (
                "{a:1}",
                "Error parsing parameter '--metadata': Invalid JSON: Expecting property name \
                 enclosed in double quotes: line 1 column 2 (char 1)\nJSON received: {a:1}",
            ),
            (
                "a=b,c,d=[1,'2'],e={f=g}",
                r#"Parameter validation failed:
Invalid type for parameter a, value: ["b","c"], type: list, valid types: string
Invalid type for parameter d, value: ["1","2"], type: list, valid types: string
Invalid type for parameter e, value: {"f":"g"}, type: map, valid types: string"#,
            ),
            (
                r#"{"a": true, "b": [1.5, null], "c": {}}"#,
                r#"Parameter validation failed:
Invalid type for parameter a, value: true, type: boolean, valid types: string
Invalid type for parameter b, value: [1.5,null], type: list, valid types: string
Invalid type for parameter c, value: {}, type: map, valid types: string"#,
            ),
        ];
        // The messages of the errors of the operating system differ on Windows.
        if cfg!(unix) {
            errors.extend([
                (
                    "file:///nonexistent",
                    "Error parsing parameter '--metadata': Unable to load paramfile \
                     file:///nonexistent: No such file or directory (os error 2)",
                ),
                (
                    "a@=file:///nonexistent",
                    "Unable to load paramfile file:///nonexistent: \
                     No such file or directory (os error 2)",
                ),
            ]);
        }
        for (value, expected) in errors {
            let message = parse_metadata(value).unwrap_err().to_string();
            let message = message
                .strip_prefix("An error occurred (ParamValidation): ")
                .unwrap_or(&message);
            assert_eq!(message, expected, "{value}");
        }
    }
}
