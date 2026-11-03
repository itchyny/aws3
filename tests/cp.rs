// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::{Env, content, wait_child, wait_until};

#[test]
fn cp_upload() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "hello");
    let result = env
        .run(&["cp", "a.txt", "s3://bucket/", "--no-progress"])
        .success();
    assert_eq!(result.lines(), ["upload: ./a.txt to s3://bucket/a.txt"]);
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"hello");

    // The destination without a trailing slash is the key.
    env.run(&["cp", "a.txt", "s3://bucket/x/y.txt", "--no-progress"])
        .success();
    assert_eq!(env.object_content("bucket", "x/y.txt").unwrap(), b"hello");

    // The bucket without a trailing slash takes the name of the source.
    env.run(&["cp", "a.txt", "s3://bucket", "--no-progress"])
        .success();
}

#[test]
fn cp_upload_recursive_filters() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("src/a.txt", "a");
    env.write("src/b.json", "b");
    env.write("src/sub/c.json", "c");
    env.write("src/sub/d.log", "d");
    let result = env
        .run(&[
            "cp",
            "src",
            "s3://bucket/dir",
            "--recursive",
            "--no-progress",
            "--exclude",
            "*",
            "--include",
            "*.json",
            "--exclude",
            "sub/*",
        ])
        .success();
    assert_eq!(
        result.lines(),
        ["upload: src/b.json to s3://bucket/dir/b.json"]
    );

    let result = env
        .run(&[
            "cp",
            "src",
            "s3://bucket/all",
            "--recursive",
            "--no-progress",
        ])
        .success();
    assert_eq!(
        result.sorted_lines(),
        [
            "upload: src/a.txt to s3://bucket/all/a.txt",
            "upload: src/b.json to s3://bucket/all/b.json",
            "upload: src/sub/c.json to s3://bucket/all/sub/c.json",
            "upload: src/sub/d.log to s3://bucket/all/sub/d.log",
        ]
    );
    assert_eq!(env.object_content("bucket", "all/sub/d.log").unwrap(), b"d");
}

#[test]
fn cp_download() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    env.object("bucket", "dir/b.txt", "b");
    env.object("bucket", "dir/sub/c.txt", "c");
    let result = env
        .run(&["cp", "s3://bucket/a.txt", "out/", "--no-progress"])
        .success();
    assert_eq!(result.lines(), ["download: s3://bucket/a.txt to out/a.txt"]);
    assert_eq!(env.read("out/a.txt"), b"hello");

    env.run(&[
        "cp",
        "s3://bucket/a.txt",
        "out/renamed.txt",
        "--no-progress",
    ])
    .success();
    assert_eq!(env.read("out/renamed.txt"), b"hello");

    let result = env
        .run(&[
            "cp",
            "s3://bucket/dir",
            "out/dir",
            "--recursive",
            "--no-progress",
        ])
        .success();
    assert_eq!(
        result.sorted_lines(),
        [
            "download: s3://bucket/dir/b.txt to out/dir/b.txt",
            "download: s3://bucket/dir/sub/c.txt to out/dir/sub/c.txt",
        ]
    );
    assert_eq!(env.read("out/dir/sub/c.txt"), b"c");
}

#[test]
fn cp_copy() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    let result = env
        .run(&[
            "cp",
            "s3://bucket/a.txt",
            "s3://bucket/copy.txt",
            "--no-progress",
        ])
        .success();
    assert_eq!(
        result.lines(),
        ["copy: s3://bucket/a.txt to s3://bucket/copy.txt"]
    );
    assert_eq!(env.object_content("bucket", "copy.txt").unwrap(), b"hello");
}

