//! The put-bucket-tagging operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{Tag, Tagging};

use super::shape::{Shape, Structure, parse_structure};
use super::{members, required_checksum_algorithm};
use crate::cli::PutBucketTaggingArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

static TAG: Structure = Structure {
    members: &[
        ("Key", Shape::String { min: 1 }),
        ("Value", Shape::String { min: 0 }),
    ],
    required: &["Key", "Value"],
};

pub(super) static TAGGING: Structure = Structure {
    members: &[("TagSet", Shape::List(&Shape::Structure(&TAG)))],
    required: &["TagSet"],
};

pub async fn run(client: &Client, args: &PutBucketTaggingArgs) -> Result<Value, Error> {
    let value = parse_structure("--tagging", "Tagging", &args.tagging, &TAGGING)?;
    let tagging = tagging(&value)?;
    Redirector::default()
        .send(client, &args.bucket, "PutBucketTagging", |client| {
            client
                .put_bucket_tagging()
                .bucket(&args.bucket)
                .tagging(tagging.clone())
                .set_content_md5(args.content_md5.clone())
                .checksum_algorithm(required_checksum_algorithm(
                    args.checksum_algorithm.as_deref(),
                ))
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("PutBucketTagging", err))?;
    Ok(Value::Map(Vec::new()))
}

/// Builds the tags from the validated argument.
pub(super) fn tagging(value: &Value) -> Result<Tagging, Error> {
    let tags = members::list(value, "TagSet")
        .iter()
        .map(|tag| {
            Tag::builder()
                .set_key(members::str(tag, "Key"))
                .set_value(members::str(tag, "Value"))
                .build()
                .map_err(|err| Error::new(err.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Tagging::builder()
        .set_tag_set(Some(tags))
        .build()
        .map_err(|err| Error::new(err.to_string()))
}
