//! The get-bucket-policy operation.

use aws_sdk_s3::Client;

use super::response::Response;
use super::send;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = send(client, &args.bucket, "GetBucketPolicy", |client| {
        client
            .get_bucket_policy()
            .bucket(&args.bucket)
            .set_expected_bucket_owner(args.expected_bucket_owner.clone())
            .send()
    })
    .await?;
    let mut response = Response::default();
    response.str("Policy", output.policy());
    Ok(response.build())
}
