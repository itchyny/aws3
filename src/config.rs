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
    /// The session of `aws login`, e.g. `arn:aws:iam::0123456789012:user/Admin`.
    pub login_session: Option<String>,
    /// The format of the errors, e.g. `json`.
    pub cli_error_format: Option<String>,
    /// The format of the outputs of the api commands, e.g. `text`.
    pub output: Option<String>,
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
        let profile = selected_profile(profile).unwrap_or_else(|| "default".to_owned());
        let settings = profile_settings(&content, &profile);
        let ca_bundle = settings
            .get("ca_bundle")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty());
        let mut s3 = S3Config::from_settings(&settings.s3);
        if let Some(value) = settings.get("s3_use_arn_region") {
            s3.use_arn_region = Some(value.eq_ignore_ascii_case("true"));
        }
        Self {
            ca_bundle,
            s3,
            login_session: settings.get("login_session").map(str::to_owned),
            cli_error_format: settings.get("cli_error_format").map(str::to_owned),
            output: settings.get("output").map(str::to_owned),
        }
    }
}

#[derive(Debug, Clone)]
pub struct S3Config {
    pub multipart_threshold: u64,
    pub multipart_chunksize: u64,
    pub max_concurrent_requests: usize,
    /// The maximum number of the parts of a multipart transfer in progress,
    /// which is fewer than the requests by default, since the parts of a large
    /// file use the network bandwidth with fewer requests and less CPU.
    pub max_concurrent_parts: usize,
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
    /// The maximum bandwidth of transfers in bytes per second.
    pub max_bandwidth: Option<u64>,
    /// Whether the payloads of the requests are signed, which is disabled by
    /// `false`, otherwise decided by the SDK.
    pub payload_signing_enabled: Option<bool>,
    /// The error of the invalid transfer settings, which is reported by the
    /// transfer commands since other commands do not validate them.
    pub invalid: Option<String>,
    /// The error of the invalid addressing style, which is reported by all
    /// the commands.
    pub invalid_addressing_style: Option<String>,
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
            // The requests are more concurrent than 10 of the AWS CLI, which
            // is slow for many small files.
            max_concurrent_requests: 64,
            max_concurrent_parts: 10,
            max_queue_size: 1000,
            addressing_style: AddressingStyle::Auto,
            use_accelerate_endpoint: false,
            use_dualstack_endpoint: false,
            use_arn_region: None,
            disable_multiregion_access_points: false,
            max_bandwidth: None,
            payload_signing_enabled: None,
            invalid: None,
            invalid_addressing_style: None,
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
                "addressing_style" => match value.as_str() {
                    "auto" => config.addressing_style = AddressingStyle::Auto,
                    "virtual" => config.addressing_style = AddressingStyle::Virtual,
                    "path" => config.addressing_style = AddressingStyle::Path,
                    _ => {
                        config.invalid_addressing_style = Some(format!(
                            "S3 addressing style {value} is invalid. \
                             Valid options are: 'auto', 'virtual', and 'path'"
                        ));
                    }
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
                // Only `true` is true like `ensure_boolean` of the AWS CLI.
                "payload_signing_enabled" => {
                    config.payload_signing_enabled = Some(value.eq_ignore_ascii_case("true"));
                }
                "s3_disable_multiregion_access_points" => {
                    config.disable_multiregion_access_points = value.eq_ignore_ascii_case("true");
                }
                _ => {}
            }
        }
        if let Err(err) = config.set_transfer_settings(settings) {
            config.invalid = Some(err);
        }
        config
    }

    /// Converts and validates the transfer settings in the same order as
    /// `RuntimeConfig.build_config` of the AWS CLI, which reports the first
    /// error.
    fn set_transfer_settings(&mut self, settings: &[(String, String)]) -> Result<(), String> {
        let get = |name: &str| {
            settings
                .iter()
                .rev()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        let size = |name| get(name).map(human_readable_to_int).transpose();
        let rate = |name| get(name).map(parse_rate).transpose();
        let multipart_chunksize = size("multipart_chunksize")?;
        let multipart_threshold = size("multipart_threshold")?;
        let io_chunksize = size("io_chunksize")?;
        let max_bandwidth = rate("max_bandwidth")?;
        let target_bandwidth = rate("target_bandwidth")?;
        let disk_throughput = rate("disk_throughput")?;
        let positive = |name: &str, value: Option<i64>| match value {
            Some(value) if value <= 0 => Err(positive_error(name, value)),
            _ => Ok(value.map(|value| value as u64)),
        };
        let positive_str = |name: &str| {
            get(name)
                .map(|value| {
                    python_int(value)
                        .filter(|&value| value > 0)
                        .map(|value| value as u64)
                        .ok_or_else(|| positive_error(name, value))
                })
                .transpose()
        };
        let multipart_chunksize = positive("multipart_chunksize", multipart_chunksize)?;
        let multipart_threshold = positive("multipart_threshold", multipart_threshold)?;
        let max_concurrent_requests = positive_str("max_concurrent_requests")?;
        let max_queue_size = positive_str("max_queue_size")?;
        let max_bandwidth = positive("max_bandwidth", max_bandwidth)?;
        positive("target_bandwidth", target_bandwidth)?;
        positive("io_chunksize", io_chunksize)?;
        positive("disk_throughput", disk_throughput)?;
        if let Some(value) = get("preferred_transfer_client")
            && !matches!(value, "auto" | "classic" | "crt" | "default")
        {
            return Err(format!(
                "Invalid value: \"{value}\" for configuration option: \"preferred_transfer_client\". \
                 Supported values are: auto, classic, crt"
            ));
        }
        let or = |value: Option<u64>, default| value.unwrap_or(default);
        self.multipart_chunksize = or(multipart_chunksize, self.multipart_chunksize);
        self.multipart_threshold = or(multipart_threshold, self.multipart_threshold);
        self.max_concurrent_requests =
            or(max_concurrent_requests, self.max_concurrent_requests as u64) as usize;
        // The explicit setting is applied to the parts as well.
        self.max_concurrent_parts =
            or(max_concurrent_requests, self.max_concurrent_parts as u64) as usize;
        self.max_queue_size = or(max_queue_size, self.max_queue_size as u64) as usize;
        self.max_bandwidth = max_bandwidth;
        Ok(())
    }
}

