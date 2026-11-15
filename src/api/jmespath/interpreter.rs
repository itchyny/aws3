//! The interpreter of JMESPath expressions, ported from `jmespath.visitor`
//! and `jmespath.functions` of Python, whose values are compared and
//! formatted in the same way as Python, but the comparisons of the values of
//! different types result in null like the specification of JMESPath.

use std::cmp::Ordering;

use super::parser::{Comparator, Node};
use super::{Error, text};
use crate::value::{Value, ascii_json_string, insert, python_float};

pub fn search(node: &Node, value: &Value) -> Result<Value, Error> {
    Ok(match node {
        Node::Literal(literal) => literal.clone(),
        Node::Field(name) => value.get(name).cloned().unwrap_or(Value::Null),
        Node::Identity | Node::Current => value.clone(),
        Node::Subexpression(children) | Node::IndexExpression(children) => {
            let mut result = value.clone();
            for child in children {
                result = search(child, &result)?;
            }
            result
        }
        Node::Index(index) => match value {
            Value::List(values) => resolve_index(*index, values.len())
                .and_then(|index| values.get(index))
                .cloned()
                .unwrap_or(Value::Null),
            _ => Value::Null,
        },
        Node::Slice(parts) => match value {
            Value::List(values) => Value::List(slice(values, *parts)?),
            _ => Value::Null,
        },
        Node::Projection(left, right) => match search(left, value)? {
            Value::List(values) => project(values.iter(), right)?,
            _ => Value::Null,
        },
        Node::ValueProjection(left, right) => match search(left, value)? {
            Value::Map(pairs) => project(pairs.iter().map(|(_, value)| value), right)?,
            _ => Value::Null,
        },
        Node::FilterProjection(left, right, condition) => match search(left, value)? {
            Value::List(values) => {
                let mut collected = Vec::new();
                for element in &values {
                    if is_true(&search(condition, element)?) {
                        let current = search(right, element)?;
                        if current != Value::Null {
                            collected.push(current);
                        }
                    }
                }
                Value::List(collected)
            }
            _ => Value::Null,
        },
        Node::Flatten(child) => match search(child, value)? {
            Value::List(values) => {
                let mut merged = Vec::new();
                for element in values {
                    match element {
                        Value::List(values) => merged.extend(values),
                        element => merged.push(element),
                    }
                }
                Value::List(merged)
            }
            _ => Value::Null,
        },
        Node::Not(child) => {
            let result = search(child, value)?;
            // The numbers are true even if zero.
            Value::Bool(number(&result).is_none() && !truthy(&result))
        }
        Node::Pipe(left, right) => search(right, &search(left, value)?)?,
        Node::Or(left, right) => {
            let matched = search(left, value)?;
            if is_true(&matched) {
                matched
            } else {
                search(right, value)?
            }
        }
        Node::And(left, right) => {
            let matched = search(left, value)?;
            if is_true(&matched) {
                search(right, value)?
            } else {
                matched
            }
        }
        Node::Comparator(comparator, left, right) => {
            let (left, right) = (search(left, value)?, search(right, value)?);
            compare(*comparator, &left, &right)?
        }
        Node::Function(name, args) => {
            let args = args
                .iter()
                .map(|arg| match arg {
                    Node::Expref(node) => Ok(Arg::Expref(node)),
                    arg => search(arg, value).map(Arg::Value),
                })
                .collect::<Result<Vec<_>, _>>()?;
            call_function(name, args)?
        }
        // The expression references are only for the functions, which fail
        // on formatting the output in the AWS CLI.
        Node::Expref(_) => {
            return Err(Error::Runtime(
                "Encountered unrecognized type in JSON encoder.".to_owned(),
            ));
        }
        Node::MultiSelectList(children) => {
            if *value == Value::Null {
                return Ok(Value::Null);
            }
            let values = children.iter().map(|child| search(child, value));
            Value::List(values.collect::<Result<_, _>>()?)
        }
        Node::MultiSelectHash(pairs) => {
            if *value == Value::Null {
                return Ok(Value::Null);
            }
            let mut collected = Vec::new();
            for (key, child) in pairs {
                insert(&mut collected, key.clone(), search(child, value)?);
            }
            Value::Map(collected)
        }
    })
}

