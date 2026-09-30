use super::{CoreClient, endpoint::TrafficStream};
use nyanpasu_traffic::*;
impl CoreClient {
    pub async fn get_current_traffic_session(&self) -> TrafficResult<Option<SessionRecord>> {
        self.connected_endpoint()
            .await
            .map_err(|e| StoreError::Unavailable(e.to_string()))?
            .get_current_traffic_session()
            .await
    }
    pub async fn traffic_status(&self) -> TrafficResult<()> {
        self.connected_endpoint()
            .await
            .map_err(|e| StoreError::Unavailable(e.to_string()))?
            .traffic_status()
            .await
    }

    pub async fn get_traffic_session(&self, request: SessionId) -> TrafficResult<SessionRecord> {
        self.connected_endpoint()
            .await
            .map_err(|e| StoreError::Unavailable(e.to_string()))?
            .get_traffic_session(request)
            .await
    }
    pub async fn query_traffic_connections(
        &self,
        request: ConnectionsQuery,
    ) -> TrafficResult<ConnectionPage> {
        self.connected_endpoint()
            .await
            .map_err(|e| StoreError::Unavailable(e.to_string()))?
            .query_traffic_connections(request)
            .await
    }
    pub async fn query_traffic_usage(&self, request: UsageQuery) -> TrafficResult<UsageResult> {
        self.connected_endpoint()
            .await
            .map_err(|e| StoreError::Unavailable(e.to_string()))?
            .query_traffic_usage(request)
            .await
    }
    pub async fn query_traffic_topology(
        &self,
        request: TopologyQuery,
    ) -> TrafficResult<TopologyResult> {
        self.connected_endpoint()
            .await
            .map_err(|e| StoreError::Unavailable(e.to_string()))?
            .query_traffic_topology(request)
            .await
    }
    pub async fn subscribe_traffic_summary(&self) -> TrafficResult<TrafficStream<TrafficSummary>> {
        self.connected_endpoint()
            .await
            .map_err(|e| StoreError::Unavailable(e.to_string()))?
            .subscribe_traffic_summary()
            .await
    }
    pub async fn subscribe_traffic_details(&self) -> TrafficResult<TrafficStream<TrafficDetails>> {
        self.connected_endpoint()
            .await
            .map_err(|e| StoreError::Unavailable(e.to_string()))?
            .subscribe_traffic_details()
            .await
    }
}
