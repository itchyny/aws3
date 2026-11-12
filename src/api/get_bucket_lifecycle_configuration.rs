//! The get-bucket-lifecycle-configuration operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{LifecycleRule, LifecycleRuleFilter, Tag};

use super::output::Response;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(
            client,
            &args.bucket,
            "GetBucketLifecycleConfiguration",
            |client| {
                client
                    .get_bucket_lifecycle_configuration()
                    .bucket(&args.bucket)
                    .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                    .send()
            },
        )
        .await
        .map_err(|err| Error::sdk("GetBucketLifecycleConfiguration", err))?;
    // The members of the headers precede those of the body, like botocore.
    let mut response = Response::default();
    response.str(
        "TransitionDefaultMinimumObjectSize",
        output.transition_default_minimum_object_size(),
    );
    let rules = output.rules();
    if !rules.is_empty() {
        response.value("Rules", Value::List(rules.iter().map(rule).collect()));
    }
    Ok(response.build())
}

fn rule(rule: &LifecycleRule) -> Value {
    let mut response = Response::default();
    if let Some(expiration) = rule.expiration() {
        let mut value = Response::default();
        value.timestamp("Date", expiration.date());
        value.int("Days", expiration.days());
        value.bool(
            "ExpiredObjectDeleteMarker",
            expiration.expired_object_delete_marker(),
        );
        response.value("Expiration", value.build());
    }
    response.str("ID", rule.id());
    #[allow(deprecated)]
    response.str("Prefix", rule.prefix());
    if let Some(filter) = rule.filter() {
        response.value("Filter", filter_value(filter));
    }
    response.str("Status", Some(rule.status()));
    if !rule.transitions().is_empty() {
        let transitions = rule.transitions().iter().map(|transition| {
            let mut value = Response::default();
            value.timestamp("Date", transition.date());
            value.int("Days", transition.days());
            value.str("StorageClass", transition.storage_class());
            value.build()
        });
        response.value("Transitions", Value::List(transitions.collect()));
    }
    if !rule.noncurrent_version_transitions().is_empty() {
        let transitions = rule
            .noncurrent_version_transitions()
            .iter()
            .map(|transition| {
                let mut value = Response::default();
                value.int("NoncurrentDays", transition.noncurrent_days());
                value.str("StorageClass", transition.storage_class());
                value.int(
                    "NewerNoncurrentVersions",
                    transition.newer_noncurrent_versions(),
                );
                value.build()
            });
        response.value(
            "NoncurrentVersionTransitions",
            Value::List(transitions.collect()),
        );
    }
    if let Some(expiration) = rule.noncurrent_version_expiration() {
        let mut value = Response::default();
        value.int("NoncurrentDays", expiration.noncurrent_days());
        value.int(
            "NewerNoncurrentVersions",
            expiration.newer_noncurrent_versions(),
        );
        response.value("NoncurrentVersionExpiration", value.build());
    }
    if let Some(abort) = rule.abort_incomplete_multipart_upload() {
        let mut value = Response::default();
        value.int("DaysAfterInitiation", abort.days_after_initiation());
        response.value("AbortIncompleteMultipartUpload", value.build());
    }
    response.build()
}

fn filter_value(filter: &LifecycleRuleFilter) -> Value {
    let mut response = Response::default();
    response.str("Prefix", filter.prefix());
    if let Some(tag) = filter.tag() {
        response.value("Tag", tag_value(tag));
    }
    response.int("ObjectSizeGreaterThan", filter.object_size_greater_than());
    response.int("ObjectSizeLessThan", filter.object_size_less_than());
    if let Some(and) = filter.and() {
        let mut value = Response::default();
        value.str("Prefix", and.prefix());
        if !and.tags().is_empty() {
            value.value(
                "Tags",
                Value::List(and.tags().iter().map(tag_value).collect()),
            );
        }
        value.int("ObjectSizeGreaterThan", and.object_size_greater_than());
        value.int("ObjectSizeLessThan", and.object_size_less_than());
        response.value("And", value.build());
    }
    response.build()
}

fn tag_value(tag: &Tag) -> Value {
    let mut response = Response::default();
    response.str("Key", Some(tag.key()));
    response.str("Value", Some(tag.value()));
    response.build()
}
