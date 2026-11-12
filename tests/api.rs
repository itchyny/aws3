// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::Env;

/// Returns the standard output without the line of the timestamp of the last
/// modification, which differs on each run.
fn without_last_modified(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|line| !line.trim_start().starts_with("\"LastModified\": "))
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
        result
            .stdout
            .contains("\n    \"ContentRange\": \"bytes 0-4/11\",\n"),
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
        ("a.txt", "dir", 255, "[Errno 21] Is a directory: 'dir'"),
        (
            "a.txt",
            "missing/out.txt",
            255,
            "[Errno 2] No such file or directory: 'missing/out.txt'",
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
        ("json", "{\n    \"Code\": \"NoSuchKey\"\n}\n"),
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
fn head_bucket() {
    let env = Env::new();
    env.bucket("bucket");
    // The fake server responds with no headers of the bucket.
    let result = env
        .run(&["api", "head-bucket", "--bucket", "bucket"])
        .success();
    assert_eq!(result.stdout, "");

    let result = env.run(&["api", "head-bucket", "--bucket", "missing"]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (404) when calling the HeadBucket operation: Not Found\n"
    );

    // The request is redirected to the region of the bucket.
    env.bucket("west");
    env.bucket_region("west", "us-west-2");
    env.run(&["api", "head-bucket", "--bucket", "west"])
        .success();
    let regions: Vec<_> = env
        .requests()
        .iter()
        .filter(|request| request.bucket() == "west")
        .map(|request| request.signed_region().to_owned())
        .collect();
    assert_eq!(regions, ["us-east-1", "us-west-2"]);
}

#[test]
fn get_bucket_location() {
    let env = Env::new();
    env.bucket("bucket");
    let result = env
        .run(&["api", "get-bucket-location", "--bucket", "bucket"])
        .success();
    // The location constraint of us-east-1 is null.
    assert_eq!(result.stdout, "{\n    \"LocationConstraint\": null\n}\n");
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.uri.path(), "/bucket/");
    assert_eq!(request.uri.query(), Some("location"));

    let result = env.run(&["api", "get-bucket-location", "--bucket", "missing"]);
    assert_eq!(result.code, 254);
    assert!(
        result
            .stderr
            .starts_with("\naws3: [ERROR]: An error occurred (NoSuchBucket) when calling the GetBucketLocation operation: "),
        "{}",
        result.stderr
    );
}

/// Returns the keys of the contents in the output of the listing.
fn listed_keys(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("\"Key\": \""))
        .map(|key| key.trim_end_matches(['"', ',']).to_owned())
        .collect()
}

