//! The create-bucket operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{
    BucketCannedAcl, BucketInfo, BucketLocationConstraint, BucketNamespace, BucketType,
    CreateBucketConfiguration, DataRedundancy, LocationInfo, LocationType, ObjectOwnership, Tag,
};

use super::output::Response;
use super::shape::{Shape, Structure, parse_structure};
use crate::cli::CreateBucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

static LOCATION_INFO: Structure = Structure {
    members: &[
        ("Type", Shape::String { min: 0 }),
        ("Name", Shape::String { min: 0 }),
    ],
    required: &[],
};

static BUCKET_INFO: Structure = Structure {
    members: &[
        ("DataRedundancy", Shape::String { min: 0 }),
        ("Type", Shape::String { min: 0 }),
    ],
    required: &[],
};

static TAG: Structure = Structure {
    members: &[
        ("Key", Shape::String { min: 1 }),
        ("Value", Shape::String { min: 0 }),
    ],
    required: &["Key", "Value"],
};

static CREATE_BUCKET_CONFIGURATION: Structure = Structure {
    members: &[
        ("LocationConstraint", Shape::String { min: 0 }),
        ("Location", Shape::Structure(&LOCATION_INFO)),
        ("Bucket", Shape::Structure(&BUCKET_INFO)),
        ("Tags", Shape::List(&Shape::Structure(&TAG))),
    ],
    required: &[],
};

pub async fn run(client: &Client, args: &CreateBucketArgs) -> Result<Value, Error> {
    let configuration = args
        .create_bucket_configuration
        .as_deref()
        .map(|value| {
            parse_structure(
                "--create-bucket-configuration",
                "CreateBucketConfiguration",
                value,
                &CREATE_BUCKET_CONFIGURATION,
            )
            .and_then(|value| configuration(&value))
        })
        .transpose()?;
    // The request is redirected to the region of the bucket except on the
    // errors of the location constraints, like botocore.
    let output = Redirector::default()
        .send(client, &args.bucket, "CreateBucket", |client| {
            client
                .create_bucket()
                .set_acl(args.acl.as_deref().map(BucketCannedAcl::from))
                .bucket(&args.bucket)
                .set_create_bucket_configuration(configuration.clone())
                .set_grant_full_control(args.grant_full_control.clone())
                .set_grant_read(args.grant_read.clone())
                .set_grant_read_acp(args.grant_read_acp.clone())
                .set_grant_write(args.grant_write.clone())
                .set_grant_write_acp(args.grant_write_acp.clone())
                .set_object_lock_enabled_for_bucket(args.object_lock_enabled_for_bucket.value())
                .set_object_ownership(args.object_ownership.as_deref().map(ObjectOwnership::from))
                .set_bucket_namespace(args.bucket_namespace.as_deref().map(BucketNamespace::from))
                .send()
        })
        .await
        .map_err(|err| Error::sdk("CreateBucket", err))?;
    let mut response = Response::default();
    response.str("Location", output.location());
    response.str("BucketArn", output.bucket_arn());
    Ok(response.build())
}

/// Builds the configuration from the validated argument.
fn configuration(value: &Value) -> Result<CreateBucketConfiguration, Error> {
    let str = |value: Option<&Value>, key| match value.and_then(|value| value.get(key)) {
        Some(Value::Str(value)) => Some(value.clone()),
        _ => None,
    };
    let tags = match value.get("Tags") {
        Some(Value::List(tags)) => Some(
            tags.iter()
                .map(|tag| {
                    Tag::builder()
                        .set_key(str(Some(tag), "Key"))
                        .set_value(str(Some(tag), "Value"))
                        .build()
                        .map_err(|err| Error::new(err.to_string()))
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        _ => None,
    };
    let location = value.get("Location").map(|location| {
        LocationInfo::builder()
            .set_type(
                str(Some(location), "Type")
                    .as_deref()
                    .map(LocationType::from),
            )
            .set_name(str(Some(location), "Name"))
            .build()
    });
    let bucket = value.get("Bucket").map(|bucket| {
        BucketInfo::builder()
            .set_data_redundancy(
                str(Some(bucket), "DataRedundancy")
                    .as_deref()
                    .map(DataRedundancy::from),
            )
            .set_type(str(Some(bucket), "Type").as_deref().map(BucketType::from))
            .build()
    });
    Ok(CreateBucketConfiguration::builder()
        .set_location_constraint(
            str(Some(value), "LocationConstraint")
                .as_deref()
                .map(BucketLocationConstraint::from),
        )
        .set_location(location)
        .set_bucket(bucket)
        .set_tags(tags)
        .build())
}
