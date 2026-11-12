//! The get-bucket-cors operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::CorsRule;

use super::output::Response;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(client, &args.bucket, "GetBucketCors", |client| {
            client
                .get_bucket_cors()
                .bucket(&args.bucket)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("GetBucketCors", err))?;
    let mut response = Response::default();
    let rules = output.cors_rules();
    if !rules.is_empty() {
        response.value("CORSRules", Value::List(rules.iter().map(rule).collect()));
    }
    Ok(response.build())
}

fn rule(rule: &CorsRule) -> Value {
    let strings = |values: &[String]| {
        (!values.is_empty()).then(|| Value::List(values.iter().cloned().map(Value::Str).collect()))
    };
    let mut response = Response::default();
    response.str("ID", rule.id());
    for (key, values) in [
        ("AllowedHeaders", rule.allowed_headers()),
        ("AllowedMethods", rule.allowed_methods()),
        ("AllowedOrigins", rule.allowed_origins()),
        ("ExposeHeaders", rule.expose_headers()),
    ] {
        if let Some(values) = strings(values) {
            response.value(key, values);
        }
    }
    response.int("MaxAgeSeconds", rule.max_age_seconds());
    response.build()
}
