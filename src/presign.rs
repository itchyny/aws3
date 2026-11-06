use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use aws_credential_types::provider::ProvideCredentials;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sigv4::sign::{v4, v4a};
use sha2::{Digest, Sha256};

use crate::cli::{GlobalArgs, PresignArgs};
use crate::client;
use crate::error::Error;
use crate::path::{split_bucket_key, validate_bucket_name};

/// The longest expiration which the SDK presigns.
const ONE_WEEK: i64 = 7 * 24 * 60 * 60;

/// Prints the pre-signed URL to get the object.
pub async fn run(args: &PresignArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let (bucket, key) = split_bucket_key(&args.path);
    validate_bucket_name(bucket).map_err(Error::param_validation)?;
    if key.is_empty() {
        return Err(Error::param_validation(
            "Parameter validation failed:\nInvalid length for parameter Key, value: 0, \
             valid min length: 1",
        ));
    }
    let config = client::load_config(global).await?;
    let client = client::new_client(&config, None);
    // The SDK presigns within a week, and the URL is signed again for the
    // other expirations, which the AWS CLI allows even if S3 rejects them.
    let expires_in = args.expires_in.clamp(0, ONE_WEEK);
    let presigning = PresigningConfig::expires_in(Duration::from_secs(expires_in as u64))
        .map_err(|err| Error::param_validation(err.to_string()))?;
    let request = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .customize()
        // The SDK adds the operation name to the query, which the AWS CLI
        // does not. It is removed before signing, which covers the query.
        .mutate_request(|request| {
            if let Some((path, query)) = request.uri().split_once('?') {
                let query: Vec<_> = query
                    .split('&')
                    .filter(|param| !param.starts_with("x-id="))
                    .collect();
                let uri = if query.is_empty() {
                    path.to_owned()
                } else {
                    format!("{path}?{}", query.join("&"))
                };
                let _ = request.set_uri(uri);
            }
        })
        .presigned(presigning)
        .await
        .map_err(|err| Error::sdk("GetObject", err))?;
    let mut url = request.uri().to_owned();
    // The AWS CLI fails without a message on the expiration not positive of
    // the URLs of Signature Version 4A, e.g. of Multi-Region Access Points.
    if args.expires_in <= 0 && url.contains(&format!("X-Amz-Algorithm={SIGV4A}&")) {
        return Err(Error::new(""));
    }
    if expires_in != args.expires_in {
        let provider = config.sdk.credentials_provider();
        let credentials = match provider {
            Some(provider) => provider.provide_credentials().await.ok(),
            None => None,
        };
        url = credentials
            .and_then(|credentials| {
                let (access_key, secret) =
                    (credentials.access_key_id(), credentials.secret_access_key());
                resign(&url, args.expires_in, access_key, secret)
            })
            .ok_or_else(|| {
                Error::param_validation("`expires_in` must be no longer than one week")
            })?;
    }
    println!("{}", reorder_params(&url));
    Ok(ExitCode::SUCCESS)
}

/// The algorithms of Signature Version 4 and 4A.
const SIGV4: &str = "AWS4-HMAC-SHA256";
const SIGV4A: &str = "AWS4-ECDSA-P256-SHA256";

/// Reorders the parameters of the query like the AWS CLI, where the signature
/// is at the end, which the SDK places before the security token. The order
/// of the parameters is not covered by the signature.
fn reorder_params(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_owned();
    };
    let mut params: Vec<_> = query.split('&').collect();
    // The order of the AWS CLI for Signature Version 4A, signed by CRT.
    const SIGV4A_ORDER: [&str; 7] = [
        "X-Amz-Algorithm",
        "X-Amz-Credential",
        "X-Amz-Date",
        "X-Amz-SignedHeaders",
        "X-Amz-Expires",
        "X-Amz-Security-Token",
        "X-Amz-Region-Set",
    ];
    let is_sigv4a = params.contains(&format!("X-Amz-Algorithm={SIGV4A}").as_str());
    params.sort_by_key(|param| {
        let name = param.split('=').next().unwrap_or_default();
        if name == "X-Amz-Signature" {
            usize::MAX
        } else if is_sigv4a {
            SIGV4A_ORDER
                .iter()
                .position(|order| *order == name)
                .unwrap_or(SIGV4A_ORDER.len())
        } else {
            0
        }
    });
    format!("{base}?{}", params.join("&"))
}

