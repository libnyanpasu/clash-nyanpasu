use std::{path::PathBuf, sync::Arc, time::Duration};

use axum::{Json, Router, extract::State, routing::post};
use interprocess::local_socket::{
    GenericFilePath, ListenerOptions, ToFsName as _,
    tokio::{Listener, Stream},
    traits::tokio::Listener as _,
};
use nyanpasu_contracts::logs::LogSource;
use nyanpasu_core::client::{
    logs::{IpcServiceLogs, ServiceLogsPort},
    test_support::TestControlEndpoint,
};
use nyanpasu_ipc::api::{
    RBuilder,
    log::{LOG_CLOSE_ENDPOINT, LOG_OPEN_ENDPOINT, LOG_QUERY_ENDPOINT, OwnedLogRequest},
};
use nyanpasu_logging::{
    Direction, Filter, FsLogFiles, LogError, LogsClient, MonotonicClock, OpenLogs, QueryLogs,
};
use nyanpasu_rpc_protocol::{
    RpcError, RpcOwner,
    frame::{Frame, Reply},
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::Notify;

use super::{ConnectionLimits, ListeningServer, LocalServer, Peer, Source, demand_count};
use crate::{
    registry::{CommandEntry, RpcDependencies, RpcFuture},
    test_support::{self, Fixture},
};

struct IpcListener(Listener);
impl axum::serve::Listener for IpcListener {
    type Io = Stream;
    type Addr = ();
    async fn accept(&mut self) -> (Stream, ()) {
        (self.0.accept().await.expect("controlled IPC accepts"), ())
    }
    fn local_addr(&self) -> std::io::Result<()> {
        Ok(())
    }
}

struct ServiceState {
    logs: LogsClient,
    opened: Mutex<Vec<(String, String)>>,
    closed: Mutex<Vec<(String, String)>>,
    hold: std::sync::atomic::AtomicBool,
    created: Notify,
    release: Notify,
}

struct ServicePeer {
    state: Arc<ServiceState>,
    port: Arc<IpcServiceLogs>,
    shutdown: tokio_util::sync::CancellationToken,
    task: tokio::task::JoinHandle<()>,
    _directory: tempfile::TempDir,
    #[cfg(unix)]
    socket: PathBuf,
}

impl ServicePeer {
    async fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(
            directory.path().join("service.e3d.app.log"),
            "service log\n",
        )
        .unwrap();
        let logs = LogsClient::start(
            Arc::new(FsLogFiles::new(
                directory.path().to_owned(),
                "service".into(),
            )),
            Arc::new(MonotonicClock::default()),
        )
        .await
        .unwrap();
        let state = Arc::new(ServiceState {
            logs,
            opened: Mutex::new(Vec::new()),
            closed: Mutex::new(Vec::new()),
            hold: false.into(),
            created: Notify::new(),
            release: Notify::new(),
        });
        let router = Router::new()
            .route(
                LOG_OPEN_ENDPOINT,
                post(
                    |State(state): State<Arc<ServiceState>>,
                     Json(body): Json<OwnedLogRequest<OpenLogs>>| async move {
                        let result = state.logs.open(body.owner.clone(), body.request).await;
                        if let Ok(session) = &result {
                            state.opened.lock().push((body.owner, session.id.clone()));
                        }
                        state.created.notify_one();
                        if state.hold.load(std::sync::atomic::Ordering::SeqCst) {
                            state.release.notified().await;
                        }
                        Json(RBuilder::success(result))
                    },
                ),
            )
            .route(
                LOG_QUERY_ENDPOINT,
                post(
                    |State(state): State<Arc<ServiceState>>,
                     Json(body): Json<OwnedLogRequest<QueryLogs>>| async move {
                        Json(RBuilder::success(
                            state.logs.query(body.owner, body.request).await,
                        ))
                    },
                ),
            )
            .route(
                LOG_CLOSE_ENDPOINT,
                post(
                    |State(state): State<Arc<ServiceState>>,
                     Json(body): Json<OwnedLogRequest<String>>| async move {
                        state
                            .closed
                            .lock()
                            .push((body.owner.clone(), body.request.clone()));
                        Json(RBuilder::success(
                            state.logs.close(body.owner, body.request).await,
                        ))
                    },
                ),
            )
            .with_state(state.clone());
        #[cfg(windows)]
        let placeholder = format!("nyanpasu-e3d-service-{}", uuid::Uuid::new_v4());
        #[cfg(windows)]
        let socket = PathBuf::from(format!(r"\\.\pipe\{placeholder}"));
        #[cfg(unix)]
        let (placeholder, socket) = {
            // The existing IPC client fixes /var/run as its base. Traverse that
            // canonical base into our private temporary directory without changing it.
            let base = std::path::Path::new("/var/run").canonicalize().unwrap();
            let depth = base
                .components()
                .filter(|part| matches!(part, std::path::Component::Normal(_)))
                .count();
            let directory = directory.path().canonicalize().unwrap();
            let placeholder = format!(
                "{}{}{}",
                "../".repeat(depth),
                directory.to_str().unwrap().trim_start_matches('/'),
                "/service"
            );
            (placeholder, directory.join("service.sock"))
        };
        let listener = ListenerOptions::new()
            .name(socket.as_path().to_fs_name::<GenericFilePath>().unwrap())
            .create_tokio()
            .unwrap();
        let shutdown = tokio_util::sync::CancellationToken::new();
        let cancelled = shutdown.clone();
        let task = tokio::spawn(async move {
            axum::serve(IpcListener(listener), router)
                .with_graceful_shutdown(cancelled.cancelled_owned())
                .await
                .unwrap();
        });
        Self {
            state,
            port: Arc::new(IpcServiceLogs::new(
                nyanpasu_ipc::client::Client::new(&placeholder).unwrap(),
            )),
            shutdown,
            task,
            _directory: directory,
            #[cfg(unix)]
            socket,
        }
    }

    async fn finish(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
        self.state.logs.shutdown().await.unwrap();
        // The Unix listener reclaims its socket file when dropped.
        #[cfg(unix)]
        match std::fs::remove_file(self.socket) {
            Ok(()) => {}
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
        }
    }
}

