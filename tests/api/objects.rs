//! The tests of the api operations of objects.

use super::{last_request_body, response_output, unimplemented_object_operation};
use crate::common::Env;

/// Returns the standard output without the line of the timestamp of the last
/// modification, which differs on each run.
fn without_last_modified(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|line| !line.trim_start().starts_with("\"LastModified\": "))
        .fold(String::new(), |mut stdout, line| {
            stdout.push_str(line);
            stdout.push('\n');
            stdout
        })
}

#[test]
fn head_object() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "hello");
    env.run(&[
        "cp",
        "a.txt",
        "s3://bucket/a.txt",
        "--metadata",
        "foo=bar",
        "--content-type",
        "text/plain",
        "--cache-control",
        "max-age=60",
        "--expires",
        "2030-01-01T00:00:00Z",
    ])
    .success();
    let result = env
        .run(&["api", "head-object", "--bucket", "bucket", "--key", "a.txt"])
        .success();
    assert!(
        result.stdout.contains("\n    \"LastModified\": \"20"),
        "{}",
        result.stdout
    );
    assert_eq!(
        without_last_modified(&result.stdout),
        r#"{
    "ContentLength": 5,
    "ChecksumCRC64NVME": "M3eFcAZSQlc=",
    "ChecksumType": "FULL_OBJECT",
    "ETag": "\"5d41402abc4b2a76b9719d911017c592\"",
    "CacheControl": "max-age=60",
    "ContentEncoding": "aws-chunked",
    "ContentType": "text/plain",
    "Expires": "2030-01-01T00:00:00+00:00",
    "Metadata": {
        "foo": "bar"
    },
    "ExpiresString": "Tue, 01 Jan 2030 00:00:00 GMT"
}
"#
    );

    env.object("bucket", "b.txt", "");
    let result = env
        .run(&["api", "head-object", "--bucket", "bucket", "--key", "b.txt"])
        .success();
    assert!(
        result.stdout.contains(
            r#"
    "Metadata": {}
"#
        ),
        "{}",
        result.stdout
    );
}

#[test]
fn head_object_request() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    env.run(&[
        "api",
        "head-object",
        "--bucket",
        "bucket",
        "--key",
        "a.txt",
        "--if-modified-since",
        "2000-01-01",
        "--part-number",
        "1",
        "--sse-customer-algorithm",
        "AES256",
        "--sse-customer-key",
        "0123456789abcdef0123456789abcdef",
        "--request-payer",
        "requester",
        "--checksum-mode",
        "ENABLED",
    ])
    .success();
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.method, http::Method::HEAD);
    assert_eq!(request.uri.path(), "/bucket/a.txt");
    assert_eq!(request.uri.query(), Some("partNumber=1"));
    let header = |name: &str| request.headers.get(name).unwrap().to_str().unwrap();
    assert_eq!(header("if-modified-since"), "Sat, 01 Jan 2000 00:00:00 GMT");
    assert_eq!(
        header("x-amz-server-side-encryption-customer-algorithm"),
        "AES256"
    );
    assert_eq!(
        header("x-amz-server-side-encryption-customer-key"),
        "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY="
    );
    assert_eq!(
        header("x-amz-server-side-encryption-customer-key-md5"),
        "hRasmdxgYDKV3nvbahU1MA=="
    );
    assert_eq!(header("x-amz-request-payer"), "requester");
    assert_eq!(header("x-amz-checksum-mode"), "ENABLED");
}

#[test]
fn head_object_errors() {
    let env = Env::new();
    env.bucket("bucket");
    let result = env.run(&["api", "head-object", "--bucket", "bucket", "--key", "a.txt"]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (404) when calling the HeadObject operation: Not Found\n"
    );

    let result = env.run(&["api", "head-object", "--bucket", "bucket"]);
    assert_eq!(result.code, 252);

    let result = env.run(&[
        "api",
        "head-object",
        "--bucket",
        "bucket",
        "--key",
        "a.txt",
        "--if-modified-since",
        "x",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
         Invalid type for parameter IfModifiedSince, value: \"x\", type: string, \
         valid types: timestamp\n"
    );

    let result = env.run(&[
        "api",
        "head-object",
        "--bucket",
        "bucket",
        "--key",
        "a.txt",
        "--part-number",
        "x",
    ]);
    assert_eq!(result.code, 255);
    assert_eq!(result.stderr, "\naws3: [ERROR]: Invalid integer value: x\n");

    let result = env.run(&[
        "api",
        "head-object",
        "--bucket",
        "bucket",
        "--key",
        "a.txt",
        "--part-number",
        "4294967296",
    ]);
    assert_eq!(result.code, 255);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: Integer value out of range: 4294967296\n"
    );
}

