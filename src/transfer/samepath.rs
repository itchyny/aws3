//! The validation of moving an object onto itself through access points, in
//! the same way as `--validate-same-s3-paths` and `S3PathResolver` of the AWS
//! CLI, which resolve the underlying buckets of the access points.

use std::sync::LazyLock;

use aws_sdk_s3::config::Region;
use regex_lite::Regex;

use super::is_same_path;
use crate::cli::GlobalArgs;
use crate::client;
use crate::error::Error;
use crate::path::split_bucket_key;

static ACCESS_POINT_ARN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^arn:aws.*:s3:[a-z0-9\-]+:(?P<account>[0-9]{12}):accesspoint[:/](?P<name>[a-z0-9\-]{3,50})$",
    )
    .expect("valid pattern")
});

static OUTPOSTS_ACCESS_POINT_ARN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^arn:aws.*:s3-outposts:[a-z0-9\-]+:(?P<account>[0-9]{12}):outpost/op-[a-zA-Z0-9]+/accesspoint[:/][a-z0-9\-]{3,50}$",
    )
    .expect("valid pattern")
});

static MULTI_REGION_ACCESS_POINT_ARN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^arn:aws:s3::(?P<account>[0-9]{12}):accesspoint[:/](?P<alias>[a-zA-Z0-9]+\.mrap)$")
        .expect("valid pattern")
});

/// Validates that the source and the destination of `mv` between S3 are not
/// the same object, or warns that they may be, like `_validate_path_args` of
/// the AWS CLI.
pub async fn validate(
    config: &client::Config,
    global: &GlobalArgs,
    source_region: Option<&str>,
    src: &str,
    dest: &str,
    enabled: bool,
) -> Result<(), Error> {
    let (_, src_key) = split_bucket_key(src);
    let (_, dest_key) = split_bucket_key(dest);
    if !is_same_path(&format!("/{src_key}"), &format!("/{dest_key}")) {
        return Ok(());
    }
    if !enabled {
        if has_underlying_path(src) || has_underlying_path(dest) {
            eprintln!(
                "warning: Provided s3 paths may resolve to same underlying s3 object(s) and \
                 result in deletion instead of being moved. To resolve and validate underlying \
                 s3 paths are not the same, specify the --validate-same-s3-paths flag or set \
                 the AWS_CLI_S3_MV_VALIDATE_SAME_S3_PATHS environment variable to true. To \
                 resolve s3 outposts access point path, the arn must be used instead of the \
                 alias."
            );
        }
        return Ok(());
    }
    let src_paths = Resolver::new(config, global, source_region)
        .resolve(src)
        .await?;
    let dest_paths = Resolver::new(config, global, None).resolve(dest).await?;
    for src_path in &src_paths {
        for dest_path in &dest_paths {
            if is_same_path(src_path, dest_path) {
                return Err(Error::param_validation(format!(
                    "Cannot mv a file onto itself: {src} - {dest}"
                )));
            }
        }
    }
    Ok(())
}

/// Returns whether the validation is enabled by the environment variable.
pub fn enabled_by_env() -> bool {
    std::env::var("AWS_CLI_S3_MV_VALIDATE_SAME_S3_PATHS")
        .is_ok_and(|value| value.eq_ignore_ascii_case("true"))
}

/// Returns whether the bucket of the path is an access point, whose
/// underlying bucket may be the bucket of the other path.
fn has_underlying_path(path: &str) -> bool {
    let (bucket, _) = split_bucket_key(path);
    ACCESS_POINT_ARN.is_match(bucket)
        || OUTPOSTS_ACCESS_POINT_ARN.is_match(bucket)
        || MULTI_REGION_ACCESS_POINT_ARN.is_match(bucket)
        || bucket.ends_with("-s3alias")
        || bucket.ends_with("--op-s3")
}

/// Resolves the underlying buckets of the access points by S3 Control and
/// STS, whose clients do not use the endpoint URL of the option like the AWS
/// CLI.
struct Resolver {
    control: aws_sdk_s3control::Client,
    sts: aws_sdk_sts::Client,
}

impl Resolver {
    fn new(config: &client::Config, global: &GlobalArgs, region: Option<&str>) -> Self {
        let endpoint_url = global
            .endpoint_url
            .as_ref()
            .and_then(|_| std::env::var("AWS_ENDPOINT_URL").ok());
        let mut control = aws_sdk_s3control::config::Builder::from(&config.sdk);
        if let Some(region) = region {
            control = control.region(Region::new(region.to_owned()));
        }
        let mut sts = aws_sdk_sts::config::Builder::from(&config.sdk);
        if global.endpoint_url.is_some() {
            control.set_endpoint_url(endpoint_url.clone());
            sts.set_endpoint_url(endpoint_url);
        }
        Self {
            control: aws_sdk_s3control::Client::from_conf(control.build()),
            sts: aws_sdk_sts::Client::from_conf(sts.build()),
        }
    }