async fn fixture(service: Option<Arc<IpcServiceLogs>>) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let mut args =
        test_support::test_client_args_with_endpoint(&directory, TestControlEndpoint::succeeding())
            .await;
    std::fs::create_dir_all(args.paths.app_logs_dir()).unwrap();
    std::fs::write(
        args.paths.app_logs_dir().join("clash-nyanpasu.e3d.app.log"),
        "application log\n",
    )
    .unwrap();
    if let Some(service) = service {
        args.logging.service = service;
    }
    let mut fixture = Fixture::with_setup(false, test_support::CAPABILITIES, directory, args).await;
    for command in ["open_log_session", "query_logs", "close_log_session"] {
        fixture.rpc.enable_local_test_command(command, None);
    }
    fixture
}

fn delayed_open(
    deps: Arc<RpcDependencies>,
    owner: RpcOwner,
    params: Value,
) -> Result<RpcFuture, RpcError> {
    Ok(Box::pin(async move {
        let original = inventory::iter::<CommandEntry>
            .into_iter()
            .find(|entry| entry.metadata.name == "open_log_session")
            .unwrap()
            .handler;
        let mut release = deps.events.subscribe();
        let result = original(deps.clone(), owner.clone(), params.clone())
            .unwrap()
            .await;
        let value = result
            .result
            .as_ref()
            .expect("real log open succeeds before completion barrier");
        deps.events.publish(
            "opened",
            json!({"owner": owner.label(), "session": value["id"], "source": params["source"]}),
        );
        while release.recv().await.unwrap().0 != "release" {}
        result
    }))
}

