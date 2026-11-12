use std::{
    collections::HashSet,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use interprocess::local_socket::tokio::Stream;
use nyanpasu_core::clash::{connection_rates::ConnectionDetails, ws::ClashConnectionDetails};
use nyanpasu_rpc_protocol::{
    EventName, RpcError, RpcOwner,
    frame::{Frame, Reply, RequestId, SubscriptionErrorCode},
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::{
    sync::{Notify, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::instrument::WithSubscriber as _;
use tracing_subscriber::layer::SubscriberExt as _;

use super::{ConnectionLimits, LocalServer, serve_stream};
use crate::{
    audit::RpcTransport,
    local::{
        ENDPOINT_NAMESPACE,
        subscriptions::{Details, SubscriptionSource},
    },
    registry::{RpcDependencies, RpcFuture},
    test_support::{Capture, Fixture, SECRET},
};
use nyanpasu_single_instance::endpoint::{Endpoint, EndpointListener};

#[path = "log_tests.rs"]
mod logs;

#[path = "client_tests.rs"]
mod client;

#[path = "audit_tests.rs"]
mod audit;

struct Peer {
    stream: Option<Stream>,
    stop: tokio_util::sync::CancellationToken,
    task: Option<JoinHandle<()>>,
    _directory: Option<tempfile::TempDir>,
    next: u64,
    writer: Arc<WriterGate>,
}

#[derive(Default)]
struct WriterGate {
    blocked: AtomicBool,
    fail: AtomicBool,
    entered: Notify,
    waker: futures::task::AtomicWaker,
}

struct ControlledStream {
    stream: Stream,
    gate: Arc<WriterGate>,
}

impl tokio::io::AsyncRead for ControlledStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(context, buffer)
    }
}
impl tokio::io::AsyncWrite for ControlledStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.gate.waker.register(context.waker());
        if self.gate.blocked.load(Ordering::SeqCst) {
            self.gate.entered.notify_one();
            return Poll::Pending;
        }
        if self.gate.fail.load(Ordering::SeqCst) {
            return Poll::Ready(Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe)));
        }
        Pin::new(&mut self.stream).poll_write(context, buffer)
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(context)
    }
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(context)
    }
}

struct ListeningServer {
    endpoint: Endpoint,
    stop: CancellationToken,
    task: JoinHandle<()>,
    _directory: tempfile::TempDir,
}

impl ListeningServer {
    async fn start(server: LocalServer) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::discover(ENDPOINT_NAMESPACE, directory.path(), None).unwrap();
        let listener = EndpointListener::bind(&endpoint).await.unwrap();
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let task = tokio::spawn(async move {
            server.serve(listener, shutdown).await.unwrap();
        });
        Self {
            endpoint,
            stop,
            task,
            _directory: directory,
        }
    }

    async fn connect(&self, features: &[&str]) -> Peer {
        let mut peer = Peer {
            stream: Some(self.endpoint.connect().await.unwrap()),
            stop: self.stop.clone(),
            task: None,
            _directory: None,
            next: 0,
            writer: Arc::new(WriterGate::default()),
        };
        peer.send(Frame::Hello {
            versions: vec![1],
            required_features: vec!["rpc".into()],
            optional_features: features.iter().map(|s| (*s).into()).collect(),
        })
        .await;
        assert!(matches!(peer.read().await, Frame::Accept { .. }));
        peer
    }

    async fn finish(self) {
        self.stop.cancel();
        tokio::time::timeout(Duration::from_secs(10), self.task)
            .await
            .expect("listener drains all connections")
            .unwrap();
    }
}

impl Peer {
    async fn start_server(server: LocalServer, features: &[&str]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::discover(ENDPOINT_NAMESPACE, directory.path(), None).unwrap();
        let listener = EndpointListener::bind(&endpoint).await.unwrap();
        let stop = tokio_util::sync::CancellationToken::new();
        let shutdown = stop.clone();
        let task = tokio::spawn(async move {
            server.serve(listener, shutdown).await.unwrap();
        });
        let mut peer = Self {
            stream: Some(endpoint.connect().await.unwrap()),
            stop,
            task: Some(task),
            _directory: Some(directory),
            next: 0,
            writer: Arc::new(WriterGate::default()),
        };
        peer.send(Frame::Hello {
            versions: vec![1],
            required_features: vec!["rpc".into()],
            optional_features: features.iter().map(|s| (*s).into()).collect(),
        })
        .await;
        assert!(matches!(peer.read().await, Frame::Accept { .. }));
        peer
    }

