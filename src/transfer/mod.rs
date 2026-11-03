//! The transfer commands, i.e. cp, mv, rm and sync, in the same way as the
//! `CommandArchitecture` of the AWS CLI.

mod bandwidth;
mod copy;
mod download;
mod generator;
pub mod params;
mod paths;
mod printer;
mod sync;
mod upload;

use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use aws_sdk_s3::Client;
use tokio::sync::{Semaphore, mpsc, watch};
use tokio::task::{JoinHandle, JoinSet};

use crate::cli::{
    CaseConflict, CopyProps, CpArgs, Filter, GlobalArgs, MvArgs, RmArgs, SyncArgs, TransferArgs,
};
use crate::client;
use crate::config::S3Config;
use crate::error::Error;
use crate::filter::Filters;
use crate::path::validate_bucket_name;
use generator::{Item, ListOptions, ObjectProps, skip_warning};
use params::RequestParams;
use paths::{Endpoint, relative_path, split_bucket_key};
use printer::{PrintMode, Printer, TransferId};

/// The maximum size of an object of S3.
const MAX_UPLOAD_SIZE: u64 = 5 * 1024 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandName {
    Cp,
    Mv,
    Rm,
    Sync,
}

impl CommandName {
    fn name(self) -> &'static str {
        match self {
            Self::Cp => "cp",
            Self::Mv => "mv",
            Self::Rm => "rm",
            Self::Sync => "sync",
        }
    }

    fn usage(self) -> &'static str {
        match self {
            Self::Rm => "<S3Uri>",
            _ => "<LocalPath> <S3Uri> or <S3Uri> <LocalPath> or <S3Uri> <S3Uri>",
        }
    }
}

/// The options of the transfer commands.
#[derive(Debug, Clone)]
pub struct Options {
    pub command: CommandName,
    pub paths: Vec<String>,
    pub dir_op: bool,
    pub dryrun: bool,
    pub quiet: bool,
    pub only_show_errors: bool,
    pub progress: bool,
    pub progress_frequency: u64,
    pub progress_multiline: bool,
    pub follow_symlinks: bool,
    pub guess_mime_type: bool,
    pub page_size: Option<i32>,
    pub filters: Vec<Filter>,
    pub source_region: Option<String>,
    pub ignore_glacier_warnings: bool,
    pub force_glacier_transfer: bool,
    pub copy_props: CopyProps,
    pub case_conflict: CaseConflict,
    pub expected_size: Option<u64>,
    pub no_overwrite: bool,
    pub size_only: bool,
    pub exact_timestamps: bool,
    pub delete: bool,
    has_checksum_algorithm: bool,
    has_checksum_mode: bool,
    has_sse_c: bool,
    has_sse_c_key: bool,
    has_sse_c_copy_source: bool,
    has_sse_c_copy_source_key: bool,
}

impl Options {
    fn new(command: CommandName, paths: Vec<String>, dir_op: bool) -> Self {
        Self {
            command,
            paths,
            dir_op,
            dryrun: false,
            quiet: false,
            only_show_errors: false,
            progress: true,
            progress_frequency: 0,
            progress_multiline: false,
            follow_symlinks: true,
            guess_mime_type: true,
            page_size: None,
            filters: Vec::new(),
            source_region: None,
            ignore_glacier_warnings: false,
            force_glacier_transfer: false,
            copy_props: CopyProps::Default,
            case_conflict: CaseConflict::Ignore,
            expected_size: None,
            no_overwrite: false,
            size_only: false,
            exact_timestamps: false,
            delete: false,
            has_checksum_algorithm: false,
            has_checksum_mode: false,
            has_sse_c: false,
            has_sse_c_key: false,
            has_sse_c_copy_source: false,
            has_sse_c_copy_source_key: false,
        }
    }

