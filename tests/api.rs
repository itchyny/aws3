mod common;

use common::Env;

/// Returns the standard output without the line of the timestamp of the last
/// modification, which differs on each run.
fn without_last_modified(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|line| !line.starts_with("    \"LastModified\": "))
        .map(|line| format!("{line}\n"))
        .collect()
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
        result.stdout.contains("\n    \"Metadata\": {}\n"),
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
         Invalid type for parameter IfModifiedSince, value: x, type: <class 'str'>, \
         valid types: <class 'datetime.datetime'>, timestamp-string\n"
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
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: invalid literal for int() with base 10: 'x'\n"
    );

    for (args, message) in [
        (&["--output", "text"][..], "--output text"),
        (&["--query", "ETag"][..], "--query"),
    ] {
        let mut command = vec!["api", "head-object", "--bucket", "bucket", "--key", "a.txt"];
        command.extend(args);
        let result = env.run(&command);
        assert_eq!(result.code, 255);
        assert_eq!(
            result.stderr,
            format!("\naws3: [ERROR]: {message} is not supported by the api command\n")
        );
    }
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
            .contains("Invalid type for parameter IfMatchLastModifiedTime, value: x,")
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
    assert!(request.headers.contains_key("x-amz-checksum-crc32"));

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
             Invalid type for parameter Delete.Objects[0].Size, value: 3, type: <class 'str'>, valid types: <class 'int'>\n",
        ),
        (
            "Objects=[{Key=a,Size=x}]",
            255,
            "\naws3: [ERROR]: invalid literal for int() with base 10: 'x'\n",
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
            255,
            "'int' object has no attribute 'encode'",
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
             Invalid type for parameter Expires, value: x, type: <class 'str'>, \
             valid types: <class 'datetime.datetime'>, timestamp-string",
        ),
        (
            &[
                "--body",
                "a.txt",
                "--object-lock-event-hold-duration-days",
                "x",
            ],
            255,
            "invalid literal for int() with base 10: 'x'",
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
