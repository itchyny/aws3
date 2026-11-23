//! The transfers of the cp, mv, rm and sync commands, in the same way as the
//! `CommandArchitecture` of the AWS CLI.
mod bandwidth;
mod checksum;
mod context;
mod copy;
mod delete;
mod download;
mod execute;
mod filter;
mod generator;
mod mime;
mod options;
mod params;
mod paths;
mod plan;
mod printer;
mod samepath;
mod source;
mod sync;
mod task;
mod upload;

use std::collections::VecDeque;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::{Semaphore, watch};

use crate::cli::{CaseConflict, GlobalArgs};
use crate::client;
use crate::error::Error;
use crate::path::{is_s3express_bucket, split_bucket_key};
use crate::redirect::Redirector;
use context::Context;
use execute::Submitter;
use generator::Item;
pub use options::{CommandName, Options};
pub use params::{RequestParams, Unpacked};
use plan::{PathsType, Plan};
use printer::{PrintMode, Printer};
use source::{create_filters, spawn_generator};
use task::{Operation, Task};

/// Runs the transfer command with the options and the request parameters.
pub async fn run(
    mut options: Options,
    params: Result<RequestParams, String>,
    global: &GlobalArgs,
) -> Result<ExitCode, Error> {
    let plan = Plan::new(&mut options)?;
    let sdk_config = client::load_config(global).await?;
    if options.command == CommandName::Mv && plan.paths_type == PathsType::S3S3 {
        let region = options.source_region.as_deref();
        let enabled = options.validate_same_s3_paths || samepath::enabled_by_env();
        samepath::validate(&sdk_config, global, region, &plan.src, &plan.dest, enabled).await?;
    }
    // The keys of directory buckets are not listed in order, where the case
    // conflicts cannot be detected like the AWS CLI.
    if matches!(options.command, CommandName::Cp | CommandName::Mv)
        && plan.paths_type == PathsType::S3Local
        && options.dir_op
        && options.case_conflict != CaseConflict::Ignore
        && is_s3express_bucket(split_bucket_key(&plan.src).0)
    {
        if options.case_conflict != CaseConflict::Warn {
            let value = clap::ValueEnum::to_possible_value(&options.case_conflict)
                .map(|value| value.get_name().to_owned())
                .unwrap_or_default();
            return Err(Error::param_validation(format!(
                "`{value}` is not a valid value for `--case-conflict` when operating on S3 \
                 Express directory buckets. Valid values: `warn`, `ignore`."
            )));
        }
        // The warning does not end with a newline like the AWS CLI.
        eprint!(
            "warning: Recursive copies/moves from an S3 Express directory bucket to a \
             case-insensitive local filesystem may result in undefined behavior if there are \
             S3 object key names that differ only by case. To disable this warning, set the \
             `--case-conflict` parameter to `ignore`. For more information, see \
             https://docs.aws.amazon.com/cli/latest/topic/s3-case-insensitivity.html."
        );
        options.case_conflict = CaseConflict::Ignore;
    }
    let client = client::new_client(&sdk_config, None);
    let source_client = match &options.source_region {
        Some(region) => client::new_client(&sdk_config, Some(region)),
        None => client.clone(),
    };
    let mode = if options.quiet {
        PrintMode::Quiet
    } else if options.only_show_errors {
        PrintMode::OnlyShowErrors
    } else if !options.progress {
        PrintMode::NoProgress
    } else {
        PrintMode::Progress {
            frequency: options.progress_frequency,
            oneline: !options.progress_multiline,
        }
    };
    let mut printer = Printer::new(mode);
    let params = match params {
        Ok(params) => params,
        Err(message) => {
            printer.error(&message);
            return Ok(printer.exit_code());
        }
    };
    let config = sdk_config.s3.clone();
    if let Some(err) = &config.invalid {
        return Err(Error::new(err));
    }
    let limiter = config
        .max_bandwidth
        .map(|rate| Arc::new(bandwidth::Limiter::new(rate)));
    let ctx = Arc::new(Context {
        client,
        source_client,
        params,
        requests: Semaphore::new(config.max_concurrent_requests),
        config,
        options,
        plan,
        printer: Mutex::new(printer),
        next_id: AtomicU64::new(0),
        cancel: watch::Sender::new(false),
        cleanup: Mutex::new(Vec::new()),
        limiter,
        redirector: Redirector::default(),
        batch_delete: AtomicBool::new(true),
    });
    // The first Ctrl-C cancels the transfers, and the second one exits.
    let listener = tokio::spawn({
        let ctx = ctx.clone();
        async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                ctx.cancel();
            }
            if tokio::signal::ctrl_c().await.is_ok() {
                std::process::exit(130);
            }
        }
    });
    let mut submitter = Submitter::new(ctx.clone());
    if ctx.plan.is_stream {
        submitter.submit(stream_task(&ctx)).await;
    } else if ctx.options.command == CommandName::Sync || sync::should_handle_case_conflicts(&ctx) {
        sync::run(&ctx, &mut submitter).await;
    } else {
        run_files(&ctx, &mut submitter).await;
    }
    submitter.finish().await;
    let cleanup = std::mem::take(&mut *ctx.cleanup.lock().unwrap_or_else(PoisonError::into_inner));
    for handle in cleanup {
        let _ = handle.await;
    }
    listener.abort();
    Ok(ctx.printer().exit_code())
}

