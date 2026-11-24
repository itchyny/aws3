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
fn mv_upload_modified() {
    let env = Env::new();
    env.bucket("bucket");
    env.write("a.txt", "old");
    // The file modified during the upload is not removed.
    let path = env.path("a.txt");
    env.intercept(move |request| {
        if request.method == http::Method::PUT && request.uri.path() == "/bucket/a.txt" {
            std::fs::write(&path, "new contents").unwrap();
        }
        None
    });
    let result = env.run(&["mv", "a.txt", "s3://bucket/a.txt", "--no-progress"]);
    assert_eq!(result.code, 1);
    assert_eq!(
        result.stderr,
        "move failed: ./a.txt to s3://bucket/a.txt The file was modified during the upload\n"
    );
    assert_eq!(env.read("a.txt"), b"new contents");
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"old");
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
fn mv_validate_same_s3_paths_resolved() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "a");
    env.intercept(|request| {
        let path = request.uri.path();
        let query = request.uri.query().unwrap_or_default();
        if let Some(name) = path.strip_prefix("/v20180820/accesspoint/") {
            if name == "denied" {
                let body = "<ErrorResponse><Error><Code>AccessDenied</Code>\
                            <Message>Access Denied</Message></Error></ErrorResponse>";
                return Some(common::xml_response(403, body));
            }
            let bucket = if name == "other-ap" {
                "other"
            } else {
                "bucket"
            };
            let body =
                format!("<GetAccessPointResult><Bucket>{bucket}</Bucket></GetAccessPointResult>");
            return Some(common::xml_response(200, &body));
        }
        if path == "/v20180820/mrap/instances" {
            // The multi-region access point is found on the second page.
            let body = if query.contains("nextToken=page2") {
                "<ListMultiRegionAccessPointsResult><AccessPoints><AccessPoint>\
                 <Alias>mfzwi23gnjvgw.mrap</Alias><Regions>\
                 <Region><Bucket>other</Bucket></Region>\
                 <Region><Bucket>bucket</Bucket></Region>\
                 </Regions></AccessPoint></AccessPoints></ListMultiRegionAccessPointsResult>"
            } else {
                "<ListMultiRegionAccessPointsResult><AccessPoints><AccessPoint>\
                 <Alias>abcdefghijklm.mrap</Alias><Regions>\
                 <Region><Bucket>other</Bucket></Region>\
                 </Regions></AccessPoint></AccessPoints>\
                 <NextToken>page2</NextToken></ListMultiRegionAccessPointsResult>"
            };
            return Some(common::xml_response(200, body));
        }
        if request.method == http::Method::POST && path == "/" {
            let body = "<GetCallerIdentityResponse><GetCallerIdentityResult>\
                        <Account>123456789012</Account>\
                        </GetCallerIdentityResult></GetCallerIdentityResponse>";
            return Some(common::xml_response(200, body));
        }
        None
    });
    // The requests to S3 Control, whose hosts are prefixed by the account ID,
    // are forwarded to the server by the proxy. The clients of S3 Control and
    // STS use AWS_ENDPOINT_URL only when --endpoint-url is specified.
    let proxy = env.proxy();
    let url = proxy.url(None);
    let endpoint = env.endpoint().to_owned();
    let run = |src: &str, dest: &str, args: &[&str], envs: &[(&str, &str)]| {
        let args = [
            &["mv", src, dest, "--dryrun", "--endpoint-url", &endpoint],
            args,
        ]
        .concat();
        let envs = [&[("HTTP_PROXY", url.as_str())], envs].concat();
        env.run_with_env(&args, &envs)
    };
    let validate = ["--validate-same-s3-paths"];

    // The paths resolved to the same object are rejected.
    for dest in [
        "s3://arn:aws:s3:us-east-1:123456789012:accesspoint/my-ap/a.txt",
        "s3://arn:aws:s3:us-east-1:123456789012:accesspoint:my-ap/a.txt",
        "s3://arn:aws:s3-outposts:us-east-1:123456789012:outpost/op-0123/accesspoint/ap1/a.txt",
        "s3://arn:aws:s3::123456789012:accesspoint/mfzwi23gnjvgw.mrap/a.txt",
        "s3://my-ap-abcdefghijklmnopqrstuvwxyz0123-s3alias/a.txt",
    ] {
        let result = run("s3://bucket/a.txt", dest, &validate, &[]);
        assert_eq!(result.code, 252, "{dest}: {}", result.stderr);
        assert!(
            result.stderr.contains(&format!(
                "Cannot mv a file onto itself: s3://bucket/a.txt - {dest}"
            )),
            "{dest}: {}",
            result.stderr
        );
    }
    let requests = env.requests();
    let get_access_point = |name: &str| {
        requests
            .iter()
            .filter(|request| request.uri.path() == format!("/v20180820/accesspoint/{name}"))
            .collect::<Vec<_>>()
    };
    assert_eq!(get_access_point("my-ap").len(), 2);
    assert!(
        get_access_point("my-ap")
            .iter()
            .all(|request| request.header("x-amz-account-id") == "123456789012")
    );
    let outposts = "arn%3Aaws%3As3-outposts%3Aus-east-1%3A123456789012%3Aoutpost%2Fop-0123%2Faccesspoint%2Fap1";
    assert_eq!(get_access_point(outposts).len(), 1);
    let s3alias = get_access_point("my-ap-abcdefghijklmnopqrstuvwxyz0123-s3alias");
    assert_eq!(s3alias.len(), 1);
    assert_eq!(s3alias[0].header("x-amz-account-id"), "123456789012");
    let mrap = requests
        .iter()
        .filter(|request| request.uri.path() == "/v20180820/mrap/instances")
        .count();
    assert_eq!(mrap, 2);

    // The paths resolved to other buckets are moved.
    let other = "s3://arn:aws:s3:us-east-1:123456789012:accesspoint/other-ap/a.txt";
    let result = run("s3://bucket/a.txt", other, &validate, &[]).success();
    assert_eq!(
        result.stdout,
        format!("(dryrun) move: s3://bucket/a.txt to {other}\n")
    );
    assert_eq!(result.stderr, "");

    // The validation is enabled by the environment variable, and resolves
    // the source in the source region.
    let access_point = "s3://arn:aws:s3:us-east-1:123456789012:accesspoint/my-ap/a.txt";
    let count = env.requests().len();
    let envs = [("AWS_CLI_S3_MV_VALIDATE_SAME_S3_PATHS", "TRUE")];
    let args = ["--source-region", "eu-west-1"];
    let result = run(access_point, "s3://bucket/a.txt", &args, &envs);
    assert_eq!(result.code, 252, "{}", result.stderr);
    assert!(
        result.stderr.contains("Cannot mv a file onto itself"),
        "{}",
        result.stderr
    );
    let requests = env.requests();
    assert_eq!(requests.len(), count + 1, "{requests:?}");
    assert_eq!(requests[count].signed_region(), "eu-west-1");
    let envs = [("AWS_CLI_S3_MV_VALIDATE_SAME_S3_PATHS", "false")];
    let result = run("s3://bucket/a.txt", access_point, &[], &envs).success();
    assert!(
        result
            .stderr
            .starts_with("warning: Provided s3 paths may resolve to same underlying"),
        "{}",
        result.stderr
    );

    // The failures of resolving the access points are reported.
    let denied = "s3://arn:aws:s3:us-east-1:123456789012:accesspoint/denied/a.txt";
    let result = run("s3://bucket/a.txt", denied, &validate, &[]);
    assert_eq!(result.code, 254, "{}", result.stderr);
    assert!(
        result
            .stderr
            .contains("An error occurred (AccessDenied) when calling the GetAccessPoint operation"),
        "{}",
        result.stderr
    );
    let missing = "s3://arn:aws:s3::123456789012:accesspoint/missing.mrap/a.txt";
    let result = run("s3://bucket/a.txt", missing, &validate, &[]);
    assert_eq!(result.code, 252, "{}", result.stderr);
    assert!(
        result.stderr.contains(
            "Couldn't find multi-region access point with alias missing.mrap \
             in account 123456789012"
        ),
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

    // The source modified before the copy fails the copy pinned by the ETag.
    env.intercept(|request| {
        (!request.header("x-amz-copy-source-if-match").is_empty()).then(|| {
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
        "s3://bucket/c.txt",
        "--no-progress",
    ]);
    assert_eq!(result.code, 1);
    assert_eq!(
        result.stderr,
        "move failed: s3://bucket/a.txt to s3://bucket/c.txt \
         Contents of stored object \"a.txt\" in bucket \"bucket\" did not match expected ETag.\n"
    );
    assert_eq!(env.object_content("bucket", "a.txt").unwrap(), b"a");
    assert_eq!(env.object_content("bucket", "c.txt"), None);
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
