//! Legacy regeneration entries delegate to the lifecycle actor.
use super::{ClientError, NyanpasuClient, Result};
use nyanpasu_config::{application::NyanpasuAppConfig, clash::config::ClashConfig};

/// Legacy-compat regeneration entries live here so the `NyanpasuClient` facade
/// in `mod.rs` stays free of legacy-global reads. Request/reply regeneration is
/// a direct typed method call — no process-global dispatcher.
impl NyanpasuClient {
    /// Legacy-draft snapshot -> typed build inputs for the regeneration bridge.
    // FIXME(actor-migration): legacy-draft-aware input assembly for BC callers.
    // Legacy Config::generate() read Config::{verge,clash}().latest() — including
    // uncommitted drafts. Legacy side-effect writers (feat::patch_clash /
    // patch_verge tun+service paths, CoreManager::change_core) draft first and
    // only reseed typed actors after the mutation commits, so regenerating from
    // typed snapshots would run one step behind (stale ports/secret/core).
    // Convert legacy latest() via the reseed converters instead — without
    // mutating the typed actors, so a later discard() stays a discard.
    // New code must use rebuild_running_config()/regenerate_runtime().
    // Remove when: PR-5/6 migrate the legacy writers onto typed clients.
    fn legacy_regen_inputs() -> Result<(NyanpasuAppConfig, ClashConfig)> {
        // MUST read latest() (draft-inclusive), never data(): legacy writers
        // draft first and expect the regen to see it (see the FIXME above).
        let legacy_verge = crate::config::Config::verge().latest().clone();
        let legacy_clash = crate::config::Config::clash().latest().0.clone();
        Self::legacy_regen_inputs_from(&legacy_verge, &legacy_clash)
    }

    /// Pure conversion half of [`Self::legacy_regen_inputs`], directly testable
    /// without touching the process-global legacy config singletons.
    fn legacy_regen_inputs_from(
        legacy_verge: &crate::config::IVerge,
        legacy_clash: &serde_yaml::Mapping,
    ) -> Result<(NyanpasuAppConfig, ClashConfig)> {
        let (app, _session, clash) =
            crate::bridge::typed_config_from_legacy_parts(legacy_verge, legacy_clash)
                .map_err(ClientError::Anyhow)?;
        Ok((app, clash))
    }

    /// Regeneration entry for legacy bridge callers (`CoreManager::update_config`,
    /// `feat::patch_clash`/`patch_verge` side-effect paths, `change_core`).
    /// Profiles come from the typed actor only; their legacy IPC writers moved
    /// onto the facade in T08 and the legacy profile code was removed in T10.
    #[allow(dead_code)]
    pub(crate) async fn regenerate_runtime_for_legacy(&self) -> Result<()> {
        self.reconcile_core()
            .await
            .map(|_| ())
            .map_err(super::client_error_from_core)
    }

    pub(crate) async fn regenerate_and_apply_for_legacy(&self) -> Result<()> {
        self.reconcile_core()
            .await
            .map(|_| ())
            .map_err(super::client_error_from_core)
    }

    pub(crate) async fn regenerate_and_restart_for_legacy(&self) -> Result<()> {
        self.reconcile_core()
            .await
            .map(|_| ())
            .map_err(super::client_error_from_core)
    }

    /// Commit the selected core before reconciling it through the control plane.
    pub async fn change_core(&self, new_core: crate::config::nyanpasu::ClashCore) -> Result<()> {
        let core = match new_core {
            crate::config::nyanpasu::ClashCore::ClashPremium => {
                nyanpasu_config::application::ClashCore::ClashPremium
            }
            crate::config::nyanpasu::ClashCore::ClashRs => {
                nyanpasu_config::application::ClashCore::ClashRs
            }
            crate::config::nyanpasu::ClashCore::Mihomo => {
                nyanpasu_config::application::ClashCore::Mihomo
            }
            crate::config::nyanpasu::ClashCore::MihomoAlpha => {
                nyanpasu_config::application::ClashCore::MihomoAlpha
            }
            crate::config::nyanpasu::ClashCore::ClashRsAlpha => {
                nyanpasu_config::application::ClashCore::ClashRsAlpha
            }
            crate::config::nyanpasu::ClashCore::Meow => {
                nyanpasu_config::application::ClashCore::Meow
            }
        };
        self.update_core(core)
            .await
            .map(|_| ())
            .map_err(super::client_error_from_core)
    }

