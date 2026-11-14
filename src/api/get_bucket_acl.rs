//! The get-bucket-acl operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::Grantee;

use super::output::Response;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(client, &args.bucket, "GetBucketAcl", |client| {
            client
                .get_bucket_acl()
                .bucket(&args.bucket)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("GetBucketAcl", err))?;
    let mut response = Response::default();
    if let Some(owner) = output.owner() {
        let mut owner_response = Response::default();
        owner_response.str("DisplayName", owner.display_name());
        owner_response.str("ID", owner.id());
        response.value("Owner", owner_response.build());
    }
    let grants = output.grants();
    if !grants.is_empty() {
        response.value(
            "Grants",
            Value::List(
                grants
                    .iter()
                    .map(|grant| {
                        let mut response = Response::default();
                        if let Some(grantee) = grant.grantee() {
                            response.value("Grantee", grantee_value(grantee));
                        }
                        response.str(
                            "Permission",
                            grant.permission().map(|permission| permission.as_str()),
                        );
                        response.build()
                    })
                    .collect(),
            ),
        );
    }
    Ok(response.build())
}

/// Returns the value of the grantee of the grants.
pub(super) fn grantee_value(grantee: &Grantee) -> Value {
    let mut response = Response::default();
    response.str("DisplayName", grantee.display_name());
    response.str("EmailAddress", grantee.email_address());
    response.str("ID", grantee.id());
    response.str("Type", Some(grantee.r#type().as_str()));
    response.str("URI", grantee.uri());
    response.build()
}
