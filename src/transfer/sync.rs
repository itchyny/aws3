//! The comparison of the source and the destination, in the same way as the
//! `Comparator` and the sync strategies of the AWS CLI.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use tokio::sync::mpsc;

use super::context::Context;
use super::execute::Submitter;
use super::filter::Filters;
use super::generator::{FileStat, Item};
use super::options::CommandName;
use super::paths::{self, Endpoint};
use super::plan::PathsType;
use super::source::{create_filters, spawn_generator};
use super::task::{CaseConflictKeys, Operation, Task};
use crate::cli::CaseConflict;

/// How a file at both the source and the destination is synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtBoth {
    SizeAndLastModified,
    SizeOnly,
    ExactTimestamps,
    Never,
    Always,
}

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
    let mut src_files = Stream::new(spawn_generator(ctx, &src, false, true), &filters, &src);
    let mut dest_files = Stream::new(
        spawn_generator(ctx, &rev_src, false, true),
        &filters,
        &rev_src,
    );
    // The prefixes of the source and the destination may be nested in a
    // bucket. The source containing the destination is listed entirely first,
    // so that the copies are not listed and copied again, and the objects of
    // the destination under the source are not deleted, which are the sources
    // being copied.
    let is_s3_s3 = src.is_s3 && dest.is_s3;
    if is_s3_s3 && dest.path.starts_with(&src.path) && src_files.list_all(ctx).await.is_err() {
        return submitter.abort();
    }
    // The files with the destination paths and the compare keys, which are
    // merged in the order of the compare keys.
    let mut src_file = None;
    let mut dest_file = None;
    loop {
        if src_file.is_none() {
            src_file = match src_files.next(ctx).await {
                Ok(file) => file.map(|stat| {
                    let (path, key) = key_of(&src, &dest, use_src_name, &stat.path);
                    (stat, path, key)
                }),
                Err(()) => return submitter.abort(),
            };
        }
        if dest_file.is_none() {
            dest_file = match dest_files.next(ctx).await {
                Ok(file) => file.map(|stat| {
                    let (path, key) = key_of(&rev_src, &rev_dest, rev_use_src_name, &stat.path);
                    (stat, path, key)
                }),
                Err(()) => return submitter.abort(),
            };
        }
        // The file with the larger compare key is kept for the next files.
        let files = match (src_file.take(), dest_file.take()) {
            (Some(src), Some(dest)) => match src.2.cmp(&dest.2) {
                std::cmp::Ordering::Equal => (Some(src), Some(dest)),
                std::cmp::Ordering::Less => {
                    dest_file = Some(dest);
                    (Some(src), None)
                }
                std::cmp::Ordering::Greater => {
                    src_file = Some(src);
                    (None, Some(dest))
                }
            },
            files => files,
        };
        match files {
            (None, None) => break,
            (Some((src_stat, dest_path, compare_key)), Some((dest_stat, _, _))) => {
                let task = task(operation, src_stat, dest_path, compare_key);
                if should_sync(at_both, &task, &dest_stat) {
                    submitter.submit(task).await;
                }
            }
            (Some((src_stat, dest_path, compare_key)), None) => {
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
            (None, Some((dest_stat, path, compare_key))) => {
                if delete && !(is_s3_s3 && dest_stat.path.starts_with(&src.path)) {
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
    /// The items received entirely before the files are returned.
    listed: Option<VecDeque<Item>>,
}

impl<'a> Stream<'a> {
    fn new(rx: mpsc::Receiver<Item>, filters: &'a Filters, endpoint: &Endpoint) -> Self {
        Self {
            rx,
            filters,
            is_local: !endpoint.is_s3,
            listed: None,
        }
    }

    /// Receives all the items before returning the files, or returns an
    /// error on the cancellation.
    async fn list_all(&mut self, ctx: &Context) -> Result<(), ()> {
        let mut listed = VecDeque::new();
        loop {
            let item = tokio::select! {
                item = self.rx.recv() => item,
                () = ctx.cancelled() => return Err(()),
            };
            let Some(item) = item else {
                break;
            };
            listed.push_back(item);
        }
        self.listed = Some(listed);
        Ok(())
    }

    /// Returns the next file, or an error on a fatal error.
    async fn next(&mut self, ctx: &Context) -> Result<Option<FileStat>, ()> {
        loop {
            let item = match &mut self.listed {
                Some(listed) => listed.pop_front(),
                None => tokio::select! {
                    item = self.rx.recv() => item,
                    () = ctx.cancelled() => return Err(()),
                },
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
/// The key of the download is added to the keys being downloaded.
fn check_case_conflict(
    ctx: &Context,
    keys: &CaseConflictKeys,
    task: &Task,
) -> Result<bool, String> {
    let key = task.compare_key.to_lowercase();
    let mut keys = keys.lock().unwrap_or_else(PoisonError::into_inner);
    if keys.contains(&key) || Path::new(&task.dest).exists() {
        let (src, dest) = (&task.src, &task.dest);
        match ctx.options.case_conflict {
            CaseConflict::Ignore => {}
            CaseConflict::Skip => {
                ctx.notice(&format!(
                    "warning: Skipping {src} -> {dest} because a file whose name differs only by case \
                     either exists or is being downloaded."
                ));
                return Ok(false);
            }
            CaseConflict::Warn => {
                ctx.notice(&format!(
                    "warning: Downloading {src} -> {dest} despite a file whose name differs only by case \
                     either existing or being downloaded. This behavior is not defined on case-insensitive \
                     filesystems and may result in overwriting existing files or race conditions between \
                     concurrent downloads. For more information, see \
                     https://docs.aws.amazon.com/cli/latest/topic/s3-case-insensitivity.html."
                ));
            }
            CaseConflict::Error => {
                return Err(format!(
                    "Failed to download {src} -> {dest} because a file whose name differs only by case \
                     either exists or is being downloaded."
                ));
            }
        }
    }
    keys.insert(key);
    Ok(true)
}