#[test]
fn list_objects_v2() {
    let env = Env::new();
    env.bucket("bucket");
    for key in ["a b+c.txt", "a.txt", "dir/sub/z.txt", "dir/x.txt", "e.txt"] {
        env.object("bucket", key, "x");
    }
    let list = |args: &[&str]| {
        let mut command = vec!["api", "list-objects-v2", "--bucket", "bucket"];
        command.extend(args);
        env.run(&command).success().stdout
    };

    // The keys are decoded, and the pages are merged.
    let stdout = list(&["--page-size", "2"]);
    assert_eq!(
        listed_keys(&stdout),
        ["a b+c.txt", "a.txt", "dir/sub/z.txt", "dir/x.txt", "e.txt"]
    );
    assert!(
        stdout.ends_with("    ],\n    \"RequestCharged\": null,\n    \"Prefix\": null\n}\n"),
        "{stdout}"
    );
    let requests = env.requests();
    let queries: Vec<_> = requests
        .iter()
        .rev()
        .take(3)
        .map(|request| request.uri.query().unwrap_or_default().to_owned())
        .collect();
    assert!(
        queries
            .iter()
            .all(|query| query.contains("encoding-type=url"))
    );
    assert!(queries.iter().all(|query| query.contains("max-keys=2")));

    let stdout = list(&["--prefix", "dir/", "--delimiter", "/"]);
    assert_eq!(listed_keys(&stdout), ["dir/x.txt"]);
    assert!(
        stdout.contains("\"CommonPrefixes\": [\n        {\n            \"Prefix\": \"dir/sub/\"\n        }\n    ],\n    \"RequestCharged\": null,\n    \"Prefix\": \"dir/\"\n"),
        "{stdout}"
    );

    // The keys are not decoded with the encoding type given by the user.
    let stdout = list(&["--encoding-type", "url", "--max-items", "1"]);
    assert_eq!(listed_keys(&stdout), ["a+b%2Bc.txt"]);

    // The output is truncated with the token to resume.
    let stdout = list(&["--max-items", "3", "--page-size", "2"]);
    assert_eq!(
        listed_keys(&stdout),
        ["a b+c.txt", "a.txt", "dir/sub/z.txt"]
    );
    let token = "eyJDb250aW51YXRpb25Ub2tlbiI6ICJhLnR4dCIsICJib3RvX3RydW5jYXRlX2Ftb3VudCI6IDF9";
    assert!(
        stdout.ends_with(&format!("    \"NextToken\": \"{token}\"\n}}\n")),
        "{stdout}"
    );
    let stdout = list(&["--starting-token", token]);
    assert_eq!(listed_keys(&stdout), ["dir/x.txt", "e.txt"]);
    assert!(!stdout.contains("NextToken"));

    // The full response of a request without the pagination.
    let stdout = list(&["--max-keys", "2"]);
    assert_eq!(listed_keys(&stdout), ["a b+c.txt", "a.txt"]);
    assert!(
        stdout.starts_with("{\n    \"IsTruncated\": true,\n"),
        "{stdout}"
    );
    assert!(
        stdout.ends_with(
            "    \"Name\": \"bucket\",\n    \"MaxKeys\": 2,\n    \"EncodingType\": \"url\",\n    \
             \"KeyCount\": 2,\n    \"NextContinuationToken\": \"a.txt\"\n}\n"
        ),
        "{stdout}"
    );
    let stdout = list(&["--no-paginate"]);
    assert!(
        stdout.starts_with("{\n    \"IsTruncated\": false,\n"),
        "{stdout}"
    );

    let stdout = list(&["--prefix", "missing/"]);
    assert_eq!(
        stdout,
        "{\n    \"RequestCharged\": null,\n    \"Prefix\": \"missing/\"\n}\n"
    );
}

#[test]
fn list_objects_v2_errors() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "x");
    let result = env.run(&[
        "api",
        "list-objects-v2",
        "--bucket",
        "bucket",
        "--no-paginate",
        "--max-items",
        "1",
        "--starting-token",
        "x",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Cannot specify --no-paginate \
         along with pagination arguments: --starting-token, --max-items\n"
    );

    let result = env
        .run(&[
            "api",
            "list-objects-v2",
            "--bucket",
            "bucket",
            "--max-items",
            "0",
        ])
        .success();
    assert_eq!(
        result.stderr,
        "warning: Non-positive values for --max-items may result in undefined behavior.\n"
    );
    assert!(result.stdout.starts_with("{\n    \"Contents\": [],\n"));

    let result = env.run(&["api", "list-objects-v2", "--bucket", "missing"]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (NoSuchBucket) when calling the ListObjectsV2 operation: Unknown\n"
    );
}

