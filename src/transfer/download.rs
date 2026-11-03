use std::fs::{File, FileTimes};
use std::hash::{BuildHasher, RandomState};
use std::sync::Arc;
use std::time::SystemTime;

use aws_sdk_s3::operation::get_object::GetObjectOutput;
use aws_sdk_s3::primitives::ByteStream;
use bytes::Bytes;
use tokio::io::AsyncWriteExt;
use tokio::sync::SemaphorePermit;
use tokio::task::JoinSet;

use super::params::set_sse_c;
use super::{Context, Outcome, TransferId};
use crate::error::{Error, has_error_code};
use crate::path::validate_bucket_name;

/// Downloads the object to the local file, which is written to a temporary
/// file in the same directory and renamed on completion.
#[allow(clippy::too_many_arguments)]
pub async fn download_file(
    ctx: &Arc<Context>,
    id: TransferId,
    bucket: &str,
    key: &str,
    path: &str,
    size: u64,
    last_modified: Option<SystemTime>,
    etag: Option<&str>,
) -> Result<Outcome, String> {
    validate_bucket_name(bucket)?;
    // The size and the ETag are retrieved when the ETag is unavailable on
    // listing, like s3transfer, so that both are of the same object.
    let (size, etag) = match etag {
        Some(etag) => (size, Some(etag.to_owned())),
        None => {
            let head = head_object(ctx, bucket, key).await?;
            let size = head.content_length().unwrap_or_default().max(0) as u64;
            (size, head.e_tag().map(str::to_owned))
        }
    };
    if let Some(parent) = std::path::Path::new(path).parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|err| err.to_string())?;
    }
    let temp_path = format!("{path}.{}", random_extension());
    // The temporary file is removed on failures and cancellation.
    let temp_file = TempFile(Some(temp_path.clone()));
    if size < ctx.config.multipart_threshold {
        download_whole(ctx, id, bucket, key, &temp_path).await?;
    } else {
        download_ranges(ctx, id, bucket, key, &temp_path, size, etag).await?;
    }
    if let Some(time) = last_modified
        && let Err(err) = set_file_times(&temp_path, time)
    {
        ctx.warning(&format!(
            "warning: Successfully Downloaded {path} but was unable to update the last modified time. {err}"
        ));
    }
    tokio::fs::rename(&temp_path, path)
        .await
        .map_err(|err| err.to_string())?;
    temp_file.keep();
    Ok(Outcome::Done)
}

/// Removes the temporary file when dropped, unless it is kept.
struct TempFile(Option<String>);

