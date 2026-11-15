//! Local-socket actor, route binding, and `LocalSocketControl` implementation.

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::Result;
use nyanpasu_config::application::LocalSocketAccessConfig;
use nyanpasu_core::client::local_socket::{
    Error, LocalSocketControl, LocalSocketFault, LocalSocketStatus,
};
use nyanpasu_rpc_protocol::{ServerInfo, ShellCapability};
use nyanpasu_single_instance::endpoint::{Endpoint, EndpointListener};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::sync::CancellationToken;

use super::{
    ENDPOINT_NAMESPACE,
    server::{ConnectionLimits, LocalServer},
};
use crate::registry::{CommandHandlers, RpcDependencies, SetupError, UnifiedRpc};

const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const STOP_JOIN_TIMEOUT: Duration = Duration::from_secs(3);

/// Infrastructure is assembled by the composition root, never by an enable command.
pub trait LocalRoutes: Send + Sync + 'static {
    fn build(&self) -> Result<UnifiedRpc>;
}
impl<F: Fn() -> Result<UnifiedRpc> + Send + Sync + 'static> LocalRoutes for F {
    fn build(&self) -> Result<UnifiedRpc> {
        self()
    }
}

/// Filled once by the composition root after the application facade exists.
/// A weak dependency graph avoids an idle local-socket actor retaining its own client.
#[derive(Default)]
pub struct RpcLocalRoutes(
    std::sync::OnceLock<(
        std::sync::Weak<RpcDependencies>,
        ServerInfo,
        Vec<ShellCapability>,
        CommandHandlers,
    )>,
);

impl RpcLocalRoutes {
    pub fn install(&self, rpc: &UnifiedRpc) -> Result<(), SetupError> {
        self.0
            .set((
                Arc::downgrade(&rpc.dependencies),
                rpc.server.clone(),
                rpc.provided.clone(),
                rpc.commands.clone(),
            ))
            .map_err(|_| SetupError::InstallRoutes)
    }
}

impl LocalRoutes for RpcLocalRoutes {
    fn build(&self) -> Result<UnifiedRpc> {
        let (dependencies, server, provided, commands) = self
            .0
            .get()
            .ok_or_else(|| anyhow::anyhow!("local RPC routes not installed"))?;
        Ok(UnifiedRpc {
            dependencies: dependencies
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("application has shut down"))?,
            server: server.clone(),
            provided: provided.clone(),
            commands: commands.clone(),
        })
    }
}

pub struct LocalSocketArgs {
    pub data_directory: PathBuf,
    pub endpoint_override: Option<PathBuf>,
    pub routes: Arc<dyn LocalRoutes>,
}

pub enum Message {
    Configure {
        config: LocalSocketAccessConfig,
        reply: RpcReplyPort<LocalSocketStatus>,
    },
    Status(RpcReplyPort<LocalSocketStatus>),
    Stopped(u64),
}

struct Running {
    generation: u64,
    endpoint: String,
    cancellation: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

pub struct LocalSocketActor;
pub struct LocalSocketState {
    data_directory: PathBuf,
    endpoint_override: Option<PathBuf>,
    routes: Arc<dyn LocalRoutes>,
    running: Option<Running>,
    fault: Option<LocalSocketFault>,
    generation: u64,
}

impl LocalSocketState {
    fn status(&self) -> LocalSocketStatus {
        LocalSocketStatus {
            enabled: self.running.is_some(),
            endpoint: self.running.as_ref().map(|r| r.endpoint.clone()),
            fault: self.fault,
        }
    }