#[test]
fn list_buckets() {
    let env = Env::new();
    env.bucket("bucket-a");
    env.bucket("bucket-b");
    let result = env.run(&["api", "list-buckets"]).success();
    // The fake server lists the buckets in the order of the file system.
    let mut names: Vec<_> = result
        .lines()
        .iter()
        .filter_map(|line| line.trim_start().strip_prefix("\"Name\": \""))
        .map(|name| name.trim_end_matches(['"', ',']).to_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["bucket-a", "bucket-b"]);
    assert!(
        result
            .stdout
            .ends_with("    ],\n    \"Owner\": null,\n    \"Prefix\": null\n}\n"),
        "{}",
        result.stdout
    );

    // The output is truncated with the token to resume.
    let result = env
        .run(&["api", "list-buckets", "--max-items", "1"])
        .success();
    assert!(
        result.stdout.ends_with(
            "    \"NextToken\": \"eyJDb250aW51YXRpb25Ub2tlbiI6IG51bGwsICJib3RvX3RydW5jYXRlX2Ftb3VudCI6IDF9\"\n}\n"
        ),
        "{}",
        result.stdout
    );

    let result = env
        .run(&["api", "list-buckets", "--no-paginate", "--prefix", "bucket"])
        .success();
    assert!(!result.stdout.contains("\"Owner\""), "{}", result.stdout);
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert!(request.uri.query().unwrap().contains("prefix=bucket"));
}

#[test]
fn list_object_versions() {
    let env = Env::new();
    env.bucket("bucket");
    // The fake server does not implement the versions.
    let result = env.run(&[
        "api",
        "list-object-versions",
        "--bucket",
        "bucket",
        "--prefix",
        "dir/",
        "--page-size",
        "2",
    ]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (NotImplemented) when calling the \
         ListObjectVersions operation: ListObjectVersions is not implemented yet\n"
    );
    let requests = env.requests();
    let mut query: Vec<_> = requests
        .last()
        .unwrap()
        .uri
        .query()
        .unwrap()
        .split('&')
        .collect();
    query.sort();
    assert_eq!(
        query,
        [
            "encoding-type=url",
            "max-keys=2",
            "prefix=dir%2F",
            "versions"
        ]
    );

    let result = env.run(&[
        "api",
        "list-object-versions",
        "--bucket",
        "bucket",
        "--key-marker",
        "a",
        "--page-size",
        "2",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Cannot specify --no-paginate \
         along with pagination arguments: --page-size\n"
    );
}

#[test]
fn create_bucket_and_delete_bucket() {
    let env = Env::new();
    let result = env
        .run(&[
            "api",
            "create-bucket",
            "--bucket",
            "new-bucket",
            "--create-bucket-configuration",
            "LocationConstraint=us-west-2,Tags=[{Key=a,Value=b}]",
            "--object-ownership",
            "BucketOwnerEnforced",
        ])
        .success();
    assert_eq!(result.stdout, "");
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.method, http::Method::PUT);
    assert_eq!(request.uri.path(), "/new-bucket/");
    assert_eq!(
        request.headers.get("x-amz-object-ownership").unwrap(),
        "BucketOwnerEnforced"
    );

    let result = env.run(&["api", "create-bucket", "--bucket", "new-bucket"]);
    assert_eq!(result.code, 254);
    assert!(
        result.stderr.starts_with(
            "\naws3: [ERROR]: An error occurred (BucketAlreadyExists) when calling the CreateBucket operation: "
        ),
        "{}",
        result.stderr
    );

    let result = env
        .run(&["api", "delete-bucket", "--bucket", "new-bucket"])
        .success();
    assert_eq!(result.stdout, "");
    let result = env.run(&["api", "delete-bucket", "--bucket", "new-bucket"]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (NoSuchBucket) when calling the DeleteBucket operation: Unknown\n"
    );

    let result = env.run(&[
        "api",
        "create-bucket",
        "--bucket",
        "new-bucket",
        "--create-bucket-configuration",
        "Tags=[{Key=a}],Foo=x",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
         Unknown parameter in CreateBucketConfiguration: \"Foo\", must be one of: \
         LocationConstraint, Location, Bucket, Tags\n\
         Missing required parameter in CreateBucketConfiguration.Tags[0]: \"Value\"\n"
    );
}

#[test]
fn query() {
    let env = Env::new();
    env.bucket("bucket");
    for (key, content) in [("a.txt", "abc"), ("b.txt", "x"), ("dir/c.txt", "hello")] {
        env.object("bucket", key, content);
    }
    let query = |args: &[&str]| {
        let mut command = vec!["api", "list-objects-v2", "--bucket", "bucket"];
        command.extend(args);
        env.run(&command)
    };

    // The query is applied to the merged pages.
    let result = query(&["--page-size", "1", "--query", "Contents[?Size > `1`].Key"]).success();
    assert_eq!(result.stdout, "[\n    \"a.txt\",\n    \"dir/c.txt\"\n]\n");
    let result = query(&["--query", "{n: length(Contents), s: sum(Contents[].Size)}"]).success();
    assert_eq!(result.stdout, "{\n    \"n\": 3,\n    \"s\": 9\n}\n");
    let result = query(&["--query", "Contents[0].Key"]).success();
    assert_eq!(result.stdout, "\"a.txt\"\n");
    let result = query(&["--query", "Missing"]).success();
    assert_eq!(result.stdout, "null\n");
    // The empty objects are not printed.
    let result = query(&["--query", "`{}`"]).success();
    assert_eq!(result.stdout, "");

    let result = env
        .run(&[
            "api",
            "head-object",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "--query",
            "[ContentLength, ETag]",
        ])
        .success();
    assert_eq!(
        result.stdout,
        "[\n    3,\n    \"\\\"900150983cd24fb0d6963f7d28e17f72\\\"\"\n]\n"
    );

    let result = query(&["--query", "Contents["]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Bad value for --query Contents[: \
         Invalid jmespath expression: Incomplete expression:\n\"Contents[\"\n          ^\n"
    );
    // The expression is validated before sending the requests.
    assert!(
        env.requests()
            .iter()
            .all(|request| request.method != http::Method::PUT)
    );

    let result = query(&["--query", "length(`1`)"]);
    assert_eq!(result.code, 255);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: In function length(), invalid type for value: 1, expected one of: \
         ['string', 'array', 'object'], received: \"number\"\n"
    );
}

/// Returns the body of the last request, and the headers of the checksum.
fn last_request_body(env: &Env) -> (String, Vec<String>) {
    let requests = env.requests();
    let request = requests.last().unwrap();
    let mut headers: Vec<_> = request
        .headers
        .iter()
        .filter(|(name, _)| {
            name.as_str().starts_with("x-amz-checksum-") || *name == "x-amz-sdk-checksum-algorithm"
        })
        .map(|(name, value)| format!("{name}: {}", value.to_str().unwrap()))
        .collect();
    headers.sort();
    (String::from_utf8_lossy(&request.body).into_owned(), headers)
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
         Invalid type for parameter PublicAccessBlockConfiguration.BlockPublicAcls, value: true, \
         type: <class 'str'>, valid types: <class 'bool'>\n"
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
             '--policy': Unable to load paramfile file://missing.json: [Errno 2] No such file or directory: "
        ),
        "{}",
        result.stderr
    );
}

