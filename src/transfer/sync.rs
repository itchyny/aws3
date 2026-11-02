//! Comparison of the source and the destination, in the same way as the
//! `Comparator` and the sync strategies of the AWS CLI.

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use tokio::sync::mpsc;

use super::generator::{FileStat, Item};
use super::paths::{self, Endpoint};
use super::{
    CommandName, Context, Operation, PathsType, Submitter, Task, create_filters, spawn_generator,
};
use crate::cli::CaseConflict;
use crate::filter::Filters;

/// How a file at both the source and the destination is synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtBoth {
    SizeAndLastModified,
    SizeOnly,
    ExactTimestamps,
    Never,
    Always,
}

/// The keys of the downloads, in lowercase, to detect case conflicts.
pub type CaseConflictKeys = Arc<Mutex<HashSet<String>>>;

/// Returns whether the recursive downloads of cp and mv are compared with
/// the local files to handle case conflicts.
pub fn should_handle_case_conflicts(ctx: &Context) -> bool {
    matches!(
        ctx.options.command,
        CommandName::Cp | CommandName::Mv | CommandName::Sync
    ) && ctx.plan.paths_type == PathsType::S3Local
        && ctx.options.case_conflict != CaseConflict::Ignore
        && ctx.options.dir_op
}

/// Compares the source and the destination, and submits the transfers of
/// the files to be synced, and the deletions with `--delete` of sync.
pub async fn run(ctx: &Arc<Context>, submitter: &mut Submitter) {
    let options = &ctx.options;
    let plan = &ctx.plan;
    let is_sync = options.command == CommandName::Sync;
    let at_both = if !is_sync {
        AtBoth::Always
    } else if options.no_overwrite {
        AtBoth::Never
    } else if options.exact_timestamps {
        AtBoth::ExactTimestamps
    } else if options.size_only {
        AtBoth::SizeOnly
    } else {
        AtBoth::SizeAndLastModified
    };
    let case_conflicts: Option<CaseConflictKeys> =
        should_handle_case_conflicts(ctx).then(|| Arc::new(Mutex::new(HashSet::new())));
    let delete = is_sync && options.delete;
    let operation = match plan.paths_type {
        PathsType::LocalS3 => Operation::Upload,
        PathsType::S3Local => Operation::Download,
        _ => Operation::Copy,
    };
    let (src, dest, use_src_name) = paths::format_paths(&plan.src, &plan.dest, true);
    let (rev_src, rev_dest, rev_use_src_name) = paths::format_paths(&plan.dest, &plan.src, true);
    let filters = create_filters(ctx);
    let mut src_files = Stream::new(spawn_generator(ctx, &src, false), &filters, &src);
    let mut dest_files = Stream::new(spawn_generator(ctx, &rev_src, false), &filters, &rev_src);
    let mut src_file = None;
    let mut dest_file = None;
    loop {
        if src_file.is_none() {
            src_file = match src_files.next(ctx).await {
                Ok(file) => file,
                Err(()) => return submitter.abort(),
            };
        }
        if dest_file.is_none() {
            dest_file = match dest_files.next(ctx).await {
                Ok(file) => file,
                Err(()) => return submitter.abort(),
            };
        }
        let src_key = src_file
            .as_ref()
            .map(|stat: &FileStat| key_of(&src, &dest, use_src_name, &stat.path));
        let dest_key = dest_file
            .as_ref()
            .map(|stat: &FileStat| key_of(&rev_src, &rev_dest, rev_use_src_name, &stat.path));
        let ordering = match (&src_key, &dest_key) {
            (Some((_, src_key)), Some((_, dest_key))) => src_key.cmp(dest_key),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => break,
        };
        match ordering {
            std::cmp::Ordering::Equal => {
                let (src_stat, dest_stat) = (src_file.take().unwrap(), dest_file.take().unwrap());
                let (dest_path, compare_key) = src_key.unwrap();
                let task = task(operation, src_stat, dest_path, compare_key);
                if should_sync(at_both, &task, &dest_stat) {
                    submitter.submit(task).await;
                }
            }
            std::cmp::Ordering::Less => {
                let src_stat = src_file.take().unwrap();
                let (dest_path, compare_key) = src_key.unwrap();
                let mut task = task(operation, src_stat, dest_path, compare_key);
                if let Some(keys) = &case_conflicts {
                    match check_case_conflict(ctx, keys, &task) {
                        Ok(true) => {}
                        Ok(false) => continue,
                        Err(message) => {
                            ctx.printer().error(&message);
                            return submitter.abort();
                        }
                    }
                    task.case_conflict = Some((keys.clone(), task.compare_key.to_lowercase()));
                }
                submitter.submit(task).await;
            }
            std::cmp::Ordering::Greater => {
                let dest_stat = dest_file.take().unwrap();
                if delete {
                    let (path, compare_key) = dest_key.unwrap();
                    let operation = if dest.is_s3 {
                        Operation::Delete
                    } else {
                        Operation::LocalDelete
                    };
                    submitter
                        .submit(task(operation, dest_stat, path, compare_key))
                        .await;
                }
            }
        }
    }
}

