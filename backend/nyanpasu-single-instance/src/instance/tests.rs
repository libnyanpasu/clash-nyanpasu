use std::time::Duration;

use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

use super::{
    Ack, Claim, FrameError, Launch, MAX_FRAME_LEN, NotifyError, Peer, Primary, VERSION, claim,
    notify_within, read_frame, write_frame,
};
use crate::endpoint::{Endpoint, EndpointListener, Namespace};

const TEST: Namespace = Namespace::new("nyanpasu-instance-test");

fn launch(url: &str) -> Launch {
    Launch {
        args: vec!["clash-nyanpasu".into(), url.into()],
        cwd: Some("work".into()),
    }
}

async fn primary(root: &std::path::Path) -> Primary {
    match claim(TEST, root).await.unwrap() {
        Claim::Primary(primary) => primary,
        Claim::AlreadyRunning(_) => panic!("a fresh directory has no owner"),
    }
}

async fn peer(root: &std::path::Path) -> Peer {
    match claim(TEST, root).await.unwrap() {
        Claim::AlreadyRunning(peer) => peer,
        Claim::Primary(_) => panic!("the directory is already owned"),
    }
}

#[tokio::test]
async fn only_the_first_claim_of_a_directory_is_primary() {
    let root = tempfile::tempdir().unwrap();
    let first = primary(root.path()).await;
    peer(root.path()).await;

    let other = tempfile::tempdir().unwrap();
    let independent = primary(other.path()).await;

    drop(first);
    let again = primary(root.path()).await;
    drop((again, independent));
}

#[tokio::test]
async fn launches_reach_the_primary_in_order_and_are_acknowledged() {
    let root = tempfile::tempdir().unwrap();
    let mut launches = primary(root.path()).await.into_launches();
    let peer = peer(root.path()).await;
    for url in ["clash://first", "clash://second"] {
        let acknowledged = peer.notify(&launch(url)).await.unwrap();
        assert_eq!(acknowledged.primary_pid, std::process::id());
    }
    assert_eq!(launches.recv().await, Some(launch("clash://first")));
    assert_eq!(launches.recv().await, Some(launch("clash://second")));
}

#[tokio::test]
async fn dropping_the_launches_releases_the_endpoint() {
    let root = tempfile::tempdir().unwrap();
    let launches = primary(root.path()).await.into_launches();
    drop(launches);
    // The aborted task drops the listener on its next poll.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Claim::Primary(primary) = claim(TEST, root.path()).await.unwrap() {
                break primary;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the endpoint must be released after the launches are dropped");
}

#[tokio::test]
async fn malformed_launches_do_not_stop_accepting() {
    let root = tempfile::tempdir().unwrap();
    let mut launches = primary(root.path()).await.into_launches();
    let peer = peer(root.path()).await;
    let oversized = [&[VERSION][..], &(MAX_FRAME_LEN + 1).to_be_bytes()].concat();
    let truncated = [&[VERSION][..], &10_u32.to_be_bytes(), b"{"].concat();
    for bytes in [vec![VERSION + 1, 0, 0, 0, 0], oversized, truncated] {
        let mut stream = peer.endpoint.connect().await.unwrap();
        stream.write_all(&bytes).await.unwrap();
        stream.flush().await.unwrap();
        // The truncated frame waits for its body until the client half closes.
        let mut rest = Vec::new();
        let closed = tokio::time::timeout(Duration::from_secs(1), stream.read_to_end(&mut rest));
        if closed.await.is_err() {
            drop(stream);
        }
    }
    peer.notify(&launch("clash://after")).await.unwrap();
    assert_eq!(launches.recv().await, Some(launch("clash://after")));
}

#[tokio::test]
async fn an_unresponsive_primary_times_out() {
    let root = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::discover(TEST, root.path(), None).unwrap();
    // Owns the endpoint but never reads or answers.
    let _listener = EndpointListener::bind(&endpoint).await.unwrap();
    let error = notify_within(&endpoint, &launch("clash://x"), Duration::from_millis(200))
        .await
        .unwrap_err();
    assert!(matches!(error, NotifyError::Timeout { .. }), "{error:?}");
}

#[tokio::test]
async fn frames_round_trip_and_refuse_other_versions_and_oversized_bodies() {
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &Ack { pid: 42 }).await.unwrap();
    assert_eq!(bytes[0], VERSION);
    let ack: Ack = read_frame(&mut bytes.as_slice()).await.unwrap();
    assert_eq!(ack.pid, 42);

    bytes[0] = VERSION + 1;
    let error = read_frame::<_, Ack>(&mut bytes.as_slice())
        .await
        .unwrap_err();
    assert!(matches!(error, FrameError::Version { found } if found == VERSION + 1));

    let header = [&[VERSION][..], &(MAX_FRAME_LEN + 1).to_be_bytes()].concat();
    let error = read_frame::<_, Ack>(&mut header.as_slice())
        .await
        .unwrap_err();
    assert!(matches!(error, FrameError::TooLarge { .. }), "{error:?}");

    let huge = Launch {
        args: vec!["x".repeat(MAX_FRAME_LEN as usize)],
        cwd: None,
    };
    let error = write_frame(&mut Vec::new(), &huge).await.unwrap_err();
    assert!(matches!(error, FrameError::TooLarge { .. }), "{error:?}");
}
