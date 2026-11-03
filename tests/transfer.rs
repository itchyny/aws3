// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::Env;

#[test]
fn mv_upload_and_download() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "hello");
    let result = env
        .run(&["mv", "a.txt", "s3://bucket/", "--no-progress"])
        .success();
    assert_eq!(result.lines(), ["move: ./a.txt to s3://bucket/a.txt"]);
    assert!(!env.path("a.txt").exists());
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"hello");

    let result = env
        .run(&["mv", "s3://bucket/a.txt", "b.txt", "--no-progress"])
        .success();
    assert_eq!(result.lines(), ["move: s3://bucket/a.txt to ./b.txt"]);
    assert_eq!(env.read("b.txt"), b"hello");
    assert_eq!(env.object_content("bucket", "a.txt"), None);
}

#[test]
fn mv_onto_itself() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    let result = env.run(&["mv", "s3://bucket/a.txt", "s3://bucket/"]);
    assert_eq!(result.code, 252);
    assert!(
        result
            .stderr
            .contains("Cannot mv a file onto itself: s3://bucket/a.txt - s3://bucket/")
    );
}

#[test]
fn rm_recursive() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "dir/a.txt", "a");
    env.object("bucket", "dir/b.bin", "b");
    env.object("bucket", "other.txt", "c");
    let result = env
        .run(&["rm", "s3://bucket/dir", "--recursive", "--exclude", "*.bin"])
        .success();
    assert_eq!(result.lines(), ["delete: s3://bucket/dir/a.txt"]);
    assert_eq!(env.object_content("bucket", "dir/a.txt"), None);
    assert!(env.object_content("bucket", "dir/b.bin").is_some());

    let result = env
        .run(&["rm", "s3://bucket/other.txt", "--dryrun"])
        .success();
    assert_eq!(result.lines(), ["(dryrun) delete: s3://bucket/other.txt"]);
    assert!(env.object_content("bucket", "other.txt").is_some());

    let result = env.run(&["rm", "a.txt"]);
    assert_eq!(result.code, 252);
}

#[test]
fn region_redirect() {
    let env = Env::new();
    env.bucket("far");
    env.bucket_region("far", "ap-northeast-1");
    env.object("far", "src.txt", "source");
    env.write("dir/a.txt", "a");
    env.write("dir/b.txt", "b");
    let config = env.multipart_config();
    let envs = [(config[0].0, config[0].1.as_str())];
    env.write("big.bin", common::content(6 * 1024 * 1024));
    // Returns the number of the redirected requests of the run.
    let redirects = |args: &[&str]| {
        let start = env.requests().len();
        env.run_with_env(args, &envs).success();
        env.requests()[start..]
            .iter()
            .filter(|request| request.signed_region() != "ap-northeast-1")
            .count()
    };
    // The requests after the first redirect are sent to the region of the
    // bucket, but the concurrent requests before it are redirected as well.
    assert_eq!(redirects(&["ls", "s3://far"]), 1);
    assert!(redirects(&["cp", "dir", "s3://far/dir", "--recursive"]) <= 2);
    assert_eq!(redirects(&["cp", "big.bin", "s3://far/big.bin"]), 1);
    assert_eq!(redirects(&["cp", "s3://far/src.txt", "out.txt"]), 1);
    assert_eq!(
        redirects(&["cp", "s3://far/src.txt", "s3://far/copy.txt"]),
        1
    );
    assert_eq!(redirects(&["sync", "s3://far/dir", "synced"]), 1);
    assert_eq!(redirects(&["rm", "s3://far/dir", "--recursive"]), 1);
    assert_eq!(env.read("out.txt"), b"source");
    assert_eq!(env.read("synced/a.txt"), b"a");
    assert_eq!(env.object_content("far", "copy.txt").unwrap(), b"source");
    assert_eq!(env.object_content("far", "dir/a.txt"), None);
    assert_eq!(
        env.object_content("far", "big.bin").unwrap().len(),
        6 * 1024 * 1024
    );
}