fn query(session: &str) -> QueryLogs {
    QueryLogs {
        session: session.into(),
        filter: Filter::default(),
        direction: Direction::Latest,
        cursor: None,
        limit: 10,
    }
}

async fn delayed(source: LogSource, shutdown: bool) {
    let service = ServicePeer::new().await;
    let mut fixture = fixture(Some(service.port.clone())).await;
    fixture
        .rpc
        .enable_local_test_command("open_log_session", Some(delayed_open));
    let mut events = fixture.rpc.dependencies.events.subscribe();
    let server = LocalServer::new(
        fixture.rpc.clone(),
        ConnectionLimits {
            resources: 1,
            ..Default::default()
        },
    );
    let mut peer = if shutdown {
        Peer::start_server(server, &[]).await
    } else {
        Peer::start(server, &[]).await
    };
    peer.request(
        "open_log_session",
        json!({"source": source, "request": {"request_id":"late", "file":null}}),
    )
    .await;
    let (_, opened) = tokio::time::timeout(Duration::from_secs(4), events.recv())
        .await
        .expect("successful Service response is inside upstream timeout")
        .unwrap();
    let owner = opened["owner"].as_str().unwrap().to_owned();
    let session = opened["session"].as_str().unwrap().to_owned();
    peer.request(
        "open_log_session",
        json!({"source": source, "request": {"request_id":"over-capacity"}}),
    )
    .await;
    assert!(
        matches!(peer.read().await, Frame::Response { reply: Reply::Error(error), .. } if error["kind"] == "resource_limit"),
        "pending successful completion already reserves capacity"
    );
    if shutdown {
        peer.stop.cancel();
    } else {
        peer.disconnect().await;
    }
    fixture
        .rpc
        .dependencies
        .events
        .publish("release", Value::Null);
    tokio::time::timeout(Duration::from_secs(10), peer.task.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture
            .rpc
            .dependencies
            .client
            .query_logs(source, owner, query(&session))
            .await
            .unwrap_err(),
        LogError::SessionExpired
    );
    {
        let records = fixture.sink.records.lock();
        let late: Vec<_> = records
            .iter()
            .filter(|record| {
                record.header.request_id
                    == Some(nyanpasu_rpc_protocol::frame::RequestId::new(1).unwrap())
            })
            .collect();
        assert_eq!(late.len(), 3);
        assert!(matches!(
            late[1].phase,
            crate::audit::AuditPhase::Finished {
                outcome: crate::audit::RpcAuditOutcome::Success,
                ..
            }
        ));
        assert_eq!(
            late[2].phase,
            crate::audit::AuditPhase::Delivery {
                outcome: crate::audit::DeliveryOutcome::NotAttempted
            }
        );
    }
    if matches!(source, LogSource::Service) {
        assert_eq!(
            *service.state.opened.lock(),
            *service.state.closed.lock(),
            "actual close uses the returned ID and the same namespaced owner"
        );
        assert_eq!(service.state.closed.lock().len(), 1);
    }
    fixture.shutdown().await;
    service.finish().await;
}

#[tokio::test]
async fn delayed_app_log_session_is_reclaimed_on_disconnect() {
    delayed(LogSource::App, false).await;
}

#[tokio::test]
async fn delayed_service_log_session_is_reclaimed_on_disconnect() {
    delayed(LogSource::Service, false).await;
}

#[tokio::test]
async fn shutdown_reclaims_delayed_app_and_service_log_sessions() {
    delayed(LogSource::App, true).await;
    delayed(LogSource::Service, true).await;
}

