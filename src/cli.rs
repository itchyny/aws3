use std::ffi::OsString;
use std::path::PathBuf;

use clap::{ArgMatches, Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};

/// Exit code of the AWS CLI on parameter validation errors.
const EXIT_PARAM_VALIDATION: i32 = 252;

#[derive(Debug, Parser)]
#[command(version, about, propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    #[command(flatten)]
    pub global: GlobalArgs,
}

impl Cli {
    /// Parses the command line arguments, exiting with the same status code
    /// as the AWS CLI on errors.
    pub fn parse_args() -> Self {
        Self::try_parse_args_from(std::env::args_os()).unwrap_or_else(|err| {
            let _ = err.print();
            std::process::exit(if err.use_stderr() {
                EXIT_PARAM_VALIDATION
            } else {
                0
            })
        })
    }

    fn try_parse_args_from<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let matches = Self::command().try_get_matches_from(args)?;
        let mut cli = Self::from_arg_matches(&matches)?;
        if let (Some(filter), Some((_, matches))) = (cli.command.filter_mut(), matches.subcommand())
        {
            filter.resolve(matches);
        }
        Ok(cli)
    }
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List S3 objects and common prefixes under a prefix or all S3 buckets
    Ls(LsArgs),
    /// Copy a local file or S3 object to another location locally or in S3
    Cp(CpArgs),
    /// Move a local file or S3 object to another location locally or in S3
    Mv(MvArgs),
    /// Delete an S3 object
    Rm(RmArgs),
    /// Sync directories and S3 prefixes
    Sync(SyncArgs),
    /// Create an S3 bucket
    Mb(MbArgs),
    /// Delete an empty S3 bucket
    Rb(RbArgs),
    /// Generate a pre-signed URL for an Amazon S3 object
    Presign(PresignArgs),
    /// Set the website configuration for a bucket
    Website(WebsiteArgs),
    /// Call an operation of the S3 API
    Api(Box<ApiArgs>),
    /// Generate the completion script of the shell
    #[command(hide = true)]
    Completion(CompletionArgs),
}

impl Command {
    fn filter_mut(&mut self) -> Option<&mut FilterArgs> {
        match self {
            Self::Cp(args) => Some(&mut args.transfer.filter),
            Self::Mv(args) => Some(&mut args.transfer.filter),
            Self::Rm(args) => Some(&mut args.filter),
            Self::Sync(args) => Some(&mut args.transfer.filter),
            _ => None,
        }
    }
}

#[derive(Debug, Args)]
#[command(next_help_heading = "Global Options")]
pub struct GlobalArgs {
    /// Turn on debug logging
    #[arg(long, global = true)]
    pub debug: bool,

    /// Override command's default URL with the given URL
    #[arg(long, global = true, value_name = "URL")]
    pub endpoint_url: Option<String>,

    /// Do not verify SSL certificates
    #[arg(long, global = true)]
    pub no_verify_ssl: bool,

    /// Disable automatic pagination
    #[arg(long, global = true)]
    pub no_paginate: bool,

    /// The formatting style for command output
    #[arg(long, global = true)]
    pub output: Option<Output>,

    /// A JMESPath query to use in filtering the response data
    #[arg(long, global = true)]
    pub query: Option<String>,

    /// Use a specific profile from your credential file
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// The region to use
    #[arg(long, global = true)]
    pub region: Option<String>,

    /// Turn on/off color output
    #[arg(long, global = true)]
    pub color: Option<Color>,

    /// Do not sign requests
    #[arg(long, global = true)]
    pub no_sign_request: bool,

    /// The CA certificate bundle to use when verifying SSL certificates
    #[arg(long, global = true, value_name = "PATH")]
    pub ca_bundle: Option<PathBuf>,

    /// The maximum socket read time in seconds
    #[arg(long, global = true, value_name = "SECONDS")]
    pub cli_read_timeout: Option<u64>,

    /// The maximum socket connect time in seconds
    #[arg(long, global = true, value_name = "SECONDS")]
    pub cli_connect_timeout: Option<u64>,

    /// The formatting style to be used for binary blobs
    #[arg(long, global = true)]
    pub cli_binary_format: Option<BinaryFormat>,

    /// Disable cli pager for output
    #[arg(long, global = true)]
    pub no_cli_pager: bool,

    /// Automatically prompt for CLI input parameters
    #[arg(long, global = true, overrides_with = "no_cli_auto_prompt")]
    pub cli_auto_prompt: bool,

    /// Disable automatically prompt for CLI input parameters
    #[arg(long, global = true, overrides_with = "cli_auto_prompt")]
    pub no_cli_auto_prompt: bool,

    /// The formatting style for error output
    #[arg(long, global = true)]
    pub cli_error_format: Option<ErrorFormat>,
}

#[derive(Debug, Args)]
pub struct LsArgs {
    #[arg(value_name = "S3Uri", default_value = "s3://")]
    pub path: String,

    /// Command is performed on all files or objects under the specified prefix
    #[arg(long)]
    pub recursive: bool,

    /// The number of results to return in each response to a list operation
    #[arg(long, value_name = "SIZE")]
    pub page_size: Option<i32>,

    /// Displays file sizes in human readable format
    #[arg(long)]
    pub human_readable: bool,

    /// Displays summary information (number of objects, total size)
    #[arg(long)]
    pub summarize: bool,

    #[command(flatten)]
    pub request_payer: RequestPayerArgs,

    /// Limits the response to bucket names that begin with the specified prefix
    #[arg(long, value_name = "PREFIX")]
    pub bucket_name_prefix: Option<String>,

    /// Limits the response to buckets that are located in the specified region
    #[arg(long, value_name = "REGION")]
    pub bucket_region: Option<String>,
}

#[derive(Debug, Args)]
pub struct CpArgs {
    #[arg(value_name = "SOURCE")]
    pub source: String,

    #[arg(value_name = "DESTINATION")]
    pub destination: String,

    #[command(flatten)]
    pub transfer: TransferArgs,

    /// The expected size of a stream in bytes
    #[arg(long, value_name = "SIZE")]
    pub expected_size: Option<u64>,

    /// Command is performed on all files or objects under the specified directory or prefix
    #[arg(long)]
    pub recursive: bool,
}

#[derive(Debug, Args)]
pub struct MvArgs {
    #[arg(value_name = "SOURCE")]
    pub source: String,

    #[arg(value_name = "DESTINATION")]
    pub destination: String,

