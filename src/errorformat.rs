//! The structured formats of the errors for `--cli-error-format`, in the same
//! way as the formatters of the AWS CLI, which render the code and the message
//! of an error.

use std::sync::LazyLock;

use regex_lite::Regex;

use crate::table;
use crate::value::Value;

/// Formats the error in JSON, in the same way as `json.dump` with the indent
/// of four spaces.
pub fn json(code: &str, message: &str) -> String {
    let string = |value: &str| serde_json::to_string(value).unwrap_or_default();
    format!(
        "{{\n    \"Code\": {},\n    \"Message\": {}\n}}\n",
        string(code),
        string(message)
    )
}

/// Formats the error in the text format, where the values are separated by
/// tabs in the order of the keys.
pub fn text(code: &str, message: &str) -> String {
    format!("{code}\t{message}\n")
}

/// Formats the error in YAML, in the same way as the emitter of libyaml used
/// by the AWS CLI.
pub fn yaml(code: &str, message: &str) -> String {
    let mut emitter = Emitter::default();
    for (key, value) in [("Code", code), ("Message", message)] {
        emitter.write(key);
        emitter.write(":");
        emitter.whitespace = false;
        emitter.scalar(value);
        emitter.write_break();
    }
    emitter.output
}

/// The width of the lines of YAML, and the indent of the folded lines.
const BEST_WIDTH: usize = 80;
const INDENT: usize = 2;

/// The state of writing YAML, following `yaml_emitter_t` of libyaml.
#[derive(Default)]
struct Emitter {
    output: String,
    column: usize,
    whitespace: bool,
    indention: bool,
}

/// The result of analyzing a scalar, following `yaml_emitter_analyze_scalar`
/// for the block context.
struct Analysis {
    allow_block_plain: bool,
    allow_single_quoted: bool,
}

impl Emitter {
    fn write(&mut self, text: &str) {
        self.output.push_str(text);
        self.column += text.chars().count();
    }

    fn write_break(&mut self) {
        self.output.push('\n');
        self.column = 0;
        self.whitespace = true;
        self.indention = true;
    }

    fn write_indent(&mut self) {
        if !self.indention || self.column > INDENT || (self.column == INDENT && !self.whitespace) {
            self.write_break();
        }
        while self.column < INDENT {
            self.write(" ");
        }
        self.whitespace = true;
        self.indention = true;
    }

    fn scalar(&mut self, value: &str) {
        let analysis = analyze(value);
        if analysis.allow_block_plain && !is_implicit_non_string(value) && !value.is_empty() {
            self.write_plain(value);
        } else if analysis.allow_single_quoted {
            self.write_single_quoted(value);
        } else {
            self.write_double_quoted(value);
        }
    }

    fn write_plain(&mut self, value: &str) {
        if !self.whitespace {
            self.write(" ");
        }
        let chars: Vec<char> = value.chars().collect();
        let mut spaces = false;
        for (index, &ch) in chars.iter().enumerate() {
            if ch == ' ' {
                if !spaces && self.column > BEST_WIDTH && chars.get(index + 1) != Some(&' ') {
                    self.write_indent();
                } else {
                    self.write(" ");
                }
                spaces = true;
            } else {
                self.write(ch.encode_utf8(&mut [0; 4]));
                self.indention = false;
                spaces = false;
            }
        }
        self.whitespace = false;
    }

    fn write_single_quoted(&mut self, value: &str) {
        if !self.whitespace {
            self.write(" ");
        }
        self.write("'");
        let chars: Vec<char> = value.chars().collect();
        let (mut spaces, mut breaks) = (false, false);
        for (index, &ch) in chars.iter().enumerate() {
            if ch == ' ' {
                if !spaces
                    && self.column > BEST_WIDTH
                    && index != 0
                    && index != chars.len() - 1
                    && chars.get(index + 1) != Some(&' ')
                {
                    self.write_indent();
                } else {
                    self.write(" ");
                }
                spaces = true;
            } else if is_break(ch) {
                if !breaks && ch == '\n' {
                    self.write_break();
                }
                if ch == '\n' {
                    self.write_break();
                } else {
                    self.write(ch.encode_utf8(&mut [0; 4]));
                    self.column = 0;
                }
                self.indention = true;
                breaks = true;
            } else {
                if breaks {
                    self.write_indent();
                }
                if ch == '\'' {
                    self.write("'");
                }
                self.write(ch.encode_utf8(&mut [0; 4]));
                self.indention = false;
                spaces = false;
                breaks = false;
            }
        }
        if breaks {
            self.write_indent();
        }
        self.write("'");
        self.whitespace = false;
        self.indention = false;
    }

    fn write_double_quoted(&mut self, value: &str) {
        if !self.whitespace {
            self.write(" ");
        }
        self.write("\"");
        let chars: Vec<char> = value.chars().collect();
        let mut spaces = false;
        for (index, &ch) in chars.iter().enumerate() {
            if !is_printable(ch) || ch == '\u{FEFF}' || is_break(ch) || ch == '"' || ch == '\\' {
                self.write("\\");
                let escaped = match ch {
                    '\0' => "0".to_owned(),
                    '\x07' => "a".to_owned(),
                    '\x08' => "b".to_owned(),
                    '\t' => "t".to_owned(),
                    '\n' => "n".to_owned(),
                    '\x0B' => "v".to_owned(),
                    '\x0C' => "f".to_owned(),
                    '\r' => "r".to_owned(),
                    '\x1B' => "e".to_owned(),
                    '"' => "\"".to_owned(),
                    '\\' => "\\".to_owned(),
                    '\u{85}' => "N".to_owned(),
                    '\u{A0}' => "_".to_owned(),
                    '\u{2028}' => "L".to_owned(),
                    '\u{2029}' => "P".to_owned(),
                    ch if (ch as u32) <= 0xFF => format!("x{:02X}", ch as u32),
                    ch if (ch as u32) <= 0xFFFF => format!("u{:04X}", ch as u32),
                    ch => format!("U{:08X}", ch as u32),
                };
                self.write(&escaped);
                spaces = false;
            } else if ch == ' ' {
                if !spaces && self.column > BEST_WIDTH && index != 0 && index != chars.len() - 1 {
                    self.write_indent();
                    if chars.get(index + 1) == Some(&' ') {
                        self.write("\\");
                    }
                } else {
                    self.write(" ");
                }
                spaces = true;
            } else {
                self.write(ch.encode_utf8(&mut [0; 4]));
                spaces = false;
            }
        }
        self.write("\"");
        self.whitespace = false;
        self.indention = false;
    }
}

