//! The tests of the api operations of CORS.

use super::{
    delete_bucket_configuration, get_bucket_configuration, last_request_body, response_output,
};
use crate::common::Env;

#[test]
fn get_bucket_cors() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-cors", "GetBucketCors");
    assert_eq!(query, "cors");

    let stdout = response_output(
        &env,
        &["get-bucket-cors", "--bucket", "bucket"],
        200,
        &[],
        r#"<CORSConfiguration>
    <CORSRule>
        <ID>web</ID>
        <AllowedHeader>*</AllowedHeader>
        <AllowedMethod>GET</AllowedMethod>
        <AllowedMethod>PUT</AllowedMethod>
        <AllowedOrigin>https://example.com</AllowedOrigin>
        <ExposeHeader>ETag</ExposeHeader>
        <MaxAgeSeconds>3000</MaxAgeSeconds>
    </CORSRule>
    <CORSRule>
        <AllowedMethod>GET</AllowedMethod>
        <AllowedOrigin>*</AllowedOrigin>
    </CORSRule>
</CORSConfiguration>"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "CORSRules": [
        {
            "ID": "web",
            "AllowedHeaders": [
                "*"
            ],
            "AllowedMethods": [
                "GET",
                "PUT"
            ],
            "AllowedOrigins": [
                "https://example.com"
            ],
            "ExposeHeaders": [
                "ETag"
            ],
            "MaxAgeSeconds": 3000
        },
        {
            "AllowedMethods": [
                "GET"
            ],
            "AllowedOrigins": [
                "*"
            ]
        }
    ]
}
"#
    );
}

#[test]
fn put_bucket_cors() {
    let env = Env::new();
    env.bucket("bucket");
    env.run(&[
        "api",
        "put-bucket-cors",
        "--bucket",
        "bucket",
        "--cors-configuration",
        "CORSRules=[{ID=web,AllowedMethods=[GET,PUT],AllowedOrigins=*,MaxAgeSeconds=60}]",
    ]);
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<CORSConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><CORSRule>\
             <ID>web</ID><AllowedMethod>GET</AllowedMethod><AllowedMethod>PUT</AllowedMethod>\
             <AllowedOrigin>*</AllowedOrigin><MaxAgeSeconds>60</MaxAgeSeconds></CORSRule>\
             </CORSConfiguration>"
        ),
        "{body}"
    );
    assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));

    let result = env.run(&[
        "api",
        "put-bucket-cors",
        "--bucket",
        "bucket",
        "--cors-configuration",
        "CORSRules=[{AllowedMethods=[GET]}]",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
         Missing required parameter in CORSConfiguration.CORSRules[0]: \"AllowedOrigins\"\n"
    );

    let stdout = response_output(
        &env,
        &[
            "put-bucket-cors",
            "--bucket",
            "bucket",
            "--cors-configuration",
            r#"{"CORSRules":[{"AllowedMethods":["GET"],"AllowedOrigins":["*"]}]}"#,
        ],
        200,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}

#[test]
fn delete_bucket_cors() {
    let env = Env::new();
    env.bucket("bucket");
    let query = delete_bucket_configuration(&env, "delete-bucket-cors", "DeleteBucketCors");
    assert_eq!(query, "cors");

    let stdout = response_output(
        &env,
        &["delete-bucket-cors", "--bucket", "bucket"],
        204,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}
