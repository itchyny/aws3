//! The deletions of the objects, where the recursive deletions are sent in the
//! batches of `DeleteObjects` instead of a `DeleteObject` for each object.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::Duration;

use aws_sdk_s3::Client;
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::types::{Delete, ObjectIdentifier};
use aws_smithy_types::base64;
use md5::{Digest, Md5};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::context::Context;
use super::download::etag_mismatch;
use super::printer::{Printer, TransferId};
use super::task::Outcome;
use crate::error::{Error, has_error_code, has_status_code, is_retryable};
use crate::path::{split_bucket_key, validate_bucket_name};

/// The maximum number of the keys in a request of `DeleteObjects`.
pub(super) const MAX_BATCH_SIZE: usize = 1000;

/// The maximum number of the batches of `DeleteObjects` sent at once. S3
/// deletes about 3500 objects per second for a prefix, and deletes a batch of
/// 1000 objects in about a second, so more batches are throttled without
/// deleting the objects faster.
pub(super) const MAX_BATCHES: usize = 4;

/// The number of the batches of `DeleteObjects` sent at once, which is halved
/// on the throttling until the batches are not throttled, where the permits
/// of the slots exceeding it are forgotten at once if they are available, or
/// as they are released.
pub(super) struct DeleteRate {
    limit: usize,
    /// The number of the permits to forget.
    debt: usize,
    /// The number of the times the limit is halved, to halve it once for the
    /// batches throttled at the same limit.
    epoch: usize,
}

impl DeleteRate {
    pub(super) fn new(limit: usize) -> Self {
        Self {
            limit,
            debt: 0,
            epoch: 0,
        }
    }

    /// Halves the limit on the throttling of the batch sent in the epoch, and
    /// returns whether the throttling counts as an attempt of the batch,
    /// which is when the batches are already sent one at a time.
    fn throttled(&mut self, epoch: usize, slots: &Semaphore) -> bool {
        if epoch != self.epoch {
            return false;
        }
        if self.limit == 1 {
            return true;
        }
        let limit = self.limit / 2;
        self.debt += self.limit - limit - slots.forget_permits(self.limit - limit);
        self.limit = limit;
        self.epoch += 1;
        false
    }
}

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

