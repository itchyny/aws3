//! The put-bucket-encryption operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{
    BlockedEncryptionTypes, EncryptionType, ServerSideEncryption, ServerSideEncryptionByDefault,
    ServerSideEncryptionConfiguration, ServerSideEncryptionRule,
};

use super::shape::{Shape, Structure, parse_structure};
use super::{members, required_checksum_algorithm};
use crate::cli::PutBucketEncryptionArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

static SERVER_SIDE_ENCRYPTION_BY_DEFAULT: Structure = Structure {
    members: &[
        ("SSEAlgorithm", Shape::String { min: 0 }),
        ("KMSMasterKeyID", Shape::String { min: 0 }),
    ],
    required: &["SSEAlgorithm"],
};

static BLOCKED_ENCRYPTION_TYPES: Structure = Structure {
    members: &[("EncryptionType", Shape::List(&Shape::String { min: 0 }))],
    required: &[],
};

static SERVER_SIDE_ENCRYPTION_RULE: Structure = Structure {
    members: &[
        (
            "ApplyServerSideEncryptionByDefault",
            Shape::Structure(&SERVER_SIDE_ENCRYPTION_BY_DEFAULT),
        ),
        ("BucketKeyEnabled", Shape::Boolean),
        (
            "BlockedEncryptionTypes",
            Shape::Structure(&BLOCKED_ENCRYPTION_TYPES),
        ),
    ],
    required: &[],
};

static SERVER_SIDE_ENCRYPTION_CONFIGURATION: Structure = Structure {
    members: &[(
        "Rules",
        Shape::List(&Shape::Structure(&SERVER_SIDE_ENCRYPTION_RULE)),
    )],
    required: &["Rules"],
};

pub async fn run(client: &Client, args: &PutBucketEncryptionArgs) -> Result<Value, Error> {
    let value = parse_structure(
        "--server-side-encryption-configuration",
        "ServerSideEncryptionConfiguration",
        &args.server_side_encryption_configuration,
        &SERVER_SIDE_ENCRYPTION_CONFIGURATION,
    )?;
    let configuration = configuration(&value)?;
    Redirector::default()
        .send(client, &args.bucket, "PutBucketEncryption", |client| {
            client
                .put_bucket_encryption()
                .bucket(&args.bucket)
                .set_content_md5(args.content_md5.clone())
                .checksum_algorithm(required_checksum_algorithm(
                    args.checksum_algorithm.as_deref(),
                ))
                .server_side_encryption_configuration(configuration.clone())
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("PutBucketEncryption", err))?;
    Ok(Value::Map(Vec::new()))
}

/// Builds the configuration from the validated argument.
fn configuration(value: &Value) -> Result<ServerSideEncryptionConfiguration, Error> {
    let rules = members::list(value, "Rules")
        .iter()
        .map(|rule| {
            let default = rule
                .get("ApplyServerSideEncryptionByDefault")
                .map(|default| {
                    ServerSideEncryptionByDefault::builder()
                        .set_sse_algorithm(
                            members::str(default, "SSEAlgorithm")
                                .as_deref()
                                .map(ServerSideEncryption::from),
                        )
                        .set_kms_master_key_id(members::str(default, "KMSMasterKeyID"))
                        .build()
                        .map_err(|err| Error::new(err.to_string()))
                })
                .transpose()?;
            let blocked = rule.get("BlockedEncryptionTypes").map(|blocked| {
                BlockedEncryptionTypes::builder()
                    .set_encryption_type(blocked.get("EncryptionType").map(|_| {
                        members::list(blocked, "EncryptionType")
                            .iter()
                            .filter_map(|value| match value {
                                Value::Str(value) => Some(EncryptionType::from(value.as_str())),
                                _ => None,
                            })
                            .collect()
                    }))
                    .build()
            });
            Ok(ServerSideEncryptionRule::builder()
                .set_apply_server_side_encryption_by_default(default)
                .set_bucket_key_enabled(members::bool(rule, "BucketKeyEnabled"))
                .set_blocked_encryption_types(blocked)
                .build())
        })
        .collect::<Result<Vec<_>, Error>>()?;
    ServerSideEncryptionConfiguration::builder()
        .set_rules(Some(rules))
        .build()
        .map_err(|err| Error::new(err.to_string()))
}
