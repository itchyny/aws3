//! The rm command, which deletes the objects.

use std::process::ExitCode;

use crate::cli::{GlobalArgs, RmArgs};
use crate::error::Error;
use crate::transfer::{self, CommandName, Options, RequestParams};

pub async fn run(args: &RmArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let mut options = Options::new(CommandName::Rm, vec![args.path.clone()], args.recursive);
    options.dryrun = args.dryrun;
    options.quiet = args.quiet;
    options.only_show_errors = args.only_show_errors;
    // The rm command has no progress option, which disables the progress.
    options.progress = false;
    options.page_size = args.page_size;
    options.filters = args.filter.filters.clone();
    let params = RequestParams::with_request_payer(args.request_payer.request_payer);
    transfer::run(options, Ok(params), global).await
}

/// Deletes the objects under the path, for rb with `--force`.
pub async fn run_recursive(path: &str, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let mut options = Options::new(CommandName::Rm, vec![path.to_owned()], true);
    options.progress = false;
    transfer::run(options, Ok(RequestParams::default()), global).await
}
