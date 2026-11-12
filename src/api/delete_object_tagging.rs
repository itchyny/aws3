//! The delete-object-tagging operation.

use aws_sdk_s3::Client;

use super::output::Response;
use crate::cli::DeleteObjectTaggingArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &DeleteObjectTaggingArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(client, &args.bucket, "DeleteObjectTagging", |client| {
            client
                .delete_object_tagging()
                .bucket(&args.bucket)
                .key(&args.key)
                .set_version_id(args.version_id.clone())
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("DeleteObjectTagging", err))?;
    let mut response = Response::default();
    response.str("VersionId", output.version_id());
    Ok(response.build())
}
