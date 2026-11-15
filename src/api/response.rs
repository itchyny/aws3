//! The builder of the responses of the operations, whose members are in the
//! same order as the shapes of botocore and omitted when absent.

use std::sync::{Arc, Mutex, PoisonError};

use aws_sdk_s3::config::{ConfigBag, Intercept, RuntimeComponents};
use aws_sdk_s3::primitives::DateTime;
use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::BeforeDeserializationInterceptorContextRef;

use super::paginate::Pages;
use crate::value::{Value, insert};

#[derive(Debug, Default)]
pub struct Response(Vec<(String, Value)>);

impl Response {
    pub fn str(&mut self, key: &str, value: Option<impl AsRef<str>>) {
        if let Some(value) = value {
            self.value(key, Value::Str(value.as_ref().to_owned()));
        }
    }

    pub fn int(&mut self, key: &str, value: Option<impl Into<i64>>) {
        if let Some(value) = value {
            self.value(key, Value::Int(value.into().to_string()));
        }
    }

    pub fn bool(&mut self, key: &str, value: Option<bool>) {
        if let Some(value) = value {
            self.value(key, Value::Bool(value));
        }
    }

    pub fn timestamp(&mut self, key: &str, value: Option<&DateTime>) {
        if let Some(value) = value {
            self.value(key, Value::Str(format_timestamp(value)));
        }
    }

    pub fn value(&mut self, key: &str, value: Value) {
        self.0.push((key.to_owned(), value));
    }

    pub fn build(self) -> Value {
        Value::Map(self.0)
    }
}

/// Formats the timestamp in ISO 8601 like `datetime.isoformat` of Python,
/// which omits the fraction of zero microseconds.
fn format_timestamp(value: &DateTime) -> String {
    let Some(date_time) = chrono::DateTime::from_timestamp(value.secs(), value.subsec_nanos())
    else {
        return value.to_string();
    };
    if date_time.timestamp_subsec_micros() == 0 {
        date_time.format("%Y-%m-%dT%H:%M:%S+00:00")
    } else {
        date_time.format("%Y-%m-%dT%H:%M:%S%.6f+00:00")
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_timestamp() {
        assert_eq!(
            super::format_timestamp(&DateTime::from_secs(1_893_456_000)),
            "2030-01-01T00:00:00+00:00"
        );
        assert_eq!(
            super::format_timestamp(&DateTime::from_millis(1_893_456_000_123)),
            "2030-01-01T00:00:00.123000+00:00"
        );
    }

    #[test]
    fn response() {
        let mut response = Response::default();
        response.str("A", Some("a"));
        response.str("B", None::<&str>);
        response.int("C", Some(1));
        response.bool("D", Some(true));
        response.timestamp("E", Some(&DateTime::from_secs(0)));
        response.value("F", Value::Map(Vec::new()));
        assert_eq!(
            response.build().to_json(),
            r#"{"A":"a","C":1,"D":true,"E":"1970-01-01T00:00:00+00:00","F":{}}"#
        );
    }
}

/// The responses of an operation, which are paginated for the list
/// operations.
pub enum Responses {
    Value(Value),
    Pages(Pages),
}

impl From<Value> for Responses {
    fn from(value: Value) -> Self {
        Self::Value(value)
    }
}

/// The interceptor to keep the headers of the response, whose order of the
/// metadata is lost in the outputs of the SDK.
#[derive(Debug, Clone, Default)]
pub struct ResponseHeaders(Arc<Mutex<Vec<(String, String)>>>);

impl ResponseHeaders {
    /// Returns the metadata in the order of the headers, whose names are
    /// stripped of the prefix like botocore.
    pub fn metadata(&self) -> Value {
        let headers = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let mut pairs = Vec::new();
        for (name, value) in headers.iter() {
            if let Some(name) = name.to_lowercase().strip_prefix("x-amz-meta-") {
                insert(&mut pairs, name.to_owned(), Value::Str(value.clone()));
            }
        }
        Value::Map(pairs)
    }
}

impl Intercept for ResponseHeaders {
    fn name(&self) -> &'static str {
        "ResponseHeaders"
    }

    fn read_before_deserialization(
        &self,
        context: &BeforeDeserializationInterceptorContextRef<'_>,
        _components: &RuntimeComponents,
        _config: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        let headers = context
            .response()
            .headers()
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = headers;
        Ok(())
    }
}
