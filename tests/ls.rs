// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::Env;

#[test]
fn ls_buckets() {
    let env = Env::new();
    env.bucket("bucket-a");
    env.bucket("bucket-b");
    let result = env.run(&["ls"]).success();
    // The fake server lists the buckets in the order of the file system.
    let mut buckets: Vec<_> = result.lines().iter().map(|line| &line[20..]).collect();
    buckets.sort();
    assert_eq!(buckets, ["bucket-a", "bucket-b"]);
}

#[test]
fn ls_objects() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    env.object("bucket", "dir/b.txt", "world!");
    env.object("bucket", "dir/sub/c.txt", "x");
    let result = env.run(&["ls", "s3://bucket"]).success();
    let lines = result.lines();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "                           PRE dir/");
    assert!(lines[1].ends_with("          5 a.txt"), "{}", lines[1]);

    let result = env.run(&["ls", "s3://bucket/dir/"]).success();
    assert_eq!(result.lines()[0], "                           PRE sub/");
    assert!(result.lines()[1].ends_with("          6 b.txt"));

    let result = env.run(&["ls", "bucket/dir"]).success();
    assert_eq!(result.lines(), ["                           PRE dir/"]);
}

#[test]
fn ls_url_encoded_keys() {
    let env = Env::new();
    env.bucket("bucket");
    for key in [
        "a+b c.txt",
        "100%25.txt",
        "ünï.txt",
        "ctrl\u{1}char.txt",
        "sub dir/x.txt",
    ] {
        env.object("bucket", &format!("dir/{key}"), "x");
    }
    let result = env.run(&["ls", "s3://bucket/dir/"]).success();
    let names: Vec<_> = result.lines().iter().map(|line| &line[31..]).collect();
    assert_eq!(
        names,
        [
            "sub dir/",
            "100%25.txt",
            "a+b c.txt",
            "ctrl\u{1}char.txt",
            "ünï.txt"
        ]
    );

    let result = env
        .run(&["ls", "s3://bucket/dir/sub dir/", "--recursive"])
        .success();
    assert!(result.lines()[0].ends_with(" dir/sub dir/x.txt"));
}

#[test]
fn ls_recursive_summarize() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    env.object("bucket", "dir/b.bin", vec![0; 1536]);
    let result = env
        .run(&[
            "ls",
            "s3://bucket",
            "--recursive",
            "--human-readable",
            "--summarize",
        ])
        .success();
    let lines = result.lines();
    assert!(lines[0].ends_with("    5 Bytes a.txt"), "{}", lines[0]);
    assert!(lines[1].ends_with("    1.5 KiB dir/b.bin"), "{}", lines[1]);
    assert_eq!(
        &lines[2..],
        ["", "Total Objects: 2", "   Total Size: 1.5 KiB"]
    );
}

#[test]
fn ls_no_match() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "hello");
    let result = env.run(&["ls", "s3://bucket/nothing"]);
    assert_eq!(result.code, 1);
    assert_eq!(result.stdout, "");
    // Listing a bucket without a key succeeds even if it is empty.
    env.bucket("empty");
    env.run(&["ls", "s3://empty"]).success();
}

#[test]
fn ls_no_such_bucket() {
    let env = Env::new();
    let result = env.run(&["ls", "s3://no-such-bucket"]);
    assert_eq!(result.code, 254);
    assert!(
        result
            .stderr
            .contains("An error occurred (NoSuchBucket) when calling the ListObjectsV2 operation"),
        "{}",
        result.stderr
    );
}

#[test]
fn ls_options() {
    let env = Env::new();
    env.bucket("bucket-a");
    for i in 0..5 {
        env.object("bucket-a", &format!("dir/{i}.txt"), "x");
    }
    let result = env
        .run(&[
            "ls",
            "s3://bucket-a/dir/",
            "--page-size",
            "2",
            "--request-payer",
        ])
        .success();
    assert_eq!(result.lines().len(), 5);
    // The bucket name prefix is not tested, since the fake server ignores it.
    env.run(&["ls", "--bucket-name-prefix", "bucket-"])
        .success();
}
