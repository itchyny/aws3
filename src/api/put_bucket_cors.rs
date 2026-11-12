//! The put-bucket-cors operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{CorsConfiguration, CorsRule};

use super::shape::{Shape, Structure, parse_structure};
use super::{members, required_checksum_algorithm};
use crate::cli::PutBucketCorsArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

static CORS_RULE: Structure = Structure {
    members: &[
        ("ID", Shape::String { min: 0 }),
        ("AllowedHeaders", Shape::List(&Shape::String { min: 0 })),
        ("AllowedMethods", Shape::List(&Shape::String { min: 0 })),
        ("AllowedOrigins", Shape::List(&Shape::String { min: 0 })),
        ("ExposeHeaders", Shape::List(&Shape::String { min: 0 })),
        ("MaxAgeSeconds", Shape::Long),
    ],
    required: &["AllowedMethods", "AllowedOrigins"],
};

static CORS_CONFIGURATION: Structure = Structure {
    members: &[("CORSRules", Shape::List(&Shape::Structure(&CORS_RULE)))],
    required: &["CORSRules"],
};

pub async fn run(client: &Client, args: &PutBucketCorsArgs) -> Result<Value, Error> {
    let value = parse_structure(
        "--cors-configuration",
        "CORSConfiguration",
        &args.cors_configuration,
        &CORS_CONFIGURATION,
    )?;
    let configuration = configuration(&value)?;
    Redirector::default()
        .send(client, &args.bucket, "PutBucketCors", |client| {
            client
                .put_bucket_cors()
                .bucket(&args.bucket)
                .cors_configuration(configuration.clone())
                .set_content_md5(args.content_md5.clone())
                .checksum_algorithm(required_checksum_algorithm(
                    args.checksum_algorithm.as_deref(),
                ))
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("PutBucketCors", err))?;
    Ok(Value::Map(Vec::new()))
}

/// Builds the configuration from the validated argument.
fn configuration(value: &Value) -> Result<CorsConfiguration, Error> {
    let strings = |value: &Value, key: &str| {
        value.get(key).map(|_| {
            members::list(value, key)
                .iter()
                .filter_map(|value| match value {
                    Value::Str(value) => Some(value.clone()),
                    _ => None,
                })
                .collect()
        })
    };
    let rules = members::list(value, "CORSRules")
        .iter()
        .map(|rule| {
            CorsRule::builder()
                .set_id(members::str(rule, "ID"))
                .set_allowed_headers(strings(rule, "AllowedHeaders"))
                .set_allowed_methods(strings(rule, "AllowedMethods"))
                .set_allowed_origins(strings(rule, "AllowedOrigins"))
                .set_expose_headers(strings(rule, "ExposeHeaders"))
                .set_max_age_seconds(members::int(rule, "MaxAgeSeconds"))
                .build()
                .map_err(|err| Error::new(err.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    CorsConfiguration::builder()
        .set_cors_rules(Some(rules))
        .build()
        .map_err(|err| Error::new(err.to_string()))
}