    #[command(flatten)]
    pub transfer: TransferArgs,

    /// Command is performed on all files or objects under the specified directory or prefix
    #[arg(long)]
    pub recursive: bool,

    /// Resolves the underlying bucket name of any S3 access points or S3 on Outposts
    /// access points to validate the source and destination are not the same
    #[arg(long)]
    pub validate_same_s3_paths: bool,
}

#[derive(Debug, Args)]
pub struct RmArgs {
    #[arg(value_name = "S3Uri")]
    pub path: String,

    /// Displays the operations that would be performed without actually running them
    #[arg(long)]
    pub dryrun: bool,

    /// Does not display the operations performed from the specified command
    #[arg(long)]
    pub quiet: bool,

    /// Command is performed on all files or objects under the specified prefix
    #[arg(long)]
    pub recursive: bool,

    #[command(flatten)]
    pub request_payer: RequestPayerArgs,

    #[command(flatten)]
    pub filter: FilterArgs,

    /// Only errors and warnings are displayed
    #[arg(long)]
    pub only_show_errors: bool,

    /// The number of results to return in each response to a list operation
    #[arg(long, value_name = "SIZE")]
    pub page_size: Option<i32>,
}

#[derive(Debug, Args)]
pub struct SyncArgs {
    #[arg(value_name = "SOURCE")]
    pub source: String,

    #[arg(value_name = "DESTINATION")]
    pub destination: String,

    #[command(flatten)]
    pub transfer: TransferArgs,

    /// Makes the size of each key the only criteria used to decide whether to sync
    #[arg(long)]
    pub size_only: bool,

    /// When syncing from S3 to local, same-sized items will be ignored only when
    /// the timestamps match exactly
    #[arg(long)]
    pub exact_timestamps: bool,

    /// Files that exist in the destination but not in the source are deleted
    #[arg(long)]
    pub delete: bool,
}

#[derive(Debug, Args)]
pub struct MbArgs {
    #[arg(value_name = "S3Uri")]
    pub path: String,

    /// Tags to be added to the bucket, can be specified multiple times
    #[arg(long, num_args = 2, value_names = ["KEY", "VALUE"], action = clap::ArgAction::Append)]
    pub tags: Vec<String>,
}

#[derive(Debug, Args)]
pub struct RbArgs {
    #[arg(value_name = "S3Uri")]
    pub path: String,

    /// Deletes all objects in the bucket including the bucket itself
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct PresignArgs {
    #[arg(value_name = "S3Uri")]
    pub path: String,

    /// Number of seconds until the pre-signed URL expires
    #[arg(
        long,
        value_name = "SECONDS",
        default_value_t = 3600,
        allow_negative_numbers = true
    )]
    pub expires_in: i64,
}

#[derive(Debug, Args)]
pub struct WebsiteArgs {
    #[arg(value_name = "S3Uri")]
    pub path: String,

    /// A suffix that is appended to a request that is for a directory on the website endpoint
    #[arg(long, value_name = "DOCUMENT")]
    pub index_document: Option<String>,

    /// The object key name to use when a 4XX class error occurs
    #[arg(long, value_name = "DOCUMENT")]
    pub error_document: Option<String>,
}

#[derive(Debug, Args)]
pub struct ApiArgs {
    #[command(subcommand)]
    pub operation: ApiOperation,
}

// The arguments are parsed only once, whose size does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
pub enum ApiOperation {
    /// Retrieve the metadata of an object without returning the object itself
    HeadObject(ObjectArgs),
    /// Retrieve an object and write it to a file
    GetObject(GetObjectArgs),
    /// Add an object to a bucket
    PutObject(PutObjectArgs),
    /// Create a copy of an object that is already stored in S3
    CopyObject(CopyObjectArgs),
    /// Remove an object or a version of an object
    DeleteObject(DeleteObjectArgs),
    /// Remove multiple objects from a bucket in a single request
    DeleteObjects(DeleteObjectsArgs),
    /// List the objects in a bucket
    ListObjectsV2(ListObjectsV2Args),
    /// List the versions of the objects in a bucket
    ListObjectVersions(ListObjectVersionsArgs),
    /// List the buckets owned by the requester
    ListBuckets(ListBucketsArgs),
    /// Determine whether a bucket exists and you have permission to access it
    HeadBucket(BucketArgs),
    /// Return the region the bucket resides in
    GetBucketLocation(BucketArgs),
    /// Create a bucket
    CreateBucket(CreateBucketArgs),
    /// Delete an empty bucket
    DeleteBucket(BucketArgs),
    /// Set the versioning state of a bucket
    PutBucketVersioning(PutBucketVersioningArgs),
}

#[derive(Debug, Args)]
pub struct PutBucketVersioningArgs {
    /// The name of the bucket
    #[arg(long)]
    pub bucket: String,

    /// The base64-encoded 128-bit MD5 digest of the request body
    #[arg(long, value_name = "STRING")]
    pub content_md5: Option<String>,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    /// The serial number and the value of the MFA device, separated by a space
    #[arg(long, value_name = "STRING")]
    pub mfa: Option<String>,

    /// The versioning configuration, i.e. the status and the MFA delete
    #[arg(long, value_name = "MFADelete=string,Status=string")]
    pub versioning_configuration: String,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
}

#[derive(Debug, Args)]
pub struct CreateBucketArgs {
    /// The canned ACL to apply to the bucket
    #[arg(long, value_name = "STRING")]
    pub acl: Option<String>,

    /// The name of the bucket to create
    #[arg(long)]
    pub bucket: String,

    /// The configuration of the bucket, e.g. the location constraint and the tags
    #[arg(
        long,
        value_name = "LocationConstraint=string,Tags=[{Key=string,Value=string},...]"
    )]
    pub create_bucket_configuration: Option<String>,

    /// Allows the grantee the read, write, read ACP, and write ACP permissions on the bucket
    #[arg(long, value_name = "STRING")]
    pub grant_full_control: Option<String>,

    /// Allows the grantee to list the objects in the bucket
    #[arg(long, value_name = "STRING")]
    pub grant_read: Option<String>,

    /// Allows the grantee to read the bucket ACL
    #[arg(long, value_name = "STRING")]
    pub grant_read_acp: Option<String>,

    /// Allows the grantee to create new objects in the bucket
    #[arg(long, value_name = "STRING")]
    pub grant_write: Option<String>,

    /// Allows the grantee to write the ACL for the applicable bucket
    #[arg(long, value_name = "STRING")]
    pub grant_write_acp: Option<String>,

    #[command(flatten)]
    pub object_lock_enabled_for_bucket: ObjectLockEnabledForBucketArgs,

    /// The ownership controls of the objects, e.g. BucketOwnerEnforced
    #[arg(long, value_name = "STRING")]
    pub object_ownership: Option<String>,

    /// The namespace of the bucket, i.e. account-regional or global
    #[arg(long, value_name = "STRING")]
    pub bucket_namespace: Option<String>,
}

