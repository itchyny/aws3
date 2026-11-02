mod cli;
mod client;
mod error;
mod ls;
mod path;

use std::process::ExitCode;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse_args();
    let result = match &cli.command {
        Command::Ls(args) => ls::run(args, &cli.global).await,
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
