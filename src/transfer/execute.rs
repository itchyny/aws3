//! The submission and the execution of the transfers.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use aws_sdk_s3::Client;
use tokio::task::JoinSet;

use super::context::Context;
use super::generator::{ObjectProps, skip_warning};
use super::options::CommandName;
use super::paths::{self, relative_path};
use super::printer::TransferId;
use super::{copy, download, sync, upload};
use crate::error::Error;
use crate::path::{split_bucket_key, validate_bucket_name};

/// The maximum size of an object of S3.
const MAX_UPLOAD_SIZE: u64 = 5 * 1024 * 1024 * 1024 * 1024;

/// The kind of a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Operation {
    Upload,
    Download,
    Copy,
    Delete,
    LocalDelete,
}

impl Operation {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
            Self::Copy => "copy",
            Self::Delete | Self::LocalDelete => "delete",
        }
    }
}

/// A transfer of a file or an object.
#[derive(Debug, Clone)]
pub(super) struct Task {
    pub(super) operation: Operation,
    /// The absolute local path, or `bucket/key` of S3.
    pub(super) src: String,
    pub(super) dest: String,
    pub(super) compare_key: String,
    pub(super) size: Option<u64>,
    pub(super) last_modified: Option<SystemTime>,
    pub(super) object: Option<ObjectProps>,
    /// The keys of the downloads to remove this key from on completion.
    pub(super) case_conflict: Option<(sync::CaseConflictKeys, String)>,
}

/// The result of a transfer which did not fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Done,
    /// The transfer is skipped because the destination already exists.
    Skipped,
}

/// Submits the transfers, limiting the number of queued transfers.
pub(super) struct Submitter {
    ctx: Arc<Context>,
    tasks: JoinSet<()>,
    submitted: u64,
    /// Whether a fatal error occurred, which aborts the command.
    fatal: bool,
}

impl Submitter {
    pub(super) fn new(ctx: Arc<Context>) -> Self {
        Self {
            ctx,
            tasks: JoinSet::new(),
            submitted: 0,
            fatal: false,
        }
    }

    pub(super) async fn submit(&mut self, task: Task) {
        let ctx = &self.ctx;
        if ctx.is_cancelled() || self.warn_and_skip(&task) {
            return;
        }
        let transfer_type = if ctx.is_move() {
            "move"
        } else {
            task.operation.name()
        };
        let (src, dest) = format_src_dest(ctx, &task);
        if ctx.options.dryrun {
            ctx.printer().dry_run(transfer_type, &src, dest.as_deref());
            return;
        }
        while self.tasks.len() >= ctx.config.max_queue_size {
            tokio::select! {
                _ = self.tasks.join_next() => {}
                () = ctx.cancelled() => return,
            }
        }
        let id = ctx.next_id.fetch_add(1, Ordering::Relaxed);
        // Deletions transfer no bytes.
        let size = match task.operation {
            Operation::Delete | Operation::LocalDelete => 0,
            _ => task.size.unwrap_or(0),
        };
        ctx.printer().queued(id, size);
        self.submitted += 1;
        let ctx = ctx.clone();
        self.tasks.spawn(async move {
            let result = execute(&ctx, id, &task).await;
            if let Some((keys, key)) = &task.case_conflict {
                keys.lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .remove(key);
            }
            let mut printer = ctx.printer();
            match result {
                Ok(Outcome::Done) => printer.success(id, transfer_type, &src, dest.as_deref()),
                Ok(Outcome::Skipped) => printer.skipped(id),
                Err(message) => printer.failure(id, transfer_type, &src, dest.as_deref(), &message),
            }
        });
    }

    /// Prints the warnings of the task, and returns whether it is skipped.
    fn warn_and_skip(&self, task: &Task) -> bool {
        let ctx = &self.ctx;
        let options = &ctx.options;
        match task.operation {
            Operation::Upload => {
                if task.size.is_some_and(|size| size > MAX_UPLOAD_SIZE) {
                    let path = relative_path(&task.src);
                    ctx.warning(&format!(
                        "warning: File {path} exceeds s3 upload limit of 5.0 TiB."
                    ));
                }
                false
            }
            Operation::Download => {
                if self.warn_glacier(task) {
                    return true;
                }
                if escapes_parent(&task.compare_key) {
                    ctx.warning(&skip_warning(
                        &task.compare_key,
                        "File references a parent directory.",
                    ));
                    return true;
                }
                if ctx.plan.is_stream {
                    return false;
                }
                if !options.follow_symlinks && has_link_in_dest_path(&ctx.plan.dest, &task.dest) {
                    return true;
                }
                options.no_overwrite
                    && options.command != CommandName::Sync
                    && Path::new(&task.dest).exists()
            }
            Operation::Copy => self.warn_glacier(task),
            Operation::Delete | Operation::LocalDelete => false,
        }
    }

    fn warn_glacier(&self, task: &Task) -> bool {
        let options = &self.ctx.options;
        if options.force_glacier_transfer
            || !task.object.as_ref().is_some_and(ObjectProps::is_glacier)
        {
            return false;
        }
        if !options.ignore_glacier_warnings {
            let operation = task.operation.name();
            self.ctx.warning(&skip_warning(
                &format!("s3://{}", task.src),
                &format!(
                    "Object is of storage class GLACIER. Unable to perform {operation} operations on GLACIER objects. \
                     You must restore the object to be able to perform the operation. \
                     See aws s3 {operation} help for additional parameter options to ignore or force these transfers."
                ),
            ));
        }
        true
    }

