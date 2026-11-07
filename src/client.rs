use std::path::PathBuf;
use std::time::Duration;

use aws_config::{BehaviorVersion, SdkConfig};
use aws_credential_types::provider::ProvideCredentials;
use aws_credential_types::provider::error::CredentialsError;
use aws_runtime::auth::PayloadSigningOverride;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::config::timeout::TimeoutConfig;
use aws_sdk_s3::config::{ConfigBag, Intercept, RuntimeComponents};
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::{
    BeforeTransmitInterceptorContextMut, FinalizerInterceptorContextMut, InterceptorContext,
};
use aws_smithy_runtime_api::client::retries::RequestAttempts;
use aws_smithy_runtime_api::client::retries::classifiers::{ClassifyRetry, RetryAction};
use aws_smithy_types::retry::RetryConfig;

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
    // The credentials of `aws login` are resolved on creating the clients,
    // where the errors are reported before any requests like the AWS CLI.
    if let Some(session) = &profile.login_session
        && !global.no_sign_request
        && let Some(provider) = sdk.credentials_provider()
        && let Err(err) = provider.provide_credentials().await
        && !matches!(err, CredentialsError::CredentialsNotLoaded(_))
    {
        return Err(Error::login(session, &err));
    }
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

/// Disables the signing of the payloads of all the requests, which is enabled
/// over HTTP by default, for `payload_signing_enabled = false`.
#[derive(Debug)]
struct UnsignedPayload;

impl Intercept for UnsignedPayload {
    fn name(&self) -> &'static str {
        "UnsignedPayload"
    }

    fn modify_before_signing(
        &self,
        _context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _components: &RuntimeComponents,
        config: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // The streaming bodies with the checksum trailers remain chunked.
        let unsigned = match config.load::<PayloadSigningOverride>() {
            Some(PayloadSigningOverride::StreamingSignedPayloadTrailer) => {
                PayloadSigningOverride::StreamingUnsignedPayloadTrailer
            }
            _ => PayloadSigningOverride::unsigned_payload(),
        };
        config.interceptor_state().store_put(unsigned);
        Ok(())
    }
}

/// The retries of a request, which is added to the extensions of the last
/// response to report the errors like `(reached max retries: 2)`.
#[derive(Debug, Clone, Copy)]
pub struct Retries {
    pub retries: u32,
    pub max_attempts_reached: bool,
}

/// Records the retries of the requests to the responses.
#[derive(Debug)]
struct RecordRetries;

impl Intercept for RecordRetries {
    fn name(&self) -> &'static str {
        "RecordRetries"
    }

    fn modify_before_attempt_completion(
        &self,
        context: &mut FinalizerInterceptorContextMut<'_>,
        _components: &RuntimeComponents,
        config: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let attempts = config.load::<RequestAttempts>().map_or(1, |a| a.attempts());
        let max_attempts = config.load::<RetryConfig>().map_or(1, |c| c.max_attempts());
        if let Some(response) = context.response_mut() {
            response.add_extension(Retries {
                retries: attempts.saturating_sub(1),
                max_attempts_reached: attempts >= max_attempts,
            });
        }
        Ok(())
    }
}

/// Retries the errors of the responses with the 200 status code, e.g. the
/// bodies which are not XML or are errors, like `_handle_200_error` of
/// botocore which handles them as 500 errors.
#[derive(Debug)]
struct Retry200Errors;

impl ClassifyRetry for Retry200Errors {
    fn classify_retry(&self, ctx: &InterceptorContext) -> RetryAction {
        let is_200 = ctx
            .response()
            .is_some_and(|response| response.status().as_u16() == 200);
        if is_200 && matches!(ctx.output_or_error(), Some(Err(_))) {
            RetryAction::server_error()
        } else {
            RetryAction::NoActionIndicated
        }
    }

    fn name(&self) -> &'static str {
        "Retry200Errors"
    }
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
        .accelerate(config.s3.use_accelerate_endpoint)
        .disable_multi_region_access_points(config.s3.disable_multiregion_access_points)
        .interceptor(RecordRetries)
        .retry_classifier(Retry200Errors);
    builder.set_use_arn_region(config.s3.use_arn_region);
    if config.s3.payload_signing_enabled == Some(false) {
        builder = builder.interceptor(UnsignedPayload);
    }
    // Custom endpoints disable the dual-stack and FIPS endpoints like the AWS
    // CLI. The settings of the profile and the environment variables are
    // loaded by the SDK, which are overridden if enabled in the `s3` section.
    if config.endpoint_url.is_some() {
        builder = builder.use_dual_stack(false).use_fips(false);
    } else if config.s3.use_dualstack_endpoint {
        builder = builder.use_dual_stack(true);
    }
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
