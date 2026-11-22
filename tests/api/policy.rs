//! The tests of the api operations of policy.

use super::{
    delete_bucket_configuration, get_bucket_configuration, last_request_body, response_output,
};
use crate::common::Env;

#[test]
fn get_bucket_policy() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-policy", "GetBucketPolicy");
    assert_eq!(query, "policy");

    let stdout = response_output(
        &env,
        &["get-bucket-policy", "--bucket", "bucket"],
        200,
        &[],
        r#"{"Version":"2012-10-17","Statement":[{"Effect":"Allow","Principal":"*","Action":"s3:GetObject","Resource":"arn:aws:s3:::bucket/*"}]}"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "Policy": "{\"Version\":\"2012-10-17\",\"Statement\":[{\"Effect\":\"Allow\",\"Principal\":\"*\",\"Action\":\"s3:GetObject\",\"Resource\":\"arn:aws:s3:::bucket/*\"}]}"
}
"#
    );
}

#[test]
fn put_bucket_policy() {
    let env = Env::new();
    env.bucket("bucket");
    let policy = r#"{"Version":"2012-10-17","Statement":[]}"#;
    env.write("policy.json", policy);
    for value in [policy, "file://policy.json"] {
        env.run(&[
            "api",
            "put-bucket-policy",
            "--bucket",
            "bucket",
            "--policy",
            value,
        ]);
        let (body, headers) = last_request_body(&env);
        assert_eq!(body, policy);
        assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));
    }

    let result = env.run(&[
        "api",
        "put-bucket-policy",
        "--bucket",
        "bucket",
        "--policy",
        "file://missing.json",
    ]);
    assert_eq!(result.code, 252);
    assert!(
        result.stderr.starts_with(
            "\naws3: [ERROR]: An error occurred (ParamValidation): Error parsing parameter \
             '--policy': Unable to load paramfile file://missing.json: No such file or directory (os error 2)"
        ),
        "{}",
        result.stderr
    );

    let stdout = response_output(
        &env,
        &["put-bucket-policy", "--bucket", "bucket", "--policy", "{}"],
        204,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}

#[test]
fn delete_bucket_policy() {
    let env = Env::new();
    env.bucket("bucket");
    let query = delete_bucket_configuration(&env, "delete-bucket-policy", "DeleteBucketPolicy");
    assert_eq!(query, "policy");

    let stdout = response_output(
        &env,
        &["delete-bucket-policy", "--bucket", "bucket"],
        204,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}
