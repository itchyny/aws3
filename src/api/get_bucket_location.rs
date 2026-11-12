//! The get-bucket-location operation.

use aws_sdk_s3::Client;

use super::output::Response;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(client, &args.bucket, "GetBucketLocation", |client| {
            // The path style is always used to avoid the errors of signing
            // for the region of the bucket, like botocore.
            client
                .get_bucket_location()
                .bucket(&args.bucket)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .customize()
                .config_override(aws_sdk_s3::Config::builder().force_path_style(true))
                .send()
        })
        .await
        .map_err(|err| Error::sdk("GetBucketLocation", err))?;
    // The location constraint is null for the buckets in us-east-1, which is
    // the empty text of the element, like `parse_get_bucket_location` of
    // botocore.
    let mut response = Response::default();
    response.value(
        "LocationConstraint",
        output
            .location_constraint()
            .map(|location| location.as_str())
            .filter(|location| !location.is_empty())
            .map_or(Value::Null, |location| Value::Str(location.to_owned())),
    );
    Ok(response.build())
}
