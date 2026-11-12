//! The values of the arguments parsed in the shorthand syntax or the JSON
//! syntax, and of the responses of the api commands, which are not only
//! strings but also lists and maps.

/// The value of an argument.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Bytes(Vec<u8>),
    /// The integer, which is kept in the digits not to lose the precision.
    Int(String),
    Float(f64),
    Bool(bool),
    Null,
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

impl Value {
    /// Returns the type name of the value in the validation errors.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Str(_) => "string",
            Self::Bytes(_) => "bytes",
            Self::Int(_) => "integer",
            Self::Float(_) => "float",
            Self::Bool(_) => "boolean",
            Self::Null => "null",
            Self::List(_) => "list",
            Self::Map(_) => "map",
        }
    }

    /// Returns the value of the key of the map.
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Map(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Returns the value in the JSON syntax, where the bytes are formatted
    /// as a string.
    pub fn to_json(&self) -> String {
        let string = |value: &str| serde_json::to_string(value).unwrap_or_default();
        match self {
            Self::Str(value) => string(value),
            Self::Bytes(value) => string(&String::from_utf8_lossy(value)),
            Self::Int(value) => value.clone(),
            Self::Float(value) if value.is_nan() => "NaN".to_owned(),
            Self::Float(value) if value.is_infinite() => if *value > 0.0 {
                "Infinity"
            } else {
                "-Infinity"
            }
            .to_owned(),
            Self::Float(value) => format!("{value:?}"),
            Self::Bool(value) => value.to_string(),
            Self::Null => "null".to_owned(),
            Self::List(values) => {
                let values: Vec<_> = values.iter().map(Self::to_json).collect();
                format!("[{}]", values.join(","))
            }
            Self::Map(pairs) => {
                let pairs: Vec<_> = pairs
                    .iter()
                    .map(|(key, value)| format!("{}:{}", string(key), value.to_json()))
                    .collect();
                format!("{{{}}}", pairs.join(","))
            }
        }
    }

    /// Returns the value in the JSON syntax indented by 4 spaces, like
    /// `json.dumps` of Python with `indent=4` and `ensure_ascii=False`.
    pub fn to_json_pretty(&self) -> String {
        let mut buf = String::new();
        self.write_json_pretty(&mut buf, 0);
        buf
    }

    fn write_json_pretty(&self, buf: &mut String, depth: usize) {
        let indent = |buf: &mut String, depth: usize| {
            buf.push('\n');
            buf.push_str(&" ".repeat(4 * depth));
        };
        match self {
            Self::List(values) if !values.is_empty() => {
                buf.push('[');
                for (i, value) in values.iter().enumerate() {
                    if i > 0 {
                        buf.push(',');
                    }
                    indent(buf, depth + 1);
                    value.write_json_pretty(buf, depth + 1);
                }
                indent(buf, depth);
                buf.push(']');
            }
            Self::Map(pairs) if !pairs.is_empty() => {
                buf.push('{');
                for (i, (key, value)) in pairs.iter().enumerate() {
                    if i > 0 {
                        buf.push(',');
                    }
                    indent(buf, depth + 1);
                    buf.push_str(&serde_json::to_string(key).unwrap_or_default());
                    buf.push_str(": ");
                    value.write_json_pretty(buf, depth + 1);
                }
                indent(buf, depth);
                buf.push('}');
            }
            // The floats are formatted like `json.dumps` of Python.
            Self::Float(value) if value.is_finite() => buf.push_str(&python_float(*value)),
            _ => buf.push_str(&self.to_json()),
        }
    }
}

/// Formats the float like `repr` of Python, which uses the exponent for the
/// large or small numbers.
pub fn python_float(value: f64) -> String {
    if value.is_nan() {
        "nan".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 { "inf" } else { "-inf" }.to_owned()
    } else if value != 0.0 && !(1e-4..1e16).contains(&value.abs()) {
        let formatted = format!("{value:e}");
        let (mantissa, exponent) = formatted.split_once('e').unwrap_or((&formatted, "0"));
        let (sign, digits) = match exponent.strip_prefix('-') {
            Some(digits) => ('-', digits),
            None => ('+', exponent),
        };
        format!("{mantissa}e{sign}{digits:0>2}")
    } else {
        format!("{value:?}")
    }
}

/// Inserts the pair into the pairs of a map, where the value of the same key
/// is replaced at the position of the key.
pub fn insert(pairs: &mut Vec<(String, Value)>, key: String, value: Value) {
    match pairs.iter_mut().find(|(k, _)| *k == key) {
        Some((_, v)) => *v = value,
        None => pairs.push((key, value)),
    }
}

/// Returns whether the character is a whitespace, like `str.isspace` of
/// Python.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_json() {
        let value = Value::Map(vec![
            (
                "a".to_owned(),
                Value::List(vec![
                    Value::Str("x\"\n".to_owned()),
                    Value::Bytes(b"y".to_vec()),
                    Value::Int("1".to_owned()),
                    Value::Float(1.5),
                    Value::Float(1e16),
                    Value::Float(f64::NEG_INFINITY),
                    Value::Bool(true),
                    Value::Null,
                ]),
            ),
            ("b".to_owned(), Value::Map(Vec::new())),
        ]);
        assert_eq!(
            value.to_json(),
            r#"{"a":["x\"\n","y",1,1.5,1e16,-Infinity,true,null],"b":{}}"#
        );
    }

    #[test]
    fn python_float() {
        assert_eq!(super::python_float(1.0), "1.0");
        assert_eq!(super::python_float(2.5), "2.5");
        assert_eq!(super::python_float(1e16), "1e+16");
        assert_eq!(super::python_float(1.5e-5), "1.5e-05");
        assert_eq!(super::python_float(0.0001), "0.0001");
    }

    #[test]
    fn to_json_pretty() {
        let value = Value::Map(vec![
            ("a".to_owned(), Value::Str("日本\u{1}".to_owned())),
            (
                "b".to_owned(),
                Value::List(vec![
                    Value::Int("1".to_owned()),
                    Value::Map(vec![("c".to_owned(), Value::Bool(false))]),
                ]),
            ),
            ("d".to_owned(), Value::Map(Vec::new())),
            ("e".to_owned(), Value::List(Vec::new())),
        ]);
        assert_eq!(
            value.to_json_pretty(),
            r#"{
    "a": "日本\u0001",
    "b": [
        1,
        {
            "c": false
        }
    ],
    "d": {},
    "e": []
}"#
        );
    }
}