    fn with_transfer_args(self, args: &TransferArgs) -> Self {
        Self {
            dryrun: args.dryrun,
            quiet: args.quiet,
            only_show_errors: args.only_show_errors,
            progress: !args.no_progress,
            progress_frequency: args.progress_frequency.unwrap_or(0),
            progress_multiline: args.progress_multiline,
            follow_symlinks: !args.no_follow_symlinks,
            guess_mime_type: !args.no_guess_mime_type,
            page_size: args.page_size,
            filters: args.filter.filters.clone(),
            source_region: args.source_region.clone(),
            ignore_glacier_warnings: args.ignore_glacier_warnings,
            force_glacier_transfer: args.force_glacier_transfer,
            copy_props: args.copy_props,
            case_conflict: args.case_conflict,
            no_overwrite: args.no_overwrite,
            has_checksum_algorithm: args.checksum_algorithm.is_some(),
            has_checksum_mode: args.checksum_mode.is_some(),
            has_sse_c: args.sse_c.is_some(),
            has_sse_c_key: args.sse_c_key.is_some(),
            has_sse_c_copy_source: args.sse_c_copy_source.is_some(),
            has_sse_c_copy_source_key: args.sse_c_copy_source_key.is_some(),
            ..self
        }
    }
}

pub async fn cp(args: &CpArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let paths = vec![args.source.clone(), args.destination.clone()];
    let mut options =
        Options::new(CommandName::Cp, paths, args.recursive).with_transfer_args(&args.transfer);
    options.expected_size = args.expected_size;
    run(options, RequestParams::new(&args.transfer, global), global).await
}

pub async fn mv(args: &MvArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let paths = vec![args.source.clone(), args.destination.clone()];
    let options =
        Options::new(CommandName::Mv, paths, args.recursive).with_transfer_args(&args.transfer);
    run(options, RequestParams::new(&args.transfer, global), global).await
}

pub async fn rm(args: &RmArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let mut options = Options::new(CommandName::Rm, vec![args.path.clone()], args.recursive);
    options.dryrun = args.dryrun;
    options.quiet = args.quiet;
    options.only_show_errors = args.only_show_errors;
    // The rm command has no progress option, which disables the progress.
    options.progress = false;
    options.page_size = args.page_size;
    options.filters = args.filter.filters.clone();
    let params = RequestParams::with_request_payer(args.request_payer.request_payer);
    run(options, Ok(params), global).await
}

/// Deletes the objects under the path, for rb with `--force`.
pub async fn rm_recursive(path: &str, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let mut options = Options::new(CommandName::Rm, vec![path.to_owned()], true);
    options.progress = false;
    run(options, Ok(RequestParams::default()), global).await
}

pub async fn sync(args: &SyncArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let paths = vec![args.source.clone(), args.destination.clone()];
    let mut options =
        Options::new(CommandName::Sync, paths, true).with_transfer_args(&args.transfer);
    options.size_only = args.size_only;
    options.exact_timestamps = args.exact_timestamps;
    options.delete = args.delete;
    // The no-overwrite option is handled by comparing the files.
    let params = RequestParams::new(&args.transfer, global).map(|params| RequestParams {
        no_overwrite: false,
        ..params
    });
    run(options, params, global).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathsType {
    LocalS3,
    S3Local,
    S3S3,
    S3,
}

impl PathsType {
    fn usage(self) -> &'static str {
        match self {
            Self::LocalS3 => "<LocalPath> <S3Uri>",
            Self::S3S3 => "<S3Uri> <S3Uri>",
            Self::S3Local => "<S3Uri> <LocalPath>",
            Self::S3 => "<S3Uri>",
        }
    }
}

/// The validated source and destination of the command.
#[derive(Debug, Clone)]
struct Plan {
    src: String,
    dest: String,
    paths_type: PathsType,
    is_stream: bool,
}