#[test]
fn put_bucket_encryption() {
    let env = Env::new();
    env.bucket("bucket");
    env.run(&[
        "api",
        "put-bucket-encryption",
        "--bucket",
        "bucket",
        "--server-side-encryption-configuration",
        "Rules=[{ApplyServerSideEncryptionByDefault={SSEAlgorithm=aws:kms,KMSMasterKeyID=key},\
         BucketKeyEnabled=true,BlockedEncryptionTypes={EncryptionType=SSE-C}}]",
    ]);
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<ServerSideEncryptionConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <Rule><ApplyServerSideEncryptionByDefault><SSEAlgorithm>aws:kms</SSEAlgorithm>\
             <KMSMasterKeyID>key</KMSMasterKeyID></ApplyServerSideEncryptionByDefault>\
             <BucketKeyEnabled>true</BucketKeyEnabled><BlockedEncryptionTypes>\
             <EncryptionType>SSE-C</EncryptionType></BlockedEncryptionTypes></Rule>\
             </ServerSideEncryptionConfiguration>"
        ),
        "{body}"
    );
    assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));

    let result = env.run(&[
        "api",
        "put-bucket-encryption",
        "--bucket",
        "bucket",
        "--server-side-encryption-configuration",
        "Rules=[{ApplyServerSideEncryptionByDefault={KMSMasterKeyID=key}}]",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
         Missing required parameter in ServerSideEncryptionConfiguration.Rules[0]\
         .ApplyServerSideEncryptionByDefault: \"SSEAlgorithm\"\n"
    );
}

