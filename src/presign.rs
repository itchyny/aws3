use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use aws_credential_types::provider::ProvideCredentials;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sigv4::sign::v4::{calculate_signature, generate_signing_key};
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
    if expires_in != args.expires_in {
        let provider = config.sdk.credentials_provider();
        let credentials = match provider {
            Some(provider) => provider.provide_credentials().await.ok(),
            None => None,
        };
        url = credentials
            .and_then(|credentials| resign(&url, args.expires_in, credentials.secret_access_key()))
            .ok_or_else(|| {
                Error::param_validation("`expires_in` must be no longer than one week")
            })?;
    }
    println!("{}", signature_last(&url));
    Ok(ExitCode::SUCCESS)
}

/// Moves the signature to the end of the query like the AWS CLI, which the
/// SDK places before the security token of temporary credentials.
fn signature_last(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_owned();
    };
    let (signatures, mut params): (Vec<_>, Vec<_>) = query
        .split('&')
        .partition(|param| param.starts_with("X-Amz-Signature="));
    params.extend(signatures);
    format!("{base}?{}", params.join("&"))
}

/// Signs the pre-signed URL of Signature Version 4 again with the expiration,
/// which keeps the other parameters.
fn resign(url: &str, expires_in: i64, secret: &str) -> Option<String> {
    let (base, query) = url.split_once('?')?;
    let (scheme, rest) = base.split_once("://")?;
    let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let param = |name: &str| {
        query
            .split('&')
            .find_map(|param| param.strip_prefix(name)?.strip_prefix('='))
    };
    if param("X-Amz-Algorithm")? != "AWS4-HMAC-SHA256" || param("X-Amz-SignedHeaders")? != "host" {
        return None;
    }
    let date_time = param("X-Amz-Date")?;
    let credential = param("X-Amz-Credential")?.replace("%2F", "/");
    let (_, scope) = credential.split_once('/')?;
    let [date, region, service, _] = scope.split('/').collect::<Vec<_>>()[..] else {
        return None;
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
        "AWS4-HMAC-SHA256\n{date_time}\n{scope}\n{}",
        hex(&Sha256::digest(canonical_request))
    );
    let time = chrono::NaiveDate::parse_from_str(date, "%Y%m%d").ok()?;
    let time = SystemTime::from(time.and_hms_opt(0, 0, 0)?.and_utc());
    let key = generate_signing_key(secret, time, region, service);
    let signature = calculate_signature(key, string_to_sign.as_bytes());
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
    async fn presign(key: &str, expires_in: u64, token: Option<&str>) -> String {
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
            .bucket("bucket")
            .key(key)
            .presigned(presigning)
            .await
            .unwrap()
            .uri()
            .to_owned()
    }

    #[tokio::test]
    async fn resign_url() {
        let secret = "wJalrXUtnFI/K7MDENG/bPxRfiCYEXAMPLEKEY";
        for (key, token) in [("a b/c+ü~*.txt", None), ("key", Some("TOKEN/+="))] {
            // Signing again with the same expiration results in the same URL,
            // except for the position of the signature.
            let url = signature_last(&presign(key, 3600, token).await);
            assert_eq!(resign(&url, 3600, secret).as_deref(), Some(url.as_str()));
            let url = resign(&presign(key, 60, token).await, 3600, secret).unwrap();
            assert_eq!(url, signature_last(&presign(key, 3600, token).await));
        }
        let url = resign(&presign("key", 3600, None).await, -1, secret).unwrap();
        assert!(url.contains("&X-Amz-Expires=-1&"), "{url}");
    }
}
