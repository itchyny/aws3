//! The tests of the api operations of versioning.

use super::{get_bucket_configuration, last_request_body, response_output};
use crate::common::Env;

#[test]
fn get_bucket_versioning() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-versioning", "GetBucketVersioning");
    assert_eq!(query, "versioning");

    let stdout = response_output(
        &env,
        &["get-bucket-versioning", "--bucket", "bucket"],
        200,
        &[],
        r#"<VersioningConfiguration>
    <Status>Enabled</Status>
    <MfaDelete>Disabled</MfaDelete>
</VersioningConfiguration>"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "Status": "Enabled",
    "MFADelete": "Disabled"
}
"#
    );
}

#[test]
fn put_bucket_versioning() {
    let env = Env::new();
    env.bucket("bucket");
    // The fake server does not implement the versioning.
    let result = env.run(&[
        "api",
        "put-bucket-versioning",
        "--bucket",
        "bucket",
        "--versioning-configuration",
        "Status=Enabled",
    ]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (NotImplemented) when calling the PutBucketVersioning \
         operation: PutBucketVersioning is not implemented yet\n"
    );
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<VersioningConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <Status>Enabled</Status></VersioningConfiguration>"
        ),
        "{body}"
    );
    // The checksum is required, which defaults to CRC64NVME.
    assert_eq!(
        headers,
        [
            "x-amz-checksum-crc64nvme: KwYdKXAPj44=",
            "x-amz-sdk-checksum-algorithm: CRC64NVME"
        ]
    );

    let result = env.run(&[
        "api",
        "put-bucket-versioning",
        "--bucket",
        "bucket",
        "--versioning-configuration",
        "Foo=x",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
         Unknown parameter in VersioningConfiguration: \"Foo\", must be one of: MFADelete, Status\n"
    );

    let stdout = response_output(
        &env,
        &[
            "put-bucket-versioning",
            "--bucket",
            "bucket",
            "--versioning-configuration",
            "Status=Enabled",
        ],
        200,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}
