//! The get-bucket-versioning operation.

use aws_sdk_s3::Client;

use super::output::Response;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(client, &args.bucket, "GetBucketVersioning", |client| {
            client
                .get_bucket_versioning()
                .bucket(&args.bucket)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("GetBucketVersioning", err))?;
    let mut response = Response::default();
    response.str("Status", output.status());
    response.str("MFADelete", output.mfa_delete());
    Ok(response.build())
}
