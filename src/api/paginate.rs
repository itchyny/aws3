//! The automatic pagination of the list operations, in the same way as the
//! `PageIterator` of botocore, whose pages are merged into one response.

use std::io::{self, Write};

use aws_smithy_types::base64;

use super::shape;
use crate::cli::PaginationArgs;
use crate::error::Error;
use crate::transfer::decode_json;
use crate::value::{Value, insert};

/// The values of the input tokens of a request, in the order of the tokens,
/// where `None` is the token not to send.
pub type Token = Vec<Option<String>>;

/// The configuration of the pagination of an operation.
pub struct Paginator {
    /// The names of the members of the input tokens.
    pub input_tokens: &'static [&'static str],
    /// The names of the result keys, whose first key is counted for the
    /// maximum number of items.
    pub result_keys: &'static [&'static str],
}

/// A page of the responses.
pub struct Page {
    /// The full response of the page.
    pub response: Value,
    /// The values of the result keys, which are `None` if absent.
    pub results: Vec<Option<Vec<Value>>>,
    /// The members of the response recorded from the first page.
    pub non_aggregate: Vec<(&'static str, Value)>,
    /// The tokens of the next page, which is empty without more results.
    pub next_token: Token,
}

impl Paginator {
    /// Returns whether the pagination is enabled, which is disabled by
    /// `--no-paginate` or the input tokens or the limit given by the user.
    /// The pagination arguments are not allowed without the pagination.
    pub fn enabled(no_paginate: bool, manual: bool, args: &PaginationArgs) -> Result<bool, Error> {
        if !no_paginate && !manual {
            return Ok(true);
        }
        let used: Vec<_> = [
            (
                "--starting-token",
                args.starting_token
                    .as_ref()
                    .is_some_and(|token| !token.is_empty()),
            ),
            ("--page-size", args.page_size.is_some_and(|size| size != 0)),
            (
                "--max-items",
                args.max_items.is_some_and(|items| items != 0),
            ),
        ]
        .into_iter()
        .filter_map(|(name, used)| used.then_some(name))
        .collect();
        if !used.is_empty() {
            return Err(Error::param_validation(format!(
                "Cannot specify --no-paginate along with pagination arguments: {}",
                used.join(", ")
            )));
        }
        Ok(false)
    }

