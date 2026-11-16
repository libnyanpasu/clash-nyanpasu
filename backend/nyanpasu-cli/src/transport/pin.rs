//! Certificate pinning for `https` targets.
//!
//! A pinned context trusts exactly one certificate: the one whose SHA-256 fingerprint the user
//! compared with the app's remote access settings. Names, validity dates and certificate
//! authorities are not consulted; the fingerprint identifies the server. A changed certificate is
//! a hard error that names both fingerprints, and nothing, not even the bearer token, is sent
//! to a server that does not match.

use std::sync::{Arc, Mutex};

use nyanpasu_contracts::auth::TlsFingerprint;
use reqwest::Url;
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, Error as TlsError, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{
        WebPkiSupportedAlgorithms, aws_lc_rs, verify_tls12_signature, verify_tls13_signature,
    },
    pki_types::{CertificateDer, ServerName, UnixTime},
};

use crate::error::CliError;

/// What a pinned verifier saw when it refused a certificate.
pub(super) type Presented = Arc<Mutex<Option<TlsFingerprint>>>;

#[derive(Debug)]
struct PinnedCertificate {
    pin: TlsFingerprint,
    presented: Presented,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedCertificate {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        let presented = TlsFingerprint::of_certificate(end_entity.as_ref());
        if presented == self.pin {
            return Ok(ServerCertVerified::assertion());
        }
        *self
            .presented
            .lock()
            .expect("no holder of the mismatch slot panics") = Some(presented);
        Err(TlsError::InvalidCertificate(
            CertificateError::ApplicationVerificationFailure,
        ))
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// A TLS client configuration that accepts only `pin`, and where a refusal is recorded.
pub(super) fn pinned_config(pin: TlsFingerprint) -> Result<(ClientConfig, Presented), CliError> {
    let provider = Arc::new(aws_lc_rs::default_provider());
    let presented = Presented::default();
    let verifier = PinnedCertificate {
        pin,
        presented: presented.clone(),
        algorithms: provider.signature_verification_algorithms,
    };
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| CliError::failed(format!("cannot configure TLS: {error}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    Ok((config, presented))
}

/// The fingerprint of the certificate `base` presents, without trusting it or sending anything
/// but a request for the public login page.
///
/// # Errors
/// The server cannot be reached or does not speak TLS.
pub async fn probe_certificate(base: &Url) -> Result<TlsFingerprint, CliError> {
    let unreachable = |detail: String| CliError::Unreachable {
        target: base.to_string(),
        detail,
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(super::http::CONNECT_TIMEOUT)
        .tls_danger_accept_invalid_certs(true)
        .tls_info(true)
        .build()
        .map_err(|error| CliError::failed(format!("cannot build the HTTP client: {error}")))?;
    let login = base
        .join("login")
        .map_err(|error| CliError::failed(format!("invalid request URL: {error}")))?;
    let response = client
        .get(login)
        .send()
        .await
        .map_err(|error| unreachable(super::error_chain(&error)))?;
    let info = response
        .extensions()
        .get::<reqwest::tls::TlsInfo>()
        .and_then(|info| info.peer_certificate())
        .ok_or_else(|| unreachable("the server did not present a TLS certificate".to_owned()))?;
    Ok(TlsFingerprint::of_certificate(info))
}
