//! The text output of the api command, ported from `awscli.text` of the AWS
//! CLI, which writes the scalars separated by tabs, where the nested values
//! are written on the following lines with the upper-cased keys.

use super::shape;
use crate::error::Error;
use crate::value::Value;

/// Formats the value, which fails on the lists of the maps and the other
/// values like the AWS CLI.
pub fn format(value: &Value) -> Result<String, Error> {
    let mut buf = String::new();
    format_value(value, &mut buf, None, None)?;
    Ok(buf)
}

fn format_value(
    value: &Value,
    buf: &mut String,
    identifier: Option<&str>,
    keys: Option<&[String]>,
) -> Result<(), Error> {
    match value {
        Value::Map(pairs) => format_map(pairs, buf, identifier, keys)?,
        Value::List(values) => format_list(values, buf, identifier)?,
        value => {
            buf.push_str(&shape::str(value));
            buf.push('\n');
        }
    }
    Ok(())
}

fn format_list(values: &[Value], buf: &mut String, identifier: Option<&str>) -> Result<(), Error> {
    if values.iter().any(|value| matches!(value, Value::Map(_))) {
        let keys = scalar_keys(values)?;
        for value in values {
            format_value(value, buf, identifier, Some(&keys))?;
        }
    } else if values.iter().any(|value| matches!(value, Value::List(_))) {
        let (scalars, others): (Vec<&Value>, Vec<&Value>) =
            values.iter().partition(|value| !is_container(value));
        if !scalars.is_empty() {
            format_scalars(&scalars, buf, identifier);
        }
        for value in others {
            format_value(value, buf, identifier, None)?;
        }
    } else if !values.is_empty() {
        format_scalars(&values.iter().collect::<Vec<_>>(), buf, identifier);
    }
    Ok(())
}

/// Writes the scalars on a line separated by tabs, or on each line after the
/// identifier.
fn format_scalars(values: &[&Value], buf: &mut String, identifier: Option<&str>) {
    match identifier {
        Some(identifier) => {
            for value in values {
                buf.push_str(&format!(
                    "{}\t{}\n",
                    identifier.to_uppercase(),
                    shape::str(value)
                ));
            }
        }
        None => {
            let values: Vec<_> = values.iter().map(|value| shape::str(value)).collect();
            buf.push_str(&values.join("\t"));
            buf.push('\n');
        }
    }
}

/// Writes the scalars of the map in the order of the keys, which are the
/// scalar keys of all the maps in the list, and then the nested values.
fn format_map(
    pairs: &[(String, Value)],
    buf: &mut String,
    identifier: Option<&str>,
    keys: Option<&[String]>,
) -> Result<(), Error> {
    let mut sorted: Vec<&(String, Value)> = pairs.iter().collect();
    sorted.sort_by(|(left, _), (right, _)| left.cmp(right));
    let (mut scalars, nested): (Vec<String>, Vec<(&str, &Value)>) = match keys {
        None => {
            let (scalars, nested): (Vec<_>, Vec<_>) = sorted
                .into_iter()
                .partition(|(_, value)| !is_container(value));
            (
                scalars.iter().map(|(_, value)| shape::str(value)).collect(),
                nested
                    .iter()
                    .map(|(key, value)| (key.as_str(), value))
                    .collect(),
            )
        }
        Some(keys) => (
            keys.iter()
                .map(|key| {
                    pairs
                        .iter()
                        .find(|(k, _)| k == key)
                        .map_or_else(String::new, |(_, value)| shape::str(value))
                })
                .collect(),
            sorted
                .into_iter()
                .filter(|(key, _)| !keys.contains(key))
                .map(|(key, value)| (key.as_str(), value))
                .collect(),
        ),
    };
    if !scalars.is_empty() {
        if let Some(identifier) = identifier {
            scalars.insert(0, identifier.to_uppercase());
        }
        buf.push_str(&scalars.join("\t"));
        buf.push('\n');
    }
    for (key, value) in nested {
        format_value(value, buf, Some(key), None)?;
    }
    Ok(())
}

/// Returns the sorted keys of the scalars in the maps, which fails on the
/// other values.
fn scalar_keys(values: &[Value]) -> Result<Vec<String>, Error> {
    let mut keys = Vec::new();
    for value in values {
        let Value::Map(pairs) = value else {
            return Err(Error::new(format!(
                "'{}' object has no attribute 'items'",
                shape::type_name(value)
            )));
        };
        for (key, value) in pairs {
            if !is_container(value) {
                keys.push(key.clone());
            }
        }
    }
    keys.sort();
    keys.dedup();
    Ok(keys)
}

fn is_container(value: &Value) -> bool {
    matches!(value, Value::Map(_) | Value::List(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::decode_json;

    fn text(json: &str) -> String {
        format(&decode_json(json).unwrap()).unwrap_or_else(|err| err.to_string())
    }

    #[test]
    fn format_values() {
        assert_eq!(text(r#""a""#), "a\n");
        assert_eq!(text("null"), "None\n");
        assert_eq!(text("[1, 2.5, true]"), "1\t2.5\tTrue\n");
        assert_eq!(text("[]"), "");
        assert_eq!(text("{}"), "");
        assert_eq!(
            text(
                r#"{"b": 1, "a": "x", "c": {"d": null}, "e": [{"k": "v", "n": 1}, {"k": "w", "m": [1, 2]}]}"#
            ),
            "x\t1\nC\tNone\nE\tv\t1\nE\tw\t\nM\t1\nM\t2\n"
        );
        assert_eq!(
            text(r#"{"l": [1, [2, 3], {"a": 4}]}"#),
            "'int' object has no attribute 'items'"
        );
        assert_eq!(text(r#"[[1, 2], 3, [4]]"#), "3\n1\t2\n4\n");
    }
}
