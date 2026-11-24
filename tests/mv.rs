// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::{Env, content};

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
fn mv_onto_itself_multipart() {
    let env = Env::new();
    env.bucket("bucket");
    let data = content(6 * 1024 * 1024);
    env.object("bucket", "dir/big.bin", &data);
    let config = env.multipart_config();
    let envs: Vec<_> = config
        .iter()
        .map(|(key, value)| (*key, value.as_str()))
        .collect();
    // The multipart copy onto itself succeeds on S3 even without changing the
    // object, which would delete it after the copy. The properties are not
    // copied, which are not supported by the server of the tests.
    let result = env.run_with_env(
        &[
            "mv",
            "s3://bucket/dir",
            "s3://bucket/dir/",
            "--recursive",
            "--copy-props",
            "none",
            "--no-progress",
        ],
        &envs,
    );
    assert_eq!(result.code, 1);
    assert_eq!(result.stdout, "");
    assert_eq!(
        result.stderr,
        "move failed: s3://bucket/dir/big.bin to s3://bucket/dir/big.bin \
         Cannot mv a file onto itself\n"
    );
    assert_eq!(env.object_content("bucket", "dir/big.bin").unwrap(), data);
    assert!(
        env.requests()
            .iter()
            .all(|request| request.header("x-amz-copy-source").is_empty())
    );
}

#[test]
fn mv_onto_itself_recursive() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "dir/a.txt", "a");
    env.object("bucket", "dir/b.txt", "b");
    // The paths differ, but each object is moved onto itself, which would
    // delete it after the copy changing the storage class.
    let result = env.run(&[
        "mv",
        "s3://bucket/dir",
        "s3://bucket/dir/",
        "--recursive",
        "--storage-class",
        "STANDARD_IA",
        "--no-progress",
    ]);
    assert_eq!(result.code, 1);
    assert_eq!(result.stdout, "");
    let mut errors: Vec<_> = result.stderr.lines().collect();
    errors.sort_unstable();
    assert_eq!(
        errors,
        ["a.txt", "b.txt"].map(|key| format!(
            "move failed: s3://bucket/dir/{key} to s3://bucket/dir/{key} \
             Cannot mv a file onto itself"
        ))
    );
    assert_eq!(env.object_content("bucket", "dir/a.txt").unwrap(), b"a");
    assert_eq!(env.object_content("bucket", "dir/b.txt").unwrap(), b"b");
}

