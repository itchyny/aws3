//! The tests of the api operations of object listing.

use crate::common::{Env, xml_response};

/// Returns the keys of the contents in the output of the listing.
fn listed_keys(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("\"Key\": \""))
        .map(|key| key.trim_end_matches(['"', ',']).to_owned())
        .collect()
}

#[test]
fn list_objects_v2() {
    let env = Env::new();
    env.bucket("bucket");
    for key in ["a b+c.txt", "a.txt", "dir/sub/z.txt", "dir/x.txt", "e.txt"] {
        env.object("bucket", key, "x");
    }
    let list = |args: &[&str]| {
        let mut command = vec!["api", "list-objects-v2", "--bucket", "bucket"];
        command.extend(args);
        env.run(&command).success().stdout
    };

    // The keys are decoded, and the pages are merged.
    let stdout = list(&["--page-size", "2"]);
    assert_eq!(
        listed_keys(&stdout),
        ["a b+c.txt", "a.txt", "dir/sub/z.txt", "dir/x.txt", "e.txt"]
    );
    assert!(
        stdout.ends_with(
            r#"    ],
    "RequestCharged": null,
    "Prefix": null
}
"#
        ),
        "{stdout}"
    );
    let requests = env.requests();
    let queries: Vec<_> = requests
        .iter()
        .rev()
        .take(3)
        .map(|request| request.uri.query().unwrap_or_default().to_owned())
        .collect();
    assert!(
        queries
            .iter()
            .all(|query| query.contains("encoding-type=url"))
    );
    assert!(queries.iter().all(|query| query.contains("max-keys=2")));

    let stdout = list(&["--prefix", "dir/", "--delimiter", "/"]);
    assert_eq!(listed_keys(&stdout), ["dir/x.txt"]);
    assert!(
        stdout.contains(
            r#"
    "CommonPrefixes": [
        {
            "Prefix": "dir/sub/"
        }
    ],
    "RequestCharged": null,
    "Prefix": "dir/"
"#
        ),
        "{stdout}"
    );

    // The keys are not decoded with the encoding type given by the user.
    let stdout = list(&["--encoding-type", "url", "--max-items", "1"]);
    assert_eq!(listed_keys(&stdout), ["a+b%2Bc.txt"]);

    // The output is truncated with the token to resume.
    let stdout = list(&["--max-items", "3", "--page-size", "2"]);
    assert_eq!(
        listed_keys(&stdout),
        ["a b+c.txt", "a.txt", "dir/sub/z.txt"]
    );
    let token = "eyJDb250aW51YXRpb25Ub2tlbiI6ICJhLnR4dCIsICJib3RvX3RydW5jYXRlX2Ftb3VudCI6IDF9";
    assert!(
        stdout.ends_with(&format!(
            r#"    "NextToken": "{token}"
}}
"#
        )),
        "{stdout}"
    );
    let stdout = list(&["--starting-token", token]);
    assert_eq!(listed_keys(&stdout), ["dir/x.txt", "e.txt"]);
    assert!(!stdout.contains("NextToken"));

    // The full response of a request without the pagination.
    let stdout = list(&["--max-keys", "2"]);
    assert_eq!(listed_keys(&stdout), ["a b+c.txt", "a.txt"]);
    assert!(
        stdout.starts_with(
            r#"{
    "IsTruncated": true,
"#
        ),
        "{stdout}"
    );
    assert!(
        stdout.ends_with(
            r#"    "Name": "bucket",
    "MaxKeys": 2,
    "EncodingType": "url",
    "KeyCount": 2,
    "NextContinuationToken": "a.txt"
}
"#
        ),
        "{stdout}"
    );
    let stdout = list(&["--no-paginate"]);
    assert!(
        stdout.starts_with(
            r#"{
    "IsTruncated": false,
"#
        ),
        "{stdout}"
    );

    let stdout = list(&["--prefix", "missing/"]);
    assert_eq!(
        stdout,
        r#"{
    "RequestCharged": null,
    "Prefix": "missing/"
}
"#
    );
}

