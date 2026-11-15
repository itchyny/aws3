//! The validation of the paths and the options of the transfer commands.

use std::path::Path;

use super::options::{CommandName, Options};
use crate::error::Error;
use crate::path::{is_s3express_bucket, split_bucket_key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PathsType {
    LocalS3,
    S3Local,
    S3S3,
    S3,
}

impl PathsType {
    fn usage(self) -> &'static str {
        match self {
            Self::LocalS3 => "<LocalPath> <S3Uri>",
            Self::S3S3 => "<S3Uri> <S3Uri>",
            Self::S3Local => "<S3Uri> <LocalPath>",
            Self::S3 => "<S3Uri>",
        }
    }
}

/// The validated source and destination of the command.
#[derive(Debug, Clone)]
pub(super) struct Plan {
    pub(super) src: String,
    pub(super) dest: String,
    pub(super) paths_type: PathsType,
    pub(super) is_stream: bool,
}

impl Plan {
    /// Validates the paths and the options in the same order as the AWS CLI.
    pub(super) fn new(options: &mut Options) -> Result<Self, Error> {
        let command = options.command;
        let is_s3: Vec<bool> = options
            .paths
            .iter()
            .map(|path| path.starts_with("s3://"))
            .collect();
        let paths_type = match (command, is_s3.as_slice()) {
            (CommandName::Rm, [true]) => PathsType::S3,
            (CommandName::Rm, _) => None.ok_or_else(|| invalid_argument_type(command))?,
            (_, [false, true]) => PathsType::LocalS3,
            (_, [true, false]) => PathsType::S3Local,
            (_, [true, true]) => PathsType::S3S3,
            _ => return Err(invalid_argument_type(command)),
        };
        let mut paths = options.paths.clone();
        for path in &mut paths {
            if let Some(rest) = path.strip_prefix("s3://")
                && split_bucket_key(rest).1.is_empty()
                && !path.ends_with('/')
            {
                path.push('/');
            }
        }
        let src = paths[0].clone();
        let dest = paths.get(1).unwrap_or(&paths[0]).clone();
        let mut is_stream = false;
        if src == "-" || dest == "-" {
            if command != CommandName::Cp || options.dir_op {
                return Err(Error::param_validation(
                    "Streaming currently is only compatible with non-recursive cp commands",
                ));
            }
            is_stream = true;
            options.dir_op = false;
            options.only_show_errors = true;
        }
        if is_stream && options.no_overwrite && dest == "-" {
            return Err(Error::param_validation(
                "--no-overwrite parameter is not supported for streaming downloads",
            ));
        }
        if command == CommandName::Mv && paths_type == PathsType::S3S3 && is_same_path(&src, &dest)
        {
            return Err(Error::param_validation(format!(
                "Cannot mv a file onto itself: {src} - {dest}"
            )));
        }
        if options.has_checksum_algorithm {
            check_paths_type(
                "checksum-algorithm",
                paths_type,
                &[PathsType::LocalS3, PathsType::S3S3],
            )?;
        }
        if options.has_checksum_mode {
            check_paths_type("checksum-mode", paths_type, &[PathsType::S3Local])?;
        }
        if options.has_tags {
            check_paths_type("tags", paths_type, &[PathsType::LocalS3])?;
        }
        if paths_type == PathsType::LocalS3 && !is_stream && !Path::new(&src).exists() {
            return Err(Error::new(format!(
                "The user-provided path {src} does not exist."
            )));
        }
        if paths_type == PathsType::S3Local && options.dir_op && !Path::new(&dest).exists() {
            std::fs::create_dir_all(&dest)?;
        }
        for (has_algorithm, has_key, name) in [
            (options.has_sse_c, options.has_sse_c_key, "sse-c"),
            (
                options.has_sse_c_copy_source,
                options.has_sse_c_copy_source_key,
                "sse-c-copy-source",
            ),
        ] {
            if has_algorithm && !has_key {
                return Err(Error::param_validation(format!(
                    "If --{name} is specified, --{name}-key must be specified as well."
                )));
            }
            if has_key && !has_algorithm {
                return Err(Error::param_validation(format!(
                    "If --{name}-key is specified, --{name} must be specified as well."
                )));
            }
        }
        if options.has_sse_c_copy_source && paths_type != PathsType::S3S3 {
            return Err(Error::param_validation(
                "--sse-c-copy-source is only supported for copy operations.",
            ));
        }
        let is_s3express = |path: &str| {
            path.strip_prefix("s3://")
                .is_some_and(|path| is_s3express_bucket(split_bucket_key(path).0))
        };
        if command == CommandName::Sync && (is_s3express(&src) || is_s3express(&dest)) {
            return Err(Error::param_validation(
                "Cannot use sync command with a directory bucket.",
            ));
        }
        Ok(Self {
            src,
            dest,
            paths_type,
            is_stream,
        })
    }
}

fn invalid_argument_type(command: CommandName) -> Error {
    Error::param_validation(format!(
        "usage: aws3 {} {}\nError: Invalid argument type",
        command.name(),
        command.usage()
    ))
}

fn check_paths_type(
    param: &str,
    paths_type: PathsType,
    allowed: &[PathsType],
) -> Result<(), Error> {
    if allowed.contains(&paths_type) {
        return Ok(());
    }
    let allowed: Vec<_> = allowed
        .iter()
        .map(|paths_type| paths_type.usage())
        .collect();
    Err(Error::param_validation(format!(
        "Expected {param} parameter to be used with one of following path formats: {}. Instead, received {}.",
        allowed.join(", "),
        paths_type.usage()
    )))
}

/// Returns whether moving the source to the destination is a no-op.
pub(super) fn is_same_path(src: &str, dest: &str) -> bool {
    src == dest
        || dest.ends_with('/')
            && src == format!("{dest}{}", src.rsplit('/').next().unwrap_or_default())
}