#[test]
fn put_bucket_lifecycle_configuration() {
    let env = Env::new();
    env.bucket("bucket");
    env.write(
        "lifecycle.json",
        r#"{"Rules": [{"ID": "archive", "Filter": {"And": {"Prefix": "data/",
            "Tags": [{"Key": "k", "Value": "v"}], "ObjectSizeGreaterThan": 1024}},
            "Status": "Enabled", "Transitions": [{"Days": 30, "StorageClass": "GLACIER"}],
            "Expiration": {"Date": "2030-01-01T00:00:00Z"}}]}"#,
    );
    env.run(&[
        "api",
        "put-bucket-lifecycle-configuration",
        "--bucket",
        "bucket",
        "--lifecycle-configuration",
        "file://lifecycle.json",
        "--transition-default-minimum-object-size",
        "all_storage_classes_128K",
    ]);
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<LifecycleConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Rule>\
             <Expiration><Date>2030-01-01T00:00:00Z</Date></Expiration><ID>archive</ID>\
             <Filter><And><Prefix>data/</Prefix><Tag><Key>k</Key><Value>v</Value></Tag>\
             <ObjectSizeGreaterThan>1024</ObjectSizeGreaterThan></And></Filter>\
             <Status>Enabled</Status><Transition><Days>30</Days>\
             <StorageClass>GLACIER</StorageClass></Transition></Rule></LifecycleConfiguration>"
        ),
        "{body}"
    );
    assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));
    let requests = env.requests();
    assert_eq!(
        requests
            .last()
            .unwrap()
            .headers
            .get("x-amz-transition-default-minimum-object-size")
            .unwrap(),
        "all_storage_classes_128K"
    );

    for (value, code, message) in [
        (
            "Rules=[{Filter={Prefix=x}}]",
            252,
            "An error occurred (ParamValidation): Parameter validation failed:\n\
             Missing required parameter in LifecycleConfiguration.Rules[0]: \"Status\"",
        ),
        (
            "Rules=[{Status=Enabled,Expiration={Days=x}}]",
            255,
            "invalid literal for int() with base 10: 'x'",
        ),
    ] {
        let result = env.run(&[
            "api",
            "put-bucket-lifecycle-configuration",
            "--bucket",
            "bucket",
            "--lifecycle-configuration",
            value,
        ]);
        assert_eq!(result.code, code, "{value}");
        assert_eq!(
            result.stderr,
            format!("\naws3: [ERROR]: {message}\n"),
            "{value}"
        );
    }
}

/// Runs the get operation of a bucket configuration, which the fake server
/// does not implement, and returns the query of the request.
fn get_bucket_configuration(env: &Env, operation: &str, name: &str) -> String {
    bucket_configuration(env, operation, name, http::Method::GET)
}

/// Runs the delete operation of a bucket configuration, which the fake server
/// does not implement, and returns the query of the request.
fn delete_bucket_configuration(env: &Env, operation: &str, name: &str) -> String {
    bucket_configuration(env, operation, name, http::Method::DELETE)
}

fn bucket_configuration(env: &Env, operation: &str, name: &str, method: http::Method) -> String {
    let result = env.run(&["api", operation, "--bucket", "bucket"]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        format!(
            "\naws3: [ERROR]: An error occurred (NotImplemented) when calling the {name} \
             operation: {name} is not implemented yet\n"
        )
    );
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.method, method);
    assert_eq!(request.uri.path(), "/bucket/");
    request.uri.query().unwrap().to_owned()
}

#[test]
fn get_bucket_versioning() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-versioning", "GetBucketVersioning");
    assert_eq!(query, "versioning");
}

