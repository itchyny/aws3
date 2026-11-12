//! The get-public-access-block operation.

use aws_sdk_s3::Client;

use super::output::Response;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(client, &args.bucket, "GetPublicAccessBlock", |client| {
            client
                .get_public_access_block()
                .bucket(&args.bucket)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("GetPublicAccessBlock", err))?;
    let mut response = Response::default();
    if let Some(configuration) = output.public_access_block_configuration() {
        let mut value = Response::default();
        value.bool("BlockPublicAcls", configuration.block_public_acls());
        value.bool("IgnorePublicAcls", configuration.ignore_public_acls());
        value.bool("BlockPublicPolicy", configuration.block_public_policy());
        value.bool(
            "RestrictPublicBuckets",
            configuration.restrict_public_buckets(),
        );
        response.value("PublicAccessBlockConfiguration", value.build());
    }
    Ok(response.build())
}
