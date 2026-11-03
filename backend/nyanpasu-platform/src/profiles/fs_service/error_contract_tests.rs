//! Wire and source-chain expectations recorded before extracting the profile contracts.

use std::{error::Error, io, path::Path, sync::Arc};

use nyanpasu_config::profile::{ManagedProfilePath, RemoteProfileOptions};
use nyanpasu_contracts::profiles::{
    error::{ProfileFileError, SubscriptionFetchError},
    ports::{
        MaterializationResource, ProfileFsPort, ProfileMaterializationPort, SubscriptionFetcher,
    },
};
use nyanpasu_core::{
    corrupt, service::profile_file::ports::SelfProxyPortSource, utils::test_support,
};
use serde::{Serialize, Serializer};
use serde_json::{Value, json};
use url::Url;

use super::{ABSENT_HASH, JournalLocation, MaterializationJournal, ProfileFileService};

fn source_chain(error: &(dyn Error + 'static)) -> Vec<String> {
    let mut chain = Vec::new();
    let mut source = error.source();
    while let Some(error) = source {
        chain.push(error.to_string());
        source = error.source();
    }
    chain
}

fn assert_error(
    error: &(impl Error + Serialize + 'static),
    wire: Value,
    display: &str,
    sources: &[&str],
) {
    assert_eq!(serde_json::to_value(error).unwrap(), wire);
    assert_eq!(error.to_string(), display);
    assert_eq!(source_chain(error), sources);
}

fn library_source<'error, T: Error + 'static>(error: &'error (dyn Error + 'static)) -> &'error T {
    error
        .source()
        .expect("variant has a library cause")
        .downcast_ref::<T>()
        .expect("variant must preserve the original library error object")
}

#[test]
fn preserve_directory_error_contract() {
    let (temp, service) = service();
    // A legal maximum-length component becomes too long when preserve appends its
    // evidence suffix. This fails without permission changes or a timestamp race.
    let path = temp.path().join("x".repeat(255));
    std::fs::write(&path, "not a directory").unwrap();
    let error = service
        .ensure_directory_chain(temp.path(), &path, true)
        .unwrap_err();
    assert!(matches!(
        error,
        ProfileFileError::PreserveDerivedDirectory { .. }
    ));
    let source = library_source::<corrupt::Error>(&error);
    let corrupt::Error::PreserveEvidence {
        path: original,
        evidence,
        source: io,
    } = source;
    assert_eq!(original, &path);
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        json!({ "kind": "preserve_derived_directory" })
    );
    assert_eq!(
        error.to_string(),
        "could not preserve invalid profile materialization directory"
    );
    let chain = source_chain(&error);
    // The temp path, evidence timestamp and OS-localized IO message vary.
    assert_eq!(chain.len(), 2);
    assert_eq!(
        chain[0]
            .replace(&evidence.display().to_string(), "<evidence>")
            .replace(&path.display().to_string(), "<path>"),
        "failed to preserve <path> as <evidence>"
    );
    assert!(chain[1].ends_with(&format!("(os error {})", io.raw_os_error().unwrap())));
}

#[test]
fn atomic_write_error_contract() {
    let (_temp, service) = service();
    let managed = ManagedProfilePath::new("profile.yaml").unwrap();
    let path = service.resolve(&managed).unwrap();
    std::fs::create_dir_all(&path).unwrap();
    let error = service
        .write_atomic(&managed, "profile: true\n")
        .unwrap_err();
    assert_atomic_write_error(&error, &path);
}

#[test]
fn atomic_journal_write_error_contract() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("journal.yaml");
    std::fs::create_dir(&path).unwrap();
    let journal = MaterializationJournal {
        managed_path: ManagedProfilePath::new("profile.yaml").unwrap(),
        operation_id: "abcdefghijklmnop".into(),
        revision: 1,
        hash: ABSENT_HASH.into(),
    };
    let error = ProfileFileService::write_journal_new(&path, &journal).unwrap_err();
    assert_atomic_write_error(&error, &path);
}

fn assert_atomic_write_error(error: &ProfileFileError, path: &Path) {
    assert!(matches!(error, ProfileFileError::AtomicWrite { .. }));
    let source = library_source::<atomicwrites::Error<io::Error>>(error);
    let (atomicwrites::Error::Internal(io) | atomicwrites::Error::User(io)) = source;
    assert!(matches!(
        io.kind(),
        io::ErrorKind::PermissionDenied
            | io::ErrorKind::IsADirectory
            | io::ErrorKind::AlreadyExists
    ));
    // atomicwrites exposes cause(), not source(); keep its object so core can
    // recover and classify the hidden IO failure exactly as it does in production.
    assert_eq!(corrupt::error_io(error), corrupt::error_io(io));
    assert!(corrupt::error_io(error).is_some());
    // The OS-localized message varies; check its native code and the full chain
    // against the recovered IO object rather than hard-coding localized text.
    assert_error(
        error,
        json!({ "kind": "atomic_write", "path": path.display().to_string() }),
        &format!("could not atomically write {}", path.display()),
        &[&io.to_string()],
    );
    assert!(
        source_chain(error)[0].ends_with(&format!("(os error {})", io.raw_os_error().unwrap()))
    );
}

