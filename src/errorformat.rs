//! The structured formats of the errors for `--cli-error-format`, in the same
//! way as the formatters of the AWS CLI, which render the code and the message
//! of an error.

use crate::value::Value;
use crate::{table, yaml};

/// Formats the error in JSON, in the same way as `json.dump` with the indent
/// of four spaces.
pub fn json(code: &str, message: Option<&str>) -> String {
    format!("{}\n", error(code, message).to_json_pretty())
}

/// Formats the error in the text format, where the values are separated by
/// tabs in the order of the keys.
pub fn text(code: &str, message: Option<&str>) -> String {
    match message {
        Some(message) => format!("{code}\t{message}\n"),
        None => format!("{code}\n"),
    }
}

/// Formats the error in YAML, in the same way as the emitter of libyaml used
/// by the AWS CLI.
pub fn yaml(code: &str, message: Option<&str>) -> String {
    yaml::dump(&error(code, message))
}

/// Formats the error in the table format, in the same way as `MultiTable` of
/// the AWS CLI, which is converted to the vertical table if it is wider than
/// the terminal. The values are colored if `colored` is true.
pub fn table(code: &str, message: Option<&str>, terminal_width: usize, colored: bool) -> String {
    match table::build("error", &error(code, message)) {
        Ok(Some(table)) => table.render(terminal_width, colored, false),
        _ => String::new(),
    }
}

/// Returns the error with the code and the message, which is absent if the
/// error response has no message like botocore.
fn error(code: &str, message: Option<&str>) -> Value {
    let mut pairs = vec![("Code".to_owned(), Value::Str(code.to_owned()))];
    if let Some(message) = message {
        pairs.push(("Message".to_owned(), Value::Str(message.to_owned())));
    }
    Value::Map(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        // The expected outputs are of the formatters of the AWS CLI.
        let cases: serde_json::Value =
            serde_json::from_str(include_str!("../tests/fixtures/error_formats.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let code = case["code"].as_str().unwrap();
            let message = case["message"].as_str();
            assert_eq!(
                json(code, message),
                case["json"].as_str().unwrap(),
                "{message:?}"
            );
            assert_eq!(
                text(code, message),
                case["text"].as_str().unwrap(),
                "{message:?}"
            );
            assert_eq!(
                yaml(code, message),
                case["yaml"].as_str().unwrap(),
                "{message:?}"
            );
            assert_eq!(
                table(code, message, 80, false),
                case["table"].as_str().unwrap(),
                "{message:?}"
            );
        }
    }
}