#[test]
fn cp_multipart() {
    let env = Env::new();
    env.bucket("bucket");
    let data = content(12 * 1024 * 1024 + 123);
    env.write("big.bin", &data);
    let config = env.config(
        r"[default]
s3 =
  multipart_threshold = 5MB
  multipart_chunksize = 5MB
",
    );
    let run = |args: &[&str]| {
        let mut command = env.command(args);
        command.env("AWS_CONFIG_FILE", &config);
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["cp", "big.bin", "s3://bucket/big.bin", "--no-progress"]);
    assert_eq!(env.object_content("bucket", "big.bin").unwrap(), data);
    run(&["cp", "s3://bucket/big.bin", "out/big.bin", "--no-progress"]);
    assert_eq!(env.read("out/big.bin"), data);
}

#[test]
fn cp_stream() {
    let env = Env::new();
    env.bucket("bucket");
    let result = env.run_with_stdin(&["cp", "-", "s3://bucket/stdin.txt"], b"piped");
    assert_eq!((result.code, result.stdout.as_str()), (0, ""));
    assert_eq!(env.object_content("bucket", "stdin.txt").unwrap(), b"piped");

    let result = env.run(&["cp", "s3://bucket/stdin.txt", "-"]).success();
    assert_eq!(result.stdout_bytes, b"piped");

    let result = env.run(&["cp", "s3://bucket/stdin.txt", "-", "--recursive"]);
    assert_eq!(result.code, 252);
    assert!(
        result
            .stderr
            .contains("Streaming currently is only compatible with non-recursive cp commands")
    );
}

#[test]
fn cp_dryrun() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("src/a.txt", "a");
    let result = env
        .run(&["cp", "src", "s3://bucket/", "--recursive", "--dryrun"])
        .success();
    assert_eq!(
        result.lines(),
        ["(dryrun) upload: src/a.txt to s3://bucket/a.txt"]
    );
    assert_eq!(env.object_content("bucket", "a.txt"), None);
}

#[test]
fn cp_progress() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "hello");
    let result = env.run(&["cp", "a.txt", "s3://bucket/a.txt"]).success();
    let (progress, success) = result.stdout.split_once('\r').unwrap();
    assert!(
        progress.starts_with("Completed 5 Bytes/5 Bytes ("),
        "{progress}"
    );
    assert!(
        progress.ends_with(") with 1 file(s) remaining"),
        "{progress}"
    );
    assert_eq!(success.trim_end(), "upload: ./a.txt to s3://bucket/a.txt");
    assert_eq!(success.len(), progress.len() + 1);
}

#[test]
fn cp_errors() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "hello");

    let result = env.run(&["cp", "a.txt", "b.txt"]);
    assert_eq!(result.code, 252);
    assert!(
        result.stderr.contains("Error: Invalid argument type"),
        "{}",
        result.stderr
    );

    let result = env.run(&["cp", "missing.txt", "s3://bucket/"]);
    assert_eq!(result.code, 255);
    assert!(
        result
            .stderr
            .contains("The user-provided path missing.txt does not exist.")
    );

    let result = env.run(&["cp", "s3://bucket/missing.txt", "out/"]);
    assert_eq!(result.code, 1);
    assert_eq!(
        result.stderr,
        "fatal error: An error occurred (404) when calling the HeadObject operation: Key \"missing.txt\" does not exist\n"
    );

    // Uploads to missing buckets are not tested, since the fake server
    // creates the directories of the buckets.

    let result = env.run(&["cp", "a.txt", "s3://bucket/", "--grants", "foo"]);
    assert_eq!(result.code, 1);
    assert_eq!(
        result.stderr,
        "fatal error: grants should be of the form permission=principal\n"
    );

    let result = env.run(&["cp", "a.txt", "s3://bucket/", "--sse-c"]);
    assert_eq!(result.code, 252);
    assert!(
        result
            .stderr
            .contains("If --sse-c is specified, --sse-c-key must be specified as well.")
    );
}

#[test]
fn cp_no_overwrite_download() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "remote");
    env.write("a.txt", "local");
    let result = env
        .run(&[
            "cp",
            "s3://bucket/a.txt",
            "a.txt",
            "--no-overwrite",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.stdout, "");
    assert_eq!(env.read("a.txt"), b"local");
}

