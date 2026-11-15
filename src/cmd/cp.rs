//! The cp command, which copies the files and the objects.

use std::process::ExitCode;

use crate::cli::{CpArgs, GlobalArgs};
use crate::error::Error;
use crate::transfer::{self, CommandName, Options, RequestParams, Unpacked};

pub async fn run(args: &CpArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let paths = vec![args.source.clone(), args.destination.clone()];
    let mut options =
        Options::new(CommandName::Cp, paths, args.recursive).with_transfer_args(&args.transfer);
    options.expected_size = args.expected_size;
    let unpacked = Unpacked::load(&args.transfer)?;
    let params = RequestParams::new(&args.transfer, &unpacked);
    transfer::run(options, params, global).await
}