/// Deletes the objects of the bucket in requests of `DeleteObjects`, and
/// prints the results of the objects in the same way as the deletions of each
/// object. The objects are deleted one by one if the endpoint does not support
/// it.
///
/// The objects not deleted by the throttling or the transient errors, of the
/// request after the retries of the SDK or of each object in the response, are
/// sent again in a batch, as many times as the attempts of the requests, since
/// some of them may have been deleted. The number of the batches sent at once
/// is halved on the throttling, to keep the rate of the deletions under the
/// limit of S3 for a prefix, and the throttling caused by sending too many
/// batches at once is not counted as an attempt.
pub(super) async fn delete_batch(ctx: Arc<Context>, bucket: String, deletions: Vec<Deletion>) {
    if !ctx.batch_delete.load(Ordering::Relaxed) {
        return delete_each(ctx, deletions).await;
    }
    let max_attempts = ctx
        .client
        .config()
        .retry_config()
        .map_or(1, |config| config.max_attempts());
    let mut deletions = deletions;
    let mut attempt = 1;
    loop {
        let keys: Vec<_> = deletions
            .iter()
            .map(|deletion| split_bucket_key(&deletion.path).1.to_owned())
            .collect();
        let result = async {
            let (ctx, bucket) = (&*ctx, bucket.as_str());
            validate_bucket_name(bucket)?;
            let objects = keys
                .iter()
                .map(|key| ObjectIdentifier::builder().key(key).build())
                .collect::<Result<Vec<_>, _>>()
                .map_err(|err| err.to_string())?;
            // The successful deletions are omitted from the response in quiet mode.
            let delete = Delete::builder()
                .set_objects(Some(objects))
                .quiet(true)
                .build()
                .map_err(|err| err.to_string())?;
            let slot = ctx
                .delete_slots
                .acquire()
                .await
                .map_err(|err| err.to_string())?;
            let epoch = ctx
                .delete_rate
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .epoch;
            let result = ctx
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
                .await;
            Ok::<_, String>((result, slot, epoch))
        }
        .await;
        let (result, slot, epoch) = match result {
            Ok(result) => result,
            Err(message) => {
                let mut printer = ctx.printer();
                for deletion in &deletions {
                    print_result(&mut printer, deletion, Err(message.clone()));
                }
                return;
            }
        };
        // The objects to send again with the errors, and whether they are
        // throttled, where the errors of the objects have no status codes, and
        // InternalError and ServiceUnavailable are of 500 and 503.
        let mut retries = Vec::new();
        let mut throttled = false;
        match result {
            Ok(output) => {
                // The errors are reported with the operation of the deletion
                // of each object, as the AWS CLI deletes the objects one by one.
                let errors: HashMap<_, _> = output
                    .errors()
                    .iter()
                    .filter_map(|err| Some((err.key()?, err)))
                    .collect();
                let mut printer = ctx.printer();
                for (deletion, key) in deletions.into_iter().zip(&keys) {
                    let Some(err) = errors.get(key.as_str()) else {
                        print_result(&mut printer, &deletion, Ok(()));
                        continue;
                    };
                    let code = err.code().unwrap_or("Unknown");
                    let message = format!(
                        "An error occurred ({code}) when calling the DeleteObject operation: {}",
                        err.message().unwrap_or("Unknown"),
                    );
                    if is_retryable(0, code)
                        || matches!(code, "InternalError" | "ServiceUnavailable")
                    {
                        throttled |= is_retryable(0, code) || code == "ServiceUnavailable";
                        retries.push((deletion, message));
                    } else {
                        print_result(&mut printer, &deletion, Err(message));
                    }
                }
            }
            Err(err) if has_error_code(&err, "NotImplemented") || has_status_code(&err, 501) => {
                drop(slot);
                ctx.batch_delete.store(false, Ordering::Relaxed);
                return delete_each(ctx.clone(), deletions).await;
            }
            Err(err) => {
                let (retryable, throttling) = match &err {
                    SdkError::ServiceError(err) => {
                        let (status, code) = (err.raw().status().as_u16(), err.err().code());
                        let code = code.unwrap_or_default();
                        (
                            is_retryable(status, code),
                            status == 503 || is_retryable(0, code),
                        )
                    }
                    _ => (false, false),
                };
                let message = Error::sdk("DeleteObject", err).to_string();
                if retryable {
                    throttled = throttling;
                    retries = deletions
                        .into_iter()
                        .map(|deletion| (deletion, message.clone()))
                        .collect();
                } else {
                    let mut printer = ctx.printer();
                    for deletion in &deletions {
                        print_result(&mut printer, deletion, Err(message.clone()));
                    }
                }
            }
        }
        let counted = {
            let mut rate = ctx
                .delete_rate
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let counted = !throttled || rate.throttled(epoch, &ctx.delete_slots);
            if rate.debt > 0 {
                rate.debt -= 1;
                slot.forget();
            } else {
                drop(slot);
            }
            counted
        };
        if retries.is_empty() {
            return;
        }
        if counted && attempt >= max_attempts {
            let mut printer = ctx.printer();
            for (deletion, message) in retries {
                print_result(&mut printer, &deletion, Err(message));
            }
            return;
        }
        deletions = retries.into_iter().map(|(deletion, _)| deletion).collect();
        let backoff = Duration::from_millis(500) * (1 << (attempt - 1).min(4));
        tokio::time::sleep(backoff).await;
        if counted {
            attempt += 1;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_rate() {
        let (mut rate, slots) = (DeleteRate::new(8), Semaphore::new(8));
        let held = slots.try_acquire_many(3).unwrap();
        // The batches sent at the same limit halve it once without counting
        // the throttling, where the available permits are forgotten at once
        // and the held ones are owed.
        assert!(!rate.throttled(0, &slots));
        assert!(!rate.throttled(0, &slots));
        assert_eq!((rate.limit, rate.debt, rate.epoch), (4, 0, 1));
        assert_eq!(slots.available_permits(), 1);
        assert!(!rate.throttled(1, &slots));
        assert!(!rate.throttled(2, &slots));
        assert_eq!((rate.limit, rate.debt, rate.epoch), (1, 2, 3));
        assert_eq!(slots.available_permits(), 0);
        // The throttling of one batch at a time counts as an attempt.
        assert!(rate.throttled(3, &slots));
        assert_eq!((rate.limit, rate.debt, rate.epoch), (1, 2, 3));
        drop(held);
    }
}