fn project<'a>(values: impl Iterator<Item = &'a Value>, right: &Node) -> Result<Value, Error> {
    let mut collected = Vec::new();
    for element in values {
        let current = search(right, element)?;
        if current != Value::Null {
            collected.push(current);
        }
    }
    Ok(Value::List(collected))
}

fn resolve_index(index: i64, len: usize) -> Option<usize> {
    let index = if index < 0 {
        index.checked_add(len as i64)?
    } else {
        index
    };
    usize::try_from(index).ok().filter(|index| *index < len)
}

/// Slices the values like the slices of Python.
fn slice(values: &[Value], [start, stop, step]: [Option<i64>; 3]) -> Result<Vec<Value>, Error> {
    let step = step.unwrap_or(1);
    if step == 0 {
        return Err(Error::Runtime("slice step cannot be zero".to_owned()));
    }
    let len = values.len() as i64;
    let (lower, upper) = if step < 0 { (-1, len - 1) } else { (0, len) };
    let bound = |value: Option<i64>, default: i64| match value {
        None => default,
        Some(value) if value < 0 => (value + len).max(lower),
        Some(value) => value.min(upper),
    };
    let start = bound(start, if step < 0 { upper } else { lower });
    let stop = bound(stop, if step < 0 { lower } else { upper });
    let mut sliced = Vec::new();
    let mut index = start;
    while if step > 0 { index < stop } else { index > stop } {
        sliced.push(values[index as usize].clone());
        index += step;
    }
    Ok(sliced)
}

/// Returns whether the value is false, i.e. an empty string, an empty array,
/// an empty object, null or false.
fn is_false(value: &Value) -> bool {
    match value {
        Value::Str(string) => string.is_empty(),
        Value::List(values) => values.is_empty(),
        Value::Map(pairs) => pairs.is_empty(),
        Value::Null | Value::Bool(false) => true,
        _ => false,
    }
}

fn is_true(value: &Value) -> bool {
    !is_false(value)
}

/// Returns the truth of the value in Python.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Bytes(bytes) => !bytes.is_empty(),
        Value::Int(_) | Value::Float(_) => number(value).is_some_and(|number| !number.is_zero()),
        value => is_true(value),
    }
}

#[derive(Debug, Clone, Copy)]
enum Number {
    Int(i128),
    Float(f64),
}

impl Number {
    fn as_f64(self) -> f64 {
        match self {
            Self::Int(value) => value as f64,
            Self::Float(value) => value,
        }
    }

    fn is_zero(self) -> bool {
        self.as_f64() == 0.0
    }

    fn partial_cmp(self, other: Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Int(left), Self::Int(right)) => Some(left.cmp(&right)),
            (left, right) => left.as_f64().partial_cmp(&right.as_f64()),
        }
    }

    fn value(self) -> Value {
        match self {
            Self::Int(value) => Value::Int(value.to_string()),
            Self::Float(value) => Value::Float(value),
        }
    }
}

/// Returns the number of the value, which is not a boolean.
fn number(value: &Value) -> Option<Number> {
    match value {
        Value::Int(digits) => Some(digits.parse().map_or_else(
            |_| Number::Float(digits.parse().unwrap_or(f64::NAN)),
            Number::Int,
        )),
        Value::Float(value) => Some(Number::Float(*value)),
        _ => None,
    }
}

/// Returns whether the values are equal in Python, where the booleans equal
/// the integers of 0 and 1.
fn python_eq(left: &Value, right: &Value) -> bool {
    let as_number = |value: &Value| match value {
        Value::Bool(value) => Some(Number::Int(i128::from(*value))),
        value => number(value),
    };
    match (left, right) {
        (Value::List(left), Value::List(right)) => {
            left.len() == right.len() && left.iter().zip(right).all(|(l, r)| python_eq(l, r))
        }
        (Value::Map(left), Value::Map(right)) => {
            left.len() == right.len()
                && left.iter().all(|(key, value)| {
                    right
                        .iter()
                        .find(|(k, _)| k == key)
                        .is_some_and(|(_, v)| python_eq(value, v))
                })
        }
        (Value::Str(left), Value::Str(right)) => left == right,
        (Value::Null, Value::Null) => true,
        (left, right) => match (as_number(left), as_number(right)) {
            (Some(left), Some(right)) => left.partial_cmp(right) == Some(Ordering::Equal),
            _ => false,
        },
    }
}

