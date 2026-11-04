//! `run` composes the real headless application, serves the local socket and
//! stops cleanly. No core binary is installed, so startup cannot start a core;
//! the server must still serve, because the user repairs it through the socket.
#![cfg(unix)]

use std::time::Duration;

use camino::Utf8PathBuf;
use nyanpasu_rpc_protocol::{LocalClient, RpcErrorKind};
use nyanpasu_server::{
    compose::ServerDirs,
    run::{RunOptions, run},
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// The socket is published when the server binds it, and nothing else says
/// when that is, so a client connects until it can.
async fn connect(socket: &std::path::Path) -> LocalClient {
    for _ in 0..600 {
        if let Ok(client) = LocalClient::connect(socket).await {
            return client;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the server never published {}", socket.display());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_server_serves_the_local_socket_until_it_is_stopped() {
    let directory = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_owned()).unwrap();
    let socket = directory.path().join("rpc.sock");
    let stop = CancellationToken::new();
    let server = tokio::spawn(run(
        RunOptions {
            dirs: ServerDirs {
                config_dir: root.join("config"),
                data_dir: root.join("data"),
            },
            rpc_socket: socket.clone(),
        },
        stop.clone(),
    ));

    let mut client = connect(&socket).await;
    let status = client.call("get_core_status", json!({})).await.unwrap();
    assert!(status.is_ok(), "{status:?}");
    let profiles = client.call("get_profiles", json!({})).await.unwrap();
    assert!(profiles.is_ok(), "{profiles:?}");
    let unknown = client.call("no_such_command", json!({})).await.unwrap();
    assert_eq!(unknown.unwrap_err().kind, RpcErrorKind::UnknownMethod);

    stop.cancel();
    server.await.unwrap().unwrap();
    assert!(!socket.exists(), "the socket is removed on a clean stop");
}