impl Plan {
    /// Validates the paths and the options in the same order as the AWS CLI.
    fn new(options: &mut Options) -> Result<Self, Error> {
        let command = options.command;
        let is_s3: Vec<bool> = options
            .paths
            .iter()
            .map(|path| path.starts_with("s3://"))
            .collect();
        let paths_type = match (command, is_s3.as_slice()) {
            (CommandName::Rm, [true]) => PathsType::S3,
            (CommandName::Rm, _) => None.ok_or_else(|| invalid_argument_type(command))?,
            (_, [false, true]) => PathsType::LocalS3,
            (_, [true, false]) => PathsType::S3Local,
            (_, [true, true]) => PathsType::S3S3,
            _ => return Err(invalid_argument_type(command)),
        };
        let mut paths = options.paths.clone();
        for path in &mut paths {
            if let Some(rest) = path.strip_prefix("s3://")
                && split_bucket_key(rest).1.is_empty()
                && !path.ends_with('/')
            {
                path.push('/');
            }
        }
        let src = paths[0].clone();
        let dest = paths.get(1).unwrap_or(&paths[0]).clone();
        let mut is_stream = false;
        if src == "-" || dest == "-" {
            if command != CommandName::Cp || options.dir_op {
                return Err(Error::param_validation(
                    "Streaming currently is only compatible with non-recursive cp commands",
                ));
            }
            is_stream = true;
            options.dir_op = false;
            options.only_show_errors = true;
        }
        if is_stream && options.no_overwrite && dest == "-" {
            return Err(Error::param_validation(
                "--no-overwrite parameter is not supported for streaming downloads",
            ));
        }
        if command == CommandName::Mv && paths_type == PathsType::S3S3 && is_same_path(&src, &dest)
        {
            return Err(Error::param_validation(format!(
                "Cannot mv a file onto itself: {src} - {dest}"
            )));
        }
        if options.has_checksum_algorithm {
            check_paths_type(
                "checksum-algorithm",
                paths_type,
                &[PathsType::LocalS3, PathsType::S3S3],
            )?;
        }
        if options.has_checksum_mode {
            check_paths_type("checksum-mode", paths_type, &[PathsType::S3Local])?;
        }
        if paths_type == PathsType::LocalS3 && !is_stream && !Path::new(&src).exists() {
            return Err(Error::new(format!(
                "The user-provided path {src} does not exist."
            )));
        }
        if paths_type == PathsType::S3Local && options.dir_op && !Path::new(&dest).exists() {
            std::fs::create_dir_all(&dest)?;
        }
        for (has_algorithm, has_key, name) in [
            (options.has_sse_c, options.has_sse_c_key, "sse-c"),
            (
                options.has_sse_c_copy_source,
                options.has_sse_c_copy_source_key,
                "sse-c-copy-source",
            ),
        ] {
            if has_algorithm && !has_key {
                return Err(Error::param_validation(format!(
                    "If --{name} is specified, --{name}-key must be specified as well."
                )));
            }
            if has_key && !has_algorithm {
                return Err(Error::param_validation(format!(
                    "If --{name}-key is specified, --{name} must be specified as well."
                )));
            }
        }
        if options.has_sse_c_copy_source && paths_type != PathsType::S3S3 {
            return Err(Error::param_validation(
                "--sse-c-copy-source is only supported for copy operations.",
            ));
        }
        Ok(Self {
            src,
            dest,
            paths_type,
            is_stream,
        })
    }
}

fn invalid_argument_type(command: CommandName) -> Error {
    Error::param_validation(format!(
        "usage: aws3 {} {}\nError: Invalid argument type",
        command.name(),
        command.usage()
    ))
}

fn check_paths_type(
    param: &str,
    paths_type: PathsType,
    allowed: &[PathsType],
) -> Result<(), Error> {
    if allowed.contains(&paths_type) {
        return Ok(());
    }
    let allowed: Vec<_> = allowed
        .iter()
        .map(|paths_type| paths_type.usage())
        .collect();
    Err(Error::param_validation(format!(
        "Expected {param} parameter to be used with one of following path formats: {}. Instead, received {}.",
        allowed.join(", "),
        paths_type.usage()
    )))
}

/// Returns whether moving the source to the destination is a no-op.
fn is_same_path(src: &str, dest: &str) -> bool {
    src == dest
        || dest.ends_with('/')
            && src == format!("{dest}{}", src.rsplit('/').next().unwrap_or_default())
}

/// The kind of a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Upload,
    Download,
    Copy,
    Delete,
    LocalDelete,
}

impl Operation {
    fn name(self) -> &'static str {
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
struct Task {
    operation: Operation,
    /// The absolute local path, or `bucket/key` of S3.
    src: String,
    dest: String,
    compare_key: String,
    size: Option<u64>,
    last_modified: Option<SystemTime>,
    object: Option<ObjectProps>,
    /// The keys of the downloads to remove this key from on completion.
    case_conflict: Option<(sync::CaseConflictKeys, String)>,
}

/// The result of a transfer which did not fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Done,
    /// The transfer is skipped because the destination already exists.
    Skipped,
}

