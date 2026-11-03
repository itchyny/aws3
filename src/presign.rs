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
    let config = client::load_config(global).await?;
    let client = client::new_client(&config, None);
    let presigning = PresigningConfig::expires_in(Duration::from_secs(args.expires_in))
        .map_err(|err| Error::param_validation(err.to_string()))?;
    let request = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .presigned(presigning)
        .await
        .map_err(|err| Error::sdk("GetObject", err))?;
    println!("{}", request.uri());
    Ok(ExitCode::SUCCESS)
}
