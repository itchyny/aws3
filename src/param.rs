//! The parameters of the arguments, which are loaded from the parameter files
//! of `file://` and `fileb://`, and parsed in the JSON syntax or the shorthand
//! syntax like `unpack_argument` of the AWS CLI.

use std::sync::LazyLock;

use aws_sdk_s3::primitives::DateTime;
use regex_lite::{Captures, Regex};

use crate::error::Error;
use crate::json;
use crate::shorthand::{self, ParseError};
use crate::value::{Value, is_space};

/// Loads the customer-provided key, which is not base64 encoded regardless
/// of the binary format unlike the blobs of the API commands, or the content
/// of a `fileb://` or `file://` file.
pub fn load_key(name: &str, value: Option<&str>) -> Result<Option<Vec<u8>>, Error> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(path) = value
        .strip_prefix("fileb://")
        .or_else(|| value.strip_prefix("file://"))
    else {
        return Ok(Some(value.as_bytes().to_vec()));
    };
    read_paramfile(value, path)
        .map(Some)
        .map_err(|err| Error::param_validation(format!("Error parsing parameter '{name}': {err}")))
}

/// Loads the string argument from the parameter file of the value with the
/// prefix, which is returned as it is without the prefix.
pub fn load_string(name: &str, value: &str) -> Result<Value, Error> {
    let error = |err| Error::param_validation(format!("Error parsing parameter '{name}': {err}"));
    if let Some(path) = value.strip_prefix("file://") {
        read_text_paramfile(value, path)
            .map(Value::Str)
            .map_err(error)
    } else if let Some(path) = value.strip_prefix("fileb://") {
        read_paramfile(value, path).map(Value::Bytes).map_err(error)
    } else {
        Ok(Value::Str(value.to_owned()))
    }
}

/// Reads the parameter file of the value with the prefix, whose path is
/// expanded like `get_file` of the AWS CLI.
fn read_paramfile(value: &str, path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(expand_path(path))
        .map_err(|err| format!("Unable to load paramfile {value}: {err}"))
}

/// Reads the parameter file of the text, whose newlines are translated like
/// the files opened in the text mode of Python.
fn read_text_paramfile(value: &str, path: &str) -> Result<String, String> {
    let contents = String::from_utf8(read_paramfile(value, path)?).map_err(|_| {
        format!(
            "Unable to load paramfile ({}), text contents could not be decoded.  If this is a \
             binary file, please use the fileb:// prefix instead of the file:// prefix.",
            expand_path(path)
        )
    })?;
    Ok(contents.replace("\r\n", "\n").replace('\r', "\n"))
}

/// Expands the home directory and the environment variables of the path.
fn expand_path(path: &str) -> String {
    expand_vars(&expand_user(path))
}

/// Expands the home directory of `~`, like `os.path.expanduser` of Python.
fn expand_user(path: &str) -> String {
    let Some(rest) = path.strip_prefix('~') else {
        return path.to_owned();
    };
    if !(rest.is_empty() || rest.starts_with('/')) {
        return path.to_owned();
    }
    match std::env::var("HOME") {
        Ok(home) => {
            let path = format!("{}{rest}", home.trim_end_matches('/'));
            if path.is_empty() {
                "/".to_owned()
            } else {
                path
            }
        }
        Err(_) => path.to_owned(),
    }
}

/// Expands the environment variables of `$name` and `${name}`, which are left
/// unchanged if not set, like `os.path.expandvars` of Python.
fn expand_vars(path: &str) -> String {
    static VARIABLE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\$(?:(\w+)|\{([^}]*)\})").expect("valid pattern"));
    VARIABLE
        .replace_all(path, |captures: &Captures| {
            let name = captures
                .get(1)
                .or(captures.get(2))
                .map_or("", |m| m.as_str());
            std::env::var(name).unwrap_or_else(|_| captures[0].to_owned())
        })
        .into_owned()
}

