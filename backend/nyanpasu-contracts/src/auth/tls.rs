//! The HTTP listener's certificate authority, the short-lived certificates it issues, their public
//! fingerprints and the generator port.
//!
//! A long-lived local root ([`TlsAuthority`]) is what clients trust or pin. The listener presents
//! a short-lived certificate it issued for the host's current addresses ([`TlsIdentity`]), so new
//! addresses or a renewal never change what clients trust. Private keys live only in these two
//! types, which have no `Serialize` and redact `Debug`. Everything a client or the settings page
//! may see is [`TlsFingerprint`] and [`TlsCertificateSummary`].

use std::{fmt, net::IpAddr, str::FromStr};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use snafu::Snafu;
use specta::Type;
use zeroize::Zeroizing;

/// SHA-256 of a certificate's DER encoding, the value clients pin.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TlsFingerprint([u8; 32]);

/// A fingerprint that is not 64 hexadecimal digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Snafu)]
#[snafu(display(
    "a certificate fingerprint is 64 hexadecimal digits, optionally separated by `:`"
))]
pub struct TlsFingerprintError;

impl TlsFingerprint {
    /// The fingerprint of a DER-encoded certificate.
    #[must_use]
    pub fn of_certificate(certificate_der: &[u8]) -> Self {
        Self(Sha256::digest(certificate_der).into())
    }

    /// The raw digest.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for TlsFingerprint {
    /// Upper-case hexadecimal pairs separated by `:`, the form browsers and `openssl` print.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.0.iter().enumerate() {
            if index > 0 {
                f.write_str(":")?;
            }
            write!(f, "{byte:02X}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for TlsFingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TlsFingerprint({self})")
    }
}

impl FromStr for TlsFingerprint {
    type Err = TlsFingerprintError;

    /// Accepts the digits in either case, with or without `:` between byte pairs.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let digits: String = text.trim().chars().filter(|c| *c != ':').collect();
        let mut digest = [0u8; 32];
        if digits.len() != 64 || !digits.is_ascii() {
            return Err(TlsFingerprintError);
        }
        for (byte, pair) in digest.iter_mut().zip(digits.as_bytes().chunks(2)) {
            let pair = std::str::from_utf8(pair).map_err(|_| TlsFingerprintError)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| TlsFingerprintError)?;
        }
        Ok(Self(digest))
    }
}

impl TryFrom<String> for TlsFingerprint {
    type Error = TlsFingerprintError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<TlsFingerprint> for String {
    fn from(value: TlsFingerprint) -> Self {
        value.to_string()
    }
}

impl Type for TlsFingerprint {
    fn definition(types: &mut specta::Types) -> specta::datatype::DataType {
        String::definition(types)
    }
}

/// What the settings page and the CLI may show of the certificate: nothing secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TlsCertificateSummary {
    /// The fingerprint of the certificate authority, the value to compare in a browser warning or
    /// to pin in a CLI context. Renewing the listener's certificate keeps it.
    pub fingerprint: TlsFingerprint,
    /// When the listener's current certificate expires, in Unix epoch milliseconds. It is renewed
    /// before then.
    pub not_after_ms: i64,
}

/// The local certificate authority: a self-signed root and its private key.
#[derive(Clone)]
pub struct TlsAuthority {
    certificate_der: Vec<u8>,
    private_key_der: Zeroizing<Vec<u8>>,
    not_after_ms: i64,
}

impl TlsAuthority {
    /// Assembles an authority from its DER certificate, its PKCS#8 DER private key and its expiry.
    #[must_use]
    pub fn new(certificate_der: Vec<u8>, private_key_der: Vec<u8>, not_after_ms: i64) -> Self {
        Self {
            certificate_der,
            private_key_der: Zeroizing::new(private_key_der),
            not_after_ms,
        }
    }

    /// The DER-encoded root certificate, which clients trust.
    #[must_use]
    pub fn certificate_der(&self) -> &[u8] {
        &self.certificate_der
    }

    /// The PKCS#8 DER private key that signs the listener's certificates.
    #[must_use]
    pub fn private_key_der(&self) -> &[u8] {
        &self.private_key_der
    }

    /// When the root expires, in Unix epoch milliseconds.
    #[must_use]
    pub const fn not_after_ms(&self) -> i64 {
        self.not_after_ms
    }

    /// The pinnable fingerprint of the root.
    #[must_use]
    pub fn fingerprint(&self) -> TlsFingerprint {
        TlsFingerprint::of_certificate(&self.certificate_der)
    }
}

impl fmt::Debug for TlsAuthority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsAuthority")
            .field("fingerprint", &self.fingerprint())
            .field("not_after_ms", &self.not_after_ms)
            .field("private_key_der", &"[REDACTED]")
            .finish()
    }
}

/// A certificate the authority issued for the listener, with its private key.
#[derive(Clone)]
pub struct TlsIdentity {
    certificate_der: Vec<u8>,
    private_key_der: Zeroizing<Vec<u8>>,
    authority_der: Vec<u8>,
    dns_names: Vec<String>,
    addresses: Vec<IpAddr>,
    not_after_ms: i64,
}

impl TlsIdentity {
    /// Assembles an identity from a DER certificate, its PKCS#8 DER private key, the DER root that
    /// issued it, the DNS names and IP addresses the certificate covers and its expiry.
    #[must_use]
    pub fn new(
        certificate_der: Vec<u8>,
        private_key_der: Vec<u8>,
        authority_der: Vec<u8>,
        dns_names: Vec<String>,
        addresses: Vec<IpAddr>,
        not_after_ms: i64,
    ) -> Self {
        Self {
            certificate_der,
            private_key_der: Zeroizing::new(private_key_der),
            authority_der,
            dns_names,
            addresses,
            not_after_ms,
        }
    }

