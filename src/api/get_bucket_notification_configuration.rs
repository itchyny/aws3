//! The get-bucket-notification-configuration operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{Event, NotificationConfigurationFilter};

use super::output::Response;
use crate::cli::BucketArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &BucketArgs) -> Result<Value, Error> {
    let output = Redirector::default()
        .send(
            client,
            &args.bucket,
            "GetBucketNotificationConfiguration",
            |client| {
                client
                    .get_bucket_notification_configuration()
                    .bucket(&args.bucket)
                    .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                    .send()
            },
        )
        .await
        .map_err(|err| Error::sdk("GetBucketNotificationConfiguration", err))?;
    let mut response = Response::default();
    let topics: Vec<_> = output
        .topic_configurations()
        .iter()
        .map(|configuration| {
            configuration_value(
                configuration.id(),
                ("TopicArn", configuration.topic_arn()),
                configuration.events(),
                configuration.filter(),
            )
        })
        .collect();
    let queues: Vec<_> = output
        .queue_configurations()
        .iter()
        .map(|configuration| {
            configuration_value(
                configuration.id(),
                ("QueueArn", configuration.queue_arn()),
                configuration.events(),
                configuration.filter(),
            )
        })
        .collect();
    let lambda_functions: Vec<_> = output
        .lambda_function_configurations()
        .iter()
        .map(|configuration| {
            configuration_value(
                configuration.id(),
                ("LambdaFunctionArn", configuration.lambda_function_arn()),
                configuration.events(),
                configuration.filter(),
            )
        })
        .collect();
    for (key, configurations) in [
        ("TopicConfigurations", topics),
        ("QueueConfigurations", queues),
        ("LambdaFunctionConfigurations", lambda_functions),
    ] {
        if !configurations.is_empty() {
            response.value(key, Value::List(configurations));
        }
    }
    if output.event_bridge_configuration().is_some() {
        response.value("EventBridgeConfiguration", Value::Map(Vec::new()));
    }
    Ok(response.build())
}

fn configuration_value(
    id: Option<&str>,
    (arn_key, arn): (&str, &str),
    events: &[Event],
    filter: Option<&NotificationConfigurationFilter>,
) -> Value {
    let mut response = Response::default();
    response.str("Id", id);
    response.str(arn_key, Some(arn));
    if !events.is_empty() {
        response.value(
            "Events",
            Value::List(
                events
                    .iter()
                    .map(|event| Value::Str(event.as_str().to_owned()))
                    .collect(),
            ),
        );
    }
    if let Some(filter) = filter {
        response.value("Filter", filter_value(filter));
    }
    response.build()
}

fn filter_value(filter: &NotificationConfigurationFilter) -> Value {
    let mut response = Response::default();
    if let Some(key) = filter.key() {
        let mut key_response = Response::default();
        let rules = key.filter_rules();
        if !rules.is_empty() {
            key_response.value(
                "FilterRules",
                Value::List(
                    rules
                        .iter()
                        .map(|rule| {
                            let mut response = Response::default();
                            response.str("Name", rule.name().map(|name| name.as_str()));
                            response.str("Value", rule.value());
                            response.build()
                        })
                        .collect(),
                ),
            );
        }
        response.value("Key", key_response.build());
    }
    response.build()
}