/// Returns whether the values are equal in JMESPath, where the booleans do
/// not equal the numbers of 0 and 1 unlike Python.
fn equals(left: &Value, right: &Value) -> bool {
    let zero_or_one =
        |value: &Value| number(value).is_some_and(|number| matches!(number.as_f64(), 0.0 | 1.0));
    let special = if number(left).is_some() {
        zero_or_one(left) && matches!(right, Value::Bool(_))
    } else {
        zero_or_one(right) && matches!(left, Value::Bool(_))
    };
    !special && python_eq(left, right)
}

fn compare(comparator: Comparator, left: &Value, right: &Value) -> Result<Value, Error> {
    let ordering = match comparator {
        Comparator::Eq => return Ok(Value::Bool(equals(left, right))),
        Comparator::Ne => return Ok(Value::Bool(!equals(left, right))),
        _ => {
            let comparable = |value: &Value| matches!(type_of(value), "number" | "string");
            if !comparable(left) || type_of(left) != type_of(right) {
                return Ok(Value::Null);
            }
            cmp(left, right)
        }
    };
    Ok(Value::Bool(match (comparator, ordering) {
        (_, None) => false,
        (Comparator::Lt, Some(ordering)) => ordering.is_lt(),
        (Comparator::Lte, Some(ordering)) => ordering.is_le(),
        (Comparator::Gt, Some(ordering)) => ordering.is_gt(),
        (_, Some(ordering)) => ordering.is_ge(),
    }))
}

/// Compares the numbers or the strings, which returns `None` for the values
/// of the other types.
fn cmp(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Str(left), Value::Str(right)) => Some(left.cmp(right)),
        _ => number(left)?.partial_cmp(number(right)?),
    }
}

/// Returns the name of the type of the value in JMESPath.
fn type_of(value: &Value) -> &'static str {
    match value {
        Value::Str(_) => "string",
        Value::Bool(_) => "boolean",
        Value::List(_) => "array",
        Value::Map(_) => "object",
        Value::Int(_) | Value::Float(_) => "number",
        Value::Null | Value::Bytes(_) => "null",
    }
}

enum Arg<'a> {
    Value(Value),
    Expref(&'a Node),
}

impl Arg<'_> {
    fn type_name(&self) -> &'static str {
        match self {
            Self::Value(value) => type_of(value),
            Self::Expref(_) => "expref",
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Value(value) => text(value),
            Self::Expref(_) => "<expref>".to_owned(),
        }
    }

    fn into_value(self) -> Value {
        match self {
            Self::Value(value) => value,
            Self::Expref(_) => Value::Null,
        }
    }
}

