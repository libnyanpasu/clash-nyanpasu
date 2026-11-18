use std::{path::PathBuf, sync::Arc, time::Duration};

use interprocess::local_socket::tokio::Stream;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    sync::Barrier,
};

use super::{Endpoint, EndpointError, EndpointListener, Namespace};

pub(super) const TEST: Namespace = Namespace::new("nyanpasu-endpoint-test");

pub(super) fn location() -> (tempfile::TempDir, Endpoint) {
    let directory = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let path = {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        directory.path().canonicalize().unwrap().join("rpc.sock")
    };
    #[cfg(windows)]
    let path = Endpoint::discover(TEST, directory.path(), None)
        .unwrap()
        .path()
        .to_owned();
    let endpoint = Endpoint::discover(TEST, directory.path(), Some(&path)).unwrap();
    (directory, endpoint)
}

pub(super) async fn connect(listener: &EndpointListener, endpoint: &Endpoint) -> (Stream, Stream) {
    tokio::time::timeout(Duration::from_secs(10), async {
        let (client, server) = tokio::join!(endpoint.connect(), listener.accept());
        (client.unwrap(), server.unwrap())
    })
    .await
    .expect("listener must accept a connection without losing its name")
}

pub(super) async fn exchange(client: &mut Stream, server: &mut Stream, value: u8) {
    tokio::time::timeout(Duration::from_secs(10), async {
        client.write_u8(value).await.unwrap();
        assert_eq!(server.read_u8().await.unwrap(), value);
        server.write_u8(value + 1).await.unwrap();
        assert_eq!(client.read_u8().await.unwrap(), value + 1);
        #[cfg(windows)]
        for stream in [client, server] {
            let Stream::NamedPipe(pipe) = stream;
            // Both reads above acknowledge every byte. Avoid the library's
            // deferred drop-flush retaining a name after this test drops its streams.
            pipe.inner().assume_flushed();
        }
    })
    .await
    .expect("accepted streams must carry bytes in both directions");
}

#[tokio::test]
async fn local_endpoint_accepts_consecutive_connections() {
    let (_directory, endpoint) = location();
    let listener = EndpointListener::bind(&endpoint).await.unwrap();
    for value in 0..3 {
        let (mut client, mut server) = connect(&listener, &endpoint).await;
        exchange(&mut client, &mut server, value).await;
    }
}

#[tokio::test]
async fn second_live_binder_is_rejected() {
    let (_directory, endpoint) = location();
    let listener = EndpointListener::bind(&endpoint).await.unwrap();
    assert!(matches!(
        EndpointListener::bind(&endpoint).await,
        Err(EndpointError::AlreadyServing { .. })
    ));
    let (mut client, mut server) = connect(&listener, &endpoint).await;
    assert!(matches!(
        EndpointListener::bind(&endpoint).await,
        Err(EndpointError::AlreadyServing { .. })
    ));
    exchange(&mut client, &mut server, 7).await;
}

#[tokio::test]
async fn racing_binders_have_one_winner() {
    let (_directory, endpoint) = location();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let crashed = std::os::unix::net::UnixListener::bind(endpoint.path()).unwrap();
        std::fs::set_permissions(endpoint.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
        drop(crashed);
    }
    let barrier = Arc::new(Barrier::new(2));
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let barrier = barrier.clone();
        let endpoint = endpoint.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            EndpointListener::bind(&endpoint).await
        }));
    }
    let mut winners = Vec::new();
    let mut refusals = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(listener) => winners.push(listener),
            Err(EndpointError::AlreadyServing { .. }) => refusals += 1,
            Err(error) => panic!("unexpected claim result: {error:?}"),
        }
    }
    assert_eq!(winners.len(), 1);
    assert_eq!(refusals, 1);
    let (mut client, mut server) = connect(&winners[0], &endpoint).await;
    exchange(&mut client, &mut server, 9).await;
}

