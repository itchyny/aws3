//! The tests of the api operations of lifecycle.

use super::{
    delete_bucket_configuration, get_bucket_configuration, last_request_body, response_output,
};
use crate::common::Env;

#[test]
fn get_bucket_lifecycle_configuration() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(
        &env,
        "get-bucket-lifecycle-configuration",
        "GetBucketLifecycleConfiguration",
    );
    assert_eq!(query, "lifecycle");

    let stdout = response_output(
        &env,
        &["get-bucket-lifecycle-configuration", "--bucket", "bucket"],
        200,
        &[(
            "x-amz-transition-default-minimum-object-size",
            "all_storage_classes_128K",
        )],
        r#"<LifecycleConfiguration>
    <Rule>
        <ID>logs</ID>
        <Filter>
            <And>
                <Prefix>logs/</Prefix>
                <Tag>
                    <Key>k</Key>
                    <Value>v</Value>
                </Tag>
                <ObjectSizeGreaterThan>1024</ObjectSizeGreaterThan>
            </And>
        </Filter>
        <Status>Enabled</Status>
        <Transition>
            <Days>30</Days>
            <StorageClass>STANDARD_IA</StorageClass>
        </Transition>
        <Transition>
            <Days>90</Days>
            <StorageClass>GLACIER</StorageClass>
        </Transition>
        <Expiration>
            <Days>365</Days>
        </Expiration>
        <NoncurrentVersionExpiration>
            <NoncurrentDays>7</NoncurrentDays>
            <NewerNoncurrentVersions>2</NewerNoncurrentVersions>
        </NoncurrentVersionExpiration>
        <AbortIncompleteMultipartUpload>
            <DaysAfterInitiation>3</DaysAfterInitiation>
        </AbortIncompleteMultipartUpload>
    </Rule>
    <Rule>
        <ID>tmp</ID>
        <Filter>
            <Prefix>tmp/</Prefix>
        </Filter>
        <Status>Disabled</Status>
        <Expiration>
            <Date>2030-01-01T00:00:00.000Z</Date>
        </Expiration>
    </Rule>
</LifecycleConfiguration>"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "TransitionDefaultMinimumObjectSize": "all_storage_classes_128K",
    "Rules": [
        {
            "Expiration": {
                "Days": 365
            },
            "ID": "logs",
            "Filter": {
                "And": {
                    "Prefix": "logs/",
                    "Tags": [
                        {
                            "Key": "k",
                            "Value": "v"
                        }
                    ],
                    "ObjectSizeGreaterThan": 1024
                }
            },
            "Status": "Enabled",
            "Transitions": [
                {
                    "Days": 30,
                    "StorageClass": "STANDARD_IA"
                },
                {
                    "Days": 90,
                    "StorageClass": "GLACIER"
                }
            ],
            "NoncurrentVersionExpiration": {
                "NoncurrentDays": 7,
                "NewerNoncurrentVersions": 2
            },
            "AbortIncompleteMultipartUpload": {
                "DaysAfterInitiation": 3
            }
        },
        {
            "Expiration": {
                "Date": "2030-01-01T00:00:00+00:00"
            },
            "ID": "tmp",
            "Filter": {
                "Prefix": "tmp/"
            },
            "Status": "Disabled"
        }
    ]
}
"#
    );
}

