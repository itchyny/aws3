//! The get-bucket-tagging operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::Tag;

use super::response::Response;
use super::send;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = send(client, &args.bucket, "GetBucketTagging", |client| {
        client
            .get_bucket_tagging()
            .bucket(&args.bucket)
            .set_expected_bucket_owner(args.expected_bucket_owner.clone())
            .send()
    })
    .await?;
    let mut response = Response::default();
    response.value("TagSet", tag_set(output.tag_set()));
    Ok(response.build())
}

/// Returns the tags, which are always responded even if empty.
pub(super) fn tag_set(tags: &[Tag]) -> Value {
    Value::List(
        tags.iter()
            .map(|tag| {
                let mut response = Response::default();
                response.str("Key", Some(tag.key()));
                response.str("Value", Some(tag.value()));
                response.build()
            })
            .collect(),
    )
}
