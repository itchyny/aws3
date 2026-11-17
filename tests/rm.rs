// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::Env;

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
