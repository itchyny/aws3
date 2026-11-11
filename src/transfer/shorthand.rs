//! The parser of the shorthand syntax of the arguments, ported from
//! `ShorthandParser` of the AWS CLI, e.g. `KeyName1=string,KeyName2=string`.

use std::sync::LazyLock;

use regex_lite::Regex;

use crate::value::{Value, insert, is_space};

/// The whitespaces of `\s` of Python, which are not ASCII-only.
const WHITESPACE: &str = r"\t\n\x0B\x0C\r\x1C-\x1F \x{85}\x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}";
const START_WORD: &str = r"!#-&(-+\--<>-Z\\-z|-\x{FFFF}";
const FIRST_FOLLOW_CHARS: &str = r"!#-&(-+\--\\\^-|~-\x{FFFF}";
const SECOND_FOLLOW_CHARS: &str = r"!#-&(-+\--<>-\x{FFFF}";

static FIRST_VALUE: LazyLock<Regex> =
    LazyLock::new(|| word_regex(&format!("{WHITESPACE}{FIRST_FOLLOW_CHARS}")));
static SECOND_VALUE: LazyLock<Regex> =
    LazyLock::new(|| word_regex(&format!("{WHITESPACE}{SECOND_FOLLOW_CHARS}")));
static SINGLE_QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^'(?:\\'|[^'])*'").expect("valid pattern"));
static DOUBLE_QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^"(?:\\"|[^"])*""#).expect("valid pattern"));

fn word_regex(follow: &str) -> Regex {
    Regex::new(&format!(r"^(?:\\,|[{START_WORD}])(?:\\,|[{follow}])*")).expect("valid pattern")
}

/// The error of parsing, which is a syntax error, or an error of loading a
/// parameter file referenced by `key@=file://path`.
#[derive(Debug, PartialEq)]
pub enum ParseError {
    Syntax(String),
    Paramfile(String),
}

/// Parses the shorthand syntax, and resolves the parameter files by the
/// function.
pub fn parse(
    value: &str,
    resolve: impl Fn(String) -> std::result::Result<Value, String>,
) -> std::result::Result<Vec<(String, Value)>, ParseError> {
    let mut parser = Parser {
        input: value,
        index: 0,
        should_resolve_paramfiles: false,
        resolve: &resolve,
    };
    parser.parameter().map_err(|err| match err {
        Error::Syntax {
            expected,
            actual,
            index,
        } => ParseError::Syntax(format!(
            "Expected: '{expected}', received: '{actual}' for input:\n {}",
            error_location(value, index)
        )),
        Error::DuplicateKey { key, index } => ParseError::Syntax(format!(
            "Second instance of key \"{key}\" encountered for input:\n{}\n\
             This is often because there is a preceding \",\" instead of a space.",
            error_location(value, index)
        )),
        Error::Paramfile(message) => ParseError::Paramfile(message),
    })
}

/// The error of parsing, where the index is in bytes.
enum Error {
    Syntax {
        expected: String,
        actual: String,
        index: usize,
    },
    DuplicateKey {
        key: String,
        index: usize,
    },
    Paramfile(String),
}

type Result<T> = std::result::Result<T, Error>;

/// Returns the input with the caret at the index on the next line, where the
/// caret is indented in characters.
fn error_location(input: &str, index: usize) -> String {
    let (before, after) = input.split_at(index.min(input.len()));
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    let (consumed, remaining) = match after.find('\n') {
        Some(newline) => input.split_at(before.len() + newline),
        None => (input, ""),
    };
    let num_spaces = before[line_start..].chars().count();
    format!("{consumed}\n{}^{remaining}", " ".repeat(num_spaces))
}

struct Parser<'a> {
    input: &'a str,
    index: usize,
    should_resolve_paramfiles: bool,
    resolve: &'a dyn Fn(String) -> std::result::Result<Value, String>,
}

impl<'a> Parser<'a> {
    fn parameter(&mut self) -> Result<Vec<(String, Value)>> {
        let mut params = Vec::new();
        let (key, value) = self.keyval()?;
        params.push((key, value));
        let mut last_index = self.index;
        while !self.at_eof() {
            self.expect(',', true)?;
            let (key, value) = self.keyval()?;
            if params.iter().any(|(k, _)| *k == key) {
                return Err(Error::DuplicateKey {
                    key,
                    index: last_index + 1,
                });
            }
            params.push((key, value));
            last_index = self.index;
        }
        Ok(params)
    }

    fn keyval(&mut self) -> Result<(String, Value)> {
        let key = self.key();
        self.should_resolve_paramfiles = self.expect('@', true).is_ok();
        self.expect('=', true)?;
        Ok((key, self.values()?))
    }

    fn key(&mut self) -> String {
        let rest = self.rest();
        let len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || "-_.#/:".contains(c)))
            .unwrap_or(rest.len());
        self.index += len;
        rest[..len].to_owned()
    }

    fn values(&mut self) -> Result<Value> {
        match self.current() {
            None => Ok(Value::Str(String::new())),
            Some('[') => self.explicit_list(),
            Some('{') => self.hash_literal(),
            Some(_) => self.csv_value(),
        }
    }

    fn csv_value(&mut self) -> Result<Value> {
        let first_value = self.first_value()?;
        self.consume_whitespace();
        if self.current() != Some(',') {
            return Ok(first_value);
        }
        self.expect(',', true)?;
        let mut csv_list = vec![first_value];
        loop {
            match self.csv_item() {
                Ok((value, last)) => {
                    csv_list.push(value);
                    if last {
                        break;
                    }
                }
                Err(Error::Syntax { .. }) if !self.at_eof() => {
                    self.backtrack_to(',');
                    break;
                }
                Err(err) => return Err(err),
            }
        }
        if csv_list.len() == 1 {
            return Ok(csv_list.remove(0));
        }
        Ok(Value::List(csv_list))
    }

    /// Returns the second value of the list, and whether it is the last.
    fn csv_item(&mut self) -> Result<(Value, bool)> {
        let value = self.second_value()?;
        self.consume_whitespace();
        if self.at_eof() {
            return Ok((value, true));
        }
        self.expect(',', true)?;
        Ok((value, false))
    }

    fn value(&mut self) -> Result<Value> {
        match self.consume_regex(&FIRST_VALUE) {
            Some(consumed) => self.processed(consumed),
            None => Ok(Value::Str(String::new())),
        }
    }

    fn explicit_list(&mut self) -> Result<Value> {
        self.expect('[', true)?;
        let mut values = Vec::new();
        while self.current() != Some(']') {
            values.push(self.explicit_values()?);
            self.consume_whitespace();
            if self.current() != Some(']') {
                self.expect(',', false)?;
                self.consume_whitespace();
            }
        }
        self.expect(']', false)?;
        Ok(Value::List(values))
    }

    fn explicit_values(&mut self) -> Result<Value> {
        match self.current() {
            Some('[') => self.explicit_list(),
            Some('{') => self.hash_literal(),
            _ => self.first_value(),
        }
    }

    fn hash_literal(&mut self) -> Result<Value> {
        self.expect('{', true)?;
        let mut keyvals = Vec::new();
        while self.current() != Some('}') {
            let key = self.key();
            self.should_resolve_paramfiles = self.expect('@', true).is_ok();
            self.expect('=', true)?;
            let value = self.explicit_values()?;
            self.consume_whitespace();
            if self.current() != Some('}') {
                self.expect(',', false)?;
                self.consume_whitespace();
            }
            insert(&mut keyvals, key, value);
        }
        self.expect('}', false)?;
        Ok(Value::Map(keyvals))
    }

    fn first_value(&mut self) -> Result<Value> {
        match self.current() {
            Some(quote @ ('\'' | '"')) => self.quoted_value(quote),
            _ => self.value(),
        }
    }

    fn second_value(&mut self) -> Result<Value> {
        match self.current() {
            Some(quote @ ('\'' | '"')) => self.quoted_value(quote),
            _ => {
                let consumed = self.must_consume_regex(&SECOND_VALUE, "second")?;
                self.processed(consumed)
            }
        }
    }

    /// Unescapes the commas and strips the trailing whitespaces of the word.
    fn processed(&self, consumed: &str) -> Result<Value> {
        let processed = consumed.replace("\\,", ",");
        self.resolve_paramfiles(processed.trim_end_matches(is_space).to_owned())
    }

    fn quoted_value(&mut self, quote: char) -> Result<Value> {
        let consumed = if quote == '\'' {
            self.must_consume_regex(&SINGLE_QUOTED, "singled quoted")?
        } else {
            self.must_consume_regex(&DOUBLE_QUOTED, "double quoted")?
        };
        let value = consumed[1..consumed.len() - 1]
            .replace(&format!("\\{quote}"), &quote.to_string())
            .replace(r"\\", r"\");
        self.resolve_paramfiles(value)
    }

    fn resolve_paramfiles(&self, value: String) -> Result<Value> {
        if self.should_resolve_paramfiles {
            (self.resolve)(value).map_err(Error::Paramfile)
        } else {
            Ok(Value::Str(value))
        }
    }

    fn consume_regex(&mut self, regex: &Regex) -> Option<&'a str> {
        let consumed = regex.find(self.rest())?.as_str();
        self.index += consumed.len();
        Some(consumed)
    }

    fn must_consume_regex(&mut self, regex: &Regex, name: &str) -> Result<&'a str> {
        self.consume_regex(regex).ok_or_else(|| Error::Syntax {
            expected: format!("<{name}>"),
            actual: "<none>".to_owned(),
            index: self.index,
        })
    }

    fn expect(&mut self, c: char, consume_whitespace: bool) -> Result<()> {
        if consume_whitespace {
            self.consume_whitespace();
        }
        match self.current() {
            Some(actual) if actual == c => {}
            actual => {
                return Err(Error::Syntax {
                    expected: c.to_string(),
                    actual: actual.map_or_else(|| "EOF".to_owned(), String::from),
                    index: self.index,
                });
            }
        }
        self.index += c.len_utf8();
        if consume_whitespace {
            self.consume_whitespace();
        }
        Ok(())
    }

    fn rest(&self) -> &'a str {
        &self.input[self.index..]
    }

    fn current(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn at_eof(&self) -> bool {
        self.index >= self.input.len()
    }

    fn backtrack_to(&mut self, c: char) {
        if self.current() != Some(c) {
            self.index = self.input[..self.index].rfind(c).unwrap_or(0);
        }
    }

    fn consume_whitespace(&mut self) {
        let rest = self.rest();
        self.index += rest.len()
            - rest
                .trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c'])
                .len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_json(value: &str) -> std::result::Result<String, ParseError> {
        let resolve = |value: String| match value.strip_prefix("file://") {
            Some("error") => Err("Unable to load paramfile file://error".to_owned()),
            Some(path) => Ok(Value::Str(format!("<{path}>"))),
            None => Ok(Value::Str(value)),
        };
        parse(value, resolve).map(|pairs| Value::Map(pairs).to_json())
    }

    #[test]
    fn values() {
        let values = [
            ("a=b", r#"{"a":"b"}"#),
            ("a=", r#"{"a":""}"#),
            ("=b", r#"{"":"b"}"#),
            ("a=b,c=d", r#"{"a":"b","c":"d"}"#),
            ("a=  b  ,  c = d ", r#"{"a":"b","c":"d"}"#),
            ("a=b c,d=e", r#"{"a":"b c","d":"e"}"#),
            ("a=b,c", r#"{"a":["b","c"]}"#),
            ("a=b, c", r#"{"a":["b","c"]}"#),
            ("a=b,c,d=e", r#"{"a":["b","c"],"d":"e"}"#),
            ("a=b,c='x,y',z", r#"{"a":"b","c":["x,y","z"]}"#),
            ("a=,b=c", r#"{"a":"","b":"c"}"#),
            (r"a=x\,y", r#"{"a":"x,y"}"#),
            (r"a=b\\,c", r#"{"a":"b\\,c"}"#),
            (r"a=b\", r#"{"a":"b\\"}"#),
            ("a='x,y'", r#"{"a":"x,y"}"#),
            (
                r#"a="x\"y\z",b='p\'q',c="\\""#,
                r#"{"a":"x\"y\\z","b":"p'q","c":"\\"}"#,
            ),
            (r"a='x\'", r#"{"a":"x\\"}"#),
            ("a=[]", r#"{"a":[]}"#),
            ("a=[x y]", r#"{"a":["x y"]}"#),
            (
                r#"a=b,c=[x,"y",'z'],d={e=[1],f={}}"#,
                r#"{"a":"b","c":["x","y","z"],"d":{"e":["1"],"f":{}}}"#,
            ),
            ("a={x=y,x=z}", r#"{"a":{"x":"z"}}"#),
            ("a=b=c{d", r#"{"a":"b=c{d"}"#),
            ("a=b\nc", r#"{"a":"b\nc"}"#),
            ("a@=file://x", r#"{"a":"<x>"}"#),
            ("a@ = file://x,b", r#"{"a":["<x>","b"]}"#),
            ("a=file://x", r#"{"a":"file://x"}"#),
            ("a@=[file://x,'file://y']", r#"{"a":["<x>","<y>"]}"#),
            (
                "a@={b=file://x,c@=file://y}",
                r#"{"a":{"b":"file://x","c":"<y>"}}"#,
            ),
            ("a=b\n,c=d", r#"{"a":"b","c":"d"}"#),
            (
                "a=b\u{3000}\u{a0}c\u{3000},d=e",
                "{\"a\":\"b\u{3000}\u{a0}c\",\"d\":\"e\"}",
            ),
        ];
        for (value, expected) in values {
            assert_eq!(parse_json(value), Ok(expected.to_owned()), "{value}");
        }
    }

    #[test]
    fn errors() {
        let errors = [
            ("a", "Expected: '=', received: 'EOF' for input:\n a\n ^"),
            ("", "Expected: '=', received: 'EOF' for input:\n \n^"),
            (" a=b", "Expected: '=', received: 'a' for input:\n  a=b\n ^"),
            (",a=b", "Expected: '=', received: ',' for input:\n ,a=b\n^"),
            (
                "a=b,",
                "Expected: '<second>', received: '<none>' for input:\n a=b,\n    ^",
            ),
            (
                "a=\"b",
                "Expected: '<double quoted>', received: '<none>' for input:\n a=\"b\n  ^",
            ),
            (
                "a=b,c='x",
                "Expected: '<singled quoted>', received: '<none>' for input:\n a=b,c='x\n      ^",
            ),
            (
                "a=\"b\"x",
                "Expected: ',', received: 'x' for input:\n a=\"b\"x\n     ^",
            ),
            (
                "a=it's",
                "Expected: ',', received: ''' for input:\n a=it's\n    ^",
            ),
            (
                "a=é,ü=x",
                "Expected: '=', received: 'ü' for input:\n a=é,ü=x\n    ^",
            ),
            (
                "a=[x,",
                "Expected: ',', received: 'EOF' for input:\n a=[x,\n     ^",
            ),
            (
                "a={x=y",
                "Expected: ',', received: 'EOF' for input:\n a={x=y\n      ^",
            ),
            (
                "a=b=c{d}",
                "Expected: ',', received: '}' for input:\n a=b=c{d}\n       ^",
            ),
            (
                "a='x'\n y",
                "Expected: ',', received: 'y' for input:\n a='x'\n y\n ^",
            ),
            (
                "a=x\u{1f600}",
                "Expected: ',', received: '\u{1f600}' for input:\n a=x\u{1f600}\n   ^",
            ),
            (
                "a=b,a=c",
                "Second instance of key \"a\" encountered for input:\na=b,a=c\n    ^\n\
                 This is often because there is a preceding \",\" instead of a space.",
            ),
        ];
        for (value, expected) in errors {
            assert_eq!(
                parse_json(value),
                Err(ParseError::Syntax(expected.to_owned())),
                "{value}"
            );
        }
        assert_eq!(
            parse_json("a=b,c@=file://error"),
            Err(ParseError::Paramfile(
                "Unable to load paramfile file://error".to_owned()
            ))
        );
    }
}
