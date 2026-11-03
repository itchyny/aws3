//! The limit of the bandwidth shared by the transfers of a command, in the
//! same way as `max_bandwidth` of the AWS CLI.

use std::convert::Infallible;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use aws_sdk_s3::primitives::ByteStream;
use aws_smithy_types::body::SdkBody;
use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use tokio::time::{Instant, Sleep};

/// The size of the chunks to limit the bandwidth, which is the same as the
/// size of the reads of the AWS CLI.
const CHUNK_SIZE: usize = 256 * 1024;

/// Limits the bandwidth by scheduling the chunks one after another.
#[derive(Debug)]
pub struct Limiter {
    /// The bytes per second.
    rate: f64,
    /// The instant when the next chunk can be transferred.
    next: Mutex<Instant>,
}

impl Limiter {
    pub fn new(rate: u64) -> Self {
        Self {
            rate: rate as f64,
            next: Mutex::new(Instant::now()),
        }
    }

    /// Reserves the bytes, and returns the instant when they can be
    /// transferred.
    fn reserve(&self, bytes: usize) -> Instant {
        let mut next = self.next.lock().unwrap_or_else(|err| err.into_inner());
        let start = (*next).max(Instant::now());
        *next = start + Duration::from_secs_f64(bytes as f64 / self.rate);
        start
    }

    /// Waits until the bytes can be transferred.
    pub async fn consume(&self, bytes: usize) {
        tokio::time::sleep_until(self.reserve(bytes)).await;
    }

    /// Returns the body of the data to upload, which is transferred in chunks
    /// within the bandwidth, and can be retried.
    pub fn body(self: &Arc<Self>, data: Bytes) -> ByteStream {
        let limiter = self.clone();
        ByteStream::new(SdkBody::retryable(move || {
            SdkBody::from_body_1_x(LimitedBody {
                data: data.clone(),
                limiter: limiter.clone(),
                sleep: None,
            })
        }))
    }
}

/// The body which yields the data in chunks within the bandwidth.
struct LimitedBody {
    data: Bytes,
    limiter: Arc<Limiter>,
    /// The wait for the current chunk.
    sleep: Option<Pin<Box<Sleep>>>,
}

impl Body for LimitedBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        let this = self.get_mut();
        if this.data.is_empty() {
            return Poll::Ready(None);
        }
        let size = this.data.len().min(CHUNK_SIZE);
        let sleep = this
            .sleep
            .get_or_insert_with(|| Box::pin(tokio::time::sleep_until(this.limiter.reserve(size))));
        ready!(sleep.as_mut().poll(cx));
        this.sleep = None;
        Poll::Ready(Some(Ok(Frame::data(this.data.split_to(size)))))
    }

    fn is_end_stream(&self) -> bool {
        self.data.is_empty()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.data.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn limit() {
        let limiter = Limiter::new(1024 * 1024);
        let start = Instant::now();
        // The first chunk is transferred immediately, and the following
        // chunks wait for the bandwidth of the previous chunks.
        for _ in 0..5 {
            limiter.consume(512 * 1024).await;
        }
        assert_eq!(start.elapsed(), Duration::from_secs(2));
    }

    #[tokio::test(start_paused = true)]
    async fn body() {
        let limiter = Arc::new(Limiter::new(1024 * 1024));
        let start = Instant::now();
        let data = Bytes::from(vec![1; 3 * 1024 * 1024]);
        let collected = limiter.body(data.clone()).collect().await.unwrap();
        assert_eq!(collected.into_bytes(), data);
        // The last chunk starts after the bandwidth of the previous chunks.
        assert_eq!(start.elapsed(), Duration::from_millis(2750));
    }
}
