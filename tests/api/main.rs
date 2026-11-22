// The paths in the outputs assume Unix.
#![cfg(unix)]

#[path = "../common/mod.rs"]
mod common;

mod acl;
mod buckets;
mod cors;
mod encryption;
mod lifecycle;
mod logging;
mod multipart_uploads;
mod notification;
mod object_listing;
mod objects;
mod output;
mod policy;
mod public_access_block;
mod tagging;
mod versioning;

use common::{Env, xml_response};

/// Returns the body of the last request, and the headers of the checksum.
fn last_request_body(env: &Env) -> (String, Vec<String>) {
    let requests = env.requests();
    let request = requests.last().unwrap();
    let mut headers: Vec<_> = request
        .headers
        .iter()
        .filter(|(name, _)| {
            name.as_str().starts_with("x-amz-checksum-") || *name == "x-amz-sdk-checksum-algorithm"
        })
        .map(|(name, value)| format!("{name}: {}", value.to_str().unwrap()))
        .collect();
    headers.sort();
    (String::from_utf8_lossy(&request.body).into_owned(), headers)
}

/// Runs an operation on an object, which the fake server does not implement,
/// and returns the method, the path and the query of the request.
fn unimplemented_object_operation(
    env: &Env,
    args: &[&str],
    name: &str,
) -> (http::Method, String, String) {
    let mut command = vec!["api"];
    command.extend(args);
    let result = env.run(&command);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        format!(
            "\naws3: [ERROR]: An error occurred (NotImplemented) when calling the {name} \
             operation: {name} is not implemented yet\n"
        )
    );
    let requests = env.requests();
    let request = requests.last().unwrap();
    (
        request.method.clone(),
        request.uri.path().to_owned(),
        request.uri.query().unwrap().to_owned(),
    )
}

/// Runs the get operation of a bucket configuration, which the fake server
/// does not implement, and returns the query of the request.
fn get_bucket_configuration(env: &Env, operation: &str, name: &str) -> String {
    bucket_configuration(env, operation, name, http::Method::GET)
}

/// Runs the delete operation of a bucket configuration, which the fake server
/// does not implement, and returns the query of the request.
fn delete_bucket_configuration(env: &Env, operation: &str, name: &str) -> String {
    bucket_configuration(env, operation, name, http::Method::DELETE)
}

fn bucket_configuration(env: &Env, operation: &str, name: &str, method: http::Method) -> String {
    let result = env.run(&["api", operation, "--bucket", "bucket"]);
    assert_eq!(result.code, 254);
    assert_eq!(
        result.stderr,
        format!(
            "\naws3: [ERROR]: An error occurred (NotImplemented) when calling the {name} \
             operation: {name} is not implemented yet\n"
        )
    );
    let requests = env.requests();
    let request = requests.last().unwrap();
    assert_eq!(request.method, method);
    assert_eq!(request.uri.path(), "/bucket/");
    request.uri.query().unwrap().to_owned()
}

/// Runs the api command with the response, which the fake server does not
/// implement, and returns the standard output.
fn response_output(
    env: &Env,
    args: &[&str],
    status: u16,
    headers: &[(&'static str, &'static str)],
    body: &'static str,
) -> String {
    let headers = headers.to_vec();
    env.intercept(move |_| {
        let mut response = xml_response(status, body);
        for &(name, value) in &headers {
            let value = http::HeaderValue::from_static(value);
            response.headers_mut().insert(name, value);
        }
        Some(response)
    });
    let mut command = vec!["api"];
    command.extend(args);
    env.run(&command).success().stdout
}
