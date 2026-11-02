//! The configuration in the `s3` section of the profile, e.g.
//!
//! ```ini
//! [default]
//! s3 =
//!   max_concurrent_requests = 20
//!   multipart_threshold = 64MB
//!   addressing_style = path
//! ```

use std::path::PathBuf;

const MIB: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct S3Config {
    pub multipart_threshold: u64,
    pub multipart_chunksize: u64,
    pub max_concurrent_requests: usize,
    pub max_queue_size: usize,
    pub addressing_style: AddressingStyle,
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
        }
    }
}

impl S3Config {
    /// Loads the configuration of the profile from the config file, falling
    /// back to the defaults for missing or invalid values.
    pub fn load(profile: Option<&str>) -> Self {
        let mut config = Self::default();
        let Some(content) = config_file().and_then(|path| std::fs::read_to_string(path).ok())
        else {
            return config;
        };
        let profile = profile
            .map(str::to_owned)
            .or_else(|| std::env::var("AWS_PROFILE").ok())
            .or_else(|| std::env::var("AWS_DEFAULT_PROFILE").ok())
            .unwrap_or_else(|| "default".to_owned());
        for (key, value) in s3_settings(&content, &profile) {
            match key.as_str() {
                "multipart_threshold" => {
                    if let Some(value) = parse_size(&value) {
                        config.multipart_threshold = value;
                    }
                }
                "multipart_chunksize" => {
                    if let Some(value) = parse_size(&value) {
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

/// Returns the nested settings of `s3` in the section of the profile.
fn s3_settings(content: &str, profile: &str) -> Vec<(String, String)> {
    let mut settings = Vec::new();
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
            settings.push((key.to_owned(), value.to_owned()));
        } else {
            in_s3 = key == "s3" && value.is_empty();
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
            s3_settings(content, "default"),
            [
                ("max_concurrent_requests".to_owned(), "20".to_owned()),
                ("multipart_threshold".to_owned(), "64MB".to_owned())
            ]
        );
        assert_eq!(
            s3_settings(content, "other"),
            [("multipart_chunksize".to_owned(), "16MiB".to_owned())]
        );
        assert_eq!(parse_size("64MB"), Some(64 * MIB));
        assert_eq!(parse_size("1GiB"), Some(1024 * MIB));
        assert_eq!(parse_size("1048576"), Some(MIB));
        assert_eq!(parse_size("x"), None);
    }

    #[test]
    fn addressing_style() {
        let content = r"[default]
s3 =
  addressing_style = path
";
        let settings = s3_settings(content, "default");
        assert_eq!(
            settings,
            [("addressing_style".to_owned(), "path".to_owned())]
        );
    }
}
