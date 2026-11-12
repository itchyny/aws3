//! The get-object-tagging operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::RequestPayer;

use super::get_bucket_tagging::tag_set;
use super::output::Response;
use crate::cli::GetObjectTaggingArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &GetObjectTaggingArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(client, &args.bucket, "GetObjectTagging", |client| {
            client
                .get_object_tagging()
                .bucket(&args.bucket)
                .key(&args.key)
                .set_version_id(args.version_id.clone())
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
                .send()
        })
        .await
        .map_err(|err| Error::sdk("GetObjectTagging", err))?;
    let mut response = Response::default();
    response.str("VersionId", output.version_id());
    response.value("TagSet", tag_set(output.tag_set()));
    Ok(response.build())
}
