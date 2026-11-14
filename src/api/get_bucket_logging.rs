//! The get-bucket-logging operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{LoggingEnabled, TargetGrant, TargetObjectKeyFormat};

use super::output::Response;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(client, &args.bucket, "GetBucketLogging", |client| {
            client
                .get_bucket_logging()
                .bucket(&args.bucket)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("GetBucketLogging", err))?;
    let mut response = Response::default();
    if let Some(logging) = output.logging_enabled() {
        response.value("LoggingEnabled", logging_value(logging));
    }
    Ok(response.build())
}

fn logging_value(logging: &LoggingEnabled) -> Value {
    let mut response = Response::default();
    response.str("TargetBucket", Some(logging.target_bucket()));
    let grants = logging.target_grants();
    if !grants.is_empty() {
        response.value(
            "TargetGrants",
            Value::List(grants.iter().map(grant_value).collect()),
        );
    }
    response.str("TargetPrefix", Some(logging.target_prefix()));
    if let Some(format) = logging.target_object_key_format() {
        response.value("TargetObjectKeyFormat", key_format_value(format));
    }
    response.build()
}

fn grant_value(grant: &TargetGrant) -> Value {
    let mut response = Response::default();
    if let Some(grantee) = grant.grantee() {
        let mut grantee_response = Response::default();
        grantee_response.str("DisplayName", grantee.display_name());
        grantee_response.str("EmailAddress", grantee.email_address());
        grantee_response.str("ID", grantee.id());
        grantee_response.str("Type", Some(grantee.r#type().as_str()));
        grantee_response.str("URI", grantee.uri());
        response.value("Grantee", grantee_response.build());
    }
    response.str(
        "Permission",
        grant.permission().map(|permission| permission.as_str()),
    );
    response.build()
}

fn key_format_value(format: &TargetObjectKeyFormat) -> Value {
    let mut response = Response::default();
    if format.simple_prefix().is_some() {
        response.value("SimplePrefix", Value::Map(Vec::new()));
    }
    if let Some(prefix) = format.partitioned_prefix() {
        let mut prefix_response = Response::default();
        prefix_response.str(
            "PartitionDateSource",
            prefix.partition_date_source().map(|source| source.as_str()),
        );
        response.value("PartitionedPrefix", prefix_response.build());
    }
    response.build()
}
