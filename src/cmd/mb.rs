//! The mb command, which creates a bucket.

use std::io::{self, Write};
use std::process::ExitCode;

use aws_sdk_s3::types::{
    BucketLocationConstraint, BucketNamespace, CreateBucketConfiguration, Tag,
};

use crate::cli::{GlobalArgs, MbArgs};
use crate::client;
use crate::error::Error;
use crate::path::{is_s3express_bucket, split_bucket_key, validate_bucket_name};
use crate::redirect::Redirector;

pub async fn run(args: &MbArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let Some(path) = args.path.strip_prefix("s3://") else {
        return Err(Error::param_validation(
            "<S3Uri>\nError: Invalid argument type",
        ));
    };
    let (bucket, _) = split_bucket_key(path);
    if is_s3express_bucket(bucket) {
        return Err(Error::param_validation(
            "Cannot use mb command with a directory bucket.",
        ));
    }
    if let Err(message) = validate_bucket_name(bucket) {
        eprintln!("make_bucket failed: {} {message}", args.path);
        return Ok(ExitCode::FAILURE);
    }
    let config = client::load_config(global).await?;
    let client = client::new_client(&config, None);
    let region = client
        .config()
        .region()
        .map(|region| region.as_ref().to_owned())
        .unwrap_or_default();
    let mut configuration = CreateBucketConfiguration::builder();
    let mut has_configuration = false;
    // Sending the location constraint of us-east-1 results in an error.
    if region != "us-east-1" {
        configuration =
            configuration.location_constraint(BucketLocationConstraint::from(region.as_str()));
        has_configuration = true;
    }
    for [key, value] in args.tags.as_chunks::<2>().0 {
        let tag = Tag::builder()
            .key(key)
            .value(value)
            .build()
            .map_err(|err| Error::new(err.to_string()))?;
        configuration = configuration.tags(tag);
        has_configuration = true;
    }
    let configuration = has_configuration.then(|| configuration.build());
    let result = Redirector::default()
        .send(&client, bucket, "CreateBucket", |client| {
            client
                .create_bucket()
                .bucket(bucket)
                .set_bucket_namespace(
                    bucket
                        .ends_with("-an")
                        .then_some(BucketNamespace::AccountRegional),
                )
                .set_create_bucket_configuration(configuration.clone())
                .send()
        })
        .await;
    match result {
        Ok(_) => {
            let _ = writeln!(io::stdout(), "make_bucket: {bucket}");
            Ok(ExitCode::SUCCESS)
        }
        Err(err) => {
            eprintln!(
                "make_bucket failed: {} {}",
                args.path,
                Error::sdk("CreateBucket", err)
            );
            Ok(ExitCode::FAILURE)
        }
    }
}
