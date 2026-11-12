//! The list-objects-v2 operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Output;
use aws_sdk_s3::types::{EncodingType, Object, OptionalObjectAttributes, RequestPayer};

use super::output::Response;
use super::paginate::{Page, Paginator};
use super::parse_int;
use crate::cli::{GlobalArgs, ListObjectsV2Args};
use crate::error::Error;
use crate::path::url_decode;
use crate::redirect::Redirector;
use crate::value::Value;

const PAGINATOR: Paginator = Paginator {
    input_tokens: &["ContinuationToken"],
    result_keys: &["Contents", "CommonPrefixes"],
};

pub async fn run(
    client: &Client,
    args: &ListObjectsV2Args,
    global: &GlobalArgs,
) -> Result<Value, Error> {
    let max_keys = parse_int(args.max_keys.as_deref())?;
    let manual = args.continuation_token.is_some() || max_keys.is_some();
    let paginate = Paginator::enabled(global.no_paginate, manual, &args.pagination)?;
    let redirector = Redirector::default();
    if !paginate {
        let output = list(
            client,
            &redirector,
            args,
            args.continuation_token.clone(),
            max_keys,
        )
        .await?;
        return Ok(response(&output, decodes(args, &output)));
    }
    PAGINATOR
        .paginate(&args.pagination, |token| {
            let redirector = &redirector;
            async move {
                let [token] = <[_; 1]>::try_from(token).unwrap_or_default();
                let output =
                    list(client, redirector, args, token, args.pagination.page_size).await?;
                Ok(page(&output, decodes(args, &output)))
            }
        })
        .await
}

async fn list(
    client: &Client,
    redirector: &Redirector,
    args: &ListObjectsV2Args,
    continuation_token: Option<String>,
    max_keys: Option<i32>,
) -> Result<ListObjectsV2Output, Error> {
    // The keys are requested to be encoded unless the encoding is specified,
    // like `set_list_objects_encoding_type_url` of botocore.
    let encoding_type = args
        .encoding_type
        .as_deref()
        .map_or(EncodingType::Url, EncodingType::from);
    redirector
        .send(client, &args.bucket, "ListObjectsV2", |client| {
            client
                .list_objects_v2()
                .bucket(&args.bucket)
                .set_delimiter(args.delimiter.clone())
                .encoding_type(encoding_type.clone())
                .set_max_keys(max_keys)
                .set_prefix(args.prefix.clone())
                .set_continuation_token(continuation_token.clone())
                .set_fetch_owner(args.fetch_owner.value())
                .set_start_after(args.start_after.clone())
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .set_optional_object_attributes(args.optional_object_attributes.as_ref().map(
                    |attributes| {
                        attributes
                            .iter()
                            .map(|attribute| OptionalObjectAttributes::from(attribute.as_str()))
                            .collect()
                    },
                ))
                .send()
        })
        .await
        .map_err(|err| Error::sdk("ListObjectsV2", err))
}

/// Returns whether the keys are decoded, which are encoded by the encoding
/// type not specified by the user, like `decode_list_object_v2` of botocore.
fn decodes(args: &ListObjectsV2Args, output: &ListObjectsV2Output) -> bool {
    args.encoding_type.is_none() && output.encoding_type() == Some(&EncodingType::Url)
}

/// Returns the full response of a request without the pagination.
fn response(output: &ListObjectsV2Output, decode: bool) -> Value {
    let decode = |value: Option<&str>| value.map(|value| decoded(value, decode));
    let mut response = Response::default();
    response.str("RequestCharged", output.request_charged());
    response.bool("IsTruncated", output.is_truncated());
    if let Some(contents) = contents(output, decode) {
        response.value("Contents", Value::List(contents));
    }
    response.str("Name", output.name());
    response.str("Prefix", decode(output.prefix()));
    response.str("Delimiter", decode(output.delimiter()));
    response.int("MaxKeys", output.max_keys());
    if let Some(common_prefixes) = common_prefixes(output, decode) {
        response.value("CommonPrefixes", Value::List(common_prefixes));
    }
    response.str("EncodingType", output.encoding_type());
    response.int("KeyCount", output.key_count());
    response.str("ContinuationToken", output.continuation_token());
    response.str("NextContinuationToken", output.next_continuation_token());
    response.str("StartAfter", decode(output.start_after()));
    response.build()
}

/// Returns the page of the response, whose prefix and the request charged
/// are recorded from the first page.
fn page(output: &ListObjectsV2Output, decode: bool) -> Page {
    let decode = |value: Option<&str>| value.map(|value| decoded(value, decode));
    let next_token = if output.is_truncated() == Some(true) {
        let token = output
            .next_continuation_token()
            .filter(|token| !token.is_empty());
        vec![token.map(str::to_owned)]
    } else {
        Vec::new()
    };
    let optional = |value: Option<String>| value.map_or(Value::Null, Value::Str);
    Page {
        results: vec![contents(output, decode), common_prefixes(output, decode)],
        non_aggregate: vec![
            (
                "RequestCharged",
                optional(
                    output
                        .request_charged()
                        .map(|charged| charged.as_str().to_owned()),
                ),
            ),
            ("Prefix", optional(decode(output.prefix()))),
        ],
        next_token,
    }
}

fn decoded(value: &str, decode: bool) -> String {
    if decode {
        url_decode(value)
    } else {
        value.to_owned()
    }
}

fn contents(
    output: &ListObjectsV2Output,
    decode: impl Fn(Option<&str>) -> Option<String>,
) -> Option<Vec<Value>> {
    let contents = output.contents();
    (!contents.is_empty()).then(|| {
        contents
            .iter()
            .map(|object| object_value(object, &decode))
            .collect()
    })
}

fn common_prefixes(
    output: &ListObjectsV2Output,
    decode: impl Fn(Option<&str>) -> Option<String>,
) -> Option<Vec<Value>> {
    let common_prefixes = output.common_prefixes();
    (!common_prefixes.is_empty()).then(|| {
        common_prefixes
            .iter()
            .map(|prefix| {
                let mut response = Response::default();
                response.str("Prefix", decode(prefix.prefix()));
                response.build()
            })
            .collect()
    })
}

fn object_value(object: &Object, decode: impl Fn(Option<&str>) -> Option<String>) -> Value {
    let mut response = Response::default();
    response.str("Key", decode(object.key()));
    response.timestamp("LastModified", object.last_modified());
    response.str("ETag", object.e_tag());
    if !object.checksum_algorithm().is_empty() {
        let algorithms = object
            .checksum_algorithm()
            .iter()
            .map(|algorithm| Value::Str(algorithm.as_str().to_owned()));
        response.value("ChecksumAlgorithm", Value::List(algorithms.collect()));
    }
    response.str("ChecksumType", object.checksum_type());
    response.int("Size", object.size());
    response.str("StorageClass", object.storage_class());
    if let Some(owner) = object.owner() {
        let mut value = Response::default();
        value.str("DisplayName", owner.display_name());
        value.str("ID", owner.id());
        response.value("Owner", value.build());
    }
    if let Some(status) = object.restore_status() {
        let mut value = Response::default();
        value.bool("IsRestoreInProgress", status.is_restore_in_progress());
        value.timestamp("RestoreExpiryDate", status.restore_expiry_date());
        response.value("RestoreStatus", value.build());
    }
    response.build()
}
