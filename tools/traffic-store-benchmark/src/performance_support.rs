use nyanpasu_traffic::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Instant,
};

pub struct MeasuredStore {
    pub inner: Arc<dyn TrafficStore>,
    pub commits: Mutex<Vec<f64>>,
}
#[async_trait::async_trait]
impl TrafficStore for MeasuredStore {
    async fn recover(&self, host: HostId) -> TrafficResult<RecoveryState> {
        self.inner.recover(host).await
    }
    async fn begin_session(&self, session: NewSession) -> TrafficResult<SessionRecord> {
        self.inner.begin_session(session).await
    }
    async fn commit_observation(&self, batch: ObservationCommit) -> TrafficResult<CommitReceipt> {
        let start = Instant::now();
        let result = self.inner.commit_observation(batch).await;
        self.commits
            .lock()
            .unwrap()
            .push(start.elapsed().as_secs_f64() * 1000.0);
        result
    }
    async fn committed_position(&self, session: SessionId) -> TrafficResult<CommittedPosition> {
        self.inner.committed_position(session).await
    }
    async fn finish_session(&self, end: SessionEnd) -> TrafficResult<CommitReceipt> {
        self.inner.finish_session(end).await
    }
    async fn session(&self, id: SessionId) -> TrafficResult<SessionRecord> {
        self.inner.session(id).await
    }
    async fn latest_session(&self, host: HostId) -> TrafficResult<Option<SessionRecord>> {
        self.inner.latest_session(host).await
    }
    async fn connection(
        &self,
        session: SessionId,
        id: String,
    ) -> TrafficResult<Option<ConnectionRecord>> {
        self.inner.connection(session, id).await
    }
    async fn connections_by_ids(
        &self,
        session: SessionId,
        ids: Vec<String>,
    ) -> TrafficResult<BTreeMap<String, ConnectionRecord>> {
        self.inner.connections_by_ids(session, ids).await
    }
    async fn query_connections(&self, query: ConnectionsQuery) -> TrafficResult<ConnectionPage> {
        self.inner.query_connections(query).await
    }
    async fn query_usage(&self, query: UsageQuery) -> TrafficResult<UsageResult> {
        self.inner.query_usage(query).await
    }
    async fn query_topology(&self, query: TopologyQuery) -> TrafficResult<TopologyResult> {
        self.inner.query_topology(query).await
    }
    async fn prune(&self, policy: RetentionPolicy) -> TrafficResult<PruneReport> {
        self.inner.prune(policy).await
    }
    async fn flush(&self) -> TrafficResult<()> {
        self.inner.flush().await
    }
}
pub fn distribution(values: &[f64]) -> serde_json::Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    serde_json::json!({"mean_ms": sorted.iter().sum::<f64>()/sorted.len() as f64,"p50_ms":sorted[(sorted.len()-1)*50/100],"p95_ms":sorted[(sorted.len()-1)*95/100],"max_ms":sorted.last(),"count":sorted.len()})
}
pub fn files(directory: &std::path::Path) -> serde_json::Value {
    let mut files = BTreeMap::new();
    let mut total = 0;
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let meta = entry.metadata().unwrap();
        if meta.is_file() {
            total += meta.len();
            files.insert(entry.file_name().to_string_lossy().into_owned(), meta.len());
        }
    }
    serde_json::json!({"files":files,"total_bytes":total})
}