    async fn start(server: LocalServer, features: &[&str]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::discover(ENDPOINT_NAMESPACE, directory.path(), None).unwrap();
        let listener = EndpointListener::bind(&endpoint).await.unwrap();
        let (stream, accepted) = tokio::join!(endpoint.connect(), listener.accept());
        let stop = tokio_util::sync::CancellationToken::new();
        let writer = Arc::new(WriterGate::default());
        let accepted = ControlledStream {
            stream: accepted.unwrap(),
            gate: writer.clone(),
        };
        let task = tokio::spawn(
            serve_stream(
                accepted,
                RpcOwner::local_connection(&server.instance, 1),
                server.rpc,
                server.source,
                server.limits,
                stop.clone(),
            )
            .with_current_subscriber(),
        );
        let mut peer = Self {
            stream: Some(stream.unwrap()),
            stop,
            task: Some(task),
            _directory: Some(directory),
            next: 0,
            writer,
        };
        peer.send(Frame::Hello {
            versions: vec![1],
            required_features: vec!["rpc".into()],
            optional_features: features.iter().map(|s| (*s).into()).collect(),
        })
        .await;
        assert!(matches!(peer.read().await, Frame::Accept { .. }));
        peer
    }

    fn id(&mut self) -> RequestId {
        self.next += 1;
        RequestId::new(self.next).unwrap()
    }
    async fn send(&mut self, frame: Frame) {
        tokio::time::timeout(
            Duration::from_secs(10),
            nyanpasu_rpc_protocol::frame::write_frame(self.stream.as_mut().unwrap(), &frame),
        )
        .await
        .expect("test input fits the peer buffer")
        .unwrap();
    }
    async fn read(&mut self) -> Frame {
        tokio::time::timeout(
            Duration::from_secs(10),
            nyanpasu_rpc_protocol::frame::read_frame(self.stream.as_mut().unwrap()),
        )
        .await
        .expect("local frame arrives")
        .unwrap()
        .expect("connection remains open")
    }
    async fn request(&mut self, method: &str, params: Value) {
        let id = self.id();
        self.send(Frame::Request {
            id,
            method: method.into(),
            params,
        })
        .await;
    }
    async fn subscribe(&mut self, stream: &str, event: Option<EventName>) {
        let id = self.id();
        self.send(Frame::Subscribe {
            id,
            stream: stream.into(),
            event: event.map(|name| name.as_str().into()),
        })
        .await;
    }
    async fn disconnect(&mut self) {
        drop(self.stream.take());
        if self.task.is_some() {
            tokio::time::timeout(Duration::from_secs(10), self.stop.cancelled())
                .await
                .expect("reader observes disconnect");
        }
    }
    async fn finish(mut self) {
        self.disconnect().await;
        if let Some(task) = self.task {
            tokio::time::timeout(Duration::from_secs(10), task)
                .await
                .expect("connection drains")
                .unwrap();
        }
    }
    fn resume_writer(&self) {
        self.writer.blocked.store(false, Ordering::SeqCst);
        self.writer.waker.wake();
    }
}

struct Source {
    value: Mutex<Value>,
    snapshots: AtomicUsize,
    details: watch::Sender<Option<Arc<ClashConnectionDetails>>>,
    entered: Notify,
    release: Notify,
    delay: std::sync::atomic::AtomicBool,
    fail: AtomicBool,
    churn: Mutex<Option<crate::events::EventBus>>,
}

impl Source {
    fn new() -> Arc<Self> {
        let (details, _) = watch::channel(None);
        Arc::new(Self {
            value: Mutex::new(json!({"version": 1})),
            snapshots: AtomicUsize::new(0),
            details,
            entered: Notify::new(),
            release: Notify::new(),
            delay: false.into(),
            fail: false.into(),
            churn: Mutex::new(None),
        })
    }
    async fn gate(&self) {
        if self.delay.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
        }
    }
}

#[async_trait::async_trait]
impl SubscriptionSource for Source {
    async fn snapshot(&self, event: EventName) -> Result<Value, RpcError> {
        self.snapshots.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            return Err(RpcError::new("unavailable", "snapshot source unavailable"));
        }
        let value = self.value.lock().clone();
        self.gate().await;
        if let Some(bus) = self.churn.lock().as_ref() {
            bus.publish(event.as_str(), value.clone());
        }
        Ok(value)
    }
    async fn details(&self) -> Result<Details, RpcError> {
        let receiver = self.details.subscribe();
        self.gate().await;
        Ok(receiver)
    }
}

fn server(fixture: &Fixture, source: Arc<Source>, limits: ConnectionLimits) -> LocalServer {
    let mut server = LocalServer::new(fixture.rpc.clone(), limits);
    server.source = source;
    server
}