/// The signatures of the functions, i.e. the allowed types of the arguments
/// and whether the last argument is variadic.
fn signature(name: &str) -> Option<(&'static [&'static [&'static str]], bool)> {
    Some(match name {
        "abs" | "ceil" | "floor" => (&[&["number"]], false),
        "avg" | "sum" => (&[&["array-number"]], false),
        "not_null" => (&[&[]], true),
        "to_array" | "to_string" | "to_number" | "type" => (&[&[]], false),
        "contains" => (&[&["array", "string"], &[]], false),
        "length" => (&[&["string", "array", "object"]], false),
        "ends_with" | "starts_with" => (&[&["string"], &["string"]], false),
        "reverse" => (&[&["array", "string"]], false),
        "join" => (&[&["string"], &["array-string"]], false),
        "map" => (&[&["expref"], &["array"]], false),
        "max" | "min" => (&[&["array-number", "array-string"]], false),
        "merge" => (&[&["object"]], true),
        "sort" => (&[&["array-string", "array-number"]], false),
        "keys" | "values" => (&[&["object"]], false),
        "sort_by" | "min_by" | "max_by" => (&[&["array"], &["expref"]], false),
        _ => return None,
    })
}

fn type_error(function: &str, value: &str, actual: &str, types: &[&str]) -> Error {
    Error::Runtime(format!(
        "In function {function}(), invalid type for value: {value}, expected {}, \
         received: \"{actual}\"",
        match types {
            [init @ .., last] if !init.is_empty() => format!("{} or {last}", init.join(", ")),
            _ => types.join(""),
        }
    ))
}

/// Validates the arguments like `_validate_arguments` of Python.
fn validate(name: &str, args: &[Arg<'_>]) -> Result<(), Error> {
    let Some((types, variadic)) = signature(name) else {
        return Err(Error::Runtime(format!("Unknown function: {name}()")));
    };
    let plural = |count: usize| if count == 1 { "argument" } else { "arguments" };
    if variadic && args.len() < types.len() {
        return Err(Error::Runtime(format!(
            "Expected at least {} {} for function {name}(), received {}",
            types.len(),
            plural(types.len()),
            args.len()
        )));
    } else if !variadic && args.len() != types.len() {
        return Err(Error::Runtime(format!(
            "Expected {} {} for function {name}(), received {}",
            types.len(),
            plural(types.len()),
            args.len()
        )));
    }
    for (arg, types) in args.iter().zip(types) {
        if types.is_empty() {
            continue;
        }
        let mut allowed = Vec::new();
        let mut subtypes = Vec::new();
        for t in *types {
            match t.split_once('-') {
                Some((t, subtype)) => {
                    allowed.push(t);
                    subtypes.push(subtype);
                }
                None => allowed.push(t),
            }
        }
        let actual = arg.type_name();
        if !allowed.contains(&actual) {
            return Err(type_error(name, &arg.text(), actual, types));
        }
        let Arg::Value(Value::List(elements)) = arg else {
            continue;
        };
        let allowed = match subtypes.as_slice() {
            [] => continue,
            [subtype] => *subtype,
            subtypes => {
                let Some(first) = elements.first() else {
                    continue;
                };
                let first_type = type_of(first);
                if !subtypes.contains(&first_type) {
                    return Err(type_error(name, &text(first), first_type, types));
                }
                first_type
            }
        };
        for element in elements {
            let actual = type_of(element);
            if actual != allowed {
                return Err(type_error(name, &text(element), actual, types));
            }
        }
    }
    Ok(())
}

fn call_function(name: &str, args: Vec<Arg<'_>>) -> Result<Value, Error> {
    validate(name, &args)?;
    if let [first, second] = args.as_slice()
        && let Some(result) = call_expref_function(name, first, second)
    {
        return result;
    }
    let mut args = args.into_iter();
    let mut next = || args.next().map_or(Value::Null, Arg::into_value);
    let list = |value: Value| match value {
        Value::List(values) => values,
        _ => Vec::new(),
    };
    Ok(match name {
        "abs" => match number(&next()) {
            Some(Number::Int(value)) => Number::Int(value.abs()).value(),
            Some(Number::Float(value)) => Value::Float(value.abs()),
            None => Value::Null,
        },
        "avg" => {
            let values = list(next());
            if values.is_empty() {
                Value::Null
            } else {
                Value::Float(sum(&values).as_f64() / values.len() as f64)
            }
        }
        "ceil" | "floor" => match number(&next()) {
            Some(Number::Float(value)) => {
                let rounded = if name == "ceil" {
                    value.ceil()
                } else {
                    value.floor()
                };
                // The non-finite numbers are not rounded to the integers.
                if rounded.is_finite() {
                    Value::Int(format!("{rounded:.0}"))
                } else {
                    Value::Float(rounded)
                }
            }
            Some(number) => number.value(),
            None => Value::Null,
        },
        "contains" => {
            let (subject, search) = (next(), next());
            match (&subject, &search) {
                (Value::Str(subject), Value::Str(search)) => {
                    Value::Bool(subject.contains(search.as_str()))
                }
                (Value::Str(_), _) => Value::Bool(false),
                (Value::List(values), search) => {
                    Value::Bool(values.iter().any(|value| python_eq(value, search)))
                }
                _ => Value::Null,
            }
        }
        "ends_with" | "starts_with" => match (next(), next()) {
            (Value::Str(subject), Value::Str(affix)) => Value::Bool(if name == "ends_with" {
                subject.ends_with(&affix)
            } else {
                subject.starts_with(&affix)
            }),
            _ => Value::Null,
        },
        "join" => match next() {
            Value::Str(separator) => {
                let values: Vec<_> = list(next()).into_iter().map(|value| text(&value)).collect();
                Value::Str(values.join(&separator))
            }
            _ => Value::Null,
        },
        "keys" | "values" => match next() {
            Value::Map(pairs) => Value::List(
                pairs
                    .into_iter()
                    .map(|(key, value)| {
                        if name == "keys" {
                            Value::Str(key)
                        } else {
                            value
                        }
                    })
                    .collect(),
            ),
            _ => Value::Null,
        },
        "length" => match next() {
            Value::Str(value) => Value::Int(value.chars().count().to_string()),
            Value::List(values) => Value::Int(values.len().to_string()),
            Value::Map(pairs) => Value::Int(pairs.len().to_string()),
            _ => Value::Null,
        },
        "max" | "min" => {
            let values = list(next());
            let mut best: Option<Value> = None;
            for value in values {
                best = Some(match best {
                    None => value,
                    Some(current) => {
                        let ordering = cmp(&value, &current);
                        let better = if name == "max" {
                            ordering == Some(Ordering::Greater)
                        } else {
                            ordering == Some(Ordering::Less)
                        };
                        if better { value } else { current }
                    }
                });
            }
            best.unwrap_or(Value::Null)
        }
        "merge" => {
            let mut merged = Vec::new();
            for arg in std::iter::once(next())
                .chain(std::iter::from_fn(|| args.next().map(Arg::into_value)))
            {
                if let Value::Map(pairs) = arg {
                    for (key, value) in pairs {
                        insert(&mut merged, key, value);
                    }
                }
            }
            Value::Map(merged)
        }
        "not_null" => std::iter::once(next())
            .chain(std::iter::from_fn(|| args.next().map(Arg::into_value)))
            .find(|value| *value != Value::Null)
            .unwrap_or(Value::Null),
        "reverse" => match next() {
            Value::Str(value) => Value::Str(value.chars().rev().collect()),
            Value::List(mut values) => {
                values.reverse();
                Value::List(values)
            }
            _ => Value::Null,
        },
        "sort" => {
            let mut values = list(next());
            values.sort_by(|left, right| cmp(left, right).unwrap_or(Ordering::Equal));
            Value::List(values)
        }
        "sum" => sum(&list(next())).value(),
        "to_array" => match next() {
            Value::List(values) => Value::List(values),
            value => Value::List(vec![value]),
        },
        "to_number" => match next() {
            value @ (Value::Int(_) | Value::Float(_)) => value,
            Value::Str(value) => to_number(&value),
            _ => Value::Null,
        },
        "to_string" => match next() {
            Value::Str(value) => Value::Str(value),
            value => Value::Str(ascii_json(&value)),
        },
        "type" => Value::Str(type_of(&next()).to_owned()),
        _ => Value::Null,
    })
}

/// Sums the numbers, which is an integer unless a float is included.
fn sum(values: &[Value]) -> Number {
    let mut total = Number::Int(0);
    for value in values {
        total = match (total, number(value)) {
            (Number::Int(left), Some(Number::Int(right))) => Number::Int(left + right),
            (left, Some(right)) => Number::Float(left.as_f64() + right.as_f64()),
            (total, None) => total,
        };
    }
    total
}

/// Converts the string to a number like `int` and then `float` of Python.
fn to_number(value: &str) -> Value {
    let trimmed = value.trim();
    let digits = trimmed.replace('_', "");
    let valid_underscores = !trimmed.starts_with('_')
        && !trimmed.ends_with('_')
        && !trimmed.contains("__")
        && !trimmed.contains("_.")
        && !trimmed.contains("._");
    if valid_underscores && let Ok(value) = digits.parse::<i128>() {
        return Value::Int(value.to_string());
    }
    let is_float = digits
        .trim_start_matches(['+', '-'])
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        || matches!(
            digits
                .trim_start_matches(['+', '-'])
                .to_lowercase()
                .as_str(),
            "inf" | "infinity" | "nan"
        );
    match digits.parse::<f64>() {
        Ok(value) if valid_underscores && is_float => Value::Float(value),
        _ => Value::Null,
    }
}

/// Returns the value in JSON with the compact separators, which escapes the
/// non-ASCII characters and formats the floats like Python.
fn ascii_json(value: &Value) -> String {
    match value {
        Value::Str(value) => ascii_json_string(value),
        Value::Float(value) if value.is_finite() => python_float(*value),
        Value::List(values) => {
            let values: Vec<_> = values.iter().map(ascii_json).collect();
            format!("[{}]", values.join(","))
        }
        Value::Map(pairs) => {
            let pairs: Vec<_> = pairs
                .iter()
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        ascii_json(&Value::Str(key.clone())),
                        ascii_json(value)
                    )
                })
                .collect();
            format!("{{{}}}", pairs.join(","))
        }
        value => value.to_json(),
    }
}

