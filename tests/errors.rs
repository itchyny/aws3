// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::Env;

#[test]
fn invalid_arguments() {
    let env = Env::new();
    let result = env.run(&["unknown"]);
    assert_eq!(result.code, 252);
    let result = env.run(&["ls", "--unknown"]);
    assert_eq!(result.code, 252);
    let result = env.run(&["cp", "a", "s3://bucket/", "--acl", "invalid"]);
    assert_eq!(result.code, 252);
    let result = env.run(&["--version"]).success();
    assert!(result.stdout.starts_with("aws3 "));
    let result = env.run(&["ls", "--help"]).success();
    assert!(result.stdout.contains("--recursive"));
}

#[test]
fn connection_refused() {
    let env = Env::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let result = env.run_with_env(
        &["ls", "--endpoint-url", &endpoint],
        &[("AWS_MAX_ATTEMPTS", "1")],
    );
    assert_eq!(result.code, 255);
    assert!(
        result
            .stderr
            .contains("Could not connect to the endpoint URL"),
        "{}",
        result.stderr
    );
}

#[test]
fn no_credentials() {
    let env = Env::new();
    let mut command = env.command(&["ls"]);
    command
        .env_remove("AWS_ACCESS_KEY_ID")
        .env_remove("AWS_SECRET_ACCESS_KEY");
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(253));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Unable to locate credentials"), "{stderr}");

    // Anonymous requests do not need credentials.
    let mut command = env.command(&["ls", "--no-sign-request"]);
    command
        .env_remove("AWS_ACCESS_KEY_ID")
        .env_remove("AWS_SECRET_ACCESS_KEY");
    let output = command.output().unwrap();
    assert_ne!(output.status.code(), Some(253));
}

#[test]
fn login_session() {
    let env = Env::new();
    env.write("a.txt", "a");
    let config = env.config(
        r"[default]
login_session = arn:aws:iam::123456789012:user/Admin
",
    );
    let cache = env.path("login-cache");
    // The errors of the credentials of `aws login` are reported before any
    // requests, even on the dry runs.
    for args in [
        &["ls"][..],
        &["cp", "a.txt", "s3://bucket/a.txt", "--dryrun"],
    ] {
        let mut command = env.command(args);
        command
            .env_remove("AWS_ACCESS_KEY_ID")
            .env_remove("AWS_SECRET_ACCESS_KEY")
            .env("AWS_CONFIG_FILE", &config)
            .env("AWS_LOGIN_CACHE_DIRECTORY", &cache);
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(255));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "\naws3: [ERROR]: Error loading login session token: Unable to load a existing \
             login session for session arn:aws:iam::123456789012:user/Admin, Please \
             reauthenticate with 'aws login'.\n"
        );
    }
}

#[test]
fn ca_bundle() {
    let env = Env::new();
    env.bucket("bucket");
    let result = env.run(&["ls", "--ca-bundle", "missing.pem"]);
    assert_eq!(result.code, 255);
    assert!(
        result
            .stderr
            .contains("Unable to load CA bundle missing.pem"),
        "{}",
        result.stderr
    );
    env.write("bad.pem", "garbage\n");
    let result = env.run_with_env(&["ls"], &[("AWS_CA_BUNDLE", "bad.pem")]);
    assert_eq!(result.code, 255);
    assert!(
        result.stderr.contains("no certificate found"),
        "{}",
        result.stderr
    );
    if let Ok(pem) = std::fs::read("/etc/ssl/cert.pem") {
        env.write("good.pem", pem);
        env.run(&["ls", "--ca-bundle", "good.pem"]).success();
    }
}

#[test]
fn timeouts_and_profile() {
    let env = Env::new();
    env.bucket("bucket");
    env.run(&[
        "ls",
        "--cli-connect-timeout",
        "0",
        "--cli-read-timeout",
        "0",
    ])
    .success();
    env.run(&[
        "ls",
        "--cli-connect-timeout",
        "5",
        "--cli-read-timeout",
        "5",
    ])
    .success();
    let path = env.config(
        r"[profile other]
region = us-west-2
",
    );
    let path = path.to_string_lossy().into_owned();
    env.run_with_env(&["ls", "--profile", "other"], &[("AWS_CONFIG_FILE", &path)])
        .success();
}