impl TempFile {
    fn keep(mut self) {
        self.0 = None;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Downloads the object to the standard output.
pub async fn download_stdout(
    ctx: &Arc<Context>,
    id: TransferId,
    bucket: &str,
    key: &str,
) -> Result<Outcome, String> {
    validate_bucket_name(bucket)?;
    // The size of the object is unknown, which is retrieved first like the
    // AWS CLI, and the errors of missing objects are reported by HeadObject.
    head_object(ctx, bucket, key).await?;
    let (mut output, _permit) = get_object(ctx, bucket, key, None, None).await?;
    let mut stdout = tokio::io::stdout();
    while let Some(bytes) = output
        .body
        .try_next()
        .await
        .map_err(|err| err.to_string())?
    {
        ctx.limit(bytes.len()).await;
        stdout
            .write_all(&bytes)
            .await
            .map_err(|err| err.to_string())?;
        ctx.progress(id, bytes.len() as u64);
    }
    stdout.flush().await.map_err(|err| err.to_string())?;
    Ok(Outcome::Done)
}

async fn download_whole(
    ctx: &Arc<Context>,
    id: TransferId,
    bucket: &str,
    key: &str,
    path: &str,
) -> Result<(), String> {
    let (mut output, _permit) = get_object(ctx, bucket, key, None, None).await?;
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|err| err.to_string())?;
    while let Some(bytes) = output
        .body
        .try_next()
        .await
        .map_err(|err| err.to_string())?
    {
        ctx.limit(bytes.len()).await;
        file.write_all(&bytes)
            .await
            .map_err(|err| err.to_string())?;
        ctx.progress(id, bytes.len() as u64);
    }
    file.flush().await.map_err(|err| err.to_string())
}

/// Downloads the ranges of the object concurrently, and writes them at the
/// offsets of the file. The ranges are requested with the ETag, so that the
/// object modified during the download fails instead of mixing the contents.
async fn download_ranges(
    ctx: &Arc<Context>,
    id: TransferId,
    bucket: &str,
    key: &str,
    path: &str,
    size: u64,
    etag: Option<String>,
) -> Result<(), String> {
    let file = File::create(path).map_err(|err| err.to_string())?;
    file.set_len(size).map_err(|err| err.to_string())?;
    let file = Arc::new(file);
    let chunk_size = ctx.config.multipart_chunksize;
    let mut ranges = JoinSet::new();
    let mut offset = 0;
    while offset < size {
        let end = (offset + chunk_size).min(size) - 1;
        let (ctx, file) = (ctx.clone(), file.clone());
        let (bucket, key) = (bucket.to_owned(), key.to_owned());
        let etag = etag.clone();
        ranges.spawn(async move {
            let range = format!("bytes={offset}-{end}");
            let (output, permit) = get_object(&ctx, &bucket, &key, Some(range), etag).await?;
            let bytes = read_body(&ctx, output.body).await?;
            drop(permit);
            let length = bytes.len() as u64;
            tokio::task::spawn_blocking(move || write_at(&file, &bytes, offset))
                .await
                .map_err(|err| err.to_string())?
                .map_err(|err| err.to_string())?;
            ctx.progress(id, length);
            Ok::<_, String>(())
        });
        offset = end + 1;
    }
    while let Some(result) = ranges.join_next().await {
        if let Err(err) = result
            .map_err(|err| err.to_string())
            .and_then(|result| result)
        {
            ranges.abort_all();
            return Err(err);
        }
    }
    file.sync_all().map_err(|err| err.to_string())
}

/// Reads the body into the memory, within the bandwidth.
async fn read_body(ctx: &Context, mut body: ByteStream) -> Result<Bytes, String> {
    if ctx.limiter.is_none() {
        return Ok(body
            .collect()
            .await
            .map_err(|err| err.to_string())?
            .into_bytes());
    }
    let mut data = Vec::new();
    while let Some(bytes) = body.try_next().await.map_err(|err| err.to_string())? {
        ctx.limit(bytes.len()).await;
        data.extend_from_slice(&bytes);
    }
    Ok(data.into())
}

/// Sends the GetObject request, and returns the response with the permit of
/// the request, which should be held until the body is read so that the
/// concurrent reads are limited by `max_concurrent_requests`.
async fn get_object<'a>(
    ctx: &'a Context,
    bucket: &str,
    key: &str,
    range: Option<String>,
    etag: Option<String>,
) -> Result<(GetObjectOutput, SemaphorePermit<'a>), String> {
    let params = &ctx.params;
    let pinned = etag.is_some();
    let request = ctx
        .source_client
        .get_object()
        .bucket(bucket)
        .key(key)
        .set_range(range)
        .set_if_match(etag)
        .set_request_payer(params.request_payer.clone())
        .set_checksum_mode(params.checksum_mode.clone());
    let request = set_sse_c!(request, &params.sse_c);
    let permit = ctx.permit().await?;
    let output = request.send().await.map_err(|err| {
        if pinned && has_error_code(&err, "PreconditionFailed") {
            etag_mismatch(bucket, key)
        } else {
            Error::sdk("GetObject", err).to_string()
        }
    })?;
    Ok((output, permit))
}

/// Returns the error message of the object modified during the multipart
/// download or copy, in the same way as s3transfer.
pub fn etag_mismatch(bucket: &str, key: &str) -> String {
    format!(
        "Contents of stored object \"{key}\" in bucket \"{bucket}\" did not match expected ETag."
    )
}

async fn head_object(
    ctx: &Context,
    bucket: &str,
    key: &str,
) -> Result<aws_sdk_s3::operation::head_object::HeadObjectOutput, String> {
    let params = &ctx.params;
    let request = ctx
        .source_client
        .head_object()
        .bucket(bucket)
        .key(key)
        .set_request_payer(params.request_payer.clone());
    let request = set_sse_c!(request, &params.sse_c);
    let _permit = ctx.permit().await?;
    request
        .send()
        .await
        .map_err(|err| Error::sdk("HeadObject", err).to_string())
}

#[cfg(unix)]
fn write_at(file: &File, bytes: &[u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, bytes, offset)
}

#[cfg(windows)]
fn write_at(file: &File, mut bytes: &[u8], mut offset: u64) -> std::io::Result<()> {
    while !bytes.is_empty() {
        let written = std::os::windows::fs::FileExt::seek_write(file, bytes, offset)?;
        bytes = &bytes[written..];
        offset += written as u64;
    }
    Ok(())
}

fn set_file_times(path: &str, time: SystemTime) -> std::io::Result<()> {
    let file = File::options().write(true).open(path)?;
    file.set_times(FileTimes::new().set_accessed(time).set_modified(time))
}

/// Returns a random extension of hex digits for temporary files, like
/// `random_file_extension` of s3transfer.
fn random_extension() -> String {
    let random = RandomState::new().hash_one(SystemTime::now());
    format!("{:08x}", random as u32)
}
