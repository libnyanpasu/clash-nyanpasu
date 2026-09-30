use super::NyanpasuClient;
use crate::core::actor_v2::endpoint::TrafficStream;
use nyanpasu_traffic::*;
impl NyanpasuClient {
    pub async fn get_current_traffic_session(&self) -> TrafficResult<Option<SessionRecord>> {
        self.inner.core_api.get_current_traffic_session().await
    }
    pub async fn traffic_status(&self) -> TrafficResult<()> {
        self.inner.core_api.traffic_status().await
    }

    pub async fn get_traffic_session(&self, request: SessionId) -> TrafficResult<SessionRecord> {
        self.inner.core_api.get_traffic_session(request).await
    }
    pub async fn query_traffic_connections(
        &self,
        request: ConnectionsQuery,
    ) -> TrafficResult<ConnectionPage> {
        self.inner.core_api.query_traffic_connections(request).await
    }
    pub async fn query_traffic_usage(&self, request: UsageQuery) -> TrafficResult<UsageResult> {
        self.inner.core_api.query_traffic_usage(request).await
    }
    pub async fn query_traffic_topology(
        &self,
        request: TopologyQuery,
    ) -> TrafficResult<TopologyResult> {
        self.inner.core_api.query_traffic_topology(request).await
    }
    pub async fn subscribe_traffic_summary(&self) -> TrafficResult<TrafficStream<TrafficSummary>> {
        self.inner.core_api.subscribe_traffic_summary().await
    }
    pub async fn subscribe_traffic_details(&self) -> TrafficResult<TrafficStream<TrafficDetails>> {
        self.inner.core_api.subscribe_traffic_details().await
    }
}
