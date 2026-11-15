//! The arguments of the operations of the api command.

use std::path::PathBuf;

use clap::{Args, Subcommand};

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
    /// Restore a temporary copy of an archived object
    RestoreObject(RestoreObjectArgs),
    /// List the objects in a bucket
    ListObjectsV2(ListObjectsV2Args),
    /// List the versions of the objects in a bucket
    ListObjectVersions(ListObjectVersionsArgs),
    /// List the in-progress multipart uploads in a bucket
    ListMultipartUploads(ListMultipartUploadsArgs),
    /// Abort a multipart upload
    AbortMultipartUpload(AbortMultipartUploadArgs),
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
    /// Return the versioning state of a bucket
    GetBucketVersioning(BucketArgs),
    /// Set the versioning state of a bucket
    PutBucketVersioning(PutBucketVersioningArgs),
    /// Return the configuration of the public access block of a bucket
    GetPublicAccessBlock(BucketArgs),
    /// Set the configuration of the public access block of a bucket
    PutPublicAccessBlock(PutPublicAccessBlockArgs),
    /// Delete the configuration of the public access block of a bucket
    DeletePublicAccessBlock(BucketArgs),
    /// Return the policy of a bucket
    GetBucketPolicy(BucketArgs),
    /// Apply a policy to a bucket
    PutBucketPolicy(PutBucketPolicyArgs),
    /// Delete the policy of a bucket
    DeleteBucketPolicy(BucketArgs),
    /// Return the default encryption of a bucket
    GetBucketEncryption(BucketArgs),
    /// Set the default encryption of a bucket
    PutBucketEncryption(PutBucketEncryptionArgs),
    /// Delete the default encryption of a bucket
    DeleteBucketEncryption(BucketArgs),
    /// Return the lifecycle configuration of a bucket
    GetBucketLifecycleConfiguration(BucketArgs),
    /// Set the lifecycle configuration of a bucket
    PutBucketLifecycleConfiguration(PutBucketLifecycleConfigurationArgs),
    /// Delete the lifecycle configuration of a bucket
    DeleteBucketLifecycle(BucketArgs),
    /// Return the CORS configuration of a bucket
    GetBucketCors(BucketArgs),
    /// Set the CORS configuration of a bucket
    PutBucketCors(PutBucketCorsArgs),
    /// Delete the CORS configuration of a bucket
    DeleteBucketCors(BucketArgs),
    /// Return the access control list of a bucket
    GetBucketAcl(BucketArgs),
    /// Set the access control list of a bucket
    PutBucketAcl(PutBucketAclArgs),
    /// Return the notification configuration of a bucket
    GetBucketNotificationConfiguration(BucketArgs),
    /// Set the notification configuration of a bucket
    PutBucketNotificationConfiguration(PutBucketNotificationConfigurationArgs),
    /// Return the logging status of a bucket
    GetBucketLogging(BucketArgs),
    /// Set the logging status of a bucket
    PutBucketLogging(PutBucketLoggingArgs),
    /// Return the tags of a bucket
    GetBucketTagging(BucketArgs),
    /// Set the tags of a bucket
    PutBucketTagging(PutBucketTaggingArgs),
    /// Delete the tags of a bucket
    DeleteBucketTagging(BucketArgs),
    /// Return the tags of an object
    GetObjectTagging(GetObjectTaggingArgs),
    /// Set the tags of an object
    PutObjectTagging(PutObjectTaggingArgs),
    /// Delete the tags of an object
    DeleteObjectTagging(DeleteObjectTaggingArgs),
}

#[derive(Debug, Args)]
pub struct PutBucketLifecycleConfigurationArgs {
    /// The name of the bucket
    #[arg(long)]
    pub bucket: String,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    /// The lifecycle configuration, e.g. file://lifecycle.json
    #[arg(
        long,
        value_name = "Rules=[{ID=string,Filter={Prefix=string},Status=string,Expiration={Days=integer}},...]"
    )]
    pub lifecycle_configuration: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    /// The default minimum size of the objects to transition, e.g. all_storage_classes_128K
    #[arg(long, value_name = "STRING")]
    pub transition_default_minimum_object_size: Option<String>,
}

#[derive(Debug, Args)]
pub struct PutBucketEncryptionArgs {
    /// The name of the bucket
    #[arg(long)]
    pub bucket: String,

