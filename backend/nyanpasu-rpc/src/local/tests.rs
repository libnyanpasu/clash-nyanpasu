use nyanpasu_rpc_protocol::{LocalClient, ShellCapability, frame::LocalReply};
use tokio::io::AsyncWriteExt as _;

use super::*;
use crate::{EventBus, registry::tests::test_rpc};

/// A client connected to `rpc` through an in-memory stream, as `owner`.
fn connect(rpc: &UnifiedRpc, owner: RpcOwner, tasks: &TaskTracker) -> LocalClient {
    let (client, server) = tokio::io::duplex(64 * 1024);
    tasks.spawn(serve_stream(
        server,
        owner,
        rpc.clone(),
        CancellationToken::new(),
        tasks.clone(),
    ));
    LocalClient::from_stream(client)
}

#[tokio::test]
async fn one_connection_carries_sequential_calls_and_refuses_unexposed_commands() {
    let directory = tempfile::tempdir().unwrap();
    let rpc = test_rpc(&directory, EventBus::new(Vec::new()), &ShellCapability::ALL).await;
    let tasks = TaskTracker::new();
    let mut client = connect(&rpc, RpcOwner::local_connection(1), &tasks);

    let first = client
        .call("get_profiles", serde_json::json!({}))
        .await
        .unwrap()
        .unwrap();
    let second = client
        .call("get_profiles", Value::Null)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first, second);
    assert!(first.get("items").is_some(), "{first}");

    let refused = client
        .call("desktop_only_probe", Value::Null)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(refused.kind, RpcErrorKind::Unsupported);
    assert_eq!(
        refused.message,
        "command `desktop_only_probe` is unavailable over the local socket"
    );
    let unknown = client
        .call("missing", Value::Null)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(unknown.kind, RpcErrorKind::UnknownMethod);
    let owner = client
        .call("owner_probe", Value::Null)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(owner, "local:1");
}

