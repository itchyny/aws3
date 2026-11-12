//! The delete-objects operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::DateTime;
use aws_sdk_s3::types::{ChecksumAlgorithm, Delete, ObjectIdentifier, RequestPayer};

use super::output::Response;
use super::shape::{Shape, Structure, parse_structure};
use crate::cli::DeleteObjectsArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::transfer::params::parse_timestamp;
use crate::value::Value;

static OBJECT_IDENTIFIER: Structure = Structure {
    members: &[
        ("Key", Shape::String { min: 1 }),
        ("VersionId", Shape::String { min: 0 }),
        ("ETag", Shape::String { min: 0 }),
        ("LastModifiedTime", Shape::Timestamp),
        ("Size", Shape::Long),
    ],
    required: &["Key"],
};

static DELETE: Structure = Structure {
    members: &[
        (
            "Objects",
            Shape::List(&Shape::Structure(&OBJECT_IDENTIFIER)),
        ),
        ("Quiet", Shape::Boolean),
    ],
    required: &["Objects"],
};

pub async fn run(client: &Client, args: &DeleteObjectsArgs) -> Result<Value, Error> {
    let delete = delete(&parse_structure(
        "--delete",
        "Delete",
        &args.delete,
        &DELETE,
    )?)?;
    let output = Redirector::default()
        .send(client, &args.bucket, "DeleteObjects", |client| {
            client
                .delete_objects()
                .bucket(&args.bucket)
                .delete(delete.clone())
                .set_mfa(args.mfa.clone())
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
                .set_bypass_governance_retention(args.bypass_governance_retention.value())
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .set_checksum_algorithm(
                    args.checksum_algorithm
                        .as_deref()
                        .map(ChecksumAlgorithm::from),
                )
                .send()
        })
        .await
        .map_err(|err| Error::sdk("DeleteObjects", err))?;
    let mut response = Response::default();
    if !output.deleted().is_empty() {
        let deleted = output.deleted().iter().map(|deleted| {
            let mut response = Response::default();
            response.str("Key", deleted.key());
            response.str("VersionId", deleted.version_id());
            response.bool("DeleteMarker", deleted.delete_marker());
            response.str("DeleteMarkerVersionId", deleted.delete_marker_version_id());
            response.build()
        });
        response.value("Deleted", Value::List(deleted.collect()));
    }
    response.str("RequestCharged", output.request_charged());
    if !output.errors().is_empty() {
        let errors = output.errors().iter().map(|error| {
            let mut response = Response::default();
            response.str("Key", error.key());
            response.str("VersionId", error.version_id());
            response.str("Code", error.code());
            response.str("Message", error.message());
            response.build()
        });
        response.value("Errors", Value::List(errors.collect()));
    }
    Ok(response.build())
}

/// Builds the request from the validated argument.
fn delete(value: &Value) -> Result<Delete, Error> {
    let str = |value: &Value, key| match value.get(key) {
        Some(Value::Str(value)) => Some(value.clone()),
        _ => None,
    };
    let objects = match value.get("Objects") {
        Some(Value::List(objects)) => objects.as_slice(),
        _ => &[],
    };
    let objects = objects
        .iter()
        .map(|object| {
            ObjectIdentifier::builder()
                .set_key(str(object, "Key"))
                .set_version_id(str(object, "VersionId"))
                .set_e_tag(str(object, "ETag"))
                .set_last_modified_time(object.get("LastModifiedTime").and_then(timestamp))
                .set_size(match object.get("Size") {
                    Some(Value::Int(size)) => size.parse().ok(),
                    _ => None,
                })
                .build()
                .map_err(|err| Error::new(err.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Delete::builder()
        .set_objects(Some(objects))
        .set_quiet(match value.get("Quiet") {
            Some(Value::Bool(quiet)) => Some(*quiet),
            _ => None,
        })
        .build()
        .map_err(|err| Error::new(err.to_string()))
}

/// Returns the timestamp of the string or the epoch seconds.
fn timestamp(value: &Value) -> Option<DateTime> {
    match value {
        Value::Str(value) => parse_timestamp(value).ok(),
        Value::Int(value) => value.parse().ok().map(DateTime::from_secs),
        Value::Float(value) => Some(DateTime::from_secs_f64(*value)),
        _ => None,
    }
}
