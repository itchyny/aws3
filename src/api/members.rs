//! The accessors of the members of the validated arguments, which convert the
//! values to the types of the requests.

use aws_sdk_s3::primitives::DateTime;

use crate::transfer::params::parse_timestamp;
use crate::value::Value;

pub fn str(value: &Value, key: &str) -> Option<String> {
    match value.get(key) {
        Some(Value::Str(value)) => Some(value.clone()),
        _ => None,
    }
}

pub fn bool(value: &Value, key: &str) -> Option<bool> {
    match value.get(key) {
        Some(Value::Bool(value)) => Some(*value),
        _ => None,
    }
}

pub fn long(value: &Value, key: &str) -> Option<i64> {
    match value.get(key) {
        Some(Value::Int(value)) => value.parse().ok(),
        _ => None,
    }
}

/// Returns the timestamp of the string or the epoch seconds.
pub fn timestamp(value: &Value, key: &str) -> Option<DateTime> {
    match value.get(key)? {
        Value::Str(value) => parse_timestamp(value).ok(),
        Value::Int(value) => value.parse().ok().map(DateTime::from_secs),
        Value::Float(value) => Some(DateTime::from_secs_f64(*value)),
        _ => None,
    }
}

/// Returns the list, which is empty if absent.
pub fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    match value.get(key) {
        Some(Value::List(values)) => values,
        _ => &[],
    }
}
