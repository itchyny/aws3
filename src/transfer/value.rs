//! The values of the arguments parsed in the shorthand syntax or the JSON
//! syntax, which are not only strings but also lists and maps.

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
}
