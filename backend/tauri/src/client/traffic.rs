use nyanpasu_config::profile::Profiles;
use nyanpasu_core::state::StateSnapshot;
use nyanpasu_traffic::{
    ClosedCursor, ClosedPage, GroupBy, Topology, TrafficSummary, Usage, UsageCursor, UsageGroup,
};

use super::{ClientError, NyanpasuClient, Result};
use crate::core::traffic::{ProfileSelection, TrafficClient};

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
    pub async fn query_traffic_usage(
        &self,
        group_by: GroupBy,
        after: Option<UsageCursor>,
        limit: usize,
    ) -> Result<Usage> {
        Ok(self.traffic()?.usage(group_by, after, limit).await?)
    }
    pub async fn query_traffic_usage_by_keys(
        &self,
        group_by: GroupBy,
        keys: Vec<String>,
    ) -> Result<Vec<UsageGroup>> {
        Ok(self.traffic()?.usage_by_keys(group_by, keys).await?)
    }
    pub async fn query_traffic_topology(&self, limit: usize) -> Result<Topology> {
        Ok(self.traffic()?.topology(limit).await?)
    }
    pub async fn query_traffic_closed_connections(
        &self,
        before: Option<ClosedCursor>,
        limit: usize,
    ) -> Result<ClosedPage> {
        Ok(self.traffic()?.closed_connections(before, limit).await?)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nyanpasu_traffic::{GroupBy, RedbTrafficStore};
    use tempfile::tempdir;

    use super::super::{
        NyanpasuClient,
        tests::{test_client_args_with_endpoint, test_idle_endpoint},
    };

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
                    .query_traffic_usage(GroupBy::Process, None, 10)
                    .await
                    .unwrap_err()
                    .to_string(),
                unavailable
            );
            assert_eq!(
                client
                    .query_traffic_usage_by_keys(GroupBy::Process, vec!["curl".into()])
                    .await
                    .unwrap_err()
                    .to_string(),
                unavailable
            );
            assert_eq!(
                client
                    .query_traffic_topology(10)
                    .await
                    .unwrap_err()
                    .to_string(),
                unavailable
            );
            assert_eq!(
                client
                    .query_traffic_closed_connections(None, 10)
                    .await
                    .unwrap_err()
                    .to_string(),
                unavailable
            );
        });
    }

    #[test]
    fn injected_store_starts_a_session_for_the_selected_profile() {
        let dir = tempdir().unwrap();
        let mut args = test_client_args_with_endpoint(&dir, test_idle_endpoint());
        args.traffic_store = Some(Arc::new(
            RedbTrafficStore::open(&dir.path().join("traffic.redb")).unwrap(),
        ));
        let client = NyanpasuClient::try_new_with_args(args).unwrap();
        tauri::async_runtime::block_on(async {
            let summary = client.traffic_summary().await.unwrap();
            assert_eq!(summary.profile, None);
            assert_eq!(summary.active_connections, 0);
        });
    }
}
