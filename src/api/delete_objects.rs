//! The delete-objects operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{Delete, ObjectIdentifier, RequestPayer};

use super::output::Response;
use super::shape::{Shape, Structure, parse_structure};
use super::{members, required_checksum_algorithm};
use crate::cli::DeleteObjectsArgs;
use crate::error::Error;
use crate::redirect::Redirector;
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
                .checksum_algorithm(required_checksum_algorithm(
                    args.checksum_algorithm.as_deref(),
                ))
                .send()
        })
        .await
        .map_err(|err| Error::sdk("DeleteObjects", err))?;
    // The members of the headers precede those of the body, like botocore.
    let mut response = Response::default();
    response.str("RequestCharged", output.request_charged());
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
    let objects = members::list(value, "Objects")
        .iter()
        .map(|object| {
            ObjectIdentifier::builder()
                .set_key(members::str(object, "Key"))
                .set_version_id(members::str(object, "VersionId"))
                .set_e_tag(members::str(object, "ETag"))
                .set_last_modified_time(members::timestamp(object, "LastModifiedTime"))
                .set_size(members::long(object, "Size"))
                .build()
                .map_err(|err| Error::new(err.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Delete::builder()
        .set_objects(Some(objects))
        .set_quiet(members::bool(value, "Quiet"))
        .build()
        .map_err(|err| Error::new(err.to_string()))
}
