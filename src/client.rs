use std::time::Duration;

use aws_config::{BehaviorVersion, SdkConfig};
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::config::timeout::TimeoutConfig;

use crate::cli::GlobalArgs;

/// The region the AWS CLI uses for S3 when no region is configured.
const DEFAULT_REGION: &str = "us-east-1";

/// The socket timeout of the AWS CLI, in seconds.
const DEFAULT_TIMEOUT: u64 = 60;

/// Loads the shared configuration from the environment, the config files and
/// the global options.
pub async fn load_config(global: &GlobalArgs) -> SdkConfig {
    let mut loader =
        aws_config::defaults(BehaviorVersion::latest()).timeout_config(timeout_config(global));
    if let Some(profile) = &global.profile {
        loader = loader.profile_name(profile);
    }
    if let Some(region) = &global.region {
        loader = loader.region(Region::new(region.clone()));
    }
    if let Some(endpoint_url) = &global.endpoint_url {
        loader = loader.endpoint_url(endpoint_url);
    }
    if global.no_sign_request {
        loader = loader.no_credentials();
    }
    loader.load().await
}

/// Creates a client, optionally overriding the region, e.g. for a bucket in
/// another region.
pub fn new_client(config: &SdkConfig, region: Option<&str>) -> Client {
    let mut builder = aws_sdk_s3::config::Builder::from(config);
    if let Some(region) = region {
        builder = builder.region(Region::new(region.to_owned()));
    } else if config.region().is_none() {
        builder = builder.region(Region::from_static(DEFAULT_REGION));
    }
    Client::from_conf(builder.build())
}

fn timeout_config(global: &GlobalArgs) -> TimeoutConfig {
    let mut builder = TimeoutConfig::builder();
    builder = match global.cli_connect_timeout.unwrap_or(DEFAULT_TIMEOUT) {
        0 => builder.disable_connect_timeout(),
        secs => builder.connect_timeout(Duration::from_secs(secs)),
    };
    builder = match global.cli_read_timeout.unwrap_or(DEFAULT_TIMEOUT) {
        0 => builder.disable_read_timeout(),
        secs => builder.read_timeout(Duration::from_secs(secs)),
    };
    builder.build()
}
