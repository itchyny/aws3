use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::task::{Poll, ready};

use aws_sdk_s3::config::RequestChecksumCalculation;
use aws_sdk_s3::operation::complete_multipart_upload::CompleteMultipartUploadOutput;
use aws_sdk_s3::primitives::{ByteStream, Length};
use aws_sdk_s3::types::{ChecksumAlgorithm, CompletedMultipartUpload, CompletedPart};
use aws_smithy_checksums::ChecksumAlgorithm as Checksums;
use aws_smithy_runtime_api::http::Headers;
use aws_smithy_types::body::{Error as BoxError, SdkBody};
use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use tokio::io::AsyncReadExt;
use tokio::task::JoinSet;

use super::params::{set_object_params, set_sse_c};
use super::{Context, Outcome, TransferId};
use crate::error::{Error, has_error_code};
use crate::mime::guess_content_type;
use crate::path::validate_bucket_name;

/// The number of the parts of the standard input in the memory, which is
/// `max_in_memory_upload_chunks` of s3transfer.
const MAX_IN_MEMORY_UPLOAD_CHUNKS: usize = 10;

/// The size of the buffer to read files, which is the same as the default
/// `io_chunksize` of the AWS CLI. The default of the SDK, 4 KiB, makes many
/// small reads on the blocking threads, which takes much time in the kernel.
const READ_BUFFER_SIZE: usize = 256 * 1024;

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
    validate_bucket_name(bucket)?;
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|err| err.to_string())?;
    if metadata.is_dir() {
        return Err(format!("[Errno 21] Is a directory: '{path}'"));
    }
    let size = metadata.len();
    let content_type = content_type(ctx, path);
    if size < ctx.config.multipart_threshold {
        let body = file_body(ctx, id, path, 0, size).await?;
        return put_object(ctx, bucket, key, body, size, content_type).await;
    }
    let chunk_size = adjust_chunk_size(ctx.config.multipart_chunksize, Some(size));
    let upload = MultipartUpload::create(ctx, bucket, key, content_type).await?;
    let _guard = upload.abort_guard(ctx);
    let mut completed = Vec::new();
    let mut parts = JoinSet::new();
    let mut offset = 0;
    let mut number = 1;
    while offset < size {
        let limit = ctx.config.max_concurrent_parts;
        upload
            .wait_parts(ctx, &mut parts, &mut completed, limit)
            .await?;
        let length = chunk_size.min(size - offset);
        let (ctx, upload, path) = (ctx.clone(), upload.clone(), path.to_owned());
        parts.spawn(async move {
            let body = file_body(&ctx, id, &path, offset, length).await?;
            upload.upload_part(&ctx, number, body, length).await
        });
        offset += length;
        number += 1;
    }
    upload.finish(ctx, completed, parts).await
}

/// Uploads the standard input to the object, where the size is unknown
/// unless the expected size is specified.
pub async fn upload_stdin(
    ctx: &Arc<Context>,
    id: TransferId,
    bucket: &str,
    key: &str,
) -> Result<Outcome, String> {
    validate_bucket_name(bucket)?;
    let mut stdin = tokio::io::stdin();
    let threshold = ctx.config.multipart_threshold;
    let mut pending = read_chunk(&mut stdin, threshold).await?;
    if (pending.len() as u64) < threshold {
        let size = pending.len() as u64;
        let outcome = put_object(ctx, bucket, key, memory_body(ctx, pending), size, None).await?;
        ctx.progress(id, size);
        return Ok(outcome);
    }
    let chunk_size = adjust_chunk_size(ctx.config.multipart_chunksize, ctx.options.expected_size);
    let upload = MultipartUpload::create(ctx, bucket, key, None).await?;
    let _guard = upload.abort_guard(ctx);
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
                .upload_part(&part_ctx, number, memory_body(&part_ctx, body), length)
                .await?;
            part_ctx.progress(id, length);
            Ok(part)
        });
        number += 1;
        // The parts in the memory are bounded in the same way as s3transfer.
        upload
            .wait_parts(ctx, &mut parts, &mut completed, MAX_IN_MEMORY_UPLOAD_CHUNKS)
            .await?;
        if eof && pending.is_empty() {
            break;
        }
    }
    upload.finish(ctx, completed, parts).await
}

/// The body of an upload, with the checksum computed before sending if the
/// payloads are not signed.
pub struct UploadBody {
    stream: ByteStream,
    checksum: Option<String>,
}

/// Returns whether the checksums of the uploads are computed before sending,
/// which disables the aws-chunked encoding of the SDK whose chunks are signed
/// over HTTP, so that the payloads are not signed like the AWS CLI.
fn precomputes_checksum(ctx: &Context) -> Option<ChecksumAlgorithm> {
    (ctx.config.payload_signing_enabled == Some(false))
        .then(|| checksum_algorithm(ctx))
        .flatten()
}