fn details(sequence: u64) -> Option<Arc<ClashConnectionDetails>> {
    Some(Arc::new(ClashConnectionDetails {
        sequence,
        connections: ConnectionDetails::new(
            Arc::new(clash_api::ConnectionsSnapshot {
                download_total: 0,
                upload_total: 0,
                connections: None,
                memory: None,
            }),
            Vec::new(),
        ),
    }))
}

async fn demand_count(source: &Source, expected: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while source.details.receiver_count() != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("connection demand is released independently of accepted calls");
}

#[tokio::test]
async fn local_details_requires_negotiated_feature() {
    let fixture = Fixture::new(false).await;
    let source = Source::new();
    let mut peer = Peer::start(
        server(&fixture, source.clone(), ConnectionLimits::default()),
        &[],
    )
    .await;
    peer.subscribe("connection-details", None).await;
    assert!(matches!(
        peer.read().await,
        Frame::SubscriptionError {
            code: SubscriptionErrorCode::FeatureNotNegotiated,
            ..
        }
    ));
    assert_eq!(source.details.receiver_count(), 0);
    assert_eq!(terminal_record_count(&fixture), 1);
    peer.finish().await;
    fixture.shutdown().await;
}

#[tokio::test]
async fn local_subscriptions_reject_unknown_stream_and_resource_limit() {
    let fixture = Fixture::new(false).await;
    let source = Source::new();
    let mut peer = Peer::start(
        server(
            &fixture,
            source.clone(),
            ConnectionLimits {
                resources: 1,
                ..Default::default()
            },
        ),
        &["connection-details"],
    )
    .await;
    peer.subscribe("unknown", None).await;
    assert!(matches!(
        peer.read().await,
        Frame::SubscriptionError {
            code: SubscriptionErrorCode::UnknownStream,
            ..
        }
    ));
    peer.subscribe("connection-details", None).await;
    assert!(matches!(peer.read().await, Frame::Subscribed { .. }));
    peer.subscribe("connection-details", None).await;
    assert!(matches!(
        peer.read().await,
        Frame::SubscriptionError {
            code: SubscriptionErrorCode::ResourceLimit,
            ..
        }
    ));
    assert_eq!(source.details.receiver_count(), 1);
    peer.finish().await;
    assert_eq!(source.details.receiver_count(), 0);
    fixture.shutdown().await;

    let fixture = Fixture::new(false).await;
    let source = Source::new();
    let mut peer = Peer::start(
        server(
            &fixture,
            source.clone(),
            ConnectionLimits {
                resources: 1,
                ..Default::default()
            },
        ),
        &["events"],
    )
    .await;
    source.fail.store(true, Ordering::SeqCst);
    peer.subscribe("events", Some(EventName::CoreStatusChanged))
        .await;
    assert!(matches!(
        peer.read().await,
        Frame::SubscriptionError {
            code: SubscriptionErrorCode::SnapshotUnavailable,
            ..
        }
    ));
    source.fail.store(false, Ordering::SeqCst);
    *source.value.lock() = json!("x".repeat(nyanpasu_rpc_protocol::frame::MAX_FRAME_LEN));
    peer.subscribe("events", Some(EventName::CoreStatusChanged))
        .await;
    assert!(matches!(
        peer.read().await,
        Frame::SubscriptionError {
            code: SubscriptionErrorCode::ResourceLimit,
            ..
        }
    ));
    *source.value.lock() = json!({"version": 1});
    *source.churn.lock() = Some(fixture.rpc.dependencies.events.clone());
    peer.subscribe("events", Some(EventName::CoreStatusChanged))
        .await;
    assert!(matches!(
        peer.read().await,
        Frame::SubscriptionError {
            code: SubscriptionErrorCode::ResourceLimit,
            ..
        }
    ));
    *source.churn.lock() = None;
    peer.subscribe("events", Some(EventName::CoreStatusChanged))
        .await;
    assert!(
        matches!(peer.read().await, Frame::Subscribed { .. }),
        "failed establishment releases capacity"
    );
    assert!(matches!(peer.read().await, Frame::Reset { .. }));
    peer.finish().await;
    fixture.shutdown().await;

    let fixture = Fixture::with_capabilities(false, &[]).await;
    let source = Source::new();
    let mut peer = Peer::start(
        server(&fixture, source.clone(), ConnectionLimits::default()),
        &["events"],
    )
    .await;
    peer.subscribe("events", Some(EventName::AppUpdateStateChanged))
        .await;
    assert!(matches!(
        peer.read().await,
        Frame::SubscriptionError {
            code: SubscriptionErrorCode::Unsupported,
            ..
        }
    ));
    assert_eq!(source.snapshots.load(Ordering::SeqCst), 0);
    assert!(matches!(
        fixture.sink.records.lock()[0].phase,
        crate::audit::AuditPhase::Rejected {
            kind: "unsupported",
            ..
        }
    ));
    peer.finish().await;
    fixture.shutdown().await;
}

