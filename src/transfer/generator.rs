//! The generators of the source files or objects, in the same way as
//! `FileGenerator` of the AWS CLI.

use std::fs;
use std::path::{MAIN_SEPARATOR as SEP, Path};
use std::time::SystemTime;

use aws_sdk_s3::Client;
use aws_sdk_s3::config::ResponseChecksumValidation;
use aws_sdk_s3::operation::head_object::HeadObjectOutput;
use aws_sdk_s3::operation::list_objects_v2::builders::ListObjectsV2FluentBuilder;
use aws_sdk_s3::types::{ChecksumMode, EncodingType, Object, RequestPayer};
use futures_util::future::try_join_all;
use tokio::sync::mpsc::Sender;

use super::params::{SseCustomerKey, set_sse_c};
use crate::error::{Error, has_status_code};
use crate::path::{is_s3express_bucket, split_bucket_key, url_decode, validate_bucket_name};
use crate::redirect::{Pages, Redirector};

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
    /// The maximum number of the ranges of the keys listed in parallel, where
    /// the objects are not listed in order unless it is 1.
    pub ranges: usize,
}

/// The maximum depth of the common prefixes to find the points to split the
/// listing at, where the prefixes of each level have only one prefix.
const MAX_SPLIT_DEPTH: usize = 4;

/// The listing of the objects under the prefix.
struct Listing<'a> {
    client: &'a Client,
    redirector: &'a Redirector,
    /// `bucket/key` of the object, or `bucket/prefix/` to list.
    path: &'a str,
    bucket: &'a str,
    prefix: &'a str,
    dir_op: bool,
    options: &'a ListOptions,
    tx: &'a Sender<Item>,
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
    let listing = Listing {
        client,
        redirector,
        path,
        bucket,
        prefix,
        dir_op,
        options,
        tx,
    };
    if options.ranges > 1 && !is_s3express_bucket(bucket) {
        listing.list_parallel().await
    } else {
        listing.list_range(None, None).await
    }
}

