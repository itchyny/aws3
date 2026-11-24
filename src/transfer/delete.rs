//! The deletions of the objects, where the recursive deletions are sent in the
//! batches of `DeleteObjects` instead of a `DeleteObject` for each object.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{Delete, ObjectIdentifier};
use aws_smithy_types::base64;
use md5::{Digest, Md5};
use tokio::task::JoinSet;

use super::context::Context;
use super::download::etag_mismatch;
use super::printer::{Printer, TransferId};
use super::task::Outcome;
use crate::error::{Error, has_error_code, has_status_code, is_retryable};
use crate::path::{split_bucket_key, validate_bucket_name};

/// The maximum number of the keys in a request of `DeleteObjects`.
pub(super) const MAX_BATCH_SIZE: usize = 1000;

/// A deletion of an object waiting to be sent in a batch.
pub(super) struct Deletion {
    pub(super) id: TransferId,
    /// `bucket/key` of S3.
    pub(super) path: String,
}

/// Deletes the object, only if it has the ETag if specified, so that the
/// object modified since it was transferred is not deleted. The ETag is not
/// sent to the endpoints which do not support it.
pub(super) async fn delete_object(
    ctx: &Context,
    client: &Client,
    bucket: &str,
    key: &str,
    etag: Option<&str>,
) -> Result<Outcome, String> {
    validate_bucket_name(bucket)?;
    let etag = etag.filter(|_| ctx.conditional_delete.load(Ordering::Relaxed));
    let send = |etag: Option<String>| {
        ctx.send(client, bucket, "DeleteObject", move |client| {
            client
                .delete_object()
                .bucket(bucket)
                .key(key)
                .set_if_match(etag.clone())
                .set_request_payer(ctx.params.request_payer.clone())
                .send()
        })
    };
    let result = match send(etag.map(str::to_owned)).await {
        Err(err)
            if etag.is_some()
                && (has_error_code(&err, "NotImplemented") || has_status_code(&err, 501)) =>
        {
            ctx.conditional_delete.store(false, Ordering::Relaxed);
            send(None).await
        }
        result => result,
    };
    match result {
        Ok(_) => Ok(Outcome::Done),
        Err(err) if etag.is_some() && has_error_code(&err, "PreconditionFailed") => {
            Err(etag_mismatch(bucket, key))
        }
        Err(err) => Err(Error::sdk("DeleteObject", err).to_string()),
    }
}

/// Deletes the objects of the bucket in a request of `DeleteObjects`, and
/// prints the results of the objects in the same way as the deletions of each
/// object. The objects are deleted one by one if the endpoint does not support
/// it.
pub(super) async fn delete_batch(ctx: Arc<Context>, bucket: String, deletions: Vec<Deletion>) {
    if !ctx.batch_delete.load(Ordering::Relaxed) {
        return delete_each(ctx, deletions).await;
    }
    let keys: Vec<_> = deletions
        .iter()
        .map(|deletion| split_bucket_key(&deletion.path).1)
        .collect();
    let result = async {
        let (ctx, bucket) = (&*ctx, bucket.as_str());
        validate_bucket_name(bucket)?;
        let objects = keys
            .iter()
            .map(|key| ObjectIdentifier::builder().key(*key).build())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| err.to_string())?;
        // The successful deletions are omitted from the response in quiet mode.
        let delete = Delete::builder()
            .set_objects(Some(objects))
            .quiet(true)
            .build()
            .map_err(|err| err.to_string())?;
        Ok(ctx
            .send(&ctx.client, bucket, "DeleteObjects", |client| {
                let delete = delete.clone();
                async move {
                    client
                        .delete_objects()
                        .bucket(bucket)
                        .delete(delete)
                        .set_request_payer(ctx.params.request_payer.clone())
                        .customize()
                        // Some S3-compatible storages require `Content-MD5`
                        // instead of the checksum of the SDK.
                        .mutate_request(|request| {
                            if let Some(body) = request.body().bytes() {
                                let md5 = base64::encode(Md5::digest(body));
                                request.headers_mut().insert("content-md5", md5);
                            }
                        })
                        .send()
                        .await
                }
            })
            .await)
    }
    .await;
    let results = match result {
        Ok(Ok(output)) => {
            // The errors are reported with the operation of the deletion of
            // each object, as the AWS CLI deletes the objects one by one.
            let errors: HashMap<_, _> = output
                .errors()
                .iter()
                .filter_map(|err| {
                    let code = err.code().unwrap_or("Unknown");
                    let message = format!(
                        "An error occurred ({code}) when calling the DeleteObject operation: {}",
                        err.message().unwrap_or("Unknown"),
                    );
                    Some((err.key()?, (code, message)))
                })
                .collect();
            // The transient errors of the objects are in the response of
            // HTTP 200, which the SDK does not retry, so the objects are
            // deleted one by one with the retries, like the AWS CLI. The
            // errors have no status codes, where InternalError and
            // ServiceUnavailable are of 500 and 503.
            keys.iter()
                .map(|key| match errors.get(key) {
                    None => Some(Ok(())),
                    Some((code, _))
                        if is_retryable(0, code)
                            || matches!(*code, "InternalError" | "ServiceUnavailable") =>
                    {
                        None
                    }
                    Some((_, message)) => Some(Err(message.clone())),
                })
                .collect()
        }
        Ok(Err(err)) if has_error_code(&err, "NotImplemented") || has_status_code(&err, 501) => {
            ctx.batch_delete.store(false, Ordering::Relaxed);
            return delete_each(ctx, deletions).await;
        }
        Ok(Err(err)) => {
            vec![Some(Err(Error::sdk("DeleteObject", err).to_string())); deletions.len()]
        }
        Err(message) => vec![Some(Err(message)); deletions.len()],
    };
    let mut retries = Vec::new();
    {
        let mut printer = ctx.printer();
        for (deletion, result) in deletions.into_iter().zip(results) {
            match result {
                Some(result) => print_result(&mut printer, &deletion, result),
                None => retries.push(deletion),
            }
        }
    }
    if !retries.is_empty() {
        delete_each(ctx, retries).await;
    }
}

/// Deletes the objects one by one concurrently, for the endpoints which do
/// not support `DeleteObjects`.
async fn delete_each(ctx: Arc<Context>, deletions: Vec<Deletion>) {
    let mut tasks = JoinSet::new();
    for deletion in deletions {
        let ctx = ctx.clone();
        tasks.spawn(async move {
            let (bucket, key) = split_bucket_key(&deletion.path);
            let result = delete_object(&ctx, &ctx.client, bucket, key, None).await;
            print_result(&mut ctx.printer(), &deletion, result.map(|_| ()));
        });
    }
    while tasks.join_next().await.is_some() {}
}

fn print_result(printer: &mut Printer, deletion: &Deletion, result: Result<(), String>) {
    let src = format!("s3://{}", deletion.path);
    match result {
        Ok(()) => printer.success(deletion.id, "delete", &src, None),
        Err(message) => printer.failure(deletion.id, "delete", &src, None, &message),
    }
}