#[tokio::test]
async fn connection_resource_table_is_bounded() {
    let fixture = fixture(None).await;
    let mut peer = Peer::start(
        LocalServer::new(
            fixture.rpc.clone(),
            ConnectionLimits {
                resources: 1,
                ..Default::default()
            },
        ),
        &[],
    )
    .await;
    peer.request(
        "open_log_session",
        json!({"source":"app", "request":{"request_id":"one"}}),
    )
    .await;
    let Frame::Response {
        reply: Reply::Result(session),
        ..
    } = peer.read().await
    else {
        panic!("first log session opens")
    };
    peer.request(
        "open_log_session",
        json!({"source":"app", "request":{"request_id":"two"}}),
    )
    .await;
    assert!(
        matches!(peer.read().await, Frame::Response { reply: Reply::Error(error), .. } if error["kind"] == "resource_limit")
    );
    peer.request(
        "close_log_session",
        json!({"source":"app", "session":session["id"]}),
    )
    .await;
    assert!(matches!(
        peer.read().await,
        Frame::Response {
            reply: Reply::Result(_),
            ..
        }
    ));
    peer.request(
        "open_log_session",
        json!({"source":"app", "request":{"request_id":"three"}}),
    )
    .await;
    assert!(matches!(
        peer.read().await,
        Frame::Response {
            reply: Reply::Result(_),
            ..
        }
    ));
    peer.finish().await;
    fixture.shutdown().await;
}

