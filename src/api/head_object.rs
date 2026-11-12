//! The head-object operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::operation::head_object::HeadObjectOutput;
use aws_sdk_s3::primitives::DateTime;
use aws_sdk_s3::types::{ChecksumMode, RequestPayer};

use super::output::Response;
use super::{ResponseHeaders, parse_int, parse_timestamp, sse_customer_key};
use crate::cli::HeadObjectArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

pub async fn run(client: &Client, args: &HeadObjectArgs) -> Result<Value, Error> {
    let if_modified_since = parse_timestamp("IfModifiedSince", args.if_modified_since.as_deref())?;
    let if_unmodified_since =
        parse_timestamp("IfUnmodifiedSince", args.if_unmodified_since.as_deref())?;
    let response_expires = parse_timestamp("ResponseExpires", args.response_expires.as_deref())?;
    let part_number = parse_int(args.part_number.as_deref())?;
    let (sse_customer_key, sse_customer_key_md5) = sse_customer_key(
        args.sse_customer_key.as_deref(),
        args.sse_customer_key_md5.as_deref(),
    )?;
    let headers = ResponseHeaders::default();
    let output = Redirector::default()
        .send(client, &args.bucket, "HeadObject", |client| {
            client
                .head_object()
                .bucket(&args.bucket)
                .key(&args.key)
                .set_if_match(args.if_match.clone())
                .set_if_modified_since(if_modified_since)
                .set_if_none_match(args.if_none_match.clone())
                .set_if_unmodified_since(if_unmodified_since)
                .set_range(args.range.clone())
                .set_response_cache_control(args.response_cache_control.clone())
                .set_response_content_disposition(args.response_content_disposition.clone())
                .set_response_content_encoding(args.response_content_encoding.clone())
                .set_response_content_language(args.response_content_language.clone())
                .set_response_content_type(args.response_content_type.clone())
                .set_response_expires(response_expires)
                .set_version_id(args.version_id.clone())
                .set_sse_customer_algorithm(args.sse_customer_algorithm.clone())
                .set_sse_customer_key(sse_customer_key.clone())
                .set_sse_customer_key_md5(sse_customer_key_md5.clone())
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
                .set_part_number(part_number)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .set_checksum_mode(args.checksum_mode.as_deref().map(ChecksumMode::from))
                .customize()
                .interceptor(headers.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("HeadObject", err))?;
    Ok(response(&output, &headers))
}

fn response(output: &HeadObjectOutput, headers: &ResponseHeaders) -> Value {
    let mut response = Response::default();
    response.bool("DeleteMarker", output.delete_marker());
    response.str("AcceptRanges", output.accept_ranges());
    response.str("Expiration", output.expiration());
    response.str("Restore", output.restore());
    response.str("ArchiveStatus", output.archive_status());
    response.timestamp("LastModified", output.last_modified());
    response.int("ContentLength", output.content_length());
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
    response.str("ETag", output.e_tag());
    response.int("MissingMeta", output.missing_meta());
    response.str("VersionId", output.version_id());
    response.str("CacheControl", output.cache_control());
    response.str("ContentDisposition", output.content_disposition());
    response.str("ContentEncoding", output.content_encoding());
    response.str("ContentLanguage", output.content_language());
    response.str("ContentType", output.content_type());
    response.str("ContentRange", output.content_range());
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
fn expires(output: &HeadObjectOutput) -> Option<DateTime> {
    #[allow(deprecated)]
    output.expires().copied()
}
