use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nyanpasu_config::{application::NyanpasuAppConfig, profile::Profiles};
use nyanpasu_core::state::StateSnapshot;
use nyanpasu_traffic::{
    ClosedCursor, ClosedPage, Dimension, Metric, ReportRequest, TrafficFilter, TrafficQuery,
    TrafficRange, TrafficReport, TrafficSummary, UsageCursor, UsageGroup, UsagePage,
};

use super::{ClientError, NyanpasuClient, Result};
use crate::core::traffic::{Clock, ProfileSelection, RetentionPolicy, TrafficClient};

/// Reads the selected profile from the committed profiles state.
pub(super) struct SelectedProfile(StateSnapshot<Profiles>);

impl SelectedProfile {
    pub(super) fn new(profiles: StateSnapshot<Profiles>) -> Self {
        Self(profiles)
    }
}

impl ProfileSelection for SelectedProfile {
    fn current(&self) -> Option<String> {
        self.0
            .load()
            .state
            .current
            .as_ref()
            .map(ToString::to_string)
    }
}

/// Reads the traffic retention from the committed application config.
pub(super) struct SettingsRetention(StateSnapshot<NyanpasuAppConfig>);

impl SettingsRetention {
    pub(super) fn new(application: StateSnapshot<NyanpasuAppConfig>) -> Self {
        Self(application)
    }
}

impl RetentionPolicy for SettingsRetention {
    fn retention(&self) -> Option<Duration> {
        self.0.load().state.traffic_retention.duration()
    }
}

pub(super) struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
            })
    }
}

impl NyanpasuClient {
    fn traffic(&self) -> Result<&TrafficClient> {
        self.inner
            .traffic
            .as_ref()
            .ok_or_else(|| ClientError::Custom("traffic recording is unavailable".into()))
    }