#[test]
fn get_object() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "hello world");
    env.run(&[
        "api",
        "put-object",
        "--bucket",
        "bucket",
        "--key",
        "a.txt",
        "--body",
        "a.txt",
        "--metadata",
        "foo=bar",
        "--content-type",
        "text/plain",
    ])
    .success();
    let result = env
        .run(&[
            "api",
            "get-object",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "out.txt",
        ])
        .success();
    assert_eq!(
        without_last_modified(&result.stdout),
        r#"{
    "ContentLength": 11,
    "ETag": "\"5eb63bbbe01eeed093cb22bb8f5acdc3\"",
    "ChecksumCRC64NVME": "jSnVw/bqjr4=",
    "ChecksumType": "FULL_OBJECT",
    "ContentType": "text/plain",
    "Metadata": {
        "foo": "bar"
    }
}
"#
    );
    assert_eq!(env.read("out.txt"), b"hello world");
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.method, http::Method::GET);
    assert_eq!(
        request.headers.get("x-amz-checksum-mode").unwrap(),
        "ENABLED"
    );

    let result = env
        .run(&[
            "api",
            "get-object",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "--range",
            "bytes=0-4",
            "out.txt",
        ])
        .success();
    assert!(
        result.stdout.contains(
            r#"
    "ContentRange": "bytes 0-4/11",
"#
        ),
        "{}",
        result.stdout
    );
    assert_eq!(env.read("out.txt"), b"hello");
}

#[test]
fn get_object_errors() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    std::fs::create_dir(env.path("dir")).unwrap();
    for (key, outfile, code, message) in [
        (
            "missing",
            "out.txt",
            254,
            "An error occurred (NoSuchKey) when calling the GetObject operation: Unknown",
        ),
        ("a.txt", "dir", 255, "dir: Is a directory (os error 21)"),
        (
            "a.txt",
            "missing/out.txt",
            255,
            "missing/out.txt: No such file or directory (os error 2)",
        ),
    ] {
        let result = env.run(&[
            "api",
            "get-object",
            "--bucket",
            "bucket",
            "--key",
            key,
            outfile,
        ]);
        assert_eq!(result.code, code, "{outfile}");
        assert_eq!(
            result.stderr,
            format!("\naws3: [ERROR]: {message}\n"),
            "{outfile}"
        );
    }
    // The file is not created on the errors of the requests.
    assert!(!env.path("out.txt").exists());

    // The structured formats omit the message absent in the error response.
    for (format, expected) in [
        (
            "json",
            r#"{
    "Code": "NoSuchKey"
}
"#,
        ),
        ("yaml", "Code: NoSuchKey\n"),
        ("text", "NoSuchKey\n"),
    ] {
        let result = env.run(&[
            "api",
            "get-object",
            "--bucket",
            "bucket",
            "--key",
            "missing",
            "out.txt",
            "--cli-error-format",
            format,
        ]);
        assert_eq!(result.code, 254);
        assert_eq!(result.stderr, expected, "{format}");
    }
}

#[test]
fn put_object() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "hello");
    let result = env
        .run(&[
            "api",
            "put-object",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "--body",
            "a.txt",
            "--metadata",
            "foo=bar",
            "--content-type",
            "text/plain",
            "--content-encoding",
            "gzip",
            "--expires",
            "2030-01-01T00:00:00Z",
            "--tagging",
            "a=b",
        ])
        .success();
    assert_eq!(
        result.stdout,
        r#"{
    "ETag": "\"5d41402abc4b2a76b9719d911017c592\"",
    "ChecksumCRC64NVME": "M3eFcAZSQlc=",
    "ChecksumType": "FULL_OBJECT"
}
"#
    );
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"hello");
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.method, http::Method::PUT);
    let header = |name: &str| request.headers.get(name).unwrap().to_str().unwrap();
    // The checksum is sent in the header over HTTP, without the aws-chunked
    // content encoding.
    assert_eq!(header("content-length"), "5");
    assert_eq!(header("content-encoding"), "gzip");
    assert_eq!(header("x-amz-checksum-crc64nvme"), "M3eFcAZSQlc=");
    assert_eq!(header("x-amz-sdk-checksum-algorithm"), "CRC64NVME");
    assert_eq!(header("x-amz-meta-foo"), "bar");
    assert_eq!(header("content-type"), "text/plain");
    assert_eq!(header("expires"), "Tue, 01 Jan 2030 00:00:00 GMT");
    assert_eq!(header("x-amz-tagging"), "a=b");

    // The empty object without the body and the content type.
    let result = env
        .run(&[
            "api",
            "put-object",
            "--bucket",
            "bucket",
            "--key",
            "empty",
            "--checksum-algorithm",
            "SHA256",
        ])
        .success();
    assert!(
        result
            .stdout
            .contains("\"ChecksumSHA256\": \"47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=\""),
        "{}",
        result.stdout
    );
    assert_eq!(env.object_content("bucket", "empty").unwrap(), b"");
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert!(!request.headers.contains_key("content-type"));

    // The given checksum is sent without the algorithm.
    env.run(&[
        "api",
        "put-object",
        "--bucket",
        "bucket",
        "--key",
        "a.txt",
        "--body",
        "a.txt",
        "--checksum-sha256",
        "LPJNul+wow4m6DsqxbninhsWHlwfp0JecwQzYpOLmCQ=",
    ])
    .success();
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert!(!request.headers.contains_key("x-amz-sdk-checksum-algorithm"));
}

