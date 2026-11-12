//! The YAML output, in the same way as the emitter of libyaml used through
//! ruamel.yaml by the AWS CLI, which writes the collections in the block style
//! with the keys of the mappings sorted.

use std::sync::LazyLock;

use regex_lite::Regex;

use crate::value::{Value, python_float};

/// Formats the value in YAML, where the empty collections are written in the
/// flow style.
pub fn dump(value: &Value) -> String {
    let mut emitter = Emitter {
        whitespace: true,
        indention: true,
        ..Emitter::default()
    };
    emitter.node(value, false);
    emitter.write_indent();
    emitter.output
}

/// The width of the lines of YAML, and the indent of the nested nodes.
const BEST_WIDTH: usize = 80;
const BEST_INDENT: usize = 2;

/// The maximum length of the simple keys in bytes, and the longer keys are
/// written after `?`.
const MAX_SIMPLE_KEY_LENGTH: usize = 128;

/// The state of writing YAML, following `yaml_emitter_t` of libyaml.
#[derive(Default)]
struct Emitter {
    output: String,
    column: usize,
    /// The current indent, which is `None` at the top level.
    indent: Option<usize>,
    indents: Vec<Option<usize>>,
    whitespace: bool,
    indention: bool,
}

/// The result of analyzing a scalar, following `yaml_emitter_analyze_scalar`
/// for the block context.
struct Analysis {
    multiline: bool,
    allow_block_plain: bool,
    allow_single_quoted: bool,
}

impl Emitter {
    fn write(&mut self, text: &str) {
        self.output.push_str(text);
        self.column += text.chars().count();
    }

    fn write_char(&mut self, ch: char) {
        self.write(ch.encode_utf8(&mut [0; 4]));
    }

    fn write_break(&mut self) {
        self.output.push('\n');
        self.column = 0;
    }

    fn write_indent(&mut self) {
        let indent = self.indent.unwrap_or(0);
        if !self.indention || self.column > indent || (self.column == indent && !self.whitespace) {
            self.write_break();
        }
        while self.column < indent {
            self.write(" ");
        }
        self.whitespace = true;
        self.indention = true;
    }

    fn write_indicator(
        &mut self,
        indicator: &str,
        need_whitespace: bool,
        is_whitespace: bool,
        is_indention: bool,
    ) {
        if need_whitespace && !self.whitespace {
            self.write(" ");
        }
        self.write(indicator);
        self.whitespace = is_whitespace;
        self.indention = self.indention && is_indention;
    }

    fn increase_indent(&mut self, flow: bool, indentless: bool) {
        self.indents.push(self.indent);
        self.indent = Some(match self.indent {
            None if flow => BEST_INDENT,
            None => 0,
            Some(indent) if indentless => indent,
            Some(indent) => indent + BEST_INDENT,
        });
    }

    fn decrease_indent(&mut self) {
        self.indent = self.indents.pop().flatten();
    }

    /// Writes the node, where `mapping` is whether the node is a value of a
    /// mapping, whose sequence is not indented.
    fn node(&mut self, value: &Value, mapping: bool) {
        match value {
            Value::List(values) if values.is_empty() => {
                self.write_indicator("[", true, true, false);
                self.write_indicator("]", false, false, false);
            }
            Value::Map(pairs) if pairs.is_empty() => {
                self.write_indicator("{", true, true, false);
                self.write_indicator("}", false, false, false);
            }
            Value::List(values) => self.sequence(values, mapping),
            Value::Map(pairs) => self.mapping(pairs),
            Value::Str(value) => self.scalar(value, true, false),
            Value::Bytes(value) => self.scalar(&String::from_utf8_lossy(value), true, false),
            Value::Int(value) => self.scalar(value, false, false),
            Value::Float(value) => self.scalar(&float(*value), false, false),
            Value::Bool(value) => self.scalar(if *value { "true" } else { "false" }, false, false),
            Value::Null => self.scalar("null", false, false),
        }
    }

    fn sequence(&mut self, values: &[Value], mapping: bool) {
        self.increase_indent(false, mapping && !self.indention);
        for value in values {
            self.write_indent();
            self.write_indicator("-", true, false, true);
            self.node(value, false);
        }
        self.decrease_indent();
    }

