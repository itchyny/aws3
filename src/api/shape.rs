//! The shapes of the structured arguments, which are converted from the
//! shorthand syntax and validated in the same way as the AWS CLI and botocore.

use crate::error::Error;
use crate::transfer::params;
use crate::value::Value;

/// The shape of a member, a subset of the types of the models of botocore.
#[derive(Debug)]
pub enum Shape {
    String { min: usize },
    Boolean,
    Long,
    Timestamp,
    List(&'static Shape),
    Structure(&'static Structure),
}

#[derive(Debug)]
pub struct Structure {
    pub members: &'static [(&'static str, Shape)],
    pub required: &'static [&'static str],
}

/// Parses the argument of the structure of the member in the JSON syntax or
/// the shorthand syntax, and validates it like `validate_parameters` of
/// botocore.
pub fn parse_structure(
    option: &str,
    member: &str,
    value: &str,
    structure: &'static Structure,
) -> Result<Value, Error> {
    let (pairs, shorthand) = params::parse_map_argument(option, value)?;
    let shape = Shape::Structure(structure);
    let mut value = Value::Map(pairs);
    if shorthand {
        convert(&mut value, &shape)?;
    }
    let mut errors = Vec::new();
    validate(&value, &shape, member, &mut errors);
    if !errors.is_empty() {
        return Err(Error::param_validation(format!(
            "Parameter validation failed:\n{}",
            errors.join("\n")
        )));
    }
    Ok(value)
}