#[test]
fn list_objects_v2_errors() {
    let env = Env::new();
    env.bucket("bucket");
    env.object("bucket", "a.txt", "x");
    let result = env.run(&[
        "api",
        "list-objects-v2",
        "--bucket",
        "bucket",
        "--no-paginate",
        "--max-items",
        "1",
        "--starting-token",
        "x",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Cannot specify --no-paginate \
         along with pagination arguments: --starting-token, --max-items\n"
    );

    let result = env
        .run(&[
            "api",
            "list-objects-v2",
            "--bucket",
            "bucket",
            "--max-items",
            "0",
        ])
        .success();
    assert_eq!(
        result.stderr,
        "warning: Non-positive values for --max-items may result in undefined behavior.\n"
    );
    assert!(result.stdout.starts_with("{\n    \"Contents\": [],\n"));

    let result = env.run(&["api", "list-objects-v2", "--bucket", "missing"]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (NoSuchBucket) when calling the ListObjectsV2 operation: Unknown\n"
    );
}

#[test]
fn list_same_next_token() {
    let env = Env::new();
    env.bucket("bucket");
    // The pagination stops on the same next token received twice, instead of
    // requesting the same page forever.
    env.intercept(|request| {
        if request.has_query("list-type") {
            Some(xml_response(
                200,
                "<ListBucketResult><Name>bucket</Name><IsTruncated>true</IsTruncated>\
                 <NextContinuationToken>t</NextContinuationToken><KeyCount>1</KeyCount>\
                 <Contents><Key>a</Key></Contents></ListBucketResult>",
            ))
        } else if request.has_query("versions") {
            Some(xml_response(
                200,
                "<ListVersionsResult><Name>bucket</Name><IsTruncated>true</IsTruncated>\
                 <NextKeyMarker>a</NextKeyMarker><Version><Key>a</Key><VersionId>v</VersionId>\
                 </Version></ListVersionsResult>",
            ))
        } else {
            None
        }
    });
    for (operation, tokens) in [
        ("list-objects-v2", "{'ContinuationToken': 't'}"),
        (
            "list-object-versions",
            "{'KeyMarker': 'a', 'VersionIdMarker': None}",
        ),
    ] {
        let count = env.requests().len();
        let result = env.run(&["api", operation, "--bucket", "bucket"]);
        assert_eq!(result.code, 255, "{operation}");
        assert_eq!(
            result.stderr,
            format!(
                "\naws3: [ERROR]: Error during pagination: The same next token was received \
                 twice: {tokens}\n"
            )
        );
        assert_eq!(env.requests().len() - count, 2, "{operation}");
    }
}

#[test]
fn list_object_versions() {
    let env = Env::new();
    env.bucket("bucket");
    // The fake server does not implement the versions.
    let result = env.run(&[
        "api",
        "list-object-versions",
        "--bucket",
        "bucket",
        "--prefix",
        "dir/",
        "--page-size",
        "2",
    ]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (NotImplemented) when calling the \
         ListObjectVersions operation: ListObjectVersions is not implemented yet\n"
    );
    let requests = env.requests();
    let mut query: Vec<_> = requests
        .last()
        .unwrap()
        .uri
        .query()
        .unwrap()
        .split('&')
        .collect();
    query.sort();
    assert_eq!(
        query,
        [
            "encoding-type=url",
            "max-keys=2",
            "prefix=dir%2F",
            "versions"
        ]
    );

    let result = env.run(&[
        "api",
        "list-object-versions",
        "--bucket",
        "bucket",
        "--key-marker",
        "a",
        "--page-size",
        "2",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Cannot specify --no-paginate \
         along with pagination arguments: --page-size\n"
    );

    // The pages are merged, with the markers of the next page.
    let first = r#"<ListVersionsResult>
    <Name>bucket</Name>
    <Prefix></Prefix>
    <KeyMarker></KeyMarker>
    <VersionIdMarker></VersionIdMarker>
    <NextKeyMarker>b.txt</NextKeyMarker>
    <NextVersionIdMarker>v2</NextVersionIdMarker>
    <MaxKeys>2</MaxKeys>
    <Delimiter>/</Delimiter>
    <IsTruncated>true</IsTruncated>
    <Version>
        <Key>a.txt</Key>
        <VersionId>v1</VersionId>
        <IsLatest>true</IsLatest>
        <LastModified>2026-01-02T03:04:05.000Z</LastModified>
        <ETag>&quot;5d41402abc4b2a76b9719d911017c592&quot;</ETag>
        <Size>5</Size>
        <StorageClass>STANDARD</StorageClass>
        <Owner>
            <ID>owner-id</ID>
            <DisplayName>owner</DisplayName>
        </Owner>
        <ChecksumAlgorithm>CRC64NVME</ChecksumAlgorithm>
        <ChecksumType>FULL_OBJECT</ChecksumType>
    </Version>
    <DeleteMarker>
        <Key>b.txt</Key>
        <VersionId>v2</VersionId>
        <IsLatest>true</IsLatest>
        <LastModified>2026-01-02T03:04:06.000Z</LastModified>
        <Owner>
            <ID>owner-id</ID>
            <DisplayName>owner</DisplayName>
        </Owner>
    </DeleteMarker>
    <CommonPrefixes>
        <Prefix>dir/</Prefix>
    </CommonPrefixes>
</ListVersionsResult>"#;
    let second = r#"<ListVersionsResult>
    <Name>bucket</Name>
    <Prefix></Prefix>
    <KeyMarker>b.txt</KeyMarker>
    <VersionIdMarker>v2</VersionIdMarker>
    <MaxKeys>2</MaxKeys>
    <Delimiter>/</Delimiter>
    <IsTruncated>false</IsTruncated>
    <Version>
        <Key>b.txt</Key>
        <VersionId>v3</VersionId>
        <IsLatest>false</IsLatest>
        <LastModified>2026-01-01T00:00:00.000Z</LastModified>
        <ETag>&quot;d41d8cd98f00b204e9800998ecf8427e&quot;</ETag>
        <Size>0</Size>
        <StorageClass>GLACIER</StorageClass>
        <Owner>
            <ID>owner-id</ID>
            <DisplayName>owner</DisplayName>
        </Owner>
        <RestoreStatus>
            <IsRestoreInProgress>false</IsRestoreInProgress>
            <RestoreExpiryDate>2026-02-01T00:00:00.000Z</RestoreExpiryDate>
        </RestoreStatus>
    </Version>
    <CommonPrefixes>
        <Prefix>logs/</Prefix>
    </CommonPrefixes>
</ListVersionsResult>"#;
    env.intercept(move |request| {
        let query = request.uri.query().unwrap_or_default();
        let body = if query.contains("key-marker=b.txt") {
            second
        } else {
            first
        };
        Some(xml_response(200, body))
    });
    let mut command = vec!["api"];
    command.extend([
        "list-object-versions",
        "--bucket",
        "bucket",
        "--delimiter",
        "/",
        "--page-size",
        "2",
    ]);
    let stdout = env.run(&command).success().stdout;
    assert_eq!(
        stdout,
        r#"{
    "Versions": [
        {
            "ETag": "\"5d41402abc4b2a76b9719d911017c592\"",
            "ChecksumAlgorithm": [
                "CRC64NVME"
            ],
            "ChecksumType": "FULL_OBJECT",
            "Size": 5,
            "StorageClass": "STANDARD",
            "Key": "a.txt",
            "VersionId": "v1",
            "IsLatest": true,
            "LastModified": "2026-01-02T03:04:05+00:00",
            "Owner": {
                "DisplayName": "owner",
                "ID": "owner-id"
            }
        },
        {
            "ETag": "\"d41d8cd98f00b204e9800998ecf8427e\"",
            "Size": 0,
            "StorageClass": "GLACIER",
            "Key": "b.txt",
            "VersionId": "v3",
            "IsLatest": false,
            "LastModified": "2026-01-01T00:00:00+00:00",
            "Owner": {
                "DisplayName": "owner",
                "ID": "owner-id"
            },
            "RestoreStatus": {
                "IsRestoreInProgress": false,
                "RestoreExpiryDate": "2026-02-01T00:00:00+00:00"
            }
        }
    ],
    "DeleteMarkers": [
        {
            "Owner": {
                "DisplayName": "owner",
                "ID": "owner-id"
            },
            "Key": "b.txt",
            "VersionId": "v2",
            "IsLatest": true,
            "LastModified": "2026-01-02T03:04:06+00:00"
        }
    ],
    "CommonPrefixes": [
        {
            "Prefix": "dir/"
        },
        {
            "Prefix": "logs/"
        }
    ],
    "RequestCharged": null,
    "Prefix": ""
}
"#
    );
    let requests = env.requests();
    let query = requests.last().unwrap().uri.query().unwrap();
    assert!(query.contains("version-id-marker=v2"), "{query}");
}