#[tokio::test]
async fn local_non_full_value_streams_are_rejected() {
    let fixture = Fixture::new(false).await;
    let source = Source::new();
    let mut peer = Peer::start(
        server(&fixture, source.clone(), ConnectionLimits::default()),
        &["events"],
    )
    .await;
    for event in EventName::ALL {
        if nyanpasu_rpc_protocol::session::RECOVERABLE_EVENTS.contains(&event) {
            continue;
        }
        peer.subscribe("events", Some(event)).await;
        assert!(
            matches!(
                peer.read().await,
                Frame::SubscriptionError {
                    code: SubscriptionErrorCode::UnknownStream,
                    ..
                }
            ),
            "{event:?}"
        );
    }
    assert_eq!(source.snapshots.load(Ordering::SeqCst), 0);
    assert_eq!(terminal_record_count(&fixture), 6);
    peer.finish().await;
    fixture.shutdown().await;
}

#[tokio::test]
async fn local_details_ack_precedes_current_frame() {
    let fixture = Fixture::new(false).await;
    let source = Source::new();
    source.details.send_replace(details(7));
    let mut peer = Peer::start(
        server(&fixture, source, ConnectionLimits::default()),
        &["connection-details"],
    )
    .await;
    peer.subscribe("connection-details", None).await;
    let Frame::Subscribed { subscription, .. } = peer.read().await else {
        panic!("ack first")
    };
    assert!(
        matches!(peer.read().await, Frame::Data { subscription: id, payload, .. } if id == subscription && payload["sequence"] == 7)
    );
    peer.finish().await;
    fixture.shutdown().await;
}

#[tokio::test]
async fn local_details_coalesce_and_release_demand() {
    let fixture = Fixture::new(false).await;
    let source = Source::new();
    let mut peer = Peer::start(
        server(
            &fixture,
            source.clone(),
            ConnectionLimits {
                output: 1,
                ..Default::default()
            },
        ),
        &["connection-details"],
    )
    .await;
    peer.subscribe("connection-details", None).await;
    let Frame::Subscribed { subscription, .. } = peer.read().await else {
        panic!("ack first")
    };
    assert_eq!(source.details.receiver_count(), 1);
    peer.writer.blocked.store(true, Ordering::SeqCst);
    source.details.send_replace(details(1));
    peer.writer.entered.notified().await;
    for sequence in 2..=1000 {
        source.details.send_replace(details(sequence));
    }
    peer.resume_writer();
    assert!(matches!(peer.read().await, Frame::Data { payload, .. } if payload["sequence"] == 1));
    assert!(
        matches!(peer.read().await, Frame::Data { payload, .. } if payload["sequence"] == 1000)
    );
    let id = peer.id();
    peer.send(Frame::Unsubscribe {
        id,
        subscription: subscription.clone(),
    })
    .await;
    assert!(matches!(peer.read().await, Frame::Unsubscribed { .. }));
    assert_eq!(
        source.details.receiver_count(),
        0,
        "demand is released before ack"
    );
    // A subsequent command fences the ordered writer after unsubscribe.
    peer.request("unknown", Value::Null).await;
    assert!(matches!(peer.read().await, Frame::Response { .. }));
    // The forwarder cancellation is drained at disconnect, with no lease involved.
    peer.finish().await;
    assert_eq!(source.details.receiver_count(), 0);
    fixture.shutdown().await;
}

#[tokio::test]
async fn local_reset_preserves_updates_during_snapshot() {
    let fixture = Fixture::new(false).await;
    let source = Source::new();
    source.delay.store(true, Ordering::SeqCst);
    let mut peer = Peer::start(
        server(&fixture, source.clone(), ConnectionLimits::default()),
        &["events"],
    )
    .await;
    peer.subscribe("events", Some(EventName::CoreStatusChanged))
        .await;
    source.entered.notified().await;
    *source.value.lock() = json!({"version": 2});
    fixture
        .rpc
        .dependencies
        .events
        .publish(EventName::CoreStatusChanged.as_str(), json!({"version": 2}));
    source.release.notify_one();
    assert!(matches!(peer.read().await, Frame::Subscribed { .. }));
    assert!(matches!(peer.read().await, Frame::Reset { snapshot, .. } if snapshot["version"] == 2));
    // Even a delayed older publication cannot regress the authoritative reset.
    fixture
        .rpc
        .dependencies
        .events
        .publish(EventName::CoreStatusChanged.as_str(), json!({"version": 1}));
    assert!(matches!(peer.read().await, Frame::Event { payload, .. } if payload["version"] == 2));
    peer.finish().await;
    fixture.shutdown().await;
}

