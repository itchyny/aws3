use std::sync::Arc;

use aws_sdk_s3::primitives::{ByteStream, Length};
use aws_sdk_s3::types::{ChecksumAlgorithm, CompletedMultipartUpload, CompletedPart};
use tokio::io::AsyncReadExt;
use tokio::task::JoinSet;

use super::params::{set_object_params, set_sse_c};
use super::{Context, Outcome, TransferId};
use crate::error::{Error, has_error_code};
use crate::mime::guess_content_type;

/// The maximum number of parts of a multipart upload.
const MAX_PARTS: u64 = 10_000;
/// The minimum size of a part of a multipart upload.
const MIN_PART_SIZE: u64 = 5 * 1024 * 1024;
/// The maximum size of a part of a multipart upload.
const MAX_PART_SIZE: u64 = 5 * 1024 * 1024 * 1024;

/// Uploads the local file to the object.
pub async fn upload_file(
    ctx: &Arc<Context>,
    id: TransferId,
    path: &str,
    bucket: &str,
    key: &str,
) -> Result<Outcome, String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|err| err.to_string())?;
    if metadata.is_dir() {
        return Err(format!("[Errno 21] Is a directory: '{path}'"));
    }
    let size = metadata.len();
    let content_type = content_type(ctx, path);
    if size < ctx.config.multipart_threshold {
        let body = ByteStream::read_from()
            .path(path)
            .build()
            .await
            .map_err(|err| err.to_string())?;
        let outcome = put_object(ctx, bucket, key, body, size, content_type).await?;
        ctx.progress(id, size);
        return Ok(outcome);
    }
    let chunk_size = adjust_chunk_size(ctx.config.multipart_chunksize, Some(size));
    let upload = MultipartUpload::create(ctx, bucket, key, content_type).await?;
    let mut parts = JoinSet::new();
    let mut offset = 0;
    let mut number = 1;
    while offset < size {
        let length = chunk_size.min(size - offset);
        let (ctx, upload, path) = (ctx.clone(), upload.clone(), path.to_owned());
        parts.spawn(async move {
            let body = ByteStream::read_from()
                .path(path)
                .offset(offset)
                .length(Length::Exact(length))
                .build()
                .await
                .map_err(|err| err.to_string())?;
            let part = upload.upload_part(&ctx, number, body, length).await?;
            ctx.progress(id, length);
            Ok(part)
        });
        offset += length;
        number += 1;
    }
    upload.finish(ctx, Vec::new(), parts).await
}

/// Uploads the standard input to the object, where the size is unknown
/// unless the expected size is specified.
pub async fn upload_stdin(
    ctx: &Arc<Context>,
    id: TransferId,
    bucket: &str,
    key: &str,
) -> Result<Outcome, String> {
    let mut stdin = tokio::io::stdin();
    let threshold = ctx.config.multipart_threshold;
    let mut pending = read_chunk(&mut stdin, threshold).await?;
    if (pending.len() as u64) < threshold {
        let size = pending.len() as u64;
        let outcome = put_object(ctx, bucket, key, ByteStream::from(pending), size, None).await?;
        ctx.progress(id, size);
        return Ok(outcome);
    }
    let chunk_size = adjust_chunk_size(ctx.config.multipart_chunksize, ctx.options.expected_size);
    let upload = MultipartUpload::create(ctx, bucket, key, None).await?;
    let mut completed = Vec::new();
    let mut parts = JoinSet::new();
    let mut number = 1;
    let mut eof = false;
    loop {
        while (pending.len() as u64) < chunk_size && !eof {
            let size = chunk_size - pending.len() as u64;
            match read_chunk(&mut stdin, size).await {
                Ok(chunk) => {
                    eof = (chunk.len() as u64) < size;
                    pending.extend_from_slice(&chunk);
                }
                Err(err) => return Err(upload.fail(ctx, &mut parts, err).await),
            }
        }
        if pending.is_empty() {
            break;
        }
        let rest = pending.split_off(pending.len().min(chunk_size as usize));
        let body = std::mem::replace(&mut pending, rest);
        let length = body.len() as u64;
        let (part_ctx, part_upload) = (ctx.clone(), upload.clone());
        parts.spawn(async move {
            let part = part_upload
                .upload_part(&part_ctx, number, ByteStream::from(body), length)
                .await?;
            part_ctx.progress(id, length);
            Ok(part)
        });
        number += 1;
        // Keep the number of buffered parts bounded.
        while parts.len() >= ctx.config.max_concurrent_requests {
            match join_part(parts.join_next().await) {
                Some(Ok(part)) => completed.push(part),
                Some(Err(err)) => return Err(upload.fail(ctx, &mut parts, err).await),
                None => {}
            }
        }
        if eof && pending.is_empty() {
            break;
        }
    }
    upload.finish(ctx, completed, parts).await
}

