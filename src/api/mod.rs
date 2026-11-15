//! The api command, which calls the operations of the S3 API in the same way
//! as `aws s3api`, and prints the responses in the same format.

mod abort_multipart_upload;
mod args;
mod copy_object;
mod create_bucket;
mod delete_bucket;
mod delete_bucket_cors;
mod delete_bucket_encryption;
mod delete_bucket_lifecycle;
mod delete_bucket_policy;
mod delete_bucket_tagging;
mod delete_object;
mod delete_object_tagging;
mod delete_objects;
mod delete_public_access_block;
mod get_bucket_acl;
mod get_bucket_cors;
mod get_bucket_encryption;
mod get_bucket_lifecycle_configuration;
mod get_bucket_location;
mod get_bucket_logging;
mod get_bucket_notification_configuration;
mod get_bucket_policy;
mod get_bucket_tagging;
mod get_bucket_versioning;
mod get_object;
mod get_object_tagging;
mod get_public_access_block;
mod head_bucket;
mod head_object;
mod jmespath;
mod list_buckets;
mod list_multipart_uploads;
mod list_object_versions;
mod list_objects_v2;
mod members;
mod output;
mod paginate;
mod paramfile;
mod put_bucket_acl;
mod put_bucket_cors;
mod put_bucket_encryption;
mod put_bucket_lifecycle_configuration;
mod put_bucket_logging;
mod put_bucket_notification_configuration;
mod put_bucket_policy;
mod put_bucket_tagging;
mod put_bucket_versioning;
mod put_object;
mod put_object_tagging;
mod put_public_access_block;
mod response;
mod restore_object;
pub mod shape;
mod text;

use std::io::{self, IsTerminal};
use std::process::ExitCode;

use crate::cli::{ApiArgs, ApiOperation, Color, GlobalArgs};
use crate::client;
use crate::error::Error;
use jmespath::Expression;
use output::{output_format, print};

