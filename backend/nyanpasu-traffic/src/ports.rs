use crate::model::*;
use futures_util::Stream;
use std::{pin::Pin, sync::Arc};
#[async_trait::async_trait]
pub trait TrafficStore: Send + Sync + 'static {
    async fn recover(&self, host: HostId) -> TrafficResult<RecoveryState>;
    async fn begin_session(&self, session: NewSession) -> TrafficResult<SessionRecord>;
    async fn commit_observation(&self, batch: ObservationCommit) -> TrafficResult<CommitReceipt>;
    async fn committed_position(&self, session: SessionId) -> TrafficResult<CommittedPosition>;
    async fn finish_session(&self, end: SessionEnd) -> TrafficResult<CommitReceipt>;
    async fn session(&self, id: SessionId) -> TrafficResult<SessionRecord>;
    async fn latest_session(&self, host: HostId) -> TrafficResult<Option<SessionRecord>>;
    async fn connection(
        &self,
        session: SessionId,
        id: String,
    ) -> TrafficResult<Option<ConnectionRecord>>;
    async fn connections_by_ids(
        &self,
        session: SessionId,
        ids: Vec<String>,
    ) -> TrafficResult<std::collections::BTreeMap<String, ConnectionRecord>> {
        let mut records = std::collections::BTreeMap::new();
        for id in ids {
            if let Some(record) = self.connection(session.clone(), id.clone()).await? {
                records.insert(id, record);
            }
        }
        Ok(records)
    }
    async fn query_connections(&self, query: ConnectionsQuery) -> TrafficResult<ConnectionPage>;
    async fn query_usage(&self, query: UsageQuery) -> TrafficResult<UsageResult>;
    async fn query_topology(&self, query: TopologyQuery) -> TrafficResult<TopologyResult>;
    async fn prune(&self, policy: RetentionPolicy) -> TrafficResult<PruneReport>;
    async fn flush(&self) -> TrafficResult<()>;
}
/// Credentials stay in the source binding; never in persisted or wire models.
#[derive(Clone)]
pub struct SourceBinding {
    pub instance_id: String,
    pub endpoint: SourceEndpoint,
    pub secret: Option<String>,
}
#[derive(Clone, Debug)]
pub enum SourceEndpoint {
    Http(String),
    UnixSocket(std::path::PathBuf),
    NamedPipe(std::path::PathBuf),
}
pub type ObservationStream = Pin<Box<dyn Stream<Item = TrafficResult<Observation>> + Send>>;
#[async_trait::async_trait]
pub trait TrafficSource: Send + Sync + 'static {
    async fn connect(
        &self,
        binding: SourceBinding,
        generation: UInt,
        clock: Arc<dyn Clock>,
    ) -> TrafficResult<ObservationStream>;
}
pub trait Clock: Send + Sync + 'static {
    fn wall_time(&self) -> UInt;
    fn monotonic_ns(&self) -> UInt;
}
pub struct SystemClock {
    origin: std::time::Instant,
}
impl Default for SystemClock {
    fn default() -> Self {
        Self {
            origin: std::time::Instant::now(),
        }
    }
}
impl Clock for SystemClock {
    fn wall_time(&self) -> UInt {
        UInt(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock predates Unix epoch")
                .as_millis()
                .try_into()
                .expect("wall clock overflow"),
        )
    }
    fn monotonic_ns(&self) -> UInt {
        UInt(
            self.origin
                .elapsed()
                .as_nanos()
                .try_into()
                .expect("monotonic clock overflow"),
        )
    }
}

impl std::fmt::Debug for SourceBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceBinding")
            .field("instance_id", &self.instance_id)
            .field("endpoint", &self.endpoint)
            .field("secret", &self.secret.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}
