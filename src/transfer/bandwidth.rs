//! The limit of the bandwidth shared by the transfers of a command, in the
//! same way as `max_bandwidth` of the AWS CLI.

use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, ready};
use std::time::Duration;

use aws_sdk_s3::primitives::ByteStream;
use aws_smithy_types::body::{Error as BoxError, SdkBody};
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

    /// Returns the body of the data to upload within the bandwidth.
    pub fn body(self: &Arc<Self>, data: Bytes) -> ByteStream {
        self.stream(ByteStream::from(data))
    }

    /// Wraps the stream to upload, which is transferred in chunks within the
    /// bandwidth, and can be retried if the stream can be.
    pub fn stream(self: &Arc<Self>, stream: ByteStream) -> ByteStream {
        let limiter = self.clone();
        stream.map(move |body| {
            SdkBody::from_body_1_x(LimitedBody {
                inner: body,
                pending: Bytes::new(),
                limiter: limiter.clone(),
                sleep: None,
            })
        })
    }
}

/// The body which yields the data of the inner body in chunks within the
/// bandwidth.
struct LimitedBody {
    inner: SdkBody,
    /// The data read from the inner body but not yielded yet.
    pending: Bytes,
    limiter: Arc<Limiter>,
    /// The wait for the current chunk.
    sleep: Option<Pin<Box<Sleep>>>,
}

impl Body for LimitedBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = self.get_mut();
        while this.pending.is_empty() {
            match ready!(Pin::new(&mut this.inner).poll_frame(cx)) {
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => this.pending = data,
                    Err(frame) => return Poll::Ready(Some(Ok(frame))),
                },
                Some(Err(err)) => return Poll::Ready(Some(Err(err))),
                None => return Poll::Ready(None),
            }
        }
        let size = this.pending.len().min(CHUNK_SIZE);
        let sleep = this
            .sleep
            .get_or_insert_with(|| Box::pin(tokio::time::sleep_until(this.limiter.reserve(size))));
        ready!(sleep.as_mut().poll(cx));
        this.sleep = None;
        Poll::Ready(Some(Ok(Frame::data(this.pending.split_to(size)))))
    }

    fn is_end_stream(&self) -> bool {
        self.pending.is_empty() && self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        let mut hint = Body::size_hint(&self.inner);
        let pending = self.pending.len() as u64;
        hint.set_lower(hint.lower() + pending);
        if let Some(upper) = hint.upper() {
            hint.set_upper(upper + pending);
        }
        hint
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
