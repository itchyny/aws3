//! The list-multipart-uploads operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::operation::list_multipart_uploads::ListMultipartUploadsOutput;
use aws_sdk_s3::types::{EncodingType, MultipartUpload, RequestPayer};

use super::list_objects_v2::{common_prefixes, owner_value};
use super::output::Response;
use super::paginate::{Page, Paginator, Token};
use super::{Responses, parse_int};
use crate::cli::{GlobalArgs, ListMultipartUploadsArgs};
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

const PAGINATOR: Paginator = Paginator {
    input_tokens: &["KeyMarker", "UploadIdMarker"],
    result_keys: &["Uploads", "CommonPrefixes"],
};

pub async fn run(
    client: &Client,
    args: &ListMultipartUploadsArgs,
    global: &GlobalArgs,
) -> Result<Responses, Error> {
    let max_uploads = parse_int(args.max_uploads.as_deref())?;
    let manual =
        args.key_marker.is_some() || args.upload_id_marker.is_some() || max_uploads.is_some();
    let paginate = Paginator::enabled(global.no_paginate, manual, &args.pagination)?;
    let redirector = Redirector::default();
    if !paginate {
        let token = vec![args.key_marker.clone(), args.upload_id_marker.clone()];
        let output = list(client, &redirector, args, token, max_uploads).await?;
        return Ok(Responses::Value(response(&output)));
    }
    let pages = PAGINATOR
        .paginate(&args.pagination, |token| {
            let redirector = &redirector;
            async move {
                let output =
                    list(client, redirector, args, token, args.pagination.page_size).await?;
                Ok(page(&output))
            }
        })
        .await?;
    Ok(Responses::Pages(pages))
}

async fn list(
    client: &Client,
    redirector: &Redirector,
    args: &ListMultipartUploadsArgs,
    token: Token,
    max_uploads: Option<i32>,
) -> Result<ListMultipartUploadsOutput, Error> {
    let [key_marker, upload_id_marker] = <[_; 2]>::try_from(token).unwrap_or_default();
    redirector
        .send(client, &args.bucket, "ListMultipartUploads", |client| {
            client
                .list_multipart_uploads()
                .bucket(&args.bucket)
                .set_delimiter(args.delimiter.clone())
                .set_encoding_type(args.encoding_type.as_deref().map(EncodingType::from))
                .set_key_marker(key_marker.clone())
                .set_max_uploads(max_uploads)
                .set_prefix(args.prefix.clone())
                .set_upload_id_marker(upload_id_marker.clone())
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
                .send()
        })
        .await
        .map_err(|err| Error::sdk("ListMultipartUploads", err))
}

/// Returns the full response of a request without the pagination, whose keys
/// are not decoded unlike the listings of the objects.
fn response(output: &ListMultipartUploadsOutput) -> Value {
    let mut response = Response::default();
    response.str("RequestCharged", output.request_charged());
    response.str("Bucket", output.bucket());
    response.str("KeyMarker", output.key_marker());
    response.str("UploadIdMarker", output.upload_id_marker());
    response.str("NextKeyMarker", output.next_key_marker());
    response.str("Prefix", output.prefix());
    response.str("Delimiter", output.delimiter());
    response.str("NextUploadIdMarker", output.next_upload_id_marker());
    response.int("MaxUploads", output.max_uploads());
    response.bool("IsTruncated", output.is_truncated());
    if let Some(uploads) = uploads(output) {
        response.value("Uploads", Value::List(uploads));
    }
    if let Some(common_prefixes) = common_prefixes(output.common_prefixes(), str_value) {
        response.value("CommonPrefixes", Value::List(common_prefixes));
    }
    response.str("EncodingType", output.encoding_type());
    response.build()
}

/// Returns the page of the response, whose prefix and the request charged
/// are recorded from the first page.
fn page(output: &ListMultipartUploadsOutput) -> Page {
    let next_token = if output.is_truncated() == Some(true) {
        [output.next_key_marker(), output.next_upload_id_marker()]
            .into_iter()
            .map(|marker| {
                marker
                    .filter(|marker| !marker.is_empty())
                    .map(str::to_owned)
            })
            .collect()
    } else {
        Vec::new()
    };
    let optional =
        |value: Option<&str>| value.map_or(Value::Null, |value| Value::Str(value.to_owned()));
    Page {
        response: response(output),
        results: vec![
            uploads(output),
            common_prefixes(output.common_prefixes(), str_value),
        ],
        non_aggregate: vec![
            (
                "RequestCharged",
                optional(output.request_charged().map(|charged| charged.as_str())),
            ),
            ("Prefix", optional(output.prefix())),
        ],
        next_token,
    }
}

fn str_value(value: Option<&str>) -> Option<String> {
    value.map(str::to_owned)
}

fn uploads(output: &ListMultipartUploadsOutput) -> Option<Vec<Value>> {
    let uploads = output.uploads();
    (!uploads.is_empty()).then(|| uploads.iter().map(upload_value).collect())
}

fn upload_value(upload: &MultipartUpload) -> Value {
    let mut response = Response::default();
    response.str("UploadId", upload.upload_id());
    response.str("Key", upload.key());
    response.timestamp("Initiated", upload.initiated());
    response.str("StorageClass", upload.storage_class());
    if let Some(owner) = upload.owner() {
        response.value("Owner", owner_value(owner));
    }
    if let Some(initiator) = upload.initiator() {
        let mut initiator_response = Response::default();
        initiator_response.str("ID", initiator.id());
        initiator_response.str("DisplayName", initiator.display_name());
        response.value("Initiator", initiator_response.build());
    }
    response.str("ChecksumAlgorithm", upload.checksum_algorithm());
    response.str("ChecksumType", upload.checksum_type());
    response.build()
}
