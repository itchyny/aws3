//! The put-bucket-versioning operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{BucketVersioningStatus, MfaDelete, VersioningConfiguration};

use super::shape::{Shape, Structure, parse_structure};
use super::{members, required_checksum_algorithm};
use crate::cli::PutBucketVersioningArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

static VERSIONING_CONFIGURATION: Structure = Structure {
    members: &[
        ("MFADelete", Shape::String { min: 0 }),
        ("Status", Shape::String { min: 0 }),
    ],
    required: &[],
};

pub async fn run(client: &Client, args: &PutBucketVersioningArgs) -> Result<Value, Error> {
    let value = parse_structure(
        "--versioning-configuration",
        "VersioningConfiguration",
        &args.versioning_configuration,
        &VERSIONING_CONFIGURATION,
    )?;
    let configuration = VersioningConfiguration::builder()
        .set_mfa_delete(
            members::str(&value, "MFADelete")
                .as_deref()
                .map(MfaDelete::from),
        )
        .set_status(
            members::str(&value, "Status")
                .as_deref()
                .map(BucketVersioningStatus::from),
        )
        .build();
    Redirector::default()
        .send(client, &args.bucket, "PutBucketVersioning", |client| {
            client
                .put_bucket_versioning()
                .bucket(&args.bucket)
                .set_content_md5(args.content_md5.clone())
                .checksum_algorithm(required_checksum_algorithm(
                    args.checksum_algorithm.as_deref(),
                ))
                .set_mfa(args.mfa.clone())
                .versioning_configuration(configuration.clone())
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("PutBucketVersioning", err))?;
    Ok(Value::Map(Vec::new()))
}
