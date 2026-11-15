//! The mv command, which moves the files and the objects.

use std::process::ExitCode;

use crate::cli::{GlobalArgs, MvArgs};
use crate::error::Error;
use crate::transfer::{self, CommandName, Options, RequestParams, Unpacked};

pub async fn run(args: &MvArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let paths = vec![args.source.clone(), args.destination.clone()];
    let mut options =
        Options::new(CommandName::Mv, paths, args.recursive).with_transfer_args(&args.transfer);
    options.validate_same_s3_paths = args.validate_same_s3_paths;
    let unpacked = Unpacked::load(&args.transfer)?;
    let params = RequestParams::new(&args.transfer, &unpacked);
    transfer::run(options, params, global).await
}
