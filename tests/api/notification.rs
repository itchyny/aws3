//! The tests of the api operations of notification.

use super::{get_bucket_configuration, last_request_body, response_output};
use crate::common::Env;

#[test]
fn get_bucket_notification_configuration() {
    let env = Env::new();
    env.bucket("bucket");
    let query = get_bucket_configuration(
        &env,
        "get-bucket-notification-configuration",
        "GetBucketNotificationConfiguration",
    );
    assert_eq!(query, "notification");

    let stdout = response_output(
        &env,
        &[
            "get-bucket-notification-configuration",
            "--bucket",
            "bucket",
        ],
        200,
        &[],
        r#"<NotificationConfiguration>
    <TopicConfiguration>
        <Id>topic</Id>
        <Topic>arn:aws:sns:us-east-1:123456789012:topic</Topic>
        <Event>s3:ObjectCreated:*</Event>
        <Filter>
            <S3Key>
                <FilterRule>
                    <Name>prefix</Name>
                    <Value>images/</Value>
                </FilterRule>
                <FilterRule>
                    <Name>suffix</Name>
                    <Value>.jpg</Value>
                </FilterRule>
            </S3Key>
        </Filter>
    </TopicConfiguration>
    <QueueConfiguration>
        <Id>queue</Id>
        <Queue>arn:aws:sqs:us-east-1:123456789012:queue</Queue>
        <Event>s3:ObjectRemoved:*</Event>
        <Event>s3:ObjectRestore:Completed</Event>
    </QueueConfiguration>
    <CloudFunctionConfiguration>
        <Id>function</Id>
        <CloudFunction>arn:aws:lambda:us-east-1:123456789012:function:f</CloudFunction>
        <Event>s3:ObjectCreated:Put</Event>
    </CloudFunctionConfiguration>
    <EventBridgeConfiguration></EventBridgeConfiguration>
</NotificationConfiguration>"#,
    );
    assert_eq!(
        stdout,
        r#"{
    "TopicConfigurations": [
        {
            "Id": "topic",
            "TopicArn": "arn:aws:sns:us-east-1:123456789012:topic",
            "Events": [
                "s3:ObjectCreated:*"
            ],
            "Filter": {
                "Key": {
                    "FilterRules": [
                        {
                            "Name": "prefix",
                            "Value": "images/"
                        },
                        {
                            "Name": "suffix",
                            "Value": ".jpg"
                        }
                    ]
                }
            }
        }
    ],
    "QueueConfigurations": [
        {
            "Id": "queue",
            "QueueArn": "arn:aws:sqs:us-east-1:123456789012:queue",
            "Events": [
                "s3:ObjectRemoved:*",
                "s3:ObjectRestore:Completed"
            ]
        }
    ],
    "LambdaFunctionConfigurations": [
        {
            "Id": "function",
            "LambdaFunctionArn": "arn:aws:lambda:us-east-1:123456789012:function:f",
            "Events": [
                "s3:ObjectCreated:Put"
            ]
        }
    ],
    "EventBridgeConfiguration": {}
}
"#
    );
}

#[test]
fn put_bucket_notification_configuration() {
    let env = Env::new();
    env.bucket("bucket");
    env.run(&[
        "api",
        "put-bucket-notification-configuration",
        "--bucket",
        "bucket",
        "--notification-configuration",
        "QueueConfigurations=[{Id=q,QueueArn=arn:aws:sqs:us-east-1:123456789012:q,\
         Events=[s3:ObjectCreated:*],Filter={Key={FilterRules=[{Name=prefix,Value=logs/}]}}}],\
         EventBridgeConfiguration={}",
        "--skip-destination-validation",
    ]);
    let skip_destination_validation =
        env.requests().last().unwrap().headers["x-amz-skip-destination-validation"].clone();
    assert_eq!(skip_destination_validation, "true");
    let (body, headers) = last_request_body(&env);
    assert!(
        body.ends_with(
            "<NotificationConfiguration xmlns=\"http://s3.amazonaws.com/doc/2006-03-01/\">\
             <QueueConfiguration><Id>q</Id><Queue>arn:aws:sqs:us-east-1:123456789012:q</Queue>\
             <Event>s3:ObjectCreated:*</Event><Filter><S3Key><FilterRule><Name>prefix</Name>\
             <Value>logs/</Value></FilterRule></S3Key></Filter></QueueConfiguration>\
             <EventBridgeConfiguration></EventBridgeConfiguration></NotificationConfiguration>"
        ),
        "{body}"
    );
    assert!(headers.is_empty(), "{headers:?}");

    let result = env.run(&[
        "api",
        "put-bucket-notification-configuration",
        "--bucket",
        "bucket",
        "--notification-configuration",
        "TopicConfigurations=[{TopicArn=arn:aws:sns:us-east-1:123456789012:t}]",
    ]);
    assert_eq!(result.code, 252);
    assert_eq!(
        result.stderr,
        "\naws3: [ERROR]: An error occurred (ParamValidation): Parameter validation failed:\n\
         Missing required parameter in NotificationConfiguration.TopicConfigurations[0]: \"Events\"\n"
    );

    let stdout = response_output(
        &env,
        &[
            "put-bucket-notification-configuration",
            "--bucket",
            "bucket",
            "--notification-configuration",
            "{}",
        ],
        200,
        &[],
        "",
    );
    assert_eq!(stdout, "");
}
