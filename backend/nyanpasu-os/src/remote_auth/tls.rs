//! The local certificate authority of the HTTPS listener and the short-lived certificates it
//! issues.

use std::net::IpAddr;

use async_trait::async_trait;
use nyanpasu_contracts::auth::{
    TlsAuthority, TlsCertificateGenerator, TlsCertificateRequest, TlsGenerateError, TlsIdentity,
};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, SanType,
};
use rustls_pki_types::CertificateDer;
use time::OffsetDateTime;

/// Makes ECDSA P-256 certificates with `rcgen`: a self-signed root that may sign only server
/// certificates, and server certificates it signs for `localhost`, the loopback addresses, the
/// host name and the addresses of the host's interfaces.
#[derive(Debug, Clone, Copy, Default)]
pub struct RcgenCertificateGenerator;

#[async_trait]
impl TlsCertificateGenerator for RcgenCertificateGenerator {
    async fn generate_authority(
        &self,
        request: TlsCertificateRequest,
    ) -> Result<TlsAuthority, TlsGenerateError> {
        tokio::task::spawn_blocking(move || generate_authority(request))
            .await
            .map_err(|error| failure(error.to_string()))?
    }

    async fn issue(
        &self,
        authority: &TlsAuthority,
        request: TlsCertificateRequest,
    ) -> Result<TlsIdentity, TlsGenerateError> {
        let authority = authority.clone();
        tokio::task::spawn_blocking(move || issue(&authority, request))
            .await
            .map_err(|error| failure(error.to_string()))?
    }

    fn addresses(&self) -> Vec<IpAddr> {
        addresses()
    }
}

fn failure(reason: impl ToString) -> TlsGenerateError {
    TlsGenerateError {
        reason: reason.to_string(),
    }
}

fn generate_authority(request: TlsCertificateRequest) -> Result<TlsAuthority, TlsGenerateError> {
    let mut params = CertificateParams::default();
    let mut subject = DistinguishedName::new();
    subject.push(DnType::CommonName, authority_name());
    subject.push(DnType::OrganizationName, "Clash Nyanpasu");
    params.distinguished_name = subject;
    // It signs only end-entity certificates, never another authority.
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    params.not_before = instant(request.not_before_ms)?;
    params.not_after = instant(request.not_after_ms)?;

    let key = KeyPair::generate().map_err(failure)?;
    let certificate = params.self_signed(&key).map_err(failure)?;
    Ok(TlsAuthority::new(
        certificate.der().to_vec(),
        key.serialize_der(),
        request.not_after_ms,
    ))
}

fn issue(
    authority: &TlsAuthority,
    request: TlsCertificateRequest,
) -> Result<TlsIdentity, TlsGenerateError> {
    let authority_key = KeyPair::try_from(authority.private_key_der()).map_err(failure)?;
    let issuer = Issuer::from_ca_cert_der(
        &CertificateDer::from(authority.certificate_der()),
        authority_key,
    )
    .map_err(failure)?;

    let dns_names = dns_names();
    let addresses = addresses();
    let mut params = CertificateParams::default();
    params.subject_alt_names = dns_names
        .iter()
        .filter_map(|name| name.clone().try_into().ok().map(SanType::DnsName))
        .chain(addresses.iter().copied().map(SanType::IpAddress))
        .collect();
    let mut subject = DistinguishedName::new();
    subject.push(DnType::CommonName, "Clash Nyanpasu");
    params.distinguished_name = subject;
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.use_authority_key_identifier_extension = true;
    params.not_before = instant(request.not_before_ms)?;
    params.not_after = instant(request.not_after_ms)?;

    let key = KeyPair::generate().map_err(failure)?;
    let certificate = params.signed_by(&key, &issuer).map_err(failure)?;
    Ok(TlsIdentity::new(
        certificate.der().to_vec(),
        key.serialize_der(),
        authority.certificate_der().to_vec(),
        dns_names,
        addresses,
        request.not_after_ms,
    ))
}

fn instant(unix_ms: i64) -> Result<OffsetDateTime, TlsGenerateError> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(unix_ms) * 1_000_000).map_err(failure)
}

/// The root's name, with the host name so a trust store holding several tells them apart.
fn authority_name() -> String {
    match whoami::hostname() {
        Ok(host) if !host.trim().is_empty() => {
            format!("Clash Nyanpasu Local CA ({})", host.trim())
        }
        _ => "Clash Nyanpasu Local CA".to_owned(),
    }
}

/// `localhost`, the host name and its mDNS form (`host.local`), lower case.
fn dns_names() -> Vec<String> {
    let mut names = vec!["localhost".to_owned()];
    if let Ok(host) = whoami::hostname() {
        let host = host.trim().to_ascii_lowercase();
        if !host.is_empty() {
            let mdns = (!host.contains('.')).then(|| format!("{host}.local"));
            for name in std::iter::once(host).chain(mdns) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names
}

/// The loopback addresses and the host's usable interface addresses.
fn addresses() -> Vec<IpAddr> {
    let discovered = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|interface| interface.ip());
    let mut addresses = vec![
        IpAddr::from([127, 0, 0, 1]),
        IpAddr::from(std::net::Ipv6Addr::LOCALHOST),
    ];
    for address in discovered {
        if address.is_unspecified() || address.is_multicast() || is_link_local(address) {
            continue;
        }
        if !addresses.contains(&address) {
            addresses.push(address);
        }
    }
    addresses
}

/// A link-local IPv6 address needs a zone, so a client cannot validate it by name.
fn is_link_local(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => address.is_link_local(),
        IpAddr::V6(address) => address.segments()[0] & 0xffc0 == 0xfe80,
    }
}
