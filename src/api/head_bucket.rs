//! The head-bucket operation.

use aws_sdk_s3::Client;

use super::output::Response;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(client, &args.bucket, "HeadBucket", |client| {
            client
                .head_bucket()
                .bucket(&args.bucket)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("HeadBucket", err))?;
    let mut response = Response::default();
    response.str("BucketArn", output.bucket_arn());
    response.str("BucketLocationType", output.bucket_location_type());
    response.str("BucketLocationName", output.bucket_location_name());
    response.str("BucketRegion", output.bucket_region());
    response.bool("AccessPointAlias", output.access_point_alias());
    Ok(response.build())
}
