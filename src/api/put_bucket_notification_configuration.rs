//! The put-bucket-notification-configuration operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{
    Event, EventBridgeConfiguration, FilterRule, FilterRuleName, LambdaFunctionConfiguration,
    NotificationConfiguration, NotificationConfigurationFilter, QueueConfiguration, S3KeyFilter,
    TopicConfiguration,
};

use super::members;
use super::shape::{Shape, Structure, parse_structure};
use crate::cli::PutBucketNotificationConfigurationArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

static FILTER_RULE: Structure = Structure {
    members: &[
        ("Name", Shape::String { min: 0 }),
        ("Value", Shape::String { min: 0 }),
    ],
    required: &[],
};

static S3_KEY_FILTER: Structure = Structure {
    members: &[("FilterRules", Shape::List(&Shape::Structure(&FILTER_RULE)))],
    required: &[],
};

static NOTIFICATION_CONFIGURATION_FILTER: Structure = Structure {
    members: &[("Key", Shape::Structure(&S3_KEY_FILTER))],
    required: &[],
};

static TOPIC_CONFIGURATION: Structure = Structure {
    members: &[
        ("Id", Shape::String { min: 0 }),
        ("TopicArn", Shape::String { min: 0 }),
        ("Events", Shape::List(&Shape::String { min: 0 })),
        (
            "Filter",
            Shape::Structure(&NOTIFICATION_CONFIGURATION_FILTER),
        ),
    ],
    required: &["TopicArn", "Events"],
};

static QUEUE_CONFIGURATION: Structure = Structure {
    members: &[
        ("Id", Shape::String { min: 0 }),
        ("QueueArn", Shape::String { min: 0 }),
        ("Events", Shape::List(&Shape::String { min: 0 })),
        (
            "Filter",
            Shape::Structure(&NOTIFICATION_CONFIGURATION_FILTER),
        ),
    ],
    required: &["QueueArn", "Events"],
};

static LAMBDA_FUNCTION_CONFIGURATION: Structure = Structure {
    members: &[
        ("Id", Shape::String { min: 0 }),
        ("LambdaFunctionArn", Shape::String { min: 0 }),
        ("Events", Shape::List(&Shape::String { min: 0 })),
        (
            "Filter",
            Shape::Structure(&NOTIFICATION_CONFIGURATION_FILTER),
        ),
    ],
    required: &["LambdaFunctionArn", "Events"],
};

static EVENT_BRIDGE_CONFIGURATION: Structure = Structure {
    members: &[],
    required: &[],
};

static NOTIFICATION_CONFIGURATION: Structure = Structure {
    members: &[
        (
            "TopicConfigurations",
            Shape::List(&Shape::Structure(&TOPIC_CONFIGURATION)),
        ),
        (
            "QueueConfigurations",
            Shape::List(&Shape::Structure(&QUEUE_CONFIGURATION)),
        ),
        (
            "LambdaFunctionConfigurations",
            Shape::List(&Shape::Structure(&LAMBDA_FUNCTION_CONFIGURATION)),
        ),
        (
            "EventBridgeConfiguration",
            Shape::Structure(&EVENT_BRIDGE_CONFIGURATION),
        ),
    ],
    required: &[],
};

pub async fn run(
    client: &Client,
    args: &PutBucketNotificationConfigurationArgs,
) -> Result<Value, Error> {
    let value = parse_structure(
        "--notification-configuration",
        "NotificationConfiguration",
        &args.notification_configuration,
        &NOTIFICATION_CONFIGURATION,
    )?;
    let configuration = configuration(&value)?;
    Redirector::default()
        .send(
            client,
            &args.bucket,
            "PutBucketNotificationConfiguration",
            |client| {
                client
                    .put_bucket_notification_configuration()
                    .bucket(&args.bucket)
                    .notification_configuration(configuration.clone())
                    .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                    .set_skip_destination_validation(args.skip_destination_validation.value())
                    .send()
            },
        )
        .await
        .map_err(|err| Error::sdk("PutBucketNotificationConfiguration", err))?;
    Ok(Value::Map(Vec::new()))
}

/// Builds the configuration from the validated argument.
fn configuration(value: &Value) -> Result<NotificationConfiguration, Error> {
    let error = |err: aws_sdk_s3::error::BuildError| Error::new(err.to_string());
    let configurations = |key: &str| {
        value
            .get(key)
            .map(|_| members::list(value, key).iter().collect::<Vec<_>>())
    };
    let topics = configurations("TopicConfigurations")
        .map(|configurations| {
            configurations
                .into_iter()
                .map(|configuration| {
                    TopicConfiguration::builder()
                        .set_id(members::str(configuration, "Id"))
                        .set_topic_arn(members::str(configuration, "TopicArn"))
                        .set_events(events(configuration))
                        .set_filter(filter(configuration))
                        .build()
                        .map_err(error)
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let queues = configurations("QueueConfigurations")
        .map(|configurations| {
            configurations
                .into_iter()
                .map(|configuration| {
                    QueueConfiguration::builder()
                        .set_id(members::str(configuration, "Id"))
                        .set_queue_arn(members::str(configuration, "QueueArn"))
                        .set_events(events(configuration))
                        .set_filter(filter(configuration))
                        .build()
                        .map_err(error)
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    let lambda_functions = configurations("LambdaFunctionConfigurations")
        .map(|configurations| {
            configurations
                .into_iter()
                .map(|configuration| {
                    LambdaFunctionConfiguration::builder()
                        .set_id(members::str(configuration, "Id"))
                        .set_lambda_function_arn(members::str(configuration, "LambdaFunctionArn"))
                        .set_events(events(configuration))
                        .set_filter(filter(configuration))
                        .build()
                        .map_err(error)
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;
    Ok(NotificationConfiguration::builder()
        .set_topic_configurations(topics)
        .set_queue_configurations(queues)
        .set_lambda_function_configurations(lambda_functions)
        .set_event_bridge_configuration(
            value
                .get("EventBridgeConfiguration")
                .map(|_| EventBridgeConfiguration::builder().build()),
        )
        .build())
}

fn events(configuration: &Value) -> Option<Vec<Event>> {
    configuration.get("Events").map(|_| {
        members::list(configuration, "Events")
            .iter()
            .filter_map(|value| match value {
                Value::Str(value) => Some(Event::from(value.as_str())),
                _ => None,
            })
            .collect()
    })
}

fn filter(configuration: &Value) -> Option<NotificationConfigurationFilter> {
    let filter = configuration.get("Filter")?;
    let key = filter.get("Key").map(|key| {
        let rules = key.get("FilterRules").map(|_| {
            members::list(key, "FilterRules")
                .iter()
                .map(|rule| {
                    FilterRule::builder()
                        .set_name(
                            members::str(rule, "Name")
                                .map(|name| FilterRuleName::from(name.as_str())),
                        )
                        .set_value(members::str(rule, "Value"))
                        .build()
                })
                .collect()
        });
        S3KeyFilter::builder().set_filter_rules(rules).build()
    });
    Some(
        NotificationConfigurationFilter::builder()
            .set_key(key)
            .build(),
    )
}