#[test]
fn cp_content_type_and_metadata() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.json", "{}");
    env.write("a.unknownext", "x");
    env.run(&["cp", "a.json", "s3://bucket/a.json", "--no-progress"])
        .success();
    assert_eq!(
        env.head("bucket", "a.json").content_type(),
        Some("application/json")
    );

    env.run(&[
        "cp",
        "a.json",
        "s3://bucket/b.json",
        "--no-guess-mime-type",
        "--no-progress",
    ])
    .success();
    // No content type is sent, which S3 defaults to binary/octet-stream.
    assert_eq!(env.head("bucket", "b.json").content_type(), None);

    env.run(&[
        "cp",
        "a.unknownext",
        "s3://bucket/m.txt",
        "--no-progress",
        "--content-type",
        "text/x-custom",
        "--metadata",
        "k1=v1,k2='a,b'",
    ])
    .success();
    let head = env.head("bucket", "m.txt");
    assert_eq!(head.content_type(), Some("text/x-custom"));
    let metadata = head.metadata().unwrap();
    assert_eq!(metadata["k1"], "v1");
    assert_eq!(metadata["k2"], "a,b");

    env.run(&[
        "cp",
        "a.json",
        "s3://bucket/j.txt",
        "--metadata",
        r#"{"json":"yes"}"#,
        "--no-progress",
    ])
    .success();
    assert_eq!(
        env.head("bucket", "j.txt").metadata().unwrap()["json"],
        "yes"
    );
}

#[test]
fn cp_copy_props() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "hello");
    env.run(&[
        "cp",
        "a.txt",
        "s3://bucket/src.txt",
        "--no-progress",
        "--content-type",
        "text/x-src",
        "--metadata",
        "k=v",
    ])
    .success();

    // The metadata is copied by the default.
    env.run(&[
        "cp",
        "s3://bucket/src.txt",
        "s3://bucket/default.txt",
        "--no-progress",
    ])
    .success();
    let head = env.head("bucket", "default.txt");
    assert_eq!(head.content_type(), Some("text/x-src"));
    assert_eq!(head.metadata().unwrap()["k"], "v");

    // The metadata replaces the source, keeping the other properties.
    env.run(&[
        "cp",
        "s3://bucket/src.txt",
        "s3://bucket/replaced.txt",
        "--no-progress",
        "--metadata",
        "x=y",
    ])
    .success();
    let head = env.head("bucket", "replaced.txt");
    assert_eq!(head.content_type(), Some("text/x-src"));
    assert_eq!(head.metadata().unwrap().get("k"), None);
    assert_eq!(head.metadata().unwrap()["x"], "y");

    // Nothing is copied with --copy-props none.
    env.run(&[
        "cp",
        "s3://bucket/src.txt",
        "s3://bucket/none.txt",
        "--no-progress",
        "--copy-props",
        "none",
    ])
    .success();
    let head = env.head("bucket", "none.txt");
    assert_ne!(head.content_type(), Some("text/x-src"));
    assert!(head.metadata().is_none_or(|metadata| metadata.is_empty()));

    let result = env.run(&[
        "cp",
        "s3://bucket/src.txt",
        "s3://bucket/directive.txt",
        "--no-progress",
        "--metadata-directive",
        "COPY",
    ]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(
        env.head("bucket", "directive.txt").content_type(),
        Some("text/x-src")
    );
}

