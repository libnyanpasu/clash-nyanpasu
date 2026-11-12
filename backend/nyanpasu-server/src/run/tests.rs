//! Real listeners over injected application adapters.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use nyanpasu_core::{
    actor_v2::endpoint::{
        CheckSubmission, CheckSupport, ControlEndpoint, CoreStatusSnapshot, CoreSubmission,
        EndpointHandle,
    },
    client::{
        effects::{
            plan::ApplicationEffectPlan,
            ports::{ApplicationEffectsPort, NoopApplicationEffects},
            status::{EffectRevision, EffectStatus},
        },
        test_support,
    },
    storage::{Storage, WebStorage as _},
};
use nyanpasu_core_manager::{CoreError, OperationId};
use nyanpasu_ipc::api::{
    core::v2::{CoreApiConnection, OperationInfo},
    status::CoreControllerInfo,
};
use nyanpasu_rpc::registry::{CommandEntry, RpcDependencies, RpcFuture};
use nyanpasu_rpc_protocol::{
    CommandKind, CommandMetadata, RpcError, RpcOwner, RpcTarget, client::LocalClient, frame::Reply,
};
use nyanpasu_runtime_values::execution_host::ExecutionHost;
use serde_json::{Value, json};
use tokio::sync::{Notify, oneshot};
use tokio_util::sync::CancellationToken;

use super::{Ready, RunOptions, serve};
use crate::compose::Server;

const WATCHDOG: Duration = Duration::from_secs(45);

async fn bounded<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(WATCHDOG, future)
        .await
        .expect("test barrier watchdog expired")
}

#[derive(Default)]
struct Effects {
    block: AtomicBool,
    entered: Notify,
    release: Notify,
    completed: Notify,
}

#[async_trait::async_trait]
impl ApplicationEffectsPort for Effects {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        if self.block.swap(false, Ordering::SeqCst) {
            self.entered.notify_one();
            self.release.notified().await;
            self.completed.notify_one();
        }
        NoopApplicationEffects.apply(revision, plan).await
    }
}

struct Fixture {
    server: Server,
    directory: tempfile::TempDir,
    effects: Arc<Effects>,
}

async fn fixture(block_startup: bool) -> Fixture {
    fixture_with_endpoint(
        block_startup,
        test_support::TestControlEndpoint::succeeding(),
    )
    .await
}

async fn fixture_with_endpoint(block_startup: bool, endpoint: EndpointHandle) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let mut args = test_support::test_client_args_with_endpoint(
        &directory,
        endpoint,
        test_support::profile_test_adapters(&directory),
    )
    .await;
    let effects = Arc::new(Effects::default());
    effects.block.store(block_startup, Ordering::SeqCst);
    args.effects = effects.clone();
    args.paths.create_base_dirs().unwrap();
    let storage = Storage::try_new(&directory.path().join("rpc.redb")).unwrap();
    let server = Server::from_test_args(args, storage).await;
    Fixture {
        server,
        directory,
        effects,
    }
}

fn options(directory: &tempfile::TempDir) -> RunOptions {
    RunOptions {
        data_directory: directory.path().to_owned(),
        http: true,
        ..Default::default()
    }
}

async fn http(ready: &Ready) -> (reqwest::Client, String, String) {
    let url = ready.http_url.as_ref().unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let response = client.get(url).send().await.unwrap();
    assert_eq!(response.status(), 303);
    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|header| header.to_str().unwrap().split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ");
    let base = url.split("/?").next().unwrap().to_owned();
    (client, base, cookies)
}

