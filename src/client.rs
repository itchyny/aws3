use std::path::PathBuf;
use std::time::Duration;

use aws_config::{BehaviorVersion, SdkConfig};
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::config::timeout::TimeoutConfig;

use crate::cli::GlobalArgs;
use aws_types::service_config::ServiceConfigKey;

use crate::config::{AddressingStyle, ProfileConfig, S3Config};
use crate::error::Error;
use crate::{insecure, tls};

/// The region the AWS CLI uses for S3 when no region is configured.
const DEFAULT_REGION: &str = "us-east-1";

/// The socket timeout of the AWS CLI, in seconds.
const DEFAULT_TIMEOUT: u64 = 60;

/// The configuration of the clients.
pub struct Config {
    pub sdk: SdkConfig,
    pub s3: S3Config,
    /// The custom endpoint URL of S3.
    pub endpoint_url: Option<String>,
}

/// Loads the configuration from the environment, the config files and the
/// global options.
pub async fn load_config(global: &GlobalArgs) -> Result<Config, Error> {
    let profile = ProfileConfig::load(global.profile.as_deref());
    let ca_bundle = global
        .ca_bundle
        .clone()
        .or_else(|| {
            std::env::var_os("AWS_CA_BUNDLE")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
        })
        .or(profile.ca_bundle);
    let timeout_config = timeout_config(global);
    let http_client = if global.no_verify_ssl {
        insecure::http_client(
            timeout_config.connect_timeout(),
            timeout_config.read_timeout(),
        )?
    } else {
        tls::http_client(ca_bundle.as_deref())?
    };
    let mut loader = aws_config::defaults(BehaviorVersion::latest())
        .http_client(http_client)
        .timeout_config(timeout_config);
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
    let sdk = loader.load().await;
    let endpoint_url = global
        .endpoint_url
        .clone()
        .or_else(|| s3_endpoint_url(&sdk))
        .or_else(|| sdk.endpoint_url().map(str::to_owned));
    Ok(Config {
        sdk,
        s3: profile.s3,
        endpoint_url,
    })
}

/// Returns the endpoint URL specific to S3, i.e. `AWS_ENDPOINT_URL_S3` or the
/// `s3` service in the `services` section of the config file.
fn s3_endpoint_url(sdk: &SdkConfig) -> Option<String> {
    let key = ServiceConfigKey::builder()
        .service_id("S3")
        .env("AWS_ENDPOINT_URL")
        .profile("endpoint_url")
        .build()
        .ok()?;
    sdk.service_config()?.load_config(key)
}

/// Creates a client, optionally overriding the region, e.g. for a bucket in
/// another region.
pub fn new_client(config: &Config, region: Option<&str>) -> Client {
    let mut builder = aws_sdk_s3::config::Builder::from(&config.sdk)
        .force_path_style(force_path_style(config))
        .accelerate(config.s3.use_accelerate_endpoint);
    if let Some(region) = region {
        builder = builder.region(Region::new(region.to_owned()));
    } else if config.sdk.region().is_none() {
        builder = builder.region(Region::from_static(DEFAULT_REGION));
    }
    Client::from_conf(builder.build())
}

/// Returns whether the path style is used in the same way as the AWS CLI,
/// which defaults to the path style for custom endpoints.
fn force_path_style(config: &Config) -> bool {
    let style = config.s3.addressing_style;
    if config.s3.use_accelerate_endpoint {
        false
    } else if config
        .endpoint_url
        .as_deref()
        .is_some_and(|url| !is_s3_accelerate_url(url))
    {
        style != AddressingStyle::Virtual
    } else {
        style == AddressingStyle::Path
    }
}

/// Returns whether the URL is an endpoint of S3 Transfer Acceleration, e.g.
/// `https://s3-accelerate.amazonaws.com`.
fn is_s3_accelerate_url(url: &str) -> bool {
    let Some(host) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .map(|rest| rest.split(['/', '?', '#']).next().unwrap_or_default())
    else {
        return false;
    };
    let host = host.split(':').next().unwrap_or_default();
    let Some(features) = host
        .strip_prefix("s3-accelerate")
        .and_then(|rest| rest.strip_suffix("amazonaws.com"))
    else {
        return false;
    };
    let features: Vec<_> = features
        .split('.')
        .filter(|part| !part.is_empty())
        .collect();
    features.iter().all(|&part| part == "dualstack") && features.len() <= 1
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerate_url() {
        assert!(is_s3_accelerate_url("https://s3-accelerate.amazonaws.com"));
        assert!(is_s3_accelerate_url(
            "https://s3-accelerate.dualstack.amazonaws.com/"
        ));
        assert!(is_s3_accelerate_url(
            "http://s3-accelerate.amazonaws.com:443"
        ));
        assert!(!is_s3_accelerate_url("https://s3.amazonaws.com"));
        assert!(!is_s3_accelerate_url(
            "https://bucket.s3-accelerate.amazonaws.com"
        ));
        assert!(!is_s3_accelerate_url(
            "https://s3-accelerate.fips.amazonaws.com"
        ));
        assert!(!is_s3_accelerate_url("https://s3-accelerate.example.com"));
        assert!(!is_s3_accelerate_url("ftp://s3-accelerate.amazonaws.com"));
    }

    #[test]
    fn path_style() {
        let config = |style, accelerate, endpoint_url: Option<&str>| Config {
            sdk: SdkConfig::builder().build(),
            s3: S3Config {
                addressing_style: style,
                use_accelerate_endpoint: accelerate,
                ..S3Config::default()
            },
            endpoint_url: endpoint_url.map(str::to_owned),
        };
        let custom = Some("http://localhost:9000");
        assert!(!force_path_style(&config(
            AddressingStyle::Auto,
            false,
            None
        )));
        assert!(force_path_style(&config(
            AddressingStyle::Path,
            false,
            None
        )));
        assert!(force_path_style(&config(
            AddressingStyle::Auto,
            false,
            custom
        )));
        assert!(!force_path_style(&config(
            AddressingStyle::Virtual,
            false,
            custom
        )));
        assert!(!force_path_style(&config(
            AddressingStyle::Path,
            true,
            None
        )));
        let accelerate = Some("https://s3-accelerate.amazonaws.com");
        assert!(!force_path_style(&config(
            AddressingStyle::Auto,
            false,
            accelerate
        )));
    }
}
