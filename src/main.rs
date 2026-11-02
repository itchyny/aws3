mod bucket;
mod cli;
mod client;
mod error;
mod filter;
mod ls;
mod mime;
mod path;
mod presign;
mod tls;
mod transfer;

use std::process::ExitCode;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse_args();
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
