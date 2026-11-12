//! The list-buckets operation.

use aws_sdk_s3::Client;
use aws_sdk_s3::operation::list_buckets::ListBucketsOutput;
use aws_sdk_s3::types::Bucket;

use super::list_objects_v2::owner_value;
use super::output::Response;
use super::paginate::{Page, Paginator, Token};
use super::{Responses, parse_int};
use crate::cli::{GlobalArgs, ListBucketsArgs};
use crate::error::Error;
use crate::value::Value;

const PAGINATOR: Paginator = Paginator {
    input_tokens: &["ContinuationToken"],
    result_keys: &["Buckets"],
};

pub async fn run(
    client: &Client,
    args: &ListBucketsArgs,
    global: &GlobalArgs,
) -> Result<Responses, Error> {
    let max_buckets = parse_int(args.max_buckets.as_deref())?;
    let manual = args.continuation_token.is_some() || max_buckets.is_some();
    let paginate = Paginator::enabled(global.no_paginate, manual, &args.pagination)?;
    if !paginate {
        let token = vec![args.continuation_token.clone()];
        let output = list(client, args, token, max_buckets).await?;
        return Ok(Responses::Value(response(&output)));
    }
    let pages = PAGINATOR
        .paginate(&args.pagination, |token| async move {
            let output = list(client, args, token, args.pagination.page_size).await?;
            Ok(page(&output))
        })
        .await?;
    Ok(Responses::Pages(pages))
}

async fn list(
    client: &Client,
    args: &ListBucketsArgs,
    token: Token,
    max_buckets: Option<i32>,
) -> Result<ListBucketsOutput, Error> {
    let [continuation_token] = <[_; 1]>::try_from(token).unwrap_or_default();
    client
        .list_buckets()
        .set_max_buckets(max_buckets)
        .set_continuation_token(continuation_token)
        .set_prefix(args.prefix.clone())
        .set_bucket_region(args.bucket_region.clone())
        .send()
        .await
        .map_err(|err| Error::sdk("ListBuckets", err))
}

/// Returns the full response of a request without the pagination.
fn response(output: &ListBucketsOutput) -> Value {
    let mut response = Response::default();
    if let Some(buckets) = buckets(output) {
        response.value("Buckets", Value::List(buckets));
    }
    if let Some(owner) = output.owner() {
        response.value("Owner", owner_value(owner));
    }
    response.str("ContinuationToken", output.continuation_token());
    response.str("Prefix", output.prefix());
    response.build()
}

/// Returns the page of the response, whose owner and prefix are recorded from
/// the first page. The pages continue while the token is responded.
fn page(output: &ListBucketsOutput) -> Page {
    let token = output
        .continuation_token()
        .filter(|token| !token.is_empty())
        .map(str::to_owned);
    Page {
        response: response(output),
        results: vec![buckets(output)],
        non_aggregate: vec![
            ("Owner", output.owner().map_or(Value::Null, owner_value)),
            (
                "Prefix",
                output
                    .prefix()
                    .map_or(Value::Null, |prefix| Value::Str(prefix.to_owned())),
            ),
        ],
        next_token: vec![token],
    }
}

fn buckets(output: &ListBucketsOutput) -> Option<Vec<Value>> {
    let buckets = output.buckets();
    (!buckets.is_empty()).then(|| buckets.iter().map(bucket_value).collect())
}

fn bucket_value(bucket: &Bucket) -> Value {
    let mut response = Response::default();
    response.str("Name", bucket.name());
    response.timestamp("CreationDate", bucket.creation_date());
    response.str("BucketRegion", bucket.bucket_region());
    response.str("BucketArn", bucket.bucket_arn());
    response.build()
}
