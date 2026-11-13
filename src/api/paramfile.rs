//! The parameter files of the arguments of the operations, which are loaded
//! from `file://` and `fileb://` like `URIArgumentHandler` of the AWS CLI.
//! The structured arguments, the policies and the keys of SSE-C load them by
//! themselves, and the integers parsed by argparse, the pagination options
//! other than the starting token, the positional arguments and the global
//! options do not load them.

use std::any::TypeId;
use std::ffi::OsString;

use clap::{Arg, CommandFactory, Parser};

use super::shape;
use crate::cli::{ApiArgs, Cli, Command};
use crate::error::Error;
use crate::transfer::params::load_string;
use crate::value::Value;

/// Loads the parameter files of the arguments of the operation, and returns
/// the arguments parsed again with the contents, or `None` without them.
pub fn load() -> Result<Option<ApiArgs>, Error> {
    let args: Vec<OsString> = std::env::args_os().collect();
    let Some(args) = replace(&args)? else {
        return Ok(None);
    };
    match Cli::try_parse_from(args) {
        Ok(Cli {
            command: Command::Api(args),
            ..
        }) => Ok(Some(*args)),
        Ok(_) => Ok(None),
        Err(err) => Err(Error::param_validation(err.to_string().trim_end())),
    }
}

/// Returns the arguments whose values of the parameter files are replaced
/// with the contents, or `None` if there are no parameter files.
fn replace(args: &[OsString]) -> Result<Option<Vec<OsString>>, Error> {
    let mut command = Cli::command();
    command.build();
    let mut command = &command;
    let mut depth = 0;
    let mut replaced = Vec::with_capacity(args.len());
    let mut changed = false;
    let mut index = 0;
    while index < args.len() {
        let Some(token) = args[index].to_str() else {
            replaced.push(args[index].clone());
            index += 1;
            continue;
        };
        if index == 0 || token == "--" {
            if token == "--" {
                replaced.extend_from_slice(&args[index..]);
                break;
            }
            replaced.push(args[index].clone());
            index += 1;
            continue;
        }
        let Some(option) = token.strip_prefix("--") else {
            match command.find_subcommand(token) {
                Some(subcommand) if depth < 2 => {
                    // The operation is the subcommand of the api subcommand.
                    if depth == 0 && subcommand.get_name() != "api" {
                        return Ok(None);
                    }
                    command = subcommand;
                    depth += 1;
                }
                _ => {}
            }
            replaced.push(args[index].clone());
            index += 1;
            continue;
        };
        let (name, inline) = match option.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (option, None),
        };
        let Some(arg) = command
            .get_arguments()
            .find(|arg| arg.get_long() == Some(name))
        else {
            replaced.push(args[index].clone());
            index += 1;
            continue;
        };
        let range = arg.get_num_args().unwrap_or_default();
        if !arg.get_action().takes_values() || range.max_values() == 0 {
            replaced.push(args[index].clone());
            index += 1;
            continue;
        }
        // The values follow the option up to the maximum number of them,
        // and stop at the next option.
        let mut values: Vec<String> = Vec::new();
        let start = index;
        if let Some(value) = inline {
            values.push(value.to_owned());
        }
        index += 1;
        while values.len() < range.max_values()
            && index < args.len()
            && (values.len() < range.min_values()
                || args[index]
                    .to_str()
                    .is_some_and(|value| !value.starts_with('-')))
        {
            let Some(value) = args[index].to_str() else {
                break;
            };
            values.push(value.to_owned());
            index += 1;
        }
        let loaded = (depth == 2 && loads_paramfile(arg))
            .then(|| load_values(arg, name, &values))
            .transpose()?
            .flatten();
        match loaded {
            Some(values) => {
                changed = true;
                replaced.push(format!("--{name}").into());
                replaced.extend(values.into_iter().map(OsString::from));
            }
            None => replaced.extend_from_slice(&args[start..index]),
        }
    }
    Ok(changed.then_some(replaced))
}

/// Returns whether the parameter files of the option are loaded here, which
/// are the options of the strings, the integers and the timestamps of the
/// operations, except for those loading them by themselves.
fn loads_paramfile(arg: &Arg) -> bool {
    let structured = arg
        .get_value_names()
        .is_some_and(|names| names.iter().any(|name| name.contains('=')));
    !arg.is_global_set()
        && arg.get_value_parser().type_id() == TypeId::of::<String>()
        && !structured
        && !matches!(
            arg.get_long(),
            Some("policy" | "sse-customer-key" | "copy-source-sse-customer-key")
        )
}

/// Loads the parameter file of the values, or returns `None` if the values
/// are not of a parameter file. The single value of a list is replaced with
/// its characters like iterating over a string in Python.
fn load_values(arg: &Arg, name: &str, values: &[String]) -> Result<Option<Vec<String>>, Error> {
    let [value] = values else {
        return Ok(None);
    };
    if !value.starts_with("file://") && !value.starts_with("fileb://") {
        return Ok(None);
    }
    let option = format!("--{name}");
    let list = arg.get_num_args().unwrap_or_default().max_values() > 1;
    let value_name = arg
        .get_value_names()
        .and_then(|names| names.first())
        .map(|name| name.as_str());
    match load_string(&option, value)? {
        Value::Str(contents) if list => Ok(Some(contents.chars().map(String::from).collect())),
        Value::Str(contents) => Ok(Some(vec![contents])),
        // The integers and the timestamps are parsed from the bytes like
        // `int` of Python and the parser of dateutil.
        Value::Bytes(contents) if matches!(value_name, Some("INTEGER" | "TIMESTAMP")) => {
            Ok(Some(vec![String::from_utf8_lossy(&contents).into_owned()]))
        }
        Value::Bytes(contents) => {
            let member = member_name(name);
            let errors: Vec<_> = if list {
                contents
                    .iter()
                    .enumerate()
                    .map(|(index, byte)| {
                        format!(
                            "Invalid type for parameter {member}[{index}], value: {byte}, \
                             type: <class 'int'>, valid types: <class 'str'>"
                        )
                    })
                    .collect()
            } else {
                vec![format!(
                    "Invalid type for parameter {member}, value: {}, type: <class 'bytes'>, \
                     valid types: <class 'str'>",
                    shape::repr(&Value::Bytes(contents))
                )]
            };
            Err(Error::param_validation(format!(
                "Parameter validation failed:\n{}",
                errors.join("\n")
            )))
        }
        _ => Ok(None),
    }
}

/// Returns the name of the member of the option, e.g. `SSECustomerKeyMD5` of
/// `--sse-customer-key-md5`, in the reverse of `xform_name` of botocore.
fn member_name(option: &str) -> String {
    option
        .split('-')
        .map(|word| match word {
            "acl" | "acp" | "md5" | "mfa" | "sse" | "ssekms" | "kms" | "crc32" | "crc32c"
            | "crc64nvme" | "sha1" | "sha256" => word.to_uppercase(),
            _ => {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().chain(chars).collect())
                    .unwrap_or_default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_names() {
        for (option, member) in [
            ("key", "Key"),
            ("expected-bucket-owner", "ExpectedBucketOwner"),
            ("sse-customer-key-md5", "SSECustomerKeyMD5"),
            ("ssekms-key-id", "SSEKMSKeyId"),
            ("grant-read-acp", "GrantReadACP"),
            ("checksum-crc64nvme", "ChecksumCRC64NVME"),
            ("mfa", "MFA"),
        ] {
            assert_eq!(member_name(option), member);
        }
    }
}
