use nyanpasu_contracts::auth::{SecretStore, SecretStoreError};

use super::FsSecretStore;

fn store_in(dir: &tempfile::TempDir, max_bytes: usize) -> FsSecretStore {
    FsSecretStore::new(
        dir.path().join("remote-access/auth-secrets.json"),
        max_bytes,
    )
}

#[tokio::test]
async fn load_of_a_missing_record_is_none() {
    let dir = tempfile::tempdir().unwrap();
    assert!(store_in(&dir, 1024).load().await.unwrap().is_none());
}

#[tokio::test]
async fn save_then_load_round_trips_and_replaces() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir, 1024);

    store.save(b"first").await.unwrap();
    store.save(b"second").await.unwrap();

    assert_eq!(store.load().await.unwrap().unwrap().as_slice(), b"second");
}

#[cfg(unix)]
#[tokio::test]
async fn directory_is_0700_and_file_is_0600() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir, 1024);
    store.save(b"secret").await.unwrap();
    // A second save replaces the file; the replacement must stay private.
    store.save(b"secret-2").await.unwrap();

    let secrets_dir = dir.path().join("remote-access");
    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&secrets_dir), 0o700);
    assert_eq!(mode(&secrets_dir.join("auth-secrets.json")), 0o600);
}

#[tokio::test]
async fn oversized_records_are_rejected_on_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir, 8);

    assert!(matches!(
        store.save(&[0u8; 9]).await,
        Err(SecretStoreError::TooLarge { max_bytes: 8 })
    ));

    store.save(&[1u8; 8]).await.unwrap();
    assert_eq!(store.load().await.unwrap().unwrap().len(), 8);

    // A record that grew past the bound outside this adapter is refused, not truncated.
    let path = dir.path().join("remote-access/auth-secrets.json");
    std::fs::write(&path, [2u8; 9]).unwrap();
    assert!(matches!(
        store.load().await,
        Err(SecretStoreError::TooLarge { max_bytes: 8 })
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_and_non_regular_targets_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = store_in(&dir, 1024);
    let secrets_dir = dir.path().join("remote-access");
    let path = secrets_dir.join("auth-secrets.json");
    std::fs::create_dir_all(&secrets_dir).unwrap();

    let outside = dir.path().join("outside");
    std::fs::write(&outside, b"outside").unwrap();
    std::os::unix::fs::symlink(&outside, &path).unwrap();
    assert!(matches!(
        store.load().await,
        Err(SecretStoreError::NotRegularFile)
    ));
    assert!(matches!(
        store.save(b"x").await,
        Err(SecretStoreError::NotRegularFile)
    ));
    assert_eq!(std::fs::read(&outside).unwrap(), b"outside");

    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(matches!(
        store.save(b"x").await,
        Err(SecretStoreError::NotRegularFile)
    ));
}

#[tokio::test]
async fn a_generated_certificate_covers_loopback_and_the_requested_address() {
    use nyanpasu_contracts::auth::{TlsCertificateGenerator as _, TlsCertificateRequest};

    use super::RcgenCertificateGenerator;

    let request = TlsCertificateRequest {
        addresses: vec!["192.168.77.5".parse().unwrap()],
        not_before_ms: 1_700_000_000_000,
        not_after_ms: 1_700_000_000_000 + 86_400_000,
    };
    let identity = RcgenCertificateGenerator
        .generate(request.clone())
        .await
        .unwrap();

    assert_eq!(identity.not_after_ms(), request.not_after_ms);
    assert!(identity.dns_names().iter().any(|name| name == "localhost"));
    // The identity reports the addresses the certificate really names.
    for covered in ["127.0.0.1", "::1", "192.168.77.5"] {
        assert!(
            identity.covers_address(covered.parse().unwrap()),
            "{covered}"
        );
    }
    assert!(!identity.covers_address("192.168.77.6".parse().unwrap()));
    assert!(!identity.certificate_der().is_empty());
    assert!(rcgen::KeyPair::try_from(identity.private_key_der()).is_ok());

    // Two identities are different certificates: the key is fresh every time.
    let other = RcgenCertificateGenerator.generate(request).await.unwrap();
    assert_ne!(identity.fingerprint(), other.fingerprint());
}

#[tokio::test]
async fn a_generated_certificate_reads_back_as_a_self_signed_one() {
    use nyanpasu_contracts::auth::{
        TlsCertificateDetails, TlsCertificateGenerator as _, TlsCertificateRequest,
    };

    use super::RcgenCertificateGenerator;

    let request = TlsCertificateRequest {
        addresses: vec!["192.168.77.5".parse().unwrap()],
        not_before_ms: 1_700_000_000_000,
        not_after_ms: 1_700_000_000_000 + 86_400_000,
    };
    let identity = RcgenCertificateGenerator
        .generate(request.clone())
        .await
        .unwrap();

    let details = TlsCertificateDetails::parse(identity.certificate_der()).unwrap();
    assert_eq!(details.subject, "CN=Clash Nyanpasu");
    assert_eq!(details.issuer, details.subject, "it signs itself");
    assert_eq!(details.not_before_ms, request.not_before_ms);
    assert_eq!(details.not_after_ms, request.not_after_ms);
    assert!(details.dns_names.iter().any(|name| name == "localhost"));
    assert_eq!(details.ip_addresses, identity.addresses());
    assert_eq!(details.signature_algorithm, "ecdsa-with-SHA256");
    assert_eq!(details.sha256, identity.fingerprint());
    assert_eq!(details.sha1.len(), 20 * 3 - 1);

    assert!(TlsCertificateDetails::parse(b"not a certificate").is_err());
}

/// A TLS handshake in which the client trusts the generated certificate as its only root and
/// checks the name, so the certificate's names and usages are the ones a real client enforces.
async fn handshake(
    identity: &nyanpasu_contracts::auth::TlsIdentity,
    name: rustls::pki_types::ServerName<'static>,
) -> Result<(), std::io::Error> {
    use std::sync::Arc;

    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let certificate = CertificateDer::from(identity.certificate_der().to_vec());
    let server = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![certificate.clone()],
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(
                identity.private_key_der().to_vec(),
            )),
        )
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(certificate).unwrap();
    let client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let (client_io, server_io) = tokio::io::duplex(16 * 1024);
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client));
    let (accepted, connected) = tokio::join!(
        acceptor.accept(server_io),
        connector.connect(name, client_io)
    );
    connected.map(drop)?;
    accepted.map(drop)
}