#[derive(Debug, Args)]
pub struct ObjectLockEnabledForBucketArgs {
    /// Enable the Object Lock of the bucket
    #[arg(long, overrides_with = "no_object_lock_enabled_for_bucket")]
    object_lock_enabled_for_bucket: bool,

    /// Do not enable the Object Lock of the bucket
    #[arg(long, overrides_with = "object_lock_enabled_for_bucket")]
    no_object_lock_enabled_for_bucket: bool,
}

impl ObjectLockEnabledForBucketArgs {
    pub fn value(&self) -> Option<bool> {
        if self.object_lock_enabled_for_bucket {
            Some(true)
        } else if self.no_object_lock_enabled_for_bucket {
            Some(false)
        } else {
            None
        }
    }
}

#[derive(Debug, Args)]
pub struct ListObjectVersionsArgs {
    /// The name of the bucket to list
    #[arg(long)]
    pub bucket: String,

    /// A character to group the keys by, e.g. /
    #[arg(long, value_name = "STRING")]
    pub delimiter: Option<String>,

    /// The encoding of the keys in the response, i.e. url
    #[arg(long, value_name = "STRING")]
    pub encoding_type: Option<String>,

    /// The key to start listing from, which disables the pagination
    #[arg(long, value_name = "STRING", hide = true)]
    pub key_marker: Option<String>,

    /// The maximum number of keys in a response, which disables the pagination
    #[arg(
        long,
        value_name = "INTEGER",
        allow_negative_numbers = true,
        hide = true
    )]
    pub max_keys: Option<String>,

    /// Limits the response to the keys that begin with the specified prefix
    #[arg(long, value_name = "STRING")]
    pub prefix: Option<String>,

    /// The version to start listing from, which disables the pagination
    #[arg(long, value_name = "STRING", hide = true)]
    pub version_id_marker: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,

    /// The optional fields to return, i.e. RestoreStatus
    #[arg(long, value_name = "STRING", num_args = 1..)]
    pub optional_object_attributes: Option<Vec<String>>,

    #[command(flatten)]
    pub pagination: PaginationArgs,
}

#[derive(Debug, Args)]
pub struct ListBucketsArgs {
    /// The maximum number of buckets in a response, which disables the pagination
    #[arg(
        long,
        value_name = "INTEGER",
        allow_negative_numbers = true,
        hide = true
    )]
    pub max_buckets: Option<String>,

    /// The token to continue the listing, which disables the pagination
    #[arg(long, value_name = "STRING", hide = true)]
    pub continuation_token: Option<String>,

    /// Limits the response to the bucket names that begin with the specified prefix
    #[arg(long, value_name = "STRING")]
    pub prefix: Option<String>,

    /// Limits the response to the buckets in the specified region
    #[arg(long, value_name = "STRING")]
    pub bucket_region: Option<String>,

    #[command(flatten)]
    pub pagination: PaginationArgs,
}

#[derive(Debug, Args)]
pub struct ListObjectsV2Args {
    /// The name of the bucket to list
    #[arg(long)]
    pub bucket: String,

    /// A character to group the keys by, e.g. /
    #[arg(long, value_name = "STRING")]
    pub delimiter: Option<String>,

    /// The encoding of the keys in the response, i.e. url
    #[arg(long, value_name = "STRING")]
    pub encoding_type: Option<String>,

    /// The maximum number of keys in a response, which disables the pagination
    #[arg(
        long,
        value_name = "INTEGER",
        allow_negative_numbers = true,
        hide = true
    )]
    pub max_keys: Option<String>,

    /// Limits the response to the keys that begin with the specified prefix
    #[arg(long, value_name = "STRING")]
    pub prefix: Option<String>,

    /// The token to continue the listing, which disables the pagination
    #[arg(long, value_name = "STRING", hide = true)]
    pub continuation_token: Option<String>,

    #[command(flatten)]
    pub fetch_owner: FetchOwnerArgs,

    /// Start listing after the specified key
    #[arg(long, value_name = "STRING")]
    pub start_after: Option<String>,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    /// The optional fields to return, i.e. RestoreStatus
    #[arg(long, value_name = "STRING", num_args = 1..)]
    pub optional_object_attributes: Option<Vec<String>>,

    #[command(flatten)]
    pub pagination: PaginationArgs,
}

#[derive(Debug, Args)]
pub struct FetchOwnerArgs {
    /// Return the owner of each key
    #[arg(long, overrides_with = "no_fetch_owner")]
    fetch_owner: bool,

    /// Do not return the owner of each key
    #[arg(long, overrides_with = "fetch_owner")]
    no_fetch_owner: bool,
}

impl FetchOwnerArgs {
    pub fn value(&self) -> Option<bool> {
        if self.fetch_owner {
            Some(true)
        } else if self.no_fetch_owner {
            Some(false)
        } else {
            None
        }
    }
}

/// The arguments of the automatic pagination.
#[derive(Debug, Args)]
pub struct PaginationArgs {
    /// The token to continue the pagination, which is the NextToken of a previous output
    #[arg(long, value_name = "STRING")]
    pub starting_token: Option<String>,

    /// The number of items in each response, which does not limit the total
    #[arg(long, value_name = "INTEGER", allow_negative_numbers = true)]
    pub page_size: Option<i32>,

    /// The total number of items to return
    #[arg(long, value_name = "INTEGER", allow_negative_numbers = true)]
    pub max_items: Option<i64>,
}

#[derive(Debug, Args)]
pub struct BucketArgs {
    /// The name of the bucket
    #[arg(long)]
    pub bucket: String,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
}

#[derive(Debug, Args)]
pub struct GetObjectArgs {
    #[command(flatten)]
    pub object: ObjectArgs,

    /// The path of the file to write the object to
    #[arg(value_name = "OUTFILE")]
    pub outfile: PathBuf,
}

