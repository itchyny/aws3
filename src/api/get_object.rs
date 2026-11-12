//! The get-object operation.

use std::path::Path;

use aws_sdk_s3::Client;
use aws_sdk_s3::config::ResponseChecksumValidation;
use aws_sdk_s3::operation::get_object::GetObjectOutput;
use aws_sdk_s3::primitives::DateTime;
use aws_sdk_s3::types::ChecksumMode;
use tokio::io::AsyncWriteExt;

use super::ResponseHeaders;
use super::head_object::{ObjectRequest, set_object_request};
use super::output::Response;
use crate::cli::GetObjectArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &GetObjectArgs) -> Result<Value, Error> {
    let object = &args.object;
    let request = ObjectRequest::parse(object)?;
    // The checksum mode is enabled to validate the checksums of the responses
    // unless they are validated only when required, like botocore.
    let checksum_mode = object
        .checksum_mode
        .as_deref()
        .map(ChecksumMode::from)
        .or_else(|| {
            let validation = client.config().response_checksum_validation();
            (validation != Some(&ResponseChecksumValidation::WhenRequired))
                .then_some(ChecksumMode::Enabled)
        });
    let headers = ResponseHeaders::default();
    let output = Redirector::default()
        .send(client, &object.bucket, "GetObject", |client| {
            set_object_request!(client.get_object(), object, &request)
                .set_checksum_mode(checksum_mode.clone())
                .customize()
                .interceptor(headers.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("GetObject", err))?;
    let response = response(&output, &headers);
    write_body(output, &args.outfile).await?;
    Ok(response)
}

/// Writes the body to the file, which is created after the response is
/// received, failing with the same messages as `open` of Python.
async fn write_body(output: GetObjectOutput, path: &Path) -> Result<(), Error> {
    let os_error = |err: std::io::Error| {
        let mut message = err.to_string();
        if let Some(code) = err.raw_os_error() {
            let suffix = format!(" (os error {code})");
            message = format!(
                "[Errno {code}] {}: '{}'",
                message.strip_suffix(&suffix).unwrap_or(&message),
                path.display()
            );
        }
        Error::new(message)
    };
    let mut file = tokio::fs::File::create(path).await.map_err(os_error)?;
    let mut body = output.body;
    while let Some(bytes) = body
        .try_next()
        .await
        .map_err(|err| Error::new(err.to_string()))?
    {
        file.write_all(&bytes).await.map_err(os_error)?;
    }
    file.flush().await.map_err(os_error)
}

fn response(output: &GetObjectOutput, headers: &ResponseHeaders) -> Value {
    let mut response = Response::default();
    response.bool("DeleteMarker", output.delete_marker());
    response.str("AcceptRanges", output.accept_ranges());
    response.str("Expiration", output.expiration());
    response.str("Restore", output.restore());
    response.timestamp("LastModified", output.last_modified());
    response.int("ContentLength", output.content_length());
    response.str("ETag", output.e_tag());
    response.str("ChecksumCRC32", output.checksum_crc32());
    response.str("ChecksumCRC32C", output.checksum_crc32_c());
    response.str("ChecksumCRC64NVME", output.checksum_crc64_nvme());
    response.str("ChecksumSHA1", output.checksum_sha1());
    response.str("ChecksumSHA256", output.checksum_sha256());
    response.str("ChecksumSHA512", output.checksum_sha512());
    response.str("ChecksumMD5", output.checksum_md5());
    response.str("ChecksumXXHASH64", output.checksum_xxhash64());
    response.str("ChecksumXXHASH3", output.checksum_xxhash3());
    response.str("ChecksumXXHASH128", output.checksum_xxhash128());
    response.str("ChecksumType", output.checksum_type());
    response.int("MissingMeta", output.missing_meta());
    response.str("VersionId", output.version_id());
    response.str("CacheControl", output.cache_control());
    response.str("ContentDisposition", output.content_disposition());
    response.str("ContentEncoding", output.content_encoding());
    response.str("ContentLanguage", output.content_language());
    response.str("ContentRange", output.content_range());
    response.str("ContentType", output.content_type());
    response.timestamp("Expires", expires(output).as_ref());
    response.str(
        "WebsiteRedirectLocation",
        output.website_redirect_location(),
    );
    response.str("ServerSideEncryption", output.server_side_encryption());
    response.value("Metadata", headers.metadata());
    response.str("SSECustomerAlgorithm", output.sse_customer_algorithm());
    response.str("SSECustomerKeyMD5", output.sse_customer_key_md5());
    response.str("SSEKMSKeyId", output.ssekms_key_id());
    response.bool("BucketKeyEnabled", output.bucket_key_enabled());
    response.str("StorageClass", output.storage_class());
    response.str("RequestCharged", output.request_charged());
    response.str("ReplicationStatus", output.replication_status());
    response.int("PartsCount", output.parts_count());
    response.int("TagCount", output.tag_count());
    response.str("ObjectLockMode", output.object_lock_mode());
    response.timestamp(
        "ObjectLockRetainUntilDate",
        output.object_lock_retain_until_date(),
    );
    response.str(
        "ObjectLockLegalHoldStatus",
        output.object_lock_legal_hold_status(),
    );
    response.str("ObjectLockEventHold", output.object_lock_event_hold());
    response.int(
        "ObjectLockEventHoldDurationDays",
        output.object_lock_event_hold_duration_days(),
    );
    response.int(
        "ObjectLockEventHoldDurationYears",
        output.object_lock_event_hold_duration_years(),
    );
    // The raw value of the Expires header is added at the end, like
    // `handle_expires_header` of botocore.
    response.str("ExpiresString", output.expires_string());
    response.build()
}

/// Returns the Expires header parsed as a timestamp, which is omitted if it
/// is not a valid timestamp.
fn expires(output: &GetObjectOutput) -> Option<DateTime> {
    #[allow(deprecated)]
    output.expires().copied()
}
