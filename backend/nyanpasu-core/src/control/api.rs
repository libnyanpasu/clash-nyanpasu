//! Instance-bound Clash API capability. The protocol client never escapes this
//! adapter: clones share revocation and every operation checks the applied binding.

use std::{
    future::Future,
    sync::{Arc, OnceLock},
    time::Duration,
};

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

const DEFAULT_DEADLINE: Duration = Duration::from_secs(30);

/// Core-side overhead allowed on top of a latency test's own timeout.
const DELAY_DEADLINE_MARGIN: Duration = Duration::from_secs(5);

/// A latency test legitimately runs for its whole measurement timeout, so its
/// deadline follows the query instead of the fixed default.
fn delay_deadline(query: &DelayQuery) -> Duration {
    query.timeout + DELAY_DEADLINE_MARGIN
}

/// One read of the proxy view. `groups` is the core's own group list, or `None`
/// when the instance offers none and groups must be inferred from `proxies`.
pub struct ProxySnapshot {
    pub proxies: clash_api::IndexMap<ProxyName, clash_api::Proxy>,
    pub providers: clash_api::IndexMap<ProviderName, clash_api::ProxyProvider>,
    pub groups: Option<clash_api::IndexMap<ProxyName, clash_api::Proxy>>,
}

/// Shared, permanently revocable capability; it cannot be rebound to a new core.
#[derive(Clone)]
pub struct ApiClient {
    client: clash_api::Client,
    binding: CoreApiConnection,
    endpoint: EndpointHandle,
    revoked: CancellationToken,
    /// Whether this instance serves `/group`, settled by its first conclusive
    /// answer. The client never outlives its instance, so the result is bound
    /// to that core and its version.
    group_list: Arc<OnceLock<bool>>,
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
            group_list: Arc::default(),
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
        self.execute_within(DEFAULT_DEADLINE, operation).await
    }

    async fn execute_within<T>(
        &self,
        deadline: Duration,
        operation: impl Future<Output = clash_api::Result<T>>,
    ) -> Result<T, ApiError> {
        tokio::select! {
            biased;
            _ = self.revoked.cancelled() => Err(ApiError::Stale),
            result = tokio::time::timeout(deadline, async {
                self.check().await?;
                let result = operation.await;
                self.check().await?;
                result.map_err(ApiError::Protocol)
            }) => result.map_err(|_| ApiError::Timeout)?,
        }
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked.is_cancelled()
    }

    pub fn instance_id(&self) -> &str {
        &self.binding.instance_id
    }

    pub fn same_instance(&self, other: &Self) -> bool {
        !self.is_revoked() && !other.is_revoked() && self.binding == other.binding
    }

    pub async fn cancelled(&self) {
        self.revoked.cancelled().await;
    }

    pub async fn connections_ws(
        &self,
    ) -> Result<ApiStream<clash_api::ConnectionsSnapshot>, ApiError> {
        let stream = self
            .execute(self.client.connections_ws(Default::default()))
            .await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn logs_ws(
        &self,
        level: clash_api::LogLevel,
    ) -> Result<ApiStream<clash_api::LogEntry>, ApiError> {
        let stream = self
            .execute(self.client.logs_ws(clash_api::LogQuery::new(level)))
            .await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn traffic_ws(&self) -> Result<ApiStream<clash_api::Traffic>, ApiError> {
        let stream = self.execute(self.client.traffic_ws()).await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn memory_ws(&self) -> Result<ApiStream<clash_api::Memory>, ApiError> {
        let stream = self.execute(self.client.memory_ws()).await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn proxy_snapshot(&self) -> Result<ProxySnapshot, ApiError> {
        self.execute(async {
            let (proxies, providers, groups) = tokio::try_join!(
                self.client.proxies(),
                self.client.proxy_providers(),
                async { Ok(self.group_list().await) },
            )?;
            Ok(ProxySnapshot {
                proxies,
                providers,
                groups,
            })
        })
        .await
    }

    // Any failure falls back to inference for this read. Only a missing route
    // settles the probe; other errors may pass, so the next read asks again.
    async fn group_list(&self) -> Option<clash_api::IndexMap<ProxyName, clash_api::Proxy>> {
        if self.group_list.get() == Some(&false) {
            return None;
        }
        match self.client.groups().await {
            Ok(groups) => {
                let _ = self.group_list.set(true);
                Some(groups)
            }
            Err(error) => {
                if error
                    .status()
                    .is_some_and(|status| matches!(status.as_u16(), 404 | 405))
                {
                    let _ = self.group_list.set(false);
                }
                tracing::debug!(%error, "proxy group list unavailable; inferring groups");
                None
            }
        }
    }

    pub async fn select_proxy(
        &self,
        group: &ProxyName,
        target: &ProxyName,
    ) -> Result<(), ApiError> {
        self.execute(
            self.client
                .select_proxy(clash_api::ProxySelection { group, target }),
        )
        .await
    }

    pub async fn clear_proxy_selection(&self, group: &ProxyName) -> Result<(), ApiError> {
        self.execute(self.client.clear_proxy_selection(group)).await
    }

    pub async fn update_proxy_provider(&self, name: &ProviderName) -> Result<(), ApiError> {
        self.execute(self.client.update_proxy_provider(name)).await
    }

    pub async fn healthcheck_proxy_provider(&self, name: &ProviderName) -> Result<(), ApiError> {
        self.execute(self.client.healthcheck_proxy_provider(name))
            .await
    }

    pub async fn configs(&self) -> Result<clash_api::RuntimeConfig, ApiError> {
        self.execute(self.client.configs()).await
    }

    pub async fn rule_providers(
        &self,
    ) -> Result<indexmap::IndexMap<clash_api::RuleProviderName, clash_api::RuleProvider>, ApiError>
    {
        self.execute(self.client.rule_providers()).await
    }

    pub async fn rules(&self) -> Result<Vec<clash_api::Rule>, ApiError> {
        self.execute(self.client.rules()).await
    }

    pub async fn update_rule_provider(
        &self,
        name: &clash_api::RuleProviderName,
    ) -> Result<(), ApiError> {
        self.execute(self.client.update_rule_provider(name)).await
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
        let deadline = delay_deadline(query);
        match provider {
            Some(provider) => {
                self.execute_within(
                    deadline,
                    self.client.provider_proxy_delay(provider, name, query),
                )
                .await
            }
            None => {
                self.execute_within(deadline, self.client.proxy_delay(name, query))
                    .await
            }
        }
    }

    pub async fn group_delay(
        &self,
        group: &ProxyName,
        query: &DelayQuery,
    ) -> Result<indexmap::IndexMap<ProxyName, u16>, ApiError> {
        self.execute_within(delay_deadline(query), self.client.group_delay(group, query))
            .await
    }

    pub async fn connections(&self) -> Result<clash_api::ConnectionsSnapshot, ApiError> {
        self.execute(self.client.connections()).await
    }

    pub async fn close_connection(&self, id: uuid::Uuid) -> Result<(), ApiError> {
        self.execute(self.client.close_connection(id)).await
    }

    pub async fn close_all_connections(&self) -> Result<(), ApiError> {
        self.execute(self.client.close_all_connections()).await
    }
}

/// An owned subscription bound to the same capability for its entire lifetime.
/// Idle reads have no deadline; cancellation drops the socket, and each decoded
/// frame must pass the authoritative binding check before delivery.
pub struct ApiStream<T> {
    api: ApiClient,
    stream: Option<clash_api::WebSocketStream<T>>,
}

impl<T> ApiStream<T> {
    fn new(api: ApiClient, stream: clash_api::WebSocketStream<T>) -> Self {
        Self {
            api,
            stream: Some(stream),
        }
    }

    pub async fn next(&mut self) -> Option<Result<T, ApiError>> {
        use futures_util::StreamExt;
        let stream = self.stream.as_mut()?;
        let result = tokio::select! {
            biased;
            _ = self.api.cancelled() => Some(Err(ApiError::Stale)),
            frame = stream.next() => match frame {
                Some(frame) => Some(self.api.execute(async { frame }).await),
                None => None,
            },
        };
        if result.is_none()
            || matches!(&result, Some(Err(error)) if !matches!(error, ApiError::Protocol(clash_api::Error::Decode { .. })))
        {
            self.stream.take();
        }
        result
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
                Ok(Err(error)) => {
                    tracing::warn!(host = ?monitored.endpoint.host(), "API lifecycle subscription failed: {error}");
                    monitored.revoke();
                    return;
                }
                Err(error) => {
                    tracing::warn!(host = ?monitored.endpoint.host(), "API lifecycle subscription timed out: {error}");
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
                        match event {
                            Some(Ok(())) => {},
                            Some(Err(error)) => {
                                tracing::warn!(host = ?monitored.endpoint.host(), "API lifecycle stream failed: {error}");
                                monitored.revoke();
                                return;
                            }
                            None => {
                                tracing::warn!(host = ?monitored.endpoint.host(), "API lifecycle stream ended; revoking API access");
                                monitored.revoke();
                                return;
                            }
                        }
                    }
                    _ = timer.tick() => {}
                }
                match tokio::time::timeout(Duration::from_secs(10), monitored.check()).await {
                    Ok(Ok(())) => {}
                    Ok(Err(ApiError::Stale)) => return,
                    Ok(Err(error)) => {
                        tracing::warn!(host = ?monitored.endpoint.host(), "API binding monitor failed: {:#}", anyhow::Error::new(error));
                        monitored.revoke();
                        return;
                    }
                    Err(error) => {
                        tracing::warn!(host = ?monitored.endpoint.host(), "API binding monitor timed out: {error}");
                        monitored.revoke();
                        return;
                    }
                }
            }
        });
        Self { client, monitor }
    }

    #[cfg(test)]
    pub(super) async fn stop_for_test(mut self) {
        self.client.revoke();
        self.monitor.abort();
        match (&mut self.monitor).await {
            Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
            _ => {}
        }
    }
}