#[test]
fn put_object_errors() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "hello");
    for (args, code, message) in [
        (
            &["--body", "missing.txt"][..],
            252,
            "An error occurred (ParamValidation): Error parsing parameter '--body': \
             Blob values must be a path to a file.",
        ),
        (
            &["--body", "a.txt", "--metadata", r#"{"n":1}"#],
            252,
            "An error occurred (ParamValidation): Parameter validation failed:\n\
             Invalid type for parameter Metadata.n, value: 1, type: integer, \
             valid types: string",
        ),
        (
            &["--body", "a.txt", "--metadata", "foo=\u{e9}"],
            252,
            "An error occurred (ParamValidation): Parameter validation failed:\n\
             Non ascii characters found in S3 metadata for key \"foo\", value: \"\u{e9}\".  \n\
             S3 metadata can only contain ASCII characters.",
        ),
        (
            &["--body", "a.txt", "--expires", "x"],
            252,
            "An error occurred (ParamValidation): Parameter validation failed:\n\
             Invalid type for parameter Expires, value: \"x\", type: string, \
             valid types: timestamp",
        ),
        (
            &[
                "--body",
                "a.txt",
                "--object-lock-event-hold-duration-days",
                "x",
            ],
            255,
            "Invalid integer value: x",
        ),
    ] {
        let mut command = vec!["api", "put-object", "--bucket", "bucket", "--key", "k"];
        command.extend(args);
        let result = env.run(&command);
        assert_eq!(result.code, code, "{args:?}");
        assert_eq!(
            result.stderr,
            format!("\naws3: [ERROR]: {message}\n"),
            "{args:?}"
        );
    }
    assert_eq!(env.object_content("bucket", "k"), None);
}

#[test]
fn put_object_content_encoding_over_tls() {
    let env = Env::new_tls();
    env.bucket("bucket");
    env.write("a.txt", "hello");
    env.run(&[
        "api",
        "put-object",
        "--bucket",
        "bucket",
        "--key",
        "a.txt",
        "--body",
        "a.txt",
        "--content-encoding",
        "gzip",
        "--ca-bundle",
        "ca.pem",
    ])
    .success();
    // The checksum is sent in the trailer of the aws-chunked encoding over
    // TLS, whose content encoding is merged into one header.
    let requests = env.requests();
    let request = requests.last().unwrap();
    let header = |name: &str| request.headers.get(name).unwrap().to_str().unwrap();
    let encodings: Vec<_> = request.headers.get_all("content-encoding").iter().collect();
    assert_eq!(encodings, ["gzip,aws-chunked"]);
    assert_eq!(header("x-amz-trailer"), "x-amz-checksum-crc64nvme");
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"hello");
}