#[tokio::test]
async fn local_dispatch_records_one_info_audit_event() {
    let mut fixture = Fixture::new(true).await;
    fixture.rpc.enable_local_test_command("audit_echo", None);
    let capture = Capture::default();
    async {
        let mut peer = Peer::start(LocalServer::new(fixture.rpc.clone(), ConnectionLimits::default()), &[]).await;
        for fail in [false, true] {
            peer.request("audit_echo", json!({"value": SECRET, "fail": fail, "owner": SECRET})).await;
            assert!(matches!(peer.read().await, Frame::Response { .. }));
        }
        peer.request(SECRET, json!({"value": SECRET})).await;
        assert!(matches!(peer.read().await, Frame::Response { .. }));
        peer.request("get_all_storage_items", Value::Null).await;
        assert!(matches!(peer.read().await, Frame::Response { reply: Reply::Error(error), .. } if error["kind"] == "unsupported"));
        peer.request("audit_echo", json!({"value": SECRET})).await;
        assert!(matches!(peer.read().await, Frame::Response { reply: Reply::Error(error), .. } if error["kind"] == "invalid_params"));
        peer.finish().await;
    }.with_subscriber(tracing_subscriber::registry().with(capture.clone())).await;
    assert_eq!(terminal_record_count(&fixture), 5);
    assert_eq!(fixture.sink.records.lock().len(), 12);
    assert_eq!(capture.0.lock().len(), 12);
    assert!(
        capture
            .0
            .lock()
            .iter()
            .all(|record| record.level == tracing::Level::INFO)
    );
    assert!(!format!("{:?}", capture.0.lock()).contains(SECRET));
    assert!(
        fixture
            .sink
            .records
            .lock()
            .iter()
            .all(|record| record.header.transport == RpcTransport::Local
                && record.header.request_id.is_some()
                && record.header.owner.as_str().starts_with("local:"))
    );
    fixture.shutdown().await;
}

fn delayed_mutation(
    deps: Arc<RpcDependencies>,
    _: RpcOwner,
    _: Value,
) -> Result<RpcFuture, RpcError> {
    Ok(Box::pin(crate::registry::unreported(async move {
        let mut release = deps.events.subscribe();
        deps.events.publish("mutation_entered", Value::Null);
        while release.recv().await.unwrap().0 != "release" {}
        deps.events.publish("mutation_finished", Value::Null);
        Ok(Value::Null)
    })))
}

async fn coalesced_requests(count: usize) {
    use tokio::io::AsyncWriteExt as _;

    let mut fixture = Fixture::new(false).await;
    fixture
        .rpc
        .enable_local_test_command("audit_echo", Some(delayed_mutation));
    let mut sequence = fixture.rpc.dependencies.events.subscribe();
    let mut peer = Peer::start(
        LocalServer::new(fixture.rpc.clone(), ConnectionLimits::default()),
        &[],
    )
    .await;
    let mut bytes = Vec::new();
    let mut expected = HashSet::new();
    for _ in 0..count {
        let id = peer.id();
        expected.insert(id);
        bytes.extend(
            nyanpasu_rpc_protocol::frame::encode(&Frame::Request {
                id,
                method: "audit_echo".into(),
                params: Value::Null,
            })
            .unwrap(),
        );
    }
    peer.stream
        .as_mut()
        .unwrap()
        .write_all(&bytes)
        .await
        .unwrap();
    for _ in 0..count {
        tokio::time::timeout(Duration::from_secs(10), async {
            assert_eq!(sequence.recv().await.unwrap().0, "mutation_entered");
        })
        .await
        .expect("every coalesced request is admitted");
    }
    fixture
        .rpc
        .dependencies
        .events
        .publish("release", Value::Null);
    for _ in 0..count {
        let Frame::Response {
            id,
            reply: Reply::Result(_),
        } = peer.read().await
        else {
            panic!("every coalesced call replies successfully")
        };
        assert!(expected.remove(&id), "reply correlates to one request");
    }
    assert!(expected.is_empty());
    // The connection still admits and answers a new call after the burst.
    peer.request("audit_echo", Value::Null).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        // Skip the burst's own release and completion events.
        while sequence.recv().await.unwrap().0 != "mutation_entered" {}
    })
    .await
    .expect("the follow-up request is admitted");
    fixture
        .rpc
        .dependencies
        .events
        .publish("release", Value::Null);
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
async fn two_coalesced_requests_keep_connection_usable() {
    coalesced_requests(2).await;
}

