//! The tests of the api operations of tagging.

use super::{
    delete_bucket_configuration, get_bucket_configuration, last_request_body, response_output,
    unimplemented_object_operation,
};
use crate::common::Env;

#[test]
fn get_bucket_tagging() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-tagging", "GetBucketTagging");
    assert_eq!(query, "tagging");

    let stdout = response_output(
        &env,
        &["get-bucket-tagging", "--bucket", "bucket"],
        200,
        &[],
        r#"<Tagging>
    <TagSet>
        <Tag>
            <Key>project</Key>
            <Value>aws3</Value>
        </Tag>
        <Tag>
            <Key>env</Key>
            <Value>test</Value>
        </Tag>
    </TagSet>
</Tagging>"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "TagSet": [
        {
            "Key": "project",
            "Value": "aws3"
        },
        {
            "Key": "env",
            "Value": "test"
        }
    ]
}
"#
    );
}

#[test]
fn put_bucket_tagging() {
    let env = Env::new();
    env.bucket("bucket");
    env.run(&[
        "api",
        "put-bucket-tagging",
        "--bucket",
        "bucket",
        "--tagging",
        r#"{"TagSet": [{"Key": "env", "Value": "dev"}, {"Key": "team", "Value": ""}]}"#,
    ]);
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<Tagging xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><TagSet>\
             <Tag><Key>env</Key><Value>dev</Value></Tag>\
             <Tag><Key>team</Key><Value></Value></Tag></TagSet></Tagging>"
        ),
        "{body}"
    );
    assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));

    for (tagging, message) in [
        (
            "TagSet=[{Key=env}]",
            "Missing required parameter in Tagging.TagSet[0]: \"Value\"",
        ),
        (
            "TagSet=[{Key=,Value=dev}]",
            "Invalid length for parameter Tagging.TagSet[0].Key, value: 0, valid min length: 1",
        ),
    ] {
        let result = env.run(&[
            "api",
            "put-bucket-tagging",
            "--bucket",
            "bucket",
            "--tagging",
            tagging,
        ]);
        assert_eq!(result.code, 252);
        assert_eq!(
            result.stderr,
            format!(
                "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
                 {message}\n"
            ),
            "{tagging}"
        );
    }

    let stdout = response_output(
        &env,
        &[
            "put-bucket-tagging",
            "--bucket",
            "bucket",
            "--tagging",
            "TagSet=[{Key=k,Value=v}]",
        ],
        204,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}

#[test]
fn delete_bucket_tagging() {
    let env = Env::new();
    env.bucket("bucket");
    let query = delete_bucket_configuration(&env, "delete-bucket-tagging", "DeleteBucketTagging");
    assert_eq!(query, "tagging");

    let stdout = response_output(
        &env,
        &["delete-bucket-tagging", "--bucket", "bucket"],
        204,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}

#[test]
fn get_object_tagging() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "dir/a b.txt", "hello");
    let (method, path, query) = unimplemented_object_operation(
        &env,
        &[
            "get-object-tagging",
            "--bucket",
            "bucket",
            "--key",
            "dir/a b.txt",
            "--version-id",
            "v1",
        ],
        "GetObjectTagging",
    );
    assert_eq!(method, http::Method::GET);
    assert_eq!(path, "/bucket/dir/a%20b.txt");
    assert_eq!(query, "tagging&versionId=v1");

    let stdout = response_output(
        &env,
        &["get-object-tagging", "--bucket", "bucket", "--key", "a.txt"],
        200,
        &[("x-amz-version-id", "version-1")],
        r#"<Tagging>
    <TagSet>
        <Tag>
            <Key>project</Key>
            <Value>aws3</Value>
        </Tag>
    </TagSet>
</Tagging>"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "VersionId": "version-1",
    "TagSet": [
        {
            "Key": "project",
            "Value": "aws3"
        }
    ]
}
"#
    );
}

#[test]
fn put_object_tagging() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    let (method, path, query) = unimplemented_object_operation(
        &env,
        &[
            "put-object-tagging",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "--tagging",
            "TagSet=[{Key=env,Value=dev}]",
        ],
        "PutObjectTagging",
    );
    assert_eq!(method, http::Method::PUT);
    assert_eq!(path, "/bucket/a.txt");
    assert_eq!(query, "tagging");
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<Tagging xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><TagSet>\
             <Tag><Key>env</Key><Value>dev</Value></Tag></TagSet></Tagging>"
        ),
        "{body}"
    );
    assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));

    let stdout = response_output(
        &env,
        &[
            "put-object-tagging",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "--tagging",
            "TagSet=[{Key=k,Value=v}]",
        ],
        200,
        &[("x-amz-version-id", "version-1")],
        "",
    );
    assert_eq!(
        stdout,
        r#"{
    "VersionId": "version-1"
}
"#
    );
}

#[test]
fn delete_object_tagging() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    let (method, path, query) = unimplemented_object_operation(
        &env,
        &[
            "delete-object-tagging",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "--version-id",
            "v1",
        ],
        "DeleteObjectTagging",
    );
    assert_eq!(method, http::Method::DELETE);
    assert_eq!(path, "/bucket/a.txt");
    assert_eq!(query, "tagging&versionId=v1");
    // The object is not deleted.
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"hello");

    let stdout = response_output(
        &env,
        &[
            "delete-object-tagging",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
        ],
        204,
        &[("x-amz-version-id", "version-1")],
        "",
    );
    assert_eq!(
        stdout,
        r#"{
    "VersionId": "version-1"
}
"#
    );
}