#[test]
fn parse_journal_error_contract() {
    let (_temp, service) = service();
    let managed = ManagedProfilePath::new("profile.yaml").unwrap();
    let prepared = service
        .prepare_state_first(
            &managed,
            MaterializationResource::File {
                content: "profile: true\n".into(),
            },
            1,
        )
        .unwrap();
    let path = ProfileFileService::journal_path(
        &service.materialization_root(),
        JournalLocation::StatePrepared,
        prepared.operation_id(),
    );
    std::fs::write(&path, "{").unwrap();
    // Recovery/compensation propagates the reader's typed error; reconcile
    // deliberately converts it into a degradation string instead.
    let error = service.compensate(&prepared).unwrap_err();
    assert!(matches!(error, ProfileFileError::ParseJournal { .. }));
    library_source::<serde_yaml::Error>(&error);
    assert_error(
        &error,
        json!({ "kind": "parse_journal", "path": path.display().to_string() }),
        &format!(
            "could not parse the materialization journal {}",
            path.display()
        ),
        &["did not find expected node content at line 2 column 1, while parsing a flow node"],
    );
}

#[test]
fn serialize_journal_error_contract() {
    struct RejectSerialization;
    impl Serialize for RejectSerialization {
        fn serialize<S: Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom(
                "fixture journal serialization failure",
            ))
        }
    }
    // MaterializationJournal contains only strings (including ManagedProfilePath)
    // and u64, serialized into an in-memory string. No representable journal can
    // trigger a serialization error, so this is the sole direct variant fixture.
    let source = serde_yaml::to_string(&RejectSerialization).unwrap_err();
    let error = ProfileFileError::SerializeJournal {
        source: source.into(),
    };
    library_source::<serde_yaml::Error>(&error);
    assert_error(
        &error,
        json!({ "kind": "serialize_journal" }),
        "could not serialize a materialization journal",
        &["fixture journal serialization failure"],
    );
}

struct NoProxy;

impl SelfProxyPortSource for NoProxy {
    fn mixed_port(&self) -> Option<u16> {
        None
    }
}

fn service() -> (tempfile::TempDir, ProfileFileService) {
    let temp = tempfile::tempdir().unwrap();
    let paths = test_support::test_paths(temp.path(), temp.path().join("data"));
    let service = ProfileFileService::new(
        paths,
        Arc::new(NoProxy),
        nyanpasu_contracts::bundle_metadata::TEST_BUNDLE_METADATA,
    );
    (temp, service)
}

fn direct_options() -> RemoteProfileOptions {
    RemoteProfileOptions {
        self_proxy: false,
        with_proxy: false,
        ..Default::default()
    }
}

#[tokio::test]
async fn build_http_client_error_contract() {
    let (_temp, service) = service();
    let error = service
        .fetch(
            &Url::parse("http://127.0.0.1/").unwrap(),
            &RemoteProfileOptions {
                user_agent: Some("invalid\nheader".into()),
                ..direct_options()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        SubscriptionFetchError::BuildHttpClient { .. }
    ));
    library_source::<reqwest::Error>(&error);
    assert_error(
        &error,
        json!({ "kind": "build_http_client" }),
        "could not build the http client",
        &["builder error", "failed to parse header value"],
    );
}

#[tokio::test]
async fn request_subscription_error_contract() {
    let (_temp, service) = service();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    drop(listener);
    let error = service.fetch(&url, &direct_options()).await.unwrap_err();
    assert!(matches!(
        error,
        SubscriptionFetchError::RequestSubscription { .. }
    ));
    let source = library_source::<reqwest::Error>(&error);
    assert!(source.is_connect());
    assert_eq!(source.url(), Some(&url));
    let io = source
        .source()
        .unwrap()
        .source()
        .unwrap()
        .source()
        .unwrap()
        .downcast_ref::<io::Error>()
        .unwrap();
    assert_eq!(io.kind(), io::ErrorKind::ConnectionRefused);
    // The ephemeral port and OS-localized refusal message vary by environment.
    assert_error(
        &error,
        json!({ "kind": "request_subscription" }),
        "could not request the subscription",
        &[
            &format!("error sending request for url ({url})"),
            "client error (Connect)",
            "tcp connect error",
            &io.to_string(),
        ],
    );
    assert!(
        source_chain(&error)[3].ends_with(&format!("(os error {})", io.raw_os_error().unwrap()))
    );
}

#[tokio::test]
async fn read_subscription_body_error_contract() {
    use std::io::{Read as _, Write as _};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    // A dedicated thread serves a deliberately truncated body without blocking the async worker.
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nx")
            .unwrap();
    });
    let (_temp, service) = service();
    let error = service
        .fetch(
            &Url::parse(&format!("http://{address}/")).unwrap(),
            &direct_options(),
        )
        .await
        .unwrap_err();
    server.join().unwrap();
    // Only the server's ephemeral port varies; every source level is still compared.
    assert!(matches!(
        error,
        SubscriptionFetchError::ReadSubscriptionBody { .. }
    ));
    library_source::<reqwest::Error>(&error);
    assert_error(
        &error,
        json!({ "kind": "read_subscription_body" }),
        "could not read the subscription response body",
        &[
            &format!("error decoding response body for url (http://{address}/)"),
            "request or response body error",
            "error reading a body from connection",
            "end of file before message length reached",
        ],
    );
}
