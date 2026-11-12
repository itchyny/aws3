//! The api command, which calls the operations of the S3 API in the same way
//! as `aws s3api`, and prints the responses in the same format.

mod copy_object;
mod create_bucket;
mod delete_bucket;
mod delete_bucket_cors;
mod delete_bucket_encryption;
mod delete_bucket_lifecycle;
mod delete_bucket_policy;
mod delete_object;
mod delete_objects;
mod delete_public_access_block;
mod get_bucket_cors;
mod get_bucket_encryption;
mod get_bucket_lifecycle_configuration;
mod get_bucket_location;
mod get_bucket_policy;
mod get_bucket_versioning;
mod get_object;
mod get_public_access_block;
mod head_bucket;
mod head_object;
mod jmespath;
mod list_buckets;
mod list_object_versions;
mod list_objects_v2;
mod members;
mod output;
mod paginate;
mod put_bucket_cors;
mod put_bucket_encryption;
mod put_bucket_lifecycle_configuration;
mod put_bucket_policy;
mod put_bucket_versioning;
mod put_object;
mod put_public_access_block;
pub mod shape;
mod text;

use std::collections::HashMap;
use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};

use aws_sdk_s3::config::{ConfigBag, Intercept, RuntimeComponents};
use aws_sdk_s3::primitives::DateTime;
use aws_sdk_s3::types::ChecksumAlgorithm;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeDeserializationInterceptorContextRef;
use aws_smithy_types::base64;
use clap::ValueEnum;
use md5::{Digest, Md5};

use crate::cli::{ApiArgs, ApiOperation, GlobalArgs};
use crate::client;
use crate::config::ProfileConfig;
use crate::error::Error;
use crate::transfer::params;
use crate::value::{Value, insert};
use jmespath::Expression;
use paginate::Pages;

pub async fn run(args: &ApiArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
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
        ApiOperation::ListObjectsV2(args) => list_objects_v2::run(&client, args, global).await?,
        ApiOperation::ListObjectVersions(args) => {
            list_object_versions::run(&client, args, global).await?
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
    };
    print(responses, format, query.as_ref())?;
    Ok(ExitCode::SUCCESS)
}

/// The responses of an operation, which are paginated for the list
/// operations.
pub enum Responses {
    Value(Value),
    Pages(Pages),
}

impl From<Value> for Responses {
    fn from(value: Value) -> Self {
        Self::Value(value)
    }
}

/// The formats of the outputs of `--output`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Json,
    Text,
    Off,
}

/// Returns the format of the outputs of the option, the environment variable
/// or the config file, like the AWS CLI.
fn output_format(global: &GlobalArgs) -> Result<Format, Error> {
    let name = match global.output {
        Some(output) => output
            .to_possible_value()
            .map(|value| value.get_name().to_owned())
            .unwrap_or_default(),
        None => std::env::var("AWS_DEFAULT_OUTPUT")
            .ok()
            .or_else(|| ProfileConfig::load(global.profile.as_deref()).output)
            .unwrap_or_else(|| "json".to_owned()),
    };
    match name.as_str() {
        "json" => Ok(Format::Json),
        "text" => Ok(Format::Text),
        "off" => Ok(Format::Off),
        "table" | "yaml" | "yaml-stream" => Err(Error::new(format!(
            "--output {name} is not supported by the api command"
        ))),
        _ => Err(Error::new(format!("Unknown output type: {name}"))),
    }
}

/// Prints the responses in the format after applying the query, ignoring the
/// errors of writing like the other outputs of the commands. The empty objects
/// are not printed in JSON, and the pages are printed one by one in text.
fn print(responses: Responses, format: Format, query: Option<&Expression>) -> Result<(), Error> {
    let search = |value: Value| match query {
        Some(query) => query
            .search(&value)
            .map_err(|err| Error::new(err.to_string())),
        None => Ok(value),
    };
    match format {
        Format::Json => {
            let response = search(match responses {
                Responses::Value(value) => value,
                Responses::Pages(pages) => pages.merge(),
            })?;
            if !matches!(&response, Value::Map(pairs) if pairs.is_empty()) {
                let _ = writeln!(io::stdout(), "{}", response.to_json_pretty());
            }
        }
        Format::Text => {
            let responses = match responses {
                Responses::Value(value) => vec![value],
                Responses::Pages(pages) => pages.into_pages(),
            };
            for response in responses {
                let _ = io::stdout().write_all(text::format(&search(response)?)?.as_bytes());
            }
        }
        Format::Off => {}
    }
    Ok(())
}