#[tokio::test]
async fn full_in_flight_coalesced_requests_keep_connection_usable() {
    coalesced_requests(ConnectionLimits::default().in_flight).await;
}

#[tokio::test]
async fn one_over_in_flight_closes_connection_and_drains_accepted_work() {
    use tokio::io::AsyncWriteExt as _;

    let mut fixture = Fixture::new(false).await;
    fixture
        .rpc
        .enable_local_test_command("audit_echo", Some(delayed_mutation));
    let mut sequence = fixture.rpc.dependencies.events.subscribe();
    let limits = ConnectionLimits {
        in_flight: 2,
        ..Default::default()
    };
    let mut peer = Peer::start(LocalServer::new(fixture.rpc.clone(), limits), &[]).await;
    let mut bytes = Vec::new();
    for _ in 0..=limits.in_flight {
        bytes.extend(
            nyanpasu_rpc_protocol::frame::encode(&Frame::Request {
                id: peer.id(),
                method: "audit_echo".into(),
                params: Value::Null,
            })
            .unwrap(),
        );
    }
    peer.stream
        .as_mut()
        .unwrap()
        .write_all(&bytes)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), peer.stop.cancelled())
        .await
        .expect("one excess request closes the connection without waiting for calls");
    for _ in 0..limits.in_flight {
        tokio::time::timeout(Duration::from_secs(10), async {
            assert_eq!(sequence.recv().await.unwrap().0, "mutation_entered");
        })
        .await
        .expect("accepted work starts even after transport closure");
    }
    assert!(!peer.task.as_ref().unwrap().is_finished());
    fixture
        .rpc
        .dependencies
        .events
        .publish("release", Value::Null);
    peer.finish().await;
    assert_eq!(sequence.recv().await.unwrap().0, "release");
    for _ in 0..limits.in_flight {
        assert_eq!(sequence.recv().await.unwrap().0, "mutation_finished");
    }
    assert_eq!(terminal_record_count(&fixture), limits.in_flight);
    fixture.shutdown().await;
}

#[tokio::test]
async fn saturated_calls_release_details_on_disconnect_before_calls_finish() {
    use tokio::io::AsyncWriteExt as _;

    for malformed in [false, true] {
        let mut fixture = Fixture::new(false).await;
        fixture
            .rpc
            .enable_local_test_command("audit_echo", Some(delayed_mutation));
        let source = Source::new();
        let mut sequence = fixture.rpc.dependencies.events.subscribe();
        let mut peer = Peer::start(
            server(
                &fixture,
                source.clone(),
                ConnectionLimits {
                    in_flight: 2,
                    ..Default::default()
                },
            ),
            &["connection-details"],
        )
        .await;
        peer.subscribe("connection-details", None).await;
        assert!(matches!(peer.read().await, Frame::Subscribed { .. }));
        for _ in 0..2 {
            peer.request("audit_echo", Value::Null).await;
            assert_eq!(sequence.recv().await.unwrap().0, "mutation_entered");
        }
        // Neither extra requests nor terminal decoding errors may hide EOF while
        // all accepted calls remain behind barriers.
        let mut excess = Vec::new();
        if malformed {
            excess.extend(1u32.to_be_bytes());
            excess.push(b'{');
        } else {
            for _ in 0..2 {
                excess.extend(
                    nyanpasu_rpc_protocol::frame::encode(&Frame::Request {
                        id: peer.id(),
                        method: "audit_echo".into(),
                        params: Value::Null,
                    })
                    .unwrap(),
                );
            }
        }
        let _ = peer.stream.as_mut().unwrap().write_all(&excess).await;
        peer.disconnect().await;
        demand_count(&source, 0).await;
        assert!(
            !peer.task.as_ref().unwrap().is_finished(),
            "accepted work still belongs to the server"
        );
        fixture
            .rpc
            .dependencies
            .events
            .publish("release", Value::Null);
        peer.finish().await;
        assert_eq!(
            terminal_record_count(&fixture),
            3,
            "only the subscription and two accepted calls run"
        );
        fixture.shutdown().await;
    }
}

