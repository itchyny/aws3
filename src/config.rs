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
        Self {
            ca_bundle,
            s3: S3Config::from_settings(&settings.s3),
        }
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
