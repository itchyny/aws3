// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::Env;

#[test]
fn mb_and_rb() {
    let env = Env::new();
    let result = env.run(&["mb", "s3://new-bucket"]).success();
    assert_eq!(result.stdout, "make_bucket: new-bucket\n");

    let result = env.run(&["mb", "new-bucket"]);
    assert_eq!(result.code, 252);
    assert!(
        result
            .stderr
            .contains("<S3Uri>\nError: Invalid argument type")
    );

    let result = env.run(&["rb", "s3://new-bucket/key"]);
    assert_eq!(result.code, 252);
    assert!(
        result
            .stderr
            .contains("Please specify a valid bucket name only. E.g. s3://new-bucket")
    );

    let result = env.run(&["rb", "s3://new-bucket"]).success();
    assert_eq!(result.stdout, "remove_bucket: new-bucket\n");

    let result = env.run(&["rb", "s3://new-bucket"]);
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .starts_with("remove_bucket failed: s3://new-bucket An error occurred (NoSuchBucket)")
    );
}

#[test]
fn rb_force() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "a");
    env.object("bucket", "dir/b.txt", "b");
    let result = env.run(&["rb", "s3://bucket", "--force"]).success();
    assert_eq!(
        result.sorted_lines(),
        [
            "delete: s3://bucket/a.txt",
            "delete: s3://bucket/dir/b.txt",
            "remove_bucket: bucket",
        ]
    );
}

#[test]
fn presign() {
    let env = Env::new();
    env.bucket("bucket");
    let result = env
        .run(&["presign", "s3://bucket/a b.txt", "--expires-in", "60"])
        .success();
    let url = result.stdout.trim_end();
    assert!(url.contains("/bucket/a%20b.txt?"), "{url}");
    assert!(url.contains("X-Amz-Expires=60"), "{url}");
    assert!(url.contains("X-Amz-Signature="), "{url}");
}

#[test]
fn mb_options_and_failures() {
    let env = Env::new();
    let result = env
        .run(&[
            "mb",
            "s3://regional",
            "--region",
            "ap-northeast-1",
            "--tags",
            "k",
            "v",
        ])
        .success();
    assert_eq!(result.stdout, "make_bucket: regional\n");
    let result = env.run(&["mb", "s3://regional"]);
    assert_eq!(result.code, 1);
    assert!(
        result
            .stderr
            .starts_with("make_bucket failed: s3://regional An error occurred ("),
        "{}",
        result.stderr
    );
}

#[test]
fn website_unsupported() {
    let env = Env::new();
    env.bucket("bucket");
    // The fake server does not support the website configuration, which
    // checks the errors are reported.
    let result = env.run(&[
        "website",
        "bucket/",
        "--index-document",
        "index.html",
        "--error-document",
        "error.html",
    ]);
    assert_eq!(result.code, 254);
    assert!(
        result
            .stderr
            .contains("when calling the PutBucketWebsite operation"),
        "{}",
        result.stderr
    );
}

#[test]
fn presign_expires_in_limit() {
    let env = Env::new();
    let result = env.run(&["presign", "s3://bucket/key", "--expires-in", "604801"]);
    assert_eq!(result.code, 252);
}
