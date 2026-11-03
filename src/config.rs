//! The configuration of the profile in the config file, which the SDK does
//! not load, e.g.
//!
//! ```ini
//! [default]
//! ca_bundle = /path/to/ca-bundle.pem
//! s3 =
//!   max_concurrent_requests = 20
//!   multipart_threshold = 64MB
//!   addressing_style = path
//!   use_accelerate_endpoint = true
//!   use_dualstack_endpoint = true
//! ```

use std::path::PathBuf;

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct ProfileConfig {
    /// The CA bundle to verify the certificates of the servers.
    pub ca_bundle: Option<PathBuf>,
    /// The settings in the `s3` section.
    pub s3: S3Config,
}

impl ProfileConfig {
    /// Loads the configuration of the profile from the config file, falling
    /// back to the defaults for missing or invalid values.
    pub fn load(profile: Option<&str>) -> Self {
        let mut config = Self::load_file(profile);
        // The environment variable and the setting of the profile override
        // the setting in the `s3` section.
        if let Ok(value) = std::env::var("AWS_S3_USE_ARN_REGION") {
            config.s3.use_arn_region = Some(value.eq_ignore_ascii_case("true"));
        }
        if let Ok(value) = std::env::var("AWS_S3_DISABLE_MULTIREGION_ACCESS_POINTS") {
            config.s3.disable_multiregion_access_points = value.eq_ignore_ascii_case("true");
        }
        config
    }

    fn load_file(profile: Option<&str>) -> Self {
        let Some(content) = config_file().and_then(|path| std::fs::read_to_string(path).ok())
        else {
            return Self::default();
        };
        let profile = profile
            .map(str::to_owned)
            .or_else(|| std::env::var("AWS_PROFILE").ok())
            .or_else(|| std::env::var("AWS_DEFAULT_PROFILE").ok())
            .unwrap_or_else(|| "default".to_owned());
        let settings = profile_settings(&content, &profile);
        let ca_bundle = settings
            .profile
            .iter()
            .find(|(key, _)| key == "ca_bundle")
            .map(|(_, value)| PathBuf::from(value))
            .filter(|path| !path.as_os_str().is_empty());
        let mut s3 = S3Config::from_settings(&settings.s3);
        if let Some((_, value)) = settings
            .profile
            .iter()
            .find(|(key, _)| key == "s3_use_arn_region")
        {
            s3.use_arn_region = Some(value.eq_ignore_ascii_case("true"));
        }
        Self { ca_bundle, s3 }
    }
}

#[derive(Debug, Clone)]
pub struct S3Config {
    pub multipart_threshold: u64,
    pub multipart_chunksize: u64,
    pub max_concurrent_requests: usize,
    pub max_queue_size: usize,
    pub addressing_style: AddressingStyle,
    pub use_accelerate_endpoint: bool,
    /// Whether the dual-stack endpoints are used, which falls back to the
    /// setting of the profile and the environment variable unless enabled.
    pub use_dualstack_endpoint: bool,
    /// Whether the region in the ARN of an access point is used instead of
    /// the region of the client, which defaults to true.
    pub use_arn_region: Option<bool>,
    /// Whether the ARNs of multi-region access points are rejected.
    pub disable_multiregion_access_points: bool,
    /// The maximum bandwidth of transfers, which is parsed by the transfer
    /// commands since other commands do not report invalid values.
    pub max_bandwidth: Option<String>,
}

/// The style of the URLs of buckets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressingStyle {
    /// The virtual-hosted style if the bucket name is DNS compatible, e.g.
    /// `https://bucket.s3.amazonaws.com/key`.
    Auto,
    /// The virtual-hosted style.
    Virtual,
    /// The path style, e.g. `https://s3.amazonaws.com/bucket/key`.
    Path,
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            multipart_threshold: 8 * MIB,
            multipart_chunksize: 8 * MIB,
            max_concurrent_requests: 10,
            max_queue_size: 1000,
            addressing_style: AddressingStyle::Auto,
            use_accelerate_endpoint: false,
            use_dualstack_endpoint: false,
            use_arn_region: None,
            disable_multiregion_access_points: false,
            max_bandwidth: None,
        }
    }
}

