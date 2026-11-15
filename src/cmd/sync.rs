//! The sync command, which synchronizes the directories and the prefixes.

use std::process::ExitCode;

use crate::cli::{GlobalArgs, SyncArgs};
use crate::error::Error;
use crate::transfer::{self, CommandName, Options, RequestParams, Unpacked};

pub async fn run(args: &SyncArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let paths = vec![args.source.clone(), args.destination.clone()];
    let mut options =
        Options::new(CommandName::Sync, paths, true).with_transfer_args(&args.transfer);
    options.size_only = args.size_only;
    options.exact_timestamps = args.exact_timestamps;
    options.delete = args.delete;
    // The no-overwrite option is handled by comparing the files.
    let unpacked = Unpacked::load(&args.transfer)?;
    let params = RequestParams::new(&args.transfer, &unpacked).map(|params| RequestParams {
        no_overwrite: false,
        ..params
    });
    transfer::run(options, params, global).await
}