/// Signs the pre-signed URL of Signature Version 4 or 4A again with the
/// expiration, which keeps the other parameters.
fn resign(url: &str, expires_in: i64, access_key: &str, secret: &str) -> Option<String> {
    let (base, query) = url.split_once('?')?;
    let (scheme, rest) = base.split_once("://")?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let param = |name: &str| {
        query
            .split('&')
            .find_map(|param| param.strip_prefix(name)?.strip_prefix('='))
    };
    let algorithm = param("X-Amz-Algorithm")?;
    if !matches!(algorithm, SIGV4 | SIGV4A) || param("X-Amz-SignedHeaders")? != "host" {
        return None;
    }
    let date_time = param("X-Amz-Date")?;
    let credential = param("X-Amz-Credential")?.replace("%2F", "/");
    let (_, scope) = credential.split_once('/')?;
    // The scope of Signature Version 4A has no region.
    let parts: Vec<_> = scope.split('/').collect();
    let (date, region, service) = match parts[..] {
        [date, region, service, _] if algorithm == SIGV4 => (date, region, service),
        [date, service, _] if algorithm == SIGV4A => (date, "", service),
        _ => return None,
    };
    let params: Vec<_> = query
        .split('&')
        .map(|param| match param.split_once('=') {
            Some(("X-Amz-Expires", _)) => format!("X-Amz-Expires={expires_in}"),
            _ => param.to_owned(),
        })
        .collect();
    let mut sorted: Vec<_> = params
        .iter()
        .filter(|param| !param.starts_with("X-Amz-Signature="))
        .collect();
    sorted.sort_by(|a, b| a.split_once('=').cmp(&b.split_once('=')));
    let path = if path.is_empty() { "/" } else { path };
    let sorted: Vec<_> = sorted.into_iter().map(String::as_str).collect();
    let canonical_request = format!(
        "GET\n{path}\n{}\nhost:{host}\n\nhost\nUNSIGNED-PAYLOAD",
        sorted.join("&")
    );
    let string_to_sign = format!(
        "{algorithm}\n{date_time}\n{scope}\n{}",
        hex(&Sha256::digest(canonical_request))
    );
    let signature = if algorithm == SIGV4A {
        let key = v4a::generate_signing_key(access_key, secret);
        v4a::calculate_signature(key, string_to_sign.as_bytes())
    } else {
        let time = chrono::NaiveDate::parse_from_str(date, "%Y%m%d").ok()?;
        let time = SystemTime::from(time.and_hms_opt(0, 0, 0)?.and_utc());
        let key = v4::generate_signing_key(secret, time, region, service);
        v4::calculate_signature(key, string_to_sign.as_bytes())
    };
    let params: Vec<_> = params
        .into_iter()
        .filter(|param| !param.starts_with("X-Amz-Signature="))
        .collect();
    Some(format!(
        "{scheme}://{host}{path}?{}&X-Amz-Signature={signature}",
        params.join("&")
    ))
}

/// Encodes the bytes in lowercase hexadecimal digits.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the URL pre-signed by the SDK without network, at the time.
    async fn presign(bucket: &str, key: &str, expires_in: u64, token: Option<&str>) -> String {
        let credentials = aws_credential_types::Credentials::new(
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            token.map(str::to_owned),
            None,
            "test",
        );
        let config = aws_sdk_s3::Config::builder()
            .behavior_version_latest()
            .region(aws_sdk_s3::config::Region::new("ap-northeast-1"))
            .credentials_provider(credentials)
            .build();
        let presigning = PresigningConfig::builder()
            .start_time(SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000))
            .expires_in(Duration::from_secs(expires_in))
            .build()
            .unwrap();
        aws_sdk_s3::Client::from_conf(config)
            .get_object()
            .bucket(bucket)
            .key(key)
            .presigned(presigning)
            .await
            .unwrap()
            .uri()
            .to_owned()
    }

    #[tokio::test]
    async fn resign_url() {
        let (access_key, secret) = (
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        );
        let resign = |url: &str, expires_in| {
            resign(url, expires_in, access_key, secret).map(|url| reorder_params(&url))
        };
        // The signatures of both versions are deterministic, where the
        // Multi-Region Access Points are signed with Signature Version 4A.
        let mrap = "arn:aws:s3::123456789012:accesspoint/mfzwi23gnjvgw.mrap";
        for (bucket, key, token) in [
            ("bucket", "a b/c+ü~*.txt", None),
            ("bucket", "key", Some("TOKEN/+=")),
            (mrap, "a b/c+ü~*.txt", None),
            (mrap, "key", Some("TOKEN/+=")),
        ] {
            // Signing again with the same expiration results in the same URL,
            // except for the order of the parameters.
            let url = reorder_params(&presign(bucket, key, 3600, token).await);
            assert_eq!(
                resign(&url, 3600).as_deref(),
                Some(url.as_str()),
                "{bucket}"
            );
            let url = resign(&presign(bucket, key, 60, token).await, 3600).unwrap();
            let expected = reorder_params(&presign(bucket, key, 3600, token).await);
            assert_eq!(url, expected, "{bucket}");
        }
        let url = resign(&presign("bucket", "key", 3600, None).await, -1).unwrap();
        assert!(url.contains("&X-Amz-Expires=-1&"), "{url}");
    }
}
