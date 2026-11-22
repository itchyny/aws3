//! The shared state of the transfers.

use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, PoisonError};

use aws_sdk_s3::Client;
use aws_sdk_s3::config::http::HttpResponse;
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use tokio::sync::{Semaphore, SemaphorePermit, watch};
use tokio::task::JoinHandle;

use super::bandwidth;
use super::options::{CommandName, Options};
use super::params::RequestParams;
use super::plan::Plan;
use super::printer::{Printer, TransferId};
use crate::config::S3Config;
use crate::redirect::Redirector;

/// The shared state of the transfers.
pub(super) struct Context {
    pub(super) client: Client,
    pub(super) source_client: Client,
    pub(super) params: RequestParams,
    pub(super) options: Options,
    pub(super) plan: Plan,
    pub(super) config: S3Config,
    /// Limits the number of concurrent requests.
    pub(super) requests: Semaphore,
    pub(super) printer: Mutex<Printer>,
    pub(super) next_id: AtomicU64,
    /// Whether the command is cancelled by Ctrl-C.
    pub(super) cancel: watch::Sender<bool>,
    /// The tasks to clean up the cancelled transfers, e.g. to abort the
    /// multipart uploads.
    pub(super) cleanup: Mutex<Vec<JoinHandle<()>>>,
    /// The limit of the bandwidth of uploads and downloads.
    pub(super) limiter: Option<Arc<bandwidth::Limiter>>,
    /// The clients of the regions of the buckets found on redirects.
    pub(super) redirector: Redirector,
    /// Whether the deletions are sent in batches of `DeleteObjects`, which is
    /// disabled when the endpoint does not support it.
    pub(super) batch_delete: AtomicBool,
}

impl Context {
    /// Acquires the permit of a request, which should be held until the
    /// request including its body completes, to limit the concurrent requests
    /// by `max_concurrent_requests`.
    pub(super) async fn permit(&self) -> Result<SemaphorePermit<'_>, String> {
        self.requests.acquire().await.map_err(|err| err.to_string())
    }

    /// Sends the request with the client for the bucket under the permit of a
    /// request, which is released when the response is received. The requests
    /// with the bodies of the responses to read should hold the permit by
    /// [`Self::permit`] instead.
    #[expect(
        clippy::result_large_err,
        reason = "the errors of the SDK are returned as they are"
    )]
    pub(super) async fn send<T, E, F, Fut>(
        &self,
        client: &Client,
        bucket: &str,
        operation: &str,
        send: F,
    ) -> Result<T, SdkError<E, HttpResponse>>
    where
        F: Fn(Client) -> Fut,
        Fut: Future<Output = Result<T, SdkError<E, HttpResponse>>>,
        E: ProvideErrorMetadata,
    {
        let _permit = self
            .requests
            .acquire()
            .await
            .map_err(SdkError::construction_failure)?;
        self.redirector.send(client, bucket, operation, send).await
    }

    pub(super) fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    /// Waits until the command is cancelled.
    pub(super) async fn cancelled(&self) {
        let _ = self
            .cancel
            .subscribe()
            .wait_for(|&cancelled| cancelled)
            .await;
    }

    /// Cancels the command, which stops submitting and aborts the transfers.
    pub(super) fn cancel(&self) {
        if !self.cancel.send_replace(true) {
            self.printer().ctrl_c();
        }
    }

    /// Spawns the task to clean up a cancelled transfer, which the command
    /// waits for before exiting.
    pub(super) fn spawn_cleanup(&self, future: impl Future<Output = ()> + Send + 'static) {
        let handle = tokio::spawn(future);
        self.cleanup
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(handle);
    }

    pub(super) fn printer(&self) -> std::sync::MutexGuard<'_, Printer> {
        self.printer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn progress(&self, id: TransferId, bytes: u64) {
        self.printer().progress(id, bytes);
    }

    pub(super) fn warning(&self, message: &str) {
        self.printer().warning(message);
    }

    /// Waits until the bytes can be transferred within the bandwidth.
    pub(super) async fn limit(&self, bytes: usize) {
        if let Some(limiter) = &self.limiter {
            limiter.consume(bytes).await;
        }
    }

    pub(super) fn notice(&self, message: &str) {
        self.printer().notice(message);
    }

    pub(super) fn is_move(&self) -> bool {
        self.options.command == CommandName::Mv
    }
}