    async fn start(&mut self, myself: ActorRef<Message>) {
        let directory = self.data_directory.clone();
        let override_path = self.endpoint_override.clone();
        let endpoint = match nyanpasu_owner_support::blocking::join(
            tokio::task::spawn_blocking(move || {
                Endpoint::discover(ENDPOINT_NAMESPACE, &directory, override_path.as_deref())
            })
            .await,
        ) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                tracing::warn!(%error, "the local RPC endpoint could not be discovered");
                self.fault = Some(LocalSocketFault::BindFailed);
                return;
            }
        };
        let listener = match EndpointListener::bind(&endpoint).await {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "the local RPC listener could not bind");
                self.fault = Some(LocalSocketFault::BindFailed);
                return;
            }
        };
        let rpc = match self.routes.build() {
            Ok(rpc) => rpc,
            Err(error) => {
                tracing::warn!(%error, "the local RPC routes could not be prepared");
                self.fault = Some(LocalSocketFault::BindFailed);
                return;
            }
        };
        self.generation += 1;
        let generation = self.generation;
        let endpoint_path = endpoint.path().to_string_lossy().into_owned();
        let cancellation = CancellationToken::new();
        let shutdown = cancellation.clone();
        let task = tokio::spawn(async move {
            if let Err(error) = LocalServer::new(rpc, ConnectionLimits::default())
                .serve_with_drain(listener, shutdown, DRAIN_TIMEOUT)
                .await
            {
                tracing::error!(%error, "local RPC listener stopped");
            }
            let _ = myself.cast(Message::Stopped(generation));
        });
        self.fault = None;
        self.running = Some(Running {
            generation,
            endpoint: endpoint_path,
            cancellation,
            task,
        });
    }

    async fn stop(&mut self) {
        if let Some(mut running) = self.running.take() {
            running.cancellation.cancel();
            match tokio::time::timeout(STOP_JOIN_TIMEOUT, &mut running.task).await {
                Ok(result) => nyanpasu_owner_support::blocking::join(result),
                Err(_) => {
                    running.task.abort();
                    if let Err(error) = running.task.await
                        && error.is_panic()
                    {
                        std::panic::resume_unwind(error.into_panic());
                    }
                }
            }
        }
    }
}

impl Actor for LocalSocketActor {
    type Msg = Message;
    type State = LocalSocketState;
    type Arguments = LocalSocketArgs;

