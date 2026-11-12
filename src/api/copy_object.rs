//! The copy-object operation.

use std::sync::LazyLock;

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{
    AnnotationDirective, ChecksumAlgorithm, MetadataDirective, ObjectCannedAcl,
    ObjectLockEventHold, ObjectLockLegalHoldStatus, ObjectLockMode, RequestPayer,
    ServerSideEncryption, StorageClass, TaggingDirective,
};
use regex_lite::Regex;

use super::output::Response;
use super::{parse_int, parse_metadata, parse_timestamp};
use crate::cli::CopyObjectArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::transfer::url_encode;
use crate::value::Value;

static VERSION_ID_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\?versionId=[^\s]+$").expect("valid pattern"));

pub async fn run(client: &Client, args: &CopyObjectArgs) -> Result<Value, Error> {
    let copy_source = quote_copy_source(&args.copy_source);
    let copy_source_if_modified_since = parse_timestamp(
        "CopySourceIfModifiedSince",
        args.copy_source_if_modified_since.as_deref(),
    )?;
    let copy_source_if_unmodified_since = parse_timestamp(
        "CopySourceIfUnmodifiedSince",
        args.copy_source_if_unmodified_since.as_deref(),
    )?;
    let expires = parse_timestamp("Expires", args.expires.as_deref())?;
    let metadata = parse_metadata(args.metadata.as_deref())?;
    let object_lock_retain_until_date = parse_timestamp(
        "ObjectLockRetainUntilDate",
        args.object_lock_retain_until_date.as_deref(),
    )?;
    let object_lock_event_hold_duration_days =
        parse_int(args.object_lock_event_hold_duration_days.as_deref())?;
    let object_lock_event_hold_duration_years =
        parse_int(args.object_lock_event_hold_duration_years.as_deref())?;
    let (sse_customer_key, sse_customer_key_md5) = super::sse_customer_key(
        "--sse-customer-key",
        args.sse_customer_key.as_deref(),
        args.sse_customer_key_md5.as_deref(),
    )?;
    let (copy_source_sse_customer_key, copy_source_sse_customer_key_md5) = super::sse_customer_key(
        "--copy-source-sse-customer-key",
        args.copy_source_sse_customer_key.as_deref(),
        args.copy_source_sse_customer_key_md5.as_deref(),
    )?;
    let output = Redirector::default()
        .send(client, &args.bucket, "CopyObject", |client| {
            client
                .copy_object()
                .set_acl(args.acl.as_deref().map(ObjectCannedAcl::from))
                .bucket(&args.bucket)
                .set_cache_control(args.cache_control.clone())
                .set_checksum_algorithm(
                    args.checksum_algorithm
                        .as_deref()
                        .map(ChecksumAlgorithm::from),
                )
                .set_content_disposition(args.content_disposition.clone())
                .set_content_encoding(args.content_encoding.clone())
                .set_content_language(args.content_language.clone())
                .set_content_type(args.content_type.clone())
                .copy_source(&copy_source)
                .set_copy_source_if_match(args.copy_source_if_match.clone())
                .set_copy_source_if_modified_since(copy_source_if_modified_since)
                .set_copy_source_if_none_match(args.copy_source_if_none_match.clone())
                .set_copy_source_if_unmodified_since(copy_source_if_unmodified_since)
                .set_expires(expires)
                .set_grant_full_control(args.grant_full_control.clone())
                .set_grant_read(args.grant_read.clone())
                .set_grant_read_acp(args.grant_read_acp.clone())
                .set_grant_write_acp(args.grant_write_acp.clone())
                .set_if_match(args.if_match.clone())
                .set_if_none_match(args.if_none_match.clone())
                .key(&args.key)
                .set_metadata(metadata.clone())
                .set_metadata_directive(
                    args.metadata_directive
                        .as_deref()
                        .map(MetadataDirective::from),
                )
                .set_tagging_directive(
                    args.tagging_directive
                        .as_deref()
                        .map(TaggingDirective::from),
                )
                .set_annotation_directive(
                    args.annotation_directive
                        .as_deref()
                        .map(AnnotationDirective::from),
                )
                .set_server_side_encryption(
                    args.server_side_encryption
                        .as_deref()
                        .map(ServerSideEncryption::from),
                )
                .set_storage_class(args.storage_class.as_deref().map(StorageClass::from))
                .set_website_redirect_location(args.website_redirect_location.clone())
                .set_sse_customer_algorithm(args.sse_customer_algorithm.clone())
                .set_sse_customer_key(sse_customer_key.clone())
                .set_sse_customer_key_md5(sse_customer_key_md5.clone())
                .set_ssekms_key_id(args.ssekms_key_id.clone())
                .set_ssekms_encryption_context(args.ssekms_encryption_context.clone())
                .set_bucket_key_enabled(args.bucket_key_enabled.value())
                .set_copy_source_sse_customer_algorithm(
                    args.copy_source_sse_customer_algorithm.clone(),
                )
                .set_copy_source_sse_customer_key(copy_source_sse_customer_key.clone())
                .set_copy_source_sse_customer_key_md5(copy_source_sse_customer_key_md5.clone())
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
                .set_tagging(args.tagging.clone())
                .set_object_lock_mode(args.object_lock_mode.as_deref().map(ObjectLockMode::from))
                .set_object_lock_retain_until_date(object_lock_retain_until_date)
                .set_object_lock_legal_hold_status(
                    args.object_lock_legal_hold_status
                        .as_deref()
                        .map(ObjectLockLegalHoldStatus::from),
                )
                .set_object_lock_event_hold(
                    args.object_lock_event_hold
                        .as_deref()
                        .map(ObjectLockEventHold::from),
                )
                .set_object_lock_event_hold_duration_days(object_lock_event_hold_duration_days)
                .set_object_lock_event_hold_duration_years(object_lock_event_hold_duration_years)
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .set_expected_source_bucket_owner(args.expected_source_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("CopyObject", err))?;
    let mut response = Response::default();
    if let Some(result) = output.copy_object_result() {
        let mut copy_object_result = Response::default();
        copy_object_result.str("ETag", result.e_tag());
        copy_object_result.timestamp("LastModified", result.last_modified());
        copy_object_result.str("ChecksumType", result.checksum_type());
        copy_object_result.str("ChecksumCRC32", result.checksum_crc32());
        copy_object_result.str("ChecksumCRC32C", result.checksum_crc32_c());
        copy_object_result.str("ChecksumCRC64NVME", result.checksum_crc64_nvme());
        copy_object_result.str("ChecksumSHA1", result.checksum_sha1());
        copy_object_result.str("ChecksumSHA256", result.checksum_sha256());
        copy_object_result.str("ChecksumSHA512", result.checksum_sha512());
        copy_object_result.str("ChecksumMD5", result.checksum_md5());
        copy_object_result.str("ChecksumXXHASH64", result.checksum_xxhash64());
        copy_object_result.str("ChecksumXXHASH3", result.checksum_xxhash3());
        copy_object_result.str("ChecksumXXHASH128", result.checksum_xxhash128());
        response.value("CopyObjectResult", copy_object_result.build());
    }
    response.str("Expiration", output.expiration());
    response.str("CopySourceVersionId", output.copy_source_version_id());
    response.str("VersionId", output.version_id());
    response.str("ServerSideEncryption", output.server_side_encryption());
    response.str("SSECustomerAlgorithm", output.sse_customer_algorithm());
    response.str("SSECustomerKeyMD5", output.sse_customer_key_md5());
    response.str("SSEKMSKeyId", output.ssekms_key_id());
    response.str(
        "SSEKMSEncryptionContext",
        output.ssekms_encryption_context(),
    );
    response.bool("BucketKeyEnabled", output.bucket_key_enabled());
    response.str("RequestCharged", output.request_charged());
    Ok(response.build())
}

/// Encodes the copy source except for the slashes and the version ID suffix,
/// like `handle_copy_source_param` of botocore.
fn quote_copy_source(value: &str) -> String {
    match VERSION_ID_SUFFIX.find(value) {
        Some(suffix) => format!(
            "{}{}",
            url_encode(&value[..suffix.start()], true),
            suffix.as_str()
        ),
        None => url_encode(value, true),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn quote_copy_source() {
        assert_eq!(
            super::quote_copy_source("bucket/a b+\u{3042}.txt"),
            "bucket/a%20b%2B%E3%81%82.txt"
        );
        assert_eq!(
            super::quote_copy_source("bucket/a b?versionId=x y?versionId=v1"),
            "bucket/a%20b%3FversionId%3Dx%20y?versionId=v1"
        );
    }
}
