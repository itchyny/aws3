//! The bucket commands, i.e. mb, rb and website.

use std::process::ExitCode;

use aws_sdk_s3::types::{
    BucketLocationConstraint, BucketNamespace, CreateBucketConfiguration, ErrorDocument,
    IndexDocument, Tag, WebsiteConfiguration,
};

use crate::cli::{GlobalArgs, MbArgs, RbArgs, WebsiteArgs};
use crate::client;
use crate::error::Error;
use crate::path::{split_bucket_key, validate_bucket_name};
use crate::transfer;

pub async fn mb(args: &MbArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let Some(path) = args.path.strip_prefix("s3://") else {
        return Err(Error::param_validation(
            "<S3Uri>\nError: Invalid argument type",
        ));
    };
    let (bucket, _) = split_bucket_key(path);
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
    let result = client
        .create_bucket()
        .bucket(bucket)
        .set_bucket_namespace(
            bucket
                .ends_with("-an")
                .then_some(BucketNamespace::AccountRegional),
        )
        .set_create_bucket_configuration(has_configuration.then(|| configuration.build()))
        .send()
        .await;
    match result {
        Ok(_) => {
            println!("make_bucket: {bucket}");
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

pub async fn rb(args: &RbArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
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
    if args.force && transfer::rm_recursive(&args.path, global).await? != ExitCode::SUCCESS {
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
    match client.delete_bucket().bucket(bucket).send().await {
        Ok(_) => {
            println!("remove_bucket: {bucket}");
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

pub async fn website(args: &WebsiteArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let path = args.path.strip_prefix("s3://").unwrap_or(&args.path);
    let bucket = path.strip_suffix('/').unwrap_or(path);
    validate_bucket_name(bucket).map_err(Error::param_validation)?;
    let mut configuration = WebsiteConfiguration::builder();
    if let Some(suffix) = &args.index_document {
        let document = IndexDocument::builder()
            .suffix(suffix)
            .build()
            .map_err(|err| Error::new(err.to_string()))?;
        configuration = configuration.index_document(document);
    }
    if let Some(key) = &args.error_document {
        let document = ErrorDocument::builder()
            .key(key)
            .build()
            .map_err(|err| Error::new(err.to_string()))?;
        configuration = configuration.error_document(document);
    }
    let config = client::load_config(global).await?;
    let client = client::new_client(&config, None);
    client
        .put_bucket_website()
        .bucket(bucket)
        .website_configuration(configuration.build())
        .send()
        .await
        .map_err(|err| Error::sdk("PutBucketWebsite", err))?;
    Ok(ExitCode::SUCCESS)
}
