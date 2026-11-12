//! The lexer of JMESPath expressions, ported from `jmespath.lexer` of Python.

use super::Error;
use crate::api::shape;
use crate::transfer::decode_json;
use crate::value::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dot,
    Star,
    RBracket,
    Comma,
    Colon,
    Current,
    LParen,
    RParen,
    LBrace,
    RBrace,
    UnquotedIdentifier,
    QuotedIdentifier,
    Flatten,
    Filter,
    LBracket,
    Literal,
    Or,
    Pipe,
    And,
    Expref,
    Number,
    Lte,
    Lt,
    Gte,
    Gt,
    Ne,
    Not,
    Eq,
    Eof,
}

impl Kind {
    /// Returns the name of the type of the token in the error messages.
    pub fn name(self) -> &'static str {
        match self {
            Self::Dot => "dot",
            Self::Star => "star",
            Self::RBracket => "rbracket",
            Self::Comma => "comma",
            Self::Colon => "colon",
            Self::Current => "current",
            Self::LParen => "lparen",
            Self::RParen => "rparen",
            Self::LBrace => "lbrace",
            Self::RBrace => "rbrace",
            Self::UnquotedIdentifier => "unquoted_identifier",
            Self::QuotedIdentifier => "quoted_identifier",
            Self::Flatten => "flatten",
            Self::Filter => "filter",
            Self::LBracket => "lbracket",
            Self::Literal => "literal",
            Self::Or => "or",
            Self::Pipe => "pipe",
            Self::And => "and",
            Self::Expref => "expref",
            Self::Number => "number",
            Self::Lte => "lte",
            Self::Lt => "lt",
            Self::Gte => "gte",
            Self::Gt => "gt",
            Self::Ne => "ne",
            Self::Not => "not",
            Self::Eq => "eq",
            Self::Eof => "eof",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: Kind,
    /// The value of the token, i.e. the name of an identifier, the value of a
    /// literal, or the integer of a number.
    pub value: Value,
    /// The position of the token in characters.
    pub start: usize,
}

impl Token {
    /// Returns the value of the token like `str` of Python.
    pub fn text(&self) -> String {
        shape::str(&self.value)
    }
}

struct Lexer<'a> {
    expression: &'a str,
    chars: Vec<char>,
    position: usize,
    current: Option<char>,
}

pub fn tokenize(expression: &str) -> Result<Vec<Token>, Error> {
    if expression.is_empty() {
        return Err(Error::Empty);
    }
    let chars: Vec<char> = expression.chars().collect();
    let mut lexer = Lexer {
        expression,
        current: chars.first().copied(),
        chars,
        position: 0,
    };
    lexer.tokenize()
}

