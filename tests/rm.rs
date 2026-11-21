// The paths in the outputs assume Unix.
#![cfg(unix)]

mod common;

use common::{Env, Request, xml_response};
use md5::Digest;

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

fn is_delete_objects(request: &Request) -> bool {
    request.method == "POST" && request.has_query("delete")
}

#[test]
fn rm_recursive_batches() {
    let env = Env::new();
    env.bucket("bucket");
    for i in 0..1001 {
        env.object("bucket", &format!("dir/{i:04}.txt"), "x");
    }
    env.object("bucket", "other.txt", "x");
    // The objects are listed in a page, since the fake server fails to list
    // the directory whose files are being deleted.
    let result = env
        .run(&[
            "rm",
            "s3://bucket/dir/",
            "--recursive",
            "--page-size",
            "2000",
        ])
        .success();
    let lines = result.sorted_lines();
    assert_eq!(lines.len(), 1001);
    assert_eq!(lines[0], "delete: s3://bucket/dir/0000.txt");
    assert_eq!(lines[1000], "delete: s3://bucket/dir/1000.txt");
    assert_eq!(env.object_content("bucket", "dir/0000.txt"), None);
    assert_eq!(env.object_content("bucket", "dir/1000.txt"), None);
    assert!(env.object_content("bucket", "other.txt").is_some());
    let requests = env.requests();
    let batches: Vec<_> = requests
        .iter()
        .filter(|request| is_delete_objects(request))
        .collect();
    assert_eq!(batches.len(), 2);
    // Content-MD5 is sent for the S3-compatible storages which require it.
    for request in batches {
        let md5 = aws_smithy_types::base64::encode(md5::Md5::digest(&request.body));
        assert_eq!(request.headers.get("content-md5").unwrap(), md5.as_str());
    }
    assert!(requests.iter().all(|request| request.method != "DELETE"));

    // The deletion of an object is not sent in a batch.
    let start = env.requests().len();
    env.run(&["rm", "s3://bucket/other.txt"]).success();
    let requests = env.requests();
    assert!(!requests[start..].iter().any(is_delete_objects));
    assert_eq!(env.object_content("bucket", "other.txt"), None);
}

#[test]
fn rm_recursive_batch_errors() {
    let env = Env::new();
    env.bucket("bucket");
    for key in ["a.txt", "b.txt", "c.txt"] {
        env.object("bucket", key, "x");
    }
    env.intercept(|request| {
        is_delete_objects(request).then(|| {
            xml_response(
                200,
                "<DeleteResult><Error><Key>b.txt</Key><Code>AccessDenied</Code>\
                 <Message>Access Denied</Message></Error></DeleteResult>",
            )
        })
    });
    let result = env.run(&["rm", "s3://bucket/", "--recursive"]);
    assert_eq!(result.code, 1);
    assert_eq!(
        result.sorted_lines(),
        ["delete: s3://bucket/a.txt", "delete: s3://bucket/c.txt"]
    );
    assert_eq!(
        result.stderr,
        "delete failed: s3://bucket/b.txt An error occurred (AccessDenied) \
         when calling the DeleteObject operation: Access Denied\n"
    );

    // The failure of the request fails the deletions of all the objects.
    env.intercept(|request| {
        is_delete_objects(request).then(|| {
            xml_response(
                403,
                "<Error><Code>AccessDenied</Code><Message>Access Denied</Message></Error>",
            )
        })
    });
    let result = env.run(&["rm", "s3://bucket/", "--recursive"]);
    assert_eq!(result.code, 1);
    assert_eq!(result.stdout, "");
    let mut errors: Vec<_> = result.stderr.lines().collect();
    errors.sort_unstable();
    assert_eq!(
        errors,
        ["a.txt", "b.txt", "c.txt"].map(|key| format!(
            "delete failed: s3://bucket/{key} An error occurred (AccessDenied) \
             when calling the DeleteObject operation: Access Denied"
        ))
    );
    assert!(env.object_content("bucket", "a.txt").is_some());
}

#[test]
fn rm_recursive_batch_not_implemented() {
    let env = Env::new();
    env.bucket("bucket");
    for key in ["a.txt", "b.txt", "c.txt"] {
        env.object("bucket", key, "x");
    }
    env.intercept(|request| {
        is_delete_objects(request).then(|| {
            xml_response(
                501,
                "<Error><Code>NotImplemented</Code>\
                 <Message>A header you provided implies functionality that is not implemented</Message></Error>",
            )
        })
    });
    let result = env.run(&["rm", "s3://bucket/", "--recursive"]).success();
    assert_eq!(
        result.sorted_lines(),
        ["a.txt", "b.txt", "c.txt"].map(|key| format!("delete: s3://bucket/{key}"))
    );
    assert_eq!(env.object_content("bucket", "a.txt"), None);
    // The objects are deleted one by one after the batch is rejected.
    let requests = env.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|request| is_delete_objects(request))
            .count(),
        1
    );
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.method == "DELETE")
            .count(),
        3
    );
}
