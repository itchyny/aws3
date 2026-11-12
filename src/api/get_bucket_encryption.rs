//! The get-bucket-encryption operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::ServerSideEncryptionRule;

use super::output::Response;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(client, &args.bucket, "GetBucketEncryption", |client| {
            client
                .get_bucket_encryption()
                .bucket(&args.bucket)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("GetBucketEncryption", err))?;
    let mut response = Response::default();
    if let Some(configuration) = output.server_side_encryption_configuration() {
        let mut value = Response::default();
        let rules = configuration.rules();
        if !rules.is_empty() {
            value.value("Rules", Value::List(rules.iter().map(rule).collect()));
        }
        response.value("ServerSideEncryptionConfiguration", value.build());
    }
    Ok(response.build())
}

fn rule(rule: &ServerSideEncryptionRule) -> Value {
    let mut response = Response::default();
    if let Some(default) = rule.apply_server_side_encryption_by_default() {
        let mut value = Response::default();
        value.str("SSEAlgorithm", Some(default.sse_algorithm()));
        value.str("KMSMasterKeyID", default.kms_master_key_id());
        response.value("ApplyServerSideEncryptionByDefault", value.build());
    }
    response.bool("BucketKeyEnabled", rule.bucket_key_enabled());
    if let Some(blocked) = rule.blocked_encryption_types() {
        let mut value = Response::default();
        let types = blocked.encryption_type();
        if !types.is_empty() {
            let types = types
                .iter()
                .map(|encryption_type| Value::Str(encryption_type.as_str().to_owned()));
            value.value("EncryptionType", Value::List(types.collect()));
        }
        response.value("BlockedEncryptionTypes", value.build());
    }
    response.build()
}
