use super::NyanpasuClient;
pub type TrafficStream<T> = std::pin::Pin<
    Box<dyn futures::Stream<Item = nyanpasu_traffic::TrafficResult<Option<T>>> + Send>,
>;
use nyanpasu_traffic::*;
impl NyanpasuClient {
    fn traffic_client(&self) -> TrafficResult<&crate::core::traffic::TrafficClient> {
        self.inner.traffic.as_ref().map_err(Clone::clone)
    }
    pub async fn get_current_traffic_session(&self) -> TrafficResult<Option<SessionRecord>> {
        self.traffic_client()?.current_session().await
    }
    pub async fn traffic_status(&self) -> TrafficResult<()> {
        match self.traffic_client()?.subscribe_status().borrow().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub async fn get_traffic_session(&self, request: SessionId) -> TrafficResult<SessionRecord> {
        self.traffic_client()?.session(request).await
    }
    pub async fn query_traffic_connections(
        &self,
        request: ConnectionsQuery,
    ) -> TrafficResult<ConnectionPage> {
        self.traffic_client()?.query_connections(request).await
    }
    pub async fn query_traffic_usage(&self, request: UsageQuery) -> TrafficResult<UsageResult> {
        self.traffic_client()?.query_usage(request).await
    }
    pub async fn query_traffic_topology(
        &self,
        request: TopologyQuery,
    ) -> TrafficResult<TopologyResult> {
        self.traffic_client()?.query_topology(request).await
    }
    pub async fn subscribe_traffic_summary(&self) -> TrafficResult<TrafficStream<TrafficSummary>> {
        let receiver = self.traffic_client()?.subscribe_summary();
        Ok(Box::pin(futures::stream::unfold(
            (receiver, true),
            |(mut receiver, first)| async move {
                if !first {
                    receiver.changed().await.ok()?;
                }
                let value = receiver.borrow_and_update().clone();
                Some((Ok(value), (receiver, false)))
            },
        )))
    }
    pub async fn subscribe_traffic_details(&self) -> TrafficResult<TrafficStream<TrafficDetails>> {
        let receiver = self.traffic_client()?.subscribe_details();
        Ok(Box::pin(futures::stream::unfold(
            (receiver, true),
            |(mut receiver, first)| async move {
                if !first {
                    receiver.changed().await.ok()?;
                }
                let value = receiver.borrow_and_update().clone();
                Some((Ok(value), (receiver, false)))
            },
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::traffic::{TrafficActorArgs, TrafficClient};
    use nyanpasu_traffic::test_support::{FakeClock, FakeTrafficStore, IdleSource};
    use std::sync::Arc;

    #[test]
    fn history_queries_survive_a_shut_down_core_endpoint() {
        let directory = tempfile::tempdir().unwrap();
        let endpoint = crate::core::actor_v2::api::tests::endpoint("http://localhost:9090/".into());
        let mut args = super::super::tests::test_client_args_with_endpoint(&directory, endpoint);
        let traffic = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    tauri::async_runtime::block_on(TrafficClient::start(TrafficActorArgs {
                        host: HostId("application".into()),
                        source: Arc::new(IdleSource),
                        store: Arc::new(FakeTrafficStore::default()),
                        clock: Arc::new(FakeClock::new(1000, 0)),
                        cancellation: args.shutdown.clone(),
                    }))
                })
                .join()
                .unwrap()
        })
        .unwrap();
        args.traffic = Ok(traffic.clone());
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        tauri::async_runtime::block_on(async {
            let session = traffic
                .instance_started(NewSession {
                    host: HostId("application".into()),
                    instance_id: "historical-process".into(),
                    process_started_at: None,
                    attached_at: UInt(1000),
                    late_attach: true,
                })
                .await
                .unwrap();
            client.inner.core_api.shutdown().await.unwrap();
            assert!(client.inner.core_api.connected_endpoint().await.is_err());
            assert_eq!(
                client
                    .get_traffic_session(session.id.clone())
                    .await
                    .unwrap()
                    .id,
                session.id
            );
            assert_eq!(
                client
                    .get_current_traffic_session()
                    .await
                    .unwrap()
                    .unwrap()
                    .id,
                session.id
            );
            let page = client
                .query_traffic_connections(ConnectionsQuery {
                    session_id: session.id,
                    filter: ConnectionFilter::default(),
                    limit: 20,
                    cursor: None,
                })
                .await
                .unwrap();
            assert!(page.connections.is_empty());
            client.traffic_status().await.unwrap();
            traffic.shutdown().await.unwrap();
            client.request_shutdown();
            client.wait_shutdown().await;
        });
    }
}
