//! The tests of the api operations of ACL.

use super::{get_bucket_configuration, last_request_body, response_output};
use crate::common::Env;

#[test]
fn get_bucket_acl() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-acl", "GetBucketAcl");
    assert_eq!(query, "acl");

    let stdout = response_output(
        &env,
        &["get-bucket-acl", "--bucket", "bucket"],
        200,
        &[],
        r#"<AccessControlPolicy>
    <Owner>
        <ID>owner-id</ID>
        <DisplayName>owner</DisplayName>
    </Owner>
    <AccessControlList>
        <Grant>
            <Grantee xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="CanonicalUser">
                <ID>owner-id</ID>
                <DisplayName>owner</DisplayName>
            </Grantee>
            <Permission>FULL_CONTROL</Permission>
        </Grant>
        <Grant>
            <Grantee xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Group">
                <URI>http://acs.amazonaws.com/groups/global/AllUsers</URI>
            </Grantee>
            <Permission>READ</Permission>
        </Grant>
    </AccessControlList>
</AccessControlPolicy>"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "Owner": {
        "DisplayName": "owner",
        "ID": "owner-id"
    },
    "Grants": [
        {
            "Grantee": {
                "DisplayName": "owner",
                "ID": "owner-id",
                "Type": "CanonicalUser"
            },
            "Permission": "FULL_CONTROL"
        },
        {
            "Grantee": {
                "Type": "Group",
                "URI": "http://acs.amazonaws.com/groups/global/AllUsers"
            },
            "Permission": "READ"
        }
    ]
}
"#
    );
}

#[test]
fn put_bucket_acl() {
    let env = Env::new();
    env.bucket("bucket");
    env.run(&[
        "api",
        "put-bucket-acl",
        "--bucket",
        "bucket",
        "--acl",
        "private",
        "--grant-read",
        "uri=http://acs.amazonaws.com/groups/s3/LogDelivery",
    ]);
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.headers["x-amz-acl"], "private");
    assert_eq!(
        request.headers["x-amz-grant-read"],
        "uri=http://acs.amazonaws.com/groups/s3/LogDelivery"
    );

    env.run(&[
        "api",
        "put-bucket-acl",
        "--bucket",
        "bucket",
        "--access-control-policy",
        "Owner={ID=o},Grants=[{Grantee={Type=CanonicalUser,ID=o},Permission=FULL_CONTROL}]",
    ]);
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<AccessControlPolicy xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <AccessControlList><Grant><Grantee \
             xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"CanonicalUser\">\
             <ID>o</ID></Grantee><Permission>FULL_CONTROL</Permission></Grant></AccessControlList>\
             <Owner><ID>o</ID></Owner></AccessControlPolicy>"
        ),
        "{body}"
    );
    assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));

    let result = env.run(&[
        "api",
        "put-bucket-acl",
        "--bucket",
        "bucket",
        "--access-control-policy",
        "Grants=[{Grantee={ID=o},Permission=READ}]",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
         Missing required parameter in AccessControlPolicy.Grants[0].Grantee: \"Type\"\n"
    );

    let stdout = response_output(
        &env,
        &["put-bucket-acl", "--bucket", "bucket", "--acl", "private"],
        200,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}
