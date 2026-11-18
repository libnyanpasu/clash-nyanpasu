//! What the settings page shows of the HTTPS certificate: the fields a certificate viewer shows,
//! read from its DER encoding. Nothing in it is secret; every TLS client receives the certificate.

use std::{fmt::Write as _, net::IpAddr};

use serde::{Deserialize, Serialize};
use sha1::{Digest as _, Sha1};
use snafu::Snafu;
use specta::Type;
use x509_parser::{
    certificate::X509Certificate,
    extensions::GeneralName,
    objects::{oid_registry, oid2sn},
    oid_registry::Oid,
    prelude::FromDer as _,
};

use super::TlsFingerprint;

/// The certificate's fields, as a certificate viewer names them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TlsCertificateDetails {
    /// The distinguished name of the subject, `CN=Clash Nyanpasu`.
    pub subject: String,
    /// The distinguished name of the issuer; the same as the subject for a self-signed one.
    pub issuer: String,
    /// The serial number, upper-case hexadecimal pairs separated by `:`.
    pub serial: String,
    /// Start of validity, in Unix epoch milliseconds.
    pub not_before_ms: i64,
    /// End of validity, in Unix epoch milliseconds.
    pub not_after_ms: i64,
    /// The DNS names of the subject alternative name extension.
    pub dns_names: Vec<String>,
    /// The IP addresses of the subject alternative name extension.
    pub ip_addresses: Vec<IpAddr>,
    /// The signature algorithm's short name, `ecdsa-with-SHA256`, or its dotted OID.
    pub signature_algorithm: String,
    /// The public key algorithm's short name, `id-ecPublicKey`, or its dotted OID.
    pub public_key_algorithm: String,
    /// SHA-256 of the DER encoding, the value clients pin.
    pub sha256: TlsFingerprint,
    /// SHA-1 of the DER encoding, the thumbprint some viewers show instead.
    pub sha1: String,
}

/// The bytes are not a DER-encoded X.509 certificate.
#[derive(Debug, Clone, PartialEq, Eq, Snafu)]
#[snafu(display("the certificate cannot be read: {reason}"))]
pub struct TlsCertificateParseError {
    reason: String,
}

impl TlsCertificateDetails {
    /// Reads the fields of a DER-encoded certificate.
    ///
    /// # Errors
    /// Fails when the bytes are not one complete X.509 certificate.
    pub fn parse(certificate_der: &[u8]) -> Result<Self, TlsCertificateParseError> {
        let failure = |reason: String| TlsCertificateParseError { reason };
        let (rest, certificate) = X509Certificate::from_der(certificate_der)
            .map_err(|error| failure(error.to_string()))?;
        if !rest.is_empty() {
            return Err(failure("trailing bytes after the certificate".to_owned()));
        }
        let mut dns_names = Vec::new();
        let mut ip_addresses = Vec::new();
        if let Some(names) = certificate
            .subject_alternative_name()
            .map_err(|error| failure(error.to_string()))?
        {
            for name in &names.value.general_names {
                match name {
                    GeneralName::DNSName(name) => dns_names.push((*name).to_owned()),
                    GeneralName::IPAddress(bytes) => {
                        if let Some(address) = ip_address(bytes) {
                            ip_addresses.push(address);
                        }
                    }
                    _ => {}
                }
            }
        }
        let validity = certificate.validity();
        Ok(Self {
            subject: certificate.subject().to_string(),
            issuer: certificate.issuer().to_string(),
            serial: hex_pairs(certificate.raw_serial()),
            not_before_ms: validity.not_before.timestamp() * 1000,
            not_after_ms: validity.not_after.timestamp() * 1000,
            dns_names,
            ip_addresses,
            signature_algorithm: algorithm_name(&certificate.signature_algorithm.algorithm),
            public_key_algorithm: algorithm_name(&certificate.public_key().algorithm.algorithm),
            sha256: TlsFingerprint::of_certificate(certificate_der),
            sha1: hex_pairs(&Sha1::digest(certificate_der)),
        })
    }
}

fn ip_address(bytes: &[u8]) -> Option<IpAddr> {
    match bytes.len() {
        4 => <[u8; 4]>::try_from(bytes).ok().map(IpAddr::from),
        16 => <[u8; 16]>::try_from(bytes).ok().map(IpAddr::from),
        _ => None,
    }
}

fn algorithm_name(oid: &Oid<'_>) -> String {
    oid2sn(oid, oid_registry()).map_or_else(|_| oid.to_id_string(), ToOwned::to_owned)
}

/// Upper-case hexadecimal pairs separated by `:`, as [`TlsFingerprint`] prints.
fn hex_pairs(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 3);
    for (index, byte) in bytes.iter().enumerate() {
        if index > 0 {
            text.push(':');
        }
        write!(text, "{byte:02X}").expect("writing to a String does not fail");
    }
    text
}
