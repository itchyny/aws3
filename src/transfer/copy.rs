use std::sync::Arc;

use aws_sdk_s3::operation::head_object::HeadObjectOutput;
use aws_sdk_s3::types::{
    AnnotationDirective, CompletedPart, MetadataDirective, Tag, Tagging, TaggingDirective,
};
use tokio::task::JoinSet;

use super::download::etag_mismatch;
use super::generator::ObjectProps;
use super::params::{set_copy_source_sse_c, set_object_params, set_sse_c};
use super::upload::{MultipartUpload, adjust_chunk_size};
use super::{Context, Outcome, TransferId};
use crate::cli::CopyProps;
use crate::error::{Error, has_error_code};
use crate::path::validate_bucket_name;

/// Sets the properties replaced by the REPLACE metadata directive, taking
/// those not specified from the HeadObject response of the source object.
macro_rules! set_source_props {
    ($builder:expr, $params:expr, $head:expr) => {
        $builder
            .set_cache_control(
                $params
                    .cache_control
                    .clone()
                    .or($head.cache_control().map(str::to_owned)),
            )
            .set_content_disposition(
                $params
                    .content_disposition
                    .clone()
                    .or($head.content_disposition().map(str::to_owned)),
            )
            .set_content_encoding(
                $params
                    .content_encoding
                    .clone()
                    .or($head.content_encoding().map(str::to_owned)),
            )
            .set_content_language(
                $params
                    .content_language
                    .clone()
                    .or($head.content_language().map(str::to_owned)),
            )
            .set_content_type(
                $params
                    .content_type
                    .clone()
                    .or($head.content_type().map(str::to_owned)),
            )
            .set_expires(
                $params
                    .expires
                    .or_else(|| parse_http_date($head.expires_string())),
            )
            .set_metadata($params.metadata.clone().or($head.metadata().cloned()))
    };
}

fn parse_http_date(value: Option<&str>) -> Option<aws_sdk_s3::primitives::DateTime> {
    aws_sdk_s3::primitives::DateTime::from_str(
        value?,
        aws_smithy_types::date_time::Format::HttpDate,
    )
    .ok()
}

/// The maximum size of the tagging header, which is otherwise set by
/// PutObjectTagging after the copy.
const MAX_TAGGING_HEADER_SIZE: usize = 2 * 1024;

/// Copies the object to the object, following the copy props option in the
/// same way as the AWS CLI.
#[allow(clippy::too_many_arguments)]
pub async fn copy_object(
    ctx: &Arc<Context>,
    id: TransferId,
    src_bucket: &str,
    src_key: &str,
    bucket: &str,
    key: &str,
    size: u64,
    object: Option<&ObjectProps>,
) -> Result<Outcome, String> {
    validate_bucket_name(bucket)?;
    let mut head = object.and_then(|object| object.head.as_deref());
    let mut size = size;
    let mut etag = object.and_then(|object| object.etag.clone());
    // The size and the ETag are retrieved when the ETag is unavailable on
    // listing, like s3transfer, so that both are of the same object.
    let fetched;
    if etag.is_none() {
        fetched = head_source(ctx, src_bucket, src_key, head).await?;
        size = fetched.content_length().unwrap_or_default().max(0) as u64;
        etag = fetched.e_tag().map(str::to_owned);
        head = Some(&fetched);
    }
    if size < ctx.config.multipart_threshold {
        let outcome = copy_single(ctx, src_bucket, src_key, bucket, key, head).await?;
        ctx.progress(id, size);
        Ok(outcome)
    } else {
        copy_multipart(ctx, id, src_bucket, src_key, bucket, key, size, head, etag).await
    }
}

/// The copy props option, which is disabled by the metadata directive.
fn copy_props(ctx: &Context) -> Option<CopyProps> {
    ctx.params
        .metadata_directive
        .is_none()
        .then_some(ctx.options.copy_props)
}