#[tokio::test]
async fn disconnect_reclaims_late_subscriptions_without_cancelling_mutations() {
    let mut fixture = Fixture::new(false).await;
    fixture
        .rpc
        .enable_local_test_command("audit_echo", Some(delayed_mutation));
    let source = Source::new();
    source.delay.store(true, Ordering::SeqCst);
    let mut sequence = fixture.rpc.dependencies.events.subscribe();
    let mut peer = Peer::start(
        server(&fixture, source.clone(), ConnectionLimits::default()),
        &["connection-details"],
    )
    .await;
    peer.subscribe("connection-details", None).await;
    source.entered.notified().await;
    peer.request("audit_echo", Value::Null).await;
    assert_eq!(sequence.recv().await.unwrap().0, "mutation_entered");
    peer.disconnect().await;
    assert_eq!(
        source.details.receiver_count(),
        1,
        "pending establishment remains owned until completion"
    );
    source.release.notify_one();
    fixture
        .rpc
        .dependencies
        .events
        .publish("release", Value::Null);
    tokio::time::timeout(Duration::from_secs(10), peer.task.unwrap())
        .await
        .unwrap()
        .unwrap();
    assert!(
        std::iter::from_fn(|| sequence.try_recv().ok())
            .any(|(name, _)| name == "mutation_finished")
    );
    assert_eq!(source.details.receiver_count(), 0);
    fixture.shutdown().await;
}

#[tokio::test]
async fn local_handler_panic_reaches_fatal_supervision() {
    fn panic_handler(
        _: Arc<RpcDependencies>,
        _: RpcOwner,
        _: Value,
    ) -> Result<RpcFuture, RpcError> {
        Ok(Box::pin(crate::registry::unreported(async {
            std::panic::panic_any(SECRET)
        })))
    }
    let mut fixture = Fixture::new(false).await;
    fixture
        .rpc
        .enable_local_test_command("audit_echo", Some(panic_handler));
    let directory = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::discover(ENDPOINT_NAMESPACE, directory.path(), None).unwrap();
    let listener = EndpointListener::bind(&endpoint).await.unwrap();
    let task = tokio::spawn(
        LocalServer::new(fixture.rpc.clone(), ConnectionLimits::default())
            .serve(listener, tokio_util::sync::CancellationToken::new()),
    );
    let mut stream = endpoint.connect().await.unwrap();
    nyanpasu_rpc_protocol::frame::write_frame(
        &mut stream,
        &Frame::Hello {
            versions: vec![1],
            required_features: vec!["rpc".into()],
            optional_features: vec![],
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        nyanpasu_rpc_protocol::frame::read_frame(&mut stream)
            .await
            .unwrap(),
        Some(Frame::Accept { .. })
    ));
    nyanpasu_rpc_protocol::frame::write_frame(
        &mut stream,
        &Frame::Request {
            id: RequestId::new(1).unwrap(),
            method: "audit_echo".into(),
            params: Value::Null,
        },
    )
    .await
    .unwrap();
    let error = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(*error.into_panic().downcast::<&str>().unwrap(), SECRET);
    fixture.shutdown().await;
}

#[tokio::test]
async fn slow_local_peer_has_bounded_buffers() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let mut fixture = Fixture::new(false).await;
    fixture.rpc.enable_local_test_command("audit_echo", None);
    let limits = ConnectionLimits {
        in_flight: 2,
        output: 1,
        resources: 1,
    };
    let source = Source::new();
    let listener = ListeningServer::start(server(&fixture, source.clone(), limits)).await;
    let mut slow = listener.connect(&["connection-details"]).await;
    let mut fast = listener.connect(&["connection-details"]).await;
    for peer in [&mut slow, &mut fast] {
        peer.subscribe("connection-details", None).await;
        assert!(matches!(peer.read().await, Frame::Subscribed { .. }));
    }
    // Exceed the OS send buffer without reading replies: exercise the real
    // listener's writer and bounded output handoff, not a separate server.
    for expected in 3..=5 {
        slow.request(
            "audit_echo",
            json!({"value":"x".repeat(1024 * 1024), "fail":false}),
        )
        .await;
        tokio::time::timeout(Duration::from_secs(10), async {
            while terminal_record_count(&fixture) < expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("accepted call reaches the bounded writer");
    }
    let mut excess = Vec::new();
    for _ in 0..8 {
        excess.extend(
            nyanpasu_rpc_protocol::frame::encode(&Frame::Request {
                id: slow.id(),
                method: "audit_echo".into(),
                params: json!({"value":"queued", "fail":false}),
            })
            .unwrap(),
        );
    }
    let _ = slow.stream.as_mut().unwrap().write_all(&excess).await;
    demand_count(&source, 1).await;
    // Windows keeps dirty pipe handles alive while flushing. Drain the
    // cancelled writer's partial output before dropping the unread client.
    let mut discarded = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(10),
        slow.stream.as_mut().unwrap().read_to_end(&mut discarded),
    )
    .await
    .expect("overloaded connection closes after its buffered output is drained")
    // Unix resets a socket closed with unread input instead of sending EOF.
    .or_else(|error| match error.kind() {
        std::io::ErrorKind::ConnectionReset => Ok(0),
        _ => Err(error),
    })
    .unwrap();
    slow.disconnect().await;
    fast.request("audit_echo", json!({"value":"independent", "fail":false}))
        .await;
    assert!(
        matches!(fast.read().await, Frame::Response { reply: Reply::Result(value), .. } if value == "independent")
    );
    let mut reconnect = listener.connect(&["connection-details"]).await;
    reconnect.subscribe("connection-details", None).await;
    assert!(matches!(reconnect.read().await, Frame::Subscribed { .. }));
    reconnect.request("unknown", Value::Null).await;
    assert!(matches!(reconnect.read().await, Frame::Response { .. }));
    {
        let records = fixture.sink.records.lock();
        let slow_owner = records[0].header.owner.as_str();
        let slow_calls = records
            .iter()
            .filter(|record| {
                record.header.owner.as_str() == slow_owner
                    && matches!(
                        record.phase,
                        crate::audit::AuditPhase::Finished { .. }
                            | crate::audit::AuditPhase::Rejected { .. }
                    )
            })
            .count();
        assert!(
            slow_calls <= 4 + limits.in_flight,
            "after one subscription and three completed calls, only the outstanding budget can drain"
        );
        let owners: HashSet<_> = records
            .iter()
            .map(|record| record.header.owner.as_str())
            .collect();
        assert_eq!(
            owners.len(),
            3,
            "one listener assigns new owners to simultaneous and reconnecting peers"
        );
    }
    fast.finish().await;
    reconnect.finish().await;
    listener.finish().await;
    assert_eq!(source.details.receiver_count(), 0);
    fixture.shutdown().await;
}

