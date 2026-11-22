//! The website command, which sets the website configuration of a bucket.

use std::process::ExitCode;

use aws_sdk_s3::types::{ErrorDocument, IndexDocument, WebsiteConfiguration};

use crate::api::send;
use crate::cli::{GlobalArgs, WebsiteArgs};
use crate::client;
use crate::error::Error;
use crate::path::validate_bucket_name;

pub async fn run(args: &WebsiteArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
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
    let configuration = configuration.build();
    send(&client, bucket, "PutBucketWebsite", |client| {
        client
            .put_bucket_website()
            .bucket(bucket)
            .website_configuration(configuration.clone())
            .send()
    })
    .await?;
    Ok(ExitCode::SUCCESS)
}