/// Returns the checksum of the data in base64.
fn checksum(algorithm: &ChecksumAlgorithm, data: &[&[u8]]) -> Option<String> {
    let mut checksum = algorithm.as_str().parse::<Checksums>().ok()?.into_impl();
    data.iter().for_each(|data| checksum.update(data));
    Some(aws_smithy_types::base64::encode(checksum.finalize()))
}

/// Returns the body of the range of the file, which is read on sending
/// within the bandwidth.
async fn file_body(
    ctx: &Arc<Context>,
    id: TransferId,
    path: &str,
    offset: u64,
    length: u64,
) -> Result<UploadBody, String> {
    let checksum = match precomputes_checksum(ctx) {
        Some(algorithm) => {
            let data = read_range(path, offset, length).await?;
            checksum(&algorithm, &[&data])
        }
        None => None,
    };
    let stream = ByteStream::read_from()
        .buffer_size(READ_BUFFER_SIZE)
        .path(path)
        .offset(offset)
        .length(Length::Exact(length))
        .build()
        .await
        .map_err(|err| err.to_string())?;
    let stream = match &ctx.limiter {
        Some(limiter) => limiter.stream(stream),
        None => stream,
    };
    let stream = track_progress(ctx, id, stream);
    Ok(UploadBody { stream, checksum })
}

/// Reports the progress of the stream as it is sent, where the bytes sent
/// again on retries are reported only beyond the bytes already reported.
fn track_progress(ctx: &Arc<Context>, id: TransferId, stream: ByteStream) -> ByteStream {
    let ctx = ctx.clone();
    let reported = Arc::new(AtomicU64::new(0));
    stream.map(move |body| {
        SdkBody::from_body_1_x(ProgressBody {
            inner: body,
            ctx: ctx.clone(),
            id,
            sent: 0,
            reported: reported.clone(),
        })
    })
}

/// The body which reports the progress of the data of the inner body.
struct ProgressBody {
    inner: SdkBody,
    ctx: Arc<Context>,
    id: TransferId,
    /// The bytes sent in this attempt.
    sent: u64,
    /// The bytes reported in all the attempts.
    reported: Arc<AtomicU64>,
}

impl Body for ProgressBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = self.get_mut();
        let frame = ready!(Pin::new(&mut this.inner).poll_frame(cx));
        if let Some(Ok(frame)) = &frame {
            this.sent += frame.data_ref().map_or(0, |data| data.len() as u64);
            let reported = this.reported.fetch_max(this.sent, Ordering::Relaxed);
            if this.sent > reported {
                this.ctx.progress(this.id, this.sent - reported);
            }
        }
        Poll::Ready(frame)
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        Body::size_hint(&self.inner)
    }
}

/// Reads the range of the file, to compute the checksum before sending.
async fn read_range(path: &str, offset: u64, length: u64) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncSeekExt;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|err| err.to_string())?;
    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(|err| err.to_string())?;
    let mut data = Vec::new();
    file.take(length)
        .read_to_end(&mut data)
        .await
        .map_err(|err| err.to_string())?;
    Ok(data)
}

/// Sets the checksum value of the algorithm computed before sending.
macro_rules! set_checksum {
    ($builder:expr, $algorithm:expr, $checksum:expr) => {{
        let builder = $builder;
        let checksum: &Option<String> = $checksum;
        match ($algorithm, checksum.clone()) {
            (Some(ChecksumAlgorithm::Crc32), Some(value)) => builder.checksum_crc32(value),
            (Some(ChecksumAlgorithm::Crc32C), Some(value)) => builder.checksum_crc32_c(value),
            (Some(ChecksumAlgorithm::Crc64Nvme), Some(value)) => builder.checksum_crc64_nvme(value),
            (Some(ChecksumAlgorithm::Sha1), Some(value)) => builder.checksum_sha1(value),
            (Some(ChecksumAlgorithm::Sha256), Some(value)) => builder.checksum_sha256(value),
            _ => builder,
        }
    }};
}

/// Clones the body to send the request again on redirects, where the bodies
/// of the uploads are retryable.
fn clone_body(body: &SdkBody) -> ByteStream {
    ByteStream::new(body.try_clone().unwrap_or_else(SdkBody::taken))
}