#[test]
fn get_public_access_block() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-public-access-block", "GetPublicAccessBlock");
    assert_eq!(query, "publicAccessBlock");
}

#[test]
fn get_bucket_policy() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-policy", "GetBucketPolicy");
    assert_eq!(query, "policy");
}

#[test]
fn get_bucket_encryption() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-encryption", "GetBucketEncryption");
    assert_eq!(query, "encryption");
}

#[test]
fn get_bucket_lifecycle_configuration() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(
        &env,
        "get-bucket-lifecycle-configuration",
        "GetBucketLifecycleConfiguration",
    );
    assert_eq!(query, "lifecycle");
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
fn output_text() {
    let env = Env::new();
    env.bucket("bucket");
    for (key, content) in [("a.txt", "abc"), ("b.txt", "x"), ("dir/c.txt", "hello")] {
        env.object("bucket", key, content);
    }
    let run = |args: &[&str]| {
        let mut command = vec!["api"];
        command.extend(args);
        command.extend(["--output", "text"]);
        env.run(&command).success().stdout
    };

    // The scalars are written in the order of the keys, and the nested values
    // follow with the upper-cased keys.
    let stdout = run(&["list-objects-v2", "--bucket", "bucket", "--delimiter", "/"]);
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(lines[0], "None\tNone");
    assert_eq!(lines[1], "COMMONPREFIXES\tdir/");
    assert!(lines[2].starts_with("CONTENTS\ta.txt\t20"), "{stdout}");
    assert!(lines[2].ends_with("\t3"), "{stdout}");
    assert!(lines[3].starts_with("CONTENTS\tb.txt\t"), "{stdout}");
    assert_eq!(lines.len(), 4);

    // The pages are written one by one with the query, followed by the token
    // to resume.
    let stdout = run(&[
        "list-objects-v2",
        "--bucket",
        "bucket",
        "--page-size",
        "2",
        "--max-items",
        "2",
        "--query",
        "Contents[].Key",
    ]);
    assert_eq!(stdout, "a.txt\tb.txt\nNone\n");
    let stdout = run(&["list-objects-v2", "--bucket", "bucket", "--max-items", "1"]);
    assert!(
        stdout.ends_with(
            "\nNEXTTOKEN\teyJDb250aW51YXRpb25Ub2tlbiI6IG51bGwsICJib3RvX3RydW5jYXRlX2Ftb3VudCI6IDF9\n"
        ),
        "{stdout}"
    );

    let stdout = run(&["get-bucket-location", "--bucket", "bucket"]);
    assert_eq!(stdout, "None\n");
}

#[test]
fn output_format() {
    let env = Env::new();
    env.bucket("bucket");
    let args = ["api", "get-bucket-location", "--bucket", "bucket"];
    let json = "{\n    \"LocationConstraint\": null\n}\n";

    // The format is of the option, the environment variable, or the config
    // file of the profile.
    let config = env.config("[default]\noutput = text\n[profile json]\noutput = json\n");
    let config = config.to_str().unwrap();
    let result = env
        .run_with_env(&args, &[("AWS_CONFIG_FILE", config)])
        .success();
    assert_eq!(result.stdout, "None\n");
    let mut with_profile = args.to_vec();
    with_profile.extend(["--profile", "json"]);
    let result = env
        .run_with_env(&with_profile, &[("AWS_CONFIG_FILE", config)])
        .success();
    assert_eq!(result.stdout, json);
    let result = env
        .run_with_env(
            &args,
            &[("AWS_CONFIG_FILE", config), ("AWS_DEFAULT_OUTPUT", "json")],
        )
        .success();
    assert_eq!(result.stdout, json);
    let mut with_option = args.to_vec();
    with_option.extend(["--output", "json"]);
    let result = env
        .run_with_env(&with_option, &[("AWS_DEFAULT_OUTPUT", "text")])
        .success();
    assert_eq!(result.stdout, json);
    let mut with_off = args.to_vec();
    with_off.extend(["--output", "off"]);
    assert_eq!(env.run(&with_off).success().stdout, "");

    let result = env.run_with_env(&args, &[("AWS_DEFAULT_OUTPUT", "TEXT")]);
    assert_eq!(result.code, 255);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: Unknown output type: TEXT\n"
    );
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
}

