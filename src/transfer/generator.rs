//! Generators of the source files or objects, in the same way as
//! `FileGenerator` of the AWS CLI.

use std::fs;
use std::path::{MAIN_SEPARATOR as SEP, Path};
use std::time::SystemTime;

use aws_sdk_s3::Client;
use aws_sdk_s3::operation::head_object::HeadObjectOutput;
use aws_sdk_s3::types::{EncodingType, RequestPayer};
use tokio::sync::mpsc::Sender;

use super::params::{SseCustomerKey, set_sse_c};
use super::paths::split_bucket_key;
use crate::error::{Error, has_status_code};
use crate::path::{url_decode, validate_bucket_name};
use crate::redirect::Redirector;

/// A file or an object listed by the generator.
#[derive(Debug, Clone)]
pub struct FileStat {
    /// The absolute local path, or `bucket/key` of S3.
    pub path: String,
    pub size: Option<u64>,
    pub last_modified: Option<SystemTime>,
    /// The properties of the object, only for S3.
    pub object: Option<ObjectProps>,
}

/// The properties of an object relevant to transfers.
#[derive(Debug, Clone, Default)]
pub struct ObjectProps {
    pub storage_class: Option<String>,
    /// The ETag to ensure that the object is not modified during multipart
    /// downloads and copies.
    pub etag: Option<String>,
    /// The `Restore` header of HeadObject, which is unavailable on listing.
    pub restore: Option<String>,
    /// The response of HeadObject for a single object.
    pub head: Option<Box<HeadObjectOutput>>,
}

impl ObjectProps {
    /// Returns whether the object is in Glacier and not restored, which
    /// cannot be downloaded or copied.
    pub fn is_glacier(&self) -> bool {
        matches!(
            self.storage_class.as_deref(),
            Some("GLACIER" | "DEEP_ARCHIVE")
        ) && !self
            .restore
            .as_deref()
            .is_some_and(|restore| restore.contains("ongoing-request=\"false\""))
    }
}

/// An item yielded by the generator.
pub enum Item {
    File(FileStat),
    Warning(String),
    /// A fatal error, which aborts the command.
    Error(String),
}

/// Creates the warning of a skipped file in the same format as the AWS CLI.
pub fn skip_warning(path: &str, message: &str) -> String {
    format!("warning: Skipping file {path}. {message}")
}

/// Lists the local file, or the files under the directory recursively in
/// the order of the keys of S3, which blocks the current thread.
pub fn list_local(path: &str, dir_op: bool, follow_symlinks: bool, tx: &Sender<Item>) {
    if should_ignore(path, follow_symlinks, tx) {
        return;
    }
    if !dir_op {
        send_file_stat(path, tx);
        return;
    }
    let dir = path.trim_end_matches(SEP);
    let Ok(entries) = fs::read_dir(if dir.is_empty() { path } else { dir }) else {
        return;
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            let message = format!(
                "There was an error trying to decode the the file {name:?} in directory \"{dir}\". \n\
                 Please check your locale settings.  The filename was decoded as: utf-8\n\
                 On posix platforms, check the LC_CTYPE environment variable."
            );
            let _ = tx.blocking_send(Item::Warning(skip_warning(&format!("{name:?}"), &message)));
            continue;
        };
        let file_path = format!("{dir}{SEP}{name}");
        if should_ignore(&file_path, follow_symlinks, tx) {
            continue;
        }
        let is_dir = Path::new(&file_path).is_dir();
        names.push((format!("{name}{}", if is_dir { "/" } else { "" }), is_dir));
    }
    // Directories are sorted with the trailing separators like S3 keys.
    names.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (name, is_dir) in names {
        let file_path = format!("{dir}{SEP}{}", name.trim_end_matches('/'));
        if is_dir {
            list_local(&format!("{file_path}{SEP}"), true, follow_symlinks, tx);
        } else {
            send_file_stat(&file_path, tx);
        }
    }
}

fn send_file_stat(path: &str, tx: &Sender<Item>) {
    match fs::metadata(path) {
        Ok(metadata) => {
            let stat = FileStat {
                path: path.to_owned(),
                size: Some(metadata.len()),
                last_modified: Some(metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH)),
                object: None,
            };
            let _ = tx.blocking_send(Item::File(stat));
        }
        Err(_) => {
            let _ = tx.blocking_send(Item::Warning(skip_warning(path, "File does not exist.")));
        }
    }
}

/// Returns whether the file is ignored, i.e. a symbolic link not to be
/// followed, or a file which triggers a warning.
fn should_ignore(path: &str, follow_symlinks: bool, tx: &Sender<Item>) -> bool {
    let trimmed = path
        .strip_suffix(SEP)
        .filter(|path| !path.is_empty())
        .unwrap_or(path);
    if !follow_symlinks && fs::symlink_metadata(trimmed).is_ok_and(|metadata| metadata.is_symlink())
    {
        return true;
    }
    let warning = match fs::metadata(path) {
        Err(_) => Some("File does not exist."),
        Ok(metadata) if is_special_file(&metadata) => {
            Some("File is character special device, block special device, FIFO, or socket.")
        }
        Ok(metadata) if !is_readable(path, metadata.is_dir()) => {
            Some("File/Directory is not readable.")
        }
        Ok(_) => None,
    };
    if let Some(message) = warning {
        let _ = tx.blocking_send(Item::Warning(skip_warning(path, message)));
        return true;
    }
    false
}

