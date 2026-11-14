//! The abort-multipart-upload operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::RequestPayer;

use super::output::Response;
use super::parse_timestamp;
use crate::cli::AbortMultipartUploadArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &AbortMultipartUploadArgs) -> Result<Value, Error> {
    let if_match_initiated_time = parse_timestamp(
        "IfMatchInitiatedTime",
        args.if_match_initiated_time.as_deref(),
    )?;
    let output = Redirector::default()
        .send(client, &args.bucket, "AbortMultipartUpload", |client| {
            client
                .abort_multipart_upload()
                .bucket(&args.bucket)
                .key(&args.key)
                .upload_id(&args.upload_id)
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .set_if_match_initiated_time(if_match_initiated_time)
                .send()
        })
        .await
        .map_err(|err| Error::sdk("AbortMultipartUpload", err))?;
    let mut response = Response::default();
    response.str("RequestCharged", output.request_charged());
    Ok(response.build())
}