impl Lexer<'_> {
    fn tokenize(&mut self) -> Result<Vec<Token>, Error> {
        let mut tokens = Vec::new();
        while let Some(c) = self.current {
            let start = self.position;
            let simple = match c {
                '.' => Some(Kind::Dot),
                '*' => Some(Kind::Star),
                ']' => Some(Kind::RBracket),
                ',' => Some(Kind::Comma),
                ':' => Some(Kind::Colon),
                '@' => Some(Kind::Current),
                '(' => Some(Kind::LParen),
                ')' => Some(Kind::RParen),
                '{' => Some(Kind::LBrace),
                '}' => Some(Kind::RBrace),
                _ => None,
            };
            if let Some(kind) = simple {
                tokens.push(token(kind, c.to_string(), start));
                self.next();
            } else if c.is_ascii_alphabetic() || c == '_' {
                let mut buf = c.to_string();
                while let Some(c) = self
                    .next()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    buf.push(c);
                }
                tokens.push(token(Kind::UnquotedIdentifier, buf, start));
            } else if matches!(c, ' ' | '\t' | '\n' | '\r') {
                self.next();
            } else if c == '[' {
                match self.next() {
                    Some(']') => {
                        self.next();
                        tokens.push(token(Kind::Flatten, "[]".to_owned(), start));
                    }
                    Some('?') => {
                        self.next();
                        tokens.push(token(Kind::Filter, "[?".to_owned(), start));
                    }
                    _ => tokens.push(token(Kind::LBracket, "[".to_owned(), start)),
                }
            } else if c == '\'' {
                let lexeme = self.consume_until('\'')?.replace("\\'", "'");
                tokens.push(token(Kind::Literal, lexeme, start));
            } else if c == '|' {
                tokens.push(self.match_or_else('|', Kind::Or, Kind::Pipe));
            } else if c == '&' {
                tokens.push(self.match_or_else('&', Kind::And, Kind::Expref));
            } else if c == '`' {
                tokens.push(self.consume_literal()?);
            } else if c.is_ascii_digit() {
                let buf = self.consume_number();
                tokens.push(number(&buf, start));
            } else if c == '-' {
                let buf = self.consume_number();
                if buf.len() > 1 {
                    tokens.push(number(&buf, start));
                } else {
                    return Err(self.error(start, format!("Unknown token '{buf}'")));
                }
            } else if c == '"' {
                tokens.push(self.consume_quoted_identifier()?);
            } else if c == '<' {
                tokens.push(self.match_or_else('=', Kind::Lte, Kind::Lt));
            } else if c == '>' {
                tokens.push(self.match_or_else('=', Kind::Gte, Kind::Gt));
            } else if c == '!' {
                tokens.push(self.match_or_else('=', Kind::Ne, Kind::Not));
            } else if c == '=' {
                if self.next() == Some('=') {
                    tokens.push(token(Kind::Eq, "==".to_owned(), self.position - 1));
                    self.next();
                } else {
                    let position = match self.current {
                        None => self.position,
                        Some(_) => self.position - 1,
                    };
                    return Err(self.error(position, "Unknown token '='".to_owned()));
                }
            } else {
                return Err(self.error(start, format!("Unknown token {c}")));
            }
        }
        tokens.push(token(Kind::Eof, String::new(), self.chars.len()));
        Ok(tokens)
    }

    /// Advances to the next character, which stays at the last character on
    /// the end like the lexer of Python.
    fn next(&mut self) -> Option<char> {
        if self.position + 1 >= self.chars.len() {
            self.current = None;
        } else {
            self.position += 1;
            self.current = Some(self.chars[self.position]);
        }
        self.current
    }

    fn error(&self, position: usize, message: String) -> Error {
        Error::Lexer {
            position,
            message,
            expression: self.expression.to_owned(),
        }
    }

    fn consume_number(&mut self) -> String {
        let mut buf = self.current.map(String::from).unwrap_or_default();
        while let Some(c) = self.next().filter(char::is_ascii_digit) {
            buf.push(c);
        }
        buf
    }

    /// Consumes the characters until the delimiter, keeping the backslashes
    /// of the escaped characters.
    fn consume_until(&mut self, delimiter: char) -> Result<String, Error> {
        let start = self.position;
        let mut buf = String::new();
        self.next();
        while self.current != Some(delimiter) {
            if self.current == Some('\\') {
                buf.push('\\');
                self.next();
            }
            let Some(c) = self.current else {
                return Err(self.error(start, format!("Unclosed {delimiter} delimiter")));
            };
            buf.push(c);
            self.next();
        }
        self.next();
        Ok(buf)
    }

    /// Consumes the literal of JSON, which falls back to the deprecated
    /// literal of a string without the quotes.
    fn consume_literal(&mut self) -> Result<Token, Error> {
        let start = self.position;
        let lexeme = self.consume_until('`')?.replace("\\`", "`");
        let value = decode_json(&lexeme)
            .or_else(|_| decode_json(&format!("\"{}\"", lexeme.trim_start())))
            .map_err(|_| self.error(start, format!("Bad token {lexeme}")))?;
        Ok(Token {
            kind: Kind::Literal,
            value,
            start,
        })
    }

    fn consume_quoted_identifier(&mut self) -> Result<Token, Error> {
        let start = self.position;
        let lexeme = format!("\"{}\"", self.consume_until('"')?);
        let value = decode_json(&lexeme).map_err(|err| {
            let message = err.split(':').next().unwrap_or_default().to_owned();
            Error::Lexer {
                position: start,
                message,
                expression: self.expression.to_owned(),
            }
        })?;
        Ok(Token {
            kind: Kind::QuotedIdentifier,
            value,
            start,
        })
    }

    fn match_or_else(&mut self, expected: char, matched: Kind, other: Kind) -> Token {
        let start = self.position;
        let current = self.current.unwrap_or_default();
        if self.next() == Some(expected) {
            self.next();
            token(matched, format!("{current}{expected}"), start)
        } else {
            token(other, current.to_string(), start)
        }
    }
}

fn token(kind: Kind, value: String, start: usize) -> Token {
    Token {
        kind,
        value: Value::Str(value),
        start,
    }
}

fn number(digits: &str, start: usize) -> Token {
    let digits = digits.strip_prefix('+').unwrap_or(digits);
    Token {
        kind: Kind::Number,
        value: Value::Int(
            digits
                .parse::<i64>()
                .map_or_else(|_| digits.to_owned(), |n| n.to_string()),
        ),
        start,
    }
}