#[cfg(unix)]
fn is_special_file(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    let file_type = metadata.file_type();
    file_type.is_char_device()
        || file_type.is_block_device()
        || file_type.is_fifo()
        || file_type.is_socket()
}

#[cfg(not(unix))]
fn is_special_file(_: &fs::Metadata) -> bool {
    false
}

fn is_readable(path: &str, is_dir: bool) -> bool {
    if is_dir {
        fs::read_dir(path).is_ok()
    } else {
        fs::File::open(path).is_ok()
    }
}

/// The options of listing objects.
pub struct ListOptions {
    pub page_size: Option<i32>,
    pub request_payer: Option<RequestPayer>,
    /// The customer-provided key to call HeadObject with.
    pub sse_c: Option<SseCustomerKey>,
    pub checksum_mode: Option<aws_sdk_s3::types::ChecksumMode>,
    /// Whether the objects are deleted, which includes directory markers and
    /// does not call HeadObject for a single object.
    pub is_delete: bool,
}

/// Lists the object, or the objects under the prefix, where the path is
/// `bucket/key` or `bucket/prefix/`.
pub async fn list_s3(
    client: &Client,
    redirector: &Redirector,
    path: &str,
    dir_op: bool,
    options: &ListOptions,
    tx: &Sender<Item>,
) {
    if let Err(message) = try_list_s3(client, redirector, path, dir_op, options, tx).await {
        let _ = tx.send(Item::Error(message)).await;
    }
}

async fn try_list_s3(
    client: &Client,
    redirector: &Redirector,
    path: &str,
    dir_op: bool,
    options: &ListOptions,
    tx: &Sender<Item>,
) -> Result<(), String> {
    let (bucket, prefix) = split_bucket_key(path);
    // A single object to delete is not requested on listing.
    if !(options.is_delete && !dir_op && !prefix.is_empty()) {
        validate_bucket_name(bucket)?;
    }
    if !dir_op && !prefix.is_empty() {
        let stat = head_single_object(client, redirector, bucket, prefix, options).await?;
        let _ = tx.send(Item::File(stat)).await;
        return Ok(());
    }
    let send = |client: &Client| {
        let mut paginator = client
            .list_objects_v2()
            .bucket(bucket)
            .prefix(prefix)
            .encoding_type(EncodingType::Url)
            .set_request_payer(options.request_payer.clone())
            .into_paginator();
        if let Some(page_size) = options.page_size {
            paginator = paginator.page_size(page_size);
        }
        paginator.send()
    };
    let mut pages = send(&redirector.client(client, bucket));
    let mut page = pages.next().await;
    // The listing is restarted in the region of the bucket on redirects.
    if let Some(Err(err)) = &page
        && let Some(client) = redirector
            .redirect(client, bucket, "ListObjectsV2", err)
            .await
    {
        pages = send(&client);
        page = pages.next().await;
    }
    while let Some(result) = page {
        let output = result.map_err(|err| Error::sdk("ListObjectsV2", err).to_string())?;
        for object in output.contents() {
            let key = url_decode(object.key().unwrap_or_default());
            let size = object.size().unwrap_or_default().max(0) as u64;
            let source_path = format!("{bucket}/{key}");
            // Directory markers are only deleted, and a single object is
            // matched exactly when listing the whole bucket.
            if size == 0 && key.ends_with('/') {
                if !options.is_delete {
                    continue;
                }
            } else if !dir_op && source_path != path {
                continue;
            }
            let stat = FileStat {
                path: source_path,
                size: Some(size),
                last_modified: object
                    .last_modified()
                    .and_then(|time| SystemTime::try_from(*time).ok()),
                object: Some(ObjectProps {
                    storage_class: object
                        .storage_class()
                        .map(|class| class.as_str().to_owned()),
                    etag: object.e_tag().map(str::to_owned),
                    ..ObjectProps::default()
                }),
            };
            if tx.send(Item::File(stat)).await.is_err() {
                return Ok(());
            }
        }
        page = pages.next().await;
    }
    Ok(())
}

async fn head_single_object(
    client: &Client,
    redirector: &Redirector,
    bucket: &str,
    key: &str,
    options: &ListOptions,
) -> Result<FileStat, String> {
    let path = format!("{bucket}/{key}");
    if options.is_delete {
        return Ok(FileStat {
            path,
            size: None,
            last_modified: None,
            object: None,
        });
    }
    let output = redirector
        .send(client, bucket, "HeadObject", |client| {
            let request = client
                .head_object()
                .bucket(bucket)
                .key(key)
                .set_request_payer(options.request_payer.clone())
                .set_checksum_mode(options.checksum_mode.clone());
            set_sse_c!(request, &options.sse_c).send()
        })
        .await
        .map_err(|err| {
            if has_status_code(&err, 404) {
                format!(
                    "An error occurred (404) when calling the HeadObject operation: Key \"{key}\" does not exist"
                )
            } else {
                Error::sdk("HeadObject", err).to_string()
            }
        })?;
    Ok(FileStat {
        path,
        size: Some(output.content_length().unwrap_or_default().max(0) as u64),
        last_modified: output
            .last_modified()
            .and_then(|time| SystemTime::try_from(*time).ok()),
        object: Some(ObjectProps {
            storage_class: output
                .storage_class()
                .map(|class| class.as_str().to_owned()),
            etag: output.e_tag().map(str::to_owned),
            restore: output.restore().map(str::to_owned),
            head: Some(Box::new(output)),
        }),
    })
}
