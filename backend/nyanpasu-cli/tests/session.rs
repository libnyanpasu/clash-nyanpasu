//! Consecutive CLI calls are separate processes, so the only thing they can
//! share is the one session the server holds. The server here is the real
//! headless application serving the real local socket; the CLI is the binary.
#![cfg(unix)]

use std::{path::Path, process::Output, time::Duration};

use camino::Utf8PathBuf;
use nyanpasu_rpc::serve_local;
use nyanpasu_server::compose::{Server, ServerDirs};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

const PROFILE: &str = "mixed-port: 17890\nproxies: []\nrules:\n  - MATCH,DIRECT\n";

/// Runs the CLI binary and parses the one JSON document it prints.
async fn cli(socket: &Path, args: Vec<String>) -> (i32, Value) {
    let socket = socket.to_owned();
    let output: Output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_nyanpasu-cli"))
            .arg("--socket")
            .arg(socket)
            .args(["--timeout", "60"])
            .args(args)
            .output()
            .expect("run the CLI")
    })
    .await
    .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let document = serde_json::from_str(&stdout)
        .unwrap_or_else(|error| panic!("{error}: stdout {stdout:?}, {:?}", output.stderr));
    (
        output.status.code().expect("the CLI exits normally"),
        document,
    )
}

fn call(method: &str) -> Vec<String> {
    vec!["call".into(), method.into()]
}

#[tokio::test(flavor = "multi_thread")]
async fn consecutive_calls_observe_the_one_server_session() {
    let directory = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_owned()).unwrap();
    let socket = directory.path().join("rpc.sock");
    let server = Server::start(ServerDirs {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
    })
    .await
    .unwrap();
    let stop = CancellationToken::new();
    let serving = tokio::spawn({
        let (socket, rpc, stop) = (socket.clone(), server.rpc().unwrap(), stop.clone());
        async move { serve_local(&socket, rpc, stop).await }
    });
    // The socket appears when it is bound and restricted; nothing else says when.
    for _ in 0..200 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    let (code, methods) = cli(&socket, vec!["methods".into()]).await;
    assert_eq!(code, 0, "{methods}");
    let methods: Vec<&str> = methods
        .as_array()
        .unwrap()
        .iter()
        .map(|name| name.as_str().unwrap())
        .collect();
    for expected in [
        "list_methods",
        "create_profile",
        "reconcile_core",
        "stop_core",
    ] {
        assert!(methods.contains(&expected), "{expected} in {methods:?}");
    }

    let (code, profiles) = cli(&socket, call("get_profiles")).await;
    assert_eq!(code, 0, "{profiles}");
    assert!(!profiles.to_string().contains("Session"), "{profiles}");

    let params = directory.path().join("create-profile.json");
    std::fs::write(
        &params,
        json!({
            "request": {
                "metadata": {"name": "Session"},
                "definition": {
                    "type": "config",
                    "config": {
                        "type": "file",
                        "source": {
                            "type": "local",
                            "binding": {"type": "managed", "file": "pending.yaml"}
                        }
                    }
                }
            },
            "fileData": PROFILE,
        })
        .to_string(),
    )
    .unwrap();
    let (code, created) = cli(
        &socket,
        vec![
            "call".into(),
            "create_profile".into(),
            "--params-file".into(),
            params.display().to_string(),
        ],
    )
    .await;
    assert_eq!(code, 0, "{created}");
    let uid = created["value"]
        .as_str()
        .expect("the created uid")
        .to_owned();

    // A new process finds what the previous one created: the server kept it.
    let (code, profiles) = cli(&socket, call("get_profiles")).await;
    assert_eq!(code, 0, "{profiles}");
    assert!(profiles.to_string().contains(&uid), "{uid} in {profiles}");
    assert!(profiles.to_string().contains("Session"), "{profiles}");

    // The parameters are camelCase, and the content the first call sent is not
    // dropped.
    let (code, content) = cli(
        &socket,
        vec![
            "call".into(),
            "read_profile_file".into(),
            "--params".into(),
            json!({ "uid": uid }).to_string(),
        ],
    )
    .await;
    assert_eq!(code, 0, "{content}");
    // The server stores the profile re-serialized, so compare what it holds.
    assert!(
        content.as_str().unwrap().contains("mixed-port: 17890"),
        "{content}"
    );

    let (code, error) = cli(&socket, call("no_such_command")).await;
    assert_eq!(code, 1, "{error}");
    assert_eq!(error["kind"], "unknown_method");

    stop.cancel();
    serving.await.unwrap().unwrap();
    server.shutdown().await;
}