    /// The base64-encoded 128-bit MD5 digest of the request body
    #[arg(long, value_name = "STRING")]
    pub content_md5: Option<String>,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    /// The configuration of the default encryption
    #[arg(
        long,
        value_name = "Rules=[{ApplyServerSideEncryptionByDefault={SSEAlgorithm=string,KMSMasterKeyID=string},BucketKeyEnabled=boolean},...]"
    )]
    pub server_side_encryption_configuration: String,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
}

#[derive(Debug, Args)]
pub struct PutBucketPolicyArgs {
    /// The name of the bucket
    #[arg(long)]
    pub bucket: String,

    /// The base64-encoded 128-bit MD5 digest of the request body
    #[arg(long, value_name = "STRING")]
    pub content_md5: Option<String>,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    #[command(flatten)]
    pub confirm_remove_self_bucket_access: ConfirmRemoveSelfBucketAccessArgs,

    /// The policy in JSON, e.g. file://policy.json
    #[arg(long, value_name = "STRING")]
    pub policy: String,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
}

#[derive(Debug, Args)]
pub struct ConfirmRemoveSelfBucketAccessArgs {
    /// Confirm removing the permissions of the requester to change the policy
    #[arg(long, overrides_with = "no_confirm_remove_self_bucket_access")]
    confirm_remove_self_bucket_access: bool,

    /// Do not confirm removing the permissions of the requester to change the policy
    #[arg(long, overrides_with = "confirm_remove_self_bucket_access")]
    no_confirm_remove_self_bucket_access: bool,
}

impl ConfirmRemoveSelfBucketAccessArgs {
    pub fn value(&self) -> Option<bool> {
        if self.confirm_remove_self_bucket_access {
            Some(true)
        } else if self.no_confirm_remove_self_bucket_access {
            Some(false)
        } else {
            None
        }
    }
}

#[derive(Debug, Args)]
pub struct PutPublicAccessBlockArgs {
    /// The name of the bucket
    #[arg(long)]
    pub bucket: String,

    /// The base64-encoded 128-bit MD5 digest of the request body
    #[arg(long, value_name = "STRING")]
    pub content_md5: Option<String>,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    /// The configuration of the public access block
    #[arg(
        long,
        value_name = "BlockPublicAcls=boolean,IgnorePublicAcls=boolean,BlockPublicPolicy=boolean,RestrictPublicBuckets=boolean"
    )]
    pub public_access_block_configuration: String,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
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
pub struct ListMultipartUploadsArgs {
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

    /// The maximum number of uploads in a response, which disables the pagination
    #[arg(
        long,
        value_name = "INTEGER",
        allow_negative_numbers = true,
        hide = true
    )]
    pub max_uploads: Option<String>,

    /// Limits the response to the keys that begin with the specified prefix
    #[arg(long, value_name = "STRING")]
    pub prefix: Option<String>,

    /// The upload to start listing from, which disables the pagination
    #[arg(long, value_name = "STRING", hide = true)]
    pub upload_id_marker: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,

    #[command(flatten)]
    pub pagination: PaginationArgs,
}

#[derive(Debug, Args)]
pub struct AbortMultipartUploadArgs {
    /// The name of the bucket of the upload
    #[arg(long)]
    pub bucket: String,

    /// The key of the object of the upload
    #[arg(long)]
    pub key: String,

    /// The ID of the upload to abort
    #[arg(long, value_name = "STRING")]
    pub upload_id: String,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    /// Abort the upload only if it was initiated at the time specified
    #[arg(long, value_name = "TIMESTAMP")]
    pub if_match_initiated_time: Option<String>,
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
pub struct PutBucketCorsArgs {
    /// The name of the bucket
    #[arg(long)]
    pub bucket: String,

    /// The CORS configuration, e.g. file://cors.json
    #[arg(
        long,
        value_name = "CORSRules=[{AllowedMethods=[string,...],AllowedOrigins=[string,...]},...]"
    )]
    pub cors_configuration: String,

    /// The base64-encoded 128-bit MD5 digest of the request body
    #[arg(long, value_name = "STRING")]
    pub content_md5: Option<String>,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
}

#[derive(Debug, Args)]
pub struct PutBucketAclArgs {
    /// The canned ACL to apply to the bucket
    #[arg(long, value_name = "STRING")]
    pub acl: Option<String>,

    /// The access control list, e.g. file://acl.json
    #[arg(
        long,
        value_name = "Grants=[{Grantee={Type=string,...},Permission=string},...],Owner={...}"
    )]
    pub access_control_policy: Option<String>,

    /// The name of the bucket
    #[arg(long)]
    pub bucket: String,

    /// The base64-encoded 128-bit MD5 digest of the request body
    #[arg(long, value_name = "STRING")]
    pub content_md5: Option<String>,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

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

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
}

