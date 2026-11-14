//! The put-bucket-acl operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{
    AccessControlPolicy, BucketCannedAcl, Grant, Grantee, Owner, Permission, Type,
};

use super::shape::{Shape, Structure, parse_structure};
use super::{members, required_checksum_algorithm};
use crate::cli::PutBucketAclArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub(super) static GRANTEE: Structure = Structure {
    members: &[
        ("DisplayName", Shape::String { min: 0 }),
        ("EmailAddress", Shape::String { min: 0 }),
        ("ID", Shape::String { min: 0 }),
        ("Type", Shape::String { min: 0 }),
        ("URI", Shape::String { min: 0 }),
    ],
    required: &["Type"],
};

static GRANT: Structure = Structure {
    members: &[
        ("Grantee", Shape::Structure(&GRANTEE)),
        ("Permission", Shape::String { min: 0 }),
    ],
    required: &[],
};

static OWNER: Structure = Structure {
    members: &[
        ("DisplayName", Shape::String { min: 0 }),
        ("ID", Shape::String { min: 0 }),
    ],
    required: &[],
};

static ACCESS_CONTROL_POLICY: Structure = Structure {
    members: &[
        ("Grants", Shape::List(&Shape::Structure(&GRANT))),
        ("Owner", Shape::Structure(&OWNER)),
    ],
    required: &[],
};

pub async fn run(client: &Client, args: &PutBucketAclArgs) -> Result<Value, Error> {
    let policy = args
        .access_control_policy
        .as_deref()
        .map(|value| {
            parse_structure(
                "--access-control-policy",
                "AccessControlPolicy",
                value,
                &ACCESS_CONTROL_POLICY,
            )
            .and_then(|value| policy(&value))
        })
        .transpose()?;
    Redirector::default()
        .send(client, &args.bucket, "PutBucketAcl", |client| {
            client
                .put_bucket_acl()
                .set_acl(args.acl.as_deref().map(BucketCannedAcl::from))
                .set_access_control_policy(policy.clone())
                .bucket(&args.bucket)
                .set_content_md5(args.content_md5.clone())
                .checksum_algorithm(required_checksum_algorithm(
                    args.checksum_algorithm.as_deref(),
                ))
                .set_grant_full_control(args.grant_full_control.clone())
                .set_grant_read(args.grant_read.clone())
                .set_grant_read_acp(args.grant_read_acp.clone())
                .set_grant_write(args.grant_write.clone())
                .set_grant_write_acp(args.grant_write_acp.clone())
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("PutBucketAcl", err))?;
    Ok(Value::Map(Vec::new()))
}

/// Builds the access control policy from the validated argument.
fn policy(value: &Value) -> Result<AccessControlPolicy, Error> {
    let grants = value
        .get("Grants")
        .map(|_| {
            members::list(value, "Grants")
                .iter()
                .map(|grant| {
                    Ok(Grant::builder()
                        .set_grantee(grant.get("Grantee").map(grantee).transpose()?)
                        .set_permission(
                            members::str(grant, "Permission")
                                .map(|value| Permission::from(value.as_str())),
                        )
                        .build())
                })
                .collect::<Result<Vec<_>, Error>>()
        })
        .transpose()?;
    let owner = value.get("Owner").map(|owner| {
        Owner::builder()
            .set_display_name(members::str(owner, "DisplayName"))
            .set_id(members::str(owner, "ID"))
            .build()
    });
    Ok(AccessControlPolicy::builder()
        .set_grants(grants)
        .set_owner(owner)
        .build())
}

/// Builds the grantee from the validated value of the shape of `GRANTEE`.
pub(super) fn grantee(value: &Value) -> Result<Grantee, Error> {
    Grantee::builder()
        .set_display_name(members::str(value, "DisplayName"))
        .set_email_address(members::str(value, "EmailAddress"))
        .set_id(members::str(value, "ID"))
        .set_type(members::str(value, "Type").map(|value| Type::from(value.as_str())))
        .set_uri(members::str(value, "URI"))
        .build()
        .map_err(|err| Error::new(err.to_string()))
}
