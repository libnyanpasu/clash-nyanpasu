//! Instance-bound Clash API capability. The protocol client never escapes this
//! adapter: clones share revocation and every operation checks the applied binding.

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
    time::Duration,
};

use clash_api::{Delay, DelayQuery, ProviderName, ProxyName, Version};
use nyanpasu_ipc::api::core::v2::CoreApiConnection;
use tokio_util::sync::CancellationToken;

use super::endpoint::EndpointHandle;

pub type ApiResult<T> = Result<T, ApiError>;
pub type ApiResponseStream<T> = Pin<Box<dyn futures::Stream<Item = clash_api::Result<T>> + Send>>;

/// One instance-bound Clash API implementation supplied by the platform
/// adapter. The application owns capability validation and revocation; this
/// port owns the actual network protocol operations.
#[async_trait::async_trait]
pub trait InstanceApiPort: Send + Sync {
    async fn connections_ws(&self) -> ApiResult<ApiResponseStream<clash_api::ConnectionsSnapshot>>;
    async fn logs_ws(
        &self,
        level: clash_api::LogLevel,
    ) -> ApiResult<ApiResponseStream<clash_api::LogEntry>>;
    async fn traffic_ws(&self) -> ApiResult<ApiResponseStream<clash_api::Traffic>>;
    async fn memory_ws(&self) -> ApiResult<ApiResponseStream<clash_api::Memory>>;
    async fn proxies(&self) -> ApiResult<clash_api::IndexMap<ProxyName, clash_api::Proxy>>;
    async fn proxy_providers(
        &self,
    ) -> ApiResult<clash_api::IndexMap<ProviderName, clash_api::ProxyProvider>>;
    async fn groups(&self) -> ApiResult<clash_api::IndexMap<ProxyName, clash_api::Proxy>>;
    async fn select_proxy(&self, group: &ProxyName, target: &ProxyName) -> ApiResult<()>;
    async fn update_proxy_provider(&self, name: &ProviderName) -> ApiResult<()>;
    async fn configs(&self) -> ApiResult<clash_api::RuntimeConfig>;
    async fn rule_providers(
        &self,
    ) -> ApiResult<indexmap::IndexMap<clash_api::RuleProviderName, clash_api::RuleProvider>>;
    async fn rules(&self) -> ApiResult<Vec<clash_api::Rule>>;
    async fn update_rule_provider(&self, name: &clash_api::RuleProviderName) -> ApiResult<()>;
    async fn version(&self) -> ApiResult<Version>;
    async fn proxy_delay(
        &self,
        name: &ProxyName,
        provider: Option<&ProviderName>,
        query: &DelayQuery,
    ) -> ApiResult<Delay>;
    async fn group_delay(
        &self,
        group: &ProxyName,
        query: &DelayQuery,
    ) -> ApiResult<indexmap::IndexMap<ProxyName, u16>>;
    async fn connections(&self) -> ApiResult<clash_api::ConnectionsSnapshot>;
    async fn close_connection(&self, id: uuid::Uuid) -> ApiResult<()>;
    async fn close_all_connections(&self) -> ApiResult<()>;
}

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
    backend: Arc<dyn InstanceApiPort>,
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
        backend: Arc<dyn InstanceApiPort>,
    ) -> Self {
        Self {
            backend,
            binding,
            endpoint,
            revoked: CancellationToken::new(),
            group_list: Arc::default(),
        }
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
        operation: impl Future<Output = ApiResult<T>>,
    ) -> Result<T, ApiError> {
        tokio::select! {
            biased;
            _ = self.revoked.cancelled() => Err(ApiError::Stale),
            result = async {
                self.check().await?;
                let result = operation.await;
                self.check().await?;
                result
            } => result,
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
        let stream = self.execute(self.backend.connections_ws()).await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn logs_ws(
        &self,
        level: clash_api::LogLevel,
    ) -> Result<ApiStream<clash_api::LogEntry>, ApiError> {
        let stream = self.execute(self.backend.logs_ws(level)).await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn traffic_ws(&self) -> Result<ApiStream<clash_api::Traffic>, ApiError> {
        let stream = self.execute(self.backend.traffic_ws()).await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn memory_ws(&self) -> Result<ApiStream<clash_api::Memory>, ApiError> {
        let stream = self.execute(self.backend.memory_ws()).await?;
        Ok(ApiStream::new(self.clone(), stream))
    }

    pub async fn proxy_snapshot(&self) -> Result<ProxySnapshot, ApiError> {
        self.execute(async {
            let (proxies, providers, groups) = tokio::try_join!(
                self.backend.proxies(),
                self.backend.proxy_providers(),
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
        match self.backend.groups().await {
            Ok(groups) => {
                let _ = self.group_list.set(true);
                Some(groups)
            }
            Err(error) => {
                if matches!(&error, ApiError::Protocol(protocol) if protocol
                    .status()
                    .is_some_and(|status| matches!(status.as_u16(), 404 | 405)))
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
        self.execute(self.backend.select_proxy(group, target)).await
    }

    pub async fn update_proxy_provider(&self, name: &ProviderName) -> Result<(), ApiError> {
        self.execute(self.backend.update_proxy_provider(name)).await
    }

    pub async fn configs(&self) -> Result<clash_api::RuntimeConfig, ApiError> {
        self.execute(self.backend.configs()).await
    }

    pub async fn rule_providers(
        &self,
    ) -> Result<indexmap::IndexMap<clash_api::RuleProviderName, clash_api::RuleProvider>, ApiError>
    {
        self.execute(self.backend.rule_providers()).await
    }

    pub async fn rules(&self) -> Result<Vec<clash_api::Rule>, ApiError> {
        self.execute(self.backend.rules()).await
    }

    pub async fn update_rule_provider(
        &self,
        name: &clash_api::RuleProviderName,
    ) -> Result<(), ApiError> {
        self.execute(self.backend.update_rule_provider(name)).await
    }

    pub async fn version(&self) -> Result<Version, ApiError> {
        self.execute(self.backend.version()).await
    }

    pub async fn proxy_delay(
        &self,
        name: &ProxyName,
        provider: Option<&ProviderName>,
        query: &DelayQuery,
    ) -> Result<Delay, ApiError> {
        match provider {
            Some(provider) => {
                self.execute(self.backend.proxy_delay(name, Some(provider), query))
                    .await
            }
            None => {
                self.execute(self.backend.proxy_delay(name, None, query))
                    .await
            }
        }
    }

    pub async fn group_delay(
        &self,
        group: &ProxyName,
        query: &DelayQuery,
    ) -> Result<indexmap::IndexMap<ProxyName, u16>, ApiError> {
        self.execute(self.backend.group_delay(group, query)).await
    }

    pub async fn connections(&self) -> Result<clash_api::ConnectionsSnapshot, ApiError> {
        self.execute(self.backend.connections()).await
    }

    pub async fn close_connection(&self, id: uuid::Uuid) -> Result<(), ApiError> {
        self.execute(self.backend.close_connection(id)).await
    }

    pub async fn close_all_connections(&self) -> Result<(), ApiError> {
        self.execute(self.backend.close_all_connections()).await
    }
}

/// An owned subscription bound to the same capability for its entire lifetime.
/// Idle reads have no deadline; cancellation drops the socket, and each decoded
/// frame must pass the authoritative binding check before delivery.
pub struct ApiStream<T> {
    api: ApiClient,
    stream: Option<ApiResponseStream<T>>,
}

impl<T> ApiStream<T> {
    fn new(api: ApiClient, stream: ApiResponseStream<T>) -> Self {
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
                Some(frame) => Some(self.api.execute(async { frame.map_err(ApiError::Protocol) }).await),
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
            let subscription = monitored.endpoint.api_changes().await;
            let mut changes = match subscription {
                Ok(Some(changes)) => changes,
                Ok(None) => Box::pin(futures::stream::pending()) as super::endpoint::ApiChanges,
                Err(error) => {
                    tracing::warn!(host = ?monitored.endpoint.host(), "API lifecycle subscription failed: {error}");
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
                match monitored.check().await {
                    Ok(()) => {}
                    Err(ApiError::Stale) => return,
                    Err(error) => {
                        tracing::warn!(host = ?monitored.endpoint.host(), "API binding monitor failed: {:#}", anyhow::Error::new(error));
                        monitored.revoke();
                        return;
                    }
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
#[path = "api_tests.rs"]
mod tests;