#[test]
fn copy_object() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a b+\u{3042}.txt", "hello");
    let result = env
        .run(&[
            "api",
            "copy-object",
            "--bucket",
            "bucket",
            "--key",
            "b.txt",
            "--copy-source",
            "bucket/a b+\u{3042}.txt",
            "--metadata-directive",
            "REPLACE",
            "--metadata",
            "foo=bar",
            "--content-type",
            "text/plain",
            "--copy-source-if-modified-since",
            "2000-01-01",
        ])
        .success();
    assert_eq!(
        without_last_modified(&result.stdout),
        r#"{
    "CopyObjectResult": {
        "ETag": "\"5d41402abc4b2a76b9719d911017c592\"",
    }
}
"#
    );
    assert!(
        result.stdout.contains("\n        \"LastModified\": \"20"),
        "{}",
        result.stdout
    );
    assert_eq!(env.object_content("bucket", "b.txt").unwrap(), b"hello");
    let requests = env.requests();
    let request = requests.last().unwrap();
    let header = |name: &str| request.headers.get(name).unwrap().to_str().unwrap();
    assert_eq!(header("x-amz-copy-source"), "bucket/a%20b%2B%E3%81%82.txt");
    assert_eq!(header("x-amz-metadata-directive"), "REPLACE");
    assert_eq!(header("x-amz-meta-foo"), "bar");
    assert_eq!(header("content-type"), "text/plain");
    assert_eq!(
        header("x-amz-copy-source-if-modified-since"),
        "Sat, 01 Jan 2000 00:00:00 GMT"
    );

    // The version ID suffix is not encoded.
    env.run(&[
        "api",
        "copy-object",
        "--bucket",
        "bucket",
        "--key",
        "c.txt",
        "--copy-source",
        "bucket/b.txt?versionId=null",
    ])
    .success();
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(
        request.headers.get("x-amz-copy-source").unwrap(),
        "bucket/b.txt?versionId=null"
    );

    let result = env.run(&[
        "api",
        "copy-object",
        "--bucket",
        "bucket",
        "--key",
        "c.txt",
        "--copy-source",
        "bucket/missing",
    ]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (NoSuchKey) when calling the CopyObject operation: Unknown\n"
    );
}

#[test]
fn delete_object() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    let result = env
        .run(&[
            "api",
            "delete-object",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "--bypass-governance-retention",
            "--if-match-size",
            "5",
            "--if-match-last-modified-time",
            "2030-01-01T00:00:00Z",
        ])
        .success();
    // The empty response is not printed.
    assert_eq!(result.stdout, "");
    assert_eq!(env.object_content("bucket", "a.txt"), None);
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.method, http::Method::DELETE);
    assert_eq!(request.uri.path(), "/bucket/a.txt");
    let header = |name: &str| request.headers.get(name).unwrap().to_str().unwrap();
    assert_eq!(header("x-amz-bypass-governance-retention"), "true");
    assert_eq!(header("x-amz-if-match-size"), "5");
    assert_eq!(
        header("x-amz-if-match-last-modified-time"),
        "Tue, 01 Jan 2030 00:00:00 GMT"
    );

    let result = env.run(&[
        "api",
        "delete-object",
        "--bucket",
        "bucket",
        "--key",
        "a.txt",
        "--if-match-last-modified-time",
        "x",
    ]);
    assert_eq!(result.code, 252);
    assert!(
        result
            .stderr
            .contains("Invalid type for parameter IfMatchLastModifiedTime, value: \"x\",")
    );
}

#[test]
fn delete_objects() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "a");
    env.object("bucket", "b.txt", "b");
    env.object("bucket", "c.txt", "c");
    let result = env
        .run(&[
            "api",
            "delete-objects",
            "--bucket",
            "bucket",
            "--delete",
            "Objects=[{Key=a.txt},{Key=b.txt,Size=1}],Quiet=false",
        ])
        .success();
    assert_eq!(
        result.stdout,
        r#"{
    "Deleted": [
        {
            "Key": "a.txt"
        },
        {
            "Key": "b.txt"
        }
    ]
}
"#
    );
    assert_eq!(env.object_content("bucket", "a.txt"), None);
    assert_eq!(env.object_content("bucket", "b.txt"), None);
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.method, http::Method::POST);
    assert_eq!(request.uri.query(), Some("delete"));
    // The checksum is required, which defaults to CRC64NVME.
    assert!(request.headers.contains_key("x-amz-checksum-crc64nvme"));

    env.write("delete.json", r#"{"Objects": [{"Key": "c.txt"}]}"#);
    let result = env
        .run(&[
            "api",
            "delete-objects",
            "--bucket",
            "bucket",
            "--delete",
            "file://delete.json",
            "--checksum-algorithm",
            "SHA256",
        ])
        .success();
    assert!(result.stdout.contains("\"Key\": \"c.txt\""));
    assert_eq!(env.object_content("bucket", "c.txt"), None);
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert!(request.headers.contains_key("x-amz-checksum-sha256"));

    // The keys failed to be deleted are output as the errors, which succeeds
    // like the AWS CLI.
    let stdout = response_output(
        &env,
        &[
            "delete-objects",
            "--bucket",
            "bucket",
            "--delete",
            "Objects=[{Key=a.txt},{Key=b.txt},{Key=c.txt}]",
        ],
        200,
        &[],
        "<DeleteResult><Deleted><Key>a.txt</Key><VersionId>v1</VersionId>\
         <DeleteMarker>true</DeleteMarker><DeleteMarkerVersionId>m1</DeleteMarkerVersionId>\
         </Deleted><Error><Key>b.txt</Key><VersionId>v2</VersionId><Code>AccessDenied</Code>\
         <Message>Access Denied</Message></Error><Error><Key>c.txt</Key>\
         <Code>InternalError</Code><Message>We encountered an internal error. Please try \
         again.</Message></Error></DeleteResult>",
    );
    assert_eq!(
        stdout,
        r#"{
    "Deleted": [
        {
            "Key": "a.txt",
            "VersionId": "v1",
            "DeleteMarker": true,
            "DeleteMarkerVersionId": "m1"
        }
    ],
    "Errors": [
        {
            "Key": "b.txt",
            "VersionId": "v2",
            "Code": "AccessDenied",
            "Message": "Access Denied"
        },
        {
            "Key": "c.txt",
            "Code": "InternalError",
            "Message": "We encountered an internal error. Please try again."
        }
    ]
}
"#
    );
}