/// Returns the body of the data in the memory, which limits the bandwidth.
fn memory_body(ctx: &Context, data: Vec<u8>) -> UploadBody {
    let checksum = precomputes_checksum(ctx).and_then(|algorithm| checksum(&algorithm, &[&data]));
    let stream = match &ctx.limiter {
        Some(limiter) => limiter.body(data.into()),
        None => ByteStream::from(data),
    };
    UploadBody { stream, checksum }
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

/// Returns the checksum algorithm of uploads, which defaults to CRC64NVME unless
/// `request_checksum_calculation` is `when_required`, like the AWS CLI.
fn checksum_algorithm(ctx: &Context) -> Option<ChecksumAlgorithm> {
    ctx.params.checksum_algorithm.clone().or_else(|| {
        let calculation = ctx.client.config().request_checksum_calculation();
        (calculation != Some(&RequestChecksumCalculation::WhenRequired))
            .then_some(ChecksumAlgorithm::Crc64Nvme)
    })
}

async fn put_object(
    ctx: &Context,
    bucket: &str,
    key: &str,
    body: UploadBody,
    size: u64,
    content_type: Option<String>,
) -> Result<Outcome, String> {
    let params = &ctx.params;
    let UploadBody { stream, checksum } = body;
    let body = stream.into_inner();
    let algorithm = checksum_algorithm(ctx);
    let has_content_type = content_type.is_some() || params.content_type.is_some();
    let content_type = content_type.or(params.content_type.clone());
    params.validate_ascii_metadata()?;
    let _permit = ctx.permit().await?;
    let result = ctx
        .redirector
        .send(&ctx.client, bucket, "PutObject", |client| {
            let request = client
                .put_object()
                .bucket(bucket)
                .key(key)
                .body(clone_body(&body))
                .content_length(size as i64);
            let request = set_object_params!(request, params)
                .set_checksum_algorithm(algorithm.clone())
                .set_tagging(params.tagging.clone())
                .set_if_none_match(params.no_overwrite.then(|| "*".to_owned()));
            let request = set_checksum!(request, &algorithm, &checksum);
            let request = set_sse_c!(request, &params.sse_c).set_content_type(content_type.clone());
            // The SDK sends the default content type of the body, whereas the
            // AWS CLI leaves it to S3, which defaults to binary/octet-stream.
            request
                .customize()
                .mutate_request(move |request| {
                    if !has_content_type {
                        request.headers_mut().remove("content-type");
                    }
                    merge_content_encodings(request.headers_mut());
                })
                .send()
        })
        .await;
    match result {
        Ok(_) => Ok(Outcome::Done),
        Err(err) if params.no_overwrite && has_error_code(&err, "PreconditionFailed") => {
            Ok(Outcome::Skipped)
        }
        Err(err) => Err(Error::sdk("PutObject", err).to_string()),
    }
}

/// Merges the content encodings into one header, where the SDK appends
/// aws-chunked as another header after the content encoding of the argument,
/// which S3-compatible storage may reject. The encodings are joined with commas
/// like the AWS CLI, e.g. `gzip,aws-chunked`.
fn merge_content_encodings(headers: &mut Headers) {
    let encodings: Vec<String> = headers
        .get_all("content-encoding")
        .map(str::to_owned)
        .collect();
    if encodings.len() > 1 {
        headers.insert("content-encoding", encodings.join(","));
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
    /// Whether the upload is completed or aborted.
    pub done: Arc<AtomicBool>,
}

/// Aborts the multipart upload when dropped before it is done, e.g. when the
/// transfer is cancelled by Ctrl-C.
pub struct AbortGuard {
    ctx: Arc<Context>,
    upload: MultipartUpload,
}

impl Drop for AbortGuard {
    fn drop(&mut self) {
        if !self.upload.done.load(Ordering::Relaxed) {
            let (ctx, upload) = (self.ctx.clone(), self.upload.clone());
            self.ctx
                .spawn_cleanup(async move { upload.abort(&ctx).await });
        }
    }
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
        params.validate_ascii_metadata()?;
        let _permit = ctx.permit().await?;
        let output = ctx
            .redirector
            .send(&ctx.client, bucket, "CreateMultipartUpload", |client| {
                let request = client.create_multipart_upload().bucket(bucket).key(key);
                let request = set_object_params!(request, params)
                    .set_checksum_algorithm(checksum_algorithm.clone())
                    .set_tagging(params.tagging.clone());
                let request = set_sse_c!(request, &params.sse_c);
                match &content_type {
                    Some(content_type) => request.content_type(content_type),
                    None => request,
                }
                .send()
            })
            .await
            .map_err(|err| Error::sdk("CreateMultipartUpload", err).to_string())?;
        Ok(Self {
            bucket: bucket.to_owned(),
            key: key.to_owned(),
            upload_id: output.upload_id().unwrap_or_default().to_owned(),
            checksum_algorithm,
            done: Arc::default(),
        })
    }

    async fn upload_part(
        &self,
        ctx: &Context,
        number: i32,
        body: UploadBody,
        length: u64,
    ) -> PartResult {
        let params = &ctx.params;
        let UploadBody { stream, checksum } = body;
        let body = stream.into_inner();
        let _permit = ctx.permit().await?;
        let output = ctx
            .redirector
            .send(&ctx.client, &self.bucket, "UploadPart", |client| {
                let request = client
                    .upload_part()
                    .bucket(&self.bucket)
                    .key(&self.key)
                    .upload_id(&self.upload_id)
                    .part_number(number)
                    .body(clone_body(&body))
                    .content_length(length as i64)
                    .set_checksum_algorithm(self.checksum_algorithm.clone())
                    .set_request_payer(params.request_payer.clone());
                let request = set_checksum!(request, &self.checksum_algorithm, &checksum);
                set_sse_c!(request, &params.sse_c).send()
            })
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

    /// Waits for the parts until the number of the parts in progress is less
    /// than the limit, to keep the number of the tasks and the buffered parts
    /// bounded. The upload is aborted if any of them failed.
    pub async fn wait_parts(
        &self,
        ctx: &Context,
        parts: &mut JoinSet<PartResult>,
        completed: &mut Vec<CompletedPart>,
        limit: usize,
    ) -> Result<(), String> {
        while parts.len() >= limit {
            match join_part(parts.join_next().await) {
                Some(Ok(part)) => completed.push(part),
                Some(Err(err)) => return Err(self.fail(ctx, parts, err).await),
                None => {}
            }
        }
        Ok(())
    }

    /// Waits for the parts and completes the upload, or aborts the upload if
    /// any of the parts failed.
    pub async fn finish(
        &self,
        ctx: &Context,
        completed: Vec<CompletedPart>,
        parts: JoinSet<PartResult>,
    ) -> Result<Outcome, String> {
        Ok(match self.complete(ctx, completed, parts).await? {
            Some(_) => Outcome::Done,
            None => Outcome::Skipped,
        })
    }

    /// Waits for the parts and completes the upload, and returns the output,
    /// or `None` if the object exists with the no-overwrite option.
    pub async fn complete(
        &self,
        ctx: &Context,
        mut completed: Vec<CompletedPart>,
        mut parts: JoinSet<PartResult>,
    ) -> Result<Option<CompleteMultipartUploadOutput>, String> {
        while let Some(result) = join_part(parts.join_next().await) {
            match result {
                Ok(part) => completed.push(part),
                Err(err) => return Err(self.fail(ctx, &mut parts, err).await),
            }
        }
        completed.sort_by_key(|part| part.part_number());
        let params = &ctx.params;
        let upload = CompletedMultipartUpload::builder()
            .set_parts(Some(completed))
            .build();
        let _permit = ctx.permit().await?;
        let result = ctx
            .redirector
            .send(
                &ctx.client,
                &self.bucket,
                "CompleteMultipartUpload",
                |client| {
                    let request = client
                        .complete_multipart_upload()
                        .bucket(&self.bucket)
                        .key(&self.key)
                        .upload_id(&self.upload_id)
                        .multipart_upload(upload.clone())
                        .set_request_payer(params.request_payer.clone())
                        .set_if_none_match(params.no_overwrite.then(|| "*".to_owned()));
                    set_sse_c!(request, &params.sse_c).send()
                },
            )
            .await;
        match result {
            Ok(output) => {
                self.done.store(true, Ordering::Relaxed);
                Ok(Some(output))
            }
            Err(err) => {
                drop(_permit);
                self.abort(ctx).await;
                if params.no_overwrite && has_error_code(&err, "PreconditionFailed") {
                    Ok(None)
                } else {
                    Err(Error::sdk("CompleteMultipartUpload", err).to_string())
                }
            }
        }
    }

    /// Returns the guard which aborts the upload when dropped before done.
    pub fn abort_guard(&self, ctx: &Arc<Context>) -> AbortGuard {
        AbortGuard {
            ctx: ctx.clone(),
            upload: self.clone(),
        }
    }

    async fn abort(&self, ctx: &Context) {
        if self.done.swap(true, Ordering::Relaxed) {
            return;
        }
        let _ = ctx
            .redirector
            .send(
                &ctx.client,
                &self.bucket,
                "AbortMultipartUpload",
                |client| {
                    client
                        .abort_multipart_upload()
                        .bucket(&self.bucket)
                        .key(&self.key)
                        .upload_id(&self.upload_id)
                        .set_request_payer(ctx.params.request_payer.clone())
                        .send()
                },
            )
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
