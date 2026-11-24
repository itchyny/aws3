//! The tests of the api operations of buckets.

use super::last_request_body;
use crate::common::Env;

#[test]
fn list_buckets() {
    let env = Env::new();
    env.bucket("bucket-a");
    env.bucket("bucket-b");
    let result = env.run(&["api", "list-buckets"]).success();
    // The fake server lists the buckets in the order of the file system.
    let mut names: Vec<_> = result
        .lines()
        .iter()
        .filter_map(|line| line.trim_start().strip_prefix("\"Name\": \""))
        .map(|name| name.trim_end_matches(['"', ',']).to_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["bucket-a", "bucket-b"]);
    assert!(
        result.stdout.ends_with(
            r#"    ],
    "Owner": null,
    "Prefix": null
}
"#
        ),
        "{}",
        result.stdout
    );

    // The output is truncated with the token to resume.
    let result = env
        .run(&["api", "list-buckets", "--max-items", "1"])
        .success();
    assert!(
        result.stdout.ends_with(
            r#"    "NextToken": "eyJDb250aW51YXRpb25Ub2tlbiI6IG51bGwsICJib3RvX3RydW5jYXRlX2Ftb3VudCI6IDF9"
}
"#
        ),
        "{}",
        result.stdout
    );

    let result = env
        .run(&["api", "list-buckets", "--no-paginate", "--prefix", "bucket"])
        .success();
    assert!(!result.stdout.contains("\"Owner\""), "{}", result.stdout);
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert!(request.uri.query().unwrap().contains("prefix=bucket"));
}

#[test]
fn head_bucket() {
    let env = Env::new();
    env.bucket("bucket");
    // The fake server responds with no headers of the bucket.
    let result = env
        .run(&["api", "head-bucket", "--bucket", "bucket"])
        .success();
    assert_eq!(result.stdout, "");

    let result = env.run(&["api", "head-bucket", "--bucket", "missing"]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (404) when calling the HeadBucket operation: Not Found\n"
    );

    // The request is redirected to the region of the bucket.
    env.bucket("west");
    env.bucket_region("west", "us-west-2");
    env.run(&["api", "head-bucket", "--bucket", "west"])
        .success();
    let regions: Vec<_> = env
        .requests()
        .iter()
        .filter(|request| request.bucket() == "west")
        .map(|request| request.signed_region().to_owned())
        .collect();
    assert_eq!(regions, ["us-east-1", "us-west-2"]);
}

#[test]
fn get_bucket_location() {
    let env = Env::new();
    env.bucket("bucket");
    let result = env
        .run(&["api", "get-bucket-location", "--bucket", "bucket"])
        .success();
    // The location constraint of us-east-1 is null.
    assert_eq!(
        result.stdout,
        r#"{
    "LocationConstraint": null
}
"#
    );
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.uri.path(), "/bucket/");
    assert_eq!(request.uri.query(), Some("location"));

    let result = env.run(&["api", "get-bucket-location", "--bucket", "missing"]);
    assert_eq!(result.code, 254);
    assert!(
        result
            .stderr
            .starts_with("\naws3: [ERROR]: An error occurred (NoSuchBucket) when calling the GetBucketLocation operation: "),
        "{}",
        result.stderr
    );
}

#[test]
fn create_bucket_and_delete_bucket() {
    let env = Env::new();
    let result = env
        .run(&[
            "api",
            "create-bucket",
            "--bucket",
            "new-bucket",
            "--create-bucket-configuration",
            "LocationConstraint=us-west-2,Tags=[{Key=a,Value=b}]",
            "--object-ownership",
            "BucketOwnerEnforced",
        ])
        .success();
    assert_eq!(result.stdout, "");
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.method, http::Method::PUT);
    assert_eq!(request.uri.path(), "/new-bucket/");
    assert_eq!(
        request.headers.get("x-amz-object-ownership").unwrap(),
        "BucketOwnerEnforced"
    );

    let result = env.run(&["api", "create-bucket", "--bucket", "new-bucket"]);
    assert_eq!(result.code, 254);
    assert!(
        result.stderr.starts_with(
            "\naws3: [ERROR]: An error occurred (BucketAlreadyExists) when calling the CreateBucket operation: "
        ),
        "{}",
        result.stderr
    );

    let result = env
        .run(&["api", "delete-bucket", "--bucket", "new-bucket"])
        .success();
    assert_eq!(result.stdout, "");
    let result = env.run(&["api", "delete-bucket", "--bucket", "new-bucket"]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (NoSuchBucket) when calling the DeleteBucket operation: Unknown\n"
    );

    let result = env.run(&[
        "api",
        "create-bucket",
        "--bucket",
        "new-bucket",
        "--create-bucket-configuration",
        "Tags=[{Key=a}],Foo=x",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
         Unknown parameter in CreateBucketConfiguration: \"Foo\", must be one of: \
         LocationConstraint, Location, Bucket, Tags\n\
         Missing required parameter in CreateBucketConfiguration.Tags[0]: \"Value\"\n"
    );

    // The location and the type of directory buckets are sent.
    env.run(&[
        "api",
        "create-bucket",
        "--bucket",
        "dir-bucket",
        "--create-bucket-configuration",
        "Location={Type=AvailabilityZone,Name=usw2-az1},\
         Bucket={DataRedundancy=SingleAvailabilityZone,Type=Directory}",
    ]);
    let (body, _) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<CreateBucketConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <Location><Type>AvailabilityZone</Type><Name>usw2-az1</Name></Location>\
             <Bucket><DataRedundancy>SingleAvailabilityZone</DataRedundancy>\
             <Type>Directory</Type></Bucket></CreateBucketConfiguration>"
        ),
        "{body}"
    );
}