/// Lists the sources, and submits the transfers of the files passing the
/// filters.
async fn run_files(ctx: &Arc<Context>, submitter: &mut Submitter) {
    let options = &ctx.options;
    let plan = &ctx.plan;
    let (src, dest, use_src_name) = paths::format_paths(&plan.src, &plan.dest, options.dir_op);
    let filters = create_filters(ctx);
    let operation = match plan.paths_type {
        PathsType::LocalS3 => Operation::Upload,
        PathsType::S3Local => Operation::Download,
        PathsType::S3S3 => Operation::Copy,
        PathsType::S3 => Operation::Delete,
    };
    // The objects are listed in order for the dry runs, which print the
    // transfers in the order of submission, and for the deletions, whose
    // batches are throttled by S3 when they are sent at once.
    let is_delete = operation == Operation::Delete;
    let mut rx = spawn_generator(ctx, &src, is_delete, options.dryrun || is_delete);
    // The objects copied under the source in the same bucket would be listed
    // and transferred again, so the source is listed entirely before the
    // transfers.
    let mut listed =
        (src.is_s3 && dest.is_s3 && options.dir_op && dest.path.starts_with(&src.path))
            .then(VecDeque::new);
    if let Some(listed) = &mut listed {
        loop {
            let item = tokio::select! {
                item = rx.recv() => item,
                () = ctx.cancelled() => return submitter.abort(),
            };
            let Some(item) = item else {
                break;
            };
            listed.push_back(item);
        }
    }
    loop {
        let item = match &mut listed {
            Some(listed) => listed.pop_front(),
            None => tokio::select! {
                item = rx.recv() => item,
                () = ctx.cancelled() => return submitter.abort(),
            },
        };
        let Some(item) = item else {
            break;
        };
        match item {
            Item::File(stat) => {
                if !filters.is_included(&stat.path, !src.is_s3) {
                    continue;
                }
                let (dest_path, compare_key) = paths::dest_path_and_compare_key(
                    &src,
                    &dest,
                    options.dir_op,
                    use_src_name,
                    &stat.path,
                );
                submitter
                    .submit(Task {
                        operation,
                        src: stat.path,
                        dest: dest_path,
                        compare_key,
                        size: stat.size,
                        last_modified: stat.last_modified,
                        object: stat.object,
                        case_conflict: None,
                    })
                    .await;
            }
            Item::Warning(message) => ctx.warning(&message),
            Item::Error(message) => {
                ctx.printer().error(&message);
                return submitter.abort();
            }
        }
    }
}

/// Creates the task of streaming from the standard input or to the standard
/// output.
fn stream_task(ctx: &Context) -> Task {
    let plan = &ctx.plan;
    let (src, dest, use_src_name) = paths::format_paths(&plan.src, &plan.dest, false);
    let (dest_path, compare_key) =
        paths::dest_path_and_compare_key(&src, &dest, false, use_src_name, &src.path);
    Task {
        operation: if src.is_s3 {
            Operation::Download
        } else {
            Operation::Upload
        },
        src: src.path,
        dest: dest_path,
        compare_key,
        size: None,
        last_modified: None,
        object: None,
        case_conflict: None,
    }
}