/// The arguments of the requests of an object, which are the same between
/// head-object and get-object.
#[derive(Debug, Args)]
pub struct ObjectArgs {
    /// The name of the bucket that contains the object
    #[arg(long)]
    pub bucket: String,

    /// The object key
    #[arg(long)]
    pub key: String,

    /// Return the object only if its entity tag (ETag) is the same as the one specified
    #[arg(long, value_name = "ETAG")]
    pub if_match: Option<String>,

    /// Return the object only if it has been modified since the specified time
    #[arg(long, value_name = "TIMESTAMP")]
    pub if_modified_since: Option<String>,

    /// Return the object only if its entity tag (ETag) is different from the one specified
    #[arg(long, value_name = "ETAG")]
    pub if_none_match: Option<String>,

    /// Return the object only if it has not been modified since the specified time
    #[arg(long, value_name = "TIMESTAMP")]
    pub if_unmodified_since: Option<String>,

    /// The range of bytes of the object, e.g. bytes=0-9
    #[arg(long)]
    pub range: Option<String>,

    /// Sets the Cache-Control header of the response
    #[arg(long, value_name = "STRING")]
    pub response_cache_control: Option<String>,

    /// Sets the Content-Disposition header of the response
    #[arg(long, value_name = "STRING")]
    pub response_content_disposition: Option<String>,

    /// Sets the Content-Encoding header of the response
    #[arg(long, value_name = "STRING")]
    pub response_content_encoding: Option<String>,

    /// Sets the Content-Language header of the response
    #[arg(long, value_name = "STRING")]
    pub response_content_language: Option<String>,

    /// Sets the Content-Type header of the response
    #[arg(long, value_name = "STRING")]
    pub response_content_type: Option<String>,

    /// Sets the Expires header of the response
    #[arg(long, value_name = "TIMESTAMP")]
    pub response_expires: Option<String>,

    /// The version ID of the object
    #[arg(long)]
    pub version_id: Option<String>,

    /// The algorithm of the server-side encryption with the customer-provided key, e.g. AES256
    #[arg(long, value_name = "STRING")]
    pub sse_customer_algorithm: Option<String>,

    /// The customer-provided key of the server-side encryption
    #[arg(long, value_name = "STRING")]
    pub sse_customer_key: Option<String>,

    /// The base64-encoded MD5 digest of the customer-provided key
    #[arg(long, value_name = "STRING")]
    pub sse_customer_key_md5: Option<String>,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,

    /// The part number of the object being read
    #[arg(long, value_name = "INTEGER", allow_negative_numbers = true)]
    pub part_number: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    /// Enables the checksums in the response, i.e. ENABLED
    #[arg(long, value_name = "STRING")]
    pub checksum_mode: Option<String>,
}

#[derive(Debug, Args)]
pub struct PutObjectArgs {
    /// The canned ACL to apply to the object
    #[arg(long, value_name = "STRING")]
    pub acl: Option<String>,

    /// The path of the file to upload
    #[arg(long, value_name = "PATH")]
    pub body: Option<String>,

    /// The name of the bucket to which the object is added
    #[arg(long)]
    pub bucket: String,

    /// Caching behavior along the request/reply chain
    #[arg(long, value_name = "STRING")]
    pub cache_control: Option<String>,

    /// Presentational information for the object
    #[arg(long, value_name = "STRING")]
    pub content_disposition: Option<String>,

    /// The content encodings that have been applied to the object
    #[arg(long, value_name = "STRING")]
    pub content_encoding: Option<String>,

    /// The language the content is in
    #[arg(long, value_name = "STRING")]
    pub content_language: Option<String>,

    /// The size of the body in bytes
    #[arg(long, value_name = "LONG", allow_negative_numbers = true)]
    pub content_length: Option<i64>,

    /// The base64-encoded 128-bit MD5 digest of the body
    #[arg(long, value_name = "STRING")]
    pub content_md5: Option<String>,

    /// A standard MIME type describing the format of the contents
    #[arg(long, value_name = "STRING")]
    pub content_type: Option<String>,

    /// The algorithm used to create the checksum of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    #[command(flatten)]
    pub checksums: ChecksumArgs,

    /// The date and time at which the object is no longer cacheable
    #[arg(long, value_name = "TIMESTAMP")]
    pub expires: Option<String>,

    /// Upload the object only if its entity tag (ETag) is the same as the one specified
    #[arg(long, value_name = "ETAG")]
    pub if_match: Option<String>,

    /// Upload the object only if its entity tag (ETag) is different from the one specified
    #[arg(long, value_name = "ETAG")]
    pub if_none_match: Option<String>,

    /// Gives the grantee READ, READ_ACP, and WRITE_ACP permissions on the object
    #[arg(long, value_name = "STRING")]
    pub grant_full_control: Option<String>,

    /// Allows the grantee to read the object data and its metadata
    #[arg(long, value_name = "STRING")]
    pub grant_read: Option<String>,

    /// Allows the grantee to read the object ACL
    #[arg(long, value_name = "STRING")]
    pub grant_read_acp: Option<String>,

    /// Allows the grantee to write the ACL for the applicable object
    #[arg(long, value_name = "STRING")]
    pub grant_write_acp: Option<String>,

    /// The object key
    #[arg(long)]
    pub key: String,

    /// The offset in bytes to append the data to the object
    #[arg(long, value_name = "LONG", allow_negative_numbers = true)]
    pub write_offset_bytes: Option<i64>,

    /// A map of metadata to store with the object
    #[arg(long, value_name = "KeyName1=string,KeyName2=string")]
    pub metadata: Option<String>,

    /// The server-side encryption algorithm, e.g. AES256, aws:kms
    #[arg(long, value_name = "STRING")]
    pub server_side_encryption: Option<String>,

    /// The storage class of the object, e.g. STANDARD
    #[arg(long, value_name = "STRING")]
    pub storage_class: Option<String>,

    /// Redirects the requests for the object to another object or URL
    #[arg(long, value_name = "STRING")]
    pub website_redirect_location: Option<String>,

    /// The algorithm of the server-side encryption with the customer-provided key, e.g. AES256
    #[arg(long, value_name = "STRING")]
    pub sse_customer_algorithm: Option<String>,

    /// The customer-provided key of the server-side encryption
    #[arg(long, value_name = "STRING")]
    pub sse_customer_key: Option<String>,

    /// The base64-encoded MD5 digest of the customer-provided key
    #[arg(long, value_name = "STRING")]
    pub sse_customer_key_md5: Option<String>,

