//! The delete-bucket operation.

use aws_sdk_s3::Client;

use crate::cli::BucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    Redirector::default()
        .send(client, &args.bucket, "DeleteBucket", |client| {
            client
                .delete_bucket()
                .bucket(&args.bucket)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("DeleteBucket", err))?;
    Ok(Value::Map(Vec::new()))
}