#[derive(Debug, Args)]
pub struct PutBucketNotificationConfigurationArgs {
    /// The name of the bucket
    #[arg(long)]
    pub bucket: String,

    /// The notification configuration, e.g. file://notification.json
    #[arg(
        long,
        value_name = "TopicConfigurations=[{TopicArn=string,Events=[string,...]},...],..."
    )]
    pub notification_configuration: String,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    #[command(flatten)]
    pub skip_destination_validation: SkipDestinationValidationArgs,
}

#[derive(Debug, Args)]
pub struct SkipDestinationValidationArgs {
    /// Skip the validation of the destinations of the notifications
    #[arg(long, overrides_with = "no_skip_destination_validation")]
    skip_destination_validation: bool,

    /// Do not skip the validation of the destinations of the notifications
    #[arg(long, overrides_with = "skip_destination_validation")]
    no_skip_destination_validation: bool,
}

impl SkipDestinationValidationArgs {
    pub fn value(&self) -> Option<bool> {
        if self.skip_destination_validation {
            Some(true)
        } else if self.no_skip_destination_validation {
            Some(false)
        } else {
            None
        }
    }
}

#[derive(Debug, Args)]
pub struct PutBucketLoggingArgs {
    /// The name of the bucket
    #[arg(long)]
    pub bucket: String,

    /// The logging status, e.g. file://logging.json
    #[arg(
        long,
        value_name = "LoggingEnabled={TargetBucket=string,TargetPrefix=string,...}"
    )]
    pub bucket_logging_status: String,

    /// The base64-encoded 128-bit MD5 digest of the request body
    #[arg(long, value_name = "STRING")]
    pub content_md5: Option<String>,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
}

#[derive(Debug, Args)]
pub struct PutBucketTaggingArgs {
    /// The name of the bucket
    #[arg(long)]
    pub bucket: String,

    /// The tags of the bucket
    #[arg(long, value_name = "TagSet=[{Key=string,Value=string},...]")]
    pub tagging: String,

    /// The base64-encoded 128-bit MD5 digest of the request body
    #[arg(long, value_name = "STRING")]
    pub content_md5: Option<String>,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
}

#[derive(Debug, Args)]
pub struct GetObjectTaggingArgs {
    /// The name of the bucket that contains the object
    #[arg(long)]
    pub bucket: String,

    /// The key of the object
    #[arg(long)]
    pub key: String,

    /// The version ID of the object
    #[arg(long)]
    pub version_id: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,
}

#[derive(Debug, Args)]
pub struct PutObjectTaggingArgs {
    /// The name of the bucket that contains the object
    #[arg(long)]
    pub bucket: String,

    /// The key of the object
    #[arg(long)]
    pub key: String,

    /// The version ID of the object
    #[arg(long)]
    pub version_id: Option<String>,

    /// The base64-encoded 128-bit MD5 digest of the request body
    #[arg(long, value_name = "STRING")]
    pub content_md5: Option<String>,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    /// The tags of the object
    #[arg(long, value_name = "TagSet=[{Key=string,Value=string},...]")]
    pub tagging: String,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,
}

#[derive(Debug, Args)]
pub struct DeleteObjectTaggingArgs {
    /// The name of the bucket that contains the object
    #[arg(long)]
    pub bucket: String,

    /// The key of the object
    #[arg(long)]
    pub key: String,

    /// The version ID of the object
    #[arg(long)]
    pub version_id: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
}

#[derive(Debug, Args)]
pub struct RestoreObjectArgs {
    /// The name of the bucket that contains the object
    #[arg(long)]
    pub bucket: String,

    /// The key of the object to restore
    #[arg(long)]
    pub key: String,

    /// The version ID of the object to restore
    #[arg(long)]
    pub version_id: Option<String>,

    /// The parameters of the restoration
    #[arg(
        long,
        value_name = "Days=integer,GlacierJobParameters={Tier=string},Description=string"
    )]
    pub restore_request: Option<String>,

    /// Confirms that the requester knows that they will be charged for the request
    #[arg(long, value_name = "STRING")]
    pub request_payer: Option<String>,

    /// The algorithm used to create the checksum of the request
    #[arg(long, value_name = "STRING")]
    pub checksum_algorithm: Option<String>,

    /// The account ID of the expected bucket owner
    #[arg(long, value_name = "STRING")]
    pub expected_bucket_owner: Option<String>,
}