    /// The ID of the KMS key to use for the object encryption
    #[arg(long, value_name = "STRING")]
    pub ssekms_key_id: Option<String>,

    /// The base64-encoded JSON of the encryption context of the KMS key
    #[arg(long, value_name = "STRING")]
    pub ssekms_encryption_context: Option<String>,

    #[command(flatten)]
    pub bucket_key_enabled: BucketKeyEnabledArgs,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,

    /// The tag-set of the object encoded as URL query parameters, e.g. Key1=Value1
    #[arg(long, value_name = "STRING")]
    pub tagging: Option<String>,

    /// The Object Lock mode to apply to the object, i.e. GOVERNANCE or COMPLIANCE
    #[arg(long, value_name = "STRING")]
    pub object_lock_mode: Option<String>,

    /// The date and time when the Object Lock of the object expires
    #[arg(long, value_name = "TIMESTAMP")]
    pub object_lock_retain_until_date: Option<String>,

    /// The legal hold status of the object, i.e. ON or OFF
    #[arg(long, value_name = "STRING")]
    pub object_lock_legal_hold_status: Option<String>,

    /// The event hold status of the object, i.e. ON or OFF
    #[arg(long, value_name = "STRING")]
    pub object_lock_event_hold: Option<String>,

    /// The number of days of the event hold of the object
    #[arg(long, value_name = "INTEGER", allow_negative_numbers = true)]
    pub object_lock_event_hold_duration_days: Option<String>,

    /// The number of years of the event hold of the object
    #[arg(long, value_name = "INTEGER", allow_negative_numbers = true)]
    pub object_lock_event_hold_duration_years: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
}

#[derive(Debug, Args)]
pub struct CopyObjectArgs {
    /// The canned ACL to apply to the object
    #[arg(long, value_name = "STRING")]
    pub acl: Option<String>,

    /// The name of the destination bucket
    #[arg(long)]
    pub bucket: String,

    /// Caching behavior along the request/reply chain
    #[arg(long, value_name = "STRING")]
    pub cache_control: Option<String>,

    /// The algorithm used to create the checksum of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    /// Presentational information for the object
    #[arg(long, value_name = "STRING")]
    pub content_disposition: Option<String>,

    /// The content encodings that have been applied to the object
    #[arg(long, value_name = "STRING")]
    pub content_encoding: Option<String>,

    /// The language the content is in
    #[arg(long, value_name = "STRING")]
    pub content_language: Option<String>,

    /// A standard MIME type describing the format of the contents
    #[arg(long, value_name = "STRING")]
    pub content_type: Option<String>,

    /// The source object, i.e. the bucket and the key separated by a slash
    #[arg(long, value_name = "STRING")]
    pub copy_source: String,

    /// Copy the object only if its entity tag (ETag) is the same as the one specified
    #[arg(long, value_name = "ETAG")]
    pub copy_source_if_match: Option<String>,

    /// Copy the object only if it has been modified since the specified time
    #[arg(long, value_name = "TIMESTAMP")]
    pub copy_source_if_modified_since: Option<String>,

    /// Copy the object only if its entity tag (ETag) is different from the one specified
    #[arg(long, value_name = "ETAG")]
    pub copy_source_if_none_match: Option<String>,

    /// Copy the object only if it has not been modified since the specified time
    #[arg(long, value_name = "TIMESTAMP")]
    pub copy_source_if_unmodified_since: Option<String>,

    /// The date and time at which the object is no longer cacheable
    #[arg(long, value_name = "TIMESTAMP")]
    pub expires: Option<String>,

    /// Gives the grantee READ, READ_ACP, and WRITE_ACP permissions on the object
    #[arg(long, value_name = "STRING")]
    pub grant_full_control: Option<String>,

    /// Allows the grantee to read the object data and its metadata
    #[arg(long, value_name = "STRING")]
    pub grant_read: Option<String>,

    /// Allows the grantee to read the object ACL
    #[arg(long, value_name = "STRING")]
    pub grant_read_acp: Option<String>,

    /// Allows the grantee to write the ACL for the applicable object
    #[arg(long, value_name = "STRING")]
    pub grant_write_acp: Option<String>,

    /// Copy the object only if the destination has the entity tag (ETag) specified
    #[arg(long, value_name = "ETAG")]
    pub if_match: Option<String>,

    /// Copy the object only if the destination does not have the entity tag (ETag) specified
    #[arg(long, value_name = "ETAG")]
    pub if_none_match: Option<String>,

    /// The key of the destination object
    #[arg(long)]
    pub key: String,

    /// A map of metadata to store with the object
    #[arg(long, value_name = "KeyName1=string,KeyName2=string")]
    pub metadata: Option<String>,

    /// Whether the metadata is copied from the source or replaced, i.e. COPY or REPLACE
    #[arg(long, value_name = "STRING")]
    pub metadata_directive: Option<String>,

    /// Whether the tag-set is copied from the source or replaced, i.e. COPY or REPLACE
    #[arg(long, value_name = "STRING")]
    pub tagging_directive: Option<String>,

    /// Whether the annotations are copied from the source or replaced, i.e. COPY or REPLACE
    #[arg(long, value_name = "STRING")]
    pub annotation_directive: Option<String>,

    /// The server-side encryption algorithm, e.g. AES256, aws:kms
    #[arg(long, value_name = "STRING")]
    pub server_side_encryption: Option<String>,

    /// The storage class of the object, e.g. STANDARD
    #[arg(long, value_name = "STRING")]
    pub storage_class: Option<String>,

    /// Redirects the requests for the object to another object or URL
    #[arg(long, value_name = "STRING")]
    pub website_redirect_location: Option<String>,

    /// The algorithm of the server-side encryption with the customer-provided key, e.g. AES256
    #[arg(long, value_name = "STRING")]
    pub sse_customer_algorithm: Option<String>,

    /// The customer-provided key of the server-side encryption
    #[arg(long, value_name = "STRING")]
    pub sse_customer_key: Option<String>,

    /// The base64-encoded MD5 digest of the customer-provided key
    #[arg(long, value_name = "STRING")]
    pub sse_customer_key_md5: Option<String>,

    /// The ID of the KMS key to use for the object encryption
    #[arg(long, value_name = "STRING")]
    pub ssekms_key_id: Option<String>,

    /// The base64-encoded JSON of the encryption context of the KMS key
    #[arg(long, value_name = "STRING")]
    pub ssekms_encryption_context: Option<String>,