/// The shared state of the transfers.
pub struct Context {
    client: Client,
    source_client: Client,
    params: RequestParams,
    options: Options,
    plan: Plan,
    config: S3Config,
    /// Limits the number of concurrent requests.
    requests: Semaphore,
    printer: Mutex<Printer>,
    next_id: AtomicU64,
    /// Whether the command is cancelled by Ctrl-C.
    cancel: watch::Sender<bool>,
    /// The tasks to clean up the cancelled transfers, e.g. to abort the
    /// multipart uploads.
    cleanup: Mutex<Vec<JoinHandle<()>>>,
    /// The limit of the bandwidth of uploads and downloads.
    limiter: Option<Arc<bandwidth::Limiter>>,
}

impl Context {
    fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    /// Waits until the command is cancelled.
    async fn cancelled(&self) {
        let _ = self
            .cancel
            .subscribe()
            .wait_for(|&cancelled| cancelled)
            .await;
    }

    /// Cancels the command, which stops submitting and aborts the transfers.
    fn cancel(&self) {
        if !self.cancel.send_replace(true) {
            self.printer().ctrl_c();
        }
    }

    /// Spawns the task to clean up a cancelled transfer, which the command
    /// waits for before exiting.
    fn spawn_cleanup(&self, future: impl Future<Output = ()> + Send + 'static) {
        let handle = tokio::spawn(future);
        self.cleanup
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .push(handle);
    }

    fn printer(&self) -> std::sync::MutexGuard<'_, Printer> {
        self.printer.lock().unwrap_or_else(|err| err.into_inner())
    }

    fn progress(&self, id: TransferId, bytes: u64) {
        self.printer().progress(id, bytes);
    }

    fn warning(&self, message: &str) {
        self.printer().warning(message);
    }

    /// Waits until the bytes can be transferred within the bandwidth.
    async fn limit(&self, bytes: usize) {
        if let Some(limiter) = &self.limiter {
            limiter.consume(bytes).await;
        }
    }

    fn notice(&self, message: &str) {
        self.printer().notice(message);
    }

    fn is_move(&self) -> bool {
        self.options.command == CommandName::Mv
    }
}

async fn run(
    mut options: Options,
    params: Result<RequestParams, String>,
    global: &GlobalArgs,
) -> Result<ExitCode, Error> {
    let plan = Plan::new(&mut options)?;
    let sdk_config = client::load_config(global).await?;
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
    let limiter = match &config.max_bandwidth {
        Some(rate) => match crate::config::parse_rate(rate).map_err(Error::new)? {
            0 => None,
            rate => Some(Arc::new(bandwidth::Limiter::new(rate))),
        },
        None => None,
    };
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
            generator::list_s3(&ctx.source_client, &path, dir_op, &options, &tx).await;
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

/// Submits the transfers, limiting the number of queued transfers.
struct Submitter {
    ctx: Arc<Context>,
    tasks: JoinSet<()>,
    submitted: u64,
    /// Whether a fatal error occurred, which aborts the command.
    fatal: bool,
}

impl Submitter {
    fn new(ctx: Arc<Context>) -> Self {
        Self {
            ctx,
            tasks: JoinSet::new(),
            submitted: 0,
            fatal: false,
        }
    }

    async fn submit(&mut self, task: Task) {
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
    fn abort(&mut self) {
        self.fatal = true;
    }

    /// Waits for the submitted transfers.
    async fn finish(mut self) {
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
                let size = task.size.unwrap_or(0);
                download::download_file(ctx, id, bucket, key, &task.dest, size, task.last_modified)
                    .await?
            }
        }
        Operation::Copy => {
            let (src_bucket, src_key) = split_bucket_key(&task.src);
            let (bucket, key) = split_bucket_key(&task.dest);
            let head = task
                .object
                .as_ref()
                .and_then(|object| object.head.as_deref());
            copy::copy_object(
                ctx,
                id,
                src_bucket,
                src_key,
                bucket,
                key,
                task.size.unwrap_or(0),
                head,
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
    let _permit = ctx
        .requests
        .acquire()
        .await
        .map_err(|err| err.to_string())?;
    client
        .delete_object()
        .bucket(bucket)
        .key(key)
        .set_request_payer(ctx.params.request_payer.clone())
        .send()
        .await
        .map_err(|err| Error::sdk("DeleteObject", err).to_string())?;
    Ok(Outcome::Done)
}
