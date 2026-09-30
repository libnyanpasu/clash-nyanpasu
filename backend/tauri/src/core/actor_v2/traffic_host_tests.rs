use super::*;
use crate::core::actor_v2::{
    CoreClient,
    endpoint::{ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost},
};
use nyanpasu_core_manager::{CoreError, OperationId};
use nyanpasu_ipc::api::{
    core::v2::{CoreApiConnection, OperationInfo},
    status::CoreControllerInfo,
};
use nyanpasu_traffic::test_support::{FakeClock, FakeTrafficStore};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

struct ConfigEndpoint {
    inner: Arc<super::super::api::tests::Endpoint>,
    api_error: std::sync::atomic::AtomicBool,
    config: std::sync::Mutex<(
        String,
        Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>,
    )>,
}
#[async_trait::async_trait]
impl ControlEndpoint for ConfigEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Service
    }
    async fn api_connection(&self) -> Result<Option<CoreApiConnection>, CoreError> {
        if self.api_error.load(Ordering::SeqCst) {
            return Err(CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::BackendUnavailable,
                "transient authority failure",
                true,
            ));
        }
        self.inner.api_connection().await
    }
    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        let mut status = self.inner.status().await?;
        status.revision = Some(nyanpasu_ipc::api::status::RevisionIdInfo {
            epoch: 1,
            generation: 1,
            effective_hash: self.config.lock().unwrap().0.clone(),
        });
        Ok(status)
    }
    async fn effective_config(
        &self,
    ) -> Result<Option<nyanpasu_ipc::api::core::v2::CoreEffectiveConfig>, CoreError> {
        match self.config.lock().unwrap().1.clone() {
            Some(snapshot) => Ok(Some(snapshot)),
            None => Err(CoreError::new(
                nyanpasu_core_manager::CoreErrorKind::BackendUnavailable,
                "config unavailable",
                false,
            )),
        }
    }
    async fn submit(&self, request: CoreSubmission) -> Result<OperationInfo, CoreError> {
        self.inner.submit(request).await
    }
    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        self.inner.wait_operation(id, timeout).await
    }
}

async fn controlled_server() -> (
    String,
    tokio::task::JoinHandle<()>,
    tokio::sync::mpsc::UnboundedReceiver<tokio::sync::mpsc::UnboundedSender<String>>,
) {
    use axum::{
        Router,
        extract::ws::{Message, WebSocketUpgrade},
        routing::get,
    };
    let (opened, receiver) = tokio::sync::mpsc::unbounded_channel();
    let (url, server) = super::super::api::tests::server(Router::new().route("/connections", get(move |ws: WebSocketUpgrade| {
        let opened = opened.clone();
        async move { ws.on_upgrade(move |mut socket| async move {
            let (sender, mut frames) = tokio::sync::mpsc::unbounded_channel::<String>();
            opened.send(sender).unwrap();
            loop { tokio::select! {
                frame = frames.recv() => match frame { Some(frame) => if socket.send(Message::Text(frame.into())).await.is_err() { break; }, None => break },
                frame = socket.recv() => if frame.is_none() { break; },
            } }
        }) }
    }))).await;
    (url, server, receiver)
}

async fn send_sample(
    traffic: &TrafficClient,
    frames: &tokio::sync::mpsc::UnboundedSender<String>,
    upload: i64,
) -> ConnectionRecord {
    let session = traffic.current_session().await.unwrap().unwrap();
    let previous = session.position.sequence;
    let mut summary = traffic.subscribe_summary();
    frames.send(serde_json::json!({"uploadTotal":upload,"downloadTotal":0,"connections":[{
        "id":format!("00000000-0000-0000-0000-{upload:012}"), "start":"2026-09-30T02:00:00Z",
        "upload":upload,"download":0,"rule":"DOMAIN","rulePayload":"example.org","chains":["DIRECT"]
    }]}).to_string()).unwrap();
    loop {
        summary.changed().await.unwrap();
        if summary
            .borrow_and_update()
            .as_ref()
            .is_some_and(|summary| summary.session.position.sequence > previous)
        {
            break;
        }
    }
    traffic
        .query_connections(ConnectionsQuery {
            session_id: session.id,
            filter: ConnectionFilter {
                status: Some(true),
                ..ConnectionFilter::default()
            },
            limit: 20,
            cursor: None,
        })
        .await
        .unwrap()
        .connections
        .remove(0)
}