    /// Fetches the pages and returns the merged response, like
    /// `build_full_result` of botocore.
    pub async fn paginate<F, Fut>(
        &self,
        args: &PaginationArgs,
        mut fetch: F,
    ) -> Result<Pages, Error>
    where
        F: FnMut(Token) -> Fut,
        Fut: Future<Output = Result<Page, Error>>,
    {
        if args.max_items.is_some_and(|items| items <= 0) {
            let _ = io::stderr().write_all(
                b"warning: Non-positive values for --max-items may result in undefined behavior.\n",
            );
        }
        let starting = args
            .starting_token
            .as_deref()
            .map(|token| self.parse_starting_token(token))
            .transpose()?;
        let (mut next_token, mut starting_truncation) = match &starting {
            Some((token, truncation)) => (token.clone(), *truncation),
            None => (vec![None; self.input_tokens.len()], 0),
        };
        let mut pages = Vec::new();
        let mut responses = Vec::new();
        let mut non_aggregate = Vec::new();
        let mut resume_token = None;
        let mut previous_token = None;
        let mut total_items = 0;
        let mut first = true;
        loop {
            let mut page = fetch(next_token.clone()).await?;
            if first {
                if starting.is_some() {
                    // The items before the truncation of the starting token
                    // are skipped, and the other result keys are emptied.
                    if let Some(items) = &mut page.results[0] {
                        items.drain(..starting_truncation.min(items.len()));
                    }
                    for items in page.results.iter_mut().skip(1).flatten() {
                        items.clear();
                    }
                    self.set_results(&mut page.response, &page.results, self.result_keys.len());
                }
                non_aggregate = std::mem::take(&mut page.non_aggregate);
                first = false;
            } else {
                starting_truncation = 0;
            }
            let count = page.results[0].as_ref().map_or(0, Vec::len) as i64;
            let truncate_amount = args
                .max_items
                .map_or(0, |max_items| total_items + count - max_items);
            if truncate_amount > 0 {
                let keep = (count - truncate_amount).max(0) as usize;
                if let Some(items) = &mut page.results[0] {
                    items.truncate(keep);
                }
                self.set_results(&mut page.response, &page.results, 1);
                responses.push(page.response);
                pages.push(page.results);
                resume_token = Some((next_token, Some(keep + starting_truncation)));
                break;
            }
            responses.push(page.response);
            pages.push(page.results);
            total_items += count;
            if page.next_token.iter().all(Option::is_none) {
                break;
            }
            if args.max_items == Some(total_items) {
                resume_token = Some((page.next_token, None));
                break;
            }
            if previous_token.as_ref() == Some(&page.next_token) {
                return Err(Error::new(format!(
                    "Error during pagination: The same next token was received twice: {{{}}}",
                    self.input_tokens
                        .iter()
                        .zip(&page.next_token)
                        .map(|(name, token)| format!(
                            "'{name}': {}",
                            token
                                .as_ref()
                                .map_or("None".to_owned(), |token| format!("'{token}'"))
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
            }
            next_token = page.next_token.clone();
            previous_token = Some(page.next_token);
        }
        Ok(Pages {
            result_keys: self.result_keys,
            pages,
            responses,
            non_aggregate,
            next_token: resume_token
                .map(|(token, truncation)| self.encode_token(&token, truncation)),
        })
    }

    /// Sets the values of the first result keys to the response, where the
    /// absent values are set to null, like `set_value_from_jmespath` of
    /// botocore.
    fn set_results(&self, response: &mut Value, results: &[Option<Vec<Value>>], count: usize) {
        if let Value::Map(pairs) = response {
            for (key, items) in self.result_keys.iter().zip(results).take(count) {
                insert(
                    pairs,
                    (*key).to_owned(),
                    items.clone().map_or(Value::Null, Value::List),
                );
            }
        }
    }

    /// Encodes the token in base64 of JSON, like `TokenEncoder` of botocore.
    fn encode_token(&self, token: &Token, truncation: Option<usize>) -> String {
        let mut pairs: Vec<_> = self
            .input_tokens
            .iter()
            .zip(token)
            .map(|(name, token)| {
                format!(
                    "{}: {}",
                    python_json_string(name),
                    token
                        .as_deref()
                        .map_or("null".to_owned(), python_json_string)
                )
            })
            .collect();
        if let Some(truncation) = truncation {
            pairs.push(format!("\"boto_truncate_amount\": {truncation}"));
        }
        base64::encode(format!("{{{}}}", pairs.join(", ")))
    }

    /// Parses the starting token, which falls back to the deprecated format of
    /// the tokens separated by `___`, like `_parse_starting_token` of botocore.
    fn parse_starting_token(&self, value: &str) -> Result<(Token, usize), Error> {
        let Some(decoded) = decode_token(value) else {
            return self.parse_deprecated_starting_token(value);
        };
        let Value::Map(pairs) = decoded else {
            return Err(Error::new(format!(
                "'{}' object has no attribute 'pop'",
                shape::type_name(&decoded)
            )));
        };
        let mut token = vec![None; self.input_tokens.len()];
        let mut truncation = 0;
        for (key, value) in pairs {
            if key == "boto_truncate_amount" {
                if let Value::Int(amount) = &value {
                    truncation = amount.parse().unwrap_or(0);
                }
                continue;
            }
            let Some(index) = self.input_tokens.iter().position(|name| *name == key) else {
                continue;
            };
            token[index] = match value {
                Value::Str(value) if value != "None" => Some(value),
                Value::Str(_) | Value::Null => None,
                value => {
                    return Err(Error::param_validation(format!(
                        "Parameter validation failed:\nInvalid type for parameter {key}, value: {}, \
                         type: <class '{}'>, valid types: <class 'str'>",
                        shape::str(&value),
                        shape::type_name(&value)
                    )));
                }
            };
        }
        Ok((token, truncation))
    }

    fn parse_deprecated_starting_token(&self, value: &str) -> Result<(Token, usize), Error> {
        let mut parts: Vec<&str> = value.split("___").collect();
        let mut truncation = 0;
        if parts.len() == self.input_tokens.len() + 1 {
            match parts.last().and_then(|part| part.parse().ok()) {
                Some(amount) => {
                    truncation = amount;
                    parts.pop();
                }
                None => parts = vec![value],
            }
        }
        if parts.len() > self.input_tokens.len() {
            return Err(Error::new(format!("Bad starting token: {value}")));
        }
        let mut token: Token = parts
            .into_iter()
            .map(|part| (part != "None").then(|| part.to_owned()))
            .collect();
        token.resize(self.input_tokens.len(), None);
        Ok((token, truncation))
    }
}

/// The pages of the responses, which are merged into one response in JSON,
/// and written page by page in text.
pub struct Pages {
    result_keys: &'static [&'static str],
    /// The values of the result keys of the pages, which are `None` if absent.
    pages: Vec<Vec<Option<Vec<Value>>>>,
    /// The full responses of the pages, whose result keys are truncated.
    responses: Vec<Value>,
    non_aggregate: Vec<(&'static str, Value)>,
    /// The token to resume the pagination, which is encoded.
    next_token: Option<String>,
}

impl Pages {
    /// Returns the merged response, like `build_full_result` of botocore.
    pub fn merge(self) -> Value {
        let mut results: Vec<Option<Vec<Value>>> = vec![None; self.result_keys.len()];
        for page in self.pages {
            for (result, items) in results.iter_mut().zip(page) {
                if let Some(items) = items {
                    result.get_or_insert_with(Vec::new).extend(items);
                }
            }
        }
        let mut response: Vec<(String, Value)> = self
            .result_keys
            .iter()
            .zip(results)
            .filter_map(|(key, items)| Some(((*key).to_owned(), Value::List(items?))))
            .collect();
        response.extend(
            self.non_aggregate
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value)),
        );
        if let Some(token) = self.next_token {
            response.push(("NextToken".to_owned(), Value::Str(token)));
        }
        Value::Map(response)
    }

    /// Returns the full responses of the pages for the streamed YAML output,
    /// like the iteration of `PageIterator` of botocore, which does not
    /// include the token to resume.
    pub fn into_responses(self) -> Vec<Value> {
        self.responses
    }

    /// Returns the responses of the pages for the text output, where the first
    /// page includes the members recorded from it, and the token to resume
    /// follows the pages, like `TextFormatter` of the AWS CLI.
    pub fn into_pages(self) -> Vec<Value> {
        let mut responses: Vec<Value> = Vec::new();
        let mut non_aggregate = Some(self.non_aggregate);
        for page in self.pages {
            let mut response: Vec<(String, Value)> = non_aggregate
                .take()
                .unwrap_or_default()
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect();
            for (key, items) in self.result_keys.iter().zip(page) {
                insert(
                    &mut response,
                    (*key).to_owned(),
                    items.map_or(Value::Null, Value::List),
                );
            }
            responses.push(Value::Map(response));
        }
        if let Some(token) = self.next_token {
            let token = Value::Map(vec![("NextToken".to_owned(), Value::Str(token))]);
            responses.push(Value::Map(vec![("NextToken".to_owned(), token)]));
        }
        responses
    }
}

/// Decodes the token of base64 of JSON, which discards the characters other
/// than those of base64 like `base64.b64decode` of Python.
fn decode_token(value: &str) -> Option<Value> {
    let filtered: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        .collect();
    if !filtered.len().is_multiple_of(4) {
        return None;
    }
    let bytes = base64::decode(&filtered).ok()?;
    decode_json(std::str::from_utf8(&bytes).ok()?).ok()
}

/// Quotes the string in JSON like `json.dumps` of Python, which escapes the
/// non-ASCII characters.
fn python_json_string(value: &str) -> String {
    let mut buf = String::from('"');
    for c in value.chars() {
        match c {
            '"' => buf.push_str("\\\""),
            '\\' => buf.push_str("\\\\"),
            '\n' => buf.push_str("\\n"),
            '\r' => buf.push_str("\\r"),
            '\t' => buf.push_str("\\t"),
            '\u{8}' => buf.push_str("\\b"),
            '\u{c}' => buf.push_str("\\f"),
            c if c.is_ascii() && !c.is_ascii_control() || c == '\u{7f}' => buf.push(c),
            c => {
                let mut units = [0; 2];
                for unit in c.encode_utf16(&mut units) {
                    buf.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    buf.push('"');
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGINATOR: Paginator = Paginator {
        input_tokens: &["ContinuationToken"],
        result_keys: &["Contents", "CommonPrefixes"],
    };

    #[test]
    fn encode_token() {
        assert_eq!(
            PAGINATOR.encode_token(&vec![Some("a.txt".to_owned())], Some(1)),
            "eyJDb250aW51YXRpb25Ub2tlbiI6ICJhLnR4dCIsICJib3RvX3RydW5jYXRlX2Ftb3VudCI6IDF9"
        );
        assert_eq!(
            PAGINATOR.encode_token(&vec![None], Some(2)),
            "eyJDb250aW51YXRpb25Ub2tlbiI6IG51bGwsICJib3RvX3RydW5jYXRlX2Ftb3VudCI6IDJ9"
        );
        assert_eq!(
            PAGINATOR.encode_token(&vec![Some("dir/x.txt".to_owned())], None),
            "eyJDb250aW51YXRpb25Ub2tlbiI6ICJkaXIveC50eHQifQ=="
        );
    }

    #[test]
    fn parse_starting_token() {
        assert_eq!(
            PAGINATOR
                .parse_starting_token(
                    "eyJDb250aW51YXRpb25Ub2tlbiI6ICJhLnR4dCIsICJib3RvX3RydW5jYXRlX2Ftb3VudCI6IDF9"
                )
                .unwrap(),
            (vec![Some("a.txt".to_owned())], 1)
        );
        assert_eq!(
            PAGINATOR.parse_starting_token("bogus").unwrap(),
            (vec![Some("bogus".to_owned())], 0)
        );
        assert_eq!(
            PAGINATOR.parse_starting_token("a.txt___3").unwrap(),
            (vec![Some("a.txt".to_owned())], 3)
        );
        assert_eq!(
            PAGINATOR.parse_starting_token("None").unwrap(),
            (vec![None], 0)
        );
        assert_eq!(
            PAGINATOR
                .parse_starting_token("a___b___c")
                .unwrap_err()
                .to_string(),
            "Bad starting token: a___b___c"
        );
    }

    #[test]
    fn python_json_string() {
        assert_eq!(
            super::python_json_string("a\"\\\n\u{1}\u{7f}é😀"),
            "\"a\\\"\\\\\\n\\u0001\u{7f}\\u00e9\\ud83d\\ude00\""
        );
    }
}