#[test]
fn desktop_and_server_share_default_endpoint_for_data_directory() {
    let directory = tempfile::tempdir().unwrap();
    let desktop = Endpoint::discover(TEST, directory.path(), None).unwrap();
    let server = Endpoint::discover(TEST, &directory.path().join("."), None).unwrap();
    let cli = Endpoint::discover(TEST, &directory.path().canonicalize().unwrap(), None).unwrap();
    assert_eq!(desktop, server);
    assert_eq!(server, cli);
    #[cfg(windows)]
    assert_eq!(
        desktop,
        Endpoint::discover(
            TEST,
            &PathBuf::from(directory.path().to_string_lossy().to_uppercase()),
            None,
        )
        .unwrap(),
        "Windows path casing must not produce a second default endpoint"
    );
    let other = tempfile::tempdir().unwrap();
    assert_ne!(
        desktop,
        Endpoint::discover(TEST, other.path(), None).unwrap()
    );
    assert_eq!(
        desktop,
        Endpoint::discover(TEST, directory.path(), Some(desktop.path())).unwrap()
    );
    assert!(matches!(
        Endpoint::discover(TEST, directory.path(), Some(&PathBuf::from("relative"))),
        Err(EndpointError::InvalidLocation { .. })
    ));
}

#[test]
fn namespaces_never_share_a_default_endpoint() {
    let directory = tempfile::tempdir().unwrap();
    let ours = Endpoint::discover(TEST, directory.path(), None).unwrap();
    let theirs = Endpoint::discover(
        Namespace::new("nyanpasu-other-test"),
        directory.path(),
        None,
    )
    .unwrap();
    assert_ne!(ours.path(), theirs.path());
}

#[test]
fn namespace_names_are_restricted() {
    for name in ["", "Upper", "dot.", "slash/", "under_score"] {
        let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
        assert!(
            std::panic::catch_unwind(|| Namespace::new(name)).is_err(),
            "{name:?}"
        );
    }
    let longest: &'static str = Box::leak("x".repeat(Namespace::MAX_LEN).into_boxed_str());
    assert_eq!(Namespace::new(longest).as_str(), longest);
    let too_long: &'static str = Box::leak("x".repeat(Namespace::MAX_LEN + 1).into_boxed_str());
    assert!(std::panic::catch_unwind(|| Namespace::new(too_long)).is_err());
}

#[test]
fn verify_uid_accepts_only_expected_user() {
    use super::{PeerMismatch, verify_uid};

    assert_eq!(verify_uid(Some(1000), 1000), Ok(()));
    assert_eq!(verify_uid(Some(0), 0), Ok(()));
    assert_eq!(
        verify_uid(Some(1001), 1000),
        Err(PeerMismatch::Uid {
            peer: 1001,
            expected: 1000,
        })
    );
    assert_eq!(
        verify_uid(Some(0), 1000),
        Err(PeerMismatch::Uid {
            peer: 0,
            expected: 1000,
        })
    );
    assert_eq!(verify_uid(None, 1000), Err(PeerMismatch::MissingIdentity));
}

#[test]
fn verify_sid_accepts_only_expected_user() {
    use super::{PeerMismatch, verify_sid};

    let expected = "S-1-5-21-100-200-300-1001";
    assert_eq!(verify_sid(Some(expected), expected), Ok(()));
    assert_eq!(
        verify_sid(Some("S-1-5-21-100-200-300-1002"), expected),
        Err(PeerMismatch::Sid {
            peer: "S-1-5-21-100-200-300-1002".to_owned(),
            expected: expected.to_owned(),
        })
    );
    assert_eq!(
        verify_sid(Some("S-1-5-18"), expected),
        Err(PeerMismatch::Sid {
            peer: "S-1-5-18".to_owned(),
            expected: expected.to_owned(),
        })
    );
    assert_eq!(
        verify_sid(Some(""), ""),
        Err(PeerMismatch::Sid {
            peer: String::new(),
            expected: String::new(),
        })
    );
    assert_eq!(
        verify_sid(None, expected),
        Err(PeerMismatch::MissingIdentity)
    );
}

#[cfg(windows)]
#[path = "windows_tests.rs"]
mod windows;