/// Parses the timestamp argument, which is an ISO 8601 date time, an RFC 2822
/// date time or the epoch seconds. Date times without offsets are in UTC.
/// The surrounding whitespaces are ignored like the parser of dateutil.
pub fn parse_timestamp(value: &str) -> Result<DateTime, String> {
    let value = value.trim();
    let date_time = chrono::DateTime::parse_from_rfc3339(value)
        .or_else(|_| chrono::DateTime::parse_from_rfc2822(value))
        .map(|date_time| date_time.to_utc())
        .or_else(|_| {
            [
                "%Y-%m-%dT%H:%M:%S%.f",
                "%Y-%m-%d %H:%M:%S%.f",
                "%Y-%m-%dT%H:%M:%S%.fZ",
            ]
            .iter()
            .find_map(|format| chrono::NaiveDateTime::parse_from_str(value, format).ok())
            .or_else(|| {
                chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                    .ok()
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
            })
            .map(|date_time| date_time.and_utc())
            .ok_or(())
        })
        .or_else(|_| {
            value
                .parse::<f64>()
                .ok()
                .and_then(|secs| chrono::DateTime::from_timestamp_millis((secs * 1000.0) as i64))
                .ok_or(())
        })
        .map_err(|_| format!("Invalid timestamp value: {value}"))?;
    Ok(DateTime::from_millis(date_time.timestamp_millis()))
}

/// Parses the argument of a structure or a map in the JSON syntax or the
/// shorthand syntax, loading the parameter file of the argument like
/// `unpack_argument` of the AWS CLI. Returns the pairs of the map, and
/// whether they are parsed in the shorthand syntax, whose values are not
/// converted to the types of the members yet.
pub fn parse_map_argument(name: &str, value: &str) -> Result<(Vec<(String, Value)>, bool), Error> {
    let error = |message: String| {
        Error::param_validation(format!("Error parsing parameter '{name}': {message}"))
    };
    let contents;
    let value = if let Some(path) = value.strip_prefix("file://") {
        contents = read_text_paramfile(value, path).map_err(error)?;
        &contents
    } else if value.starts_with("fileb://") {
        return Err(error(
            "Binary file is not supported, use file:// instead".to_owned(),
        ));
    } else {
        value
    };
    let trimmed = value.trim_matches(is_space);
    if trimmed.starts_with('{') {
        match json::decode(value)
            .map_err(|err| error(format!("Invalid JSON: {err}\nJSON received: {value}")))?
        {
            Value::Map(pairs) => Ok((pairs, false)),
            _ => unreachable!("an object is decoded"),
        }
    } else if trimmed.starts_with('[') {
        Err(error(format!("Invalid JSON:\n{value}")))
    } else {
        let pairs = shorthand::parse(value, resolve_paramfile).map_err(|err| match err {
            ParseError::Syntax(message) => error(message),
            ParseError::Paramfile(message) => Error::new(message),
        })?;
        Ok((pairs, true))
    }
}

/// Resolves the parameter file of the value of the shorthand syntax, like
/// `get_paramfile` of the AWS CLI.
fn resolve_paramfile(value: String) -> Result<Value, String> {
    if let Some(path) = value.strip_prefix("file://") {
        read_text_paramfile(&value, path).map(Value::Str)
    } else if let Some(path) = value.strip_prefix("fileb://") {
        read_paramfile(&value, path).map(Value::Bytes)
    } else {
        Ok(Value::Str(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_user("~"), home.trim_end_matches('/'));
        assert_eq!(
            expand_user("~/a"),
            format!("{}/a", home.trim_end_matches('/'))
        );
        assert_eq!(expand_user("~user/a"), "~user/a");
        assert_eq!(expand_user("a/~"), "a/~");
        assert_eq!(expand_vars("$HOME/a"), format!("{home}/a"));
        assert_eq!(expand_vars("${HOME}a"), format!("{home}a"));
        assert_eq!(
            expand_vars("$ $1x_UNSET ${} ${UNSET} ${HOME"),
            "$ $1x_UNSET ${} ${UNSET} ${HOME"
        );
    }

    #[test]
    fn timestamp() {
        let expected = DateTime::from_secs(1412195400);
        assert_eq!(parse_timestamp("2014-10-01T20:30:00Z").unwrap(), expected);
        assert_eq!(parse_timestamp("2014-10-01T20:30:00").unwrap(), expected);
        assert_eq!(parse_timestamp("1412195400").unwrap(), expected);
        assert_eq!(parse_timestamp(" 1412195400\n").unwrap(), expected);
        assert!(parse_timestamp("tomorrow").is_err());
    }
}