#[tokio::test]
async fn context_is_cleared_on_failed_revision_fetch_and_before_same_uuid_rebind() {
    let (url, server, mut connections) = controlled_server().await;
    let inner = super::super::api::tests::endpoint(url);
    let snapshot =
        |instance: &str, revision: &str| nyanpasu_ipc::api::core::v2::CoreEffectiveConfig {
            instance_id: instance.into(),
            revision: nyanpasu_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation: 1,
                source_hash: "source".into(),
                effective_hash: revision.into(),
            },
            config: "rules: ['DOMAIN,example.org,DIRECT']\n".into(),
        };
    let endpoint = Arc::new(ConfigEndpoint {
        inner: inner.clone(),
        api_error: std::sync::atomic::AtomicBool::new(false),
        config: std::sync::Mutex::new(("r1".into(), Some(snapshot("first-process", "r1")))),
    });
    let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
    core.refresh_status().await.unwrap();
    let traffic = TrafficClient::start(TrafficActorArgs {
        host: HostId("application".into()),
        source: Arc::new(nyanpasu_traffic::test_support::IdleSource),
        store: Arc::new(FakeTrafficStore::default()),
        clock: Arc::new(FakeClock::new(1000, 0)),
        cancellation: CancellationToken::new(),
    })
    .await
    .unwrap();
    let mut bridge = ApiBridge::default();
    bridge.refresh(&core, &traffic).await.unwrap();
    let frames = connections.recv().await.unwrap();
    assert_eq!(
        send_sample(&traffic, &frames, 1)
            .await
            .dimensions
            .rule
            .context
            .as_deref(),
        Some("r1")
    );
    *endpoint.config.lock().unwrap() = ("r2".into(), None);
    core.refresh_status().await.unwrap();
    bridge.refresh(&core, &traffic).await.unwrap();
    assert_eq!(
        send_sample(&traffic, &frames, 2)
            .await
            .dimensions
            .rule
            .context,
        None
    );
    *endpoint.config.lock().unwrap() = ("r2".into(), Some(snapshot("first-process", "r2")));
    bridge.refresh(&core, &traffic).await.unwrap();
    assert_eq!(
        send_sample(&traffic, &frames, 3)
            .await
            .dimensions
            .rule
            .context
            .as_deref(),
        Some("r2")
    );
    let binding = inner.binding.send_replace(None);
    bridge.refresh(&core, &traffic).await.unwrap();
    inner.binding.send_replace(binding);
    *endpoint.config.lock().unwrap() = ("r3".into(), Some(snapshot("wrong-process", "r3")));
    core.refresh_status().await.unwrap();
    bridge.refresh(&core, &traffic).await.unwrap();
    let resumed = connections.recv().await.unwrap();
    assert_eq!(
        send_sample(&traffic, &resumed, 4)
            .await
            .dimensions
            .rule
            .context,
        None
    );
    traffic.shutdown().await.unwrap();
    core.shutdown().await.unwrap();
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn replaced_api_identity_is_fenced_before_bridge_refresh() {
    let (url, server, mut connections) = controlled_server().await;
    let inner = super::super::api::tests::endpoint(url);
    let core = CoreClient::spawn(Arc::new(Endpoint {
        inner: inner.clone(),
        host: ExecutionHost::Service,
    }))
    .await
    .unwrap();
    let traffic = TrafficClient::start(TrafficActorArgs {
        host: HostId("application".into()),
        source: Arc::new(nyanpasu_traffic::test_support::IdleSource),
        store: Arc::new(FakeTrafficStore::default()),
        clock: Arc::new(FakeClock::new(1000, 0)),
        cancellation: CancellationToken::new(),
    })
    .await
    .unwrap();
    let mut bridge = ApiBridge::default();
    bridge.refresh(&core, &traffic).await.unwrap();
    let frames = connections.recv().await.unwrap();
    send_sample(&traffic, &frames, 1).await;
    let before = traffic.current_session().await.unwrap().unwrap();
    let mut status = traffic.subscribe_status();
    inner.binding.send_modify(|binding| {
        binding.as_mut().unwrap().instance_id = "replacement-process".into()
    });
    let _ = frames.send(r#"{"uploadTotal":9999,"downloadTotal":9999,"connections":[]}"#.into());
    loop {
        status.changed().await.unwrap();
        if status.borrow_and_update().is_some() {
            break;
        }
    }
    let after = traffic.session(before.id).await.unwrap();
    assert_eq!(after.position, before.position);
    assert_eq!(after.core_reported_bytes, before.core_reported_bytes);
    assert!(after.ended_at.is_none());
    traffic.shutdown().await.unwrap();
    core.shutdown().await.unwrap();
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn revoked_lease_is_replaced_when_the_same_binding_recovers_between_polls() {
    let (url, server, mut connections) = controlled_server().await;
    let endpoint = Arc::new(ConfigEndpoint {
        inner: super::super::api::tests::endpoint(url),
        api_error: std::sync::atomic::AtomicBool::new(false),
        config: std::sync::Mutex::new(("unknown".into(), None)),
    });
    let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
    let traffic = TrafficClient::start(TrafficActorArgs {
        host: HostId("application".into()),
        source: Arc::new(nyanpasu_traffic::test_support::IdleSource),
        store: Arc::new(FakeTrafficStore::default()),
        clock: Arc::new(FakeClock::new(1000, 0)),
        cancellation: CancellationToken::new(),
    })
    .await
    .unwrap();
    let mut bridge = ApiBridge::default();
    bridge.refresh(&core, &traffic).await.unwrap();
    let frames = connections.recv().await.unwrap();
    send_sample(&traffic, &frames, 1).await;
    let session = traffic.current_session().await.unwrap().unwrap();
    let original_binding = bridge.binding.clone();
    endpoint.api_error.store(true, Ordering::SeqCst);
    assert!(bridge.api.as_ref().unwrap().connections().await.is_err());
    assert!(bridge.api.as_ref().unwrap().is_revoked());
    endpoint.api_error.store(false, Ordering::SeqCst);
    // The bridge never observed a None binding or a different process UUID.
    bridge.refresh(&core, &traffic).await.unwrap();
    assert_eq!(bridge.binding, original_binding);
    assert!(!bridge.api.as_ref().unwrap().is_revoked());
    let resumed = connections.recv().await.unwrap();
    send_sample(&traffic, &resumed, 2).await;
    let recovered = traffic.current_session().await.unwrap().unwrap();
    assert_eq!(recovered.id, session.id);
    assert!(recovered.source_generation > session.source_generation);
    assert!(recovered.ended_at.is_none());
    assert!(
        traffic
            .subscribe_summary()
            .borrow()
            .as_ref()
            .unwrap()
            .current_rate
            .is_none()
    );
    traffic.shutdown().await.unwrap();
    core.shutdown().await.unwrap();
    server.abort();
    let _ = server.await;
}

struct Endpoint {
    inner: Arc<super::super::api::tests::Endpoint>,
    host: ExecutionHost,
}
#[async_trait::async_trait]
impl ControlEndpoint for Endpoint {
    fn host(&self) -> ExecutionHost {
        self.host
    }
    async fn api_connection(&self) -> Result<Option<CoreApiConnection>, CoreError> {
        self.inner.api_connection().await
    }
    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        self.inner.status().await
    }
    async fn submit(&self, request: CoreSubmission) -> Result<OperationInfo, CoreError> {
        self.inner.submit(request).await
    }
    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        self.inner.wait_operation(id, timeout).await
    }
}
struct Dropped(Arc<AtomicUsize>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct Source {
    opened: tokio::sync::mpsc::UnboundedSender<String>,
    dropped: Arc<AtomicUsize>,
}
#[async_trait::async_trait]
impl TrafficSource for Source {
    async fn connect(
        &self,
        binding: SourceBinding,
        _: UInt,
        _: Arc<dyn Clock>,
    ) -> TrafficResult<ObservationStream> {
        use futures_util::StreamExt;
        let guard = Dropped(self.dropped.clone());
        self.opened.send(binding.instance_id).unwrap();
        Ok(Box::pin(futures_util::stream::pending().map(
            move |value| {
                let _ = &guard;
                value
            },
        )))
    }
}

async fn refresh_with_source(
    bridge: &mut ApiBridge,
    core: &CoreClient,
    traffic: &TrafficClient,
    source: Arc<dyn TrafficSource>,
) {
    let binding = match core.connected_endpoint().await {
        Ok(endpoint) => endpoint.api_connection().await.unwrap(),
        Err(_) => None,
    };
    bridge
        .bind(traffic, binding, None, Some(source))
        .await
        .unwrap();
}

#[tokio::test]
async fn api_bridge_switches_both_hosts_and_detaches_without_claiming_remote_exit() {
    let local = super::super::api::tests::endpoint("http://localhost:9090/".into());
    let remote = super::super::api::tests::endpoint("http://localhost:9091/".into());
    remote
        .binding
        .send_modify(|binding| binding.as_mut().unwrap().instance_id = "service-process".into());
    let core = CoreClient::spawn(Arc::new(Endpoint {
        inner: local.clone(),
        host: ExecutionHost::Local,
    }))
    .await
    .unwrap();
    let (opened, mut opens) = tokio::sync::mpsc::unbounded_channel();
    let dropped = Arc::new(AtomicUsize::new(0));
    let source = Arc::new(Source {
        opened,
        dropped: dropped.clone(),
    });
    let traffic = TrafficClient::start(TrafficActorArgs {
        host: HostId("application".into()),
        source: source.clone(),
        store: Arc::new(FakeTrafficStore::default()),
        clock: Arc::new(FakeClock::new(1000, 0)),
        cancellation: CancellationToken::new(),
    })
    .await
    .unwrap();
    let mut bridge = ApiBridge::default();
    refresh_with_source(&mut bridge, &core, &traffic, source.clone()).await;
    assert_eq!(opens.recv().await.unwrap(), "first-process");
    let local_session = traffic.current_session().await.unwrap().unwrap();
    core.change_host(Arc::new(Endpoint {
        inner: remote.clone(),
        host: ExecutionHost::Service,
    }))
    .await
    .unwrap();
    refresh_with_source(&mut bridge, &core, &traffic, source.clone()).await;
    assert_eq!(opens.recv().await.unwrap(), "service-process");
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    let remote_session = traffic.current_session().await.unwrap().unwrap();
    assert!(remote_session.quality.contains(&Quality::LateAttach));
    assert!(
        traffic
            .session(local_session.id.clone())
            .await
            .unwrap()
            .ended_at
            .is_none()
    );

    let old_binding = remote.binding.send_replace(None);
    refresh_with_source(&mut bridge, &core, &traffic, source.clone()).await;
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
    assert!(traffic.subscribe_summary().borrow().is_none());
    let history = traffic.session(remote_session.id.clone()).await.unwrap();
    assert!(history.ended_at.is_none());
    assert!(history.quality.contains(&Quality::LifecycleUnknown));
    remote.binding.send_replace(old_binding);
    refresh_with_source(&mut bridge, &core, &traffic, source.clone()).await;
    assert_eq!(opens.recv().await.unwrap(), "service-process");
    assert_eq!(
        traffic.current_session().await.unwrap().unwrap().id,
        remote_session.id
    );

    local.binding.send_replace(Some(CoreApiConnection {
        instance_id: "next-local-process".into(),
        controller: CoreControllerInfo::Http("http://localhost:9090/".into()),
        secret: None,
    }));
    core.change_host(Arc::new(Endpoint {
        inner: local,
        host: ExecutionHost::Local,
    }))
    .await
    .unwrap();
    refresh_with_source(&mut bridge, &core, &traffic, source.clone()).await;
    assert_eq!(opens.recv().await.unwrap(), "next-local-process");
    assert_eq!(dropped.load(Ordering::SeqCst), 3);
    assert!(
        traffic
            .session(remote_session.id)
            .await
            .unwrap()
            .ended_at
            .is_none()
    );
    core.shutdown().await.unwrap();
    refresh_with_source(&mut bridge, &core, &traffic, source.clone()).await;
    assert!(
        traffic
            .session(local_session.id)
            .await
            .unwrap()
            .ended_at
            .is_none()
    );
    traffic.shutdown().await.unwrap();
}

#[tokio::test]
async fn service_binding_collects_from_existing_connections_api_with_credentials() {
    use axum::{
        Router,
        extract::ws::{Message, WebSocketUpgrade},
        http::{HeaderMap, StatusCode},
        response::IntoResponse,
        routing::get,
    };
    let (url, server) = super::super::api::tests::server(Router::new().route(
        "/connections",
        get(|headers: HeaderMap, ws: WebSocketUpgrade| async move {
            if headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                != Some("Bearer private")
            {
                return StatusCode::UNAUTHORIZED.into_response();
            }
            ws.on_upgrade(|mut socket| async move {
                socket
                    .send(Message::Text(
                        r#"{"uploadTotal":123,"downloadTotal":456,"connections":[]}"#.into(),
                    ))
                    .await
                    .unwrap();
                while socket.recv().await.is_some() {}
            })
            .into_response()
        }),
    ))
    .await;
    let endpoint = super::super::api::tests::endpoint(url);
    endpoint.binding.send_modify(|binding| {
        let binding = binding.as_mut().unwrap();
        binding.instance_id = "old-service-api-process".into();
        binding.secret = Some("private".into());
    });
    let core = CoreClient::spawn(Arc::new(Endpoint {
        inner: endpoint,
        host: ExecutionHost::Service,
    }))
    .await
    .unwrap();
    let traffic = TrafficClient::start(TrafficActorArgs {
        host: HostId("application".into()),
        source: Arc::new(ClashTrafficSource::default()),
        store: Arc::new(FakeTrafficStore::default()),
        clock: Arc::new(FakeClock::new(1000, 0)),
        cancellation: CancellationToken::new(),
    })
    .await
    .unwrap();
    let mut summary = traffic.subscribe_summary();
    ApiBridge::default().refresh(&core, &traffic).await.unwrap();
    loop {
        summary.changed().await.unwrap();
        if summary
            .borrow_and_update()
            .as_ref()
            .is_some_and(|summary| summary.session.position.sequence.0 > 0)
        {
            break;
        }
    }
    let session = traffic.current_session().await.unwrap().unwrap();
    assert_eq!(session.instance_id, "old-service-api-process");
    assert_eq!(session.global_counters.as_ref().unwrap().upload, UInt(123));
    assert_eq!(
        session.global_counters.as_ref().unwrap().download,
        UInt(456)
    );
    traffic.shutdown().await.unwrap();
    core.shutdown().await.unwrap();
    server.abort();
    let _ = server.await;
}
