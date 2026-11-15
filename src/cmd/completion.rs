//! The completion command, which prints the completion script of the shell.

use std::io::{self, Write};
use std::process::ExitCode;

use clap::CommandFactory;

use crate::cli::{Cli, CompletionArgs};

/// Prints the completion script, ignoring the errors of writing like the other
/// outputs of the commands.
pub fn run(args: &CompletionArgs) -> ExitCode {
    // The generators include the hidden subcommands, so the command is
    // rebuilt without them.
    let full = Cli::command();
    let mut command = clap::Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .propagate_version(true)
        .args(full.get_arguments().cloned())
        .subcommands(full.get_subcommands().filter(|c| !c.is_hide_set()).cloned());
    let mut script = Vec::new();
    clap_complete::generate(
        args.shell,
        &mut command,
        env!("CARGO_PKG_NAME"),
        &mut script,
    );
    let _ = io::stdout().write_all(&script);
    ExitCode::SUCCESS
}