async fn http_call(http: &(reqwest::Client, String, String), method: &str, params: Value) -> Value {
    http.0
        .post(format!("{}/bridge/rpc", http.1))
        .header("cookie", &http.2)
        .json(&json!({"method": method, "params": params}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn server_builds_one_application_for_all_transports() {
    let Fixture {
        server, directory, ..
    } = fixture(false).await;
    let client = server.client().clone();
    let stop = CancellationToken::new();
    let (ready, announced) = oneshot::channel();
    let task = tokio::spawn(serve(server, options(&directory), stop.clone(), ready));
    let ready = bounded(announced).await.unwrap();
    let initial = client.startup_reconcile().await.operation_id;
    let web = http(&ready).await;
    for (index, value) in ["first", "second"].into_iter().enumerate() {
        let local = LocalClient::connect(&ready.endpoint, WATCHDOG)
            .await
            .unwrap();
        assert!(matches!(
            local
                .call("set_storage_item", json!({"key":"css", "value":value}))
                .await
                .unwrap(),
            Reply::Result(_)
        ));
        let result = http_call(&web, "get_storage_item", json!({"key":"css"})).await;
        assert_eq!(result["value"], value, "HTTP storage response: {result}");
        assert_eq!(
            result["revision"],
            index + 1,
            "one live storage revision sequence"
        );
        local.close().await;
        assert_eq!(client.startup_reconcile().await.operation_id, initial);
    }
    stop.cancel();
    bounded(task).await.unwrap().unwrap();
}

struct StreamEndpoint {
    delegate: Arc<test_support::TestControlEndpoint>,
    url: String,
}

#[async_trait::async_trait]
impl ControlEndpoint for StreamEndpoint {
    fn host(&self) -> ExecutionHost {
        self.delegate.host()
    }
    async fn check_config(&self, submission: CheckSubmission) -> CheckSupport {
        self.delegate.check_config(submission).await
    }
    async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
        self.delegate.submit(submission).await
    }
    async fn wait_operation(&self, id: OperationId, timeout: Duration) -> Option<OperationInfo> {
        self.delegate.wait_operation(id, timeout).await
    }
    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        self.delegate.status().await
    }
    async fn api_connection(&self) -> Result<Option<CoreApiConnection>, CoreError> {
        Ok(Some(CoreApiConnection {
            instance_id: "e4b-stream-probe".into(),
            controller: CoreControllerInfo::Http(self.url.clone()),
            secret: None,
        }))
    }
}

#[tokio::test]
async fn server_readiness_waits_for_listeners_and_forwarders() {
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _};
    let streams = Arc::new(Notify::new());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = Arc::new(StreamEndpoint {
        delegate: test_support::TestControlEndpoint::succeeding(),
        url: format!("http://{}/", listener.local_addr().unwrap()),
    });
    let probe_stop = CancellationToken::new();
    let probe = tokio::spawn({
        let stop = probe_stop.clone();
        let streams = streams.clone();
        async move {
            loop {
                let stream = tokio::select! {
                    () = stop.cancelled() => break,
                    stream = listener.accept() => stream.unwrap().0,
                };
                let mut stream = tokio::io::BufReader::new(stream);
                let mut request = String::new();
                stream.read_line(&mut request).await.unwrap();
                if request.starts_with("GET /traffic ") {
                    streams.notify_one();
                }
                let _ = stream.get_mut().write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
            }
        }
    });
    let Fixture {
        server,
        directory,
        effects,
    } = fixture_with_endpoint(true, endpoint).await;
    let mut events = server.events().subscribe();
    let storage = server.storage().clone();
    let (stream_entered, activating) = oneshot::channel();
    let (activate, stream_release) = oneshot::channel();
    let mut options = options(&directory);
    options.startup_gate = Some((stream_entered, stream_release));
    let stop = CancellationToken::new();
    let (ready, mut announced) = oneshot::channel();
    let task = tokio::spawn(serve(server, options, stop.clone(), ready));
    bounded(effects.entered.notified()).await;
    storage.set_item("web:css", &"during-startup").unwrap();
    bounded(async {
        loop {
            let (name, value) = events.recv().await.unwrap();
            if name == "storage-value-changed-event" {
                assert_eq!(value["key"], "web:css");
                assert_eq!(value["value"], "\"during-startup\"");
                break;
            }
        }
    })
    .await;
    assert!(matches!(
        announced.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    effects.release.notify_one();
    bounded(async {
        tokio::select! {
            biased;
            ready = &mut announced => panic!("readiness preceded stream activation: {ready:?}"),
            entered = activating => entered.unwrap(),
        }
    })
    .await;
    assert!(matches!(
        announced.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    activate.send(()).unwrap();
    let ready = bounded(announced).await.unwrap();
    // A real outbound traffic-stream request proves stream activation independently
    // of readiness bookkeeping or log delivery.
    bounded(streams.notified()).await;
    let local = LocalClient::connect(&ready.endpoint, WATCHDOG)
        .await
        .unwrap();
    let web = http(&ready).await;
    local
        .call(
            "set_storage_item",
            json!({"key":"css", "value":"forwarded"}),
        )
        .await
        .unwrap();
    bounded(async {
        loop {
            let (name, value) = events.recv().await.unwrap();
            if name == "storage-value-changed-event" {
                assert_eq!(value["key"], "web:css");
                break;
            }
        }
    })
    .await;
    let status = http_call(&web, "get_configuration_status", json!({})).await;
    assert!(
        status.get("error").is_none(),
        "HTTP status response: {status}"
    );
    local.close().await;
    stop.cancel();
    bounded(task).await.unwrap().unwrap();
    probe_stop.cancel();
    bounded(probe).await.unwrap();
}

#[tokio::test]
async fn server_shutdown_drains_admitted_owner_work() {
    let Fixture {
        server,
        directory,
        effects,
    } = fixture(false).await;
    let client = server.client().clone();
    let owner_stop = client.shutdown_child_token();
    let stop = CancellationToken::new();
    let (ready, announced) = oneshot::channel();
    let mut task = tokio::spawn(serve(server, options(&directory), stop.clone(), ready));
    let ready = bounded(announced).await.unwrap();
    effects.block.store(true, Ordering::SeqCst);
    let local = LocalClient::connect(&ready.endpoint, WATCHDOG)
        .await
        .unwrap();
    let work = tokio::spawn(async move {
        let result = local.call("e4b_test_owner_work", json!({})).await;
        local.close().await;
        result
    });
    bounded(effects.entered.notified()).await;
    // The mutation is committed and replies while its effect owner still works.
    assert!(matches!(
        bounded(work).await.unwrap().unwrap(),
        Reply::Result(_)
    ));
    stop.cancel();
    bounded(owner_stop.cancelled()).await;
    assert!(
        futures::poll!(&mut task).is_pending(),
        "owner work must keep shutdown pending"
    );
    effects.release.notify_one();
    bounded(effects.completed.notified()).await;
    bounded(task).await.unwrap().unwrap();
    let endpoint = nyanpasu_single_instance::endpoint::Endpoint::discover(
        nyanpasu_rpc::local::ENDPOINT_NAMESPACE,
        directory.path(),
        Some(&ready.endpoint),
    )
    .unwrap();
    drop(
        nyanpasu_single_instance::endpoint::EndpointListener::bind(&endpoint)
            .await
            .unwrap(),
    );
}

#[tokio::test]
async fn server_startup_failure_releases_bound_endpoints() {
    let Fixture {
        server, directory, ..
    } = fixture(false).await;
    let endpoint = nyanpasu_single_instance::endpoint::Endpoint::discover(
        nyanpasu_rpc::local::ENDPOINT_NAMESPACE,
        directory.path(),
        None,
    )
    .unwrap();
    let mut options = options(&directory);
    let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = http.local_addr().unwrap();
    options.http_listener = Some(http);
    options.frontend = Some(nyanpasu_rpc::http::Frontend::Dev(
        url::Url::parse("https://example.invalid").unwrap(),
    ));
    let (ready, announced) = oneshot::channel();
    assert!(matches!(
        bounded(serve(server, options, CancellationToken::new(), ready)).await,
        Err(super::Error::Http { .. })
    ));
    assert!(announced.await.is_err());
    drop(
        tokio::net::TcpListener::bind(address)
            .await
            .expect("failed startup releases the acquired HTTP address"),
    );
    drop(
        nyanpasu_single_instance::endpoint::EndpointListener::bind(&endpoint)
            .await
            .unwrap(),
    );
}

fn owner_work(deps: Arc<RpcDependencies>, _: RpcOwner, _: Value) -> Result<RpcFuture, RpcError> {
    use struct_patch::Patch as _;
    Ok(Box::pin(nyanpasu_rpc::registry::unreported(async move {
        let mut patch = nyanpasu_config::application::NyanpasuAppConfig::new_empty_patch();
        patch.max_log_file_size = Some(9);
        deps.client
            .patch_app_config(patch)
            .await
            .map_err(nyanpasu_rpc::error::application_error)?;
        Ok(Value::Null)
    })))
}

inventory::submit! {
    CommandEntry {
        metadata: CommandMetadata { name: "e4b_test_owner_work", kind: CommandKind::Shared,
            targets: &[RpcTarget::Local], capabilities: &[], optional_capabilities: &[] },
        handler: owner_work,
    }
}
