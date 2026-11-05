//! `run` composes the real headless application, serves the local socket and
//! stops cleanly. No core binary is installed, so startup cannot start a core;
//! the server must still serve, because the user repairs it through the socket.
#![cfg(unix)]

use std::time::Duration;

use camino::Utf8PathBuf;
use nyanpasu_rpc_protocol::{LocalClient, RpcErrorKind};
use nyanpasu_server::{
    compose::ServerDirs,
    run::{RunOptions, WebOptions, run},
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
            web: None,
        },
        stop.clone(),
    ));

    let mut client = connect(&socket).await;
    // Without a listener there is nothing to sign a browser in to.
    let refused = client
        .call("issue_web_ticket", json!({}))
        .await
        .unwrap()
        .unwrap_err();
    assert!(refused.message.contains("not being served"), "{refused}");
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

/// The WebUI listener and the local socket serve one application: a ticket
/// issued on the socket signs a browser in, and its session reaches the API and
/// the assets the server was given.
#[tokio::test(flavor = "multi_thread")]
async fn a_ticket_from_the_socket_signs_a_browser_in_to_the_webui() {
    let directory = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_owned()).unwrap();
    let socket = directory.path().join("rpc.sock");
    let webui = directory.path().join("webui");
    std::fs::create_dir(&webui).unwrap();
    std::fs::write(webui.join("index.html"), "<html>webui</html>").unwrap();
    let stop = CancellationToken::new();
    let server = tokio::spawn(run(
        RunOptions {
            dirs: ServerDirs {
                config_dir: root.join("config"),
                data_dir: root.join("data"),
            },
            rpc_socket: socket.clone(),
            web: Some(WebOptions {
                listen: ([127, 0, 0, 1], 0).into(),
                dir: Some(webui),
            }),
        },
        stop.clone(),
    ));
    let mut client = connect(&socket).await;
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let mut issue = async || {
        let ticket = client
            .call("issue_web_ticket", json!({}))
            .await
            .unwrap()
            .unwrap();
        format!(
            "http://127.0.0.1:{}{}",
            ticket["port"].as_u64().unwrap(),
            ticket["path"].as_str().unwrap()
        )
    };

    let first = issue().await;
    let base = first.split_once("/?").unwrap().0.to_owned();
    assert_eq!(
        http.get(format!("{base}/")).send().await.unwrap().status(),
        401,
        "no session"
    );
    let signed_in = http.get(&first).send().await.unwrap();
    assert_eq!(signed_in.status(), 303);
    let cookie = signed_in.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(
        http.get(&first).send().await.unwrap().status(),
        401,
        "a ticket is single use"
    );

    let page = http
        .get(format!("{base}/"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), 200);
    assert_eq!(page.text().await.unwrap(), "<html>webui</html>");
    let profiles = http
        .post(format!("{base}/bridge/rpc"))
        .header("cookie", &cookie)
        .json(&json!({"method": "get_profiles", "params": {}}))
        .send()
        .await
        .unwrap();
    assert_eq!(profiles.status(), 200);
    // A session cannot mint tickets, nor reach a command that is local only.
    let minted = http
        .post(format!("{base}/bridge/rpc"))
        .header("cookie", &cookie)
        .json(&json!({"method": "issue_web_ticket", "params": {}}))
        .send()
        .await
        .unwrap();
    assert_eq!(minted.status(), 501);

    stop.cancel();
    server.await.unwrap().unwrap();
    assert!(
        http.get(format!("{base}/")).send().await.is_err(),
        "the listener stops with the server"
    );
}

/// With no WebUI directory the listener serves the API alone.
#[tokio::test(flavor = "multi_thread")]
async fn without_a_webui_directory_the_listener_serves_only_the_api() {
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
            web: Some(WebOptions {
                listen: ([127, 0, 0, 1], 0).into(),
                dir: None,
            }),
        },
        stop.clone(),
    ));
    let mut client = connect(&socket).await;
    let ticket = client
        .call("issue_web_ticket", json!({}))
        .await
        .unwrap()
        .unwrap();
    let base = format!("http://127.0.0.1:{}", ticket["port"].as_u64().unwrap());
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let signed_in = http
        .get(format!("{base}{}", ticket["path"].as_str().unwrap()))
        .send()
        .await
        .unwrap();
    // The exchange redirects to the app, which is not here.
    assert_eq!(signed_in.status(), 303);
    let cookie = signed_in.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let page = http
        .get(format!("{base}/"))
        .header("cookie", cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(page.status(), 404);

    stop.cancel();
    server.await.unwrap().unwrap();
}