    /// The DER root that issued the certificate, sent after it so clients can build the chain.
    #[must_use]
    pub fn authority_der(&self) -> &[u8] {
        &self.authority_der
    }

    /// The fingerprint of the issuing root, which clients pin.
    #[must_use]
    pub fn authority_fingerprint(&self) -> TlsFingerprint {
        TlsFingerprint::of_certificate(&self.authority_der)
    }

    /// The DNS names the certificate covers, lower case. A request may name the server by one
    /// of them or by an IP address.
    #[must_use]
    pub fn dns_names(&self) -> &[String] {
        &self.dns_names
    }

    /// The IP addresses the certificate covers.
    #[must_use]
    pub fn addresses(&self) -> &[IpAddr] {
        &self.addresses
    }

    /// Whether a client that connects to `address` can validate the certificate against it.
    #[must_use]
    pub fn covers_address(&self, address: IpAddr) -> bool {
        self.addresses.contains(&address)
    }

    /// The DER-encoded certificate.
    #[must_use]
    pub fn certificate_der(&self) -> &[u8] {
        &self.certificate_der
    }

    /// The PKCS#8 DER private key.
    #[must_use]
    pub fn private_key_der(&self) -> &[u8] {
        &self.private_key_der
    }

    /// When the certificate expires, in Unix epoch milliseconds.
    #[must_use]
    pub const fn not_after_ms(&self) -> i64 {
        self.not_after_ms
    }

    /// The fingerprint of this certificate, which changes with every renewal.
    #[must_use]
    pub fn fingerprint(&self) -> TlsFingerprint {
        TlsFingerprint::of_certificate(&self.certificate_der)
    }

    /// The non-secret view: the root's fingerprint and this certificate's expiry.
    #[must_use]
    pub fn summary(&self) -> TlsCertificateSummary {
        TlsCertificateSummary {
            fingerprint: self.authority_fingerprint(),
            not_after_ms: self.not_after_ms,
        }
    }
}

impl fmt::Debug for TlsIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsIdentity")
            .field("fingerprint", &self.fingerprint())
            .field("authority", &self.authority_fingerprint())
            .field("not_after_ms", &self.not_after_ms)
            .field("private_key_der", &"[REDACTED]")
            .finish()
    }
}

/// How long a new certificate is valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TlsCertificateRequest {
    /// Start of validity, in Unix epoch milliseconds.
    pub not_before_ms: i64,
    /// End of validity, in Unix epoch milliseconds.
    pub not_after_ms: i64,
}

/// The generator could not produce a certificate.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(display("TLS certificate generation failed: {reason}"))]
pub struct TlsGenerateError {
    /// Non-secret diagnostic.
    pub reason: String,
}

/// Makes the local root and the listener's certificates it issues.
#[async_trait]
pub trait TlsCertificateGenerator: Send + Sync {
    /// A fresh self-signed root, valid for `request`, that may sign server certificates.
    async fn generate_authority(
        &self,
        request: TlsCertificateRequest,
    ) -> Result<TlsAuthority, TlsGenerateError>;

    /// A fresh server certificate signed by `authority` and valid for `request`. It covers the
    /// loopback addresses, `localhost`, the host name and [`Self::addresses`].
    async fn issue(
        &self,
        authority: &TlsAuthority,
        request: TlsCertificateRequest,
    ) -> Result<TlsIdentity, TlsGenerateError>;

    /// The IP addresses a certificate issued now would cover. When they differ from the current
    /// certificate's, the owner issues a new one.
    fn addresses(&self) -> Vec<IpAddr>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fingerprint_prints_as_colon_separated_upper_case_and_parses_back() {
        let fingerprint = TlsFingerprint::of_certificate(b"certificate");
        let text = fingerprint.to_string();
        assert_eq!(text.len(), 32 * 3 - 1);
        assert_eq!(text, text.to_uppercase());
        assert_eq!(text.parse::<TlsFingerprint>(), Ok(fingerprint));
        assert_eq!(
            text.to_lowercase().parse::<TlsFingerprint>(),
            Ok(fingerprint)
        );
        assert_eq!(
            text.replace(':', "").parse::<TlsFingerprint>(),
            Ok(fingerprint)
        );
    }

    #[test]
    fn malformed_fingerprints_are_rejected() {
        let valid = TlsFingerprint::of_certificate(b"x").to_string();
        for bad in [
            "",
            "AA",
            &valid[..valid.len() - 1],
            &format!("{valid}00"),
            &valid.replace('A', "G"),
            &"é".repeat(32),
        ] {
            assert_eq!(
                bad.parse::<TlsFingerprint>(),
                Err(TlsFingerprintError),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_identity_never_prints_its_private_key() {
        let identity = TlsIdentity::new(
            b"cert".to_vec(),
            b"super-secret-key".to_vec(),
            b"root".to_vec(),
            Vec::new(),
            Vec::new(),
            7,
        );
        let debug = format!("{identity:?}");
        assert!(!debug.contains("super-secret-key"), "{debug}");
        assert!(debug.contains("REDACTED"), "{debug}");
        let summary = serde_json::to_string(&identity.summary()).unwrap();
        assert!(!summary.contains("secret"), "{summary}");
        assert_eq!(
            identity.summary().fingerprint,
            identity.authority_fingerprint()
        );

        let authority = TlsAuthority::new(b"root".to_vec(), b"root-secret-key".to_vec(), 9);
        let debug = format!("{authority:?}");
        assert!(!debug.contains("root-secret-key"), "{debug}");
        assert!(debug.contains("REDACTED"), "{debug}");
    }
}