#[test]
fn cp_multipart_copy_and_stream() {
    let env = Env::new();
    env.bucket("bucket");
    let config = env.multipart_config();
    let envs: Vec<_> = config
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    let data = content(11 * 1024 * 1024);
    env.write("big.bin", &data);
    env.run_with_env(
        &[
            "cp",
            "big.bin",
            "s3://bucket/big.bin",
            "--no-progress",
            "--content-type",
            "application/x-big",
            "--metadata",
            "k=v",
        ],
        &envs,
    )
    .success();

    // The multipart copy takes the properties from the source object, and
    // the tags are not copied since the fake server does not support them.
    env.run_with_env(
        &[
            "cp",
            "s3://bucket/big.bin",
            "s3://bucket/copy.bin",
            "--no-progress",
            "--copy-props",
            "metadata-directive",
        ],
        &envs,
    )
    .success();
    assert_eq!(env.object_content("bucket", "copy.bin").unwrap(), data);
    let head = env.head("bucket", "copy.bin");
    assert_eq!(head.content_type(), Some("application/x-big"));
    assert_eq!(head.metadata().unwrap()["k"], "v");

    env.run_with_env(
        &[
            "cp",
            "s3://bucket/big.bin",
            "s3://bucket/none.bin",
            "--no-progress",
            "--copy-props",
            "none",
        ],
        &envs,
    )
    .success();
    assert_eq!(env.object_content("bucket", "none.bin").unwrap(), data);

    let mut command = env.command(&[
        "cp",
        "-",
        "s3://bucket/stdin.bin",
        "--expected-size",
        "11534336",
    ]);
    command.envs(envs.iter().copied());
    let mut child = command.stdin(std::process::Stdio::piped()).spawn().unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), &data).unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(env.object_content("bucket", "stdin.bin").unwrap(), data);

    let result = env
        .run_with_env(&["cp", "s3://bucket/stdin.bin", "-"], &envs)
        .success();
    assert_eq!(result.stdout_bytes, data);
}

#[test]
fn cp_multipart_etag() {
    let env = Env::new();
    env.bucket("bucket");
    let config = env.multipart_config();
    let envs: Vec<_> = config
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    let data = content(11 * 1024 * 1024);
    env.object("bucket", "big.bin", &data);
    let etag = env.head("bucket", "big.bin").e_tag().unwrap().to_owned();

    // The ranged downloads and the part copies are requested with the ETag,
    // so that the object modified during the transfer fails.
    let cases: [(&[&str], &str); 3] = [
        (&["cp", "s3://bucket/big.bin", "big.bin"], "if-match"),
        (&["cp", "s3://bucket/", "dir/", "--recursive"], "if-match"),
        (
            &[
                "cp",
                "s3://bucket/big.bin",
                "s3://bucket/copy.bin",
                "--copy-props",
                "metadata-directive",
            ],
            "x-amz-copy-source-if-match",
        ),
    ];
    for (args, header) in cases {
        let start = env.requests().len();
        env.run_with_env(args, &envs).success();
        let pinned: Vec<_> = env.requests()[start..]
            .iter()
            .filter(|request| {
                request.headers.contains_key("range")
                    || request.headers.contains_key("x-amz-copy-source-range")
            })
            .map(|request| request.headers[header].to_str().unwrap().to_owned())
            .collect();
        assert_eq!(pinned, [etag.as_str(); 3], "{args:?}");
    }
    assert_eq!(env.read("big.bin"), data);
    assert_eq!(env.read("dir/big.bin"), data);
    assert_eq!(env.object_content("bucket", "copy.bin").unwrap(), data);
}

#[test]
fn cp_request_checksum_calculation() {
    let env = Env::new();
    env.bucket("bucket");
    let config = env.multipart_config();
    let config = config[0].1.as_str();
    env.write("small.txt", "small");
    env.write("big.bin", content(6 * 1024 * 1024));
    // Returns whether the uploads of the run sent the checksums.
    let checksums = |envs: &[(&str, &str)]| {
        let start = env.requests().len();
        for file in ["small.txt", "big.bin"] {
            let mut envs = envs.to_vec();
            envs.push(("AWS_CONFIG_FILE", config));
            env.run_with_env(&["cp", file, &format!("s3://bucket/{file}")], &envs)
                .success();
        }
        env.requests()[start..]
            .iter()
            .filter(|request| {
                request.method == "PUT"
                    || request.method == "POST"
                        && request
                            .uri
                            .query()
                            .is_some_and(|query| query.starts_with("uploads"))
            })
            .map(|request| {
                request.headers.iter().any(|(name, value)| {
                    name.as_str().starts_with("x-amz-checksum-")
                        || name == "x-amz-sdk-checksum-algorithm"
                        || name == "x-amz-trailer"
                            && value.to_str().unwrap().starts_with("x-amz-checksum-")
                })
            })
            .collect::<Vec<_>>()
    };
    // The single upload, and the creation and the two parts of the multipart
    // upload, except for the completion.
    assert_eq!(checksums(&[]), [true; 4]);
    // The algorithm defaults to CRC64NVME like the AWS CLI.
    let algorithms: Vec<_> = env
        .requests()
        .iter()
        .filter_map(|request| {
            let headers = &request.headers;
            headers
                .get("x-amz-sdk-checksum-algorithm")
                .or_else(|| headers.get("x-amz-checksum-algorithm"))
        })
        .map(|value| value.to_str().unwrap().to_owned())
        .collect();
    assert_eq!(algorithms, ["CRC64NVME"; 4]);
    assert_eq!(
        checksums(&[("AWS_REQUEST_CHECKSUM_CALCULATION", "when_required")]),
        [false; 4]
    );
}

