//! The rb command, which deletes a bucket.

use std::io::{self, Write};
use std::process::ExitCode;

use super::rm;
use crate::cli::{GlobalArgs, RbArgs};
use crate::client;
use crate::error::Error;
use crate::path::{split_bucket_key, validate_bucket_name};
use crate::redirect::Redirector;

pub async fn run(args: &RbArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let Some(path) = args.path.strip_prefix("s3://") else {
        return Err(Error::param_validation(
            "<S3Uri>\nError: Invalid argument type",
        ));
    };
    let (bucket, key) = split_bucket_key(path);
    if !key.is_empty() {
        return Err(Error::param_validation(format!(
            "Please specify a valid bucket name only. E.g. s3://{bucket}"
        )));
    }
    if args.force && rm::run_recursive(&args.path, global).await? != ExitCode::SUCCESS {
        return Err(Error::new(
            "remove_bucket failed: Unable to delete all objects in the bucket, bucket will not be deleted.",
        ));
    }
    if let Err(message) = validate_bucket_name(bucket) {
        eprintln!("remove_bucket failed: {} {message}", args.path);
        return Ok(ExitCode::FAILURE);
    }
    let config = client::load_config(global).await?;
    let client = client::new_client(&config, None);
    let result = Redirector::default()
        .send(&client, bucket, "DeleteBucket", |client| {
            client.delete_bucket().bucket(bucket).send()
        })
        .await;
    match result {
        Ok(_) => {
            let _ = writeln!(io::stdout(), "remove_bucket: {bucket}");
            Ok(ExitCode::SUCCESS)
        }
        Err(err) => {
            eprintln!(
                "remove_bucket failed: {} {}",
                args.path,
                Error::sdk("DeleteBucket", err)
            );
            Ok(ExitCode::FAILURE)
        }
    }
}