#[test]
fn tls_verification() {
    let env = Env::new_tls();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");

    // The certificate signed by an unknown CA is rejected by the default.
    let result = env.run(&["ls", "s3://bucket/", "--cli-connect-timeout", "5"]);
    assert_eq!(result.code, 255);
    assert!(
        result
            .stderr
            .contains("Could not connect to the endpoint URL"),
        "{}",
        result.stderr
    );

    // The CA bundle trusts the certificate.
    let result = env
        .run(&["ls", "s3://bucket/", "--ca-bundle", "ca.pem"])
        .success();
    assert!(result.stdout.ends_with(" a.txt\n"), "{}", result.stdout);

    // The CA bundle in the profile trusts the certificate, which is overridden
    // by the environment variable and the option.
    let config = env.config(&format!(
        "[default]\nca_bundle = {}\n",
        env.path("ca.pem").display()
    ));
    let config = config.to_string_lossy().into_owned();
    let envs = [("AWS_CONFIG_FILE", config.as_str())];
    env.run_with_env(&["ls", "s3://bucket/"], &envs).success();
    let envs = [
        ("AWS_CONFIG_FILE", config.as_str()),
        ("AWS_CA_BUNDLE", "missing.pem"),
    ];
    let result = env.run_with_env(&["ls", "s3://bucket/"], &envs);
    assert_eq!(result.code, 255);
    assert!(
        result
            .stderr
            .contains("Unable to load CA bundle missing.pem"),
        "{}",
        result.stderr
    );
    env.run_with_env(&["ls", "s3://bucket/", "--ca-bundle", "ca.pem"], &envs)
        .success();

    // The certificate is not verified with --no-verify-ssl.
    let result = env
        .run(&["ls", "s3://bucket/", "--no-verify-ssl"])
        .success();
    assert!(result.stdout.ends_with(" a.txt\n"), "{}", result.stdout);
    let result = env
        .run(&[
            "cp",
            "s3://bucket/a.txt",
            "out.txt",
            "--no-verify-ssl",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.lines(), ["download: s3://bucket/a.txt to ./out.txt"]);
    assert_eq!(env.read("out.txt"), b"hello");
}

#[test]
fn invalid_bucket_name() {
    let env = Env::new();
    env.write("a.txt", "a");
    let report = "Parameter validation failed:\nInvalid bucket name \"bad~name\": Bucket name must match the regex";

    // The requests are validated like the AWS CLI, so the errors are reported
    // in the same way as the errors of the requests.
    let result = env.run(&["ls", "s3://bad~name/"]);
    assert_eq!(result.code, 252);
    assert!(
        result.stderr.starts_with(&format!(
            "\naws3: [ERROR]: An error occurred (ParamValidation): {report}"
        )),
        "{}",
        result.stderr
    );
    let result = env.run(&["cp", "s3://bad~name/key", "out.txt"]);
    assert_eq!(result.code, 1);
    assert!(
        result.stderr.starts_with(&format!("fatal error: {report}")),
        "{}",
        result.stderr
    );
    let result = env.run(&["cp", "a.txt", "s3://bad~name/key", "--no-progress"]);
    assert_eq!(result.code, 1);
    assert!(
        result.stderr.starts_with(&format!(
            "upload failed: ./a.txt to s3://bad~name/key {report}"
        )),
        "{}",
        result.stderr
    );
    let result = env.run(&["rm", "s3://bad~name/key"]);
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .starts_with(&format!("delete failed: s3://bad~name/key {report}"))
    );
    let result = env.run(&["mb", "s3://bad~name"]);
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .starts_with(&format!("make_bucket failed: s3://bad~name {report}"))
    );

    // No requests are made on dry runs.
    let result = env
        .run(&["cp", "a.txt", "s3://bad~name/key", "--dryrun"])
        .success();
    assert_eq!(
        result.lines(),
        ["(dryrun) upload: ./a.txt to s3://bad~name/key"]
    );
}
