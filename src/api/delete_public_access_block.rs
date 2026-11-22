//! The delete-public-access-block operation.

use aws_sdk_s3::Client;

use super::send;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    send(client, &args.bucket, "DeletePublicAccessBlock", |client| {
        client
            .delete_public_access_block()
            .bucket(&args.bucket)
            .set_expected_bucket_owner(args.expected_bucket_owner.clone())
            .send()
    })
    .await?;
    Ok(Value::Map(Vec::new()))
}
