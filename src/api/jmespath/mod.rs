//! JMESPath for the `--query` option, ported from jmespath 1.0.1 of Python,
//! which the AWS CLI uses. The objects keep the order of the keys, and the
//! values are compared and the errors are reported in the same way as Python.

mod interpreter;
mod lexer;
mod parser;

use std::fmt;

use crate::value::Value;

/// A compiled expression.
#[derive(Debug)]
pub struct Expression(parser::Node);

impl Expression {
    pub fn compile(expression: &str) -> Result<Self, Error> {
        parser::parse(expression).map(Self)
    }

    pub fn search(&self, value: &Value) -> Result<Value, Error> {
        interpreter::search(&self.0, value)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::decode_json;

    fn search(expression: &str, data: &str) -> Result<String, String> {
        let data = decode_json(data).unwrap();
        Expression::compile(expression)
            .and_then(|expression| expression.search(&data))
            .map(|value| value.to_json())
            .map_err(|err| err.to_string())
    }

    #[test]
    fn search_values() {
        let data = r#"{"b": [{"k": "x", "n": 2}, {"k": "y", "n": 1}, {"k": "z"}], "a": {}}"#;
        assert_eq!(search("b[].k", data), Ok(r#"["x","y","z"]"#.to_owned()));
        assert_eq!(search("b[?n > `1`].k | [0]", data), Ok(r#""x""#.to_owned()));
        assert_eq!(search("b[::-2].k", data), Ok(r#"["z","x"]"#.to_owned()));
        assert_eq!(
            search("sort_by(b[:2], &n)[].k", data),
            Ok(r#"["y","x"]"#.to_owned())
        );
        assert_eq!(
            search("{z: a, y: b[0].n}", data),
            Ok(r#"{"z":{},"y":2}"#.to_owned())
        );
        assert_eq!(
            search("*", data),
            Ok(r#"[[{"k":"x","n":2},{"k":"y","n":1},{"k":"z"}],{}]"#.to_owned())
        );
        assert_eq!(search("a || 'x'", data), Ok(r#""x""#.to_owned()));
        assert_eq!(search("!`0`", data), Ok("false".to_owned()));
        assert_eq!(search("`0` == `false`", data), Ok("false".to_owned()));
        assert_eq!(search("avg(b[].n)", data), Ok("1.5".to_owned()));
        assert_eq!(
            search("to_string(`[1.5, \"é\"]`)", data),
            Ok(r#""[1.5,\"\\u00e9\"]""#.to_owned())
        );
    }

    #[test]
    fn errors() {
        for (expression, message) in [
            ("", "Invalid JMESPath expression: cannot be empty."),
            (
                "a[",
                "Invalid jmespath expression: Incomplete expression:\n\"a[\"\n   ^",
            ),
            (
                "a]",
                "Unexpected token: ]: Parse error at column 1, token \"]\" (RBRACKET), \
                 for expression:\n\"a]\"\n  ^",
            ),
            (
                "a.1",
                "Expecting: ['quoted_identifier', 'unquoted_identifier', 'lbracket', 'lbrace'], \
                 got: number: Parse error at column 2, token \"1\" (NUMBER), for expression:\n\
                 \"a.1\"\n   ^",
            ),
            (
                "a = b",
                "Bad jmespath expression: Unknown token '=':\na = b\n  ^",
            ),
            (
                "`a",
                "Bad jmespath expression: Unclosed ` delimiter:\n`a\n^",
            ),
            ("foo(@)", "Unknown function: foo()"),
            (
                "length(@, @)",
                "Expected 1 argument for function length(), received 2",
            ),
            (
                "merge()",
                "Expected at least 1 argument for function merge(), received 0",
            ),
            (
                "length(`1`)",
                "In function length(), invalid type for value: 1, expected one of: \
                 ['string', 'array', 'object'], received: \"number\"",
            ),
            (
                "sum(`[1, \"a\"]`)",
                "In function sum(), invalid type for value: a, expected one of: \
                 ['array-number'], received: \"str\"",
            ),
            (
                "`1` < 'a'",
                "'<' not supported between instances of 'int' and 'str'",
            ),
            ("@[::0]", "slice step cannot be zero"),
        ] {
            assert_eq!(
                search(expression, "[]"),
                Err(message.to_owned()),
                "{expression}"
            );
        }
    }
}
