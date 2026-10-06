//! Concrete Clash API transport bound to one endpoint's controller details.

use std::{future::Future, sync::Arc, time::Duration};

use clash_api::{Delay, DelayQuery, ProviderName, ProxyName, Version};
use nyanpasu_application::core::api::{ApiError, ApiResponseStream, ApiResult, InstanceApiPort};
use nyanpasu_ipc::api::{core::v2::CoreApiConnection, status::CoreControllerInfo};

const CLASH_REQUEST_DEADLINE: Duration = Duration::from_secs(30);

async fn clash_request<T>(request: impl Future<Output = clash_api::Result<T>>) -> ApiResult<T> {
    tokio::time::timeout(CLASH_REQUEST_DEADLINE, request)
        .await
        .map_err(|_| ApiError::Timeout)?
        .map_err(ApiError::Protocol)
}

pub(super) struct ClashApiBackend {
    client: clash_api::Client,
}

impl ClashApiBackend {
    pub(super) fn new(binding: &CoreApiConnection) -> Result<Self, ApiError> {
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
        })
    }
}

pub fn clash_api_backend(
    binding: &CoreApiConnection,
) -> Result<Arc<dyn InstanceApiPort>, ApiError> {
    Ok(Arc::new(ClashApiBackend::new(binding)?))
}

#[async_trait::async_trait]
impl InstanceApiPort for ClashApiBackend {
    async fn connections_ws(&self) -> ApiResult<ApiResponseStream<clash_api::ConnectionsSnapshot>> {
        Ok(Box::pin(
            clash_request(self.client.connections_ws(Default::default())).await?,
        ))
    }

    async fn logs_ws(
        &self,
        level: clash_api::LogLevel,
    ) -> ApiResult<ApiResponseStream<clash_api::LogEntry>> {
        Ok(Box::pin(
            clash_request(self.client.logs_ws(clash_api::LogQuery::new(level))).await?,
        ))
    }

    async fn traffic_ws(&self) -> ApiResult<ApiResponseStream<clash_api::Traffic>> {
        Ok(Box::pin(clash_request(self.client.traffic_ws()).await?))
    }

    async fn memory_ws(&self) -> ApiResult<ApiResponseStream<clash_api::Memory>> {
        Ok(Box::pin(clash_request(self.client.memory_ws()).await?))
    }

    async fn proxies(&self) -> ApiResult<clash_api::IndexMap<ProxyName, clash_api::Proxy>> {
        clash_request(self.client.proxies()).await
    }

    async fn proxy_providers(
        &self,
    ) -> ApiResult<clash_api::IndexMap<ProviderName, clash_api::ProxyProvider>> {
        clash_request(self.client.proxy_providers()).await
    }

    async fn groups(&self) -> ApiResult<clash_api::IndexMap<ProxyName, clash_api::Proxy>> {
        clash_request(self.client.groups()).await
    }

    async fn select_proxy(&self, group: &ProxyName, target: &ProxyName) -> ApiResult<()> {
        clash_request(
            self.client
                .select_proxy(clash_api::ProxySelection { group, target }),
        )
        .await
    }

    async fn update_proxy_provider(&self, name: &ProviderName) -> ApiResult<()> {
        clash_request(self.client.update_proxy_provider(name)).await
    }

    async fn configs(&self) -> ApiResult<clash_api::RuntimeConfig> {
        clash_request(self.client.configs()).await
    }

    async fn rule_providers(
        &self,
    ) -> ApiResult<indexmap::IndexMap<clash_api::RuleProviderName, clash_api::RuleProvider>> {
        clash_request(self.client.rule_providers()).await
    }

    async fn rules(&self) -> ApiResult<Vec<clash_api::Rule>> {
        clash_request(self.client.rules()).await
    }

    async fn update_rule_provider(&self, name: &clash_api::RuleProviderName) -> ApiResult<()> {
        clash_request(self.client.update_rule_provider(name)).await
    }

    async fn version(&self) -> ApiResult<Version> {
        clash_request(self.client.version()).await
    }

    async fn proxy_delay(
        &self,
        name: &ProxyName,
        provider: Option<&ProviderName>,
        query: &DelayQuery,
    ) -> ApiResult<Delay> {
        match provider {
            Some(provider) => {
                clash_request(self.client.provider_proxy_delay(provider, name, query)).await
            }
            None => clash_request(self.client.proxy_delay(name, query)).await,
        }
    }

    async fn group_delay(
        &self,
        group: &ProxyName,
        query: &DelayQuery,
    ) -> ApiResult<indexmap::IndexMap<ProxyName, u16>> {
        clash_request(self.client.group_delay(group, query)).await
    }

    async fn connections(&self) -> ApiResult<clash_api::ConnectionsSnapshot> {
        clash_request(self.client.connections()).await
    }

    async fn close_connection(&self, id: uuid::Uuid) -> ApiResult<()> {
        clash_request(self.client.close_connection(id)).await
    }

    async fn close_all_connections(&self) -> ApiResult<()> {
        clash_request(self.client.close_all_connections()).await
    }
}