    #[command(flatten)]
    pub bucket_key_enabled: BucketKeyEnabledArgs,

    /// The algorithm to decrypt the source object with the customer-provided key, e.g. AES256
    #[arg(long, value_name = "STRING")]
    pub copy_source_sse_customer_algorithm: Option<String>,

    /// The customer-provided key to decrypt the source object
    #[arg(long, value_name = "STRING")]
    pub copy_source_sse_customer_key: Option<String>,

    /// The base64-encoded MD5 digest of the customer-provided key of the source object
    #[arg(long, value_name = "STRING")]
    pub copy_source_sse_customer_key_md5: Option<String>,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,

    /// The tag-set of the object encoded as URL query parameters, e.g. Key1=Value1
    #[arg(long, value_name = "STRING")]
    pub tagging: Option<String>,

    /// The Object Lock mode to apply to the object, i.e. GOVERNANCE or COMPLIANCE
    #[arg(long, value_name = "STRING")]
    pub object_lock_mode: Option<String>,

    /// The date and time when the Object Lock of the object expires
    #[arg(long, value_name = "TIMESTAMP")]
    pub object_lock_retain_until_date: Option<String>,

    /// The legal hold status of the object, i.e. ON or OFF
    #[arg(long, value_name = "STRING")]
    pub object_lock_legal_hold_status: Option<String>,

    /// The event hold status of the object, i.e. ON or OFF
    #[arg(long, value_name = "STRING")]
    pub object_lock_event_hold: Option<String>,

    /// The number of days of the event hold of the object
    #[arg(long, value_name = "INTEGER", allow_negative_numbers = true)]
    pub object_lock_event_hold_duration_days: Option<String>,

    /// The number of years of the event hold of the object
    #[arg(long, value_name = "INTEGER", allow_negative_numbers = true)]
    pub object_lock_event_hold_duration_years: Option<String>,

    /// The account ID of the expected destination bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    /// The account ID of the expected source bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_source_bucket_owner: Option<String>,
}

/// The base64-encoded checksums of the object, which are sent instead of
/// being calculated.
#[derive(Debug, Args)]
pub struct ChecksumArgs {
    /// The base64-encoded CRC32 checksum of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_crc32: Option<String>,

    /// The base64-encoded CRC32C checksum of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_crc32_c: Option<String>,

    /// The base64-encoded CRC64NVME checksum of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_crc64_nvme: Option<String>,

    /// The base64-encoded SHA1 digest of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_sha1: Option<String>,

    /// The base64-encoded SHA256 digest of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_sha256: Option<String>,

    /// The base64-encoded SHA512 digest of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_sha512: Option<String>,

    /// The base64-encoded MD5 digest of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_md5: Option<String>,

    /// The base64-encoded XXHASH64 checksum of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_xxhash64: Option<String>,

    /// The base64-encoded XXHASH3 checksum of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_xxhash3: Option<String>,

    /// The base64-encoded XXHASH128 checksum of the object
    #[arg(long, value_name = "STRING")]
    pub checksum_xxhash128: Option<String>,
}

impl ChecksumArgs {
    /// Returns whether any checksum is given.
    pub fn is_empty(&self) -> bool {
        [
            &self.checksum_crc32,
            &self.checksum_crc32_c,
            &self.checksum_crc64_nvme,
            &self.checksum_sha1,
            &self.checksum_sha256,
            &self.checksum_sha512,
            &self.checksum_md5,
            &self.checksum_xxhash64,
            &self.checksum_xxhash3,
            &self.checksum_xxhash128,
        ]
        .iter()
        .all(|checksum| checksum.is_none())
    }
}

#[derive(Debug, Args)]
pub struct BucketKeyEnabledArgs {
    /// Use an S3 Bucket Key for the object encryption with KMS keys
    #[arg(long, overrides_with = "no_bucket_key_enabled")]
    bucket_key_enabled: bool,

    /// Do not use an S3 Bucket Key for the object encryption with KMS keys
    #[arg(long, overrides_with = "bucket_key_enabled")]
    no_bucket_key_enabled: bool,
}

impl BucketKeyEnabledArgs {
    pub fn value(&self) -> Option<bool> {
        if self.bucket_key_enabled {
            Some(true)
        } else if self.no_bucket_key_enabled {
            Some(false)
        } else {
            None
        }
    }
}

#[derive(Debug, Args)]
pub struct DeleteObjectArgs {
    /// The name of the bucket that contains the object
    #[arg(long)]
    pub bucket: String,

    /// The key of the object to delete
    #[arg(long)]
    pub key: String,

    /// The serial number and the value of the MFA device, separated by a space
    #[arg(long, value_name = "STRING")]
    pub mfa: Option<String>,

    /// The version ID of the object to delete
    #[arg(long)]
    pub version_id: Option<String>,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,

    #[command(flatten)]
    pub bypass_governance_retention: BypassGovernanceRetentionArgs,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    /// Delete the object only if its entity tag (ETag) is the same as the one specified
    #[arg(long, value_name = "ETAG")]
    pub if_match: Option<String>,

    /// Delete the object only if its last modified time is the same as the one specified
    #[arg(long, value_name = "TIMESTAMP")]
    pub if_match_last_modified_time: Option<String>,

    /// Delete the object only if its size is the same as the one specified
    #[arg(long, value_name = "LONG", allow_negative_numbers = true)]
    pub if_match_size: Option<i64>,
}

#[derive(Debug, Args)]
pub struct DeleteObjectsArgs {
    /// The name of the bucket that contains the objects
    #[arg(long)]
    pub bucket: String,

    /// The objects to delete
    #[arg(
        long,
        value_name = "Objects=[{Key=string,VersionId=string},...],Quiet=boolean"
    )]
    pub delete: String,

    /// The serial number and the value of the MFA device, separated by a space
    #[arg(long, value_name = "STRING")]
    pub mfa: Option<String>,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,

    #[command(flatten)]
    pub bypass_governance_retention: BypassGovernanceRetentionArgs,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,
}

#[derive(Debug, Args)]
pub struct BypassGovernanceRetentionArgs {
    /// Bypass the restrictions of the governance-mode Object Lock
    #[arg(long, overrides_with = "no_bypass_governance_retention")]
    bypass_governance_retention: bool,

    /// Do not bypass the restrictions of the governance-mode Object Lock
    #[arg(long, overrides_with = "bypass_governance_retention")]
    no_bypass_governance_retention: bool,
}

