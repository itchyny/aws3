// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::{Env, content};

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
    let config =
        env.config("[default]\ns3 =\n  multipart_threshold = 5MB\n  multipart_chunksize = 5MB\n");
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
