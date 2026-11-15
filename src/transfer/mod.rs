//! The transfers of the cp, mv, rm and sync commands, in the same way as the
//! `CommandArchitecture` of the AWS CLI.
mod bandwidth;
mod checksum;
mod context;
mod copy;
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
mod sync;
mod upload;

use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use tokio::sync::{Semaphore, mpsc, watch};

use crate::cli::{CaseConflict, GlobalArgs};
use crate::client;
use crate::error::Error;
use crate::path::{is_s3express_bucket, split_bucket_key};
use crate::redirect::Redirector;
use context::Context;
use execute::{Operation, Submitter, Task};
use filter::Filters;
use generator::{Item, ListOptions};
pub use options::{CommandName, Options};
pub use params::{RequestParams, Unpacked};
use paths::Endpoint;
use plan::{PathsType, Plan};
use printer::{PrintMode, Printer};

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
    let cleanup = std::mem::take(&mut *ctx.cleanup.lock().unwrap_or_else(|err| err.into_inner()));
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
    let mut rx = spawn_generator(ctx, &src, operation == Operation::Delete);
    loop {
        let item = tokio::select! {
            item = rx.recv() => item,
            () = ctx.cancelled() => return submitter.abort(),
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

/// Creates the filters rooted at the source and the destination.
fn create_filters(ctx: &Context) -> Filters {
    let dir_op = ctx.options.dir_op;
    let root = |path: &str| {
        if let Some(path) = path.strip_prefix("s3://") {
            let (bucket, key) = split_bucket_key(path);
            let key = if !dir_op && !key.ends_with('/') {
                key.rsplit_once('/').map_or("", |(dir, _)| dir)
            } else {
                key
            };
            format!("{bucket}/{key}")
        } else if dir_op {
            paths::abspath(path)
        } else {
            let parent = Path::new(path)
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned());
            paths::abspath(
                parent
                    .as_deref()
                    .filter(|parent| !parent.is_empty())
                    .unwrap_or("."),
            )
        }
    };
    Filters::new(
        &ctx.options.filters,
        &root(&ctx.plan.src),
        &root(&ctx.plan.dest),
    )
}

/// Spawns the generator of the files or objects at the endpoint.
fn spawn_generator(
    ctx: &Arc<Context>,
    endpoint: &Endpoint,
    is_delete: bool,
) -> mpsc::Receiver<Item> {
    let (tx, rx) = mpsc::channel(ctx.config.max_queue_size);
    let path = endpoint.path.clone();
    let dir_op = ctx.options.dir_op;
    if endpoint.is_s3 {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let options = ListOptions {
                page_size: ctx.options.page_size,
                request_payer: ctx.params.request_payer.clone(),
                sse_c: if ctx.plan.paths_type == PathsType::S3S3 {
                    ctx.params.sse_c_copy_source.clone()
                } else {
                    ctx.params.sse_c.clone()
                },
                checksum_mode: ctx.params.checksum_mode.clone(),
                is_delete,
            };
            let client = &ctx.source_client;
            generator::list_s3(client, &ctx.redirector, &path, dir_op, &options, &tx).await;
        });
    } else {
        let follow_symlinks = ctx.options.follow_symlinks;
        tokio::task::spawn_blocking(move || {
            generator::list_local(&path, dir_op, follow_symlinks, &tx)
        });
    }
    rx
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
