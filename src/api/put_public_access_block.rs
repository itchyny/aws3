//! The put-public-access-block operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::PublicAccessBlockConfiguration;

use super::shape::{Shape, Structure, parse_structure};
use super::{members, required_checksum_algorithm};
use crate::cli::PutPublicAccessBlockArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

static PUBLIC_ACCESS_BLOCK_CONFIGURATION: Structure = Structure {
    members: &[
        ("BlockPublicAcls", Shape::Boolean),
        ("IgnorePublicAcls", Shape::Boolean),
        ("BlockPublicPolicy", Shape::Boolean),
        ("RestrictPublicBuckets", Shape::Boolean),
    ],
    required: &[],
};

pub async fn run(client: &Client, args: &PutPublicAccessBlockArgs) -> Result<Value, Error> {
    let value = parse_structure(
        "--public-access-block-configuration",
        "PublicAccessBlockConfiguration",
        &args.public_access_block_configuration,
        &PUBLIC_ACCESS_BLOCK_CONFIGURATION,
    )?;
    let configuration = PublicAccessBlockConfiguration::builder()
        .set_block_public_acls(members::bool(&value, "BlockPublicAcls"))
        .set_ignore_public_acls(members::bool(&value, "IgnorePublicAcls"))
        .set_block_public_policy(members::bool(&value, "BlockPublicPolicy"))
        .set_restrict_public_buckets(members::bool(&value, "RestrictPublicBuckets"))
        .build();
    Redirector::default()
        .send(client, &args.bucket, "PutPublicAccessBlock", |client| {
            client
                .put_public_access_block()
                .bucket(&args.bucket)
                .set_content_md5(args.content_md5.clone())
                .checksum_algorithm(required_checksum_algorithm(
                    args.checksum_algorithm.as_deref(),
                ))
                .public_access_block_configuration(configuration.clone())
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("PutPublicAccessBlock", err))?;
    Ok(Value::Map(Vec::new()))
}
