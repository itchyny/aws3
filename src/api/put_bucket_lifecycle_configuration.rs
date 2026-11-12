//! The put-bucket-lifecycle-configuration operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{
    AbortIncompleteMultipartUpload, BucketLifecycleConfiguration, ExpirationStatus,
    LifecycleExpiration, LifecycleRule, LifecycleRuleAndOperator, LifecycleRuleFilter,
    NoncurrentVersionExpiration, NoncurrentVersionTransition, Tag, Transition,
    TransitionDefaultMinimumObjectSize, TransitionStorageClass,
};

use super::output::Response;
use super::shape::{Shape, Structure, parse_structure};
use super::{members, required_checksum_algorithm};
use crate::cli::PutBucketLifecycleConfigurationArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

static LIFECYCLE_EXPIRATION: Structure = Structure {
    members: &[
        ("Date", Shape::Timestamp),
        ("Days", Shape::Long),
        ("ExpiredObjectDeleteMarker", Shape::Boolean),
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

static LIFECYCLE_RULE_AND_OPERATOR: Structure = Structure {
    members: &[
        ("Prefix", Shape::String { min: 0 }),
        ("Tags", Shape::List(&Shape::Structure(&TAG))),
        ("ObjectSizeGreaterThan", Shape::Long),
        ("ObjectSizeLessThan", Shape::Long),
    ],
    required: &[],
};

static LIFECYCLE_RULE_FILTER: Structure = Structure {
    members: &[
        ("Prefix", Shape::String { min: 0 }),
        ("Tag", Shape::Structure(&TAG)),
        ("ObjectSizeGreaterThan", Shape::Long),
        ("ObjectSizeLessThan", Shape::Long),
        ("And", Shape::Structure(&LIFECYCLE_RULE_AND_OPERATOR)),
    ],
    required: &[],
};

static TRANSITION: Structure = Structure {
    members: &[
        ("Date", Shape::Timestamp),
        ("Days", Shape::Long),
        ("StorageClass", Shape::String { min: 0 }),
    ],
    required: &[],
};

static NONCURRENT_VERSION_TRANSITION: Structure = Structure {
    members: &[
        ("NoncurrentDays", Shape::Long),
        ("StorageClass", Shape::String { min: 0 }),
        ("NewerNoncurrentVersions", Shape::Long),
    ],
    required: &[],
};

static NONCURRENT_VERSION_EXPIRATION: Structure = Structure {
    members: &[
        ("NoncurrentDays", Shape::Long),
        ("NewerNoncurrentVersions", Shape::Long),
    ],
    required: &[],
};

static ABORT_INCOMPLETE_MULTIPART_UPLOAD: Structure = Structure {
    members: &[("DaysAfterInitiation", Shape::Long)],
    required: &[],
};

static LIFECYCLE_RULE: Structure = Structure {
    members: &[
        ("Expiration", Shape::Structure(&LIFECYCLE_EXPIRATION)),
        ("ID", Shape::String { min: 0 }),
        ("Prefix", Shape::String { min: 0 }),
        ("Filter", Shape::Structure(&LIFECYCLE_RULE_FILTER)),
        ("Status", Shape::String { min: 0 }),
        ("Transitions", Shape::List(&Shape::Structure(&TRANSITION))),
        (
            "NoncurrentVersionTransitions",
            Shape::List(&Shape::Structure(&NONCURRENT_VERSION_TRANSITION)),
        ),
        (
            "NoncurrentVersionExpiration",
            Shape::Structure(&NONCURRENT_VERSION_EXPIRATION),
        ),
        (
            "AbortIncompleteMultipartUpload",
            Shape::Structure(&ABORT_INCOMPLETE_MULTIPART_UPLOAD),
        ),
    ],
    required: &["Status"],
};

static BUCKET_LIFECYCLE_CONFIGURATION: Structure = Structure {
    members: &[("Rules", Shape::List(&Shape::Structure(&LIFECYCLE_RULE)))],
    required: &["Rules"],
};

pub async fn run(
    client: &Client,
    args: &PutBucketLifecycleConfigurationArgs,
) -> Result<Value, Error> {
    let configuration = args
        .lifecycle_configuration
        .as_deref()
        .map(|value| {
            parse_structure(
                "--lifecycle-configuration",
                "LifecycleConfiguration",
                value,
                &BUCKET_LIFECYCLE_CONFIGURATION,
            )
            .and_then(|value| configuration(&value))
        })
        .transpose()?;
    let output = Redirector::default()
        .send(
            client,
            &args.bucket,
            "PutBucketLifecycleConfiguration",
            |client| {
                client
                    .put_bucket_lifecycle_configuration()
                    .bucket(&args.bucket)
                    .checksum_algorithm(required_checksum_algorithm(
                        args.checksum_algorithm.as_deref(),
                    ))
                    .set_lifecycle_configuration(configuration.clone())
                    .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                    .set_transition_default_minimum_object_size(
                        args.transition_default_minimum_object_size
                            .as_deref()
                            .map(TransitionDefaultMinimumObjectSize::from),
                    )
                    .send()
            },
        )
        .await
        .map_err(|err| Error::sdk("PutBucketLifecycleConfiguration", err))?;
    let mut response = Response::default();
    response.str(
        "TransitionDefaultMinimumObjectSize",
        output.transition_default_minimum_object_size(),
    );
    Ok(response.build())
}

/// Builds the configuration from the validated argument.
fn configuration(value: &Value) -> Result<BucketLifecycleConfiguration, Error> {
    let rules = members::list(value, "Rules")
        .iter()
        .map(rule)
        .collect::<Result<Vec<_>, _>>()?;
    BucketLifecycleConfiguration::builder()
        .set_rules(Some(rules))
        .build()
        .map_err(|err| Error::new(err.to_string()))
}

fn rule(value: &Value) -> Result<LifecycleRule, Error> {
    let storage_class = |value: &Value| {
        members::str(value, "StorageClass")
            .as_deref()
            .map(TransitionStorageClass::from)
    };
    let expiration = value.get("Expiration").map(|expiration| {
        LifecycleExpiration::builder()
            .set_date(members::timestamp(expiration, "Date"))
            .set_days(members::int(expiration, "Days"))
            .set_expired_object_delete_marker(members::bool(
                expiration,
                "ExpiredObjectDeleteMarker",
            ))
            .build()
    });
    let filter = value.get("Filter").map(filter).transpose()?;
    let transitions = value.get("Transitions").map(|_| {
        members::list(value, "Transitions")
            .iter()
            .map(|transition| {
                Transition::builder()
                    .set_date(members::timestamp(transition, "Date"))
                    .set_days(members::int(transition, "Days"))
                    .set_storage_class(storage_class(transition))
                    .build()
            })
            .collect()
    });
    let noncurrent_version_transitions = value.get("NoncurrentVersionTransitions").map(|_| {
        members::list(value, "NoncurrentVersionTransitions")
            .iter()
            .map(|transition| {
                NoncurrentVersionTransition::builder()
                    .set_noncurrent_days(members::int(transition, "NoncurrentDays"))
                    .set_storage_class(storage_class(transition))
                    .set_newer_noncurrent_versions(members::int(
                        transition,
                        "NewerNoncurrentVersions",
                    ))
                    .build()
            })
            .collect()
    });
    let noncurrent_version_expiration =
        value.get("NoncurrentVersionExpiration").map(|expiration| {
            NoncurrentVersionExpiration::builder()
                .set_noncurrent_days(members::int(expiration, "NoncurrentDays"))
                .set_newer_noncurrent_versions(members::int(expiration, "NewerNoncurrentVersions"))
                .build()
        });
    let abort_incomplete_multipart_upload =
        value.get("AbortIncompleteMultipartUpload").map(|abort| {
            AbortIncompleteMultipartUpload::builder()
                .set_days_after_initiation(members::int(abort, "DaysAfterInitiation"))
                .build()
        });
    #[allow(deprecated)]
    let builder = LifecycleRule::builder().set_prefix(members::str(value, "Prefix"));
    builder
        .set_expiration(expiration)
        .set_id(members::str(value, "ID"))
        .set_filter(filter)
        .set_status(
            members::str(value, "Status")
                .as_deref()
                .map(ExpirationStatus::from),
        )
        .set_transitions(transitions)
        .set_noncurrent_version_transitions(noncurrent_version_transitions)
        .set_noncurrent_version_expiration(noncurrent_version_expiration)
        .set_abort_incomplete_multipart_upload(abort_incomplete_multipart_upload)
        .build()
        .map_err(|err| Error::new(err.to_string()))
}

fn filter(value: &Value) -> Result<LifecycleRuleFilter, Error> {
    let and = value
        .get("And")
        .map(|and| -> Result<_, Error> {
            Ok(LifecycleRuleAndOperator::builder()
                .set_prefix(members::str(and, "Prefix"))
                .set_tags(
                    and.get("Tags")
                        .map(|_| members::list(and, "Tags").iter().map(tag).collect())
                        .transpose()?,
                )
                .set_object_size_greater_than(members::long(and, "ObjectSizeGreaterThan"))
                .set_object_size_less_than(members::long(and, "ObjectSizeLessThan"))
                .build())
        })
        .transpose()?;
    Ok(LifecycleRuleFilter::builder()
        .set_prefix(members::str(value, "Prefix"))
        .set_tag(value.get("Tag").map(tag).transpose()?)
        .set_object_size_greater_than(members::long(value, "ObjectSizeGreaterThan"))
        .set_object_size_less_than(members::long(value, "ObjectSizeLessThan"))
        .set_and(and)
        .build())
}

fn tag(value: &Value) -> Result<Tag, Error> {
    Tag::builder()
        .set_key(members::str(value, "Key"))
        .set_value(members::str(value, "Value"))
        .build()
        .map_err(|err| Error::new(err.to_string()))
}
