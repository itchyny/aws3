//! The errors of compiling and searching the expressions, in the same way as
//! the exceptions of jmespath.py.

use std::fmt;

use crate::value::Value;

/// Returns the text of the value in the error messages, which is the string
/// itself or the value in JSON.
pub(super) fn text(value: &Value) -> String {
    match value {
        Value::Str(value) => value.clone(),
        value => value.to_json(),
    }
}

#[derive(Debug, PartialEq)]
pub enum Error {
    Empty,
    Lexer {
        position: usize,
        message: String,
        expression: String,
    },
    Parse {
        position: usize,
        value: String,
        kind: String,
        message: String,
        expression: String,
    },
    Incomplete {
        expression: String,
    },
    /// The errors on searching, e.g. of the types of the arguments.
    Runtime(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Invalid JMESPath expression: cannot be empty."),
            Self::Lexer {
                position,
                message,
                expression,
            } => write!(
                f,
                "Bad jmespath expression: {message}:\n{expression}\n{}^",
                " ".repeat(*position)
            ),
            Self::Parse {
                position,
                value,
                kind,
                message,
                expression,
            } => write!(
                f,
                "{message}: Parse error at column {position}, token \"{value}\" ({kind}), \
                 for expression:\n\"{expression}\"\n{}^",
                " ".repeat(position + 1)
            ),
            Self::Incomplete { expression } => write!(
                f,
                "Invalid jmespath expression: Incomplete expression:\n\"{expression}\"\n{}^",
                " ".repeat(expression.chars().count() + 1)
            ),
            Self::Runtime(message) => f.write_str(message),
        }
    }
}
