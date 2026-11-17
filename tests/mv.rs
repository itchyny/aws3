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
fn mv_validate_same_s3_paths() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "a");
    let access_point = "s3://arn:aws:s3:us-east-1:123456789012:accesspoint/my-ap";
    // The paths of the same key through an access point are warned about,
    // unless the underlying buckets are resolved.
    let dest = format!("{access_point}/a.txt");
    let result = env
        .run(&["mv", "s3://bucket/a.txt", &dest, "--dryrun"])
        .success();
    assert!(
        result
            .stderr
            .starts_with("warning: Provided s3 paths may resolve to same underlying"),
        "{}",
        result.stderr
    );
    for args in [
        &[
            "mv",
            "s3://bucket/a.txt",
            &format!("{access_point}/b.txt"),
            "--dryrun",
        ][..],
        &["cp", "s3://bucket/a.txt", &dest, "--dryrun"],
    ] {
        assert_eq!(env.run(args).success().stderr, "");
    }
    let alias = "s3://my-ap-abcdefghijklmnopqrstuvwxyz0123--op-s3/a.txt";
    let result = env.run(&["mv", "s3://bucket/a.txt", alias, "--validate-same-s3-paths"]);
    assert_eq!(result.code, 252);
    assert!(
        result
            .stderr
            .contains("Can't resolve underlying bucket name of s3 outposts access point alias."),
        "{}",
        result.stderr
    );
    assert!(env.object_content("bucket", "a.txt").is_some());
}