    fn mapping(&mut self, pairs: &[(String, Value)]) {
        self.increase_indent(false, false);
        let mut pairs: Vec<&(String, Value)> = pairs.iter().collect();
        pairs.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (key, value) in pairs {
            self.write_indent();
            if !analyze(key).multiline && key.len() <= MAX_SIMPLE_KEY_LENGTH {
                self.scalar(key, true, true);
                self.write_indicator(":", false, false, false);
            } else {
                self.write_indicator("?", true, false, true);
                self.scalar(key, true, false);
                self.write_indent();
                self.write_indicator(":", true, false, true);
            }
            self.node(value, true);
        }
        self.decrease_indent();
    }

    /// Writes the scalar in the plain style if possible, where the strings
    /// resolved to the other types are quoted. The long lines are folded
    /// unless the scalar is a simple key.
    fn scalar(&mut self, value: &str, string: bool, simple_key: bool) {
        self.increase_indent(true, false);
        let allow_breaks = !simple_key;
        if !string {
            self.write_plain(value, allow_breaks);
        } else {
            let analysis = analyze(value);
            if analysis.allow_block_plain && !is_implicit_non_string(value) {
                self.write_plain(value, allow_breaks);
            } else if analysis.allow_single_quoted {
                self.write_single_quoted(value, allow_breaks);
            } else {
                self.write_double_quoted(value, allow_breaks);
            }
        }
        self.decrease_indent();
    }

    fn write_plain(&mut self, value: &str, allow_breaks: bool) {
        if !self.whitespace {
            self.write(" ");
        }
        let chars: Vec<char> = value.chars().collect();
        let mut spaces = false;
        for (index, &ch) in chars.iter().enumerate() {
            if ch == ' ' {
                if allow_breaks
                    && !spaces
                    && self.column > BEST_WIDTH
                    && chars.get(index + 1) != Some(&' ')
                {
                    self.write_indent();
                } else {
                    self.write(" ");
                }
                spaces = true;
            } else {
                self.write_char(ch);
                self.indention = false;
                spaces = false;
            }
        }
        self.whitespace = false;
        self.indention = false;
    }

    fn write_single_quoted(&mut self, value: &str, allow_breaks: bool) {
        self.write_indicator("'", true, false, false);
        let chars: Vec<char> = value.chars().collect();
        let (mut spaces, mut breaks) = (false, false);
        for (index, &ch) in chars.iter().enumerate() {
            if ch == ' ' {
                if allow_breaks
                    && !spaces
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
                    self.write_char(ch);
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
                self.write_char(ch);
                self.indention = false;
                spaces = false;
                breaks = false;
            }
        }
        if breaks {
            self.write_indent();
        }
        self.write_indicator("'", false, false, false);
        self.whitespace = false;
        self.indention = false;
    }

    fn write_double_quoted(&mut self, value: &str, allow_breaks: bool) {
        self.write_indicator("\"", true, false, false);
        let chars: Vec<char> = value.chars().collect();
        let mut spaces = false;
        for (index, &ch) in chars.iter().enumerate() {
            if !is_printable(ch) || is_break(ch) || ch == '"' || ch == '\\' {
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
                    '\u{2028}' => "L".to_owned(),
                    '\u{2029}' => "P".to_owned(),
                    ch if (ch as u32) <= 0xFF => format!("x{:02X}", ch as u32),
                    ch if (ch as u32) <= 0xFFFF => format!("u{:04X}", ch as u32),
                    ch => format!("U{:08X}", ch as u32),
                };
                self.write(&escaped);
                spaces = false;
            } else if ch == ' ' {
                if allow_breaks
                    && !spaces
                    && self.column > BEST_WIDTH
                    && index != 0
                    && index != chars.len() - 1
                {
                    self.write_indent();
                    if chars.get(index + 1) == Some(&' ') {
                        self.write("\\");
                    }
                } else {
                    self.write(" ");
                }
                spaces = true;
            } else {
                self.write_char(ch);
                spaces = false;
            }
        }
        self.write_indicator("\"", false, false, false);
        self.whitespace = false;
        self.indention = false;
    }
}

/// Formats the float like the representer of ruamel.yaml.
fn float(value: f64) -> String {
    if value.is_nan() {
        ".nan".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 { ".inf" } else { "-.inf" }.to_owned()
    } else {
        python_float(value).to_lowercase()
    }
}

