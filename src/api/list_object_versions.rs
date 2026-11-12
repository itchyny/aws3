//! The list-object-versions operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::operation::list_object_versions::ListObjectVersionsOutput;
use aws_sdk_s3::types::{
    DeleteMarkerEntry, EncodingType, ObjectVersion, OptionalObjectAttributes, RequestPayer,
};

use super::list_objects_v2::{
    checksum_algorithms, common_prefixes, decoded, owner_value, restore_status_value,
};
use super::output::Response;
use super::paginate::{Page, Paginator, Token};
use super::{Responses, parse_int};
use crate::cli::{GlobalArgs, ListObjectVersionsArgs};
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

const PAGINATOR: Paginator = Paginator {
    input_tokens: &["KeyMarker", "VersionIdMarker"],
    result_keys: &["Versions", "DeleteMarkers", "CommonPrefixes"],
};

pub async fn run(
    client: &Client,
    args: &ListObjectVersionsArgs,
    global: &GlobalArgs,
) -> Result<Responses, Error> {
    let max_keys = parse_int(args.max_keys.as_deref())?;
    let manual =
        args.key_marker.is_some() || args.version_id_marker.is_some() || max_keys.is_some();
    let paginate = Paginator::enabled(global.no_paginate, manual, &args.pagination)?;
    let redirector = Redirector::default();
    if !paginate {
        let token = vec![args.key_marker.clone(), args.version_id_marker.clone()];
        let output = list(client, &redirector, args, token, max_keys).await?;
        return Ok(Responses::Value(response(&output, decodes(args, &output))));
    }
    let pages = PAGINATOR
        .paginate(&args.pagination, |token| {
            let redirector = &redirector;
            async move {
                let output =
                    list(client, redirector, args, token, args.pagination.page_size).await?;
                Ok(page(&output, decodes(args, &output)))
            }
        })
        .await?;
    Ok(Responses::Pages(pages))
}

async fn list(
    client: &Client,
    redirector: &Redirector,
    args: &ListObjectVersionsArgs,
    token: Token,
    max_keys: Option<i32>,
) -> Result<ListObjectVersionsOutput, Error> {
    let [key_marker, version_id_marker] = <[_; 2]>::try_from(token).unwrap_or_default();
    // The keys are requested to be encoded unless the encoding is specified,
    // like `set_list_objects_encoding_type_url` of botocore.
    let encoding_type = args
        .encoding_type
        .as_deref()
        .map_or(EncodingType::Url, EncodingType::from);
    redirector
        .send(client, &args.bucket, "ListObjectVersions", |client| {
            client
                .list_object_versions()
                .bucket(&args.bucket)
                .set_delimiter(args.delimiter.clone())
                .encoding_type(encoding_type.clone())
                .set_key_marker(key_marker.clone())
                .set_max_keys(max_keys)
                .set_prefix(args.prefix.clone())
                .set_version_id_marker(version_id_marker.clone())
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
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
        .map_err(|err| Error::sdk("ListObjectVersions", err))
}

/// Returns whether the keys are decoded, which are encoded by the encoding
/// type not specified by the user, like `decode_list_object_versions` of
/// botocore.
fn decodes(args: &ListObjectVersionsArgs, output: &ListObjectVersionsOutput) -> bool {
    args.encoding_type.is_none() && output.encoding_type() == Some(&EncodingType::Url)
}

/// Returns the full response of a request without the pagination.
fn response(output: &ListObjectVersionsOutput, decode: bool) -> Value {
    let decode = |value: Option<&str>| value.map(|value| decoded(value, decode));
    let mut response = Response::default();
    response.str("RequestCharged", output.request_charged());
    response.bool("IsTruncated", output.is_truncated());
    response.str("KeyMarker", decode(output.key_marker()));
    response.str("VersionIdMarker", output.version_id_marker());
    response.str("NextKeyMarker", decode(output.next_key_marker()));
    response.str("NextVersionIdMarker", output.next_version_id_marker());
    if let Some(versions) = versions(output, decode) {
        response.value("Versions", Value::List(versions));
    }
    if let Some(delete_markers) = delete_markers(output, decode) {
        response.value("DeleteMarkers", Value::List(delete_markers));
    }
    response.str("Name", output.name());
    response.str("Prefix", decode(output.prefix()));
    response.str("Delimiter", decode(output.delimiter()));
    response.int("MaxKeys", output.max_keys());
    if let Some(common_prefixes) = common_prefixes(output.common_prefixes(), decode) {
        response.value("CommonPrefixes", Value::List(common_prefixes));
    }
    response.str("EncodingType", output.encoding_type());
    response.build()
}

/// Returns the page of the response, whose prefix and the request charged
/// are recorded from the first page.
fn page(output: &ListObjectVersionsOutput, decode: bool) -> Page {
    let decode = |value: Option<&str>| value.map(|value| decoded(value, decode));
    let next_token = if output.is_truncated() == Some(true) {
        vec![
            decode(output.next_key_marker()).filter(|marker| !marker.is_empty()),
            output
                .next_version_id_marker()
                .filter(|marker| !marker.is_empty())
                .map(str::to_owned),
        ]
    } else {
        Vec::new()
    };
    let optional = |value: Option<String>| value.map_or(Value::Null, Value::Str);
    Page {
        results: vec![
            versions(output, decode),
            delete_markers(output, decode),
            common_prefixes(output.common_prefixes(), decode),
        ],
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

fn versions(
    output: &ListObjectVersionsOutput,
    decode: impl Fn(Option<&str>) -> Option<String>,
) -> Option<Vec<Value>> {
    let versions = output.versions();
    (!versions.is_empty()).then(|| {
        versions
            .iter()
            .map(|version| version_value(version, &decode))
            .collect()
    })
}

fn version_value(
    version: &ObjectVersion,
    decode: impl Fn(Option<&str>) -> Option<String>,
) -> Value {
    let mut response = Response::default();
    response.str("ETag", version.e_tag());
    checksum_algorithms(&mut response, version.checksum_algorithm());
    response.str("ChecksumType", version.checksum_type());
    response.int("Size", version.size());
    response.str("StorageClass", version.storage_class());
    response.str("Key", decode(version.key()));
    response.str("VersionId", version.version_id());
    response.bool("IsLatest", version.is_latest());
    response.timestamp("LastModified", version.last_modified());
    if let Some(owner) = version.owner() {
        response.value("Owner", owner_value(owner));
    }
    if let Some(status) = version.restore_status() {
        response.value("RestoreStatus", restore_status_value(status));
    }
    response.build()
}

fn delete_markers(
    output: &ListObjectVersionsOutput,
    decode: impl Fn(Option<&str>) -> Option<String>,
) -> Option<Vec<Value>> {
    let delete_markers = output.delete_markers();
    (!delete_markers.is_empty()).then(|| {
        delete_markers
            .iter()
            .map(|marker| delete_marker_value(marker, &decode))
            .collect()
    })
}

fn delete_marker_value(
    marker: &DeleteMarkerEntry,
    decode: impl Fn(Option<&str>) -> Option<String>,
) -> Value {
    let mut response = Response::default();
    if let Some(owner) = marker.owner() {
        response.value("Owner", owner_value(owner));
    }
    response.str("Key", decode(marker.key()));
    response.str("VersionId", marker.version_id());
    response.bool("IsLatest", marker.is_latest());
    response.timestamp("LastModified", marker.last_modified());
    response.build()
}