    /// Boot fallback (spec §5.6, D8): the default config is ALSO routed through
    /// candidate -> check -> promote — D5 has no exceptions. A failed check
    /// leaves no product; boot continues and the core start fails visibly.
    pub(crate) async fn promote_default_runtime_config(&self) -> Result<()> {
        self.reconcile_core()
            .await
            .map(|_| ())
            .map_err(super::client_error_from_core)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// T07 review fix regression pin: the regeneration bridge assembles its
    /// inputs from legacy verge/clash values as the writers drafted them
    /// (feat::patch_clash / change_core draft first, reseed typed actors only
    /// after commit). Tests the pure conversion half — the production wrapper
    /// reads Config::{verge,clash}().latest() and must stay draft-inclusive
    /// (mutating the process-global singletons here is inherently racy, so the
    /// wrapper's latest() choice is locked by comment + review, not by test).
    #[test]
    fn legacy_regen_inputs_conversion_reflects_drafted_fields() {
        let verge = crate::config::IVerge {
            clash_core: Some(crate::config::nyanpasu::ClashCore::ClashRs),
            verge_mixed_port: Some(49301),
            ..crate::config::IVerge::default()
        };
        let template = crate::config::IClashTemp::template().0;

        let (app, clash) = NyanpasuClient::legacy_regen_inputs_from(&verge, &template)
            .expect("legacy regen inputs should assemble");
        assert_eq!(
            app.core,
            nyanpasu_config::application::ClashCore::ClashRs,
            "drafted clash_core must reach the app input"
        );
        assert_eq!(
            clash.mixed_port.start_port, 49301,
            "drafted mixed-port must reach the clash input"
        );
    }

    #[test]
    fn change_core_commits_and_reconciles_through_the_control_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            client
                .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                .await
                .unwrap();
            assert_eq!(
                client.get_app_config().await.unwrap().core,
                nyanpasu_config::application::ClashCore::ClashRs
            );
        });
        assert_eq!(endpoint.submissions(), 1);
    }

    #[test]
    fn change_core_failure_keeps_the_committed_selection() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::failing();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            assert!(
                client
                    .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                    .await
                    .is_err()
            );
            assert_eq!(
                client.get_app_config().await.unwrap().core,
                nyanpasu_config::application::ClashCore::ClashRs
            );
        });
    }

    #[test]
    fn concurrent_reconciles_publish_monotonic_runtime_revisions() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            let (left, right) = tokio::join!(client.reconcile_core(), client.reconcile_core());
            left.unwrap();
            right.unwrap();
            assert_eq!(endpoint.submissions(), 2);
            assert_eq!(client.promoted_runtime().await.unwrap().revision.get(), 2);
        });
    }

    #[test]
    fn change_core_publishes_a_product_for_the_selected_core() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            client
                .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                .await
                .unwrap();
            assert_eq!(
                client.promoted_runtime().await.unwrap().target_core,
                nyanpasu_config::application::ClashCore::ClashRs
            );
        });
    }

    #[test]
    fn repeated_core_updates_advance_the_runtime_product() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            client
                .update_core(nyanpasu_config::application::ClashCore::ClashRs)
                .await
                .unwrap();
            let first = client.promoted_runtime().await.unwrap();
            client
                .update_core(nyanpasu_config::application::ClashCore::Mihomo)
                .await
                .unwrap();
            let second = client.promoted_runtime().await.unwrap();
            assert!(second.revision > first.revision);
        });
    }

    #[test]
    fn promote_default_runtime_config_routes_through_reconcile() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(client.promote_default_runtime_config()).unwrap();
        assert_eq!(endpoint.submissions(), 1);
    }

    #[test]
    fn s09_two_client_graphs_are_independent() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();
        let endpoint_a = crate::client::tests::TestControlEndpoint::succeeding();
        let endpoint_b = crate::client::tests::TestControlEndpoint::succeeding();
        let client_a = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir_a, endpoint_a.clone()),
        )
        .unwrap();
        let client_b = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir_b, endpoint_b.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            client_a.reconcile_core().await.unwrap();
            client_b.reconcile_core().await.unwrap();
            client_b.reconcile_core().await.unwrap();
        });
        assert_eq!(endpoint_a.submissions(), 1);
        assert_eq!(endpoint_b.submissions(), 2);
    }

    #[test]
    fn s09_clones_share_one_coordinator() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint),
        )
        .unwrap();
        let clone = client.clone();
        assert!(std::ptr::eq(
            &client.inner.lifecycle,
            &clone.inner.lifecycle
        ));
    }

    #[test]
    fn s09_legacy_call_sites_use_supplied_client() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(client.regenerate_runtime_for_legacy()).unwrap();
        assert_eq!(endpoint.submissions(), 1);
    }

    #[test]
    fn legacy_apply_and_restart_entries_both_reconcile() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = crate::client::tests::TestControlEndpoint::succeeding();
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_client_args_with_endpoint(&dir, endpoint.clone()),
        )
        .unwrap();

        tauri::async_runtime::block_on(async {
            client.regenerate_and_apply_for_legacy().await.unwrap();
            client.regenerate_and_restart_for_legacy().await.unwrap();
        });
        assert_eq!(endpoint.submissions(), 2);
    }
}