/// Converts the strings of the shorthand syntax to the types of the members,
/// like `BackCompatVisitor` of the AWS CLI.
fn convert(value: &mut Value, shape: &Shape) -> Result<(), Error> {
    match (shape, value) {
        (Shape::Structure(structure), Value::Map(pairs)) => {
            for (name, member) in structure.members {
                if let Some((_, value)) = pairs.iter_mut().find(|(key, _)| key == name) {
                    convert(value, member)?;
                }
            }
        }
        (Shape::List(member), Value::List(values)) => {
            for value in values {
                convert(value, member)?;
            }
        }
        // The single values are wrapped in the lists, which are not
        // converted to the types of the members.
        (Shape::List(_), value) => {
            *value = Value::List(vec![std::mem::replace(value, Value::Null)])
        }
        (Shape::Long, value @ Value::Str(_)) => {
            let Value::Str(string) = &*value else {
                unreachable!()
            };
            *value = Value::Int(python_int(string)?.to_string());
        }
        (Shape::Boolean, value @ Value::Str(_)) => {
            let Value::Str(string) = &*value else {
                unreachable!()
            };
            match string.to_lowercase().as_str() {
                "true" => *value = Value::Bool(true),
                "false" => *value = Value::Bool(false),
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

/// Parses the integer like `int` of Python, failing with the same message.
pub fn python_int(value: &str) -> Result<i64, Error> {
    value.trim().parse().map_err(|_| {
        Error::new(format!(
            "invalid literal for int() with base 10: {}",
            repr(&Value::Str(value.to_owned()))
        ))
    })
}

/// Validates the value of the shape, collecting the errors with the same
/// messages as botocore.
fn validate(value: &Value, shape: &Shape, name: &str, errors: &mut Vec<String>) {
    let mut invalid_type = |valid_types: &str| {
        errors.push(format!(
            "Invalid type for parameter {name}, value: {}, type: <class '{}'>, valid types: {valid_types}",
            str(value),
            type_name(value)
        ));
    };
    match (shape, value) {
        (Shape::String { min }, Value::Str(string)) => {
            let length = string.chars().count();
            if length < *min {
                errors.push(format!(
                    "Invalid length for parameter {name}, value: {length}, valid min length: {min}"
                ));
            }
        }
        (Shape::String { .. }, _) => invalid_type("<class 'str'>"),
        (Shape::Boolean, Value::Bool(_)) | (Shape::Long, Value::Int(_)) => {}
        (Shape::Boolean, _) => invalid_type("<class 'bool'>"),
        (Shape::Long, _) => invalid_type("<class 'int'>"),
        (Shape::Timestamp, Value::Int(_) | Value::Float(_)) => {}
        (Shape::Timestamp, Value::Str(string)) if params::parse_timestamp(string).is_ok() => {}
        (Shape::Timestamp, _) => invalid_type("<class 'datetime.datetime'>, timestamp-string"),
        (Shape::List(member), Value::List(values)) => {
            for (i, value) in values.iter().enumerate() {
                validate(value, member, &format!("{name}[{i}]"), errors);
            }
        }
        (Shape::List(_), _) => invalid_type("<class 'list'>, <class 'tuple'>"),
        (Shape::Structure(structure), Value::Map(pairs)) => {
            for required in structure.required {
                if !pairs.iter().any(|(key, _)| key == required) {
                    errors.push(format!(
                        "Missing required parameter in {name}: \"{required}\""
                    ));
                }
            }
            let mut known = Vec::new();
            for (key, value) in pairs {
                match structure.members.iter().find(|(member, _)| member == key) {
                    Some((_, shape)) => known.push((key, value, shape)),
                    None => {
                        let names: Vec<_> =
                            structure.members.iter().map(|(name, _)| *name).collect();
                        errors.push(format!(
                            "Unknown parameter in {name}: \"{key}\", must be one of: {}",
                            names.join(", ")
                        ));
                    }
                }
            }
            for (key, value, shape) in known {
                validate(value, shape, &format!("{name}.{key}"), errors);
            }
        }
        (Shape::Structure(_), _) => invalid_type("<class 'dict'>"),
    }
}

/// Returns the name of the type of the value in Python.
pub fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Str(_) => "str",
        Value::Bytes(_) => "bytes",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Bool(_) => "bool",
        Value::Null => "NoneType",
        Value::List(_) => "list",
        Value::Map(_) => "dict",
    }
}

/// Returns the value like `str` of Python, which is the same as `repr`
/// except for the strings.
pub fn str(value: &Value) -> String {
    match value {
        Value::Str(string) => string.clone(),
        value => repr(value),
    }
}

/// Returns the value like `repr` of Python.
fn repr(value: &Value) -> String {
    match value {
        Value::Str(string) => repr_string(string, ""),
        Value::Bytes(bytes) => repr_string(&String::from_utf8_lossy(bytes), "b"),
        Value::Int(digits) => digits.clone(),
        Value::Float(value) => repr_float(*value),
        Value::Bool(true) => "True".to_owned(),
        Value::Bool(false) => "False".to_owned(),
        Value::Null => "None".to_owned(),
        Value::List(values) => {
            let values: Vec<_> = values.iter().map(repr).collect();
            format!("[{}]", values.join(", "))
        }
        Value::Map(pairs) => {
            let pairs: Vec<_> = pairs
                .iter()
                .map(|(key, value)| format!("{}: {}", repr_string(key, ""), repr(value)))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
    }
}

/// Quotes the string like `repr` of Python, which prefers single quotes.
fn repr_string(string: &str, prefix: &str) -> String {
    let quote = if string.contains('\'') && !string.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut buf = format!("{prefix}{quote}");
    for c in string.chars() {
        match c {
            '\\' => buf.push_str("\\\\"),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            c if c == quote => {
                buf.push('\\');
                buf.push(c);
            }
            c if c.is_control() => buf.push_str(&format!("\\x{:02x}", c as u32)),
            c => buf.push(c),
        }
    }
    buf.push(quote);
    buf
}

/// Formats the float like `repr` of Python, which uses the exponent for the
/// large or small numbers.
fn repr_float(value: f64) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    static OBJECT: Structure = Structure {
        members: &[
            ("Key", Shape::String { min: 1 }),
            ("Size", Shape::Long),
            ("Time", Shape::Timestamp),
        ],
        required: &["Key"],
    };

    static DELETE: Structure = Structure {
        members: &[
            ("Objects", Shape::List(&Shape::Structure(&OBJECT))),
            ("Quiet", Shape::Boolean),
        ],
        required: &["Objects"],
    };

    fn parse(value: &str) -> Result<String, String> {
        parse_structure("--delete", "Delete", value, &DELETE)
            .map(|value| value.to_json())
            .map_err(|err| err.to_string())
    }

    #[test]
    fn parse_shorthand() {
        assert_eq!(
            parse("Objects=[{Key=a,Size=1,Time=2030-01-01},{Key=b}],Quiet=TRUE"),
            Ok(r#"{"Objects":[{"Key":"a","Size":1,"Time":"2030-01-01"},{"Key":"b"}],"Quiet":true}"#.to_owned())
        );
        assert_eq!(
            parse("Objects={Key=a}"),
            Ok(r#"{"Objects":[{"Key":"a"}]}"#.to_owned())
        );
        assert_eq!(
            parse("Objects=[{Key=a,Size=x}]"),
            Err("invalid literal for int() with base 10: 'x'".to_owned())
        );
        assert_eq!(
            parse("Objects=a,Quiet=x"),
            Err("An error occurred (ParamValidation): Parameter validation failed:\n\
                 Invalid type for parameter Delete.Objects[0], value: a, type: <class 'str'>, valid types: <class 'dict'>\n\
                 Invalid type for parameter Delete.Quiet, value: x, type: <class 'str'>, valid types: <class 'bool'>"
                .to_owned())
        );
    }

    #[test]
    fn parse_json() {
        assert_eq!(
            parse(r#"{"Objects":[{"Key":"a","Size":1,"Time":1893456000}]}"#),
            Ok(r#"{"Objects":[{"Key":"a","Size":1,"Time":1893456000}]}"#.to_owned())
        );
        assert_eq!(
            parse(r#"{"Objects":[{"Key":"","Size":"1","Time":"x","Foo":null}],"Bar":{"x":[1,2.5,1e20,true,"'"]}}"#),
            Err("An error occurred (ParamValidation): Parameter validation failed:\n\
                 Unknown parameter in Delete: \"Bar\", must be one of: Objects, Quiet\n\
                 Unknown parameter in Delete.Objects[0]: \"Foo\", must be one of: Key, Size, Time\n\
                 Invalid length for parameter Delete.Objects[0].Key, value: 0, valid min length: 1\n\
                 Invalid type for parameter Delete.Objects[0].Size, value: 1, type: <class 'str'>, valid types: <class 'int'>\n\
                 Invalid type for parameter Delete.Objects[0].Time, value: x, type: <class 'str'>, valid types: <class 'datetime.datetime'>, timestamp-string"
                .to_owned())
        );
        assert_eq!(
            parse(r#"{"Objects":{"x":[1,2.5,1e20,true,null,"'"]}}"#),
            Err("An error occurred (ParamValidation): Parameter validation failed:\n\
                 Invalid type for parameter Delete.Objects, value: {'x': [1, 2.5, 1e+20, True, None, \"'\"]}, \
                 type: <class 'dict'>, valid types: <class 'list'>, <class 'tuple'>"
                .to_owned())
        );
        assert_eq!(
            parse(r#"{"Quiet":false}"#),
            Err(
                "An error occurred (ParamValidation): Parameter validation failed:\n\
                 Missing required parameter in Delete: \"Objects\""
                    .to_owned()
            )
        );
    }

    #[test]
    fn repr_float() {
        assert_eq!(super::repr_float(1.0), "1.0");
        assert_eq!(super::repr_float(2.5), "2.5");
        assert_eq!(super::repr_float(1e16), "1e+16");
        assert_eq!(super::repr_float(1.5e-5), "1.5e-05");
        assert_eq!(super::repr_float(0.0001), "0.0001");
    }
}
