use std::process::ExitCode;
use std::time::Duration;

use aws_sdk_s3::presigning::PresigningConfig;

use crate::cli::{GlobalArgs, PresignArgs};
use crate::client;
use crate::error::Error;
use crate::path::{split_bucket_key, validate_bucket_name};

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
    let presigning = PresigningConfig::expires_in(Duration::from_secs(args.expires_in))
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
    println!("{}", request.uri());
    Ok(ExitCode::SUCCESS)
}
