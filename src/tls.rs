use std::path::Path;

use aws_sdk_s3::config::SharedHttpClient;
use aws_smithy_http_client::proxy::ProxyConfig;
use aws_smithy_http_client::tls::rustls_provider::CryptoMode;
use aws_smithy_http_client::tls::{Provider, TlsContext, TrustStore};
use aws_smithy_http_client::{Builder, Connector};
use aws_smithy_types::base64;
use rustls_pki_types::CertificateDer;
use rustls_pki_types::pem::PemObject;

use crate::error::Error;

/// Creates an HTTP client trusting the certificates in the CA bundle, or the
/// Mozilla root certificates like the AWS CLI. Loading the certificates of the
/// platform is avoided, since it takes tens of milliseconds on macOS. The
/// requests are sent through the proxies of the environment variables like the
/// AWS CLI, which the client of the SDK does not by default.
pub fn http_client(ca_bundle: Option<&Path>) -> Result<SharedHttpClient, Error> {
    let certs = match ca_bundle {
        Some(path) => load_ca_bundle(path)?,
        None => webpki_root_certs::TLS_SERVER_ROOT_CERTS.to_vec(),
    };
    let trust_store = TrustStore::empty().with_pem_certificate(encode_pem(&certs));
    let tls_context = TlsContext::builder()
        .with_trust_store(trust_store)
        .build()
        .map_err(|err| Error::new(err.to_string()))?;
    // The connectors are built like the client of the SDK with the timeouts
    // and the sleep of the runtime, which the client caches by the settings.
    Ok(
        Builder::new().build_with_connector_fn(move |settings, components| {
            let mut builder = Connector::builder()
                .tls_provider(Provider::Rustls(CryptoMode::AwsLc))
                .tls_context(tls_context.clone())
                .proxy_config(ProxyConfig::from_env());
            builder.set_connector_settings(settings.cloned());
            builder.set_sleep_impl(components.and_then(|components| components.sleep_impl()));
            builder.build()
        }),
    )
}

/// Loads the certificates in the CA bundle, which are validated here because
/// the TLS context panics on invalid certificates.
fn load_ca_bundle(path: &Path) -> Result<Vec<CertificateDer<'static>>, Error> {
    let error = |message: &dyn std::fmt::Display| {
        Error::new(format!(
            "Unable to load CA bundle {}: {message}",
            path.display()
        ))
    };
    let pem = std::fs::read(path).map_err(|err| error(&err))?;
    let certs = CertificateDer::pem_slice_iter(&pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| error(&err))?;
    if certs.is_empty() {
        return Err(error(&"no certificate found"));
    }
    for cert in &certs {
        webpki::anchor_from_trusted_cert(cert).map_err(|err| error(&err))?;
    }
    Ok(certs)
}

fn encode_pem(certs: &[CertificateDer<'_>]) -> String {
    let mut pem = String::new();
    for cert in certs {
        pem.push_str("-----BEGIN CERTIFICATE-----\n");
        let encoded = base64::encode(cert);
        for line in encoded.as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(line).unwrap());
            pem.push('\n');
        }
        pem.push_str("-----END CERTIFICATE-----\n");
    }
    pem
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_pem_roundtrip() {
        let certs = webpki_root_certs::TLS_SERVER_ROOT_CERTS;
        let pem = encode_pem(certs);
        let decoded = CertificateDer::pem_slice_iter(pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(decoded, certs);
    }

    #[test]
    fn invalid_ca_bundle() {
        let dir = std::env::temp_dir();
        let missing = dir.join("aws3-test-missing.pem");
        assert!(load_ca_bundle(&missing).is_err());

        let empty = dir.join(format!("aws3-test-empty-{}.pem", std::process::id()));
        std::fs::write(&empty, "garbage\n").unwrap();
        let result = load_ca_bundle(&empty);
        std::fs::remove_file(&empty).unwrap();
        assert!(result.is_err());
    }
}