#[test]
fn delete_bucket_policy() {
    let env = Env::new();
    env.bucket("bucket");
    let query = delete_bucket_configuration(&env, "delete-bucket-policy", "DeleteBucketPolicy");
    assert_eq!(query, "policy");
}

#[test]
fn delete_bucket_encryption() {
    let env = Env::new();
    env.bucket("bucket");
    let query =
        delete_bucket_configuration(&env, "delete-bucket-encryption", "DeleteBucketEncryption");
    assert_eq!(query, "encryption");
}

#[test]
fn delete_bucket_lifecycle() {
    let env = Env::new();
    env.bucket("bucket");
    let query =
        delete_bucket_configuration(&env, "delete-bucket-lifecycle", "DeleteBucketLifecycle");
    assert_eq!(query, "lifecycle");
}

#[test]
fn get_bucket_cors() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-cors", "GetBucketCors");
    assert_eq!(query, "cors");
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
}

#[test]
fn delete_bucket_cors() {
    let env = Env::new();
    env.bucket("bucket");
    let query = delete_bucket_configuration(&env, "delete-bucket-cors", "DeleteBucketCors");
    assert_eq!(query, "cors");
}

#[test]
fn output_table() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "abc");
    env.object("bucket", "dir/b.txt", "x");
    let run = |args: &[&str]| {
        let mut command = vec!["api"];
        command.extend(args);
        command.extend(["--output", "table"]);
        env.run(&command)
    };

    // The scalars are the columns of the rows, and the nested values are the
    // indented sections, where the title is the name of the operation.
    let result = run(&[
        "list-objects-v2",
        "--bucket",
        "bucket",
        "--delimiter",
        "/",
        "--query",
        "{Prefix: Prefix, Keys: Contents[].{Key: Key, Size: Size}, CommonPrefixes: CommonPrefixes}",
    ])
    .success();
    assert_eq!(
        result.stdout,
        "---------------------\n\
         |   ListObjectsV2   |\n\
         +----------+--------+\n\
         |  Prefix  |  None  |\n\
         +----------+--------+\n\
         || CommonPrefixes  ||\n\
         |+-----------------+|\n\
         ||     Prefix      ||\n\
         |+-----------------+|\n\
         ||  dir/           ||\n\
         |+-----------------+|\n\
         ||      Keys       ||\n\
         |+--------+--------+|\n\
         ||   Key  | Size   ||\n\
         |+--------+--------+|\n\
         ||  a.txt |  3     ||\n\
         |+--------+--------+|\n"
    );

    // The colors are reset after each write like the AWS CLI. The expected
    // outputs are of the formatter of the AWS CLI.
    let result = env
        .run(&[
            "api",
            "get-bucket-location",
            "--bucket",
            "bucket",
            "--query",
            "{a: 'x'}",
            "--output",
            "table",
            "--color",
            "on",
        ])
        .success();
    assert_eq!(
        result.stdout,
        "-------------------\n\x1b[0m\
         \x1b[0m|GetBucketLocation|\x1b[0m\x1b[0m\n\x1b[0m\
         \x1b[0m+-------+---------+\x1b[0m\x1b[0m\n\x1b[0m\
         \x1b[0m|  \x1b[1m\x1b[34ma\x1b[0m    |  \x1b[1m\x1b[34mx\x1b[0m      |\x1b[0m\x1b[0m\n\x1b[0m\
         \x1b[0m+-------+---------+\x1b[0m\x1b[0m\n\x1b[0m"
    );

    // The false values of Python are not written.
    let result = run(&[
        "get-bucket-location",
        "--bucket",
        "bucket",
        "--query",
        "LocationConstraint",
    ]);
    assert_eq!(result.success().stdout, "");

    let result = run(&[
        "get-bucket-location",
        "--bucket",
        "bucket",
        "--query",
        "`[{\"a\": 1}, \"x\"]`",
    ]);
    assert_eq!(result.code, 255);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: string indices must be integers, not 'str'\n"
    );
}

