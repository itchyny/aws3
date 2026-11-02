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

#[test]
fn use_dualstack_endpoint() {
    let env = Env::new();
    let presign = |config: &str, envs: &[(&str, &str)]| {
        let config = env.config(config);
        let mut command = env.command(&["presign", "s3://bucket/key"]);
        command
            .env("AWS_CONFIG_FILE", &config)
            .env("AWS_REGION", "us-west-2")
            .env_remove("AWS_ENDPOINT_URL")
            .envs(envs.iter().copied());
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let url = String::from_utf8_lossy(&output.stdout).into_owned();
        url.split('?').next().unwrap().to_owned()
    };
    let dualstack = "https://bucket.s3.dualstack.us-west-2.amazonaws.com/key";
    let nested = |value: &str| {
        format!(
            r"[default]
s3 =
  use_dualstack_endpoint = {value}
"
        )
    };
    assert_eq!(presign(&nested("true"), &[]), dualstack);
    assert_eq!(presign(&nested("TRUE"), &[]), dualstack);
    assert_eq!(
        presign(&nested("false"), &[]),
        "https://bucket.s3.us-west-2.amazonaws.com/key"
    );
    // The nested setting does not disable the top-level setting.
    let config = r"[default]
use_dualstack_endpoint = true
s3 =
  use_dualstack_endpoint = false
";
    assert_eq!(presign(config, &[]), dualstack);
    assert_eq!(
        presign("[default]\n", &[("AWS_USE_DUALSTACK_ENDPOINT", "true")]),
        dualstack
    );

    // Custom endpoints disable the dual-stack and FIPS endpoints.
    let endpoint = [("AWS_ENDPOINT_URL", "http://localhost:9000")];
    assert_eq!(
        presign(&nested("true"), &endpoint),
        "http://localhost:9000/bucket/key"
    );
    let config = r"[default]
use_fips_endpoint = true
";
    assert_eq!(
        presign(config, &endpoint),
        "http://localhost:9000/bucket/key"
    );
}

#[test]
fn use_arn_region() {
    let env = Env::new();
    let presign = |config: &str, envs: &[(&str, &str)]| {
        let config = env.config(config);
        let mut command = env.command(&[
            "presign",
            "s3://arn:aws:s3:us-east-1:123456789012:accesspoint/my-ap/key",
        ]);
        command
            .env("AWS_CONFIG_FILE", &config)
            .env("AWS_REGION", "us-west-2")
            .env_remove("AWS_ENDPOINT_URL")
            .envs(envs.iter().copied());
        let output = command.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        (
            output.status.code().unwrap(),
            stdout.split('?').next().unwrap().to_owned(),
            stderr.trim().to_owned(),
        )
    };
    let arn_region = (
        0,
        "https://my-ap-123456789012.s3-accesspoint.us-east-1.amazonaws.com/key".to_owned(),
        String::new(),
    );
    let mismatch = (
        255,
        String::new(),
        "aws3: [ERROR]: Unsupported configuration when using S3 access-points: \
         Invalid configuration: region from ARN `us-east-1` does not match client region \
         `us-west-2` and UseArnRegion is `false`"
            .to_owned(),
    );
    // The region in the ARN is used by the default.
    assert_eq!(presign("[default]\n", &[]), arn_region);
    let nested_false = r"[default]
s3 =
  use_arn_region = false
";
    assert_eq!(presign(nested_false, &[]), mismatch);
    // The setting of the profile and the environment variable take precedence.
    let top_true = r"[default]
s3_use_arn_region = true
s3 =
  use_arn_region = false
";
    assert_eq!(presign(top_true, &[]), arn_region);
    assert_eq!(
        presign("[default]\ns3_use_arn_region = false\n", &[]),
        mismatch
    );
    assert_eq!(
        presign(nested_false, &[("AWS_S3_USE_ARN_REGION", "TRUE")]),
        arn_region
    );
    assert_eq!(
        presign("[default]\n", &[("AWS_S3_USE_ARN_REGION", "false")]),
        mismatch
    );
}

#[test]
fn disable_multiregion_access_points() {
    let env = Env::new();
    let presign = |config: &str, envs: &[(&str, &str)]| {
        let config = env.config(config);
        let mut command = env.command(&[
            "presign",
            "s3://arn:aws:s3::123456789012:accesspoint/mfzwi23gnjvgw.mrap/key",
        ]);
        command
            .env("AWS_CONFIG_FILE", &config)
            .env_remove("AWS_ENDPOINT_URL")
            .envs(envs.iter().copied());
        let output = command.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        (output.status.code().unwrap(), stdout, stderr)
    };
    // The multi-region access points are signed with SigV4a.
    let (code, url, _) = presign("[default]\n", &[]);
    assert_eq!(code, 0);
    assert!(
        url.starts_with("https://mfzwi23gnjvgw.mrap.accesspoint.s3-global.amazonaws.com/key?"),
        "{url}"
    );
    assert!(
        url.contains("X-Amz-Algorithm=AWS4-ECDSA-P256-SHA256"),
        "{url}"
    );

    let disabled = r"[default]
s3 =
  s3_disable_multiregion_access_points = true
";
    let error = "aws3: [ERROR]: Unsupported configuration when using S3 access-points: \
                 Invalid configuration: Multi-Region Access Point ARNs are disabled.";
    assert_eq!(
        presign(disabled, &[]),
        (255, String::new(), error.to_owned())
    );
    let envs = [("AWS_S3_DISABLE_MULTIREGION_ACCESS_POINTS", "TRUE")];
    assert_eq!(
        presign("[default]\n", &envs),
        (255, String::new(), error.to_owned())
    );
    // The environment variable overrides the config file.
    let envs = [("AWS_S3_DISABLE_MULTIREGION_ACCESS_POINTS", "false")];
    assert_eq!(presign(disabled, &envs).0, 0);

    let dualstack = r"[default]
s3 =
  use_dualstack_endpoint = true
";
    assert_eq!(
        presign(dualstack, &[]).2,
        "aws3: [ERROR]: Unsupported configuration when using S3 access-points: \
         S3 MRAP does not support dual-stack"
    );
}