fn is_break(ch: char) -> bool {
    matches!(ch, '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn is_blank_or_end(ch: Option<char>) -> bool {
    matches!(ch, None | Some('\0' | ' ' | '\t')) || ch.is_some_and(is_break)
}

/// Returns whether the character is printable in the same way as libyaml,
/// where the next line character and the characters outside of the basic
/// multilingual plane are escaped.
fn is_printable(ch: char) -> bool {
    matches!(ch as u32, 0x0A | 0x20..=0x7E | 0xA0..=0xD7FF | 0xE000..=0xFFFD) && ch != '\u{FEFF}'
}

fn analyze(value: &str) -> Analysis {
    let chars: Vec<char> = value.chars().collect();
    let mut block_indicators = false;
    let (mut line_breaks, mut special_characters) = (false, false);
    let (mut leading_space, mut leading_break) = (false, false);
    let (mut trailing_space, mut trailing_break) = (false, false);
    let (mut break_space, mut space_break) = (false, false);
    let (mut previous_space, mut previous_break) = (false, false);
    if value.starts_with("---") || value.starts_with("...") {
        block_indicators = true;
    }
    let mut preceded_by_whitespace = true;
    for (index, &ch) in chars.iter().enumerate() {
        let followed_by_whitespace = is_blank_or_end(chars.get(index + 1).copied());
        if index == 0 {
            block_indicators |= "#,[]{}&*!|>'\"%@`".contains(ch)
                || (matches!(ch, '?' | ':' | '-') && followed_by_whitespace);
        } else {
            block_indicators |=
                (ch == ':' && followed_by_whitespace) || (ch == '#' && preceded_by_whitespace);
        }
        if !is_printable(ch) || ch == '\u{FEFF}' {
            special_characters = true;
        }
        if is_break(ch) {
            line_breaks = true;
        }
        if ch == ' ' {
            leading_space |= index == 0;
            trailing_space |= index == chars.len() - 1;
            break_space |= previous_break;
            previous_space = true;
            previous_break = false;
        } else if is_break(ch) {
            leading_break |= index == 0;
            trailing_break |= index == chars.len() - 1;
            space_break |= previous_space;
            previous_space = false;
            previous_break = true;
        } else {
            previous_space = false;
            previous_break = false;
        }
        preceded_by_whitespace = is_blank_or_end(Some(ch));
    }
    let mut allow_block_plain = true;
    let mut allow_single_quoted = true;
    if leading_space || leading_break || trailing_space || trailing_break {
        allow_block_plain = false;
    }
    if break_space {
        allow_block_plain = false;
        allow_single_quoted = false;
    }
    if space_break || special_characters {
        allow_block_plain = false;
        allow_single_quoted = false;
    }
    if line_breaks || block_indicators {
        allow_block_plain = false;
    }
    Analysis {
        allow_block_plain,
        allow_single_quoted,
    }
}

/// Returns whether the plain scalar is resolved to a type other than string by
/// the resolver of YAML 1.2 of ruamel.yaml, which needs to be quoted.
fn is_implicit_non_string(value: &str) -> bool {
    static PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
        [
            r"^(?:true|True|TRUE|false|False|FALSE)$",
            r"^(?:[-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+]?[0-9]+)?|[-+]?(?:[0-9][0-9_]*)(?:[eE][-+]?[0-9]+)|\.[0-9_]+(?:[eE][-+][0-9]+)?|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$",
            r"^(?:[-+]?0b[0-1_]+|[-+]?0o?[0-7_]+|[-+]?[0-9_]+|[-+]?0x[0-9a-fA-F_]+)$",
            r"^(?:<<)$",
            r"^(?:~|null|Null|NULL|)$",
            r"^(?:[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]|[0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?(?:[Tt]|[ \t]+)[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?)$",
            r"^(?:=)$",
        ]
        .iter()
        .map(|pattern| Regex::new(pattern).expect("valid pattern"))
        .collect()
    });
    PATTERNS.iter().any(|pattern| pattern.is_match(value))
}

/// Formats the error in the table format, in the same way as `MultiTable` of
/// the AWS CLI, which is converted to the vertical table if it is wider than
/// the terminal. The values are colored if `colored` is true.
pub fn table(code: &str, message: &str, terminal_width: usize, colored: bool) -> String {
    let error = Value::Map(vec![
        ("Code".to_owned(), Value::Str(code.to_owned())),
        ("Message".to_owned(), Value::Str(message.to_owned())),
    ]);
    match table::build("error", &error) {
        Ok(Some(table)) => table.render(terminal_width, colored),
        _ => String::new(),
    }
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
            let message = case["message"].as_str().unwrap();
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
