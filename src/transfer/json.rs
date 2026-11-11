//! The decoder of JSON arguments, ported from the `json` module of Python
//! with the C scanner, which the AWS CLI uses with the errors like
//! `Expecting value: line 1 column 6 (char 5)`.

use std::sync::LazyLock;

use regex_lite::Regex;

use crate::value::{Value, insert};

static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^-?(?:0|[1-9][0-9]*)(\.[0-9]+)?([eE][-+]?[0-9]+)?").expect("valid pattern")
});

/// The error of decoding, with the message and the position in bytes.
struct DecodeError(&'static str, usize);

type Result<T> = std::result::Result<T, DecodeError>;

/// Decodes the JSON document, where the objects keep the order of the keys.
pub fn decode(value: &str) -> std::result::Result<Value, String> {
    Decoder { s: value }
        .decode()
        .map_err(|DecodeError(message, pos)| {
            // The positions are reported in characters.
            let s = &value[..pos.min(value.len())];
            let lineno = s.matches('\n').count() + 1;
            let colno = s.rsplit('\n').next().unwrap_or_default().chars().count() + 1;
            let pos = s.chars().count();
            format!("{message}: line {lineno} column {colno} (char {pos})")
        })
}

struct Decoder<'a> {
    s: &'a str,
}

impl Decoder<'_> {
    fn decode(&self) -> Result<Value> {
        let idx = self.skip_whitespace(0);
        let (value, end) = self.scan_value(idx)?;
        let end = self.skip_whitespace(end);
        if end != self.s.len() {
            return Err(DecodeError("Extra data", end));
        }
        Ok(value)
    }

    fn get(&self, idx: usize) -> Option<char> {
        self.s.get(idx..)?.chars().next()
    }

    fn skip_whitespace(&self, idx: usize) -> usize {
        let rest = &self.s[idx..];
        idx + rest.len() - rest.trim_start_matches([' ', '\t', '\n', '\r']).len()
    }

    fn starts_with(&self, idx: usize, word: &str) -> bool {
        self.s[idx..].starts_with(word)
    }

    fn scan_value(&self, idx: usize) -> Result<(Value, usize)> {
        self.scan_once(idx)?
            .ok_or(DecodeError("Expecting value", idx))
    }

    /// Scans a value at the index, or returns `None` if there is no value.
    fn scan_once(&self, idx: usize) -> Result<Option<(Value, usize)>> {
        let constants = [
            ("null", Value::Null),
            ("true", Value::Bool(true)),
            ("false", Value::Bool(false)),
            ("NaN", Value::Float(f64::NAN)),
            ("Infinity", Value::Float(f64::INFINITY)),
            ("-Infinity", Value::Float(f64::NEG_INFINITY)),
        ];
        match self.get(idx) {
            None => Ok(None),
            Some('"') => {
                let (value, end) = self.scan_string(idx + 1)?;
                Ok(Some((Value::Str(value), end)))
            }
            Some('{') => self.parse_object(idx + 1).map(Some),
            Some('[') => self.parse_array(idx + 1).map(Some),
            _ => Ok(constants
                .into_iter()
                .find(|(word, _)| self.starts_with(idx, word))
                .map(|(word, value)| (value, idx + word.len()))
                .or_else(|| self.match_number(idx))),
        }
    }

    /// Matches the number at the index, like `NUMBER_RE` of Python.
    fn match_number(&self, start: usize) -> Option<(Value, usize)> {
        let captures = NUMBER.captures(&self.s[start..])?;
        let number = &captures[0];
        let value = if captures.get(1).is_some() || captures.get(2).is_some() {
            Value::Float(number.parse().unwrap_or_default())
        } else if number == "-0" {
            Value::Int("0".to_owned())
        } else {
            Value::Int(number.to_owned())
        };
        Some((value, start + number.len()))
    }

    /// Scans the string after the quote at the index, and returns the string
    /// and the index after the end quote.
    fn scan_string(&self, mut end: usize) -> Result<(String, usize)> {
        let begin = end - 1;
        let mut value = String::new();
        loop {
            let next = end
                + self.s[end..]
                    .find(|c| c == '"' || c == '\\' || c < ' ')
                    .unwrap_or(self.s.len() - end);
            let Some(c) = self.get(next) else {
                return Err(DecodeError("Unterminated string starting at", begin));
            };
            if c != '"' && c != '\\' {
                return Err(DecodeError("Invalid control character at", next));
            }
            value.push_str(&self.s[end..next]);
            let next = next + 1;
            if c == '"' {
                return Ok((value, next));
            }
            let Some(c) = self.get(next) else {
                return Err(DecodeError("Unterminated string starting at", begin));
            };
            if c != 'u' {
                value.push(match c {
                    '"' | '\\' | '/' => c,
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    _ => return Err(DecodeError("Invalid \\escape", next - 1)),
                });
                end = next + 1;
                continue;
            }
            let mut code = self.decode_hex(next)?;
            end = next + 5;
            if (0xd800..=0xdbff).contains(&code) && self.starts_with(end, "\\u") {
                let low = self.decode_hex(end + 1)?;
                if (0xdc00..=0xdfff).contains(&low) {
                    code = 0x10000 + (((code - 0xd800) << 10) | (low - 0xdc00));
                    end += 6;
                }
            }
            value.push(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER));
        }
    }

    /// Decodes the four hexadecimal digits after the `u` at the index.
    fn decode_hex(&self, idx: usize) -> Result<u32> {
        let error = DecodeError("Invalid \\uXXXX escape", idx);
        self.s
            .get(idx + 1..idx + 5)
            .filter(|digits| digits.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .ok_or(error)
    }

    fn parse_object(&self, idx: usize) -> Result<(Value, usize)> {
        let mut pairs = Vec::new();
        let mut idx = self.skip_whitespace(idx);
        if self.get(idx) != Some('}') {
            while idx < self.s.len() {
                if self.get(idx) != Some('"') {
                    return Err(DecodeError(
                        "Expecting property name enclosed in double quotes",
                        idx,
                    ));
                }
                let (key, end) = self.scan_string(idx + 1)?;
                idx = self.skip_whitespace(end);
                if self.get(idx) != Some(':') {
                    return Err(DecodeError("Expecting ':' delimiter", idx));
                }
                idx = self.skip_whitespace(idx + 1);
                let (value, end) = self.scan_value(idx)?;
                insert(&mut pairs, key, value);
                idx = self.skip_whitespace(end);
                if self.get(idx) == Some('}') {
                    break;
                }
                if self.get(idx) != Some(',') {
                    return Err(DecodeError("Expecting ',' delimiter", idx));
                }
                let comma = idx;
                idx = self.skip_whitespace(idx + 1);
                if self.get(idx) == Some('}') {
                    return Err(DecodeError(
                        "Illegal trailing comma before end of object",
                        comma,
                    ));
                }
            }
            if self.get(idx) != Some('}') {
                return Err(DecodeError(
                    "Expecting property name enclosed in double quotes",
                    idx,
                ));
            }
        }
        Ok((Value::Map(pairs), idx + 1))
    }

    fn parse_array(&self, idx: usize) -> Result<(Value, usize)> {
        let mut values = Vec::new();
        let mut idx = self.skip_whitespace(idx);
        if self.get(idx) != Some(']') {
            while idx < self.s.len() {
                let (value, end) = self.scan_value(idx)?;
                values.push(value);
                idx = self.skip_whitespace(end);
                if self.get(idx) == Some(']') {
                    break;
                }
                if self.get(idx) != Some(',') {
                    return Err(DecodeError("Expecting ',' delimiter", idx));
                }
                let comma = idx;
                idx = self.skip_whitespace(idx + 1);
                if self.get(idx) == Some(']') {
                    return Err(DecodeError(
                        "Illegal trailing comma before end of array",
                        comma,
                    ));
                }
            }
            if self.get(idx) != Some(']') {
                return Err(DecodeError("Expecting value", idx));
            }
        }
        Ok((Value::List(values), idx + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_values() {
        let value =
            decode(r#" {"a": [1, -0, 1.5e3, true, null, NaN], "b": {}, "a": "é😀\/"} "#).unwrap();
        assert_eq!(value.to_json(), r#"{"a":"é😀/","b":{}}"#);
        let value =
            decode(r#"{"a": [1, -0, 1.5e3, true, false, null, -Infinity, "x\n"]}"#).unwrap();
        assert_eq!(
            value.to_json(),
            r#"{"a":[1,0,1500.0,true,false,null,-Infinity,"x\n"]}"#
        );
    }

    #[test]
    fn decode_errors() {
        let errors = [
            (
                "{",
                "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)",
            ),
            (
                r#"{"a""#,
                "Expecting ':' delimiter: line 1 column 5 (char 4)",
            ),
            (
                r#"{"a"  "#,
                "Expecting ':' delimiter: line 1 column 7 (char 6)",
            ),
            (r#"{"a":"#, "Expecting value: line 1 column 6 (char 5)"),
            (
                r#"{"a":"b""#,
                "Expecting ',' delimiter: line 1 column 9 (char 8)",
            ),
            (
                r#"{"a":1,  "#,
                "Expecting property name enclosed in double quotes: line 1 column 10 (char 9)",
            ),
            (
                r#"{"a":"b",}"#,
                "Illegal trailing comma before end of object: line 1 column 9 (char 8)",
            ),
            (
                r#"{"a":[1,]}"#,
                "Illegal trailing comma before end of array: line 1 column 8 (char 7)",
            ),
            (
                r#"{"a":[1 2]}"#,
                "Expecting ',' delimiter: line 1 column 9 (char 8)",
            ),
            (r#"{"a":[}"#, "Expecting value: line 1 column 7 (char 6)"),
            (r#"{"a":[1,"#, "Expecting value: line 1 column 9 (char 8)"),
            ("[", "Expecting value: line 1 column 2 (char 1)"),
            (
                r#"{"a":"\x"}"#,
                "Invalid \\escape: line 1 column 7 (char 6)",
            ),
            (
                r#"{"a":"\u12"}"#,
                "Invalid \\uXXXX escape: line 1 column 8 (char 7)",
            ),
            (
                r#"{"a":"\ud800\u12"}"#,
                "Invalid \\uXXXX escape: line 1 column 14 (char 13)",
            ),
            (
                r#"{"a":"ሴ"#,
                "Unterminated string starting at: line 1 column 6 (char 5)",
            ),
            (
                r#"{"a":"\"#,
                "Unterminated string starting at: line 1 column 6 (char 5)",
            ),
            (
                "{\"a\":\"\t\"}",
                "Invalid control character at: line 1 column 7 (char 6)",
            ),
            (r#"{"a":tru}"#, "Expecting value: line 1 column 6 (char 5)"),
            (
                r#"{"a":01}"#,
                "Expecting ',' delimiter: line 1 column 7 (char 6)",
            ),
            (
                r#"{"a":1.}"#,
                "Expecting ',' delimiter: line 1 column 7 (char 6)",
            ),
            (
                r#"{"a":1e}"#,
                "Expecting ',' delimiter: line 1 column 7 (char 6)",
            ),
            (r#"{"a":-}"#, "Expecting value: line 1 column 6 (char 5)"),
            (r#"{"a":"b"}x"#, "Extra data: line 1 column 10 (char 9)"),
            (
                "{\"a\":\"é\"\n,\n}",
                "Illegal trailing comma before end of object: line 2 column 1 (char 9)",
            ),
            ("\u{3000}{}", "Expecting value: line 1 column 1 (char 0)"),
        ];
        for (value, expected) in errors {
            assert_eq!(decode(value).unwrap_err(), expected, "{value}");
        }
    }
}