#[test]
fn put_bucket_lifecycle_configuration() {
    let env = Env::new();
    env.bucket("bucket");
    env.write(
        "lifecycle.json",
        r#"{"Rules": [{"ID": "archive", "Filter": {"And": {"Prefix": "data/",
            "Tags": [{"Key": "k", "Value": "v"}], "ObjectSizeGreaterThan": 1024}},
            "Status": "Enabled", "Transitions": [{"Days": 30, "StorageClass": "GLACIER"}],
            "Expiration": {"Date": "2030-01-01T00:00:00Z"}}]}"#,
    );
    env.run(&[
        "api",
        "put-bucket-lifecycle-configuration",
        "--bucket",
        "bucket",
        "--lifecycle-configuration",
        "file://lifecycle.json",
        "--transition-default-minimum-object-size",
        "all_storage_classes_128K",
    ]);
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<LifecycleConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\"><Rule>\
             <Expiration><Date>2030-01-01T00:00:00Z</Date></Expiration><ID>archive</ID>\
             <Filter><And><Prefix>data/</Prefix><Tag><Key>k</Key><Value>v</Value></Tag>\
             <ObjectSizeGreaterThan>1024</ObjectSizeGreaterThan></And></Filter>\
             <Status>Enabled</Status><Transition><Days>30</Days>\
             <StorageClass>GLACIER</StorageClass></Transition></Rule></LifecycleConfiguration>"
        ),
        "{body}"
    );
    assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));
    let requests = env.requests();
    assert_eq!(
        requests
            .last()
            .unwrap()
            .headers
            .get("x-amz-transition-default-minimum-object-size")
            .unwrap(),
        "all_storage_classes_128K"
    );

    // The legacy prefix, the rules of the noncurrent versions, and the other
    // filters are sent.
    env.write(
        "lifecycle.json",
        r#"{"Rules": [{"ID": "legacy", "Prefix": "old/", "Status": "Disabled",
            "Expiration": {"ExpiredObjectDeleteMarker": true},
            "AbortIncompleteMultipartUpload": {"DaysAfterInitiation": 7}},
            {"ID": "versions", "Filter": {"Tag": {"Key": "k", "Value": "v"}}, "Status": "Enabled",
            "Transitions": [{"Date": "2030-01-01T00:00:00Z", "StorageClass": "STANDARD_IA"}],
            "NoncurrentVersionTransitions": [{"NoncurrentDays": 30, "StorageClass": "GLACIER",
            "NewerNoncurrentVersions": 2}],
            "NoncurrentVersionExpiration": {"NoncurrentDays": 90, "NewerNoncurrentVersions": 3}},
            {"ID": "size", "Filter": {"And": {"ObjectSizeGreaterThan": 1,
            "ObjectSizeLessThan": 1024}}, "Status": "Enabled", "Expiration": {"Days": 1}},
            {"ID": "less", "Filter": {"ObjectSizeLessThan": 10}, "Status": "Enabled",
            "Expiration": {"Days": 2}}]}"#,
    );
    env.run(&[
        "api",
        "put-bucket-lifecycle-configuration",
        "--bucket",
        "bucket",
        "--lifecycle-configuration",
        "file://lifecycle.json",
    ]);
    let (body, _) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<LifecycleConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <Rule><Expiration><ExpiredObjectDeleteMarker>true</ExpiredObjectDeleteMarker>\
             </Expiration><ID>legacy</ID><Prefix>old/</Prefix><Status>Disabled</Status>\
             <AbortIncompleteMultipartUpload><DaysAfterInitiation>7</DaysAfterInitiation>\
             </AbortIncompleteMultipartUpload></Rule>\
             <Rule><ID>versions</ID><Filter><Tag><Key>k</Key><Value>v</Value></Tag></Filter>\
             <Status>Enabled</Status><Transition><Date>2030-01-01T00:00:00Z</Date>\
             <StorageClass>STANDARD_IA</StorageClass></Transition>\
             <NoncurrentVersionTransition><NoncurrentDays>30</NoncurrentDays>\
             <StorageClass>GLACIER</StorageClass>\
             <NewerNoncurrentVersions>2</NewerNoncurrentVersions></NoncurrentVersionTransition>\
             <NoncurrentVersionExpiration><NoncurrentDays>90</NoncurrentDays>\
             <NewerNoncurrentVersions>3</NewerNoncurrentVersions></NoncurrentVersionExpiration>\
             </Rule>\
             <Rule><Expiration><Days>1</Days></Expiration><ID>size</ID><Filter><And>\
             <ObjectSizeGreaterThan>1</ObjectSizeGreaterThan>\
             <ObjectSizeLessThan>1024</ObjectSizeLessThan></And></Filter>\
             <Status>Enabled</Status></Rule>\
             <Rule><Expiration><Days>2</Days></Expiration><ID>less</ID><Filter>\
             <ObjectSizeLessThan>10</ObjectSizeLessThan></Filter><Status>Enabled</Status></Rule>\
             </LifecycleConfiguration>"
        ),
        "{body}"
    );

    for (value, code, message) in [
        (
            "Rules=[{Filter={Prefix=x}}]",
            252,
            "An error occurred (ParamValidation): Parameter validation failed:\n\
             Missing required parameter in LifecycleConfiguration.Rules[0]: \"Status\"",
        ),
        (
            "Rules=[{Status=Enabled,Expiration={Days=x}}]",
            255,
            "Invalid integer value: x",
        ),
    ] {
        let result = env.run(&[
            "api",
            "put-bucket-lifecycle-configuration",
            "--bucket",
            "bucket",
            "--lifecycle-configuration",
            value,
        ]);
        assert_eq!(result.code, code, "{value}");
        assert_eq!(
            result.stderr,
            format!("\naws3: [ERROR]: {message}\n"),
            "{value}"
        );
    }

    let stdout = response_output(
        &env,
        &[
            "put-bucket-lifecycle-configuration",
            "--bucket",
            "bucket",
            "--lifecycle-configuration",
            r#"{"Rules":[{"ID":"a","Status":"Enabled","Filter":{"Prefix":""},"Expiration":{"Days":1}}]}"#,
        ],
        200,
        &[(
            "x-amz-transition-default-minimum-object-size",
            "varies_by_storage_class",
        )],
        "",
    );
    assert_eq!(
        stdout,
        r#"{
    "TransitionDefaultMinimumObjectSize": "varies_by_storage_class"
}
"#
    );
}

#[test]
fn delete_bucket_lifecycle() {
    let env = Env::new();
    env.bucket("bucket");
    let query =
        delete_bucket_configuration(&env, "delete-bucket-lifecycle", "DeleteBucketLifecycle");
    assert_eq!(query, "lifecycle");

    let stdout = response_output(
        &env,
        &["delete-bucket-lifecycle", "--bucket", "bucket"],
        204,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}
