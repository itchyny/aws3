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
fn completion() {
    let env = Env::new();
    for shell in ["bash", "zsh", "fish", "elvish", "powershell"] {
        let result = env.run(&["completion", shell]).success();
        assert!(result.stdout.contains("recursive"), "{shell}");
        // The completion subcommand is hidden from the completions.
        assert!(
            !result.stdout.contains("Generate the completion script"),
            "{shell}"
        );
    }
    let result = env.run(&["--help"]).success();
    assert!(!result.stdout.contains("completion"));
    let result = env
        .run_with_closed_stdout(&["completion", "bash"])
        .success();
    assert_eq!(result.stderr, "");
    let result = env.run(&["completion", "tcsh"]);
    assert_eq!(result.code, 252);
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
        result.stderr.contains(&format!(
            "Could not connect to the endpoint URL: \"{endpoint}/\""
        )),
        "{}",
        result.stderr
    );
}

/// Starts the server responding to every request with the status and the body,
/// and returns the endpoint and the counter of the requests.
fn serve(
    status: &'static str,
    body: &'static str,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = count.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let _ = write!(
                &stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (endpoint, count)
}

#[test]
fn retried_errors() {
    let env = Env::new();
    // The errors of the responses with the 200 status code are retried as 500
    // errors, and reported with the retries and the reason phrase.
    for (status, body, attempts, expected, requests) in [
        (
            "200 OK",
            "not xml",
            "3",
            "An error occurred (500) when calling the ListObjectsV2 operation \
             (reached max retries: 2): Internal Server Error",
            3,
        ),
        // The empty bodies are not retried, which botocore parses as empty.
        (
            "200 OK",
            "",
            "3",
            "An error occurred (500) when calling the ListObjectsV2 operation: \
             Internal Server Error",
            1,
        ),
        (
            "503 Service Unavailable",
            "<Error><Code>SlowDown</Code><Message>Please reduce your request rate.</Message></Error>",
            "2",
            "An error occurred (SlowDown) when calling the ListObjectsV2 operation \
             (reached max retries: 1): Please reduce your request rate.",
            2,
        ),
        (
            "404 Not Found",
            "nope",
            "3",
            "An error occurred (404) when calling the ListObjectsV2 operation: Not Found",
            1,
        ),
    ] {
        let (endpoint, count) = serve(status, body);
        let result = env.run_with_env(
            &["ls", "s3://bucket/", "--endpoint-url", &endpoint],
            &[("AWS_MAX_ATTEMPTS", attempts)],
        );
        assert_eq!(result.code, 254, "{status}");
        assert!(result.stderr.contains(expected), "{}", result.stderr);
        assert_eq!(
            count.load(std::sync::atomic::Ordering::SeqCst),
            requests,
            "{status}"
        );
    }
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
fn cli_error_format() {
    let env = Env::new();
    let run = |format: &[&str], envs: &[(&str, &str)]| {
        let mut args = vec!["presign", "s3://bucket"];
        args.extend(format);
        let result = env.run_with_env(&args, envs);
        assert_eq!(result.code, 252);
        result.stderr
    };
    let message = "Parameter validation failed:\nInvalid length for parameter Key, value: 0, \
                   valid min length: 1";
    assert_eq!(
        run(&[], &[]),
        format!("\naws3: [ERROR]: An error occurred (ParamValidation): {message}\n")
    );
    assert_eq!(
        run(&["--cli-error-format", "legacy"], &[]),
        format!("\naws3: [ERROR]: {message}\n")
    );
    let json = format!(
        "{{\n    \"Code\": \"ParamValidation\",\n    \"Message\": {}\n}}\n",
        serde_json::to_string(message).unwrap()
    );
    assert_eq!(run(&["--cli-error-format", "json"], &[]), json);
    assert_eq!(run(&[], &[("AWS_CLI_ERROR_FORMAT", "JSON")]), json);
    assert_eq!(
        run(&["--cli-error-format", "text"], &[]),
        format!("ParamValidation\t{message}\n")
    );
    let config = env.config("[default]\ncli_error_format = yaml\n");
    assert_eq!(
        run(&[], &[("AWS_CONFIG_FILE", config.to_str().unwrap())]),
        "Code: ParamValidation\nMessage: 'Parameter validation failed:\n\n  \
         Invalid length for parameter Key, value: 0, valid min length: 1'\n"
    );
    // The invalid formats of the environment variable are ignored.
    assert_eq!(
        run(&[], &[("AWS_CLI_ERROR_FORMAT", "bogus")]),
        run(&[], &[])
    );
}

#[test]
fn directory_buckets() {
    let env = Env::new();
    env.write("dir/a.txt", "a");
    let bucket = "s3://bucket--use1-az4--x-s3";
    let cases: [(&[&str], &str); 4] = [
        (
            &["mb", bucket],
            "Cannot use mb command with a directory bucket.",
        ),
        (
            &["sync", "dir", &format!("{bucket}/dir/")],
            "Cannot use sync command with a directory bucket.",
        ),
        (
            &["sync", &format!("{bucket}/dir/"), "dir", "--dryrun"],
            "Cannot use sync command with a directory bucket.",
        ),
        (
            &[
                "cp",
                &format!("{bucket}/dir/"),
                "out",
                "--recursive",
                "--case-conflict",
                "skip",
            ],
            "`skip` is not a valid value for `--case-conflict` when operating on S3 Express \
             directory buckets. Valid values: `warn`, `ignore`.",
        ),
    ];
    for (args, message) in cases {
        let result = env.run(args);
        assert_eq!(result.code, 252, "{args:?}");
        assert_eq!(
            result.stderr,
            format!("\naws3: [ERROR]: An error occurred (ParamValidation): {message}\n"),
        );
    }
    // The case conflicts are warned about without the newline, before the
    // transfers fail on the fake server without S3 Express.
    let source = format!("{bucket}/dir/");
    let result = env.run(&[
        "cp",
        &source,
        "out",
        "--recursive",
        "--case-conflict",
        "warn",
    ]);
    assert!(
        result.stderr.starts_with(
            "warning: Recursive copies/moves from an S3 Express directory bucket to a \
             case-insensitive local filesystem"
        ),
        "{}",
        result.stderr
    );
    assert!(
        !result.stderr.contains("s3-case-insensitivity.html.\n"),
        "{}",
        result.stderr
    );
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
        result.stderr.contains("SSL validation failed for https://"),
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
fn proxy() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    let proxy = env.proxy();
    let url = proxy.url(Some("user:pass"));

    // The requests to the HTTP endpoint are forwarded by HTTP_PROXY, with the
    // credentials of the proxy.
    let result = env
        .run_with_env(&["ls", "s3://bucket/"], &[("HTTP_PROXY", &url)])
        .success();
    assert!(result.stdout.ends_with(" a.txt\n"), "{}", result.stdout);
    let requests = proxy.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let prefix = format!("GET {}/bucket", env.endpoint());
    assert!(requests[0].line.starts_with(&prefix), "{requests:?}");
    assert_eq!(requests[0].auth.as_deref(), Some("Basic dXNlcjpwYXNz"));

    // The lowercase name is also read.
    env.run_with_env(&["ls", "s3://bucket/"], &[("http_proxy", &url)])
        .success();
    assert_eq!(proxy.requests().len(), 2);

    // HTTPS_PROXY is not used for the HTTP endpoint, and NO_PROXY bypasses the
    // proxy.
    env.run_with_env(&["ls", "s3://bucket/"], &[("HTTPS_PROXY", &url)])
        .success();
    let envs = [("HTTP_PROXY", url.as_str()), ("NO_PROXY", "127.0.0.1")];
    env.run_with_env(&["ls", "s3://bucket/"], &envs).success();
    assert_eq!(proxy.requests().len(), 2);

    // The client of --no-verify-ssl tunnels the requests.
    env.run_with_env(
        &["ls", "s3://bucket/", "--no-verify-ssl"],
        &[("HTTP_PROXY", &url)],
    )
    .success();
    let requests = proxy.requests();
    let host = env.endpoint().trim_start_matches("http://");
    assert_eq!(requests[2].line, format!("CONNECT {host} HTTP/1.1"));
    assert_eq!(requests[2].auth.as_deref(), Some("Basic dXNlcjpwYXNz"));

    // The requests fail when the proxy is unreachable.
    let result = env.run_with_env(
        &["ls", "s3://bucket/"],
        &[("HTTP_PROXY", "http://127.0.0.1:9")],
    );
    assert_eq!(result.code, 255, "{}", result.stderr);
}

#[test]
fn proxy_tls() {
    let env = Env::new_tls();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    let proxy = env.proxy();
    let url = proxy.url(None);
    let host = env.endpoint().trim_start_matches("https://").to_owned();

    // The requests to the HTTPS endpoint are tunneled by HTTPS_PROXY, with or
    // without the verification of the certificate.
    let options: [&[&str]; 2] = [&["--ca-bundle", "ca.pem"], &["--no-verify-ssl"]];
    for (i, option) in options.iter().enumerate() {
        let args = [&["ls", "s3://bucket/"], *option].concat();
        let result = env.run_with_env(&args, &[("HTTPS_PROXY", &url)]).success();
        assert!(result.stdout.ends_with(" a.txt\n"), "{}", result.stdout);
        let requests = proxy.requests();
        assert_eq!(requests.len(), i + 1, "{requests:?}");
        assert_eq!(requests[i].line, format!("CONNECT {host} HTTP/1.1"));
        assert_eq!(requests[i].auth, None);
    }

    // HTTP_PROXY is not used for the HTTPS endpoint.
    env.run_with_env(
        &["ls", "s3://bucket/", "--no-verify-ssl"],
        &[("HTTP_PROXY", &url)],
    )
    .success();
    assert_eq!(proxy.requests().len(), 2);
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