    /// Stops submitting transfers on a fatal error.
    pub(super) fn abort(&mut self) {
        self.fatal = true;
    }

    /// Waits for the submitted transfers.
    pub(super) async fn finish(mut self) {
        if !self.fatal && !self.ctx.is_cancelled() {
            self.ctx.printer().final_total(self.submitted);
        }
        loop {
            tokio::select! {
                result = self.tasks.join_next() => {
                    if result.is_none() {
                        return;
                    }
                }
                () = self.ctx.cancelled() => break,
            }
        }
        // The cancelled transfers are dropped, which spawns the cleanups.
        self.tasks.abort_all();
        while self.tasks.join_next().await.is_some() {}
    }
}

/// Returns the source and the destination to display.
fn format_src_dest(ctx: &Context, task: &Task) -> (String, Option<String>) {
    let local = |path: &str| {
        if ctx.plan.is_stream {
            "-".to_owned()
        } else {
            relative_path(path)
        }
    };
    let s3 = |path: &str| format!("s3://{path}");
    match task.operation {
        Operation::Upload => (local(&task.src), Some(s3(&task.dest))),
        Operation::Download => (s3(&task.src), Some(local(&task.dest))),
        Operation::Copy => (s3(&task.src), Some(s3(&task.dest))),
        Operation::Delete => (s3(&task.src), None),
        Operation::LocalDelete => (relative_path(&task.src), None),
    }
}

/// Returns whether the key escapes the destination directory.
fn escapes_parent(compare_key: &str) -> bool {
    let mut depth = 0i64;
    for component in compare_key.split(['/', std::path::MAIN_SEPARATOR]) {
        match component {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => depth += 1,
        }
    }
    false
}

/// Returns whether the destination or its parents under the destination root
/// are symbolic links, which are not followed with `--no-follow-symlinks`.
fn has_link_in_dest_path(root: &str, dest: &str) -> bool {
    let is_link =
        |path: &Path| std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_symlink());
    let root = paths::abspath(root);
    if is_link(Path::new(&root)) {
        return true;
    }
    let Some(relative) = Path::new(dest).strip_prefix(&root).ok() else {
        return false;
    };
    let mut path = Path::new(&root).to_path_buf();
    for component in relative.components() {
        path.push(component);
        if component.as_os_str() == ".." || is_link(&path) {
            return true;
        }
    }
    false
}

async fn execute(ctx: &Arc<Context>, id: TransferId, task: &Task) -> Result<Outcome, String> {
    let is_stream = ctx.plan.is_stream;
    let outcome = match task.operation {
        Operation::Upload => {
            let (bucket, key) = split_bucket_key(&task.dest);
            if is_stream {
                upload::upload_stdin(ctx, id, bucket, key).await?
            } else {
                upload::upload_file(ctx, id, &task.src, bucket, key).await?
            }
        }
        Operation::Download => {
            let (bucket, key) = split_bucket_key(&task.src);
            if is_stream {
                download::download_stdout(ctx, id, bucket, key).await?
            } else {
                download::download_file(
                    ctx,
                    id,
                    bucket,
                    key,
                    &task.dest,
                    task.size.unwrap_or(0),
                    task.last_modified,
                    task.object.as_ref(),
                )
                .await?
            }
        }
        Operation::Copy => {
            let (src_bucket, src_key) = split_bucket_key(&task.src);
            let (bucket, key) = split_bucket_key(&task.dest);
            copy::copy_object(
                ctx,
                id,
                src_bucket,
                src_key,
                bucket,
                key,
                task.size.unwrap_or(0),
                task.object.as_ref(),
            )
            .await?
        }
        Operation::Delete => {
            let (bucket, key) = split_bucket_key(&task.src);
            delete_object(ctx, &ctx.client, bucket, key).await?
        }
        Operation::LocalDelete => {
            tokio::fs::remove_file(&task.src)
                .await
                .map_err(|err| err.to_string())?;
            Outcome::Done
        }
    };
    if ctx.is_move() && outcome == Outcome::Done {
        match task.operation {
            Operation::Upload => {
                tokio::fs::remove_file(&task.src)
                    .await
                    .map_err(|err| err.to_string())?;
            }
            Operation::Download | Operation::Copy => {
                let (bucket, key) = split_bucket_key(&task.src);
                delete_object(ctx, &ctx.source_client, bucket, key).await?;
            }
            _ => {}
        }
    }
    Ok(outcome)
}

async fn delete_object(
    ctx: &Context,
    client: &Client,
    bucket: &str,
    key: &str,
) -> Result<Outcome, String> {
    validate_bucket_name(bucket)?;
    let _permit = ctx.permit().await?;
    ctx.redirector
        .send(client, bucket, "DeleteObject", |client| async move {
            client
                .delete_object()
                .bucket(bucket)
                .key(key)
                .set_request_payer(ctx.params.request_payer.clone())
                .send()
                .await
        })
        .await
        .map_err(|err| Error::sdk("DeleteObject", err).to_string())?;
    Ok(Outcome::Done)
}