#[test]
fn delete_objects_errors() {
    let env = Env::new();
    env.bucket("bucket");
    for (delete, code, message) in [
        (
            r#"{"Objects":[{"Key":"a","Size":"3","Foo":null}],"Bar":1}"#,
            252,
            "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
             Unknown parameter in Delete: \"Bar\", must be one of: Objects, Quiet\n\
             Unknown parameter in Delete.Objects[0]: \"Foo\", must be one of: Key, VersionId, ETag, LastModifiedTime, Size\n\
             Invalid type for parameter Delete.Objects[0].Size, value: \"3\", type: string, valid types: integer\n",
        ),
        (
            "Objects=[{Key=a,Size=x}]",
            255,
            "\naws3: [ERROR]: Invalid integer value: x\n",
        ),
        (
            "Objects=[{Key=a}",
            252,
            "\naws3: [ERROR]: An error occurred (ParamValidation): Error parsing parameter '--delete': \
             Expected: ',', received: 'EOF' for input:\n Objects=[{Key=a}\n                ^\n",
        ),
    ] {
        let result = env.run(&[
            "api",
            "delete-objects",
            "--bucket",
            "bucket",
            "--delete",
            delete,
        ]);
        assert_eq!(result.code, code, "{delete}");
        assert_eq!(result.stderr, message, "{delete}");
    }
}

#[test]
fn restore_object() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    let (method, path, query) = unimplemented_object_operation(
        &env,
        &[
            "restore-object",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "--restore-request",
            "Days=7,GlacierJobParameters={Tier=Standard},Description=test",
        ],
        "RestoreObject",
    );
    assert_eq!(method, http::Method::POST);
    assert_eq!(path, "/bucket/a.txt");
    assert_eq!(query, "restore");
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<RestoreRequest xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Days>7</Days>\
             <GlacierJobParameters><Tier>Standard</Tier></GlacierJobParameters>\
             <Description>test</Description></RestoreRequest>"
        ),
        "{body}"
    );
    assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));

    // The objects in the archive tiers of Intelligent-Tiering are restored
    // without the request, whose empty body has the checksum.
    unimplemented_object_operation(
        &env,
        &["restore-object", "--bucket", "bucket", "--key", "a.txt"],
        "RestoreObject",
    );
    let (body, headers) = last_request_body(&env);
    assert_eq!(body, "");
    assert!(
        headers.contains(&"x-amz-checksum-crc64nvme: AAAAAAAAAAA=".to_owned()),
        "{headers:?}"
    );

    // The restores of S3 Select are not supported.
    let result = env.run(&[
        "api",
        "restore-object",
        "--bucket",
        "bucket",
        "--key",
        "a.txt",
        "--restore-request",
        "Type=SELECT",
    ]);
    assert_eq!(result.code, 255);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: RestoreRequest.Type of the restores of S3 Select is not supported by \
         the api command, use aws s3api restore-object instead\n"
    );

    let stdout = response_output(
        &env,
        &[
            "restore-object",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "--restore-request",
            "Days=1",
        ],
        202,
        &[
            ("x-amz-request-charged", "requester"),
            ("x-amz-restore-output-path", "bucket/out/"),
        ],
        "",
    );
    assert_eq!(
        stdout,
        r#"{
    "RequestCharged": "requester",
    "RestoreOutputPath": "bucket/out/"
}
"#
    );
}