#[tokio::test]
async fn local_owners_cannot_use_each_others_resources() {
    let service = ServicePeer::new().await;
    let fixture = fixture(Some(service.port.clone())).await;
    let source = Source::new();
    let listener = ListeningServer::start(super::server(
        &fixture,
        source.clone(),
        ConnectionLimits::default(),
    ))
    .await;
    let mut one = listener.connect(&["connection-details"]).await;
    let mut two = listener.connect(&["connection-details"]).await;
    let mut sessions = Vec::new();
    for peer in [&mut one, &mut two] {
        peer.subscribe("connection-details", None).await;
        assert!(matches!(peer.read().await, Frame::Subscribed { .. }));
        peer.request("open_log_session", json!({"source":"service", "owner":"forged-owner", "request":{"request_id":"same-request"}})).await;
        let Frame::Response {
            reply: Reply::Result(session),
            ..
        } = peer.read().await
        else {
            panic!("opens own resource")
        };
        sessions.push(session["id"].as_str().unwrap().to_owned());
    }
    assert_ne!(
        sessions[0], sessions[1],
        "deduplication is scoped to distinct connection owners"
    );
    {
        let opened = service.state.opened.lock();
        assert_ne!(
            opened[0].0, opened[1].0,
            "one server assigns distinct simultaneous owners"
        );
    }
    let params = json!({"source":"service", "owner":"forged-owner", "request":query(&sessions[0])});
    two.request("query_logs", params.clone()).await;
    assert!(matches!(
        two.read().await,
        Frame::Response {
            reply: Reply::Error(_),
            ..
        }
    ));
    two.request(
        "close_log_session",
        json!({"source":"service", "owner":"forged-owner", "session":sessions[0]}),
    )
    .await;
    let _ = two.read().await;
    one.request("query_logs", params).await;
    assert!(matches!(
        one.read().await,
        Frame::Response {
            reply: Reply::Result(_),
            ..
        }
    ));
    one.disconnect().await;
    demand_count(&source, 1).await;
    // Fence resource cleanup, not just the IO half closing.
    tokio::time::timeout(Duration::from_secs(10), async {
        let owner = service.state.opened.lock()[0].0.clone();
        while service
            .state
            .logs
            .query(owner.clone(), query(&sessions[0]))
            .await
            .is_ok()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first connection closes its own log session");
    two.request(
        "query_logs",
        json!({"source":"service", "request":query(&sessions[1])}),
    )
    .await;
    assert!(matches!(
        two.read().await,
        Frame::Response {
            reply: Reply::Result(_),
            ..
        }
    ));
    let mut three = listener.connect(&[]).await;
    three
        .request(
            "open_log_session",
            json!({"source":"service", "request":{"request_id":"same-request"}}),
        )
        .await;
    assert!(matches!(
        three.read().await,
        Frame::Response {
            reply: Reply::Result(_),
            ..
        }
    ));
    {
        let opened = service.state.opened.lock();
        assert_ne!(
            opened[0].0, opened[2].0,
            "reconnect never reuses the closed owner's identity"
        );
        assert_ne!(opened[1].0, opened[2].0);
    }
    three
        .request(
            "query_logs",
            json!({"source":"service", "request":query(&sessions[1])}),
        )
        .await;
    assert!(matches!(
        three.read().await,
        Frame::Response {
            reply: Reply::Error(_),
            ..
        }
    ));
    two.request(
        "query_logs",
        json!({"source":"service", "request":query(&sessions[1])}),
    )
    .await;
    assert!(matches!(
        two.read().await,
        Frame::Response {
            reply: Reply::Result(_),
            ..
        }
    ));
    two.finish().await;
    three.finish().await;
    listener.finish().await;
    assert_eq!(source.details.receiver_count(), 0);
    fixture.shutdown().await;
    service.finish().await;
}

fn delayed_close(
    deps: Arc<RpcDependencies>,
    owner: RpcOwner,
    params: Value,
) -> Result<RpcFuture, RpcError> {
    Ok(Box::pin(async move {
        let original = inventory::iter::<CommandEntry>
            .into_iter()
            .find(|entry| entry.metadata.name == "close_log_session")
            .unwrap()
            .handler;
        let mut release = deps.events.subscribe();
        let result = original(deps.clone(), owner, params).unwrap().await;
        assert!(
            result.result.is_ok(),
            "explicit close succeeds before its completion barrier"
        );
        deps.events.publish("closed", Value::Null);
        while release.recv().await.unwrap().0 != "release" {}
        result
    }))
}

#[tokio::test]
async fn disconnect_closes_duplicate_successful_log_ids_once() {
    let service = ServicePeer::new().await;
    let mut fixture = fixture(Some(service.port.clone())).await;
    fixture
        .rpc
        .enable_local_test_command("open_log_session", Some(delayed_open));
    let mut events = fixture.rpc.dependencies.events.subscribe();
    let mut peer = Peer::start(
        LocalServer::new(fixture.rpc.clone(), ConnectionLimits::default()),
        &[],
    )
    .await;
    let mut opened = Vec::new();
    for _ in 0..2 {
        peer.request(
            "open_log_session",
            json!({"source":"service", "request":{"request_id":"duplicate"}}),
        )
        .await;
        let (name, value) = events.recv().await.unwrap();
        assert_eq!(name, "opened");
        opened.push(value);
    }
    assert_eq!(
        opened[0], opened[1],
        "real service deduplicates successful opens"
    );
    peer.disconnect().await;
    fixture
        .rpc
        .dependencies
        .events
        .publish("release", Value::Null);
    peer.finish().await;
    let (owner, session) = service.state.opened.lock()[0].clone();
    assert_eq!(
        *service.state.closed.lock(),
        vec![(owner.clone(), session.clone())]
    );
    assert_eq!(
        service
            .state
            .logs
            .query(owner, query(&session))
            .await
            .unwrap_err(),
        LogError::SessionExpired
    );
    fixture.shutdown().await;
    service.finish().await;
}

#[tokio::test]
async fn disconnect_during_explicit_log_close_does_not_close_twice() {
    let service = ServicePeer::new().await;
    let mut fixture = fixture(Some(service.port.clone())).await;
    fixture
        .rpc
        .enable_local_test_command("close_log_session", Some(delayed_close));
    let mut events = fixture.rpc.dependencies.events.subscribe();
    let mut peer = Peer::start(
        LocalServer::new(fixture.rpc.clone(), ConnectionLimits::default()),
        &[],
    )
    .await;
    peer.request(
        "open_log_session",
        json!({"source":"service", "request":{"request_id":"explicit"}}),
    )
    .await;
    let Frame::Response {
        reply: Reply::Result(session),
        ..
    } = peer.read().await
    else {
        panic!("opens log session")
    };
    peer.request(
        "close_log_session",
        json!({"source":"service", "session":session["id"]}),
    )
    .await;
    assert_eq!(events.recv().await.unwrap().0, "closed");
    peer.disconnect().await;
    fixture
        .rpc
        .dependencies
        .events
        .publish("release", Value::Null);
    peer.finish().await;
    assert_eq!(*service.state.closed.lock(), *service.state.opened.lock());
    assert_eq!(service.state.closed.lock().len(), 1);
    fixture.shutdown().await;
    service.finish().await;
}

#[tokio::test]
async fn explicit_log_close_wins_over_earlier_delayed_duplicate_open() {
    let service = ServicePeer::new().await;
    let mut fixture = fixture(Some(service.port.clone())).await;
    fixture
        .rpc
        .enable_local_test_command("open_log_session", Some(delayed_open));
    let mut events = fixture.rpc.dependencies.events.subscribe();
    let mut peer = Peer::start(
        LocalServer::new(fixture.rpc.clone(), ConnectionLimits::default()),
        &[],
    )
    .await;
    let params = json!({"source":"service", "request":{"request_id":"stale-duplicate"}});
    peer.request("open_log_session", params.clone()).await;
    let (name, first) = events.recv().await.unwrap();
    assert_eq!(name, "opened");
    fixture
        .rpc
        .dependencies
        .events
        .publish("release", Value::Null);
    let Frame::Response {
        reply: Reply::Result(session),
        ..
    } = peer.read().await
    else {
        panic!("first open returns the session ID")
    };
    assert_eq!(events.recv().await.unwrap().0, "release");
    peer.request("open_log_session", params).await;
    let (name, duplicate) = events.recv().await.unwrap();
    assert_eq!(name, "opened");
    assert_eq!(
        first, duplicate,
        "duplicate open already obtained the same upstream ID"
    );
    peer.request(
        "close_log_session",
        json!({"source":"service", "session":session["id"]}),
    )
    .await;
    assert!(matches!(
        peer.read().await,
        Frame::Response {
            reply: Reply::Result(_),
            ..
        }
    ));
    assert_eq!(
        service.state.closed.lock().len(),
        1,
        "explicit close has completed"
    );
    peer.disconnect().await;
    fixture
        .rpc
        .dependencies
        .events
        .publish("release", Value::Null);
    peer.finish().await;
    let (owner, session) = service.state.opened.lock()[0].clone();
    assert_eq!(
        *service.state.closed.lock(),
        vec![(owner.clone(), session.clone())],
        "stale completion must not resurrect the explicitly closed session"
    );
    assert_eq!(
        service
            .state
            .logs
            .query(owner, query(&session))
            .await
            .unwrap_err(),
        LogError::SessionExpired
    );
    fixture.shutdown().await;
    service.finish().await;
}

#[tokio::test]
async fn service_open_lost_response_reports_unavailable_without_session_id() {
    let service = ServicePeer::new().await;
    service
        .state
        .hold
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let port = service.port.clone();
    let open = tokio::spawn(async move {
        port.open(
            "lost-response".into(),
            OpenLogs {
                request_id: "lost".into(),
                file: None,
            },
        )
        .await
    });
    service.state.created.notified().await;
    assert_eq!(
        service.state.opened.lock().len(),
        1,
        "the real remote LogsClient created a session"
    );
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(6)).await;
    assert_eq!(open.await.unwrap().unwrap_err(), LogError::Unavailable);
    tokio::time::resume();
    assert!(
        service.state.closed.lock().is_empty(),
        "no returned ID exists to close"
    );
    let (owner, session) = service.state.opened.lock()[0].clone();
    assert!(
        service
            .state
            .logs
            .query(owner, query(&session))
            .await
            .is_ok(),
        "unconfirmed creation remains under the existing service lease"
    );
    service.state.release.notify_one();
    service.finish().await;
}