impl BypassGovernanceRetentionArgs {
    pub fn value(&self) -> Option<bool> {
        if self.bypass_governance_retention {
            Some(true)
        } else if self.no_bypass_governance_retention {
            Some(false)
        } else {
            None
        }
    }
}

#[derive(Debug, Args)]
pub struct CompletionArgs {
    #[arg(value_name = "SHELL")]
    pub shell: clap_complete::Shell,
}

impl CompletionArgs {
    /// Prints the completion script, ignoring the errors of writing like the
    /// other outputs of the commands.
    pub fn run(&self) -> std::process::ExitCode {
        use std::io::Write;
        // The generators include the hidden subcommands, so the command is
        // rebuilt without them.
        let full = Cli::command();
        let mut command = clap::Command::new(env!("CARGO_PKG_NAME"))
            .version(env!("CARGO_PKG_VERSION"))
            .propagate_version(true)
            .args(full.get_arguments().cloned())
            .subcommands(full.get_subcommands().filter(|c| !c.is_hide_set()).cloned());
        let mut script = Vec::new();
        clap_complete::generate(
            self.shell,
            &mut command,
            env!("CARGO_PKG_NAME"),
            &mut script,
        );
        let _ = std::io::stdout().write_all(&script);
        std::process::ExitCode::SUCCESS
    }
}

/// Arguments shared by cp, mv and sync.
#[derive(Debug, Args)]
pub struct TransferArgs {
    /// Displays the operations that would be performed without actually running them
    #[arg(long)]
    pub dryrun: bool,

    /// Does not display the operations performed from the specified command
    #[arg(long)]
    pub quiet: bool,

    #[command(flatten)]
    pub filter: FilterArgs,

    /// Sets the ACL for the object when the command is performed
    #[arg(long)]
    pub acl: Option<Acl>,

    /// Follow symbolic links (default)
    #[arg(long, overrides_with = "no_follow_symlinks")]
    pub follow_symlinks: bool,

    /// Do not follow symbolic links
    #[arg(long, overrides_with = "follow_symlinks")]
    pub no_follow_symlinks: bool,

    /// Do not try to guess the mime type for uploaded files
    #[arg(long)]
    pub no_guess_mime_type: bool,

    /// Specifies server-side encryption of the object in S3
    #[arg(long, num_args = 0..=1, default_missing_value = "AES256")]
    pub sse: Option<Sse>,

    /// Specifies server-side encryption using customer provided keys
    #[arg(long, num_args = 0..=1, default_missing_value = "AES256")]
    pub sse_c: Option<SseC>,

    /// The customer-provided encryption key to use to server-side encrypt the object
    #[arg(long, value_name = "KEY")]
    pub sse_c_key: Option<String>,

    /// The customer-managed AWS KMS key ID that should be used to server-side encrypt the object
    #[arg(long, value_name = "KEY_ID")]
    pub sse_kms_key_id: Option<String>,

    /// Decrypts the source object using customer provided keys
    #[arg(long, num_args = 0..=1, default_missing_value = "AES256")]
    pub sse_c_copy_source: Option<SseC>,

    /// The customer-provided encryption key to use to decrypt the source object
    #[arg(long, value_name = "KEY")]
    pub sse_c_copy_source_key: Option<String>,

    /// The type of storage to use for the object
    #[arg(long)]
    pub storage_class: Option<StorageClass>,

    /// Tags to set on objects uploaded from local paths, can be specified multiple times
    #[arg(long, num_args = 2, value_names = ["KEY", "VALUE"], action = clap::ArgAction::Append)]
    pub tags: Vec<String>,

    /// Grant specific permissions to individual users or groups
    #[arg(long, num_args = 1.., value_name = "Permission=Grantee_Type=Grantee_ID")]
    pub grants: Vec<String>,

    /// Redirect requests for this object to another object or an external URL
    #[arg(long, value_name = "LOCATION")]
    pub website_redirect: Option<String>,

    /// Specify an explicit content type for this operation
    #[arg(long)]
    pub content_type: Option<String>,

    /// Specifies caching behavior along the request/reply chain
    #[arg(long)]
    pub cache_control: Option<String>,

    /// Specifies presentational information for the object
    #[arg(long)]
    pub content_disposition: Option<String>,

    /// Specifies what content encodings have been applied to the object
    #[arg(long)]
    pub content_encoding: Option<String>,

    /// The language the content is in
    #[arg(long)]
    pub content_language: Option<String>,

    /// The date and time at which the object is no longer cacheable
    #[arg(long)]
    pub expires: Option<String>,

    /// The region of the source bucket for S3 to S3 transfers
    #[arg(long, value_name = "REGION")]
    pub source_region: Option<String>,

    /// Only errors and warnings are displayed
    #[arg(long)]
    pub only_show_errors: bool,

    /// File transfer progress is not displayed
    #[arg(long)]
    pub no_progress: bool,

    /// Number of seconds to wait before updating file transfer progress
    #[arg(long, value_name = "SECONDS")]
    pub progress_frequency: Option<u64>,

    /// Show progress on multiple lines
    #[arg(long)]
    pub progress_multiline: bool,

    /// The number of results to return in each response to a list operation
    #[arg(long, value_name = "SIZE")]
    pub page_size: Option<i32>,

    /// Turns off glacier warnings
    #[arg(long)]
    pub ignore_glacier_warnings: bool,

    /// Forces a transfer request on all Glacier objects in a sync or recursive copy
    #[arg(long)]
    pub force_glacier_transfer: bool,

    #[command(flatten)]
    pub request_payer: RequestPayerArgs,

    /// Enable checksum validation of the retrieved object
    #[arg(long)]
    pub checksum_mode: Option<ChecksumMode>,

    /// The algorithm used to create the checksum for the object
    #[arg(long)]
    pub checksum_algorithm: Option<ChecksumAlgorithm>,

    /// A map of metadata to store with the objects in S3
    #[arg(long, value_name = "KeyName1=string,KeyName2=string")]
    pub metadata: Option<String>,

    /// Determines which properties are copied from the source S3 object
    #[arg(long, default_value = "default")]
    pub copy_props: CopyProps,

    /// Sets the x-amz-metadata-directive header for CopyObject operations
    #[arg(long)]
    pub metadata_directive: Option<MetadataDirective>,

    /// Behavior when downloading multiple objects whose keys differ only by case
    #[arg(long, default_value = "ignore")]
    pub case_conflict: CaseConflict,