#[test]
fn rm_url_encoded_keys() {
    let env = Env::new();
    env.bucket("bucket");
    for key in ["a+b c.txt", "100%25.txt", "ctrl\u{1}char.txt"] {
        env.object("bucket", key, "x");
    }
    let result = env.run(&["rm", "s3://bucket", "--recursive"]).success();
    let mut lines = result.lines();
    lines.sort();
    assert_eq!(
        lines,
        [
            "delete: s3://bucket/100%25.txt",
            "delete: s3://bucket/a+b c.txt",
            "delete: s3://bucket/ctrl\u{1}char.txt",
        ]
    );
    assert_eq!(env.object_content("bucket", "a+b c.txt"), None);
}

#[test]
fn sync_upload() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("src/a.txt", "a");
    env.write("src/sub/b.txt", "b");
    let result = env
        .run(&["sync", "src", "s3://bucket/dir", "--no-progress"])
        .success();
    assert_eq!(
        result.sorted_lines(),
        [
            "upload: src/a.txt to s3://bucket/dir/a.txt",
            "upload: src/sub/b.txt to s3://bucket/dir/sub/b.txt",
        ]
    );
    // Nothing is synced when the destination is up to date.
    let result = env
        .run(&["sync", "src", "s3://bucket/dir", "--no-progress"])
        .success();
    assert_eq!(result.stdout, "");

    // Changed sizes are synced, and deleted files are deleted with --delete.
    env.write("src/a.txt", "changed");
    std::fs::remove_file(env.path("src/sub/b.txt")).unwrap();
    let result = env
        .run(&[
            "sync",
            "src",
            "s3://bucket/dir",
            "--delete",
            "--no-progress",
        ])
        .success();
    assert_eq!(
        result.sorted_lines(),
        [
            "delete: s3://bucket/dir/sub/b.txt",
            "upload: src/a.txt to s3://bucket/dir/a.txt",
        ]
    );
    assert_eq!(
        env.object_content("bucket", "dir/a.txt").unwrap(),
        b"changed"
    );
    assert_eq!(env.object_content("bucket", "dir/sub/b.txt"), None);
}

#[test]
fn sync_download() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "dir/a.txt", "a");
    env.object("bucket", "dir/b.txt", "b");
    let result = env
        .run(&["sync", "s3://bucket/dir", "out", "--no-progress"])
        .success();
    assert_eq!(
        result.sorted_lines(),
        [
            "download: s3://bucket/dir/a.txt to out/a.txt",
            "download: s3://bucket/dir/b.txt to out/b.txt",
        ]
    );
    // The downloaded files have the modified times of the objects.
    let result = env
        .run(&["sync", "s3://bucket/dir", "out", "--no-progress"])
        .success();
    assert_eq!(result.stdout, "");

    env.write("out/extra.txt", "x");
    let result = env
        .run(&[
            "sync",
            "s3://bucket/dir",
            "out",
            "--delete",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.lines(), ["delete: out/extra.txt"]);
    assert!(!env.path("out/extra.txt").exists());
}

#[test]
fn sync_size_only_and_no_overwrite() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("src/a.txt", "aaa");
    env.run(&["sync", "src", "s3://bucket/", "--no-progress"])
        .success();
    env.write("src/a.txt", "bbb");
    let result = env
        .run(&[
            "sync",
            "src",
            "s3://bucket/",
            "--size-only",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.stdout, "");
    env.write("src/a.txt", "cccc");
    let result = env
        .run(&[
            "sync",
            "src",
            "s3://bucket/",
            "--no-overwrite",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.stdout, "");
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"aaa");
}