fn positive_error(name: &str, value: impl std::fmt::Display) -> String {
    format!("Value for {name} must be a positive integer: {value}")
}

/// Returns the profile specified explicitly by the option or `AWS_PROFILE`.
fn selected_profile(profile: Option<&str>) -> Option<String> {
    profile
        .map(str::to_owned)
        .or_else(|| std::env::var("AWS_PROFILE").ok())
}

/// Returns the profile specified explicitly by the option or `AWS_PROFILE` if
/// it is in neither the config file nor the credentials file, which the AWS
/// CLI rejects even when the credentials are not needed.
pub fn missing_profile(profile: Option<&str>) -> Option<String> {
    let profile = selected_profile(profile)?;
    (!profile_exists(&profile)).then_some(profile)
}

/// Returns the error of the invalid configuration of the credentials, which
/// the AWS CLI reports on resolving the credentials before any requests: the
/// access key in the environment variables without the secret key, unless
/// the profile is specified by the option, or the source profile of the role
/// of the profile not found.
pub fn credentials_error(profile: Option<&str>) -> Option<String> {
    if profile.is_none() {
        let is_set = |name: &str| std::env::var_os(name).is_some_and(|value| !value.is_empty());
        match (is_set("AWS_ACCESS_KEY_ID"), is_set("AWS_SECRET_ACCESS_KEY")) {
            (true, false) => {
                return Some(
                    "Partial credentials found in env, missing: AWS_SECRET_ACCESS_KEY".to_owned(),
                );
            }
            (true, true) => return None,
            _ => {}
        }
    }
    let profile = selected_profile(profile).unwrap_or_else(|| "default".to_owned());
    let content = read_file(config_file())?;
    let settings = profile_settings(&content, &profile);
    let source = settings.get("source_profile")?;
    (settings.get("role_arn").is_some() && !profile_exists(source)).then(|| {
        format!("The source_profile \"{source}\" referenced in the profile \"{profile}\" does not exist.")
    })
}

/// Returns whether the profile is in the config file or the credentials file.
fn profile_exists(profile: &str) -> bool {
    let credentials = aws_file("AWS_SHARED_CREDENTIALS_FILE", "credentials");
    read_file(config_file()).is_some_and(|content| profile_settings(&content, profile).found)
        || read_file(credentials).is_some_and(|content| {
            content.lines().any(|line| {
                line.trim()
                    .strip_prefix('[')
                    .and_then(|line| line.strip_suffix(']'))
                    .is_some_and(|section| section.trim() == profile)
            })
        })
}

fn read_file(path: Option<PathBuf>) -> Option<String> {
    path.and_then(|path| std::fs::read_to_string(path).ok())
}

fn config_file() -> Option<PathBuf> {
    aws_file("AWS_CONFIG_FILE", "config")
}

fn aws_file(name: &str, file: &str) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(name).filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let home = aws_runtime::fs_util::home_dir(
        &aws_types::os_shim_internal::Env::real(),
        aws_runtime::fs_util::Os::real(),
    )?;
    Some(PathBuf::from(home).join(".aws").join(file))
}

/// The settings in the section of a profile.
#[derive(Debug, Default)]
struct Settings {
    /// Whether the section of the profile is found.
    found: bool,
    profile: Vec<(String, String)>,
    s3: Vec<(String, String)>,
}