    async fn pre_start(
        &self,
        _: ActorRef<Message>,
        args: LocalSocketArgs,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(LocalSocketState {
            data_directory: args.data_directory,
            endpoint_override: args.endpoint_override,
            routes: args.routes,
            running: None,
            fault: None,
            generation: 0,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Message>,
        message: Message,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Status(reply) => {
                let _ = reply.send(state.status());
            }
            Message::Stopped(generation) => {
                if state
                    .running
                    .as_ref()
                    .is_some_and(|r| r.generation == generation)
                {
                    state.stop().await;
                }
            }
            Message::Configure { config, reply } => {
                if !config.enabled {
                    state.stop().await;
                    state.fault = None;
                } else if state.running.is_some() {
                    state.fault = None;
                } else {
                    state.stop().await;
                    state.start(myself).await;
                }
                let _ = reply.send(state.status());
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.stop().await;
        Ok(())
    }
}

#[derive(Clone)]
pub struct LocalSocketClient(Arc<LocalSocketClientInner>);
struct LocalSocketClientInner {
    actor: ActorRef<Message>,
    task: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl LocalSocketClient {
    pub async fn spawn(
        data_directory: PathBuf,
        endpoint_override: Option<PathBuf>,
        routes: Arc<dyn LocalRoutes>,
    ) -> std::result::Result<Self, Error> {
        let (actor, task) = Actor::spawn(
            None,
            LocalSocketActor,
            LocalSocketArgs {
                data_directory,
                endpoint_override,
                routes,
            },
        )
        .await
        .map_err(|source| Error::SpawnActor { source })?;
        let task = tokio::spawn(async move { nyanpasu_owner_support::blocking::join(task.await) });
        Ok(Self(Arc::new(LocalSocketClientInner {
            actor,
            task: tokio::sync::Mutex::new(Some(task)),
        })))
    }

    async fn ask<T>(&self, make: impl FnOnce(RpcReplyPort<T>) -> Message) -> Result<T>
    where
        T: Send + 'static,
    {
        match self.0.actor.call(make, None).await? {
            CallResult::Success(result) => Ok(result),
            _ => anyhow::bail!(
                "local socket response unavailable; inspect its status before retrying"
            ),
        }
    }

    pub async fn configure(&self, config: &LocalSocketAccessConfig) -> Result<LocalSocketStatus> {
        let config = config.clone();
        self.ask(|reply| Message::Configure { config, reply }).await
    }

    pub async fn status(&self) -> Result<LocalSocketStatus> {
        self.ask(Message::Status).await
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.configure(&LocalSocketAccessConfig::default()).await?;
        self.0.actor.get_cell().drain_and_wait(None).await?;
        if let Some(task) = self.0.task.lock().await.take() {
            nyanpasu_owner_support::blocking::join(task.await);
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl LocalSocketControl for LocalSocketClient {
    async fn status(&self) -> Result<LocalSocketStatus> {
        Self::status(self).await
    }

    async fn configure(&self, config: &LocalSocketAccessConfig) -> Result<LocalSocketStatus> {
        Self::configure(self, config).await
    }

    async fn shutdown(&self) -> Result<()> {
        Self::shutdown(self).await
    }
}

impl Drop for LocalSocketClientInner {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    use super::*;

    pub fn control_factory(
        data_directory: PathBuf,
        endpoint_override: Option<PathBuf>,
        routes: Arc<dyn LocalRoutes>,
    ) -> Arc<nyanpasu_core::client::host_adapters::LocalSocketFactory> {
        Arc::new(move || {
            let (data_directory, endpoint_override, routes) = (
                data_directory.clone(),
                endpoint_override.clone(),
                routes.clone(),
            );
            Box::pin(async move {
                let server =
                    LocalSocketClient::spawn(data_directory, endpoint_override, routes).await?;
                Ok(Arc::new(server) as Arc<dyn LocalSocketControl>)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{path::Path, time::Duration};

    use nyanpasu_config::application::LocalSocketAccessConfig;
    use nyanpasu_core::client::local_socket::LocalSocketFault;
    use nyanpasu_rpc_protocol::{RpcError, client::LocalClient, frame::Reply};
    use nyanpasu_single_instance::endpoint::{Endpoint, EndpointListener};
    use serde_json::{Value, json};

    use super::{LocalSocketClient, RpcLocalRoutes};
    use crate::{
        local::ENDPOINT_NAMESPACE,
        registry::{RpcCallMeta, RpcCaller, RpcDependencies, RpcFuture, unreported},
        test_support::Fixture,
    };

    const WATCHDOG: Duration = Duration::from_secs(5);

    fn gated_echo(
        deps: Arc<RpcDependencies>,
        _: RpcCallMeta,
        _: RpcCaller,
        _: Value,
    ) -> Result<RpcFuture, RpcError> {
        let mut events = deps.events.subscribe();
        deps.events.publish("in_flight_entered", Value::Null);
        Ok(Box::pin(unreported(async move {
            while let Ok((name, _)) = events.recv().await {
                if name == "release" {
                    break;
                }
            }
            Ok(json!("drained"))
        })))
    }

    use std::sync::Arc;

    #[tokio::test]
    async fn enable_connect_disable_and_reenable() {
        let fixture = Fixture::new(false).await;
        let routes = Arc::new(RpcLocalRoutes::default());
        routes.install(&fixture.rpc).unwrap();
        let data_dir = tempfile::tempdir().unwrap();

        let client = LocalSocketClient::spawn(data_dir.path().to_owned(), None, routes)
            .await
            .unwrap();
        let initial = client.status().await.unwrap();
        assert!(!initial.enabled);
        assert!(initial.endpoint.is_none());
        assert!(initial.fault.is_none());

        let enabled = client
            .configure(&LocalSocketAccessConfig { enabled: true })
            .await
            .unwrap();
        assert!(enabled.enabled);
        assert!(enabled.fault.is_none());
        let endpoint_path = enabled.endpoint.clone().unwrap();

        let conn = LocalClient::connect(Path::new(&endpoint_path), WATCHDOG)
            .await
            .unwrap();
        assert!(matches!(
            conn.call("get_configuration_status", json!(null))
                .await
                .unwrap(),
            Reply::Result(_)
        ));
        conn.close().await;

        let disabled = client
            .configure(&LocalSocketAccessConfig { enabled: false })
            .await
            .unwrap();
        assert!(!disabled.enabled);
        assert!(disabled.endpoint.is_none());
        assert!(disabled.fault.is_none());
        assert!(
            LocalClient::connect(Path::new(&endpoint_path), Duration::from_millis(250))
                .await
                .is_err()
        );

        let reenabled = client
            .configure(&LocalSocketAccessConfig { enabled: true })
            .await
            .unwrap();
        assert!(reenabled.enabled);
        assert!(reenabled.fault.is_none());
        let conn2 = LocalClient::connect(Path::new(&reenabled.endpoint.unwrap()), WATCHDOG)
            .await
            .unwrap();
        assert!(matches!(
            conn2
                .call("get_configuration_status", json!(null))
                .await
                .unwrap(),
            Reply::Result(_)
        ));
        conn2.close().await;

        client.shutdown().await.unwrap();
        fixture.shutdown().await;
    }

    #[tokio::test]
    async fn disable_while_call_in_flight_drains_reply_and_rejects_new_connects() {
        let mut fixture = Fixture::new(false).await;
        fixture
            .rpc
            .enable_local_test_command("audit_echo", Some(gated_echo));
        let routes = Arc::new(RpcLocalRoutes::default());
        routes.install(&fixture.rpc).unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let mut events = fixture.rpc.dependencies.events.subscribe();

        let socket = LocalSocketClient::spawn(data_dir.path().to_owned(), None, routes)
            .await
            .unwrap();
        let status = socket
            .configure(&LocalSocketAccessConfig { enabled: true })
            .await
            .unwrap();
        let endpoint_path = status.endpoint.unwrap();

        let rpc_client = LocalClient::connect(Path::new(&endpoint_path), WATCHDOG)
            .await
            .unwrap();
        let call_task = tokio::spawn(async move {
            let reply = rpc_client.call("audit_echo", json!(null)).await;
            rpc_client.close().await;
            reply
        });

        tokio::time::timeout(WATCHDOG, async {
            loop {
                if events.recv().await.unwrap().0 == "in_flight_entered" {
                    break;
                }
            }
        })
        .await
        .unwrap();

        let disable_socket = socket.clone();
        let disable_task = tokio::spawn(async move {
            disable_socket
                .configure(&LocalSocketAccessConfig { enabled: false })
                .await
        });

        // Listener is dropped right when shutdown begins, even before the in-flight call finishes.
        let mut refused = false;
        for _ in 0..40 {
            if LocalClient::connect(Path::new(&endpoint_path), Duration::from_millis(50))
                .await
                .is_err()
            {
                refused = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(refused, "new connections must be rejected during drain");

        fixture
            .rpc
            .dependencies
            .events
            .publish("release", Value::Null);

        let reply = tokio::time::timeout(WATCHDOG, call_task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(matches!(reply, Reply::Result(v) if v == json!("drained")));

        let disabled = tokio::time::timeout(WATCHDOG, disable_task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!disabled.enabled);
        assert!(disabled.fault.is_none());

        socket.shutdown().await.unwrap();
        fixture.shutdown().await;
    }

    #[tokio::test]
    async fn bind_conflict_reports_fault_without_error() {
        let fixture = Fixture::new(false).await;
        let routes = Arc::new(RpcLocalRoutes::default());
        routes.install(&fixture.rpc).unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let endpoint = Endpoint::discover(ENDPOINT_NAMESPACE, data_dir.path(), None).unwrap();
        let held = EndpointListener::bind(&endpoint).await.unwrap();

        let client = LocalSocketClient::spawn(data_dir.path().to_owned(), None, routes)
            .await
            .unwrap();
        let status = client
            .configure(&LocalSocketAccessConfig { enabled: true })
            .await
            .unwrap();
        assert!(!status.enabled);
        assert!(status.endpoint.is_none());
        assert_eq!(status.fault, Some(LocalSocketFault::BindFailed));

        drop(held);
        let recovered = client
            .configure(&LocalSocketAccessConfig { enabled: true })
            .await
            .unwrap();
        assert!(recovered.enabled);
        assert_eq!(recovered.fault, None);

        client.shutdown().await.unwrap();
        fixture.shutdown().await;
    }

    #[tokio::test]
    async fn shutdown_drains_running_listener() {
        let fixture = Fixture::new(false).await;
        let routes = Arc::new(RpcLocalRoutes::default());
        routes.install(&fixture.rpc).unwrap();
        let data_dir = tempfile::tempdir().unwrap();

        let client = LocalSocketClient::spawn(data_dir.path().to_owned(), None, routes)
            .await
            .unwrap();
        let enabled = client
            .configure(&LocalSocketAccessConfig { enabled: true })
            .await
            .unwrap();
        let endpoint_path = enabled.endpoint.unwrap();

        client.shutdown().await.unwrap();

        let endpoint = Endpoint::discover(ENDPOINT_NAMESPACE, data_dir.path(), None).unwrap();
        drop(EndpointListener::bind(&endpoint).await.unwrap());
        assert!(
            LocalClient::connect(Path::new(&endpoint_path), Duration::from_millis(150))
                .await
                .is_err()
        );
        fixture.shutdown().await;
    }
}