fn is_break(ch: char) -> bool {
    matches!(ch, '\r' | '\n' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn is_blank_or_end(ch: Option<char>) -> bool {
    matches!(ch, None | Some('\0' | ' ' | '\t')) || ch.is_some_and(is_break)
}

/// Returns whether the character is printable in the same way as libyaml,
/// where the next line character, the byte order mark and the characters
/// outside of the basic multilingual plane are escaped.
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
        if !is_printable(ch) {
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
        multiline: line_breaks,
        allow_block_plain,
        allow_single_quoted,
    }
}

/// Returns whether the plain scalar is resolved to a type other than string by
/// the resolver of YAML 1.2 of ruamel.yaml, which needs to be quoted. The
/// empty string is resolved to null. The resolvers are looked up by the first
/// character, where no resolver starts with an underscore.
fn is_implicit_non_string(value: &str) -> bool {
    static PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
        [
            r"^(?:true|True|TRUE|false|False|FALSE)$",
            r"^(?:[-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+]?[0-9]+)?|[-+]?(?:[0-9][0-9_]*)(?:[eE][-+]?[0-9]+)|[-+]?\.[0-9_]+(?:[eE][-+][0-9]+)?|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN))$",
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
    !value.starts_with('_') && PATTERNS.iter().any(|pattern| pattern.is_match(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::decode_json;

    fn yaml(json: &str) -> String {
        dump(&decode_json(json).unwrap())
    }

    #[test]
    fn dump_values() {
        // The expected outputs are of the YAML dumper of the AWS CLI.
        assert_eq!(yaml("[]"), "[]\n");
        assert_eq!(yaml("[[], {}]"), "- []\n- {}\n");
        assert_eq!(
            yaml(
                r#"{"b": 1, "a": [{"x": "y", "k": [1, 2]}, [3, []], {}], "c": {}, "d": null, "e": true, "f": 1.5, "g": 1e20, "h": 1e-7}"#
            ),
            "a:\n- k:\n  - 1\n  - 2\n  x: y\n- - 3\n  - []\n- {}\nb: 1\nc: {}\nd: null\ne: true\nf: 1.5\ng: 1e+20\nh: 1e-07\n"
        );
        assert_eq!(
            yaml(&format!(
                r#"{{"m": {{"n": {{"o": ["p", {{"q": "{}"}}]}}}}}}"#,
                "w ".repeat(50)
            )),
            "m:\n  n:\n    o:\n    - p\n    - q: 'w w w w w w w w w w w w w w w w w w w w w w w w w w w w w w w w w w w w\n        w w w w w w w w w w w w w w '\n"
        );
    }

    #[test]
    fn dump_scalars() {
        for (value, expected) in [
            ("yes", "yes"),
            ("true", "'true'"),
            ("null", "'null'"),
            ("1_000", "'1_000'"),
            ("_1", "_1"),
            ("2024-01-01T00:00:00Z", "'2024-01-01T00:00:00Z'"),
            ("1:20", "1:20"),
            ("", "''"),
            ("a: b", "'a: b'"),
            ("a:b", "a:b"),
            ("?x", "?x"),
            ("x:", "'x:'"),
            ("'q'", "'''q'''"),
            ("\"etag\"", "'\"etag\"'"),
            (" x", "' x'"),
            ("a\n\nb\n", "'a\n\n\n  b\n\n  '"),
            ("tab\tx", r#""tab\tx""#),
            ("\u{85}", r#""\N""#),
            ("\u{feff}", r#""\uFEFF""#),
            ("\u{1f389}", r#""\U0001F389""#),
            ("x\u{7f}", r#""x\x7F""#),
            ("\u{a0}x", "\u{a0}x"),
            ("日本語", "日本語"),
        ] {
            let map = Value::Map(vec![("k".to_owned(), Value::Str(value.to_owned()))]);
            assert_eq!(dump(&map), format!("k: {expected}\n"), "{value:?}");
        }
    }

    #[test]
    fn dump_keys() {
        let long = "k".repeat(130);
        assert_eq!(
            yaml(&format!(
                r#"{{"{long}": "v", "a\nb": "c", "": "", "l": {{"{long}": [1]}}}}"#
            )),
            format!("'': ''\n? 'a\n\n  b'\n: c\n? {long}\n: v\nl:\n  ? {long}\n  : - 1\n")
        );
    }
}