async fn read_chunk(reader: &mut tokio::io::Stdin, size: u64) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    reader
        .take(size)
        .read_to_end(&mut buffer)
        .await
        .map_err(|err| err.to_string())?;
    Ok(buffer)
}

fn content_type(ctx: &Context, path: &str) -> Option<String> {
    if ctx.params.content_type.is_none() && ctx.options.guess_mime_type {
        guess_content_type(path).map(str::to_owned)
    } else {
        None
    }
}

/// Returns the checksum algorithm of uploads, which defaults to CRC32 like
/// the AWS CLI.
fn checksum_algorithm(ctx: &Context) -> ChecksumAlgorithm {
    ctx.params
        .checksum_algorithm
        .clone()
        .unwrap_or(ChecksumAlgorithm::Crc32)
}

async fn put_object(
    ctx: &Context,
    bucket: &str,
    key: &str,
    body: ByteStream,
    size: u64,
    content_type: Option<String>,
) -> Result<Outcome, String> {
    let params = &ctx.params;
    let request = ctx
        .client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(body)
        .content_length(size as i64);
    let request = set_object_params!(request, params)
        .set_checksum_algorithm(Some(checksum_algorithm(ctx)))
        .set_if_none_match(params.no_overwrite.then(|| "*".to_owned()));
    let request = set_sse_c!(request, &params.sse_c);
    let has_content_type = content_type.is_some() || params.content_type.is_some();
    let request = request.set_content_type(content_type.or(params.content_type.clone()));
    let _permit = ctx
        .requests
        .acquire()
        .await
        .map_err(|err| err.to_string())?;
    // The SDK sends the default content type of the body, whereas the AWS CLI
    // leaves it to S3, which defaults to binary/octet-stream.
    let result = request
        .customize()
        .mutate_request(move |request| {
            if !has_content_type {
                request.headers_mut().remove("content-type");
            }
        })
        .send()
        .await;
    match result {
        Ok(_) => Ok(Outcome::Done),
        Err(err) if params.no_overwrite && has_error_code(&err, "PreconditionFailed") => {
            Ok(Outcome::Skipped)
        }
        Err(err) => Err(Error::sdk("PutObject", err).to_string()),
    }
}

/// Adjusts the chunk size so that the number of parts and the size of each
/// part are within the limits, in the same way as `ChunksizeAdjuster`.
pub fn adjust_chunk_size(chunk_size: u64, size: Option<u64>) -> u64 {
    let mut chunk_size = chunk_size.clamp(MIN_PART_SIZE, MAX_PART_SIZE);
    if let Some(size) = size {
        while size.div_ceil(chunk_size) > MAX_PARTS {
            chunk_size *= 2;
        }
    }
    chunk_size.min(MAX_PART_SIZE)
}

/// A multipart upload, which is shared by the tasks of the parts.
#[derive(Clone)]
pub struct MultipartUpload {
    pub bucket: String,
    pub key: String,
    pub upload_id: String,
    pub checksum_algorithm: Option<ChecksumAlgorithm>,
}

pub type PartResult = Result<CompletedPart, String>;

fn join_part(result: Option<Result<PartResult, tokio::task::JoinError>>) -> Option<PartResult> {
    result.map(|result| result.map_err(|err| err.to_string()).and_then(|part| part))
}

impl MultipartUpload {
    async fn create(
        ctx: &Context,
        bucket: &str,
        key: &str,
        content_type: Option<String>,
    ) -> Result<Self, String> {
        let params = &ctx.params;
        let checksum_algorithm = checksum_algorithm(ctx);
        let request = ctx.client.create_multipart_upload().bucket(bucket).key(key);
        let request =
            set_object_params!(request, params).checksum_algorithm(checksum_algorithm.clone());
        let request = set_sse_c!(request, &params.sse_c);
        let request = match content_type {
            Some(content_type) => request.content_type(content_type),
            None => request,
        };
        let _permit = ctx
            .requests
            .acquire()
            .await
            .map_err(|err| err.to_string())?;
        let output = request
            .send()
            .await
            .map_err(|err| Error::sdk("CreateMultipartUpload", err).to_string())?;
        Ok(Self {
            bucket: bucket.to_owned(),
            key: key.to_owned(),
            upload_id: output.upload_id().unwrap_or_default().to_owned(),
            checksum_algorithm: Some(checksum_algorithm),
        })
    }

