//! The submission and the execution of the transfers.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use tokio::task::JoinSet;

use super::context::Context;
use super::delete::{self, Deletion, delete_object};
use super::generator::{ObjectProps, skip_warning};
use super::options::CommandName;
use super::paths::{self, relative_path};
use super::printer::TransferId;
use super::task::{Operation, Outcome, Task};
use super::{copy, download, upload};
use crate::path::split_bucket_key;

/// The maximum size of an object of S3.
const MAX_UPLOAD_SIZE: u64 = 5 * 1024 * 1024 * 1024 * 1024;

/// Submits the transfers, limiting the number of queued transfers.
pub(super) struct Submitter {
    ctx: Arc<Context>,
    tasks: JoinSet<()>,
    submitted: u64,
    /// The deletions of the objects to send in a batch of `DeleteObjects`.
    deletions: Vec<Deletion>,
    /// The bucket of the objects of the deletions.
    deletion_bucket: String,
    /// Whether a fatal error occurred, which aborts the command.
    fatal: bool,
}

impl Submitter {
    pub(super) fn new(ctx: Arc<Context>) -> Self {
        Self {
            ctx,
            tasks: JoinSet::new(),
            submitted: 0,
            deletions: Vec::new(),
            deletion_bucket: String::new(),
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
        // The recursive deletions are sent in batches of a bucket, while the
        // deletion of an object is sent alone like the AWS CLI. The keys are
        // sent in XML 1.0, where the control characters are not allowed even
        // as character references, so they are also deleted alone.
        if task.operation == Operation::Delete
            && ctx.options.dir_op
            && ctx.batch_delete.load(Ordering::Relaxed)
            && task.src.chars().all(|c| {
                matches!(c, '\t' | '\n' | '\r')
                    || (c >= ' ' && !matches!(c, '\u{FFFE}' | '\u{FFFF}'))
            })
        {
            let (bucket, _) = split_bucket_key(&task.src);
            if bucket != self.deletion_bucket {
                self.flush_deletions();
                self.deletion_bucket = bucket.to_owned();
            }
            self.deletions.push(Deletion { id, path: task.src });
            if self.deletions.len() >= delete::MAX_BATCH_SIZE {
                self.flush_deletions();
            }
            return;
        }
        let is_local_delete = task.operation == Operation::LocalDelete;
        let ctx = ctx.clone();
        let transfer = async move {
            let result = execute(&ctx, id, &task).await;
            if let Some((keys, key)) = &task.case_conflict {
                keys.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(key);
            }
            let mut printer = ctx.printer();
            match result {
                Ok(Outcome::Done) => printer.success(id, transfer_type, &src, dest.as_deref()),
                Ok(Outcome::Skipped) => printer.skipped(id),
                Err(message) => printer.failure(id, transfer_type, &src, dest.as_deref(), &message),
            }
        };
        // The local deletions of sync are done before the next files are
        // compared like the AWS CLI, so that a file is deleted before the
        // download of the key which differs only by case, which is the same
        // file on case-insensitive file systems.
        if is_local_delete {
            transfer.await;
        } else {
            self.tasks.spawn(transfer);
        }
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

    fn flush_deletions(&mut self) {
        if !self.deletions.is_empty() {
            let deletions = std::mem::take(&mut self.deletions);
            let bucket = self.deletion_bucket.clone();
            self.tasks
                .spawn(delete::delete_batch(self.ctx.clone(), bucket, deletions));
        }
    }

    /// Stops submitting transfers on a fatal error.
    pub(super) fn abort(&mut self) {
        self.fatal = true;
    }

    /// Waits for the submitted transfers.
    pub(super) async fn finish(mut self) {
        if !self.ctx.is_cancelled() {
            self.flush_deletions();
        }
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
    // The size and the modification time of the local file of a move, which
    // is removed only if they are unchanged by the upload.
    let stamp = |path: &str| {
        std::fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.len(), metadata.modified().ok()))
    };
    let uploaded = (ctx.is_move() && task.operation == Operation::Upload).then(|| stamp(&task.src));
    // The ETag of the object transferred, which pins the deletion of the
    // source of a move.
    let (outcome, etag) = match task.operation {
        Operation::Upload => {
            let (bucket, key) = split_bucket_key(&task.dest);
            if is_stream {
                (upload::upload_stdin(ctx, id, bucket, key).await?, None)
            } else {
                (
                    upload::upload_file(ctx, id, &task.src, bucket, key).await?,
                    None,
                )
            }
        }
        Operation::Download => {
            let (bucket, key) = split_bucket_key(&task.src);
            if is_stream {
                (download::download_stdout(ctx, id, bucket, key).await?, None)
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
            // The move of an object onto itself deletes it after the copy,
            // which succeeds when the object is changed, e.g. by the storage
            // class. This guards the objects which the validation of the
            // paths fails to detect.
            if ctx.is_move() && task.src == task.dest {
                return Err("Cannot mv a file onto itself".to_owned());
            }
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
            (
                delete_object(ctx, &ctx.client, bucket, key, None).await?,
                None,
            )
        }
        Operation::LocalDelete => {
            tokio::fs::remove_file(&task.src)
                .await
                .map_err(|err| err.to_string())?;
            (Outcome::Done, None)
        }
    };
    if ctx.is_move() && outcome == Outcome::Done {
        match task.operation {
            Operation::Upload => {
                // The file modified during the upload is kept, although it
                // may still be modified between the check and the removal.
                if uploaded.is_some_and(|uploaded| uploaded != stamp(&task.src)) {
                    return Err("The file was modified during the upload".to_owned());
                }
                tokio::fs::remove_file(&task.src)
                    .await
                    .map_err(|err| err.to_string())?;
            }
            Operation::Download | Operation::Copy => {
                // The source is deleted only if it is the object transferred,
                // so that a new object written to the source during the move
                // is kept.
                let (bucket, key) = split_bucket_key(&task.src);
                delete_object(ctx, &ctx.source_client, bucket, key, etag.as_deref()).await?;
            }
            _ => {}
        }
    }
    Ok(outcome)
}
