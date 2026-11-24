//! The tests of the api operations of encryption.

use super::{
    delete_bucket_configuration, get_bucket_configuration, last_request_body, response_output,
};
use crate::common::Env;

#[test]
fn get_bucket_encryption() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-encryption", "GetBucketEncryption");
    assert_eq!(query, "encryption");

    let stdout = response_output(
        &env,
        &["get-bucket-encryption", "--bucket", "bucket"],
        200,
        &[],
        r#"<ServerSideEncryptionConfiguration>
    <Rule>
        <ApplyServerSideEncryptionByDefault>
            <SSEAlgorithm>aws:kms</SSEAlgorithm>
            <KMSMasterKeyID>arn:aws:kms:us-east-1:123456789012:key/abcd</KMSMasterKeyID>
        </ApplyServerSideEncryptionByDefault>
        <BucketKeyEnabled>true</BucketKeyEnabled>
    </Rule>
</ServerSideEncryptionConfiguration>"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "ServerSideEncryptionConfiguration": {
        "Rules": [
            {
                "ApplyServerSideEncryptionByDefault": {
                    "SSEAlgorithm": "aws:kms",
                    "KMSMasterKeyID": "arn:aws:kms:us-east-1:123456789012:key/abcd"
                },
                "BucketKeyEnabled": true
            }
        ]
    }
}
"#
    );

    // The encryption types blocked are output, which are empty if none.
    let stdout = response_output(
        &env,
        &["get-bucket-encryption", "--bucket", "bucket"],
        200,
        &[],
        "<ServerSideEncryptionConfiguration><Rule><ApplyServerSideEncryptionByDefault>\
         <SSEAlgorithm>aws:kms</SSEAlgorithm></ApplyServerSideEncryptionByDefault>\
         <BlockedEncryptionTypes><EncryptionType>SSE-C</EncryptionType>\
         </BlockedEncryptionTypes></Rule><Rule><ApplyServerSideEncryptionByDefault>\
         <SSEAlgorithm>AES256</SSEAlgorithm></ApplyServerSideEncryptionByDefault>\
         <BlockedEncryptionTypes></BlockedEncryptionTypes></Rule>\
         </ServerSideEncryptionConfiguration>",
    );
    assert_eq!(
        stdout,
        r#"{
    "ServerSideEncryptionConfiguration": {
        "Rules": [
            {
                "ApplyServerSideEncryptionByDefault": {
                    "SSEAlgorithm": "aws:kms"
                },
                "BlockedEncryptionTypes": {
                    "EncryptionType": [
                        "SSE-C"
                    ]
                }
            },
            {
                "ApplyServerSideEncryptionByDefault": {
                    "SSEAlgorithm": "AES256"
                },
                "BlockedEncryptionTypes": {}
            }
        ]
    }
}
"#
    );
}

#[test]
fn put_bucket_encryption() {
    let env = Env::new();
    env.bucket("bucket");
    env.run(&[
        "api",
        "put-bucket-encryption",
        "--bucket",
        "bucket",
        "--server-side-encryption-configuration",
        "Rules=[{ApplyServerSideEncryptionByDefault={SSEAlgorithm=aws:kms,KMSMasterKeyID=key},\
         BucketKeyEnabled=true,BlockedEncryptionTypes={EncryptionType=SSE-C}}]",
    ]);
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<ServerSideEncryptionConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <Rule><ApplyServerSideEncryptionByDefault><SSEAlgorithm>aws:kms</SSEAlgorithm>\
             <KMSMasterKeyID>key</KMSMasterKeyID></ApplyServerSideEncryptionByDefault>\
             <BucketKeyEnabled>true</BucketKeyEnabled><BlockedEncryptionTypes>\
             <EncryptionType>SSE-C</EncryptionType></BlockedEncryptionTypes></Rule>\
             </ServerSideEncryptionConfiguration>"
        ),
        "{body}"
    );
    assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));

    let result = env.run(&[
        "api",
        "put-bucket-encryption",
        "--bucket",
        "bucket",
        "--server-side-encryption-configuration",
        "Rules=[{ApplyServerSideEncryptionByDefault={KMSMasterKeyID=key}}]",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
         Missing required parameter in ServerSideEncryptionConfiguration.Rules[0]\
         .ApplyServerSideEncryptionByDefault: \"SSEAlgorithm\"\n"
    );

    let stdout = response_output(
        &env,
        &[
            "put-bucket-encryption",
            "--bucket",
            "bucket",
            "--server-side-encryption-configuration",
            r#"{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"AES256"}}]}"#,
        ],
        200,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}

#[test]
fn delete_bucket_encryption() {
    let env = Env::new();
    env.bucket("bucket");
    let query =
        delete_bucket_configuration(&env, "delete-bucket-encryption", "DeleteBucketEncryption");
    assert_eq!(query, "encryption");

    let stdout = response_output(
        &env,
        &["delete-bucket-encryption", "--bucket", "bucket"],
        204,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}