#[tokio::test]
async fn a_generated_certificate_is_valid_for_every_name_it_covers_and_no_other() {
    use nyanpasu_contracts::auth::{TlsCertificateGenerator as _, TlsCertificateRequest};
    use rustls::pki_types::ServerName;

    use super::RcgenCertificateGenerator;

    let identity = RcgenCertificateGenerator
        .generate(TlsCertificateRequest {
            addresses: vec!["192.168.77.5".parse().unwrap(), "fd00::5".parse().unwrap()],
            not_before_ms: 1_700_000_000_000 - 86_400_000,
            not_after_ms: i64::MAX / 1_000_000,
        })
        .await
        .unwrap();

    for covered in ["127.0.0.1", "::1", "192.168.77.5", "fd00::5", "localhost"] {
        let name = ServerName::try_from(covered).unwrap().to_owned();
        handshake(&identity, name)
            .await
            .unwrap_or_else(|error| panic!("{covered}: {error}"));
    }
    for host in identity.dns_names() {
        let name = ServerName::try_from(host.clone()).unwrap();
        handshake(&identity, name)
            .await
            .unwrap_or_else(|error| panic!("{host}: {error}"));
    }
    for foreign in ["10.9.9.9", "evil.example"] {
        let name = ServerName::try_from(foreign).unwrap().to_owned();
        assert!(handshake(&identity, name).await.is_err(), "{foreign}");
    }
}
