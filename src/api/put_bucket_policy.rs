//! The put-bucket-policy operation.

use aws_sdk_s3::Client;

use super::required_checksum_algorithm;
use super::shape;
use crate::cli::PutBucketPolicyArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::transfer::params::load_string;
use crate::value::Value;

pub async fn run(client: &Client, args: &PutBucketPolicyArgs) -> Result<Value, Error> {
    let policy = match load_string("--policy", &args.policy)? {
        Value::Str(policy) => policy,
        value => {
            return Err(Error::param_validation(format!(
                "Parameter validation failed:\nInvalid type for parameter Policy, value: {}, \
                 type: <class '{}'>, valid types: <class 'str'>",
                shape::str(&value),
                shape::type_name(&value)
            )));
        }
    };
    Redirector::default()
        .send(client, &args.bucket, "PutBucketPolicy", |client| {
            client
                .put_bucket_policy()
                .bucket(&args.bucket)
                .set_content_md5(args.content_md5.clone())
                .checksum_algorithm(required_checksum_algorithm(
                    args.checksum_algorithm.as_deref(),
                ))
                .set_confirm_remove_self_bucket_access(
                    args.confirm_remove_self_bucket_access.value(),
                )
                .policy(&policy)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("PutBucketPolicy", err))?;
    Ok(Value::Map(Vec::new()))
}