impl Listing<'_> {
    fn request(&self, client: &Client) -> ListObjectsV2FluentBuilder {
        client
            .list_objects_v2()
            .bucket(self.bucket)
            .prefix(self.prefix)
            .encoding_type(EncodingType::Url)
            .set_request_payer(self.options.request_payer.clone())
    }

    /// Lists the objects after `start` up to `end` inclusively, or all the
    /// objects without them.
    async fn list_range(&self, start: Option<&str>, end: Option<&str>) -> Result<(), String> {
        let mut pages = self
            .redirector
            .paginate(self.client, self.bucket, |client| {
                self.request(client)
                    .set_start_after(start.map(str::to_owned))
                    .set_max_keys(self.options.page_size)
            });
        self.send_pages(&mut pages, start, end).await
    }

    /// Lists the first page, and then the ranges of the rest in parallel,
    /// split at the common prefixes after the first page.
    async fn list_parallel(&self) -> Result<(), String> {
        let mut pages = self
            .redirector
            .paginate(self.client, self.bucket, |client| {
                self.request(client).set_max_keys(self.options.page_size)
            });
        let Some(result) = pages.next().await else {
            return Ok(());
        };
        let output = result.map_err(|err| Error::sdk("ListObjectsV2", err).to_string())?;
        if !self.send_objects(output.contents(), None, None).await
            || output.is_truncated() != Some(true)
        {
            return Ok(());
        }
        // The rest is listed with the continuation token without the key to
        // split after.
        let Some(last) = output.contents().last().and_then(Object::key) else {
            return self.send_pages(&mut pages, None, None).await;
        };
        // The ranges are between the adjacent bounds, where the last range
        // has no end.
        let mut bounds = vec![url_decode(last)];
        bounds.extend(self.split_points(&bounds[0]).await?);
        try_join_all(bounds.iter().enumerate().map(|(i, start)| {
            let end = bounds.get(i + 1).map(String::as_str);
            self.list_range(Some(start), end)
        }))
        .await?;
        Ok(())
    }

    /// Sends the objects of the pages after `start` up to `end` inclusively.
    async fn send_pages<F>(
        &self,
        pages: &mut Pages<'_, F>,
        start: Option<&str>,
        end: Option<&str>,
    ) -> Result<(), String>
    where
        F: Fn(&Client) -> ListObjectsV2FluentBuilder,
    {
        while let Some(result) = pages.next().await {
            let output = result.map_err(|err| Error::sdk("ListObjectsV2", err).to_string())?;
            if !self.send_objects(output.contents(), start, end).await {
                break;
            }
        }
        Ok(())
    }

    /// Returns the points to split the listing of the keys after the key,
    /// which are chosen evenly from the common prefixes after the key.
    async fn split_points(&self, after: &str) -> Result<Vec<String>, String> {
        let mut prefix = self.prefix.to_owned();
        for _ in 0..MAX_SPLIT_DEPTH {
            let output = self
                .redirector
                .send(self.client, self.bucket, "ListObjectsV2", |client| {
                    self.request(&client)
                        .prefix(&prefix)
                        .delimiter("/")
                        .start_after(after)
                        .send()
                })
                .await
                .map_err(|err| Error::sdk("ListObjectsV2", err).to_string())?;
            let mut prefixes: Vec<_> = output
                .common_prefixes()
                .iter()
                .filter_map(|prefix| prefix.prefix())
                .map(url_decode)
                .filter(|prefix| prefix.as_str() > after)
                .collect();
            if prefixes.len() >= 2 {
                let ranges = self.options.ranges.min(prefixes.len() + 1);
                return Ok((1..ranges)
                    .map(|i| prefixes[i * prefixes.len() / ranges].clone())
                    .collect());
            }
            // The rest of the keys under a prefix are split at the next level,
            // where the prefix is the only one after the key, or the one which
            // contains the key.
            if output.is_truncated() == Some(true) || !output.contents().is_empty() {
                return Ok(Vec::new());
            }
            let next = prefixes.pop().or_else(|| {
                let index = after.strip_prefix(prefix.as_str())?.find('/')?;
                Some(after[..=prefix.len() + index].to_owned())
            });
            let Some(next) = next else {
                return Ok(Vec::new());
            };
            prefix = next;
        }
        Ok(Vec::new())
    }

    /// Sends the objects after `start` up to `end` inclusively, and returns
    /// whether the listing continues. The objects up to `start` are skipped,
    /// in case the storage ignores the key to start after.
    async fn send_objects(
        &self,
        objects: &[Object],
        start: Option<&str>,
        end: Option<&str>,
    ) -> bool {
        for object in objects {
            let key = url_decode(object.key().unwrap_or_default());
            if start.is_some_and(|start| key.as_str() <= start) {
                continue;
            }
            if end.is_some_and(|end| key.as_str() > end) {
                return false;
            }
            let size = object.size().unwrap_or_default().max(0) as u64;
            let source_path = format!("{}/{key}", self.bucket);
            // Directory markers are only deleted, and a single object is
            // matched exactly when listing the whole bucket.
            if size == 0 && key.ends_with('/') {
                if !self.options.is_delete {
                    continue;
                }
            } else if !self.dir_op && source_path != self.path {
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
            if self.tx.send(Item::File(stat)).await.is_err() {
                return false;
            }
        }
        true
    }
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
    // The checksum mode is enabled for the full object checksum unless the
    // response checksums are validated only when required.
    let validation = client.config().response_checksum_validation();
    let checksum_mode = options.checksum_mode.clone().or_else(|| {
        (validation != Some(&ResponseChecksumValidation::WhenRequired))
            .then_some(ChecksumMode::Enabled)
    });
    let output = redirector
        .send(client, bucket, "HeadObject", |client| {
            let request = client
                .head_object()
                .bucket(bucket)
                .key(key)
                .set_request_payer(options.request_payer.clone())
                .set_checksum_mode(checksum_mode.clone());
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