#[test]
fn output_yaml() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "abc");
    env.object("bucket", "dir/b.txt", "x");
    let run = |args: &[&str], output: &str| {
        let mut command = vec!["api"];
        command.extend(args);
        command.extend(["--output", output]);
        env.run(&command).success().stdout
    };

    // The keys of the mappings are sorted, and the strings resolved to the
    // other types are quoted.
    let stdout = run(
        &[
            "list-objects-v2",
            "--bucket",
            "bucket",
            "--query",
            "{Keys: Contents[].Key, Prefix: Prefix, Size: to_string(Contents[0].Size), Empty: `[]`}",
        ],
        "yaml",
    );
    assert_eq!(
        stdout,
        "Empty: []\nKeys:\n- a.txt\n- dir/b.txt\nPrefix: null\nSize: '3'\n"
    );
    let stdout = run(&["get-bucket-location", "--bucket", "bucket"], "yaml");
    assert_eq!(stdout, "LocationConstraint: null\n");

    // The scalars are written in JSON.
    let stdout = run(
        &[
            "list-objects-v2",
            "--bucket",
            "bucket",
            "--query",
            "Contents[0].Key",
        ],
        "yaml",
    );
    assert_eq!(stdout, "\"a.txt\"\n");

    // The pages are written one by one in the lists, where the other result
    // keys are null on the first page of the starting token.
    let stdout = run(
        &[
            "list-objects-v2",
            "--bucket",
            "bucket",
            "--page-size",
            "1",
            "--query",
            "Contents[].Key",
        ],
        "yaml-stream",
    );
    assert_eq!(stdout, "- - a.txt\n- - dir/b.txt\n");
    let stdout = run(
        &[
            "list-objects-v2",
            "--bucket",
            "bucket",
            "--starting-token",
            "eyJDb250aW51YXRpb25Ub2tlbiI6IG51bGwsICJib3RvX3RydW5jYXRlX2Ftb3VudCI6IDF9",
            "--query",
            "[CommonPrefixes, Contents[].Key, NextToken]",
        ],
        "yaml-stream",
    );
    assert_eq!(stdout, "- - null\n  - - dir/b.txt\n  - null\n");

    // The empty response is not written.
    let stdout = run(
        &[
            "get-bucket-location",
            "--bucket",
            "bucket",
            "--query",
            "`{}`",
        ],
        "yaml-stream",
    );
    assert_eq!(stdout, "");
}

#[test]
fn get_bucket_tagging() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-tagging", "GetBucketTagging");
    assert_eq!(query, "tagging");
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
}

#[test]
fn delete_bucket_tagging() {
    let env = Env::new();
    env.bucket("bucket");
    let query = delete_bucket_configuration(&env, "delete-bucket-tagging", "DeleteBucketTagging");
    assert_eq!(query, "tagging");
}

/// Runs an operation of the tags of an object, which the fake server does not
/// implement, and returns the method, the path and the query of the request.
fn object_tagging(env: &Env, args: &[&str], name: &str) -> (http::Method, String, String) {
    let mut command = vec!["api"];
    command.extend(args);
    let result = env.run(&command);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        format!(
            "\naws3: [ERROR]: An error occurred (NotImplemented) when calling the {name} \
             operation: {name} is not implemented yet\n"
        )
    );
    let requests = env.requests();
    let request = requests.last().unwrap();
    (
        request.method.clone(),
        request.uri.path().to_owned(),
        request.uri.query().unwrap().to_owned(),
    )
}

#[test]
fn get_object_tagging() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "dir/a b.txt", "hello");
    let (method, path, query) = object_tagging(
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
}
