//! Self-signed certificate generation for the HTTPS listener.

use std::net::IpAddr;

use async_trait::async_trait;
use nyanpasu_contracts::auth::{
    TlsCertificateGenerator, TlsCertificateRequest, TlsGenerateError, TlsIdentity,
};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use time::OffsetDateTime;

/// Generates an ECDSA P-256 self-signed server certificate with `rcgen`.
///
/// The certificate always covers `localhost`, the loopback addresses, the host name and the
/// addresses of the host's interfaces, plus the addresses of the request.
#[derive(Debug, Clone, Copy, Default)]
pub struct RcgenCertificateGenerator;

#[async_trait]
impl TlsCertificateGenerator for RcgenCertificateGenerator {
    async fn generate(
        &self,
        request: TlsCertificateRequest,
    ) -> Result<TlsIdentity, TlsGenerateError> {
        tokio::task::spawn_blocking(move || generate(&request))
            .await
            .map_err(|error| failure(error.to_string()))?
    }
}

fn failure(reason: impl ToString) -> TlsGenerateError {
    TlsGenerateError {
        reason: reason.to_string(),
    }
}

fn generate(request: &TlsCertificateRequest) -> Result<TlsIdentity, TlsGenerateError> {
    let dns_names = dns_names();
    let addresses = addresses(request);

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
    params.not_before = instant(request.not_before_ms)?;
    params.not_after = instant(request.not_after_ms)?;

    let key = KeyPair::generate().map_err(failure)?;
    let certificate = params.self_signed(&key).map_err(failure)?;
    Ok(TlsIdentity::new(
        certificate.der().to_vec(),
        key.serialize_der(),
        dns_names,
        addresses,
        request.not_after_ms,
    ))
}

fn instant(unix_ms: i64) -> Result<OffsetDateTime, TlsGenerateError> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(unix_ms) * 1_000_000).map_err(failure)
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

/// The loopback addresses, the host's usable interface addresses and the requested ones.
fn addresses(request: &TlsCertificateRequest) -> Vec<IpAddr> {
    let discovered = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .map(|interface| interface.ip());
    let mut addresses = vec![
        IpAddr::from([127, 0, 0, 1]),
        IpAddr::from(std::net::Ipv6Addr::LOCALHOST),
    ];
    for address in discovered.chain(request.addresses.iter().copied()) {
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
