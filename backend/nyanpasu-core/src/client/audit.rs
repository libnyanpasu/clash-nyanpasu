//! Audit setup inputs, live retention snapshots and the application query facade.

use std::time::Duration;

use nyanpasu_config::application::NyanpasuAppConfig;
use nyanpasu_observers::audit::{
    AuditCursor, AuditDropped, AuditFilter, AuditPage, AuditRetention, AuditRow, AuditStore,
    AuditStoreError,
};
use nyanpasu_state_engine::StateSnapshot;
use snafu::ResultExt as _;
use tokio::sync::mpsc;

use super::NyanpasuClient;

/// Opened by the shell; ownership of the store and receiver passes to the audit
/// actor. Startup failure drops the receiver, naturally disabling the sink.
pub struct AuditSetup {
    /// Unique synchronous storage adapter.
    pub store: Box<dyn AuditStore>,
    /// The bounded diagnostic channel created by the shell.
    pub rows: mpsc::Receiver<AuditRow>,
    /// Shared loss counter also held by the shell's sink.
    pub dropped: AuditDropped,
}

pub(super) struct SettingsAuditRetention(StateSnapshot<NyanpasuAppConfig>);

impl SettingsAuditRetention {
    pub(super) fn new(application: StateSnapshot<NyanpasuAppConfig>) -> Self {
        Self(application)
    }
}

impl AuditRetention for SettingsAuditRetention {
    fn retention(&self) -> (Duration, u64) {
        let snapshot = self.0.load();
        let settings = &snapshot.state.rpc_audit;
        (
            Duration::from_secs(u64::from(settings.max_age_days) * 24 * 60 * 60),
            u64::from(settings.max_size_mib) * 1024 * 1024,
        )
    }
}

/// Failing facade step, retaining the store's typed cause.
#[derive(Debug, snafu::Snafu)]
pub enum Error {
    /// The optional owner is absent because recording could not start.
    #[snafu(display("audit recording is unavailable"))]
    AccessStore,
    /// The owner or its storage could not answer the query.
    #[snafu(display("failed to query RPC audit"))]
    QueryAudit { source: AuditStoreError },
}

impl NyanpasuClient {
    /// Returns persisted HTTP/local audit rows newest first, exclusively before
    /// the opaque cursor. Reads do not flush queued rows; the page limit is 500.
    ///
    /// # Errors
    /// Reports unavailable recording or the underlying storage query failure.
    pub async fn query_rpc_audit(
        &self,
        filter: AuditFilter,
        before: Option<AuditCursor>,
        limit: usize,
    ) -> Result<AuditPage, Error> {
        let audit = self.inner.audit.as_ref().ok_or(Error::AccessStore)?;
        audit
            .query(filter, before, limit)
            .await
            .context(QueryAuditSnafu)
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use nyanpasu_config::application::RpcAuditSettings;
    use nyanpasu_tsdb::storage::memory::MemoryDisk;
    use struct_patch::Patch as _;

    use super::{AuditSetup, Error, SettingsAuditRetention};
    use crate::{
        actor_v2::test_support::test_idle_endpoint,
        audit::TsdbAuditStore,
        client::{
            NyanpasuClient,
            test_support::{profile_test_adapters, test_client_args_with_endpoint},
        },
    };
    use nyanpasu_observers::audit::{AuditDropped, AuditFilter, AuditRetention as _, AuditRow};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn audit_disabled_facade_reports_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let args =
            test_client_args_with_endpoint(&dir, test_idle_endpoint(), profile_test_adapters(&dir))
                .await;
        let client = NyanpasuClient::try_new(args).await.unwrap();
        assert!(matches!(
            client
                .query_rpc_audit(AuditFilter::default(), None, 10)
                .await,
            Err(Error::AccessStore)
        ));
        client.request_shutdown();
        client.wait_shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn audit_facade_reads_persisted_rows_and_applies_settings_on_next_tick() {
        let dir = tempfile::tempdir().unwrap();
        let mut args =
            test_client_args_with_endpoint(&dir, test_idle_endpoint(), profile_test_adapters(&dir))
                .await;
        let (sender, rows) = tokio::sync::mpsc::channel(4);
        let store = tokio::task::spawn_blocking(|| {
            TsdbAuditStore::open(MemoryDisk::new().open().unwrap()).unwrap()
        })
        .await
        .unwrap();
        args.audit = Some(AuditSetup {
            store: Box::new(store),
            rows,
            dropped: AuditDropped::default(),
        });
        let client = NyanpasuClient::try_new(args).await.unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let row = AuditRow {
            at: now - 2 * 86400 * 1000,
            transport: "local".into(),
            method: "get_profiles".into(),
            owner: "local:0123456789abcdef".into(),
            invocation_id: 1,
            request_id: Some(2),
            phase: "finished".into(),
            outcome: Some("success".into()),
            kind: None,
            code: None,
            duration_ns: 3,
        };
        sender.try_send(row.clone()).unwrap();
        assert!(
            client
                .query_rpc_audit(AuditFilter::default(), None, 10)
                .await
                .unwrap()
                .rows
                .is_empty()
        );
        let audit = client.inner.audit.as_ref().unwrap();
        audit.tick().await.unwrap();
        assert_eq!(
            client
                .query_rpc_audit(AuditFilter::default(), None, 10)
                .await
                .unwrap()
                .rows
                .as_slice(),
            std::slice::from_ref(&row)
        );

        let mut patch = nyanpasu_config::application::NyanpasuAppConfig::new_empty_patch();
        patch.rpc_audit = Some(RpcAuditSettings {
            max_age_days: 1,
            max_size_mib: 4,
        });
        client.patch_app_config(patch).await.unwrap();
        let retention = SettingsAuditRetention::new(client.inner.application.snapshot_handle());
        assert_eq!(
            retention.retention(),
            (std::time::Duration::from_secs(86400), 4 * 1024 * 1024)
        );
        assert_eq!(
            client
                .query_rpc_audit(AuditFilter::default(), None, 10)
                .await
                .unwrap()
                .rows,
            [row]
        );
        audit.tick().await.unwrap();
        assert!(
            client
                .query_rpc_audit(AuditFilter::default(), None, 10)
                .await
                .unwrap()
                .rows
                .is_empty()
        );
        client.request_shutdown();
        client.wait_shutdown().await;
        assert!(sender.is_closed());
    }
}
