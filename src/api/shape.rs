//! The shapes of the structured arguments, which are converted from the
//! shorthand syntax and validated in the same way as the AWS CLI and botocore.

use std::num::{IntErrorKind, ParseIntError};

use crate::error::Error;
use crate::param;
use crate::value::{Value, invalid_type};

/// The shape of a member, a subset of the types of the models of botocore.
#[derive(Debug)]
pub enum Shape {
    String {
        min: usize,
    },
    Boolean,
    Long,
    Timestamp,
    List(&'static Shape),
    Structure(&'static Structure),
    /// Any value, which is neither converted nor validated, for the members
    /// whose shapes are not modeled.
    Any,
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
    let (pairs, shorthand) = param::parse_map_argument(option, value)?;
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

/// Parses the integer, which allows the surrounding whitespaces like `int` of
/// Python.
pub fn python_int(value: &str) -> Result<i64, Error> {
    let value = value.trim();
    value.parse().map_err(|err: ParseIntError| {
        Error::new(match err.kind() {
            IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => {
                format!("Integer value out of range: {value}")
            }
            _ => format!("Invalid integer value: {value}"),
        })
    })
}

/// Validates the value of the shape, collecting the errors like the parameter
/// validation of botocore.
fn validate(value: &Value, shape: &Shape, name: &str, errors: &mut Vec<String>) {
    let mut invalid = |valid_types: &str| errors.push(invalid_type(name, value, valid_types));
    match (shape, value) {
        (Shape::String { min }, Value::Str(string)) => {
            let length = string.chars().count();
            if length < *min {
                errors.push(format!(
                    "Invalid length for parameter {name}, value: {length}, valid min length: {min}"
                ));
            }
        }
        (Shape::String { .. }, _) => invalid("string"),
        (Shape::Boolean, Value::Bool(_)) | (Shape::Long, Value::Int(_)) => {}
        (Shape::Boolean, _) => invalid("boolean"),
        (Shape::Long, _) => invalid("integer"),
        (Shape::Timestamp, Value::Int(_) | Value::Float(_)) => {}
        (Shape::Timestamp, Value::Str(string)) if param::parse_timestamp(string).is_ok() => {}
        (Shape::Timestamp, _) => invalid("timestamp"),
        (Shape::List(member), Value::List(values)) => {
            for (i, value) in values.iter().enumerate() {
                validate(value, member, &format!("{name}[{i}]"), errors);
            }
        }
        (Shape::List(_), _) => invalid("list"),
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
        (Shape::Structure(_), _) => invalid("map"),
        (Shape::Any, _) => {}
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
            Err("Invalid integer value: x".to_owned())
        );
        assert_eq!(
            parse("Objects=[{Key=a,Size=99999999999999999999}]"),
            Err("Integer value out of range: 99999999999999999999".to_owned())
        );
        assert_eq!(
            parse("Objects=a,Quiet=x"),
            Err("An error occurred (ParamValidation): Parameter validation failed:\n\
                 Invalid type for parameter Delete.Objects[0], value: \"a\", type: string, valid types: map\n\
                 Invalid type for parameter Delete.Quiet, value: \"x\", type: string, valid types: boolean"
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
                 Invalid type for parameter Delete.Objects[0].Size, value: \"1\", type: string, valid types: integer\n\
                 Invalid type for parameter Delete.Objects[0].Time, value: \"x\", type: string, valid types: timestamp"
                .to_owned())
        );
        assert_eq!(
            parse(r#"{"Objects":{"x":[1,2.5,1e20,true,null,"'"]}}"#),
            Err("An error occurred (ParamValidation): Parameter validation failed:\n\
                 Invalid type for parameter Delete.Objects, value: {\"x\":[1,2.5,1e20,true,null,\"'\"]}, \
                 type: map, valid types: list"
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
}