/// Parses the integer argument, failing with the same message as `int` of
/// Python.
fn parse_int(value: Option<&str>) -> Result<Option<i32>, Error> {
    value
        .map(|value| {
            i32::try_from(shape::python_int(value)?).map_err(|_| {
                Error::new(format!("invalid literal for int() with base 10: '{value}'"))
            })
        })
        .transpose()
}

/// Returns the checksum algorithm of the requests whose checksums are
/// required, which defaults to CRC64NVME like the AWS CLI.
fn required_checksum_algorithm(value: Option<&str>) -> ChecksumAlgorithm {
    value.map_or(ChecksumAlgorithm::Crc64Nvme, ChecksumAlgorithm::from)
}

/// Parses the timestamp argument of the member, failing with the same message
/// as the parameter validation of botocore.
fn parse_timestamp(member: &str, value: Option<&str>) -> Result<Option<DateTime>, Error> {
    value
        .map(|value| {
            params::parse_timestamp(value).map_err(|_| {
                Error::param_validation(format!(
                    "Parameter validation failed:\nInvalid type for parameter {member}, \
                     value: {value}, type: <class 'str'>, \
                     valid types: <class 'datetime.datetime'>, timestamp-string"
                ))
            })
        })
        .transpose()
}

/// Parses the `--metadata` argument, whose values must be ASCII strings like
/// `validate_ascii_metadata` of botocore, which fails on other types before
/// validating the parameters.
fn parse_metadata(value: Option<&str>) -> Result<Option<HashMap<String, String>>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    let (pairs, _) = params::parse_map_argument("--metadata", value)?;
    let mut metadata = HashMap::new();
    for (key, value) in pairs {
        let Value::Str(value) = value else {
            return Err(Error::new(format!(
                "'{}' object has no attribute 'encode'",
                shape::type_name(&value)
            )));
        };
        if !key.is_ascii() || !value.is_ascii() {
            return Err(Error::param_validation(format!(
                "Parameter validation failed:\nNon ascii characters found in S3 metadata for \
                 key \"{key}\", value: \"{value}\".  \nS3 metadata can only contain ASCII \
                 characters. "
            )));
        }
        metadata.insert(key, value);
    }
    Ok(Some(metadata))
}

/// Returns the customer-provided key and its MD5 digest, where the key is
/// base64 encoded and the digest is calculated unless the digest is given,
/// like `sse_md5` of botocore.
fn sse_customer_key(
    option: &str,
    key: Option<&str>,
    key_md5: Option<&str>,
) -> Result<(Option<String>, Option<String>), Error> {
    let key_md5 = key_md5.map(str::to_owned);
    let Some(key) = params::load_key(option, key)? else {
        return Ok((None, key_md5));
    };
    Ok(match key_md5 {
        Some(key_md5) => (
            Some(String::from_utf8_lossy(&key).into_owned()),
            Some(key_md5),
        ),
        None => (
            Some(base64::encode(&key)),
            Some(base64::encode(Md5::digest(&key))),
        ),
    })
}

/// The interceptor to keep the headers of the response, whose order of the
/// metadata is lost in the outputs of the SDK.
#[derive(Debug, Clone, Default)]
struct ResponseHeaders(Arc<Mutex<Vec<(String, String)>>>);

impl ResponseHeaders {
    /// Returns the metadata in the order of the headers, whose names are
    /// stripped of the prefix like botocore.
    fn metadata(&self) -> Value {
        let headers = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut pairs = Vec::new();
        for (name, value) in headers.iter() {
            if let Some(name) = name.to_lowercase().strip_prefix("x-amz-meta-") {
                insert(&mut pairs, name.to_owned(), Value::Str(value.clone()));
            }
        }
        Value::Map(pairs)
    }
}

impl Intercept for ResponseHeaders {
    fn name(&self) -> &'static str {
        "ResponseHeaders"
    }

    fn read_before_deserialization(
        &self,
        context: &BeforeDeserializationInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _config: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let headers = context
            .response()
            .headers()
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = headers;
        Ok(())
    }
}