impl S3Config {
    /// Creates the configuration from the settings, falling back to the
    /// defaults for missing or invalid values.
    fn from_settings(settings: &[(String, String)]) -> Self {
        let mut config = Self::default();
        for (key, value) in settings {
            match key.as_str() {
                "multipart_threshold" => {
                    if let Some(value) = parse_size(value) {
                        config.multipart_threshold = value;
                    }
                }
                "multipart_chunksize" => {
                    if let Some(value) = parse_size(value) {
                        config.multipart_chunksize = value;
                    }
                }
                "max_concurrent_requests" => {
                    if let Some(value) = value.parse().ok().filter(|&value| value > 0) {
                        config.max_concurrent_requests = value;
                    }
                }
                "max_queue_size" => {
                    if let Some(value) = value.parse().ok().filter(|&value| value > 0) {
                        config.max_queue_size = value;
                    }
                }
                "addressing_style" => match value.as_str() {
                    "auto" => config.addressing_style = AddressingStyle::Auto,
                    "virtual" => config.addressing_style = AddressingStyle::Virtual,
                    "path" => config.addressing_style = AddressingStyle::Path,
                    _ => {}
                },
                "use_accelerate_endpoint" => {
                    config.use_accelerate_endpoint = value.eq_ignore_ascii_case("true");
                }
                "use_dualstack_endpoint" => {
                    config.use_dualstack_endpoint = value.eq_ignore_ascii_case("true");
                }
                "use_arn_region" => {
                    config.use_arn_region = Some(value.eq_ignore_ascii_case("true"));
                }
                "max_bandwidth" => config.max_bandwidth = Some(value.clone()),
                "s3_disable_multiregion_access_points" => {
                    config.disable_multiregion_access_points = value.eq_ignore_ascii_case("true");
                }
                _ => {}
            }
        }
        config
    }
}

fn config_file() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("AWS_CONFIG_FILE").filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".aws").join("config"))
}

/// The settings in the section of a profile.
#[derive(Debug, Default)]
struct Settings {
    profile: Vec<(String, String)>,
    s3: Vec<(String, String)>,
}

/// Returns the settings in the section of the profile, and the nested
/// settings of `s3`.
fn profile_settings(content: &str, profile: &str) -> Settings {
    let mut settings = Settings::default();
    let mut in_profile = false;
    let mut in_s3 = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(section) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let section = section.trim();
            let name = section
                .strip_prefix("profile ")
                .map(str::trim)
                .unwrap_or(section);
            in_profile =
                name == profile && (section.starts_with("profile ") || section == "default");
            in_s3 = false;
            continue;
        }
        if !in_profile {
            continue;
        }
        let indented = line.starts_with([' ', '\t']);
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if indented && in_s3 {
            settings.s3.push((key.to_owned(), value.to_owned()));
        } else {
            in_s3 = key == "s3" && value.is_empty();
            if !in_s3 {
                settings.profile.push((key.to_owned(), value.to_owned()));
            }
        }
    }
    settings
}

/// Parses the rate in bytes per second, e.g. `10MB/s`, or in bits per second,
/// e.g. `10Mb/s`, or an integer in bytes per second, in the same way as
/// `_convert_human_readable_rates` of the AWS CLI.
pub fn parse_rate(value: &str) -> Result<u64, String> {
    let rate = if let Some(size) = value.strip_suffix("B/s") {
        parse_rate_size(size, "B")
    } else if let Some(size) = value.strip_suffix("b/s") {
        parse_rate_size(size, "b").map(|bits| bits / 8)
    } else {
        value.parse().ok()
    };
    rate.ok_or_else(|| {
        format!(
            "Invalid rate: {value}. The value must be expressed as an integer in terms of bytes \
             per second (e.g. 10485760) or a rate in terms of bytes per second (e.g. 10MB/s or \
             800KB/s) or bits per second (e.g. 10Mb/s or 800Kb/s)"
        )
    })
}