    pub async fn traffic_summary(&self) -> Result<TrafficSummary> {
        Ok(self.traffic()?.summary().await?)
    }
    pub async fn query_traffic_report(&self, request: ReportRequest) -> Result<TrafficReport> {
        Ok(self.traffic()?.report(request).await?)
    }
    pub async fn query_traffic_usage(
        &self,
        query: TrafficQuery,
        group_by: Dimension,
        metric: Metric,
        after: Option<UsageCursor>,
        limit: usize,
    ) -> Result<UsagePage> {
        Ok(self
            .traffic()?
            .usage(query, group_by, metric, after, limit)
            .await?)
    }
    pub async fn query_traffic_usage_by_keys(
        &self,
        query: TrafficQuery,
        group_by: Dimension,
        keys: Vec<String>,
    ) -> Result<Vec<UsageGroup>> {
        Ok(self.traffic()?.usage_by_keys(query, group_by, keys).await?)
    }
    pub async fn query_traffic_closed_connections(
        &self,
        range: TrafficRange,
        filters: Vec<TrafficFilter>,
        before: Option<ClosedCursor>,
        limit: usize,
    ) -> Result<ClosedPage> {
        Ok(self
            .traffic()?
            .closed_connections(range, filters, before, limit)
            .await?)
    }
    pub async fn query_traffic_active_connection_ids(
        &self,
        filters: Vec<TrafficFilter>,
    ) -> Result<Vec<String>> {
        Ok(self.traffic()?.active_connection_ids(filters).await?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use nyanpasu_traffic::{
        ActiveConnection, ClosedCursor, ClosedPage, ClosedSelection, Dimension, Dimensions,
        FlushBatch, Flushed, Metric, RedbTrafficStore, ReportRequest, SessionMeta, Tier,
        TrafficError, TrafficQuery, TrafficRange, TrafficResult, TrafficScope, TrafficStore, Usage,
    };
    use tempfile::tempdir;

    use super::super::{
        NyanpasuClient,
        tests::{test_client_args_with_endpoint, test_idle_endpoint},
    };

    fn everything() -> TrafficQuery {
        TrafficQuery {
            range: TrafficRange::All,
            scope: TrafficScope::All,
            filters: Vec::new(),
        }
    }

    /// A store whose session cannot be read; it must never be written.
    #[derive(Default)]
    struct UnreadableStore {
        flushes: AtomicUsize,
    }

    impl TrafficStore for UnreadableStore {
        fn load(&self) -> TrafficResult<Option<(SessionMeta, Vec<ActiveConnection>)>> {
            Err(TrafficError::Storage("unreadable".into()))
        }

        fn flush(&self, _: &FlushBatch) -> TrafficResult<Flushed> {
            self.flushes.fetch_add(1, Ordering::SeqCst);
            Ok(Flushed::default())
        }

        fn closed_connections(
            &self,
            _: Option<&ClosedCursor>,
            _: usize,
            _: &ClosedSelection,
        ) -> TrafficResult<ClosedPage> {
            unreachable!("recording is disabled")
        }

        fn closed_count(&self) -> TrafficResult<u64> {
            unreachable!("recording is disabled")
        }

        fn usage(&self, _: Tier, _: Option<u32>) -> TrafficResult<Vec<(Arc<Dimensions>, Usage)>> {
            unreachable!("recording is disabled")
        }

        fn collect_tuples(&self, _: &[Arc<Dimensions>]) -> TrafficResult<u64> {
            unreachable!("recording is disabled")
        }
    }

    #[test]
    fn disabled_recording_reports_unavailable() {
        let dir = tempdir().unwrap();
        let client = NyanpasuClient::try_new_with_args(test_client_args_with_endpoint(
            &dir,
            test_idle_endpoint(),
        ))
        .unwrap();
        tauri::async_runtime::block_on(async {
            let unavailable = "traffic recording is unavailable";
            assert_eq!(
                client.traffic_summary().await.unwrap_err().to_string(),
                unavailable
            );
            assert_eq!(
                client
                    .query_traffic_usage(everything(), Dimension::Process, Metric::Bytes, None, 10)
                    .await
                    .unwrap_err()
                    .to_string(),
                unavailable
            );
            assert_eq!(
                client
                    .query_traffic_usage_by_keys(
                        everything(),
                        Dimension::Process,
                        vec!["curl".into()]
                    )
                    .await
                    .unwrap_err()
                    .to_string(),
                unavailable
            );
            assert_eq!(
                client
                    .query_traffic_report(ReportRequest {
                        query: everything(),
                        metric: Metric::Bytes,
                        rankings: vec![Dimension::Process],
                        ranking_limit: 10,
                        topology: None,
                    })
                    .await
                    .unwrap_err()
                    .to_string(),
                unavailable
            );
            assert_eq!(
                client
                    .query_traffic_closed_connections(TrafficRange::All, Vec::new(), None, 10)
                    .await
                    .unwrap_err()
                    .to_string(),
                unavailable
            );
            assert_eq!(
                client
                    .query_traffic_active_connection_ids(Vec::new())
                    .await
                    .unwrap_err()
                    .to_string(),
                unavailable
            );
        });
    }

    #[test]
    fn an_unreadable_store_disables_recording_without_failing_the_client() {
        let dir = tempdir().unwrap();
        let store = Arc::new(UnreadableStore::default());
        let mut args = test_client_args_with_endpoint(&dir, test_idle_endpoint());
        args.traffic_store = Some(store.clone());
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        tauri::async_runtime::block_on(async {
            assert_eq!(
                client.traffic_summary().await.unwrap_err().to_string(),
                "traffic recording is unavailable"
            );
        });
        drop(client);
        assert_eq!(store.flushes.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn injected_store_starts_an_empty_session() {
        let dir = tempdir().unwrap();
        let mut args = test_client_args_with_endpoint(&dir, test_idle_endpoint());
        args.traffic_store = Some(Arc::new(
            RedbTrafficStore::open(&dir.path().join("traffic.redb")).unwrap(),
        ));
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        tauri::async_runtime::block_on(async {
            let summary = client.traffic_summary().await.unwrap();
            assert_eq!(summary.active_connections, 0);
        });
    }
}