impl Settings {
    /// Returns the value of the setting in the section of the profile.
    fn get(&self, name: &str) -> Option<&str> {
        let mut settings = self.profile.iter();
        settings.find_map(|(key, value)| (key == name).then_some(value.as_str()))
    }
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
            settings.found |= in_profile;
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
fn parse_rate(value: &str) -> Result<i64, String> {
    // The size is without `B/s` if it is an integer, e.g. `1024` of
    // `1024B/s`, or without `/s` otherwise, e.g. `10MB` of `10MB/s`.
    let rate_to_int = |value: &str| {
        let size = &value[..value.len() - 3];
        if python_int(size).is_some() {
            human_readable_to_int(size)
        } else {
            human_readable_to_int(&value[..value.len() - 2])
        }
    };
    if value.ends_with("B/s") {
        rate_to_int(value)
    } else if value.ends_with("b/s") {
        rate_to_int(value).map(|bits| bits / 8)
    } else if let Some(rate) = python_int(value) {
        Ok(rate)
    } else {
        Err(format!(
            "Invalid rate: {value}. The value must be expressed as an integer in terms of bytes \
             per second (e.g. 10485760) or a rate in terms of bytes per second (e.g. 10MB/s or \
             800KB/s) or bits per second (e.g. 10Mb/s or 800Kb/s)"
        ))
    }
}

/// Parses the size with an optional suffix, e.g. `64MB` or `1GiB`, in the
/// same way as `human_readable_to_int` of the AWS CLI.
fn human_readable_to_int(value: &str) -> Result<i64, String> {
    let value = value.to_lowercase();
    let suffixes = [
        ("kb", 1 << 10),
        ("mb", 1 << 20),
        ("gb", 1 << 30),
        ("tb", 1 << 40),
        ("kib", 1 << 10),
        ("mib", 1 << 20),
        ("gib", 1 << 30),
        ("tib", 1 << 40),
    ];
    let length = if value.ends_with("ib") { 3 } else { 2 };
    let start = value
        .char_indices()
        .rev()
        .nth(length - 1)
        .map_or(0, |(index, _)| index);
    let suffix = &value[start..];
    match suffixes.iter().find(|(name, _)| *name == suffix) {
        Some((_, multiplier)) if value.chars().count() >= 2 => {
            python_int(&value[..start]).map(|number| number.saturating_mul(*multiplier))
        }
        _ => python_int(&value),
    }
    .ok_or_else(|| format!("Invalid size value: {value}"))
}

/// Formats the size in base-2 units, in the same way as the AWS CLI.
pub fn human_readable_size(size: i64) -> String {
    const BASE: f64 = 1024.0;
    const SUFFIXES: [&str; 6] = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let bytes = size as f64;
    if size == 1 {
        return "1 Byte".to_owned();
    } else if bytes < BASE {
        return format!("{size} Bytes");
    }
    for (i, suffix) in SUFFIXES.into_iter().enumerate() {
        let unit = BASE.powi(i as i32 + 2);
        if (bytes / unit * BASE).round_ties_even() < BASE || i == SUFFIXES.len() - 1 {
            return format!("{:.1} {suffix}", BASE * bytes / unit);
        }
    }
    unreachable!()
}

/// Parses the integer in the same way as `int` of Python, which allows the
/// surrounding whitespaces, the sign and the underscores between the digits.
fn python_int(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'_')
    {
        return None;
    }
    trimmed.replace('_', "").parse().ok()
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
        assert!(profile_settings(content, "other").found);
        assert!(!profile_settings(content, "missing").found);
        assert!(profile_settings(content, "missing").profile.is_empty());
        // The profiles other than the default need the prefix.
        assert!(!profile_settings("[other]\nregion = us-west-2\n", "other").found);
    }

    #[test]
    fn size() {
        assert_eq!(human_readable_to_int("64MB"), Ok(64 << 20));
        assert_eq!(human_readable_to_int("1GiB"), Ok(1 << 30));
        assert_eq!(human_readable_to_int("1048576"), Ok(1 << 20));
        assert_eq!(human_readable_to_int(" 1_024 kb"), Ok(1 << 20));
        assert_eq!(
            human_readable_to_int("ABC"),
            Err("Invalid size value: abc".to_owned())
        );
        assert_eq!(
            human_readable_to_int("1.5MB"),
            Err("Invalid size value: 1.5mb".to_owned())
        );
        assert_eq!(
            human_readable_to_int("5b"),
            Err("Invalid size value: 5b".to_owned())
        );
        assert_eq!(
            human_readable_to_int("kb"),
            Err("Invalid size value: kb".to_owned())
        );
        assert_eq!(
            human_readable_to_int("99999999999999999999kb"),
            Err("Invalid size value: 99999999999999999999kb".to_owned())
        );
    }

    #[test]
    fn human_readable() {
        assert_eq!(human_readable_size(0), "0 Bytes");
        assert_eq!(human_readable_size(1), "1 Byte");
        assert_eq!(human_readable_size(1023), "1023 Bytes");
        assert_eq!(human_readable_size(1024), "1.0 KiB");
        assert_eq!(human_readable_size(1536000), "1.5 MiB");
        assert_eq!(human_readable_size(1024 * 1024 - 1), "1.0 MiB");
        assert_eq!(human_readable_size(1024 * 1024 - 512), "1.0 MiB");
        assert_eq!(human_readable_size(1024 * 1024 - 513), "1023.5 KiB");
        assert_eq!(human_readable_size(5 << 40), "5.0 TiB");
        assert_eq!(human_readable_size(i64::MAX), "8.0 EiB");
    }

    #[test]
    fn rate() {
        assert_eq!(parse_rate("10485760"), Ok(10 << 20));
        assert_eq!(parse_rate("1024B/s"), Ok(1024));
        assert_eq!(parse_rate("10MB/s"), Ok(10 << 20));
        assert_eq!(parse_rate("800KB/s"), Ok(800 << 10));
        assert_eq!(parse_rate("1MiB/s"), Ok(1 << 20));
        assert_eq!(parse_rate("10Mb/s"), Ok((10 << 20) / 8));
        assert_eq!(parse_rate("8000b/s"), Ok(1000));
        assert_eq!(parse_rate("0B/s"), Ok(0));
        assert!(
            parse_rate("fast")
                .unwrap_err()
                .starts_with("Invalid rate: fast. ")
        );
        assert!(parse_rate("10MB").is_err());
        assert_eq!(
            parse_rate("10XB/s"),
            Err("Invalid size value: 10xb".to_owned())
        );
    }

    #[test]
    fn transfer_settings() {
        let config =
            |content: &str| S3Config::from_settings(&profile_settings(content, "default").s3);
        let valid = config(
            r"[default]
s3 =
  multipart_chunksize = 16MB
  multipart_threshold = 1GiB
  max_concurrent_requests = 20
  max_bandwidth = 10MB/s
  preferred_transfer_client = classic
",
        );
        assert_eq!(valid.invalid, None);
        assert_eq!(valid.multipart_chunksize, 16 * MIB);
        assert_eq!(valid.multipart_threshold, 1024 * MIB);
        assert_eq!(valid.max_concurrent_requests, 20);
        assert_eq!(valid.max_concurrent_parts, 20);
        // The parts are fewer than the requests by default.
        let default = config("[default]\n");
        assert_eq!(default.max_concurrent_requests, 64);
        assert_eq!(default.max_concurrent_parts, 10);
        assert_eq!(valid.max_bandwidth, Some(10 * MIB));
        for (setting, error) in [
            (
                "multipart_chunksize = 0",
                "Value for multipart_chunksize must be a positive integer: 0",
            ),
            (
                "multipart_threshold = -1MB",
                "Value for multipart_threshold must be a positive integer: -1048576",
            ),
            (
                "max_concurrent_requests = x",
                "Value for max_concurrent_requests must be a positive integer: x",
            ),
            (
                "max_queue_size = 0",
                "Value for max_queue_size must be a positive integer: 0",
            ),
            (
                "max_bandwidth = 0B/s",
                "Value for max_bandwidth must be a positive integer: 0",
            ),
            ("io_chunksize = abc", "Invalid size value: abc"),
            (
                "preferred_transfer_client = fast",
                r#"Invalid value: "fast" for configuration option: "preferred_transfer_client". Supported values are: auto, classic, crt"#,
            ),
        ] {
            let config = config(&format!("[default]\ns3 =\n  {setting}\n"));
            assert_eq!(config.invalid.as_deref(), Some(error), "{setting}");
        }
        // The sizes are converted before the integers are validated.
        let config = config(
            r"[default]
s3 =
  max_concurrent_requests = 0
  multipart_chunksize = x
",
        );
        assert_eq!(config.invalid.as_deref(), Some("Invalid size value: x"));
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
        let config = S3Config::from_settings(&settings);
        assert_eq!(config.addressing_style, AddressingStyle::Path);
        assert_eq!(config.invalid_addressing_style, None);
        // Only the lowercase values are valid like the AWS CLI.
        for value in ["Path", "path-style", ""] {
            let settings = [("addressing_style".to_owned(), value.to_owned())];
            let config = S3Config::from_settings(&settings);
            assert_eq!(
                config.invalid_addressing_style,
                Some(format!(
                    "S3 addressing style {value} is invalid. \
                     Valid options are: 'auto', 'virtual', and 'path'"
                ))
            );
        }
    }
}