/// Calls the functions of the expression references, i.e. map, sort_by,
/// min_by and max_by, which return `None` for the other functions.
fn call_expref_function(
    name: &str,
    first: &Arg<'_>,
    second: &Arg<'_>,
) -> Option<Result<Value, Error>> {
    let (expref, values) = match (name, first, second) {
        ("map", Arg::Expref(expref), Arg::Value(Value::List(values)))
        | ("sort_by" | "min_by" | "max_by", Arg::Value(Value::List(values)), Arg::Expref(expref)) => {
            (*expref, values)
        }
        _ => return None,
    };
    Some(match name {
        "map" => values
            .iter()
            .map(|value| search(expref, value))
            .collect::<Result<_, _>>()
            .map(Value::List),
        "sort_by" => sort_by(expref, values),
        _ => min_max_by(name, expref, values),
    })
}

/// Returns the keys of the expression reference paired with the values,
/// where the keys must be numbers or strings of the same type.
fn keys<'a>(
    function: &str,
    expref: &Node,
    values: &'a [Value],
    types: &[&str],
) -> Result<Vec<(Value, &'a Value)>, Error> {
    let mut required = None;
    values
        .iter()
        .map(|value| {
            let key = search(expref, value)?;
            let actual = type_of(&key);
            let required = *required.get_or_insert(actual);
            if !matches!(required, "number" | "string") {
                return Err(type_error(function, &text(value), actual, types));
            } else if actual != required {
                return Err(type_error(function, &text(&key), actual, &[required]));
            }
            Ok((key, value))
        })
        .collect()
}

fn sort_by(expref: &Node, values: &[Value]) -> Result<Value, Error> {
    let mut keyed = keys("sort_by", expref, values, &["string", "number"])?;
    keyed.sort_by(|(left, _), (right, _)| cmp(left, right).unwrap_or(Ordering::Equal));
    Ok(Value::List(
        keyed.into_iter().map(|(_, value)| value.clone()).collect(),
    ))
}

fn min_max_by(name: &str, expref: &Node, values: &[Value]) -> Result<Value, Error> {
    let keyed = keys(name, expref, values, &["number", "string"])?;
    let best = keyed.into_iter().reduce(|best, current| {
        let ordering = cmp(&current.0, &best.0);
        let better = if name == "max_by" {
            ordering == Some(Ordering::Greater)
        } else {
            ordering == Some(Ordering::Less)
        };
        if better { current } else { best }
    });
    Ok(best.map_or(Value::Null, |(_, value)| value.clone()))
}
