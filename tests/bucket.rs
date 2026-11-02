// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::Env;

#[test]
fn mb_and_rb() {
    let env = Env::new();
    let result = env.run(&["mb", "s3://new-bucket"]).success();
    assert_eq!(result.stdout, "make_bucket: new-bucket\n");

    let result = env.run(&["mb", "new-bucket"]);
    assert_eq!(result.code, 252);
    assert!(
        result
            .stderr
            .contains("<S3Uri>\nError: Invalid argument type")
    );

    let result = env.run(&["rb", "s3://new-bucket/key"]);
    assert_eq!(result.code, 252);
    assert!(
        result
            .stderr
            .contains("Please specify a valid bucket name only. E.g. s3://new-bucket")
    );

    let result = env.run(&["rb", "s3://new-bucket"]).success();
    assert_eq!(result.stdout, "remove_bucket: new-bucket\n");

    let result = env.run(&["rb", "s3://new-bucket"]);
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .starts_with("remove_bucket failed: s3://new-bucket An error occurred (NoSuchBucket)")
    );
}

#[test]
fn rb_force() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "a");
    env.object("bucket", "dir/b.txt", "b");
    let result = env.run(&["rb", "s3://bucket", "--force"]).success();
    assert_eq!(
        result.sorted_lines(),
        [
            "delete: s3://bucket/a.txt",
            "delete: s3://bucket/dir/b.txt",
            "remove_bucket: bucket",
        ]
    );
}

#[test]
fn presign() {
    let env = Env::new();
    env.bucket("bucket");
    let result = env
        .run(&["presign", "s3://bucket/a b.txt", "--expires-in", "60"])
        .success();
    let url = result.stdout.trim_end();
    assert!(url.contains("/bucket/a%20b.txt?"), "{url}");
    assert!(url.contains("X-Amz-Expires=60"), "{url}");
    assert!(url.contains("X-Amz-Signature="), "{url}");
}

#[test]
fn mb_options_and_failures() {
    let env = Env::new();
    let result = env
        .run(&[
            "mb",
            "s3://regional",
            "--region",
            "ap-northeast-1",
            "--tags",
            "k",
            "v",
        ])
        .success();
    assert_eq!(result.stdout, "make_bucket: regional\n");
    let result = env.run(&["mb", "s3://regional"]);
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .starts_with("make_bucket failed: s3://regional An error occurred ("),
        "{}",
        result.stderr
    );
}

#[test]
fn website_unsupported() {
    let env = Env::new();
    env.bucket("bucket");
    // The fake server does not support the website configuration, which
    // checks the errors are reported.
    let result = env.run(&[
        "website",
        "bucket/",
        "--index-document",
        "index.html",
        "--error-document",
        "error.html",
    ]);
    assert_eq!(result.code, 254);
    assert!(
        result
            .stderr
            .contains("when calling the PutBucketWebsite operation"),
        "{}",
        result.stderr
    );
}

#[test]
fn presign_expires_in_limit() {
    let env = Env::new();
    let result = env.run(&["presign", "s3://bucket/key", "--expires-in", "604801"]);
    assert_eq!(result.code, 252);
}

#[test]
fn addressing_style() {
    let env = Env::new();
    env.bucket("bucket");
    // The path style is required for the endpoint of a host name, since the
    // virtual-hosted style puts the bucket name in the host name.
    let endpoint = env.endpoint().replace("127.0.0.1", "localhost");
    let presign = |style: &str| {
        let path = env.config(&format!(
            r"[default]
s3 =
  addressing_style = {style}
"
        ));
        let path = path.to_string_lossy().into_owned();
        let envs = [
            ("AWS_CONFIG_FILE", path.as_str()),
            ("AWS_ENDPOINT_URL", endpoint.as_str()),
        ];
        env.run_with_env(&["presign", "s3://bucket/key"], &envs)
            .success()
            .stdout
    };
    let host = endpoint.trim_start_matches("http://");
    assert!(presign("path").starts_with(&format!("http://{host}/bucket/key?")));
    assert!(presign("virtual").starts_with(&format!("http://bucket.{host}/key?")));
    // The path style is the default for custom endpoints like the AWS CLI.
    assert!(presign("auto").starts_with(&format!("http://{host}/bucket/key?")));

    let path = env.config(
        r"[default]
s3 =
  addressing_style = path
",
    );
    let path = path.to_string_lossy().into_owned();
    let envs = [
        ("AWS_CONFIG_FILE", path.as_str()),
        ("AWS_ENDPOINT_URL", endpoint.as_str()),
    ];
    env.write("a.txt", "hello");
    env.run_with_env(
        &["cp", "a.txt", "s3://bucket/a.txt", "--no-progress"],
        &envs,
    )
    .success();
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"hello");
    let result = env.run_with_env(&["ls", "s3://bucket/"], &envs).success();
    assert!(result.stdout.ends_with(" a.txt\n"), "{}", result.stdout);
}

#[test]
fn use_accelerate_endpoint() {
    let env = Env::new();
    let config = env.config(
        r"[default]
s3 =
  use_accelerate_endpoint = True
",
    );
    let presign = |args: &[&str], endpoint: Option<&str>| {
        let mut command = env.command(args);
        command
            .env("AWS_CONFIG_FILE", &config)
            .env_remove("AWS_ENDPOINT_URL");
        if let Some(endpoint) = endpoint {
            command.env("AWS_ENDPOINT_URL", endpoint);
        }
        command.output().unwrap()
    };
    let output = presign(&["presign", "s3://bucket/key"], None);
    let url = String::from_utf8_lossy(&output.stdout);
    assert!(
        url.starts_with("https://bucket.s3-accelerate.amazonaws.com/key?"),
        "{url}"
    );

    // S3 Accelerate cannot be used with custom endpoints and bucket names
    // with dots, which require the path style.
    let output = presign(&["presign", "s3://bucket/key"], Some(env.endpoint()));
    assert_eq!(output.status.code(), Some(255));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("aws3: [ERROR]: A custom endpoint cannot be combined with S3 Accelerate")
    );
    let output = presign(&["presign", "s3://my.bucket/key"], None);
    assert_eq!(output.status.code(), Some(255));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("aws3: [ERROR]: Path-style addressing cannot be used with S3 Accelerate")
    );
}
