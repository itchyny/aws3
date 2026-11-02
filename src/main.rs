mod bucket;
mod cli;
mod client;
mod config;
mod error;
mod filter;
mod insecure;
mod ls;
mod mime;
mod path;
mod presign;
mod tls;
mod transfer;

use std::process::ExitCode;

use cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse_args();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build the runtime");
    let code = runtime.block_on(run(&cli));
    // The blocking reads of the standard input are not waited for, which
    // remain when the transfers are cancelled by Ctrl-C.
    runtime.shutdown_background();
    code
}

async fn run(cli: &Cli) -> ExitCode {
    let result = match &cli.command {
        Command::Ls(args) => ls::run(args, &cli.global).await,
        Command::Cp(args) => transfer::cp(args, &cli.global).await,
        Command::Mv(args) => transfer::mv(args, &cli.global).await,
        Command::Rm(args) => transfer::rm(args, &cli.global).await,
        Command::Sync(args) => transfer::sync(args, &cli.global).await,
        Command::Mb(args) => bucket::mb(args, &cli.global).await,
        Command::Rb(args) => bucket::rb(args, &cli.global).await,
        Command::Website(args) => bucket::website(args, &cli.global).await,
        Command::Presign(args) => presign::run(args, &cli.global).await,
    };
    result.unwrap_or_else(|err| {
        eprintln!("\naws3: [ERROR]: {err}");
        err.exit_code()
    })
}
