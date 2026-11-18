//! Certificate pinning for `https` targets.
//!
//! The app serves HTTPS with a short-lived certificate issued by its own local root, and sends the
//! root after it. A pinned context trusts exactly one root: the one whose SHA-256 fingerprint the
//! user compared with the app's remote access settings. The server's certificate must be signed
//! by that root and be within its validity; names are not consulted, because the pin identifies
//! the server. Renewing the certificate keeps the root, so a pin outlives renewals. A different
//! root is a hard error that names both fingerprints, and nothing, not even the bearer token, is
//! sent to a server that does not match.

use std::sync::{Arc, Mutex};

use nyanpasu_contracts::auth::TlsFingerprint;
use reqwest::Url;
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, Error as TlsError, RootCertStore,
    SignatureScheme,
    client::{
        WebPkiServerVerifier,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    crypto::{
        CryptoProvider, WebPkiSupportedAlgorithms, aws_lc_rs, verify_tls12_signature,
        verify_tls13_signature,
    },
    pki_types::{CertificateDer, ServerName, UnixTime},
};

use crate::error::CliError;

/// What a pinned verifier saw when it refused a certificate.
pub(super) type Presented = Arc<Mutex<Option<TlsFingerprint>>>;

/// The root of a presented chain: the last certificate after the server's own, or the server's
/// own when it sends nothing else.
fn presented_root(
    end_entity: &CertificateDer<'_>,
    intermediates: &[CertificateDer<'_>],
) -> TlsFingerprint {
    TlsFingerprint::of_certificate(intermediates.last().unwrap_or(end_entity).as_ref())
}

#[derive(Debug)]
struct PinnedAuthority {
    pin: TlsFingerprint,
    presented: Presented,
    provider: Arc<CryptoProvider>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl PinnedAuthority {
    fn refuse(
        &self,
        presented: TlsFingerprint,
        error: TlsError,
    ) -> Result<ServerCertVerified, TlsError> {
        *self
            .presented
            .lock()
            .expect("no holder of the mismatch slot panics") = Some(presented);
        Err(error)
    }
}

impl ServerCertVerifier for PinnedAuthority {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        let Some(root) = intermediates
            .iter()
            .find(|certificate| TlsFingerprint::of_certificate(certificate.as_ref()) == self.pin)
        else {
            return self.refuse(
                presented_root(end_entity, intermediates),
                TlsError::InvalidCertificate(CertificateError::ApplicationVerificationFailure),
            );
        };
        let mut roots = RootCertStore::empty();
        roots.add(root.clone().into_owned())?;
        let verifier =
            WebPkiServerVerifier::builder_with_provider(Arc::new(roots), self.provider.clone())
                .build()
                .map_err(|error| TlsError::General(error.to_string()))?;
        match verifier.verify_server_cert(end_entity, &[], server_name, ocsp_response, now) {
            Ok(verified) => Ok(verified),
            // The pin identifies the server; the name it was reached by does not matter.
            Err(TlsError::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            Err(error) => self.refuse(self.pin, error),
        }
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

/// A TLS client configuration that accepts only certificates `pin` issued, and where a refusal is
/// recorded.
pub(super) fn pinned_config(pin: TlsFingerprint) -> Result<(ClientConfig, Presented), CliError> {
    let provider = Arc::new(aws_lc_rs::default_provider());
    let presented = Presented::default();
    let verifier = PinnedAuthority {
        pin,
        presented: presented.clone(),
        provider: provider.clone(),
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

/// Accepts any chain and records its root, for a probe that sends nothing but a request for the
/// public login page.
#[derive(Debug)]
struct RecordRoot {
    root: Presented,
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for RecordRoot {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        *self.root.lock().expect("no holder of the root slot panics") =
            Some(presented_root(end_entity, intermediates));
        Ok(ServerCertVerified::assertion())
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

/// The fingerprint of the root `base` presents its certificate with, without trusting it or
/// sending anything but a request for the public login page.
///
/// # Errors
/// The server cannot be reached or does not speak TLS.
pub async fn probe_certificate(base: &Url) -> Result<TlsFingerprint, CliError> {
    let unreachable = |detail: String| CliError::Unreachable {
        target: base.to_string(),
        detail,
    };
    let provider = Arc::new(aws_lc_rs::default_provider());
    let root = Presented::default();
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|error| CliError::failed(format!("cannot configure TLS: {error}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(RecordRoot {
            root: root.clone(),
            algorithms: provider.signature_verification_algorithms,
        }))
        .with_no_client_auth();
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(super::http::CONNECT_TIMEOUT)
        .tls_backend_preconfigured(config)
        .build()
        .map_err(|error| CliError::failed(format!("cannot build the HTTP client: {error}")))?;
    let login = base
        .join("login")
        .map_err(|error| CliError::failed(format!("invalid request URL: {error}")))?;
    client
        .get(login)
        .send()
        .await
        .map_err(|error| unreachable(super::error_chain(&error)))?;
    root.lock()
        .expect("no holder of the root slot panics")
        .take()
        .ok_or_else(|| unreachable("the server did not present a TLS certificate".to_owned()))
}