impl Drop for ApiLease {
    fn drop(&mut self) {
        self.client.revoke();
        self.monitor.abort();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Arc;

    use axum::{Json, Router, body::Body, response::Response, routing::get};
    use nyanpasu_core_manager::{CoreError, CoreErrorKind, OperationId};
    use nyanpasu_ipc::api::{
        core::v2::{OperationInfo, OperationOutputInfo, OperationPhase},
        status::CoreStateDetail,
    };
    use tokio::sync::{Notify, watch};

    use super::*;
    use crate::control::{
        CoreClient,
        endpoint::{
            ApiChanges, ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
        },
    };

    pub(crate) struct Endpoint {
        pub(super) host: ExecutionHost,
        pub(crate) binding: watch::Sender<Option<CoreApiConnection>>,
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
                controller: None,
                state: Some(if self.binding.borrow().is_some() {
                    CoreStateDetail::Running { epoch: 1, pid: 7 }
                } else {
                    CoreStateDetail::Stopped { reason: None }
                }),
                state_changed_at: 0,
                revision: None,
                source_hash: None,
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

    pub(crate) fn endpoint(url: String) -> Arc<Endpoint> {
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

    pub(crate) async fn server(router: Router) -> (String, tokio::task::JoinHandle<()>) {
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
    async fn config_reads_accept_partial_responses_and_reject_retired_instances() {
        let (url, server) = server(Router::new().route(
            "/configs",
            get(|| async { Json(serde_json::json!({"mixed-port":7890,"mode":"rule"})) }),
        ))
        .await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        let config = api.configs().await.unwrap();
        assert_eq!(config.mixed_port, Some(7890));
        assert_eq!(config.allow_lan, None);
        endpoint.binding.send_replace(None);
        assert!(matches!(api.configs().await, Err(ApiError::Stale)));
        core.actor.stop(None);
        server.abort();
    }

    #[tokio::test]
    async fn provider_reads_preserve_order_and_reject_retired_instances() {
        let (url, server) = server(Router::new().route(
            "/providers/rules/",
            get(|| async {
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"providers":{"z":{"name":"z"},"a":{"name":"a"}}}"#,
                    ))
                    .unwrap()
            }),
        ))
        .await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        let providers = api.rule_providers().await.unwrap();
        assert_eq!(
            providers
                .keys()
                .map(|name| name.as_str())
                .collect::<Vec<_>>(),
            ["z", "a"]
        );
        endpoint.binding.send_replace(None);
        assert!(matches!(api.rule_providers().await, Err(ApiError::Stale)));
        core.actor.stop(None);
        server.abort();
    }

    #[tokio::test]
    async fn rules_and_provider_refresh_use_the_bound_client() {
        use axum::{extract::Path, http::StatusCode, routing::put};
        let (url, server) = server(
            Router::new()
                .route(
                    "/rules/",
                    get(|| async {
                        Json(serde_json::json!({"rules":[
                            {"type":"Match","payload":"","proxy":"DIRECT"}
                        ]}))
                    }),
                )
                .route(
                    "/providers/rules/{name}/",
                    put(|Path(name): Path<String>| async move {
                        assert_eq!(name, "rules/日本 ?#");
                        StatusCode::NO_CONTENT
                    }),
                ),
        )
        .await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        let rules = api.rules().await.unwrap();
        assert_eq!(rules[0].proxy, "DIRECT");
        assert_eq!(rules[0].index, None);
        let provider = clash_api::RuleProviderName::new("rules/日本 ?#");
        api.update_rule_provider(&provider).await.unwrap();
        endpoint.binding.send_replace(None);
        assert!(matches!(api.rules().await, Err(ApiError::Stale)));
        assert!(matches!(
            api.update_rule_provider(&provider).await,
            Err(ApiError::Stale)
        ));
        core.actor.stop(None);
        server.abort();
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

    fn delay_query(timeout: Duration) -> DelayQuery {
        DelayQuery::new("http://example.com/".parse().unwrap(), timeout).unwrap()
    }

    #[test]
    fn a_delay_deadline_outlasts_the_query_timeout() {
        assert_eq!(
            delay_deadline(&delay_query(Duration::from_secs(5))),
            Duration::from_secs(10)
        );
        assert!(delay_deadline(&delay_query(Duration::from_secs(30))) > DEFAULT_DEADLINE);
    }

    #[tokio::test(start_paused = true)]
    async fn a_delay_call_waits_for_its_own_timeout_plus_the_margin() {
        let (url, server) = server(Router::new().route(
            "/proxies/node/delay",
            get(|| async { std::future::pending::<Json<()>>().await }),
        ))
        .await;
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let api = core.api_client().await.unwrap();
        let query = delay_query(Duration::from_secs(30));
        let name = ProxyName::from("node");
        let call = tokio::spawn(async move { api.proxy_delay(&name, None, &query).await });

        // The fixed 30 s default would already have fired here.
        tokio::time::sleep(Duration::from_secs(31)).await;
        assert!(!call.is_finished());
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(matches!(call.await.unwrap(), Err(ApiError::Timeout)));
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

#[cfg(test)]
mod stream_tests {
    use super::{
        tests::{endpoint, server},
        *,
    };
    use crate::control::CoreClient;
    use axum::{
        Router,
        extract::{State, WebSocketUpgrade, ws::Message},
        response::IntoResponse,
        routing::get,
    };
    use tokio::sync::mpsc;

    async fn idle(
        State(closed): State<mpsc::UnboundedSender<()>>,
        ws: WebSocketUpgrade,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |mut socket| async move {
            socket
                .send(Message::Text(r#"{"up":1,"down":2}"#.into()))
                .await
                .unwrap();
            while socket.recv().await.is_some() {}
            let _ = closed.send(());
        })
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn service_named_pipe_supports_rest_and_websocket() {
        use axum::Json;
        use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

        struct PipeListener {
            path: String,
            next: NamedPipeServer,
        }

        impl axum::serve::Listener for PipeListener {
            type Io = NamedPipeServer;
            type Addr = ();

            async fn accept(&mut self) -> (Self::Io, Self::Addr) {
                self.next.connect().await.unwrap();
                let next = ServerOptions::new().create(&self.path).unwrap();
                (std::mem::replace(&mut self.next, next), ())
            }

            fn local_addr(&self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let path = format!(r"\\.\pipe\nyanpasu-app-test-{}", uuid::Uuid::new_v4());
        let listener = PipeListener {
            next: ServerOptions::new()
                .first_pipe_instance(true)
                .create(&path)
                .unwrap(),
            path: path.clone(),
        };
        let (closed, mut closed_rx) = mpsc::unbounded_channel();
        let router = Router::new()
            .route("/traffic", get(idle))
            .route(
                "/configs",
                get(|| async { Json(serde_json::json!({"mode":"rule"})) }),
            )
            .with_state(closed);
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut endpoint = endpoint("http://127.0.0.1:1/".into());
        std::sync::Arc::get_mut(&mut endpoint).unwrap().host =
            crate::control::endpoint::ExecutionHost::Service;
        endpoint.binding.send_modify(|binding| {
            binding.as_mut().unwrap().controller = CoreControllerInfo::NamedPipe(path.into());
        });
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            api.configs().await.unwrap();
            let mut stream = api.traffic_ws().await.unwrap();
            assert_eq!(stream.next().await.unwrap().unwrap().up.get(), 1);
            endpoint.binding.send_replace(None);
            assert!(matches!(stream.next().await, Some(Err(ApiError::Stale))));
            closed_rx.recv().await.unwrap();
        })
        .await
        .unwrap();
        core.shutdown().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn websocket_keeps_hot_patch_binding_and_releases_idle_socket_on_revocation() {
        for change in ["instance", "secret", "controller", "shutdown"] {
            let (closed, mut rx) = mpsc::unbounded_channel();
            let (url, server) = server(
                Router::new()
                    .route("/traffic", get(idle))
                    .with_state(closed),
            )
            .await;
            let endpoint = endpoint(url.clone());
            let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
            let api = core.api_client().await.unwrap();
            let mut stream = api.traffic_ws().await.unwrap();
            endpoint.binding.send_modify(|_| {});
            assert!(core.api_client().await.unwrap().same_instance(&api));
            assert_eq!(stream.next().await.unwrap().unwrap().up.get(), 1);
            if change == "shutdown" {
                core.shutdown().await.unwrap();
            } else {
                endpoint.binding.send_modify(|binding| {
                    let binding = binding.as_mut().unwrap();
                    match change {
                        "instance" => binding.instance_id = "replacement".into(),
                        "secret" => binding.secret = Some("changed".into()),
                        "controller" => {
                            binding.controller =
                                CoreControllerInfo::Http(format!("{url}replacement/"))
                        }
                        _ => unreachable!(),
                    }
                });
                core.api_client().await.unwrap();
            }
            assert!(matches!(
                tokio::time::timeout(Duration::from_secs(2), stream.next())
                    .await
                    .unwrap(),
                Some(Err(ApiError::Stale))
            ));
            assert!(stream.next().await.is_none());
            tokio::time::timeout(Duration::from_secs(2), rx.recv())
                .await
                .unwrap()
                .unwrap();
            server.abort();
        }
    }
}

#[cfg(test)]
mod proxy_snapshot_tests {
    use super::{
        tests::{endpoint, server},
        *,
    };
    use crate::control::CoreClient;
    use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn router(
        group: impl Fn(usize) -> axum::response::Response + Clone + Send + Sync + 'static,
    ) -> (Router, Arc<AtomicUsize>) {
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = reads.clone();
        let router = Router::new()
            .route(
                "/proxies",
                get(|| async { Json(serde_json::json!({"proxies":{}})) }),
            )
            .route(
                "/providers/proxies",
                get(|| async { Json(serde_json::json!({"providers":{}})) }),
            )
            .route(
                "/group",
                get(move || {
                    let response = group(counter.fetch_add(1, Ordering::SeqCst));
                    async move { response }
                }),
            );
        (router, reads)
    }

    #[tokio::test]
    async fn missing_group_route_is_probed_once_per_instance() {
        let (router, reads) = router(|_| StatusCode::NOT_FOUND.into_response());
        let (url, server) = server(router).await;
        let endpoint = endpoint(url);
        let core = CoreClient::spawn(endpoint.clone()).await.unwrap();
        let api = core.api_client().await.unwrap();
        for _ in 0..2 {
            assert!(api.proxy_snapshot().await.unwrap().groups.is_none());
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);

        endpoint.binding.send_modify(|binding| {
            binding.as_mut().unwrap().instance_id = "second-process".into();
        });
        let api = core.api_client().await.unwrap();
        assert!(api.proxy_snapshot().await.unwrap().groups.is_none());
        assert_eq!(reads.load(Ordering::SeqCst), 2);
        core.shutdown().await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn transient_group_failures_fall_back_until_the_list_answers() {
        let (router, reads) = router(|attempt| match attempt {
            0 => StatusCode::SERVICE_UNAVAILABLE.into_response(),
            1 => Json(serde_json::json!({"proxies":{
                "Meow": {"name":"Meow","type":"Selector","udp":true,"history":[],"all":[]}
            }}))
            .into_response(),
            _ => StatusCode::NOT_FOUND.into_response(),
        });
        let (url, server) = server(router).await;
        let core = CoreClient::spawn(endpoint(url)).await.unwrap();
        let api = core.api_client().await.unwrap();
        assert!(api.proxy_snapshot().await.unwrap().groups.is_none());
        let groups = api.proxy_snapshot().await.unwrap().groups.unwrap();
        assert!(groups.contains_key(&ProxyName::from("Meow")));
        // A later failure no longer changes what the instance supports.
        assert!(api.proxy_snapshot().await.unwrap().groups.is_none());
        assert!(api.proxy_snapshot().await.unwrap().groups.is_none());
        assert_eq!(reads.load(Ordering::SeqCst), 4);
        core.shutdown().await.unwrap();
        server.abort();
    }
}