    async fn upload_part(
        &self,
        ctx: &Context,
        number: i32,
        body: ByteStream,
        length: u64,
    ) -> PartResult {
        let params = &ctx.params;
        let request = ctx
            .client
            .upload_part()
            .bucket(&self.bucket)
            .key(&self.key)
            .upload_id(&self.upload_id)
            .part_number(number)
            .body(body)
            .content_length(length as i64)
            .set_checksum_algorithm(self.checksum_algorithm.clone())
            .set_request_payer(params.request_payer.clone());
        let request = set_sse_c!(request, &params.sse_c);
        let _permit = ctx
            .requests
            .acquire()
            .await
            .map_err(|err| err.to_string())?;
        let output = request
            .send()
            .await
            .map_err(|err| Error::sdk("UploadPart", err).to_string())?;
        Ok(CompletedPart::builder()
            .part_number(number)
            .set_e_tag(output.e_tag().map(str::to_owned))
            .set_checksum_crc32(output.checksum_crc32().map(str::to_owned))
            .set_checksum_crc32_c(output.checksum_crc32_c().map(str::to_owned))
            .set_checksum_crc64_nvme(output.checksum_crc64_nvme().map(str::to_owned))
            .set_checksum_sha1(output.checksum_sha1().map(str::to_owned))
            .set_checksum_sha256(output.checksum_sha256().map(str::to_owned))
            .build())
    }

    /// Waits for the parts and completes the upload, or aborts the upload if
    /// any of the parts failed.
    pub async fn finish(
        &self,
        ctx: &Context,
        mut completed: Vec<CompletedPart>,
        mut parts: JoinSet<PartResult>,
    ) -> Result<Outcome, String> {
        while let Some(result) = join_part(parts.join_next().await) {
            match result {
                Ok(part) => completed.push(part),
                Err(err) => return Err(self.fail(ctx, &mut parts, err).await),
            }
        }
        completed.sort_by_key(|part| part.part_number());
        let params = &ctx.params;
        let request = ctx
            .client
            .complete_multipart_upload()
            .bucket(&self.bucket)
            .key(&self.key)
            .upload_id(&self.upload_id)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(completed))
                    .build(),
            )
            .set_request_payer(params.request_payer.clone())
            .set_if_none_match(params.no_overwrite.then(|| "*".to_owned()));
        let request = set_sse_c!(request, &params.sse_c);
        let _permit = ctx
            .requests
            .acquire()
            .await
            .map_err(|err| err.to_string())?;
        match request.send().await {
            Ok(_) => Ok(Outcome::Done),
            Err(err) => {
                drop(_permit);
                self.abort(ctx).await;
                if params.no_overwrite && has_error_code(&err, "PreconditionFailed") {
                    Ok(Outcome::Skipped)
                } else {
                    Err(Error::sdk("CompleteMultipartUpload", err).to_string())
                }
            }
        }
    }

    async fn abort(&self, ctx: &Context) {
        let _ = ctx
            .client
            .abort_multipart_upload()
            .bucket(&self.bucket)
            .key(&self.key)
            .upload_id(&self.upload_id)
            .set_request_payer(ctx.params.request_payer.clone())
            .send()
            .await;
    }

    /// Cancels the remaining parts and aborts the upload, and returns the
    /// error of the failure.
    pub async fn fail(
        &self,
        ctx: &Context,
        parts: &mut JoinSet<PartResult>,
        err: String,
    ) -> String {
        parts.abort_all();
        self.abort(ctx).await;
        err
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_size() {
        const MIB: u64 = 1024 * 1024;
        assert_eq!(adjust_chunk_size(8 * MIB, None), 8 * MIB);
        assert_eq!(adjust_chunk_size(MIB, Some(100 * MIB)), 5 * MIB);
        assert_eq!(adjust_chunk_size(8 * MIB, Some(80_000 * MIB)), 8 * MIB);
        assert_eq!(adjust_chunk_size(8 * MIB, Some(80_001 * MIB)), 16 * MIB);
    }
}