/// The filtered stream of the files from a generator.
struct Stream<'a> {
    rx: mpsc::Receiver<Item>,
    filters: &'a Filters,
    is_local: bool,
}

impl<'a> Stream<'a> {
    fn new(rx: mpsc::Receiver<Item>, filters: &'a Filters, endpoint: &Endpoint) -> Self {
        Self {
            rx,
            filters,
            is_local: !endpoint.is_s3,
        }
    }

    /// Returns the next file, or an error on a fatal error.
    async fn next(&mut self, ctx: &Context) -> Result<Option<FileStat>, ()> {
        loop {
            let item = tokio::select! {
                item = self.rx.recv() => item,
                () = ctx.cancelled() => return Err(()),
            };
            let Some(item) = item else {
                break;
            };
            match item {
                Item::File(stat) if self.filters.is_included(&stat.path, self.is_local) => {
                    return Ok(Some(stat));
                }
                Item::File(_) => {}
                Item::Warning(message) => ctx.warning(&message),
                Item::Error(message) => {
                    ctx.printer().error(&message);
                    return Err(());
                }
            }
        }
        Ok(None)
    }
}

fn key_of(src: &Endpoint, dest: &Endpoint, use_src_name: bool, path: &str) -> (String, String) {
    paths::dest_path_and_compare_key(src, dest, true, use_src_name, path)
}

fn task(operation: Operation, stat: FileStat, dest: String, compare_key: String) -> Task {
    Task {
        operation,
        src: stat.path,
        dest,
        compare_key,
        size: stat.size,
        last_modified: stat.last_modified,
        object: stat.object,
        case_conflict: None,
    }
}

/// Returns whether the file at both the source and the destination is synced.
fn should_sync(at_both: AtBoth, src: &Task, dest: &FileStat) -> bool {
    let same_size = || src.size == dest.size;
    match at_both {
        AtBoth::Always => true,
        AtBoth::Never => false,
        AtBoth::SizeOnly => !same_size(),
        AtBoth::SizeAndLastModified | AtBoth::ExactTimestamps => {
            let delta = micros(dest.last_modified) - micros(src.last_modified);
            let same_time = match src.operation {
                Operation::Download if at_both == AtBoth::ExactTimestamps => delta == 0,
                Operation::Download => delta <= 0,
                _ => delta >= 0,
            };
            !same_size() || !same_time
        }
    }
}

fn micros(time: Option<SystemTime>) -> i128 {
    let time = time.unwrap_or(SystemTime::UNIX_EPOCH);
    match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(duration) => duration.as_micros() as i128,
        Err(err) => -(err.duration().as_micros() as i128),
    }
}

/// Checks the download whose key may conflict with another file only by
/// case, and returns whether it is downloaded, or an error message.
fn check_case_conflict(
    ctx: &Context,
    keys: &CaseConflictKeys,
    task: &Task,
) -> Result<bool, String> {
    let key = task.compare_key.to_lowercase();
    let conflicts = keys
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .contains(&key)
        || Path::new(&task.dest).exists();
    if !conflicts {
        return Ok(true);
    }
    let (src, dest) = (&task.src, &task.dest);
    match ctx.options.case_conflict {
        CaseConflict::Ignore => Ok(true),
        CaseConflict::Skip => {
            ctx.notice(&format!(
                "warning: Skipping {src} -> {dest} because a file whose name differs only by case \
                 either exists or is being downloaded."
            ));
            Ok(false)
        }
        CaseConflict::Warn => {
            ctx.notice(&format!(
                "warning: Downloading {src} -> {dest} despite a file whose name differs only by case \
                 either existing or being downloaded. This behavior is not defined on case-insensitive \
                 filesystems and may result in overwriting existing files or race conditions between \
                 concurrent downloads. For more information, see \
                 https://docs.aws.amazon.com/cli/latest/topic/s3-case-insensitivity.html."
            ));
            Ok(true)
        }
        CaseConflict::Error => Err(format!(
            "Failed to download {src} -> {dest} because a file whose name differs only by case \
             either exists or is being downloaded."
        )),
    }
}