#[test]
fn mv_into_source() {
    let env = Env::new();
    env.bucket("bucket");
    for key in ["a.txt", "b.txt", "c.txt"] {
        env.object("bucket", &format!("dir/{key}"), key);
    }
    // The objects moved under the source are not listed and moved again.
    let result = env
        .run(&[
            "mv",
            "s3://bucket/dir/",
            "s3://bucket/dir/sub/",
            "--recursive",
            "--page-size",
            "1",
            "--no-progress",
        ])
        .success();
    assert_eq!(
        result.sorted_lines(),
        ["a.txt", "b.txt", "c.txt"]
            .map(|key| format!("move: s3://bucket/dir/{key} to s3://bucket/dir/sub/{key}"))
    );
    for key in ["a.txt", "b.txt", "c.txt"] {
        assert_eq!(env.object_content("bucket", &format!("dir/{key}")), None);
        assert_eq!(
            env.object_content("bucket", &format!("dir/sub/{key}"))
                .unwrap(),
            key.as_bytes()
        );
        assert_eq!(
            env.object_content("bucket", &format!("dir/sub/sub/{key}")),
            None
        );
    }
    // The copies are not copied again either.
    let result = env
        .run(&[
            "cp",
            "s3://bucket/dir/",
            "s3://bucket/dir/copy/",
            "--recursive",
            "--page-size",
            "1",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.lines().len(), 3, "{}", result.stdout);
    for key in ["a.txt", "b.txt", "c.txt"] {
        assert_eq!(
            env.object_content("bucket", &format!("dir/copy/sub/{key}"))
                .unwrap(),
            key.as_bytes()
        );
        assert_eq!(
            env.object_content("bucket", &format!("dir/copy/copy/sub/{key}")),
            None
        );
    }
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

#[test]
fn mv_conditional_delete() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "dir/a.txt", "a");
    env.object("bucket", "dir/b.txt", "b");
    // The sources are copied and deleted only if they have the ETags of the
    // listing.
    let result = env
        .run(&[
            "mv",
            "s3://bucket/dir/",
            "s3://bucket/moved/",
            "--recursive",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.lines().len(), 2, "{}", result.stdout);
    let requests = env.requests();
    let copies: Vec<_> = requests
        .iter()
        .filter(|request| !request.header("x-amz-copy-source").is_empty())
        .collect();
    assert_eq!(copies.len(), 2);
    assert!(copies.iter().all(|request| {
        request
            .header("x-amz-copy-source-if-match")
            .starts_with('"')
    }));
    let deletes: Vec<_> = requests
        .iter()
        .filter(|request| request.method == http::Method::DELETE)
        .collect();
    assert_eq!(deletes.len(), 2);
    assert!(
        deletes
            .iter()
            .all(|request| request.header("if-match").starts_with('"'))
    );

    // The copies of cp are not pinned.
    let start = env.requests().len();
    env.run(&[
        "cp",
        "s3://bucket/moved/a.txt",
        "s3://bucket/copied.txt",
        "--no-progress",
    ])
    .success();
    let requests = env.requests();
    assert!(
        requests[start..]
            .iter()
            .all(|request| request.header("x-amz-copy-source-if-match").is_empty())
    );
}

#[test]
fn mv_source_modified() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "a");
    // The source modified after the copy fails the deletion, and is kept.
    env.intercept(|request| {
        (request.method == http::Method::DELETE && !request.header("if-match").is_empty()).then(|| {
            common::xml_response(
                412,
                "<Error><Code>PreconditionFailed</Code>\
                 <Message>At least one of the pre-conditions you specified did not hold</Message></Error>",
            )
        })
    });
    let result = env.run(&[
        "mv",
        "s3://bucket/a.txt",
        "s3://bucket/b.txt",
        "--no-progress",
    ]);
    assert_eq!(result.code, 1);
    assert_eq!(
        result.stderr,
        "move failed: s3://bucket/a.txt to s3://bucket/b.txt \
         Contents of stored object \"a.txt\" in bucket \"bucket\" did not match expected ETag.\n"
    );
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"a");
    assert_eq!(env.object_content("bucket", "b.txt").unwrap(), b"a");
}

#[test]
fn mv_conditional_delete_not_implemented() {
    let env = Env::new();
    env.bucket("bucket");
    for key in ["a.txt", "b.txt", "c.txt"] {
        env.object("bucket", &format!("dir/{key}"), key);
    }
    // The storage not supporting the conditional deletions deletes without
    // the condition, which is not sent anymore.
    env.intercept(|request| {
        (request.method == http::Method::DELETE && !request.header("if-match").is_empty()).then(|| {
            common::xml_response(
                501,
                "<Error><Code>NotImplemented</Code>\
                 <Message>A header you provided implies functionality that is not implemented</Message></Error>",
            )
        })
    });
    let result = env
        .run(&[
            "mv",
            "s3://bucket/dir/",
            "s3://bucket/moved/",
            "--recursive",
            "--no-progress",
        ])
        .success();
    assert_eq!(result.lines().len(), 3, "{}", result.stdout);
    for key in ["a.txt", "b.txt", "c.txt"] {
        assert_eq!(env.object_content("bucket", &format!("dir/{key}")), None);
        assert_eq!(
            env.object_content("bucket", &format!("moved/{key}"))
                .unwrap(),
            key.as_bytes()
        );
    }
    let conditional = env
        .requests()
        .iter()
        .filter(|request| {
            request.method == http::Method::DELETE && !request.header("if-match").is_empty()
        })
        .count();
    // The concurrent deletions may send the condition before it is disabled.
    assert!((1..=3).contains(&conditional), "{conditional}");
}

#[test]
fn mv_download_pinned() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "a");
    env.run(&["mv", "s3://bucket/a.txt", "a.txt", "--no-progress"])
        .success();
    assert_eq!(env.read("a.txt"), b"a");
    let requests = env.requests();
    let get = requests
        .iter()
        .find(|request| {
            request.method == http::Method::GET && request.uri.path() == "/bucket/a.txt"
        })
        .unwrap();
    assert!(get.header("if-match").starts_with('"'));
    assert!(
        requests
            .iter()
            .any(|request| request.method == http::Method::DELETE
                && request.header("if-match") == get.header("if-match"))
    );
}
