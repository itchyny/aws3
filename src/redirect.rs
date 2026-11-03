//! The redirects of the requests to the regions of the buckets, in the same
//! way as `S3RegionRedirectorv2` of botocore.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use aws_sdk_s3::Client;
use aws_sdk_s3::config::Region;
use aws_sdk_s3::config::http::HttpResponse;
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};

/// The clients of the regions of the buckets, which are found on redirects
/// and used for the following requests to the buckets.
#[derive(Debug, Default)]
pub struct Redirector {
    clients: Mutex<HashMap<String, Client>>,
}

impl Redirector {
    /// Returns the client for the bucket, which is of the region of the
    /// bucket if the requests to it were redirected.
    pub fn client(&self, client: &Client, bucket: &str) -> Client {
        let clients = self.clients.lock().unwrap_or_else(PoisonError::into_inner);
        clients.get(bucket).unwrap_or(client).clone()
    }

    /// Sends the request with the client for the bucket, and sends it again
    /// to the region of the bucket if redirected. The errors of the SDK are
    /// returned as they are.
    #[allow(clippy::result_large_err)]
    pub async fn send<T, E, F, Fut>(
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
        let result = send(self.client(client, bucket)).await;
        let Err(err) = &result else {
            return result;
        };
        match self.redirect(client, bucket, operation, err).await {
            Some(client) => send(client).await,
            None => result,
        }
    }

    /// Returns the client of the region of the bucket if the error is caused
    /// by the request to another region, which is used for the bucket later.
    pub async fn redirect<E: ProvideErrorMetadata>(
        &self,
        client: &Client,
        bucket: &str,
        operation: &str,
        err: &SdkError<E, HttpResponse>,
    ) -> Option<Client> {
        if bucket.starts_with("arn:") || !is_redirect(err, operation) {
            return None;
        }
        let region = match err.raw_response().and_then(response_region) {
            Some(region) => region,
            None => head_bucket_region(client, bucket).await?,
        };
        let config = client.config().to_builder().region(Region::new(region));
        let client = Client::from_conf(config.build());
        let mut clients = self.clients.lock().unwrap_or_else(PoisonError::into_inner);
        clients.insert(bucket.to_owned(), client.clone());
        Some(client)
    }
}

/// Returns whether the error is caused by the request to another region, in
/// the same way as `is_redirect_response` of botocore.
fn is_redirect<E: ProvideErrorMetadata>(err: &SdkError<E, HttpResponse>, operation: &str) -> bool {
    let Some(response) = err.raw_response() else {
        return false;
    };
    let status = response.status().as_u16();
    let has_region = response.headers().contains_key("x-amz-bucket-region");
    match err.code() {
        Some("PermanentRedirect") => true,
        Some("AuthorizationHeaderMalformed") => body_region(response).is_some(),
        Some("IllegalLocationConstraintException") => operation != "CreateBucket",
        _ => {
            matches!(status, 301 | 302 | 307)
                || matches!(status, 400)
                    && (operation == "HeadObject" || operation == "HeadBucket" && has_region)
        }
    }
}

/// Returns the region of the bucket in the header or the body of the response.
fn response_region(response: &HttpResponse) -> Option<String> {
    match response.headers().get("x-amz-bucket-region") {
        Some(region) => Some(region.to_owned()),
        None => body_region(response),
    }
}

/// Returns the region in the error body, e.g. `<Region>us-west-2</Region>`.
fn body_region(response: &HttpResponse) -> Option<String> {
    let body = std::str::from_utf8(response.body().bytes()?).ok()?;
    let (_, rest) = body.split_once("<Region>")?;
    let (region, _) = rest.split_once("</Region>")?;
    (!region.is_empty()).then(|| region.to_owned())
}

/// Returns the region of the bucket by HeadBucket, which responds with the
/// region even on errors.
async fn head_bucket_region(client: &Client, bucket: &str) -> Option<String> {
    match client.head_bucket().bucket(bucket).send().await {
        Ok(output) => output.bucket_region().map(str::to_owned),
        Err(err) => err.raw_response().and_then(response_region),
    }
}
