//! The tests of the api operations of logging.

use super::{get_bucket_configuration, last_request_body, response_output};
use crate::common::Env;

#[test]
fn get_bucket_logging() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(&env, "get-bucket-logging", "GetBucketLogging");
    assert_eq!(query, "logging");

    let stdout = response_output(
        &env,
        &["get-bucket-logging", "--bucket", "bucket"],
        200,
        &[],
        r#"<BucketLoggingStatus>
    <LoggingEnabled>
        <TargetBucket>logs</TargetBucket>
        <TargetPrefix>bucket/</TargetPrefix>
        <TargetGrants>
            <Grant>
                <Grantee xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:type="Group">
                    <URI>http://acs.amazonaws.com/groups/global/AuthenticatedUsers</URI>
                </Grantee>
                <Permission>READ</Permission>
            </Grant>
        </TargetGrants>
        <TargetObjectKeyFormat>
            <PartitionedPrefix>
                <PartitionDateSource>EventTime</PartitionDateSource>
            </PartitionedPrefix>
        </TargetObjectKeyFormat>
    </LoggingEnabled>
</BucketLoggingStatus>"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "LoggingEnabled": {
        "TargetBucket": "logs",
        "TargetGrants": [
            {
                "Grantee": {
                    "Type": "Group",
                    "URI": "http://acs.amazonaws.com/groups/global/AuthenticatedUsers"
                },
                "Permission": "READ"
            }
        ],
        "TargetPrefix": "bucket/",
        "TargetObjectKeyFormat": {
            "PartitionedPrefix": {
                "PartitionDateSource": "EventTime"
            }
        }
    }
}
"#
    );
}

#[test]
fn put_bucket_logging() {
    let env = Env::new();
    env.bucket("bucket");
    env.run(&[
        "api",
        "put-bucket-logging",
        "--bucket",
        "bucket",
        "--bucket-logging-status",
        "LoggingEnabled={TargetBucket=logs,TargetPrefix=bucket/,\
         TargetObjectKeyFormat={PartitionedPrefix={PartitionDateSource=EventTime}}}",
    ]);
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<BucketLoggingStatus xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <LoggingEnabled><TargetBucket>logs</TargetBucket><TargetPrefix>bucket/</TargetPrefix>\
             <TargetObjectKeyFormat><PartitionedPrefix><PartitionDateSource>EventTime\
             </PartitionDateSource></PartitionedPrefix></TargetObjectKeyFormat></LoggingEnabled>\
             </BucketLoggingStatus>"
        ),
        "{body}"
    );
    assert!(headers.contains(&"x-amz-sdk-checksum-algorithm: CRC64NVME".to_owned()));

    let result = env.run(&[
        "api",
        "put-bucket-logging",
        "--bucket",
        "bucket",
        "--bucket-logging-status",
        "LoggingEnabled={TargetBucket=logs,TargetPrefix=x,TargetGrants=[{Grantee={ID=x}}]}",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
         Missing required parameter in BucketLoggingStatus.LoggingEnabled.TargetGrants[0].Grantee: \"Type\"\n"
    );

    let stdout = response_output(
        &env,
        &[
            "put-bucket-logging",
            "--bucket",
            "bucket",
            "--bucket-logging-status",
            "{}",
        ],
        200,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}
