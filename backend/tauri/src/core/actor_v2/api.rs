//! Instance-bound Clash API capability. The protocol client never escapes this
//! adapter: clones share revocation and every operation checks the applied binding.

use std::{future::Future, time::Duration};

use clash_api::{Delay, DelayQuery, ProviderName, ProxyName, Version};
use nyanpasu_ipc::api::{core::v2::CoreApiConnection, status::CoreControllerInfo};
use tokio_util::sync::CancellationToken;

use super::endpoint::EndpointHandle;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("the Clash API capability belongs to a retired instance")]
    Stale,
    #[error("the running core's API is unavailable: {0}")]
    Unavailable(String),
    #[error("Clash API operation timed out; a submitted mutation may have taken effect")]
    Timeout,
    #[error(transparent)]
    Protocol(#[from] clash_api::Error),
}

/// Shared, permanently revocable capability; it cannot be rebound to a new core.
#[derive(Clone)]
pub struct ApiClient {
    client: clash_api::Client,
    binding: CoreApiConnection,
    endpoint: EndpointHandle,
    revoked: CancellationToken,
}

impl std::fmt::Debug for ApiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiClient")
            .field("instance_id", &self.binding.instance_id)
            .field("revoked", &self.revoked.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl ApiClient {
    pub(super) fn new(
        binding: CoreApiConnection,
        endpoint: EndpointHandle,
    ) -> Result<Self, ApiError> {
        let host = match &binding.controller {
            CoreControllerInfo::Http(url) => clash_api::Host::url(url)?,
            CoreControllerInfo::UnixSocket(path) => clash_api::Host::unix_socket(path),
            CoreControllerInfo::NamedPipe(path) => clash_api::Host::named_pipe(path),
        };
        let mut builder = clash_api::Client::builder(host);
        if let Some(secret) = &binding.secret {
            builder = builder.secret(secret.clone());
        }
        Ok(Self {
            client: builder.build()?,
            binding,
            endpoint,
            revoked: CancellationToken::new(),
        })
    }

    pub(super) fn matches(&self, binding: &CoreApiConnection) -> bool {
        !self.revoked.is_cancelled() && self.binding == *binding
    }

    pub(super) fn revoke(&self) {
        self.revoked.cancel();
    }

    async fn check(&self) -> Result<(), ApiError> {
        if self.revoked.is_cancelled() {
            return Err(ApiError::Stale);
        }
        match self.endpoint.api_connection().await {
            Ok(Some(binding)) if self.matches(&binding) => Ok(()),
            Ok(_) => {
                self.revoke();
                Err(ApiError::Stale)
            }
            Err(error) => {
                self.revoke();
                Err(ApiError::Unavailable(error.to_string()))
            }
        }
    }

    // One bound includes preflight, the complete body decode, and postflight.
    // No automatic retry: losing a mutation reply does not authorize replay.
    async fn execute<T>(
        &self,
        operation: impl Future<Output = clash_api::Result<T>>,
    ) -> Result<T, ApiError> {
        tokio::select! {
            biased;
            _ = self.revoked.cancelled() => Err(ApiError::Stale),
            result = tokio::time::timeout(Duration::from_secs(30), async {
                self.check().await?;
                let result = operation.await;
                self.check().await?;
                result.map_err(ApiError::Protocol)
            }) => result.map_err(|_| ApiError::Timeout)?,
        }
    }

    pub async fn version(&self) -> Result<Version, ApiError> {
        self.execute(self.client.version()).await
    }

    pub async fn proxy_delay(
        &self,
        name: &ProxyName,
        provider: Option<&ProviderName>,
        query: &DelayQuery,
    ) -> Result<Delay, ApiError> {
        match provider {
            Some(provider) => {
                self.execute(self.client.provider_proxy_delay(provider, name, query))
                    .await
            }
            None => self.execute(self.client.proxy_delay(name, query)).await,
        }
    }

    pub async fn group_delay(
        &self,
        group: &ProxyName,
        query: &DelayQuery,
    ) -> Result<indexmap::IndexMap<ProxyName, u16>, ApiError> {
        self.execute(self.client.group_delay(group, query)).await
    }

    pub async fn close_connection(&self, id: uuid::Uuid) -> Result<(), ApiError> {
        self.execute(self.client.close_connection(id)).await
    }

    pub async fn close_all_connections(&self) -> Result<(), ApiError> {
        self.execute(self.client.close_all_connections()).await
    }
}

/// Owned only by CoreActor. Dropping it revokes every outstanding clone even
/// when actor teardown skips post_stop (for example, actor state unwinding).
pub(super) struct ApiLease {
    pub client: ApiClient,
    monitor: tokio::task::JoinHandle<()>,
}

impl ApiLease {
    pub fn new(client: ApiClient) -> Self {
        let monitored = client.clone();
        let monitor = tokio::spawn(async move {
            use futures::StreamExt;
            let subscription =
                tokio::time::timeout(Duration::from_secs(10), monitored.endpoint.api_changes())
                    .await;
            let mut changes = match subscription {
                Ok(Ok(Some(changes))) => changes,
                Ok(Ok(None)) => Box::pin(futures::stream::pending()) as super::endpoint::ApiChanges,
                _ => {
                    monitored.revoke();
                    return;
                }
            };
            let mut timer = tokio::time::interval(Duration::from_secs(2));
            loop {
                tokio::select! {
                    biased;
                    _ = monitored.revoked.cancelled() => return,
                    event = changes.next() => {
                        if !matches!(event, Some(Ok(()))) { monitored.revoke(); return; }
                    }
                    _ = timer.tick() => {}
                }
                if !matches!(
                    tokio::time::timeout(Duration::from_secs(10), monitored.check()).await,
                    Ok(Ok(()))
                ) {
                    monitored.revoke();
                    return;
                }
            }
        });
        Self { client, monitor }
    }
}

impl Drop for ApiLease {
    fn drop(&mut self) {
        self.client.revoke();
        self.monitor.abort();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{Json, Router, body::Body, response::Response, routing::get};
    use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
    use nyanpasu_ipc::api::{
        core::v2::{OperationInfo, OperationOutputInfo, OperationPhase},
        status::CoreStateDetail,
    };
    use tokio::sync::{Notify, watch};

    use super::*;
    use crate::core::actor_v2::{
        CoreClient,
        endpoint::{
            ApiChanges, ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
        },
    };

    struct Endpoint {
        host: ExecutionHost,
        binding: watch::Sender<Option<CoreApiConnection>>,
    }

    #[async_trait::async_trait]
    impl ControlEndpoint for Endpoint {
        fn host(&self) -> ExecutionHost {
            self.host
        }

        async fn api_connection(&self) -> Result<Option<CoreApiConnection>, CoreError> {
            Ok(self.binding.borrow().clone())
        }

        async fn api_changes(&self) -> Result<Option<ApiChanges>, CoreError> {
            Ok(Some(Box::pin(futures::stream::unfold(
                self.binding.subscribe(),
                |mut rx| async move {
                    rx.changed().await.ok()?;
                    Some((Ok(()), rx))
                },
            ))))
        }

        async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
            Ok(CoreStatusSnapshot {
                state: Some(if self.binding.borrow().is_some() {
                    CoreStateDetail::Running { epoch: 1, pid: 7 }
                } else {
                    CoreStateDetail::Stopped { reason: None }
                }),
                state_changed_at: 0,
                revision: None,
                healthy: Some(true),
                applied_kind: None,
            })
        }

        async fn submit(&self, submission: CoreSubmission) -> Result<OperationInfo, CoreError> {
            if !matches!(
                submission.envelope.command,
                nyanpasu_core_manager::CoreCommand::Stop
            ) {
                return Err(CoreError::new(
                    CoreErrorKind::Internal,
                    "test accepts only stop",
                    false,
                ));
            }
            self.binding.send_replace(None);
            Ok(OperationInfo {
                id: submission.envelope.operation_id.to_string(),
                phase: OperationPhase::Succeeded,
                output: Some(OperationOutputInfo::Stopped),
                error: None,
            })
        }

        async fn wait_operation(&self, _: OperationId, _: Duration) -> Option<OperationInfo> {
            None
        }
    }

    fn endpoint(url: String) -> Arc<Endpoint> {
        let (binding, _) = watch::channel(Some(CoreApiConnection {
            instance_id: "first-process".into(),
            controller: CoreControllerInfo::Http(url),
            secret: None,
        }));
        Arc::new(Endpoint {
            binding,
            host: ExecutionHost::Local,
        })
    }

    async fn server(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (url, task)
    }

    async fn revoked(api: &ApiClient) {
        tokio::time::timeout(Duration::from_secs(2), api.revoked.cancelled())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn same_epoch_and_pid_replacement_revokes_every_clone_without_reviving() {
        let endpoint = endpoint("http://127.0.0.1:1/".into());
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let first = core.api_client().await.unwrap();
        let clone = first.clone();
        endpoint
            .binding
            .send_modify(|binding| binding.as_mut().unwrap().instance_id = "second-process".into());
        revoked(&first).await;
        assert!(matches!(clone.version().await, Err(ApiError::Stale)));
        let second = core.api_client().await.unwrap();
        assert_eq!(second.binding.instance_id, "second-process");
        assert!(!second.revoked.is_cancelled());
        endpoint
            .binding
            .send_modify(|binding| binding.as_mut().unwrap().instance_id = "first-process".into());
        assert!(matches!(first.version().await, Err(ApiError::Stale)));
        core.actor.stop(None);
    }

    #[tokio::test]
    async fn unchanged_binding_keeps_capability_and_shutdown_revokes_it() {
        let (url, server) = server(Router::new().route(
            "/version",
            get(|| async { Json(serde_json::json!({"version":"test"})) }),
        ))
        .await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        // Hot patch notification: revision may change, process and credentials do not.
        endpoint.binding.send_modify(|_| {});
        assert_eq!(api.version().await.unwrap().version, "test");
        let reacquired = core.api_client().await.unwrap();
        assert!(api.matches(&reacquired.binding));
        core.shutdown().await.unwrap();
        assert!(matches!(api.version().await, Err(ApiError::Stale)));
        assert!(core.api_client().await.is_err());
        core.actor.stop(None);
        server.abort();
    }

    #[tokio::test]
    async fn changed_secret_or_endpoint_revokes_cached_capability() {
        for change_secret in [true, false] {
            let endpoint = endpoint("http://127.0.0.1:1/".into());
            let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
            let api = core.api_client().await.unwrap();
            endpoint.binding.send_modify(|binding| {
                let binding = binding.as_mut().unwrap();
                if change_secret {
                    binding.secret = Some("replacement-secret".into());
                } else {
                    binding.controller = CoreControllerInfo::Http("http://127.0.0.1:2/".into());
                }
            });
            // Acquisition uses an authoritative binding, independent of the status pump.
            let replacement = core.api_client().await.unwrap();
            revoked(&api).await;
            assert!(!replacement.revoked.is_cancelled());
            assert!(!format!("{replacement:?}").contains("replacement-secret"));
            core.actor.stop(None);
        }
    }

    #[tokio::test]
    async fn invalidation_cancels_a_partially_received_response_body() {
        let started = Arc::new(Notify::new());
        let signal = started.clone();
        let (url, server) = server(Router::new().route(
            "/version",
            get(move || {
                let signal = signal.clone();
                async move {
                    use futures::StreamExt;
                    let first = futures::stream::once(async move {
                        signal.notify_one();
                        Ok::<_, std::io::Error>("{\"version\":")
                    });
                    Response::new(Body::from_stream(first.chain(futures::stream::pending())))
                }
            }),
        ))
        .await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        let call = tokio::spawn(async move { api.version().await });
        tokio::time::timeout(Duration::from_secs(2), started.notified())
            .await
            .unwrap();
        endpoint.binding.send_replace(None);
        let result = tokio::time::timeout(Duration::from_secs(2), call)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, Err(ApiError::Stale)));
        core.actor.stop(None);
        server.abort();
    }

    #[tokio::test]
    async fn actor_termination_revokes_outstanding_client() {
        let endpoint = endpoint("http://127.0.0.1:1/".into());
        let core = CoreClient::spawn(endpoint).await.unwrap();
        let api = core.api_client().await.unwrap();
        core.actor.stop(None);
        revoked(&api).await;
        assert!(matches!(api.version().await, Err(ApiError::Stale)));
    }
    #[tokio::test]
    async fn handoff_revokes_even_when_target_has_the_same_binding() {
        let source = endpoint("http://127.0.0.1:1/".into());
        let (binding, _) = watch::channel(source.binding.borrow().clone());
        let target = Arc::new(Endpoint {
            binding,
            host: ExecutionHost::Service,
        });
        let core = CoreClient::spawn(source).await.unwrap();
        let old = core.api_client().await.unwrap();
        core.change_host(target).await.unwrap();
        assert!(matches!(old.version().await, Err(ApiError::Stale)));
        let new = core.api_client().await.unwrap();
        assert!(!new.revoked.is_cancelled());
        core.actor.stop(None);
    }
}
