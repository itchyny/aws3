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