#[test]
fn cp_no_overwrite_upload() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "remote");
    env.write("a.txt", "local");
    let result = env
        .run(&[
            "cp",
            "a.txt",
            "s3://bucket/a.txt",
            "--no-overwrite",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.stdout, "");
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"remote");
}

#[test]
fn cp_output_modes() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("src/a.txt", "a");
    env.write("src/b.txt", "b");
    for flag in ["--quiet", "--only-show-errors"] {
        let result = env
            .run(&["cp", "src", "s3://bucket/", "--recursive", flag])
            .success();
        assert_eq!((result.stdout.as_str(), result.stderr.as_str()), ("", ""));
    }
    let result = env
        .run(&[
            "cp",
            "src",
            "s3://bucket/",
            "--recursive",
            "--progress-multiline",
        ])
        .success();
    assert!(result.stdout.contains("Completed "), "{}", result.stdout);
    assert!(!result.stdout.contains('\r'), "{}", result.stdout);
    let result = env
        .run(&[
            "cp",
            "src",
            "s3://bucket/",
            "--recursive",
            "--progress-frequency",
            "60",
        ])
        .success();
    assert_eq!(
        result.stdout.matches("Completed ").count(),
        1,
        "{}",
        result.stdout
    );

    // Errors are printed with --only-show-errors, but not with --quiet.
    let result = env.run(&["cp", "s3://bucket/missing", "out/", "--only-show-errors"]);
    assert_eq!(result.code, 1);
    assert!(
        result.stderr.starts_with("fatal error: "),
        "{}",
        result.stderr
    );
    let result = env.run(&["cp", "s3://bucket/missing", "out/", "--quiet"]);
    assert_eq!((result.code, result.stderr.as_str()), (1, ""));
}

#[test]
fn cp_warnings() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("src/a.txt", "a");
    std::os::unix::fs::symlink("missing.txt", env.path("src/broken.txt")).unwrap();
    let result = env.run(&["cp", "src", "s3://bucket/", "--recursive", "--no-progress"]);
    assert_eq!(result.code, 2);
    assert_eq!(result.lines(), ["upload: src/a.txt to s3://bucket/a.txt"]);
    let warning = format!(
        "warning: Skipping file {}. File does not exist.\n",
        std::fs::canonicalize(env.work())
            .unwrap()
            .join("src/broken.txt")
            .display()
    );
    assert_eq!(
        result.stderr.replace("/private/var/", "/var/"),
        warning.replace("/private/var/", "/var/")
    );

    // Symbolic links are not followed with --no-follow-symlinks.
    let result = env
        .run(&[
            "cp",
            "src",
            "s3://bucket/x/",
            "--recursive",
            "--no-progress",
            "--no-follow-symlinks",
        ])
        .success();
    assert_eq!(result.lines(), ["upload: src/a.txt to s3://bucket/x/a.txt"]);
}