#[test]
fn sync_s3_to_s3() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "src/a.txt", "a");
    env.object("bucket", "src/sub/b.txt", "b");
    env.object("bucket", "dest/stale.txt", "x");
    let result = env
        .run(&[
            "sync",
            "s3://bucket/src",
            "s3://bucket/dest",
            "--delete",
            "--no-progress",
        ])
        .success();
    assert_eq!(
        result.sorted_lines(),
        [
            "copy: s3://bucket/src/a.txt to s3://bucket/dest/a.txt",
            "copy: s3://bucket/src/sub/b.txt to s3://bucket/dest/sub/b.txt",
            "delete: s3://bucket/dest/stale.txt",
        ]
    );
    assert_eq!(
        env.object_content("bucket", "dest/sub/b.txt").unwrap(),
        b"b"
    );
    assert_eq!(env.object_content("bucket", "dest/stale.txt"), None);

    let result = env
        .run(&[
            "sync",
            "s3://bucket/src",
            "s3://bucket/dry",
            "--dryrun",
            "--exclude",
            "sub/*",
        ])
        .success();
    assert_eq!(
        result.lines(),
        ["(dryrun) copy: s3://bucket/src/a.txt to s3://bucket/dry/a.txt"]
    );
}

#[test]
fn sync_exact_timestamps() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "dir/a.txt", "a");
    env.run(&["sync", "s3://bucket/dir", "out", "--no-progress"])
        .success();
    // The local file of the same size but an older time is not synced by the
    // default, but synced with --exact-timestamps.
    let file = std::fs::File::options()
        .write(true)
        .open(env.path("out/a.txt"))
        .unwrap();
    let older = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    file.set_modified(older).unwrap();
    let result = env
        .run(&["sync", "s3://bucket/dir", "out", "--no-progress"])
        .success();
    assert_eq!(result.stdout, "");
    let result = env
        .run(&[
            "sync",
            "s3://bucket/dir",
            "out",
            "--exact-timestamps",
            "--no-progress",
        ])
        .success();
    assert_eq!(
        result.lines(),
        ["download: s3://bucket/dir/a.txt to out/a.txt"]
    );
}

#[test]
fn sync_case_conflict() {
    let env = Env::new();
    // Case conflicts are detected only on case-insensitive file systems.
    env.write("probe", "");
    if !env.path("PROBE").exists() {
        return;
    }
    env.bucket("bucket");
    env.object("bucket", "dir/A.txt", "upper");
    env.write("out/a.txt", "lower");
    let result = env
        .run(&[
            "sync",
            "s3://bucket/dir",
            "out",
            "--case-conflict",
            "skip",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.stdout, "");
    assert!(
        result
            .stderr
            .starts_with("warning: Skipping bucket/dir/A.txt -> "),
        "{}",
        result.stderr
    );
    let result = env.run(&[
        "cp",
        "s3://bucket/dir",
        "out",
        "--recursive",
        "--case-conflict",
        "error",
    ]);
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .starts_with("fatal error: Failed to download bucket/dir/A.txt -> "),
        "{}",
        result.stderr
    );
    let result = env
        .run(&[
            "cp",
            "s3://bucket/dir",
            "out",
            "--recursive",
            "--case-conflict",
            "warn",
            "--no-progress",
        ])
        .success();
    assert!(
        result
            .stderr
            .starts_with("warning: Downloading bucket/dir/A.txt -> "),
        "{}",
        result.stderr
    );
}

#[test]
fn rm_output_modes() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "a");
    env.object("bucket", "b.txt", "b");
    let result = env.run(&["rm", "s3://bucket/a.txt", "--quiet"]).success();
    assert_eq!(result.stdout, "");
    let result = env
        .run(&["rm", "s3://bucket/", "--recursive", "--only-show-errors"])
        .success();
    assert_eq!(result.stdout, "");
    assert_eq!(env.object_content("bucket", "b.txt"), None);
    let result = env.run(&["rm", "s3://no-such-bucket/", "--recursive"]);
    assert_eq!(result.code, 1);
    assert!(result.stderr.starts_with(
        "fatal error: An error occurred (NoSuchBucket) when calling the ListObjectsV2 operation"
    ));
}
