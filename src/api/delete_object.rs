//! The delete-object operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::RequestPayer;

use super::output::Response;
use super::parse_timestamp;
use crate::cli::DeleteObjectArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &DeleteObjectArgs) -> Result<Value, Error> {
    let if_match_last_modified_time = parse_timestamp(
        "IfMatchLastModifiedTime",
        args.if_match_last_modified_time.as_deref(),
    )?;
    let output = Redirector::default()
        .send(client, &args.bucket, "DeleteObject", |client| {
            client
                .delete_object()
                .bucket(&args.bucket)
                .key(&args.key)
                .set_mfa(args.mfa.clone())
                .set_version_id(args.version_id.clone())
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
                .set_bypass_governance_retention(args.bypass_governance_retention.value())
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .set_if_match(args.if_match.clone())
                .set_if_match_last_modified_time(if_match_last_modified_time)
                .set_if_match_size(args.if_match_size)
                .send()
        })
        .await
        .map_err(|err| Error::sdk("DeleteObject", err))?;
    let mut response = Response::default();
    response.bool("DeleteMarker", output.delete_marker());
    response.str("VersionId", output.version_id());
    response.str("RequestCharged", output.request_charged());
    Ok(response.build())
}
