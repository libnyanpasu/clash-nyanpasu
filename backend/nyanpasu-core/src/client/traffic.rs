use std::{
    sync::RwLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use nyanpasu_config::{
    application::{IpLookupProvider, NyanpasuAppConfig, StunServer},
    profile::Profiles,
};
use nyanpasu_observers::traffic::{
    Clock, LocalSourceIps, LocalSourceLocation, ProfileSelection, RetentionPolicy, TrafficClient,
};
use nyanpasu_state_engine::StateSnapshot;
use nyanpasu_traffic::{
    ClosedCursor, ClosedPage, Dimension, Metric, ReportRequest, TrafficFilter, TrafficQuery,
    TrafficRange, TrafficReport, TrafficSummary, UsageCursor, UsageGroup, UsagePage,
};

use super::{ClientError, NyanpasuClient, Result};

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

/// The facade owns this cache; traffic reads immutable copies through its port.
pub(super) struct LocalSourceCache {
    application: StateSnapshot<NyanpasuAppConfig>,
    clash: StateSnapshot<nyanpasu_config::clash::config::ClashConfig>,
    addresses: RwLock<LocalSourceIps>,
}

impl LocalSourceCache {
    pub(super) fn new(
        application: StateSnapshot<NyanpasuAppConfig>,
        clash: StateSnapshot<nyanpasu_config::clash::config::ClashConfig>,
    ) -> Self {
        Self {
            application,
            clash,
            addresses: RwLock::new(LocalSourceIps::default()),
        }
    }

    pub(super) fn set(&self, result: &super::DirectEgress) {
        *self
            .addresses
            .write()
            .expect("local source cache lock poisoned") = match result {
            super::DirectEgress::Probed { ipv4, ipv6 } => LocalSourceIps {
                ipv4: ipv4.as_ref().map(|egress| egress.address),
                ipv6: ipv6.as_ref().map(|egress| egress.address),
            },
            _ => LocalSourceIps::default(),
        };
    }

    pub(super) fn blocked(&self) -> Option<super::DirectEgress> {
        if !self.application.load().state.enable_local_ip_probe {
            Some(super::DirectEgress::Disabled)
        } else {
            let clash = self.clash.load();
            if clash.state.enable_tun_mode {
                Some(super::DirectEgress::TunEnabled)
            } else if captures_local_traffic(&clash.state) {
                Some(super::DirectEgress::TransparentProxyEnabled)
            } else {
                None
            }
        }
    }
}

fn captures_local_traffic(config: &nyanpasu_config::clash::config::ClashConfig) -> bool {
    config.transparent_proxy.local
        && config.transparent_proxy.mode
            != nyanpasu_config::clash::config::TransparentProxyMode::Disabled
}

#[cfg(test)]
mod transparent_capture_tests {
    use super::captures_local_traffic;
    use nyanpasu_config::clash::config::{ClashConfig, TransparentProxyMode};

    #[test]
    fn blocks_only_enabled_local_capture() {
        let mut config = ClashConfig::default();
        assert!(!captures_local_traffic(&config));
        config.transparent_proxy.mode = TransparentProxyMode::Tproxy;
        assert!(captures_local_traffic(&config));
        config.transparent_proxy.local = false;
        assert!(!captures_local_traffic(&config));
        config.transparent_proxy.mode = TransparentProxyMode::Redir;
        config.transparent_proxy.local = true;
        assert!(captures_local_traffic(&config));
    }
}

/// A running probe stops asking once detection is off or local capture is on, and skips
/// a server or provider that has since left the user's list.
impl nyanpasu_contracts::net::ProbePermission for LocalSourceCache {
    fn allows_stun(&self, server: &StunServer) -> bool {
        self.blocked().is_none()
            && self
                .application
                .load()
                .state
                .stun_servers
                .iter()
                .any(|current| {
                    current.enabled
                        && current.port == server.port
                        && current.host.eq_ignore_ascii_case(&server.host)
                })
    }

    fn allows_lookup(&self, provider: IpLookupProvider) -> bool {
        self.blocked().is_none()
            && self
                .application
                .load()
                .state
                .ip_lookup_providers
                .contains(&provider)
    }
}

impl LocalSourceLocation for LocalSourceCache {
    fn addresses(&self) -> LocalSourceIps {
        if self.blocked().is_some() {
            LocalSourceIps::default()
        } else {
            *self
                .addresses
                .read()
                .expect("local source cache lock poisoned")
        }
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
    use nyanpasu_observers::traffic::{
        SpawnError, TrafficArgs, TrafficClient, TrafficOwnerFactory, TrafficStartArgs,
        TrafficStorage,
    };
    use nyanpasu_traffic::{
        Dimension, Metric, ReportRequest, TrafficQuery, TrafficRange, TrafficScope,
    };
    use nyanpasu_tsdb::{
        Profile,
        storage::{StorageError, redb::RedbStorage},
    };
    use std::sync::Arc;
    use tempfile::tempdir;
    use tokio_util::{sync::CancellationToken, task::TaskTracker};

    use super::super::NyanpasuClient;
    use crate::{
        actor_v2::test_support::test_idle_endpoint,
        client::test_support::test_client_args_with_endpoint, traffic::TrafficFileStorage,
    };

    fn everything() -> TrafficQuery {
        TrafficQuery {
            range: TrafficRange::All,
            scope: TrafficScope::All,
            filters: Vec::new(),
        }
    }

    struct UnreadableStore;
    #[async_trait::async_trait]
    impl TrafficOwnerFactory for UnreadableStore {
        async fn spawn(
            self: Arc<Self>,
            args: TrafficStartArgs,
            shutdown: CancellationToken,
            tasks: &TaskTracker,
        ) -> Result<TrafficClient, SpawnError> {
            TrafficClient::spawn(
                TrafficArgs::<RedbStorage> {
                    storage: self,
                    profile: args.profile,
                    profiles: args.profiles,
                    retention: args.retention,
                    clock: args.clock,
                    frames: args.frames,
                    geo: args.geo,
                    local_source: args.local_source,
                },
                shutdown,
                tasks,
            )
            .await
        }
    }

    impl TrafficStorage<RedbStorage> for UnreadableStore {
        fn open(&self) -> Result<RedbStorage, StorageError> {
            Err(StorageError::Failed {
                source: Box::new(std::io::Error::other("unreadable")),
            })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn disabled_recording_reports_unavailable() {
        let dir = tempdir().unwrap();
        let client = NyanpasuClient::try_new(
            test_client_args_with_endpoint(
                &dir,
                test_idle_endpoint(),
                crate::client::test_support::profile_test_adapters(&dir),
            )
            .await,
        )
        .await
        .unwrap();
        (async {
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
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_unreadable_store_disables_recording_without_failing_the_client() {
        let dir = tempdir().unwrap();
        let store = Arc::new(UnreadableStore);
        let mut args = test_client_args_with_endpoint(
            &dir,
            test_idle_endpoint(),
            crate::client::test_support::profile_test_adapters(&dir),
        )
        .await;
        args.traffic_store = Some(store.clone());
        let client = NyanpasuClient::try_new(args).await.unwrap();
        (async {
            assert_eq!(
                client.traffic_summary().await.unwrap_err().to_string(),
                "traffic recording is unavailable"
            );
        })
        .await;
        drop(client);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn injected_store_starts_an_empty_session() {
        let dir = tempdir().unwrap();
        let mut args = test_client_args_with_endpoint(
            &dir,
            test_idle_endpoint(),
            crate::client::test_support::profile_test_adapters(&dir),
        )
        .await;
        args.traffic_store = Some(Arc::new(TrafficFileStorage::new(
            dir.path().join("traffic.tsdb"),
            Profile::desktop().backend_cache,
        )));
        let client = NyanpasuClient::try_new(args).await.unwrap();
        (async {
            let summary = client.traffic_summary().await.unwrap();
            assert_eq!(summary.active_connections, 0);
        })
        .await;
    }
}