async fn copy_single(
    ctx: &Context,
    src_bucket: &str,
    src_key: &str,
    bucket: &str,
    key: &str,
    head: Option<&HeadObjectOutput>,
) -> Result<Outcome, String> {
    let params = &ctx.params;
    let mut metadata_directive = params.metadata_directive.clone();
    if params.metadata.is_some() && metadata_directive.is_none() {
        metadata_directive = Some(MetadataDirective::Replace);
    }
    let mut tagging_directive = None;
    let mut annotation_directive = None;
    let mut inject = None;
    match copy_props(ctx) {
        Some(CopyProps::None) => {
            metadata_directive = Some(MetadataDirective::Replace);
            tagging_directive = Some(TaggingDirective::Replace);
            annotation_directive = Some(AnnotationDirective::Exclude);
        }
        Some(copy_props) => {
            // Properties not specified are taken from the source object, since
            // the REPLACE directive replaces all of them.
            if params.has_metadata_props() {
                metadata_directive = Some(MetadataDirective::Replace);
                inject = Some(head_source(ctx, src_bucket, src_key, head).await?);
            }
            if copy_props == CopyProps::MetadataDirective {
                tagging_directive = Some(TaggingDirective::Replace);
            }
            if copy_props != CopyProps::All {
                annotation_directive = Some(AnnotationDirective::Exclude);
            }
        }
        None => {}
    }
    params.validate_ascii_metadata()?;
    let _permit = ctx.permit().await?;
    let result = ctx
        .redirector
        .send(&ctx.client, bucket, "CopyObject", |client| {
            let request = client
                .copy_object()
                .bucket(bucket)
                .key(key)
                .copy_source(copy_source(src_bucket, src_key));
            let request = set_object_params!(request, params)
                .set_metadata_directive(metadata_directive.clone())
                .set_tagging_directive(tagging_directive.clone())
                .set_annotation_directive(annotation_directive.clone())
                .set_checksum_algorithm(params.checksum_algorithm.clone())
                .set_if_none_match(params.no_overwrite.then(|| "*".to_owned()));
            let request = set_sse_c!(request, &params.sse_c);
            let mut request = set_copy_source_sse_c!(request, &params.sse_c_copy_source);
            if let Some(head) = &inject {
                request = set_source_props!(request, params, head);
            }
            request.send()
        })
        .await;
    match result {
        Ok(_) => Ok(Outcome::Done),
        Err(err) if params.no_overwrite && has_error_code(&err, "PreconditionFailed") => {
            Ok(Outcome::Skipped)
        }
        Err(err) => Err(Error::sdk("CopyObject", err).to_string()),
    }
}

#[allow(clippy::too_many_arguments)]
async fn copy_multipart(
    ctx: &Arc<Context>,
    id: TransferId,
    src_bucket: &str,
    src_key: &str,
    bucket: &str,
    key: &str,
    size: u64,
    head: Option<&HeadObjectOutput>,
    etag: Option<String>,
) -> Result<Outcome, String> {
    let params = &ctx.params;
    let copy_props = copy_props(ctx);
    // Multipart copies do not copy the properties, so they are taken from
    // the source object.
    let source = match copy_props {
        Some(CopyProps::MetadataDirective | CopyProps::Default | CopyProps::All) => {
            Some(head_source(ctx, src_bucket, src_key, head).await?)
        }
        _ => None,
    };
    let tags = match copy_props {
        Some(CopyProps::Default | CopyProps::All) => get_tags(ctx, src_bucket, src_key).await?,
        _ => Vec::new(),
    };
    let tagging_header = serialize_tags(&tags);
    let put_tags_later = !tags.is_empty() && tagging_header.len() > MAX_TAGGING_HEADER_SIZE;
    let tagging = (!tags.is_empty() && !put_tags_later).then_some(tagging_header);
    params.validate_ascii_metadata()?;
    let output = {
        let _permit = ctx.permit().await?;
        ctx.redirector
            .send(&ctx.client, bucket, "CreateMultipartUpload", |client| {
                let request = client.create_multipart_upload().bucket(bucket).key(key);
                let mut request = set_object_params!(request, params)
                    .set_checksum_algorithm(params.checksum_algorithm.clone())
                    .set_tagging(tagging.clone());
                request = set_sse_c!(request, &params.sse_c);
                if let Some(head) = &source {
                    request = set_source_props!(request, params, head);
                }
                request.send()
            })
            .await
            .map_err(|err| Error::sdk("CreateMultipartUpload", err).to_string())?
    };
    let upload = MultipartUpload {
        bucket: bucket.to_owned(),
        key: key.to_owned(),
        upload_id: output.upload_id().unwrap_or_default().to_owned(),
        checksum_algorithm: params.checksum_algorithm.clone(),
        done: Arc::default(),
    };
    let _guard = upload.abort_guard(ctx);
    let chunk_size = adjust_chunk_size(ctx.config.multipart_chunksize, Some(size));
    let source = Arc::new(PartSource {
        bucket: src_bucket.to_owned(),
        key: src_key.to_owned(),
        etag,
    });
    let mut completed = Vec::new();
    let mut parts = JoinSet::new();
    let mut offset = 0;
    let mut number = 1;
    while offset < size {
        let limit = ctx.config.max_concurrent_parts;
        upload
            .wait_parts(ctx, &mut parts, &mut completed, limit)
            .await?;
        let end = (offset + chunk_size).min(size) - 1;
        let (ctx, upload, source) = (ctx.clone(), upload.clone(), source.clone());
        parts.spawn(async move {
            let part = upload_part_copy(&ctx, &upload, number, &source, offset, end).await?;
            ctx.progress(id, end - offset + 1);
            Ok(part)
        });
        offset = end + 1;
        number += 1;
    }
    let outcome = upload.finish(ctx, completed, parts).await?;
    if put_tags_later && outcome == Outcome::Done {
        let permit = ctx.permit().await?;
        let tagging = Tagging::builder()
            .set_tag_set(Some(tags))
            .build()
            .map_err(|err| err.to_string())?;
        let result = ctx
            .redirector
            .send(&ctx.client, bucket, "PutObjectTagging", |client| {
                client
                    .put_object_tagging()
                    .bucket(bucket)
                    .key(key)
                    .tagging(tagging.clone())
                    .set_request_payer(params.request_payer.clone())
                    .send()
            })
            .await;
        drop(permit);
        if let Err(err) = result {
            let _permit = ctx.permit().await?;
            let _ = ctx
                .redirector
                .send(&ctx.client, bucket, "DeleteObject", |client| {
                    client
                        .delete_object()
                        .bucket(bucket)
                        .key(key)
                        .set_request_payer(params.request_payer.clone())
                        .send()
                })
                .await;
            return Err(Error::sdk("PutObjectTagging", err).to_string());
        }
    }
    Ok(outcome)
}

