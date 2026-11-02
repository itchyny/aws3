//! An HTTP client which does not verify the certificates of the servers, for
//! `--no-verify-ssl`. The HTTP client of the SDK always verifies them.

use std::sync::Arc;
use std::time::Duration;

use aws_sdk_s3::config::SharedHttpClient;
use aws_smithy_runtime_api::client::http::{
    HttpConnector, HttpConnectorFuture, SharedHttpConnector, http_client_fn,
};
use aws_smithy_runtime_api::client::orchestrator::{HttpRequest, HttpResponse};
use aws_smithy_runtime_api::client::result::ConnectorError;
use aws_smithy_types::body::SdkBody;
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector as TcpConnector;
use hyper_util::rt::TokioExecutor;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, aws_lc_rs};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, SignatureScheme};

use crate::error::Error;

/// Creates an HTTP client which accepts any certificates of the servers.
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
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .wrap_connector(tcp);
    // The connector is shared to reuse the connections across the requests.
    let connector = SharedHttpConnector::new(Connector {
        client: Client::builder(TokioExecutor::new()).build(https),
        read_timeout,
    });
    Ok(http_client_fn(move |_, _| connector.clone()))
}

#[derive(Debug)]
struct Connector {
    client: Client<HttpsConnector<TcpConnector>, SdkBody>,
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