#[tokio::test]
async fn local_overflow_reset_reconstructs_each_recoverable_stream() {
    let fixture = Fixture::new(false).await;
    for event in nyanpasu_rpc_protocol::session::RECOVERABLE_EVENTS {
        let source = Source::new();
        // Start with the actual facade's payload shape, including core-log wrapper.
        *source.value.lock() = fixture
            .rpc
            .dependencies
            .client
            .snapshot(event)
            .await
            .unwrap();
        let mut peer = Peer::start(
            server(
                &fixture,
                source.clone(),
                ConnectionLimits {
                    output: 1,
                    ..Default::default()
                },
            ),
            &["events"],
        )
        .await;
        peer.subscribe("events", Some(event)).await;
        assert!(matches!(peer.read().await, Frame::Subscribed { .. }));
        assert!(matches!(peer.read().await, Frame::Reset { .. }));
        peer.writer.blocked.store(true, Ordering::SeqCst);
        fixture
            .rpc
            .dependencies
            .events
            .publish(event.as_str(), source.value.lock().clone());
        peer.writer.entered.notified().await;
        let mut fresh = source.value.lock().clone();
        let marker = match event {
            EventName::ConfigurationStatusChanged => &mut fresh["event_seq"],
            EventName::CoreStatusChanged => &mut fresh["generation"],
            EventName::ServiceStatusChanged => &mut fresh["restart_attempts"],
            EventName::AppUpdateStateChanged => &mut fresh["revision"],
            EventName::CoreLogsChanged => &mut fresh["status"]["version"],
            _ => unreachable!(),
        };
        *marker = json!(marker.as_u64().unwrap() + 1);
        *source.value.lock() = fresh;
        for _ in 0..1024 {
            fixture
                .rpc
                .dependencies
                .events
                .publish(event.as_str(), source.value.lock().clone());
        }
        let expected = source.value.lock().clone();
        peer.resume_writer();
        loop {
            if matches!(peer.read().await, Frame::Lag { .. }) {
                break;
            }
        }
        let Frame::Reset { snapshot, .. } = peer.read().await else {
            panic!("{event:?}: fresh reset immediately follows lag")
        };
        assert_eq!(snapshot, expected);
        fixture
            .rpc
            .dependencies
            .events
            .publish(event.as_str(), expected.clone());
        assert!(matches!(peer.read().await, Frame::Event { payload, .. } if payload == expected));
        assert!(
            fixture
                .sink
                .records
                .lock()
                .iter()
                .all(|record| record.header.method == "local_subscribe"),
            "no recovery query or per-frame audit"
        );
        peer.finish().await;
    }
    fixture.shutdown().await;
}

fn terminal_record_count(fixture: &Fixture) -> usize {
    fixture
        .sink
        .records
        .lock()
        .iter()
        .filter(|record| {
            matches!(
                record.phase,
                crate::audit::AuditPhase::Finished { .. }
                    | crate::audit::AuditPhase::Rejected { .. }
            )
        })
        .count()
}
