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