#[test]
fn cp_invalid_paths_type() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "a");
    let result = env.run(&["cp", "a.txt", "s3://bucket/", "--checksum-mode", "ENABLED"]);
    assert_eq!(result.code, 252);
    assert!(result.stderr.contains(
        "Expected checksum-mode parameter to be used with one of following path formats: <S3Uri> <LocalPath>. Instead, received <LocalPath> <S3Uri>."
    ));
    let result = env.run(&[
        "cp",
        "a.txt",
        "s3://bucket/",
        "--sse-c-copy-source",
        "--sse-c-copy-source-key",
        "a2V5",
    ]);
    assert_eq!(result.code, 252);
    assert!(
        result
            .stderr
            .contains("--sse-c-copy-source is only supported for copy operations.")
    );
    let result = env.run(&["cp", "a.txt", "s3://bucket/", "--sse-c-key", "a2V5"]);
    assert_eq!(result.code, 252);
    assert!(
        result
            .stderr
            .contains("If --sse-c-key is specified, --sse-c must be specified as well.")
    );
}

#[test]
fn cp_ctrl_c() {
    use std::io::Write;
    let env = Env::new();
    env.bucket("bucket");
    let config = env.multipart_config();
    let envs: Vec<_> = config
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    let mut child = env
        .command(&["cp", "-", "s3://bucket/stdin.bin"])
        .envs(envs.iter().copied())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // The multipart upload is in progress while the standard input is open,
    // and Ctrl-C is handled since it is created.
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&content(6 * 1024 * 1024)).unwrap();
    wait_until("the multipart upload", || env.multipart_uploads() == 1);
    let status = std::process::Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let output = wait_child(child);
    drop(stdin);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim_end(),
        "cancelled: ctrl-c received"
    );
    assert_eq!(env.object_content("bucket", "stdin.bin"), None);
    // The cancelled multipart upload is aborted.
    assert_eq!(env.multipart_uploads(), 0);
}

#[test]
fn cp_max_bandwidth() {
    let env = Env::new();
    env.bucket("bucket");
    let data = content(640 * 1024);
    env.write("a.bin", &data);
    let config = env.config(
        r"[default]
s3 =
  max_bandwidth = 1MB/s
",
    );
    let config = config.to_string_lossy().into_owned();
    let envs = [("AWS_CONFIG_FILE", config.as_str())];
    // The data of 640 KiB is transferred in three chunks of 256 KiB at most,
    // where the third chunk waits for the bandwidth of the first two chunks.
    for args in [
        ["cp", "a.bin", "s3://bucket/a.bin", "--no-progress"],
        ["cp", "s3://bucket/a.bin", "b.bin", "--no-progress"],
    ] {
        let start = std::time::Instant::now();
        env.run_with_env(&args, &envs).success();
        assert!(
            start.elapsed() >= std::time::Duration::from_millis(450),
            "{args:?}"
        );
    }
    assert_eq!(env.object_content("bucket", "a.bin").unwrap(), data);
    assert_eq!(env.read("b.bin"), data);

    let config = env.config(
        r"[default]
s3 =
  max_bandwidth = fast
",
    );
    let config = config.to_string_lossy().into_owned();
    let result = env.run_with_env(
        &["cp", "a.bin", "s3://bucket/c.bin"],
        &[("AWS_CONFIG_FILE", &config)],
    );
    assert_eq!(result.code, 255);
    assert!(
        result.stderr.contains("Invalid rate: fast."),
        "{}",
        result.stderr
    );
}

#[test]
fn cp_invalid_transfer_settings() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "a");
    let config = env.config(
        r"[default]
s3 =
  multipart_chunksize = 0
",
    );
    let envs = [("AWS_CONFIG_FILE", config.to_str().unwrap())];
    let result = env.run_with_env(&["cp", "a.txt", "s3://bucket/a.txt"], &envs);
    assert_eq!(result.code, 255);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: Value for multipart_chunksize must be a positive integer: 0\n"
    );
    assert_eq!(env.object_content("bucket", "a.txt"), None);

    // The commands other than the transfer commands do not validate them.
    env.run_with_env(&["ls", "s3://bucket"], &envs).success();
}
