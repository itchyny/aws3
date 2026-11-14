//! The put-bucket-logging operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{
    BucketLoggingStatus, BucketLogsPermission, Grantee, LoggingEnabled, PartitionDateSource,
    PartitionedPrefix, SimplePrefix, TargetGrant, TargetObjectKeyFormat, Type,
};

use super::shape::{Shape, Structure, parse_structure};
use super::{members, required_checksum_algorithm};
use crate::cli::PutBucketLoggingArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

static GRANTEE: Structure = Structure {
    members: &[
        ("DisplayName", Shape::String { min: 0 }),
        ("EmailAddress", Shape::String { min: 0 }),
        ("ID", Shape::String { min: 0 }),
        ("Type", Shape::String { min: 0 }),
        ("URI", Shape::String { min: 0 }),
    ],
    required: &["Type"],
};

static TARGET_GRANT: Structure = Structure {
    members: &[
        ("Grantee", Shape::Structure(&GRANTEE)),
        ("Permission", Shape::String { min: 0 }),
    ],
    required: &[],
};

static SIMPLE_PREFIX: Structure = Structure {
    members: &[],
    required: &[],
};

static PARTITIONED_PREFIX: Structure = Structure {
    members: &[("PartitionDateSource", Shape::String { min: 0 })],
    required: &[],
};

static TARGET_OBJECT_KEY_FORMAT: Structure = Structure {
    members: &[
        ("SimplePrefix", Shape::Structure(&SIMPLE_PREFIX)),
        ("PartitionedPrefix", Shape::Structure(&PARTITIONED_PREFIX)),
    ],
    required: &[],
};

static LOGGING_ENABLED: Structure = Structure {
    members: &[
        ("TargetBucket", Shape::String { min: 0 }),
        (
            "TargetGrants",
            Shape::List(&Shape::Structure(&TARGET_GRANT)),
        ),
        ("TargetPrefix", Shape::String { min: 0 }),
        (
            "TargetObjectKeyFormat",
            Shape::Structure(&TARGET_OBJECT_KEY_FORMAT),
        ),
    ],
    required: &["TargetBucket", "TargetPrefix"],
};

static BUCKET_LOGGING_STATUS: Structure = Structure {
    members: &[("LoggingEnabled", Shape::Structure(&LOGGING_ENABLED))],
    required: &[],
};

pub async fn run(client: &Client, args: &PutBucketLoggingArgs) -> Result<Value, Error> {
    let value = parse_structure(
        "--bucket-logging-status",
        "BucketLoggingStatus",
        &args.bucket_logging_status,
        &BUCKET_LOGGING_STATUS,
    )?;
    let status = status(&value)?;
    Redirector::default()
        .send(client, &args.bucket, "PutBucketLogging", |client| {
            client
                .put_bucket_logging()
                .bucket(&args.bucket)
                .bucket_logging_status(status.clone())
                .set_content_md5(args.content_md5.clone())
                .checksum_algorithm(required_checksum_algorithm(
                    args.checksum_algorithm.as_deref(),
                ))
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("PutBucketLogging", err))?;
    Ok(Value::Map(Vec::new()))
}

/// Builds the logging status from the validated argument.
fn status(value: &Value) -> Result<BucketLoggingStatus, Error> {
    let error = |err: aws_sdk_s3::error::BuildError| Error::new(err.to_string());
    let logging_enabled = value
        .get("LoggingEnabled")
        .map(|logging| {
            let grants = logging
                .get("TargetGrants")
                .map(|_| {
                    members::list(logging, "TargetGrants")
                        .iter()
                        .map(|grant| {
                            let grantee = grant
                                .get("Grantee")
                                .map(|grantee| {
                                    Grantee::builder()
                                        .set_display_name(members::str(grantee, "DisplayName"))
                                        .set_email_address(members::str(grantee, "EmailAddress"))
                                        .set_id(members::str(grantee, "ID"))
                                        .set_type(
                                            members::str(grantee, "Type")
                                                .map(|value| Type::from(value.as_str())),
                                        )
                                        .set_uri(members::str(grantee, "URI"))
                                        .build()
                                        .map_err(error)
                                })
                                .transpose()?;
                            Ok(TargetGrant::builder()
                                .set_grantee(grantee)
                                .set_permission(
                                    members::str(grant, "Permission")
                                        .map(|value| BucketLogsPermission::from(value.as_str())),
                                )
                                .build())
                        })
                        .collect::<Result<Vec<_>, Error>>()
                })
                .transpose()?;
            let key_format = logging.get("TargetObjectKeyFormat").map(|format| {
                TargetObjectKeyFormat::builder()
                    .set_simple_prefix(
                        format
                            .get("SimplePrefix")
                            .map(|_| SimplePrefix::builder().build()),
                    )
                    .set_partitioned_prefix(format.get("PartitionedPrefix").map(|prefix| {
                        PartitionedPrefix::builder()
                            .set_partition_date_source(
                                members::str(prefix, "PartitionDateSource")
                                    .map(|value| PartitionDateSource::from(value.as_str())),
                            )
                            .build()
                    }))
                    .build()
            });
            LoggingEnabled::builder()
                .set_target_bucket(members::str(logging, "TargetBucket"))
                .set_target_grants(grants)
                .set_target_prefix(members::str(logging, "TargetPrefix"))
                .set_target_object_key_format(key_format)
                .build()
                .map_err(error)
        })
        .transpose()?;
    Ok(BucketLoggingStatus::builder()
        .set_logging_enabled(logging_enabled)
        .build())
}