/// The source object of the part copies, with the ETag to ensure that the
/// object is not modified during the copy.
struct PartSource {
    bucket: String,
    key: String,
    etag: Option<String>,
}

async fn upload_part_copy(
    ctx: &Context,
    upload: &MultipartUpload,
    number: i32,
    source: &PartSource,
    start: u64,
    end: u64,
) -> Result<CompletedPart, String> {
    let params = &ctx.params;
    let _permit = ctx.permit().await?;
    let result = ctx
        .redirector
        .send(&ctx.client, &upload.bucket, "UploadPartCopy", |client| {
            let request = client
                .upload_part_copy()
                .bucket(&upload.bucket)
                .key(&upload.key)
                .upload_id(&upload.upload_id)
                .part_number(number)
                .copy_source(copy_source(&source.bucket, &source.key))
                .copy_source_range(format!("bytes={start}-{end}"))
                .set_copy_source_if_match(source.etag.clone())
                .set_request_payer(params.request_payer.clone());
            let request = set_sse_c!(request, &params.sse_c);
            set_copy_source_sse_c!(request, &params.sse_c_copy_source).send()
        })
        .await;
    let output = result.map_err(|err| {
        if has_error_code(&err, "PreconditionFailed") {
            etag_mismatch(&source.bucket, &source.key)
        } else {
            Error::sdk("UploadPartCopy", err).to_string()
        }
    })?;
    let result = output.copy_part_result();
    Ok(CompletedPart::builder()
        .part_number(number)
        .set_e_tag(result.and_then(|r| r.e_tag()).map(str::to_owned))
        .set_checksum_crc32(result.and_then(|r| r.checksum_crc32()).map(str::to_owned))
        .set_checksum_crc32_c(result.and_then(|r| r.checksum_crc32_c()).map(str::to_owned))
        .set_checksum_crc64_nvme(
            result
                .and_then(|r| r.checksum_crc64_nvme())
                .map(str::to_owned),
        )
        .set_checksum_sha1(result.and_then(|r| r.checksum_sha1()).map(str::to_owned))
        .set_checksum_sha256(result.and_then(|r| r.checksum_sha256()).map(str::to_owned))
        .build())
}

/// Returns the HeadObject response of the source object, which is already
/// available for a single object.
async fn head_source(
    ctx: &Context,
    bucket: &str,
    key: &str,
    head: Option<&HeadObjectOutput>,
) -> Result<HeadObjectOutput, String> {
    if let Some(head) = head {
        return Ok(head.clone());
    }
    let _permit = ctx.permit().await?;
    ctx.redirector
        .send(&ctx.source_client, bucket, "HeadObject", |client| {
            let request = client
                .head_object()
                .bucket(bucket)
                .key(key)
                .set_request_payer(ctx.params.request_payer.clone());
            set_sse_c!(request, &ctx.params.sse_c_copy_source).send()
        })
        .await
        .map_err(|err| Error::sdk("HeadObject", err).to_string())
}

async fn get_tags(ctx: &Context, bucket: &str, key: &str) -> Result<Vec<Tag>, String> {
    let _permit = ctx.permit().await?;
    let output = ctx
        .redirector
        .send(&ctx.source_client, bucket, "GetObjectTagging", |client| {
            client
                .get_object_tagging()
                .bucket(bucket)
                .key(key)
                .set_request_payer(ctx.params.request_payer.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("GetObjectTagging", err).to_string())?;
    Ok(output.tag_set().to_vec())
}

/// Serializes the tags to the value of the tagging header.
fn serialize_tags(tags: &[Tag]) -> String {
    tags.iter()
        .map(|tag| {
            format!(
                "{}={}",
                url_encode(tag.key(), false),
                url_encode(tag.value(), false)
            )
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Returns the copy source of the object, where the key is URL encoded.
pub fn copy_source(bucket: &str, key: &str) -> String {
    format!("{bucket}/{}", url_encode(key, true))
}

/// Encodes the string, keeping unreserved characters and optionally slashes.
fn url_encode(value: &str, keep_slash: bool) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            b'/' if keep_slash => encoded.push('/'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode() {
        assert_eq!(copy_source("b", "dir/a b+c.txt"), "b/dir/a%20b%2Bc.txt");
        assert_eq!(copy_source("b", "ü"), "b/%C3%BC");
    }
}
