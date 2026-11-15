//! JMESPath for the `--query` option, ported from jmespath 1.0.1 of Python,
//! which the AWS CLI uses. The objects keep the order of the keys, and the
//! values are compared in the same way as Python.

mod error;
mod interpreter;
mod lexer;
mod parser;

use crate::value::Value;
pub use error::Error;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::decode as decode_json;

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
        assert_eq!(search("ceil(`1e999`)", data), Ok("Infinity".to_owned()));
        assert_eq!(
            search("floor(to_number('-inf'))", data),
            Ok("-Infinity".to_owned())
        );
        assert_eq!(
            search("floor(to_number('nan'))", data),
            Ok("NaN".to_owned())
        );
        assert_eq!(search("`1` < 'a'", data), Ok("null".to_owned()));
        assert_eq!(search("'a' < 'b'", data), Ok("true".to_owned()));
        assert_eq!(search("b[?k < `1`]", data), Ok("[]".to_owned()));
        assert_eq!(search("contains('a1', `1`)", data), Ok("false".to_owned()));
        assert_eq!(search("max_by(b[:2], &n).k", data), Ok(r#""x""#.to_owned()));
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
                "Expecting: quoted_identifier, unquoted_identifier, lbracket or lbrace, \
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
                "In function length(), invalid type for value: 1, expected \
                 string, array or object, received: \"number\"",
            ),
            (
                "sum(`[1, \"a\"]`)",
                "In function sum(), invalid type for value: a, expected \
                 array-number, received: \"string\"",
            ),
            (
                "max(`[1, \"a\"]`)",
                "In function max(), invalid type for value: a, expected \
                 array-number or array-string, received: \"string\"",
            ),
            (
                "min_by(`[{\"n\": 1}, {\"n\": \"a\"}]`, &n)",
                "In function min_by(), invalid type for value: a, expected \
                 number, received: \"string\"",
            ),
            (
                "abs(`{\"a\": [true, null, 1.5]}`)",
                "In function abs(), invalid type for value: {\"a\":[true,null,1.5]}, \
                 expected number, received: \"object\"",
            ),
            (
                "length(&a)",
                "In function length(), invalid type for value: <expref>, expected \
                 string, array or object, received: \"expref\"",
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
