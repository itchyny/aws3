//! The tests of the api operations of multipart uploads.

use super::{get_bucket_configuration, response_output};
use crate::common::{Env, xml_response};

#[test]
fn list_multipart_uploads() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "list-multipart-uploads", "ListMultipartUploads");
    assert_eq!(query, "uploads");

    let stdout = response_output(
        &env,
        &["list-multipart-uploads", "--bucket", "bucket"],
        200,
        &[],
        r#"<ListMultipartUploadsResult>
    <Bucket>bucket</Bucket>
    <KeyMarker></KeyMarker>
    <UploadIdMarker></UploadIdMarker>
    <NextKeyMarker>b.txt</NextKeyMarker>
    <NextUploadIdMarker>upload-2</NextUploadIdMarker>
    <MaxUploads>1000</MaxUploads>
    <IsTruncated>false</IsTruncated>
    <Upload>
        <Key>a.txt</Key>
        <UploadId>upload-1</UploadId>
        <Initiator>
            <ID>owner-id</ID>
            <DisplayName>owner</DisplayName>
        </Initiator>
        <Owner>
            <ID>owner-id</ID>
            <DisplayName>owner</DisplayName>
        </Owner>
        <StorageClass>STANDARD</StorageClass>
        <Initiated>2026-01-02T03:04:05.000Z</Initiated>
        <ChecksumAlgorithm>CRC32</ChecksumAlgorithm>
        <ChecksumType>COMPOSITE</ChecksumType>
    </Upload>
    <Upload>
        <Key>b.txt</Key>
        <UploadId>upload-2</UploadId>
        <Initiator>
            <ID>owner-id</ID>
            <DisplayName>owner</DisplayName>
        </Initiator>
        <Owner>
            <ID>owner-id</ID>
            <DisplayName>owner</DisplayName>
        </Owner>
        <StorageClass>STANDARD_IA</StorageClass>
        <Initiated>2026-01-02T03:04:06.000Z</Initiated>
    </Upload>
</ListMultipartUploadsResult>"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "Uploads": [
        {
            "UploadId": "upload-1",
            "Key": "a.txt",
            "Initiated": "2026-01-02T03:04:05+00:00",
            "StorageClass": "STANDARD",
            "Owner": {
                "DisplayName": "owner",
                "ID": "owner-id"
            },
            "Initiator": {
                "ID": "owner-id",
                "DisplayName": "owner"
            },
            "ChecksumAlgorithm": "CRC32",
            "ChecksumType": "COMPOSITE"
        },
        {
            "UploadId": "upload-2",
            "Key": "b.txt",
            "Initiated": "2026-01-02T03:04:06+00:00",
            "StorageClass": "STANDARD_IA",
            "Owner": {
                "DisplayName": "owner",
                "ID": "owner-id"
            },
            "Initiator": {
                "ID": "owner-id",
                "DisplayName": "owner"
            }
        }
    ],
    "RequestCharged": null,
    "Prefix": null
}
"#
    );
}