    /// Prevents overwriting of files at the destination
    #[arg(long)]
    pub no_overwrite: bool,
}

#[derive(Debug, Args)]
pub struct RequestPayerArgs {
    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, num_args = 0..=1, default_missing_value = "requester")]
    pub request_payer: Option<RequestPayer>,
}

#[derive(Debug, Args)]
pub struct FilterArgs {
    /// Don't exclude files or objects that match the specified pattern
    #[arg(long, value_name = "PATTERN")]
    include: Vec<String>,

    /// Exclude all files or objects that match the specified pattern
    #[arg(long, value_name = "PATTERN")]
    exclude: Vec<String>,

    /// The filters in the order of the command line, since later filters
    /// take precedence over earlier ones.
    #[arg(skip)]
    pub filters: Vec<Filter>,
}

impl FilterArgs {
    fn resolve(&mut self, matches: &ArgMatches) {
        let indices = |id| matches.indices_of(id).into_iter().flatten();
        let mut filters: Vec<_> = indices("include")
            .zip(self.include.drain(..).map(Filter::Include))
            .chain(indices("exclude").zip(self.exclude.drain(..).map(Filter::Exclude)))
            .collect();
        filters.sort_by_key(|(index, _)| *index);
        self.filters = filters.into_iter().map(|(_, filter)| filter).collect();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    Include(String),
    Exclude(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Output {
    Json,
    Text,
    Table,
    Yaml,
    YamlStream,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Color {
    On,
    Off,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BinaryFormat {
    Base64,
    RawInBase64Out,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ErrorFormat {
    Legacy,
    Json,
    Yaml,
    Text,
    Table,
    Enhanced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Acl {
    Private,
    PublicRead,
    PublicReadWrite,
    AuthenticatedRead,
    AwsExecRead,
    BucketOwnerRead,
    BucketOwnerFullControl,
    LogDeliveryWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Sse {
    #[value(name = "AES256")]
    Aes256,
    #[value(name = "aws:kms")]
    AwsKms,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SseC {
    #[value(name = "AES256")]
    Aes256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StorageClass {
    Standard,
    ReducedRedundancy,
    StandardIa,
    OnezoneIa,
    IntelligentTiering,
    Glacier,
    DeepArchive,
    GlacierIr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RequestPayer {
    Requester,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ChecksumMode {
    #[value(name = "ENABLED")]
    Enabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "UPPER")]
pub enum ChecksumAlgorithm {
    Crc64nvme,
    Crc32,
    Sha256,
    Sha1,
    Crc32c,
    Sha512,
    Xxhash64,
    Xxhash3,
    Xxhash128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CopyProps {
    None,
    MetadataDirective,
    Default,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "UPPER")]
pub enum MetadataDirective {
    Copy,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum CaseConflict {
    Ignore,
    Skip,
    Warn,
    Error,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::try_parse_args_from(std::iter::once("aws3").chain(args.iter().copied()))
    }

    #[test]
    fn verify_command() {
        Cli::command().debug_assert();
    }

    #[test]
    fn ls_default_path() {
        let cli = parse(&["ls"]).unwrap();
        let Command::Ls(args) = cli.command else {
            panic!()
        };
        assert_eq!(args.path, "s3://");
    }

    #[test]
    fn global_options_after_subcommand() {
        let cli = parse(&[
            "ls",
            "s3://bucket",
            "--region",
            "us-west-2",
            "--output",
            "json",
        ])
        .unwrap();
        assert_eq!(cli.global.region.as_deref(), Some("us-west-2"));
        assert_eq!(cli.global.output, Some(Output::Json));
    }

    #[test]
    fn filters_keep_order() {
        let cli = parse(&[
            "cp",
            ".",
            "s3://bucket",
            "--recursive",
            "--exclude",
            "*",
            "--include",
            "*.log",
            "--exclude",
            "tmp/*",
        ])
        .unwrap();
        let Command::Cp(args) = cli.command else {
            panic!()
        };
        assert_eq!(
            args.transfer.filter.filters,
            [
                Filter::Exclude("*".into()),
                Filter::Include("*.log".into()),
                Filter::Exclude("tmp/*".into()),
            ]
        );
    }

    #[test]
    fn optional_values() {
        let cli = parse(&["cp", "a", "s3://bucket/a", "--sse", "--request-payer"]).unwrap();
        let Command::Cp(args) = cli.command else {
            panic!()
        };
        assert_eq!(args.transfer.sse, Some(Sse::Aes256));
        assert_eq!(
            args.transfer.request_payer.request_payer,
            Some(RequestPayer::Requester)
        );

        let cli = parse(&["cp", "a", "s3://bucket/a", "--sse", "aws:kms"]).unwrap();
        let Command::Cp(args) = cli.command else {
            panic!()
        };
        assert_eq!(args.transfer.sse, Some(Sse::AwsKms));
    }

    #[test]
    fn choice_names() {
        let cli = parse(&[
            "sync",
            ".",
            "s3://bucket",
            "--acl",
            "bucket-owner-full-control",
            "--storage-class",
            "GLACIER_IR",
            "--checksum-algorithm",
            "CRC64NVME",
            "--metadata-directive",
            "REPLACE",
        ])
        .unwrap();
        let Command::Sync(args) = cli.command else {
            panic!()
        };
        assert_eq!(args.transfer.acl, Some(Acl::BucketOwnerFullControl));
        assert_eq!(args.transfer.storage_class, Some(StorageClass::GlacierIr));
        assert_eq!(
            args.transfer.checksum_algorithm,
            Some(ChecksumAlgorithm::Crc64nvme)
        );
        assert_eq!(
            args.transfer.metadata_directive,
            Some(MetadataDirective::Replace)
        );
        assert!(parse(&["cp", "a", "s3://bucket/a", "--acl", "foo"]).is_err());
    }

    #[test]
    fn mb_tags() {
        let cli = parse(&[
            "mb",
            "s3://bucket",
            "--tags",
            "k1",
            "v1",
            "--tags",
            "k2",
            "v2",
        ])
        .unwrap();
        let Command::Mb(args) = cli.command else {
            panic!()
        };
        assert_eq!(args.tags, ["k1", "v1", "k2", "v2"]);
    }

    #[test]
    fn missing_paths() {
        assert!(parse(&["cp", "a"]).is_err());
        assert!(parse(&["rm"]).is_err());
        assert!(parse(&["cp", "a", "b", "c"]).is_err());
    }
}
