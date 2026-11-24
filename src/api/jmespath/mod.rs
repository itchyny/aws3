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

    /// The data of the following tests, whose results are checked against
    /// jmespath 1.0.1 of Python except for the formats of the error messages.
    const DATA: &str = r#"{
        "a": {"b": "x", "c": [1, 2, 3]}, "foo bar": 1, "l": [[1, 2], [3, [4]], 5, null],
        "o": {"x": 1, "y": {"z": 2}}, "s": "abc", "u": "a\u00e9", "n": [3, 1, 2],
        "people": [{"name": "b", "age": 30}, {"name": "a", "age": 25}, {"name": "c"}],
        "e": [], "m": {}, "t": true, "f": false, "z": 0, "fl": 2.5
    }"#;

    #[test]
    fn search_expressions() {
        for (expression, expected) in [
            ("s[0]", "null"),
            ("s[1:]", "null"),
            ("s.*", "null"),
            ("s[?@]", "null"),
            ("s[]", "null"),
            ("l[]", "[1,2,3,[4],5]"),
            ("l[][]", "[1,2,3,4,5]"),
            ("l[1][1]", "[4]"),
            ("l[*]", "[[1,2],[3,[4]],5]"),
            ("l[*][0]", "[1,3]"),
            ("l | [*][0]", "[1,3]"),
            ("a.[b, *]", r#"["x",["x",[1,2,3]]]"#),
            ("people[?age].missing", "[]"),
            ("t && s", r#""abc""#),
            ("f && s", "false"),
            ("e && s", "[]"),
            ("m || 'x'", r#""x""#),
            ("s || 'x'", r#""abc""#),
            ("l[-1]", "null"),
            ("l[-10]", "null"),
            ("n[10]", "null"),
            ("n[1:-1]", "[1]"),
            ("n[-1:0:-1]", "[2,1]"),
            ("n[10:]", "[]"),
            ("n[:-10]", "[]"),
            ("n[-10::-1]", "[]"),
            ("n[:2][0]", "[]"),
            ("!z", "false"),
            ("!f", "true"),
            ("!e", "true"),
            ("z == f", "false"),
            ("`1` == t", "false"),
            ("`[1, true]` == `[1, true]`", "true"),
            ("o == o", "true"),
            (r#"o == `{"y": {"z": 2}, "x": 1}`"#, "true"),
            (r#"o == `{"x": 1}`"#, "false"),
            ("`null` == `null`", "true"),
            ("s != 'abc'", "false"),
            ("`1` == `1.0`", "true"),
            ("n[0] >= n[1]", "true"),
            ("n[0] <= n[1]", "false"),
            ("n[0] > n[1]", "true"),
            ("n[0] < n[1]", "false"),
            ("s >= 'abc'", "true"),
            ("`[1]` < `[2]`", "null"),
            ("fl > `2`", "true"),
            ("to_number('nan') < `1`", "false"),
            ("missing.{a: s}", "null"),
            ("missing.[s]", "null"),
            ("[s, z]", r#"["abc",0]"#),
            (
                "@",
                r#"{"a":{"b":"x","c":[1,2,3]},"foo bar":1,"l":[[1,2],[3,[4]],5,null],"o":{"x":1,"y":{"z":2}},"s":"abc","u":"aé","n":[3,1,2],"people":[{"name":"b","age":30},{"name":"a","age":25},{"name":"c"}],"e":[],"m":{},"t":true,"f":false,"z":0,"fl":2.5}"#,
            ),
            (r#""foo bar""#, "1"),
            (r#""a"."b""#, r#""x""#),
            (r#"{"k": s, j: z}"#, r#"{"k":"abc","j":0}"#),
            (r#""é""#, "null"),
            (r#""a\"b""#, "null"),
            ("`foo`", r#""foo""#),
            (r#"`\``"#, r#""`""#),
            ("`  bar`", r#""bar""#),
            ("people[*].name", r#"["b","a","c"]"#),
            ("o.*.z", "[2]"),
            ("*.z", "[]"),
            ("people | [?age > `26`].name", r#"["b"]"#),
            (
                "people[?age][]",
                r#"[{"name":"b","age":30},{"name":"a","age":25}]"#,
            ),
            ("(s)", r#""abc""#),
            ("l | [][]", "[1,2,3,4,5]"),
            ("n | [0]", "3"),
            ("n | [1:]", "[1,2]"),
            ("a.b.c", "null"),
            ("abs(`-3`)", "3"),
            ("abs(`-1.5`)", "1.5"),
            ("abs(`5`)", "5"),
            ("avg(e)", "null"),
            ("ceil(`1.2`)", "2"),
            ("floor(`-1.2`)", "-2"),
            ("ceil(`3`)", "3"),
            ("contains('abc', 'b')", "true"),
            ("contains(n, `1`)", "true"),
            ("contains(n, `true`)", "true"),
            ("contains(`[[1]]`, `[1]`)", "true"),
            (r#"contains(`[{"a": 1}]`, `{"a": 1}`)"#, "true"),
            (r#"contains(`[{"a": 1}]`, `{"a": 2}`)"#, "false"),
            ("ends_with(s, 'bc')", "true"),
            ("starts_with(s, 'ab')", "true"),
            ("starts_with(s, 'x')", "false"),
            (r#"join(', ', `["a", "b"]`)"#, r#""a, b""#),
            ("join('-', e)", r#""""#),
            ("keys(o)", r#"["x","y"]"#),
            ("values(o)", r#"[1,{"z":2}]"#),
            ("keys(m)", "[]"),
            ("length(u)", "2"),
            ("length(n)", "3"),
            ("length(o)", "2"),
            ("max(n)", "3"),
            ("min(n)", "1"),
            (r#"max(`["a", "c", "b"]`)"#, r#""c""#),
            (r#"min(`["b", "a"]`)"#, r#""a""#),
            ("max(e)", "null"),
            (
                r#"merge(o, `{"x": 3, "w": 4}`)"#,
                r#"{"x":3,"y":{"z":2},"w":4}"#,
            ),
            ("merge(m)", "{}"),
            ("not_null(missing, z, s)", "0"),
            ("not_null(missing)", "null"),
            ("reverse(s)", r#""cba""#),
            ("reverse(n)", "[2,1,3]"),
            ("reverse(u)", r#""éa""#),
            ("sort(n)", "[1,2,3]"),
            (r#"sort(`["b", "a"]`)"#, r#"["a","b"]"#),
            ("sort(e)", "[]"),
            ("sum(n)", "6"),
            ("sum(`[1, 2.5]`)", "3.5"),
            ("sum(e)", "0"),
            ("to_array(s)", r#"["abc"]"#),
            ("to_array(n)", "[3,1,2]"),
            ("to_number('12')", "12"),
            ("to_number('1_000')", "1000"),
            ("to_number('_1')", "null"),
            ("to_number('1.5e3')", "1500.0"),
            ("to_number(' 7 ')", "7"),
            ("to_number('abc')", "null"),
            ("to_number(`true`)", "null"),
            ("to_number(fl)", "2.5"),
            ("to_number('1__0')", "null"),
            ("to_number('1_.5')", "null"),
            ("to_number('+inf')", "Infinity"),
            ("to_number('0x10')", "null"),
            ("to_string(o)", r#""{\"x\":1,\"y\":{\"z\":2}}""#),
            ("to_string(s)", r#""abc""#),
            ("to_string(`1.0`)", r#""1.0""#),
            ("to_string(`true`)", r#""true""#),
            ("type(s)", r#""string""#),
            ("type(n)", r#""array""#),
            ("type(o)", r#""object""#),
            ("type(t)", r#""boolean""#),
            ("type(z)", r#""number""#),
            ("type(missing)", r#""null""#),
            ("map(&name, people)", r#"["b","a","c"]"#),
            ("map(&age, people)", "[30,25,null]"),
            ("sort_by(people[:2], &name)[].name", r#"["a","b"]"#),
            ("min_by(people[:2], &age).name", r#""a""#),
            ("max_by(e, &age)", "null"),
            ("s.*", "null"),
        ] {
            assert_eq!(
                search(expression, DATA),
                Ok(expected.to_owned()),
                "{expression}"
            );
        }
    }

    #[test]
    fn search_errors() {
        for (expression, message) in [
            (
                "length(s s)",
                "Expected 1 argument for function length(), received 2",
            ),
            (
                "sort_by(people, &age)",
                r#"In function sort_by(), invalid type for value: null, expected number, received: "null""#,
            ),
            (
                r#"sort_by(`[{"a": 1}, {"a": "x"}]`, &a)"#,
                r#"In function sort_by(), invalid type for value: x, expected number, received: "string""#,
            ),
            (
                "reverse(`1`)",
                r#"In function reverse(), invalid type for value: 1, expected array or string, received: "number""#,
            ),
            (
                "join(`1`, n)",
                r#"In function join(), invalid type for value: 1, expected string, received: "number""#,
            ),
            (
                "join(',', n)",
                r#"In function join(), invalid type for value: 3, expected array-string, received: "number""#,
            ),
            (
                r#"sort(`[1, "a"]`)"#,
                r#"In function sort(), invalid type for value: a, expected array-string or array-number, received: "string""#,
            ),
            (
                "sort(`[true]`)",
                r#"In function sort(), invalid type for value: true, expected array-string or array-number, received: "boolean""#,
            ),
            (
                "max(`[true]`)",
                r#"In function max(), invalid type for value: true, expected array-number or array-string, received: "boolean""#,
            ),
            (
                "keys(s)",
                r#"In function keys(), invalid type for value: abc, expected object, received: "string""#,
            ),
            (
                "map(n, &s)",
                r#"In function map(), invalid type for value: [3,1,2], expected expref, received: "array""#,
            ),
            (
                "starts_with(s)",
                "Expected 2 arguments for function starts_with(), received 1",
            ),
            (
                "not_null()",
                "Expected at least 1 argument for function not_null(), received 0",
            ),
            (
                "sort_by(n, s)",
                r#"In function sort_by(), invalid type for value: abc, expected expref, received: "string""#,
            ),
            (
                "ceil('a')",
                r#"In function ceil(), invalid type for value: a, expected number, received: "string""#,
            ),
            ("&s", "Encountered unrecognized type in JSON encoder."),
            (
                r#""abs"(s)"#,
                "Quoted identifier not allowed for function names.: Parse error at column 0, token \"(\" (LPAREN), for expression:\n\"\"abs\"(s)\"\n ^",
            ),
            (
                "a.length(s)",
                r#"In function length(), invalid type for value: null, expected string, array or object, received: "null""#,
            ),
            (
                "n[1:2:3:4]",
                "syntax error: Parse error at column 7, token \":\" (COLON), for expression:\n\"n[1:2:3:4]\"\n        ^",
            ),
            (
                "n[1:a]",
                "syntax error: Parse error at column 4, token \"a\" (UNQUOTED_IDENTIFIER), for expression:\n\"n[1:a]\"\n     ^",
            ),
            (
                "{a: s",
                "Invalid jmespath expression: Incomplete expression:\n\"{a: s\"\n      ^",
            ),
            (
                "{1: s}",
                "Expecting: ['quoted_identifier', 'unquoted_identifier'], got: number: Parse error at column 1, token \"1\" (NUMBER), for expression:\n\"{1: s}\"\n  ^",
            ),
            (
                "{[: s}",
                "Expecting: ['quoted_identifier', 'unquoted_identifier'], got: lbracket: Parse error at column 1, token \"[\" (LBRACKET), for expression:\n\"{[: s}\"\n  ^",
            ),
            (
                "{.: s}",
                "Expecting: ['quoted_identifier', 'unquoted_identifier'], got: dot: Parse error at column 1, token \".\" (DOT), for expression:\n\"{.: s}\"\n  ^",
            ),
            (
                "@(s)",
                "Invalid function name '@': Parse error at column 0, token \"@\" (CURRENT), for expression:\n\"@(s)\"\n ^",
            ),
            (
                "[s,",
                "Invalid jmespath expression: Incomplete expression:\n\"[s,\"\n    ^",
            ),
            (
                "[s \"x\"]",
                "Expecting: comma, got: quoted_identifier: Parse error at column 3, token \"x\" (QUOTED_IDENTIFIER), for expression:\n\"[s \"x\"]\"\n    ^",
            ),
            (
                "[s s]",
                "Expecting: comma, got: unquoted_identifier: Parse error at column 3, token \"s\" (UNQUOTED_IDENTIFIER), for expression:\n\"[s s]\"\n    ^",
            ),
            (
                "n[a]",
                "Expecting: star, got: unquoted_identifier: Parse error at column 2, token \"a\" (UNQUOTED_IDENTIFIER), for expression:\n\"n[a]\"\n   ^",
            ),
            (
                "n[*]*",
                "syntax error: Parse error at column 4, token \"*\" (STAR), for expression:\n\"n[*]*\"\n     ^",
            ),
            (
                "n[*]{a: s}",
                "syntax error: Parse error at column 4, token \"{\" (LBRACE), for expression:\n\"n[*]{a: s}\"\n     ^",
            ),
            (
                "n[*](",
                "syntax error: Parse error at column 4, token \"(\" (LPAREN), for expression:\n\"n[*](\"\n     ^",
            ),
            ("-", "Bad jmespath expression: Unknown token '-':\n-\n^"),
            (
                "-1",
                "invalid token: Parse error at column 0, token \"-1\" (NUMBER), for expression:\n\"-1\"\n ^",
            ),
            (
                "a =",
                "Bad jmespath expression: Unknown token '=':\na =\n  ^",
            ),
            ("#", "Bad jmespath expression: Unknown token #:\n#\n^"),
            (
                r#"`"a`"#,
                "Bad jmespath expression: Bad token \"a:\n`\"a`\n^",
            ),
            (
                r#""a"#,
                "Bad jmespath expression: Unclosed \" delimiter:\n\"a\n^",
            ),
            (
                r#""\x""#,
                "Bad jmespath expression: Invalid \\escape:\n\"\\x\"\n^",
            ),
            (
                "s[0",
                "Invalid jmespath expression: Incomplete expression:\n\"s[0\"\n    ^",
            ),
            (
                "(s",
                "Invalid jmespath expression: Incomplete expression:\n\"(s\"\n   ^",
            ),
            (
                "s *",
                "invalid token: Parse error at column 2, token \"*\" (STAR), for expression:\n\"s *\"\n   ^",
            ),
            (
                "s {",
                "invalid token: Parse error at column 2, token \"{\" (LBRACE), for expression:\n\"s {\"\n   ^",
            ),
            (
                "s !",
                "invalid token: Parse error at column 2, token \"!\" (NOT), for expression:\n\"s !\"\n   ^",
            ),
            (
                "s ]",
                "Unexpected token: ]: Parse error at column 2, token \"]\" (RBRACKET), for expression:\n\"s ]\"\n   ^",
            ),
            (
                "s )",
                "Unexpected token: ): Parse error at column 2, token \")\" (RPAREN), for expression:\n\"s )\"\n   ^",
            ),
            (
                "s ,",
                "Unexpected token: ,: Parse error at column 2, token \",\" (COMMA), for expression:\n\"s ,\"\n   ^",
            ),
            (
                "s :",
                "Unexpected token: :: Parse error at column 2, token \":\" (COLON), for expression:\n\"s :\"\n   ^",
            ),
        ] {
            assert_eq!(
                search(expression, DATA),
                Err(message.to_owned()),
                "{expression}"
            );
        }
    }
}
