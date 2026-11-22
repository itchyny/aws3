//! The tests of the api operations of multipart uploads.

use super::{get_bucket_configuration, response_output};
use crate::common::Env;

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
