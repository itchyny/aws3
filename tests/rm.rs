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
fn rm_recursive_batch_transient_errors() {
    let env = Env::new();
    env.bucket("bucket");
    for key in ["a.txt", "b.txt", "c.txt", "d.txt"] {
        env.object("bucket", key, "x");
    }
    // The objects with the transient errors are sent again in a batch, while
    // the object with the other error is not.
    let first = std::sync::atomic::AtomicBool::new(true);
    env.intercept(move |request| {
        (is_delete_objects(request) && first.swap(false, std::sync::atomic::Ordering::SeqCst)).then(
            || {
                xml_response(
                    200,
                    "<DeleteResult>\
                     <Error><Key>b.txt</Key><Code>InternalError</Code>\
                     <Message>We encountered an internal error. Please try again.</Message></Error>\
                     <Error><Key>c.txt</Key><Code>SlowDown</Code>\
                     <Message>Please reduce your request rate.</Message></Error>\
                     <Error><Key>d.txt</Key><Code>AccessDenied</Code>\
                     <Message>Access Denied</Message></Error>\
                     </DeleteResult>",
                )
            },
        )
    });
    let result = env.run(&["rm", "s3://bucket/", "--recursive"]);
    assert_eq!(result.code, 1);
    assert_eq!(
        result.sorted_lines(),
        ["a.txt", "b.txt", "c.txt"].map(|key| format!("delete: s3://bucket/{key}"))
    );
    assert_eq!(
        result.stderr,
        "delete failed: s3://bucket/d.txt An error occurred (AccessDenied) \
         when calling the DeleteObject operation: Access Denied\n"
    );
    assert_eq!(env.object_content("bucket", "b.txt"), None);
    assert_eq!(env.object_content("bucket", "c.txt"), None);
    assert!(env.object_content("bucket", "d.txt").is_some());
    let requests = env.requests();
    let batches: Vec<_> = requests.iter().filter(|r| is_delete_objects(r)).collect();
    assert_eq!(batches.len(), 2);
    assert!(String::from_utf8_lossy(&batches[1].body).contains("<Key>b.txt</Key>"));
    assert!(!String::from_utf8_lossy(&batches[1].body).contains("<Key>d.txt</Key>"));
    assert!(requests.iter().all(|request| request.method != "DELETE"));
}

#[test]
fn rm_recursive_batch_throttled() {
    let env = Env::new();
    env.bucket("bucket");
    let keys = ["a.txt", "b.txt", "c.txt"];
    let throttle = |requests: usize| {
        let count = std::sync::atomic::AtomicUsize::new(0);
        move |request: &Request| {
            (is_delete_objects(request)
                && count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < requests)
                .then(|| {
                    xml_response(
                        503,
                        "<Error><Code>SlowDown</Code>\
                         <Message>Please reduce your request rate.</Message></Error>",
                    )
                })
        }
    };
    let batches = |env: &Env| {
        env.requests()
            .iter()
            .filter(|r| is_delete_objects(r))
            .count()
    };
    let envs = [("AWS_MAX_ATTEMPTS", "2")];
    // The batch throttled through the retries of the SDK is sent again.
    for key in keys {
        env.object("bucket", key, "x");
    }
    env.intercept(throttle(2));
    let result = env
        .run_with_env(&["rm", "s3://bucket/", "--recursive"], &envs)
        .success();
    assert_eq!(
        result.sorted_lines(),
        keys.map(|key| format!("delete: s3://bucket/{key}"))
    );
    assert_eq!(batches(&env), 3);
    assert_eq!(env.object_content("bucket", "a.txt"), None);
    // The batch fails after it is sent as many times as the attempts one at
    // a time, after the throttling halves the batches sent at once from two.
    for key in keys {
        env.object("bucket", key, "x");
    }
    let config = env.config("[default]\ns3 =\n  max_concurrent_requests = 2\n");
    let envs = [envs[0], ("AWS_CONFIG_FILE", config.to_str().unwrap())];
    let start = batches(&env);
    env.intercept(throttle(usize::MAX));
    let result = env.run_with_env(&["rm", "s3://bucket/", "--recursive"], &envs);
    assert_eq!(result.code, 1);
    assert_eq!(result.stderr.lines().count(), 3, "{}", result.stderr);
    assert!(
        result.stderr.starts_with(
            "delete failed: s3://bucket/a.txt An error occurred (SlowDown) when calling \
             the DeleteObject operation (reached max retries: 1): Please reduce your request rate."
        ),
        "{}",
        result.stderr
    );
    assert_eq!(batches(&env) - start, 6);
    assert!(env.object_content("bucket", "a.txt").is_some());
    // The transient errors other than the throttling do not halve the batches
    // sent at once, and count as the attempts from the first one.
    let start = batches(&env);
    env.intercept(|request| {
        is_delete_objects(request).then(|| {
            xml_response(
                500,
                "<Error><Code>InternalError</Code>\
                 <Message>We encountered an internal error. Please try again.</Message></Error>",
            )
        })
    });
    let result = env.run_with_env(&["rm", "s3://bucket/", "--recursive"], &envs);
    assert_eq!(result.code, 1);
    assert_eq!(batches(&env) - start, 4);
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
