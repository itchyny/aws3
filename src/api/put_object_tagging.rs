//! The put-object-tagging operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::RequestPayer;

use super::output::Response;
use super::put_bucket_tagging::{TAGGING, tagging};
use super::required_checksum_algorithm;
use super::shape::parse_structure;
use crate::cli::PutObjectTaggingArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &PutObjectTaggingArgs) -> Result<Value, Error> {
    let value = parse_structure("--tagging", "Tagging", &args.tagging, &TAGGING)?;
    let tagging = tagging(&value)?;
    let output = Redirector::default()
        .send(client, &args.bucket, "PutObjectTagging", |client| {
            client
                .put_object_tagging()
                .bucket(&args.bucket)
                .key(&args.key)
                .set_version_id(args.version_id.clone())
                .set_content_md5(args.content_md5.clone())
                .checksum_algorithm(required_checksum_algorithm(
                    args.checksum_algorithm.as_deref(),
                ))
                .tagging(tagging.clone())
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
                .send()
        })
        .await
        .map_err(|err| Error::sdk("PutObjectTagging", err))?;
    let mut response = Response::default();
    response.str("VersionId", output.version_id());
    Ok(response.build())
}
