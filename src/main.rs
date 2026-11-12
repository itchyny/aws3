mod api;
mod bucket;
mod cli;
mod client;
mod config;
mod debug;
mod east_asian_width;
mod error;
mod errorformat;
mod filter;
mod insecure;
mod ls;
mod mime;
mod path;
mod presign;
mod redirect;
mod table;
mod tls;
mod transfer;
mod value;

use std::process::ExitCode;

use cli::{Cli, Command};

fn main() -> ExitCode {
    let cli = Cli::parse_args();
    if cli.global.debug {
        debug::init();
    }
    // The file operations of the transfers run on the blocking threads, which
    // spawn up to 512 threads by default and waste CPU time and memory.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .max_blocking_threads(16)
        .build()
        .expect("failed to build the runtime");
    let code = runtime.block_on(run(&cli));
    // The blocking reads of the standard input are not waited for, which
    // remain when the transfers are cancelled by Ctrl-C.
    runtime.shutdown_background();
    code
}

async fn run(cli: &Cli) -> ExitCode {
    let result = match check_resources(&cli.command) {
        Ok(()) => run_command(cli).await,
        Err(err) => Err(err),
    };
    result.unwrap_or_else(|err| {
        let (format, colored) = error_style(&cli.global);
        eprint!("{}", err.render(format, colored));
        err.exit_code()
    })
}

/// Returns the format of the errors from the option, the environment variable
/// or the config file, and whether the tables are colored, like the AWS CLI.
fn error_style(global: &cli::GlobalArgs) -> (cli::ErrorFormat, bool) {
    use clap::ValueEnum;
    let format = global.cli_error_format.unwrap_or_else(|| {
        std::env::var("AWS_CLI_ERROR_FORMAT")
            .ok()
            .or_else(|| config::ProfileConfig::load(global.profile.as_deref()).cli_error_format)
            .and_then(|value| cli::ErrorFormat::from_str(&value, true).ok())
            .unwrap_or(cli::ErrorFormat::Enhanced)
    });
    let colored = match global.color {
        Some(cli::Color::On) => true,
        Some(cli::Color::Off) => false,
        _ => std::io::IsTerminal::is_terminal(&std::io::stdout()),
    };
    (format, colored)
}

/// Rejects the S3 paths of the resources unsupported by the s3 commands,
/// which the AWS CLI checks on parsing the paths.
fn check_resources(command: &Command) -> Result<(), error::Error> {
    let s3_paths = |paths: &[&String]| {
        paths
            .iter()
            .filter(|path| path.starts_with("s3://"))
            .try_for_each(|path| path::check_supported_resource(path))
    };
    match command {
        Command::Ls(args) => path::check_supported_resource(&args.path),
        Command::Cp(args) => s3_paths(&[&args.source, &args.destination]),
        Command::Mv(args) => s3_paths(&[&args.source, &args.destination]),
        Command::Rm(args) => path::check_supported_resource(&args.path),
        Command::Sync(args) => s3_paths(&[&args.source, &args.destination]),
        Command::Mb(args) => path::check_supported_resource(&args.path),
        Command::Rb(args) => path::check_supported_resource(&args.path),
        Command::Website(args) => path::check_supported_resource(&args.path),
        Command::Presign(args) => path::check_supported_resource(&args.path),
        Command::Api(_) | Command::Completion(_) => Ok(()),
    }
    .map_err(error::Error::param_validation)
}

async fn run_command(cli: &Cli) -> Result<ExitCode, error::Error> {
    match &cli.command {
        Command::Ls(args) => ls::run(args, &cli.global).await,
        Command::Cp(args) => transfer::cp(args, &cli.global).await,
        Command::Mv(args) => transfer::mv(args, &cli.global).await,
        Command::Rm(args) => transfer::rm(args, &cli.global).await,
        Command::Sync(args) => transfer::sync(args, &cli.global).await,
        Command::Mb(args) => bucket::mb(args, &cli.global).await,
        Command::Rb(args) => bucket::rb(args, &cli.global).await,
        Command::Website(args) => bucket::website(args, &cli.global).await,
        Command::Presign(args) => presign::run(args, &cli.global).await,
        Command::Api(args) => api::run(args, &cli.global).await,
        Command::Completion(args) => Ok(args.run()),
    }
}