    /// Returns the paths of the underlying buckets of the path, in the same
    /// way as `resolve_underlying_s3_paths` of the AWS CLI.
    async fn resolve(&self, path: &str) -> Result<Vec<String>, Error> {
        let (bucket, key) = split_bucket_key(path);
        let paths = |buckets: Vec<String>| {
            buckets
                .into_iter()
                .map(|bucket| format!("s3://{bucket}/{key}"))
                .collect()
        };
        if let Some(captures) = ACCESS_POINT_ARN.captures(bucket) {
            let bucket = self
                .access_point_bucket(&captures["account"], &captures["name"])
                .await?;
            return Ok(paths(vec![bucket]));
        }
        if let Some(captures) = OUTPOSTS_ACCESS_POINT_ARN.captures(bucket) {
            let bucket = self
                .access_point_bucket(&captures["account"], bucket)
                .await?;
            return Ok(paths(vec![bucket]));
        }
        if let Some(captures) = MULTI_REGION_ACCESS_POINT_ARN.captures(bucket) {
            let buckets = self
                .multi_region_access_point_buckets(&captures["account"], &captures["alias"])
                .await?;
            return Ok(paths(buckets));
        }
        if bucket.ends_with("-s3alias") {
            let output = self
                .sts
                .get_caller_identity()
                .send()
                .await
                .map_err(|err| Error::sdk("GetCallerIdentity", err))?;
            let account = output.account().unwrap_or_default();
            let bucket = self.access_point_bucket(account, bucket).await?;
            return Ok(paths(vec![bucket]));
        }
        if bucket.ends_with("--op-s3") {
            return Err(Error::param_validation(
                "Can't resolve underlying bucket name of s3 outposts access point alias. \
                 Use arn instead to resolve the bucket name and validate the mv command.",
            ));
        }
        Ok(vec![path.to_owned()])
    }

    async fn access_point_bucket(&self, account: &str, name: &str) -> Result<String, Error> {
        let output = self
            .control
            .get_access_point()
            .account_id(account)
            .name(name)
            .send()
            .await
            .map_err(|err| Error::sdk("GetAccessPoint", err))?;
        Ok(output.bucket().unwrap_or_default().to_owned())
    }

    async fn multi_region_access_point_buckets(
        &self,
        account: &str,
        alias: &str,
    ) -> Result<Vec<String>, Error> {
        let mut token = None;
        loop {
            let output = self
                .control
                .list_multi_region_access_points()
                .account_id(account)
                .set_next_token(token)
                .send()
                .await
                .map_err(|err| Error::sdk("ListMultiRegionAccessPoints", err))?;
            if let Some(access_point) = output
                .access_points()
                .iter()
                .find(|access_point| access_point.alias() == Some(alias))
            {
                return Ok(access_point
                    .regions()
                    .iter()
                    .filter_map(|region| region.bucket().map(str::to_owned))
                    .collect());
            }
            token = output.next_token().map(str::to_owned);
            if token.is_none() {
                return Err(Error::param_validation(format!(
                    "Couldn't find multi-region access point with alias {alias} \
                     in account {account}"
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn underlying_path() {
        let paths = [
            (
                "s3://arn:aws:s3:us-east-1:123456789012:accesspoint/my-ap/key",
                true,
            ),
            (
                "s3://arn:aws:s3:us-east-1:123456789012:accesspoint:my-ap/key",
                true,
            ),
            (
                "s3://arn:aws:s3:us-east-1:123456789012:accesspoint/MyAp/key",
                false,
            ),
            (
                "s3://arn:aws:s3::123456789012:accesspoint/mfzwi23gnjvgw.mrap/key",
                true,
            ),
            (
                "s3://arn:aws:s3-outposts:us-east-1:123456789012:outpost/op-0123/accesspoint/ap1/k",
                true,
            ),
            (
                "s3://my-ap-abcdefghijklmnopqrstuvwxyz0123-s3alias/key",
                true,
            ),
            ("s3://my-ap-abcdefghijklmnopqrstuvwxyz0123--op-s3/key", true),
            ("s3://bucket/key", false),
        ];
        for (path, expected) in paths {
            assert_eq!(has_underlying_path(path), expected, "{path}");
        }
    }
}
