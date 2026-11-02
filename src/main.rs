mod bucket;
mod cli;
mod client;
mod error;
mod filter;
mod ls;
mod mime;
mod path;
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
        command => {
            eprintln!("aws3: {} is not implemented yet", command_name(command));
            return ExitCode::FAILURE;
        }
    };
    result.unwrap_or_else(|err| {
        eprintln!("\naws3: [ERROR]: {err}");
        err.exit_code()
    })
}

fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Ls(_) => "ls",
        Command::Cp(_) => "cp",
        Command::Mv(_) => "mv",
        Command::Rm(_) => "rm",
        Command::Sync(_) => "sync",
        Command::Mb(_) => "mb",
        Command::Rb(_) => "rb",
        Command::Presign(_) => "presign",
        Command::Website(_) => "website",
    }
}
