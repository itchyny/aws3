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