pub async fn run(args: &ApiArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let loaded = paramfile::load()?;
    let args = loaded.as_ref().unwrap_or(args);
    let format = output_format(global)?;
    let query = global
        .query
        .as_deref()
        .map(|query| {
            Expression::compile(query).map_err(|err| {
                Error::param_validation(format!("Bad value for --query {query}: {err}"))
            })
        })
        .transpose()?;
    let config = client::load_config(global).await?;
    let client = client::new_client(&config, None);
    let responses = match &args.operation {
        ApiOperation::HeadObject(args) => head_object::run(&client, args).await?.into(),
        ApiOperation::GetObject(args) => get_object::run(&client, args).await?.into(),
        ApiOperation::PutObject(args) => put_object::run(&config, &client, args).await?.into(),
        ApiOperation::CopyObject(args) => copy_object::run(&client, args).await?.into(),
        ApiOperation::DeleteObject(args) => delete_object::run(&client, args).await?.into(),
        ApiOperation::DeleteObjects(args) => delete_objects::run(&client, args).await?.into(),
        ApiOperation::RestoreObject(args) => restore_object::run(&client, args).await?.into(),
        ApiOperation::ListObjectsV2(args) => list_objects_v2::run(&client, args, global).await?,
        ApiOperation::ListObjectVersions(args) => {
            list_object_versions::run(&client, args, global).await?
        }
        ApiOperation::ListMultipartUploads(args) => {
            list_multipart_uploads::run(&client, args, global).await?
        }
        ApiOperation::AbortMultipartUpload(args) => {
            abort_multipart_upload::run(&client, args).await?.into()
        }
        ApiOperation::ListBuckets(args) => list_buckets::run(&client, args, global).await?,
        ApiOperation::HeadBucket(args) => head_bucket::run(&client, args).await?.into(),
        ApiOperation::GetBucketLocation(args) => {
            get_bucket_location::run(&client, args).await?.into()
        }
        ApiOperation::CreateBucket(args) => create_bucket::run(&client, args).await?.into(),
        ApiOperation::DeleteBucket(args) => delete_bucket::run(&client, args).await?.into(),
        ApiOperation::GetBucketVersioning(args) => {
            get_bucket_versioning::run(&client, args).await?.into()
        }
        ApiOperation::PutBucketVersioning(args) => {
            put_bucket_versioning::run(&client, args).await?.into()
        }
        ApiOperation::GetPublicAccessBlock(args) => {
            get_public_access_block::run(&client, args).await?.into()
        }
        ApiOperation::PutPublicAccessBlock(args) => {
            put_public_access_block::run(&client, args).await?.into()
        }
        ApiOperation::DeletePublicAccessBlock(args) => {
            delete_public_access_block::run(&client, args).await?.into()
        }
        ApiOperation::GetBucketPolicy(args) => get_bucket_policy::run(&client, args).await?.into(),
        ApiOperation::PutBucketPolicy(args) => put_bucket_policy::run(&client, args).await?.into(),
        ApiOperation::DeleteBucketPolicy(args) => {
            delete_bucket_policy::run(&client, args).await?.into()
        }
        ApiOperation::GetBucketEncryption(args) => {
            get_bucket_encryption::run(&client, args).await?.into()
        }
        ApiOperation::PutBucketEncryption(args) => {
            put_bucket_encryption::run(&client, args).await?.into()
        }
        ApiOperation::DeleteBucketEncryption(args) => {
            delete_bucket_encryption::run(&client, args).await?.into()
        }
        ApiOperation::GetBucketLifecycleConfiguration(args) => {
            get_bucket_lifecycle_configuration::run(&client, args)
                .await?
                .into()
        }
        ApiOperation::PutBucketLifecycleConfiguration(args) => {
            put_bucket_lifecycle_configuration::run(&client, args)
                .await?
                .into()
        }
        ApiOperation::DeleteBucketLifecycle(args) => {
            delete_bucket_lifecycle::run(&client, args).await?.into()
        }
        ApiOperation::GetBucketCors(args) => get_bucket_cors::run(&client, args).await?.into(),
        ApiOperation::PutBucketCors(args) => put_bucket_cors::run(&client, args).await?.into(),
        ApiOperation::DeleteBucketCors(args) => {
            delete_bucket_cors::run(&client, args).await?.into()
        }
        ApiOperation::GetBucketAcl(args) => get_bucket_acl::run(&client, args).await?.into(),
        ApiOperation::PutBucketAcl(args) => put_bucket_acl::run(&client, args).await?.into(),
        ApiOperation::GetBucketNotificationConfiguration(args) => {
            get_bucket_notification_configuration::run(&client, args)
                .await?
                .into()
        }
        ApiOperation::PutBucketNotificationConfiguration(args) => {
            put_bucket_notification_configuration::run(&client, args)
                .await?
                .into()
        }
        ApiOperation::GetBucketLogging(args) => {
            get_bucket_logging::run(&client, args).await?.into()
        }
        ApiOperation::PutBucketLogging(args) => {
            put_bucket_logging::run(&client, args).await?.into()
        }
        ApiOperation::GetBucketTagging(args) => {
            get_bucket_tagging::run(&client, args).await?.into()
        }
        ApiOperation::PutBucketTagging(args) => {
            put_bucket_tagging::run(&client, args).await?.into()
        }
        ApiOperation::DeleteBucketTagging(args) => {
            delete_bucket_tagging::run(&client, args).await?.into()
        }
        ApiOperation::GetObjectTagging(args) => {
            get_object_tagging::run(&client, args).await?.into()
        }
        ApiOperation::PutObjectTagging(args) => {
            put_object_tagging::run(&client, args).await?.into()
        }
        ApiOperation::DeleteObjectTagging(args) => {
            delete_object_tagging::run(&client, args).await?.into()
        }
    };
    let colored = match global.color {
        Some(Color::On) => true,
        Some(Color::Off) => false,
        _ => io::stdout().is_terminal(),
    };
    print(&args.operation, responses, format, query.as_ref(), colored)?;
    Ok(ExitCode::SUCCESS)
}