#[tokio::test]
async fn malformed_frames_get_an_invalid_request_and_the_connection_survives() {
    let directory = tempfile::tempdir().unwrap();
    let rpc = test_rpc(&directory, EventBus::new(Vec::new()), &ShellCapability::ALL).await;
    let tasks = TaskTracker::new();
    let (mut raw, server) = tokio::io::duplex(64 * 1024);
    tasks.spawn(serve_stream(
        server,
        RpcOwner::local_connection(1),
        rpc,
        CancellationToken::new(),
        tasks.clone(),
    ));

    async fn send_body(raw: &mut tokio::io::DuplexStream, body: &[u8]) -> LocalResponse {
        raw.write_all(&u32::try_from(body.len()).unwrap().to_be_bytes())
            .await
            .unwrap();
        raw.write_all(body).await.unwrap();
        let reply = frame::read_frame(raw).await.unwrap().unwrap();
        serde_json::from_slice(&reply).unwrap()
    }
    for (body, id) in [
        (&b"not json"[..], None),
        (br#"{"v":2,"id":5,"method":"get_profiles"}"#, Some(5)),
        (br#"{"v":1,"id":6}"#, Some(6)),
    ] {
        let response = send_body(&mut raw, body).await;
        assert_eq!(response.id, id);
        let LocalReply::Error(error) = response.reply else {
            panic!("{body:?} was answered");
        };
        assert_eq!(error.kind, RpcErrorKind::InvalidRequest);
    }
    let response = send_body(&mut raw, br#"{"v":1,"id":7,"method":"get_profiles"}"#).await;
    assert_eq!(response.id, Some(7));
    assert!(matches!(response.reply, LocalReply::Result(_)));

    raw.write_all(
        &u32::try_from(frame::MAX_FRAME_LEN + 1)
            .unwrap()
            .to_be_bytes(),
    )
    .await
    .unwrap();
    let reply = frame::read_frame(&mut raw).await.unwrap().unwrap();
    let response: LocalResponse = serde_json::from_slice(&reply).unwrap();
    assert!(matches!(
        response.reply,
        LocalReply::Error(RpcError {
            kind: RpcErrorKind::InvalidRequest,
            ..
        })
    ));
    assert_eq!(frame::read_frame(&mut raw).await.unwrap(), None);
}

#[tokio::test]
async fn a_panicking_command_gets_an_internal_error_and_the_connection_survives() {
    let directory = tempfile::tempdir().unwrap();
    let rpc = test_rpc(&directory, EventBus::new(Vec::new()), &ShellCapability::ALL).await;
    let tasks = TaskTracker::new();
    let mut client = connect(&rpc, RpcOwner::local_connection(1), &tasks);

    let panicked = client
        .call("panic_probe", Value::Null)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(panicked.kind, RpcErrorKind::Internal);
    assert!(
        panicked.message.contains("panic_probe"),
        "{}",
        panicked.message
    );
    let after = client
        .call("get_profiles", Value::Null)
        .await
        .unwrap()
        .unwrap();
    assert!(after.get("items").is_some(), "{after}");
}

#[tokio::test]
async fn a_call_completes_after_its_connection_closes() {
    let directory = tempfile::tempdir().unwrap();
    let rpc = test_rpc(&directory, EventBus::new(Vec::new()), &ShellCapability::ALL).await;
    let tasks = TaskTracker::new();
    let (mut raw, server) = tokio::io::duplex(64 * 1024);
    tasks.spawn(serve_stream(
        server,
        RpcOwner::local_connection(1),
        rpc.clone(),
        CancellationToken::new(),
        tasks.clone(),
    ));
    let request = frame::encode(&LocalRequest::new(1, "reconcile_core", Value::Null)).unwrap();
    raw.write_all(&request).await.unwrap();
    drop(raw);
    tasks.close();
    tasks.wait().await;
    let yaml = rpc
        .call(
            RpcTarget::Local,
            "get_runtime_yaml_redacted",
            RpcOwner::local_connection(2),
            Value::Null,
        )
        .await;
    assert!(yaml.is_ok(), "the reconcile ran to completion: {yaml:?}");
}

#[cfg(feature = "http")]
#[tokio::test]
async fn failures_and_owners_have_the_same_shape_on_http_and_the_local_socket() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt as _;

    let directory = tempfile::tempdir().unwrap();
    let rpc = test_rpc(&directory, EventBus::new(Vec::new()), &ShellCapability::ALL).await;
    let app = crate::http::router(rpc.clone());
    let tasks = TaskTracker::new();
    let mut client = connect(&rpc, RpcOwner::local_connection(9), &tasks);

    let session = uuid::Uuid::new_v4();
    let http = |method: &str| {
        let app = app.clone();
        let body = serde_json::json!({"method": method, "params": {}}).to_string();
        async move {
            let response = app
                .oneshot(
                    Request::post("/bridge/rpc")
                        .header("content-type", "application/json")
                        .header("cookie", format!("nyanpasu_http_session={session}"))
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        }
    };

    let over_http = http("get_runtime_yaml").await;
    let over_local = client
        .call("get_runtime_yaml", Value::Null)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(over_http, serde_json::to_value(&over_local).unwrap());
    assert_eq!(over_local.kind, RpcErrorKind::ApplicationError);

    assert_eq!(http("owner_probe").await, format!("http:{session}"));
    assert_eq!(
        client
            .call("owner_probe", Value::Null)
            .await
            .unwrap()
            .unwrap(),
        "local:9"
    );
    let reconcile_over_http = http("reconcile_core").await;
    assert_eq!(reconcile_over_http["kind"], "unsupported");
}

#[cfg(unix)]
#[tokio::test]
async fn the_unix_socket_is_owner_only_and_each_connection_is_an_owner() {
    use std::os::unix::fs::PermissionsExt as _;

    let directory = tempfile::tempdir().unwrap();
    let rpc = test_rpc(&directory, EventBus::new(Vec::new()), &ShellCapability::ALL).await;
    let socket = directory.path().join("run/nyanpasu/rpc.sock");
    let shutdown = CancellationToken::new();
    let server = tokio::spawn({
        let (socket, rpc, shutdown) = (socket.clone(), rpc.clone(), shutdown.clone());
        async move { serve_local(&socket, rpc, shutdown).await }
    });
    let mut first = loop {
        match LocalClient::connect(&socket).await {
            Ok(client) => break client,
            Err(_) => tokio::task::yield_now().await,
        }
    };
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&socket), 0o600);
    assert_eq!(mode(socket.parent().unwrap()), 0o700);

    let profiles = first
        .call("get_profiles", Value::Null)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        first
            .call("get_profiles", Value::Null)
            .await
            .unwrap()
            .unwrap(),
        profiles
    );
    let mut second = LocalClient::connect(&socket).await.unwrap();
    let owners = [
        first
            .call("owner_probe", Value::Null)
            .await
            .unwrap()
            .unwrap(),
        second
            .call("owner_probe", Value::Null)
            .await
            .unwrap()
            .unwrap(),
    ];
    assert_ne!(owners[0], owners[1]);

    // A live server keeps its socket.
    let refused = serve_local(&socket, rpc.clone(), CancellationToken::new()).await;
    assert!(matches!(
        refused,
        Err(LocalServeError::AlreadyServing { .. })
    ));

    shutdown.cancel();
    server.await.unwrap().unwrap();
    assert!(!socket.exists());

    // A socket file left by a server that is gone is replaced.
    let stale = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    drop(stale);
    let shutdown = CancellationToken::new();
    let server = tokio::spawn({
        let (socket, shutdown) = (socket.clone(), shutdown.clone());
        async move { serve_local(&socket, rpc, shutdown).await }
    });
    let mut client = loop {
        match LocalClient::connect(&socket).await {
            Ok(client) => break client,
            Err(_) => tokio::task::yield_now().await,
        }
    };
    assert!(
        client
            .call("get_profiles", Value::Null)
            .await
            .unwrap()
            .is_ok()
    );
    shutdown.cancel();
    server.await.unwrap().unwrap();
}