#[test]
fn list_multipart_uploads_pages() {
    let env = Env::new();
    let upload = |key: &str, id: &str, second: u8| {
        format!(
            "<Upload><Key>{key}</Key><UploadId>{id}</UploadId><Initiator><ID>init-id</ID>\
             <DisplayName>init</DisplayName></Initiator><Owner><ID>owner-id</ID>\
             <DisplayName>owner</DisplayName></Owner><StorageClass>STANDARD</StorageClass>\
             <Initiated>2026-01-02T03:04:0{second}.000Z</Initiated></Upload>"
        )
    };
    // The pages continue from both the key marker and the upload ID marker.
    let first = format!(
        "<ListMultipartUploadsResult><Bucket>bucket</Bucket><KeyMarker></KeyMarker>\
         <UploadIdMarker></UploadIdMarker><NextKeyMarker>a.bin</NextKeyMarker>\
         <NextUploadIdMarker>u1</NextUploadIdMarker><Delimiter>/</Delimiter>\
         <MaxUploads>1</MaxUploads><IsTruncated>true</IsTruncated>{}\
         </ListMultipartUploadsResult>",
        upload("a.bin", "u1", 5)
    );
    let second = format!(
        "<ListMultipartUploadsResult><Bucket>bucket</Bucket><KeyMarker>a.bin</KeyMarker>\
         <UploadIdMarker>u1</UploadIdMarker><Delimiter>/</Delimiter><MaxUploads>1</MaxUploads>\
         <IsTruncated>false</IsTruncated>{}<CommonPrefixes><Prefix>dir/</Prefix>\
         </CommonPrefixes></ListMultipartUploadsResult>",
        upload("b.bin", "u2", 6)
    );
    env.intercept(move |request| {
        let query = request.uri.query().unwrap_or_default();
        let body = if query.contains("key-marker=a.bin") && query.contains("upload-id-marker=u1") {
            &second
        } else {
            &first
        };
        Some(xml_response(200, body))
    });
    let initiated = |id: &str, key: &str, second: u8| {
        format!(
            r#"        {{
            "UploadId": "{id}",
            "Key": "{key}",
            "Initiated": "2026-01-02T03:04:0{second}+00:00",
            "StorageClass": "STANDARD",
            "Owner": {{
                "DisplayName": "owner",
                "ID": "owner-id"
            }},
            "Initiator": {{
                "ID": "init-id",
                "DisplayName": "init"
            }}
        }}"#
        )
    };
    let args = [
        "api",
        "list-multipart-uploads",
        "--bucket",
        "bucket",
        "--delimiter",
        "/",
    ];
    let result = env.run(&args).success();
    assert_eq!(
        result.stdout,
        format!(
            r#"{{
    "Uploads": [
{},
{}
    ],
    "CommonPrefixes": [
        {{
            "Prefix": "dir/"
        }}
    ],
    "RequestCharged": null,
    "Prefix": null
}}
"#,
            initiated("u1", "a.bin", 5),
            initiated("u2", "b.bin", 6)
        )
    );

    // The single page of the markers is output as it is with --no-paginate.
    let args = [
        &args[..],
        &[
            "--no-paginate",
            "--key-marker",
            "x",
            "--upload-id-marker",
            "y",
        ],
    ]
    .concat();
    let result = env.run(&args).success();
    let query = env
        .requests()
        .last()
        .unwrap()
        .uri
        .query()
        .unwrap()
        .to_owned();
    assert!(
        query.contains("key-marker=x") && query.contains("upload-id-marker=y"),
        "{query}"
    );
    assert_eq!(
        result.stdout,
        format!(
            r#"{{
    "Bucket": "bucket",
    "KeyMarker": "",
    "UploadIdMarker": "",
    "NextKeyMarker": "a.bin",
    "Delimiter": "/",
    "NextUploadIdMarker": "u1",
    "MaxUploads": 1,
    "IsTruncated": true,
    "Uploads": [
{}
    ]
}}
"#,
            initiated("u1", "a.bin", 5)
        )
    );
}

#[test]
fn abort_multipart_upload() {
    let env = Env::new();
    env.bucket("bucket");
    let result = env.run(&[
        "api",
        "abort-multipart-upload",
        "--bucket",
        "bucket",
        "--key",
        "dir/a.txt",
        "--upload-id",
        "u1",
    ]);
    assert_eq!(result.code, 254);
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.method, http::Method::DELETE);
    assert_eq!(request.uri.path(), "/bucket/dir/a.txt");
    assert_eq!(
        request.uri.query(),
        Some("x-id=AbortMultipartUpload&uploadId=u1")
    );

    let result = env.run(&[
        "api",
        "abort-multipart-upload",
        "--bucket",
        "bucket",
        "--key",
        "dir/a.txt",
    ]);
    assert_eq!(result.code, 252);

    let stdout = response_output(
        &env,
        &[
            "abort-multipart-upload",
            "--bucket",
            "bucket",
            "--key",
            "a.txt",
            "--upload-id",
            "u1",
        ],
        204,
        &[("x-amz-request-charged", "requester")],
        "",
    );
    assert_eq!(
        stdout,
        r#"{
    "RequestCharged": "requester"
}
"#
    );
}