/// Parses the size of a rate without `/s`, e.g. `1024` of `1024B/s` or `10MB`
/// of `10MB/s`, with the unit character.
fn parse_rate_size(size: &str, unit: &str) -> Option<u64> {
    size.parse()
        .ok()
        .or_else(|| parse_size(&format!("{size}{unit}")))
}

/// Parses the size with an optional suffix, e.g. `64MB` or `1GiB`, in the
/// same way as `human_readable_to_int` of the AWS CLI.
fn parse_size(value: &str) -> Option<u64> {
    let lower = value.trim().to_lowercase();
    let suffixes = [
        ("kib", 1024),
        ("mib", MIB),
        ("gib", 1024 * MIB),
        ("tib", 1024 * 1024 * MIB),
        ("kb", 1024),
        ("mb", MIB),
        ("gb", 1024 * MIB),
        ("tb", 1024 * 1024 * MIB),
    ];
    for (suffix, multiplier) in suffixes {
        if let Some(number) = lower.strip_suffix(suffix) {
            return number.trim().parse::<u64>().ok().map(|n| n * multiplier);
        }
    }
    lower.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings() {
        let content = r"[default]
region = us-east-1
s3 =
  max_concurrent_requests = 20
  multipart_threshold = 64MB

[profile other]
s3 =
  multipart_chunksize = 16MiB
";
        assert_eq!(
            profile_settings(content, "default").s3,
            [
                ("max_concurrent_requests".to_owned(), "20".to_owned()),
                ("multipart_threshold".to_owned(), "64MB".to_owned())
            ]
        );
        assert_eq!(
            profile_settings(content, "other").s3,
            [("multipart_chunksize".to_owned(), "16MiB".to_owned())]
        );
        assert_eq!(parse_size("64MB"), Some(64 * MIB));
        assert_eq!(parse_size("1GiB"), Some(1024 * MIB));
        assert_eq!(parse_size("1048576"), Some(MIB));
        assert_eq!(parse_size("x"), None);
    }

    #[test]
    fn profile() {
        let content = r"[default]
ca_bundle = /etc/default.pem
s3 =
  ca_bundle = /etc/nested.pem

[profile other]
region = us-west-2
ca_bundle = /etc/other.pem
";
        let settings = profile_settings(content, "default");
        assert_eq!(
            settings.profile,
            [("ca_bundle".to_owned(), "/etc/default.pem".to_owned())]
        );
        assert_eq!(
            profile_settings(content, "other").profile,
            [
                ("region".to_owned(), "us-west-2".to_owned()),
                ("ca_bundle".to_owned(), "/etc/other.pem".to_owned())
            ]
        );
        assert!(profile_settings(content, "missing").profile.is_empty());
    }

    #[test]
    fn rate() {
        assert_eq!(parse_rate("10485760"), Ok(10 * MIB));
        assert_eq!(parse_rate("1024B/s"), Ok(1024));
        assert_eq!(parse_rate("10MB/s"), Ok(10 * MIB));
        assert_eq!(parse_rate("800KB/s"), Ok(800 * 1024));
        assert_eq!(parse_rate("1MiB/s"), Ok(MIB));
        assert_eq!(parse_rate("10Mb/s"), Ok(10 * MIB / 8));
        assert_eq!(parse_rate("8000b/s"), Ok(1000));
        assert!(parse_rate("fast").is_err());
        assert!(parse_rate("10MB").is_err());
        assert!(parse_rate("10XB/s").is_err());
    }

    #[test]
    fn addressing_style() {
        let content = r"[default]
s3 =
  addressing_style = path
";
        let settings = profile_settings(content, "default").s3;
        assert_eq!(
            settings,
            [("addressing_style".to_owned(), "path".to_owned())]
        );
    }
}
