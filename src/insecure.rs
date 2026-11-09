//! An HTTP client which does not verify the certificates of the servers, for
//! `--no-verify-ssl`. The HTTP client of the SDK always verifies them.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use aws_sdk_s3::config::SharedHttpClient;
use aws_smithy_runtime_api::client::http::{
    HttpConnector, HttpConnectorFuture, SharedHttpConnector, http_client_fn,
};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_types::body::SdkBody;
use http::Uri;
use hyper_rustls::{HttpsConnector, MaybeHttpsStream};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector as TcpConnector;
use hyper_util::client::legacy::connect::proxy::Tunnel;
use hyper_util::client::proxy::matcher::Matcher;
use hyper_util::rt::{TokioExecutor, TokioIo};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, aws_lc_rs};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};
use tokio::net::TcpStream;
use tower_service::Service;

use crate::error::Error;

/// Creates an HTTP client which accepts any certificates of the servers, and
/// sends the requests through the proxies of the environment variables.
pub fn http_client(
    connect_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
) -> Result<SharedHttpClient, Error> {
    let provider = Arc::new(aws_lc_rs::default_provider());
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|err| Error::new(err.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerifier(provider)))
        .with_no_client_auth();
    let mut tcp = TcpConnector::new();
    tcp.enforce_http(false);
    tcp.set_connect_timeout(connect_timeout);
    // The connections to the proxies of HTTPS are also not verified, and use
    // HTTP/1.1 for the CONNECT requests.
    let proxy = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config.clone())
        .https_or_http()
        .enable_http1()
        .wrap_connector(tcp.clone());
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .wrap_connector(ProxyConnector {
            tcp,
            proxy,
            matcher: Arc::new(Matcher::from_env()),
        });
    // The connector is shared to reuse the connections across the requests.
    let connector = SharedHttpConnector::new(Connector {
        client: Client::builder(TokioExecutor::new()).build(https),
        read_timeout,
    });
    Ok(http_client_fn(move |_, _| connector.clone()))
}

#[derive(Debug)]
struct Connector {
    client: Client<HttpsConnector<ProxyConnector>, SdkBody>,
    read_timeout: Option<Duration>,
}

impl HttpConnector for Connector {
    fn call(&self, request: HttpRequest) -> HttpConnectorFuture {
        let client = self.client.clone();
        let read_timeout = self.read_timeout;
        HttpConnectorFuture::new(async move {
            let request = request
                .try_into_http1x()
                .map_err(|err| ConnectorError::user(err.into()))?;
            let response = client.request(request);
            let response = match read_timeout {
                Some(timeout) => tokio::time::timeout(timeout, response)
                    .await
                    .map_err(|err| ConnectorError::timeout(err.into()))?,
                None => response.await,
            };
            let response = response.map_err(|err| {
                if err.is_connect() {
                    ConnectorError::io(err.into())
                } else {
                    ConnectorError::other(err.into(), None)
                }
            })?;
            HttpResponse::try_from(response.map(SdkBody::from_body_1_x))
                .map_err(|err| ConnectorError::other(err.into(), None))
        })
    }
}

/// Connects to the servers directly or through the CONNECT tunnels of the
/// proxies, over which the TLS connections are established.
#[derive(Clone, Debug)]
struct ProxyConnector {
    tcp: TcpConnector,
    proxy: HttpsConnector<TcpConnector>,
    matcher: Arc<Matcher>,
}

type BoxError = Box<dyn std::error::Error + Send + Sync>;

impl Service<Uri> for ProxyConnector {
    type Response = MaybeHttpsStream<TokioIo<TcpStream>>;
    type Error = BoxError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.tcp.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        match self.matcher.intercept(&dst) {
            Some(intercept) => {
                let mut tunnel = Tunnel::new(intercept.uri().clone(), self.proxy.clone());
                if let Some(auth) = intercept.basic_auth() {
                    tunnel = tunnel.with_auth(auth.clone());
                }
                Box::pin(async move { Ok(tunnel.call(dst).await?) })
            }
            None => {
                let connecting = self.tcp.call(dst);
                Box::pin(async move { Ok(MaybeHttpsStream::Http(connecting.await?)) })
            }
        }
    }
}

/// Accepts any certificates, while the signatures are verified with the
/// crypto provider.
#[derive(Debug)]
struct NoVerifier(Arc<CryptoProvider>);

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}
