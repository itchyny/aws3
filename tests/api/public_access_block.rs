//! The tests of the api operations of public access block.

use super::{
    delete_bucket_configuration, get_bucket_configuration, last_request_body, response_output,
};
use crate::common::Env;

#[test]
fn get_public_access_block() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-public-access-block", "GetPublicAccessBlock");
    assert_eq!(query, "publicAccessBlock");

    let stdout = response_output(
        &env,
        &["get-public-access-block", "--bucket", "bucket"],
        200,
        &[],
        r#"<PublicAccessBlockConfiguration>
    <BlockPublicAcls>true</BlockPublicAcls>
    <IgnorePublicAcls>false</IgnorePublicAcls>
    <BlockPublicPolicy>true</BlockPublicPolicy>
    <RestrictPublicBuckets>false</RestrictPublicBuckets>
</PublicAccessBlockConfiguration>"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "PublicAccessBlockConfiguration": {
        "BlockPublicAcls": true,
        "IgnorePublicAcls": false,
        "BlockPublicPolicy": true,
        "RestrictPublicBuckets": false
    }
}
"#
    );
}

#[test]
fn put_public_access_block() {
    let env = Env::new();
    env.bucket("bucket");
    // The fake server does not implement the public access block.
    let result = env.run(&[
        "api",
        "put-public-access-block",
        "--bucket",
        "bucket",
        "--public-access-block-configuration",
        "BlockPublicAcls=true,IgnorePublicAcls=TRUE,BlockPublicPolicy=false",
    ]);
    assert_eq!(result.code, 254);
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<PublicAccessBlockConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <BlockPublicAcls>true</BlockPublicAcls><IgnorePublicAcls>true</IgnorePublicAcls>\
             <BlockPublicPolicy>false</BlockPublicPolicy></PublicAccessBlockConfiguration>"
        ),
        "{body}"
    );
    assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));

    let result = env.run(&[
        "api",
        "put-public-access-block",
        "--bucket",
        "bucket",
        "--public-access-block-configuration",
        r#"{"BlockPublicAcls":"true"}"#,
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
         Invalid type for parameter PublicAccessBlockConfiguration.BlockPublicAcls, value: \"true\", \
         type: string, valid types: boolean\n"
    );

    let stdout = response_output(
        &env,
        &[
            "put-public-access-block",
            "--bucket",
            "bucket",
            "--public-access-block-configuration",
            "BlockPublicAcls=true",
        ],
        200,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}

#[test]
fn delete_public_access_block() {
    let env = Env::new();
    env.bucket("bucket");
    let query = delete_bucket_configuration(
        &env,
        "delete-public-access-block",
        "DeletePublicAccessBlock",
    );
    assert_eq!(query, "publicAccessBlock");

    let stdout = response_output(
        &env,
        &["delete-public-access-block", "--bucket", "bucket"],
        204,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}
