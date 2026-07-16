//! Rebuild plumbing (PR-3 T07): the actor-side fire-and-forget notifier, the
//! receiver-side debounced listener (design §6.4: debouncing is the receiver's
//! concern), and the legacy regeneration bridge.

use std::{future::Future, sync::Arc};

use nyanpasu_config::{application::NyanpasuAppConfig, clash::config::ClashConfig};
use once_cell::sync::OnceCell;
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot};

use super::{ClientError, NyanpasuClient, Result};
use crate::state::profiles::ports::RebuildNotifier;

pub struct ChannelRebuildNotifier(mpsc::UnboundedSender<()>);

impl ChannelRebuildNotifier {
    pub fn new(sender: mpsc::UnboundedSender<()>) -> Self {
        Self(sender)
    }
}

impl RebuildNotifier for ChannelRebuildNotifier {
    fn request_rebuild(&self) {
        let _ = self.0.send(());
    }
}

const COALESCE_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

pub(super) fn spawn_listener_with<F, Fut>(mut rx: mpsc::UnboundedReceiver<()>, rebuild: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tauri::async_runtime::spawn(async move {
        while rx.recv().await.is_some() {
            tokio::time::sleep(COALESCE_WINDOW).await;
            while rx.try_recv().is_ok() {}
            if let Err(error) = rebuild().await {
                tracing::warn!(%error, "background-driven rebuild failed (degraded)");
            }
        }
    });
}

// FIXME(actor-migration): process-level regeneration bridge for legacy core/verge
// flows (CoreManager::update_config / core switch / tun patch paths) that cannot
// receive the client by injection yet. New code must use
// NyanpasuClient::rebuild_running_config() / regenerate_runtime().
// Remove after PR-5/PR-6 migrate those flows onto injected clients.
// Known limitation (accepted): installation is first-install-wins — a second
// client constructed in the same process (tests) keeps the FIRST client's
// handler, including its paths. Production installs exactly once at setup.
pub(super) enum RegenKind {
    /// 仅重建(build→check→晋升→发布)。
    Regenerate,
    /// 重建 + put_configs,gate 单次持有内完成(P0-2:消灭「gate 内 regen、
    /// gate 外 apply」的产物覆盖窗口)。
    RegenerateAndApply,
    /// 重建 + 重启核心,gate 单次持有内完成(P0-2)。
    RegenerateAndRestart,
}
struct RegenRequest {
    kind: RegenKind,
    reply: oneshot::Sender<anyhow::Result<()>>,
}
static REGEN_BRIDGE: OnceCell<mpsc::UnboundedSender<RegenRequest>> = OnceCell::new();

pub(super) fn install_regen_bridge<F, Fut>(handler: F)
where
    F: Fn(RegenKind) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    if !install_regen_bridge_inner(handler) {
        tracing::warn!("regeneration bridge already installed; keeping the first");
    }
}

/// Returns `true` when this call became the process-level handler.
/// Test-only observation of the first-install-wins OnceCell (S01/S09).
#[cfg(test)]
fn try_install_regen_bridge<F, Fut>(handler: F) -> bool
where
    F: Fn(RegenKind) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    install_regen_bridge_inner(handler)
}

fn install_regen_bridge_inner<F, Fut>(handler: F) -> bool
where
    F: Fn(RegenKind) -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<RegenRequest>();
    if REGEN_BRIDGE.set(tx).is_err() {
        return false;
    }
    tauri::async_runtime::spawn(async move {
        while let Some(request) = rx.recv().await {
            let result = handler(request.kind).await;
            let _ = request.reply.send(result);
        }
    });
    true
}

/// Sequenced regeneration for legacy callers: awaits the facade rebuild of the
/// runtime draft (no core apply) before returning, preserving the legacy
/// `Config::generate().await?` ordering guarantees.
pub async fn regenerate() -> anyhow::Result<()> {
    dispatch(RegenKind::Regenerate).await
}

pub async fn regenerate_and_apply() -> anyhow::Result<()> {
    dispatch(RegenKind::RegenerateAndApply).await
}

pub async fn regenerate_and_restart() -> anyhow::Result<()> {
    dispatch(RegenKind::RegenerateAndRestart).await
}

async fn dispatch(kind: RegenKind) -> anyhow::Result<()> {
    let bridge = REGEN_BRIDGE
        .get()
        .ok_or_else(|| anyhow::anyhow!("regeneration bridge not installed"))?;
    let (tx, rx) = oneshot::channel();
    bridge
        .send(RegenRequest { kind, reply: tx })
        .map_err(|_| anyhow::anyhow!("regeneration bridge is gone"))?;
    rx.await
        .map_err(|_| anyhow::anyhow!("regeneration handler dropped the reply"))?
}

/// Legacy-compat regeneration entries live here with the rest of the legacy
/// bridge so the `NyanpasuClient` facade in `mod.rs` stays free of
/// legacy-global reads.
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
    pub(crate) async fn regenerate_runtime_for_legacy(&self) -> Result<()> {
        // Inputs are read under the rebuild gate so a legacy regeneration
        // serializes with facade rebuilds and always sees the newest drafts.
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        self.regenerate_for_legacy_inner(&mut *lease)
            .await
            .map(|_| ())
    }

    async fn regenerate_for_legacy_inner(
        &self,
        lease: &mut dyn crate::client::CoreLifecycleLease,
    ) -> Result<std::sync::Arc<crate::client::runtime::RuntimeSnapshot>> {
        let revision = self
            .inner
            .runtime_revisions
            .allocate()
            .map_err(ClientError::Anyhow)?;
        let (app, clash) = Self::legacy_regen_inputs()?;
        let profiles = self.inner.profiles.get().await?;
        self.regenerate_runtime_with(lease, revision, profiles, clash, app)
            .await
    }

    pub(crate) async fn regenerate_and_apply_for_legacy(&self) -> Result<()> {
        // P0-2: one gate hold covers regenerate AND apply — a concurrent rebuild
        // cannot replace the product between the two steps.
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let promoted = self.regenerate_for_legacy_inner(&mut *lease).await?;
        lease
            .apply_promoted(self.inner.runtime_paths.product())
            .await
            .map_err(ClientError::Anyhow)?;
        self.publish_applied(promoted).await
    }

    pub(crate) async fn regenerate_and_restart_for_legacy(&self) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let promoted = self.regenerate_for_legacy_inner(&mut *lease).await?;
        lease.restart().await.map_err(ClientError::Anyhow)?;
        self.publish_applied(promoted).await
    }

    /// Core-switch transaction (spec §5.4 / S03). The WHOLE
    /// draft→rebuild→restart→commit/rollback sequence holds the rebuild gate,
    /// so no concurrent rebuild can replace the checked product between check
    /// and start (P0-2). Rollback restores product, Promoted, Applied, and the
    /// selected core according to the captured transaction snapshot.
    pub async fn change_core(&self, new_core: crate::config::nyanpasu::ClashCore) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;

        // Capture rollback material BEFORE any mutation (spec §6.5 D5).
        let product_path = self.inner.runtime_paths.product().to_owned();
        let product = match tokio::fs::read(&product_path).await {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(ClientError::Anyhow(error.into())),
        };
        let (selected_app, _) = Self::legacy_regen_inputs()?;
        let transaction = crate::client::runtime::RuntimeTransactionSnapshot {
            product,
            lifecycle: self.runtime_lifecycle_state().await,
            selected_core: selected_app.core,
        };

        // TODO(actor-migration): core selection still drafts the legacy verge.
        // Reason: verge feature flows migrate in PR-5/6.
        // Remove when: core selection patches the typed app config.
        crate::config::Config::verge().draft().clash_core = Some(new_core);

        let new_snapshot = match self.regenerate_for_legacy_inner(&mut *lease).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                // New-core build/check failed: product/promoted/applied untouched.
                crate::config::Config::verge().discard();
                return Err(error);
            }
        };

        // TODO(actor-migration): legacy log sink clear on core switch (C7).
        // Remove when: PR-5 injects the LogSink into CoreActor.
        crate::core::logger::Logger::global().clear_log();

        match lease.restart().await {
            Ok(()) => {
                crate::config::Config::verge().apply();
                if let Err(error) = crate::config::Config::verge().latest().save_file() {
                    tracing::error!(%error, "failed to persist verge after core switch");
                }
                // Successful apply/restart advances Applied (promoted already set).
                self.publish_applied(new_snapshot).await
            }
            Err(new_core_error) => {
                tracing::error!("failed to change core: {new_core_error:?}");
                // 1. Restore old selected-core desired value before rebuild.
                crate::config::Config::verge().discard();

                // 2. Rebuild old-core runtime from committed desired state.
                match self.regenerate_for_legacy_inner(&mut *lease).await {
                    Ok(rollback_snapshot) => {
                        // 5. Start old core off the rebuilt product.
                        if let Err(restart_error) = lease.restart().await {
                            // Promoted is the rollback rebuild; Applied stays the
                            // pre-transaction snapshot (never advanced on failure).
                            return Err(ClientError::Anyhow(new_core_error.context(format!(
                                "old core restart failed after rollback: {restart_error}"
                            ))));
                        }
                        // 6. Old core started → Applied tracks the rollback snapshot.
                        self.publish_applied(rollback_snapshot).await?;
                        Err(ClientError::Anyhow(new_core_error))
                    }
                    Err(rebuild_error) => {
                        // 3. Rebuild failed → atomically restore previous product.
                        let restored: anyhow::Result<()> = match &transaction.product {
                            Some(bytes) => {
                                crate::client::core_bridge::restore_product(
                                    product_path.as_std_path(),
                                    bytes,
                                )
                                .await
                            }
                            None => match tokio::fs::remove_file(&product_path).await {
                                Ok(()) => Ok(()),
                                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                    Ok(())
                                }
                                Err(error) => Err(error.into()),
                            },
                        };
                        if let Err(restore_error) = restored {
                            return Err(ClientError::Anyhow(
                                new_core_error
                                    .context(format!("rollback rebuild failed: {rebuild_error}"))
                                    .context(format!(
                                        "product restore failed: {restore_error}; core left stopped"
                                    )),
                            ));
                        }
                        // 4. Product is authoritative → restore Promoted with it.
                        // New-core restart never advanced Applied, but pre-transaction
                        // Promoted may still be ahead of Applied (promote-ok / apply-fail).
                        self.restore_promoted(transaction.lifecycle.promoted.clone())
                            .await?;
                        // 5. Start old core on the restored product.
                        if let Err(restart_error) = lease.restart().await {
                            // 7. Applied keeps the pre-transaction snapshot; core is
                            // stopped/degraded. Structured error chain is preserved.
                            return Err(ClientError::Anyhow(
                                new_core_error
                                    .context(format!("rollback rebuild failed: {rebuild_error}"))
                                    .context(format!("old core restart failed: {restart_error}")),
                            ));
                        }
                        // 6. Successful old restart runs the restored product, so
                        // Applied must track the restored Promoted (including the
                        // Promoted > Applied case). Skip when Promoted was unknown.
                        if let Some(restored) = transaction.lifecycle.promoted.clone() {
                            self.publish_applied(restored).await?;
                        }
                        // selected_core was restored via discard() before rebuild
                        // (transaction.selected_core is the captured baseline).
                        debug_assert_eq!(
                            Self::legacy_regen_inputs().map(|(app, _)| app.core).ok(),
                            Some(transaction.selected_core),
                            "selected core must match the pre-transaction snapshot after discard"
                        );
                        Err(ClientError::Anyhow(new_core_error.context(format!(
                            "rollback rebuild failed: {rebuild_error}; restored previous product"
                        ))))
                    }
                }
            }
        }
    }

    /// Boot fallback (spec §5.6, D8): the default config is ALSO routed through
    /// candidate -> check -> promote — D5 has no exceptions. A failed check
    /// leaves no product; boot continues and the core start fails visibly.
    pub(crate) async fn promote_default_runtime_config(&self) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let revision = self
            .inner
            .runtime_revisions
            .allocate()
            .map_err(ClientError::Anyhow)?;
        // TODO(actor-migration): boot fallback reads the legacy clash mapping
        // directly (same source the old resolve.rs fallback used).
        // Remove when: PR-6 migrates boot/resolve onto typed clients.
        let mapping = crate::config::Config::clash().latest().0.clone();
        let (app, _clash) = Self::legacy_regen_inputs()?;
        let yaml = format!(
            "# Clash Nyanpasu Runtime (default fallback)\n\n{}",
            serde_yaml::to_string(&mapping)
                .map_err(|error| ClientError::Custom(format!("serialize default: {error}")))?
        );
        let product_sha256: [u8; 32] = Sha256::digest(yaml.as_bytes()).into();
        let snapshot = Arc::new(crate::client::runtime::RuntimeSnapshot::from_data(
            revision,
            app.core,
            product_sha256,
            crate::client::runtime::RuntimeSnapshotData {
                exists_keys: mapping
                    .keys()
                    .filter_map(serde_yaml::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect(),
                config: mapping,
                postprocessing_output: Default::default(),
            },
        ));
        let candidate = self
            .inner
            .runtime_paths
            .create_candidate(yaml.as_bytes())
            .await
            .map_err(ClientError::Anyhow)?;
        let mut lease = self.inner.core.begin().await.map_err(ClientError::Anyhow)?;
        let checked = lease
            .check_and_promote(&candidate, app.core, self.inner.runtime_paths.product())
            .await;
        if let Err(error) = candidate.cleanup().await {
            tracing::warn!(%error, "failed to remove candidate config");
        }
        checked.map_err(ClientError::Anyhow)?;
        self.publish_promoted(snapshot).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use nyanpasu_config::application::ClashCore;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };
    use tokio::sync::oneshot as tokio_oneshot;

    #[tokio::test(flavor = "multi_thread")]
    async fn listener_coalesces_bursts_into_one_rebuild() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        spawn_listener_with(rx, move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        for _ in 0..5 {
            tx.send(()).unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1, "burst must coalesce");
        tx.send(()).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

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
    fn change_core_rolls_back_via_second_regenerate_and_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        // 新核:check+晋升成功 → 启动失败
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("new core boom")));
        // 回滚:旧核 check+晋升成功 → 旧核启动成功
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(
            client.change_core(crate::config::nyanpasu::ClashCore::ClashRs),
        );
        assert!(
            result.is_err(),
            "change_core must surface the new-core error"
        );
    }

    /// P0-4 deepest branch (brief Step 7 ¶2 fallback coverage): when the ROLLBACK
    /// rebuild ALSO fails its check, the failure is never swallowed. change_core
    /// restores the previous checked product bytes, makes one old-core restart
    /// attempt, and surfaces a compound error whose chain records the
    /// rollback-rebuild failure. The mock sequence pins that a second check Err
    /// never reaches the restart-success path (which would apply the verge draft).
    ///
    /// RuntimePaths keeps the product under this test's TempDir, so the fallback
    /// restore is exercised without touching the user's product config.
    #[test]
    fn change_core_rollback_rebuild_failure_restores_product_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        // Seed the injected product so change_core captures old_product before
        // entering the rollback-rebuild failure path.
        let product = crate::client::RuntimePaths::from_resolver(
            &crate::utils::path::PathResolver::with_base_dirs(
                dir.path().into(),
                dir.path().join("data"),
            ),
        )
        .unwrap()
        .product()
        .to_owned();
        if let Some(parent) = product.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&product, b"# nyanpasu-test previous product\n").unwrap();

        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        // 新核:check+晋升成功 → 启动失败
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("new core boom")));
        // 回滚重建:check 失败(P0-4 深分支)→ 恢复旧产物 → 旧核重启一次
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Err(anyhow::anyhow!("rollback check boom")));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(
            client.change_core(crate::config::nyanpasu::ClashCore::ClashRs),
        );

        let err = result.expect_err("rollback-rebuild failure must surface an error");
        let rendered = format!("{err:?}");
        assert!(
            rendered.contains("rollback rebuild failed"),
            "compound error must record the rollback-rebuild failure; got: {rendered}"
        );
    }

    // ── S03/S04 regression contracts and the remaining S09 failure pin ─────

    struct BarrierCore {
        lifecycle: Arc<tokio::sync::Mutex<()>>,
        state: Arc<BarrierState>,
    }

    struct BarrierState {
        begin_calls: AtomicUsize,
        restart_calls: AtomicUsize,
        concurrent_begin_attempted_tx: Mutex<Option<tokio_oneshot::Sender<()>>>,
        entered_before_rollback: AtomicBool,
        first_entered_tx: Mutex<Option<tokio_oneshot::Sender<()>>>,
        release_first_rx: Mutex<Option<tokio_oneshot::Receiver<()>>>,
        rollback_finished: AtomicBool,
        rollback_finished_tx: Mutex<Option<tokio_oneshot::Sender<()>>>,
        concurrent_entered_tx: Mutex<Option<tokio_oneshot::Sender<()>>>,
    }

    struct BarrierLease {
        state: Arc<BarrierState>,
        _guard: tokio::sync::OwnedMutexGuard<()>,
    }

    #[async_trait]
    impl crate::client::CoreLifecyclePort for BarrierCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn crate::client::CoreLifecycleLease>> {
            let begin_call = self.state.begin_calls.fetch_add(1, Ordering::SeqCst);
            if begin_call > 0 {
                if let Some(tx) = self
                    .state
                    .concurrent_begin_attempted_tx
                    .lock()
                    .unwrap()
                    .take()
                {
                    let _ = tx.send(());
                }
            }
            let guard = self.lifecycle.clone().lock_owned().await;
            if begin_call > 0 && !self.state.rollback_finished.load(Ordering::SeqCst) {
                self.state
                    .entered_before_rollback
                    .store(true, Ordering::SeqCst);
            }
            Ok(Box::new(BarrierLease {
                state: self.state.clone(),
                _guard: guard,
            }))
        }

        async fn status(&self) -> anyhow::Result<crate::client::core_bridge::CoreStatusSnapshot> {
            anyhow::bail!("status is not used by this barrier test")
        }

        async fn on_profile_change(&self) {}
    }

    #[async_trait]
    impl crate::client::CoreLifecycleLease for BarrierLease {
        async fn check_and_promote(
            &mut self,
            candidate: &crate::client::runtime::CandidateFile,
            _target_core: ClashCore,
            _product: &camino::Utf8Path,
        ) -> anyhow::Result<[u8; 32]> {
            Ok(candidate.bytes_sha256())
        }

        async fn apply_promoted(&mut self, _product: &camino::Utf8Path) -> anyhow::Result<()> {
            Ok(())
        }

        async fn restart(&mut self) -> anyhow::Result<()> {
            match self.state.restart_calls.fetch_add(1, Ordering::SeqCst) {
                0 => {
                    if let Some(tx) = self.state.first_entered_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    let release = self.state.release_first_rx.lock().unwrap().take();
                    if let Some(release) = release {
                        let _ = release.await;
                    }
                    Err(anyhow::anyhow!("new core boom (barrier)"))
                }
                1 => {
                    self.state.rollback_finished.store(true, Ordering::SeqCst);
                    if let Some(tx) = self.state.rollback_finished_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    Ok(())
                }
                _ => {
                    assert!(
                        self.state.rollback_finished.load(Ordering::SeqCst),
                        "concurrent lifecycle restart entered before rollback completed"
                    );
                    if let Some(tx) = self.state.concurrent_entered_tx.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                    Ok(())
                }
            }
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn s04_concurrent_restart_waits_until_change_core_rollback_completes() {
        let dir = tempfile::tempdir().unwrap();
        let (first_entered_tx, first_entered_rx) = tokio_oneshot::channel();
        let (concurrent_begin_attempted_tx, concurrent_begin_attempted_rx) =
            tokio_oneshot::channel();
        let (release_first_tx, release_first_rx) = tokio_oneshot::channel();
        let (rollback_finished_tx, rollback_finished_rx) = tokio_oneshot::channel();
        let (concurrent_entered_tx, concurrent_entered_rx) = tokio_oneshot::channel();
        let state = Arc::new(BarrierState {
            begin_calls: AtomicUsize::new(0),
            restart_calls: AtomicUsize::new(0),
            concurrent_begin_attempted_tx: Mutex::new(Some(concurrent_begin_attempted_tx)),
            entered_before_rollback: AtomicBool::new(false),
            first_entered_tx: Mutex::new(Some(first_entered_tx)),
            release_first_rx: Mutex::new(Some(release_first_rx)),
            rollback_finished: AtomicBool::new(false),
            rollback_finished_tx: Mutex::new(Some(rollback_finished_tx)),
            concurrent_entered_tx: Mutex::new(Some(concurrent_entered_tx)),
        });
        let core = Arc::new(BarrierCore {
            lifecycle: Arc::new(tokio::sync::Mutex::new(())),
            state: state.clone(),
        });
        let client =
            crate::client::NyanpasuClient::try_new_with_args(crate::client::ClientSetupArgs {
                core: core.clone(),
                ..crate::client::tests::test_profiles_client_args(
                    &dir,
                    Arc::new(crate::client::tests::MockRunningCoreBridge::new()),
                )
            })
            .unwrap();

        tauri::async_runtime::block_on(async move {
            let change = tauri::async_runtime::spawn({
                let client = client.clone();
                async move {
                    client
                        .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                        .await
                }
            });
            first_entered_rx.await.expect("new-core restart must enter");

            let concurrent = tauri::async_runtime::spawn(async move {
                let mut lease = crate::client::CoreLifecyclePort::begin(&*core).await?;
                lease.restart().await
            });
            concurrent_begin_attempted_rx
                .await
                .expect("concurrent restart must attempt lifecycle begin while rollback is active");
            assert!(
                !state.entered_before_rollback.load(Ordering::SeqCst),
                "concurrent lifecycle begin must wait for rollback"
            );
            let _ = release_first_tx.send(());

            rollback_finished_rx
                .await
                .expect("old-core rollback restart must complete");
            concurrent_entered_rx
                .await
                .expect("concurrent restart must enter after rollback");
            concurrent
                .await
                .expect("concurrent restart task must join")
                .expect("concurrent restart must succeed");
            assert!(
                !state.entered_before_rollback.load(Ordering::SeqCst),
                "concurrent lifecycle begin entered before rollback completed"
            );
            assert!(
                change.await.expect("change_core task must join").is_err(),
                "change_core must surface the new-core failure"
            );
        });
    }

    /// S01→S03 contract (task §S01.2 / design failure matrix change_core row):
    /// when rollback rebuild fails and product bytes are restored, the runtime
    /// read model (Promoted) must also be restored to the pre-transaction
    /// snapshot. Product restore + Promoted restore are atomic (S03).
    #[test]
    fn s01_contract_product_restore_leaves_runtime_read_model_on_new_core() {
        let dir = tempfile::tempdir().unwrap();
        let product = crate::client::RuntimePaths::from_resolver(
            &crate::utils::path::PathResolver::with_base_dirs(
                dir.path().into(),
                dir.path().join("data"),
            ),
        )
        .unwrap()
        .product()
        .to_owned();
        if let Some(parent) = product.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Distinct old product bytes so we can detect the restore.
        const OLD_PRODUCT: &[u8] = b"# s01-old-product\nmode: rule\n";
        std::fs::write(&product, OLD_PRODUCT).unwrap();

        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        // New core: promote ok → restart fails.
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("new core boom")));
        // Rollback rebuild: check fails → product restore path → old restart.
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Err(anyhow::anyhow!("rollback check boom")));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();

        // No prior Promoted/Applied before change_core — product restore must
        // clear the new-core publish left by regenerate_for_legacy_inner.
        let pre = tauri::async_runtime::block_on(client.runtime_lifecycle_state());
        assert!(pre.promoted.is_none());
        assert!(pre.applied.is_none());

        let result = tauri::async_runtime::block_on(
            client.change_core(crate::config::nyanpasu::ClashCore::ClashRs),
        );
        assert!(result.is_err(), "change_core must surface compound error");

        // Product bytes restored to OLD_PRODUCT by the last-resort path.
        let restored_bytes = std::fs::read(&product).unwrap_or_default();
        assert_eq!(
            restored_bytes, OLD_PRODUCT,
            "product restore must rewrite the injected product path"
        );

        // S03: after product restore, Promoted is the pre-transaction snapshot
        // (None here) — never the new-core publish.
        let lifecycle = tauri::async_runtime::block_on(client.runtime_lifecycle_state());
        assert!(
            lifecycle.promoted.is_none(),
            "product restore after rollback-rebuild failure must restore Promoted; \
             got Some(revision={:?}, core={:?})",
            lifecycle.promoted.as_ref().map(|s| s.revision.get()),
            lifecycle.promoted.as_ref().map(|s| s.target_core),
        );
        assert!(
            lifecycle.applied.is_none(),
            "Applied must stay pre-transaction (None) when new-core restart never applied"
        );
        assert!(
            tauri::async_runtime::block_on(client.promoted_runtime()).is_none(),
            "runtime read model (Promoted) must not remain on the new-core publish"
        );
    }

    /// H-1: deep product-restore with pre-transaction Promoted > Applied.
    /// Setup: product bytes = P2; Promoted = P2; Applied = P1 (stale after a
    /// prior promote-ok/apply-fail). change_core fails new-core restart, then
    /// rollback rebuild also fails check → product restore + Promoted restore
    /// + successful old-core restart. Applied must advance to the restored P2.
    #[test]
    fn change_core_product_restore_advances_applied_when_promoted_ahead() {
        use sha2::{Digest, Sha256};

        let dir = tempfile::tempdir().unwrap();
        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        // Seed Applied=P1: promote existing P1 product, then start core.
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        // Seed Promoted=P2 without advancing Applied (promote-ok / apply-fail shape).
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        // change_core: new-core promote ok → restart fails.
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("new core boom")));
        // Rollback rebuild check fails → product restore path → old restart ok.
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Err(anyhow::anyhow!("rollback check boom")));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();

        let product = client.runtime_product_path().to_owned();
        if let Some(parent) = product.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        const P1_BYTES: &[u8] = b"# h1-p1-product\nmode: direct\n";
        const P2_BYTES: &[u8] = b"# h1-p2-product\nmode: rule\n";

        tauri::async_runtime::block_on(async {
            std::fs::write(&product, P1_BYTES).unwrap();
            let p1 = client
                .promote_existing_runtime_product()
                .await
                .expect("seed Promoted=P1");
            client
                .start_promoted_runtime()
                .await
                .expect("seed Applied=P1");

            std::fs::write(&product, P2_BYTES).unwrap();
            let p2 = client
                .promote_existing_runtime_product()
                .await
                .expect("seed Promoted=P2 ahead of Applied");
            assert_eq!(
                p2.product_sha256,
                <[u8; 32]>::from(Sha256::digest(P2_BYTES)),
                "seed product must be P2"
            );

            let before = client.runtime_lifecycle_state().await;
            let before_promoted = before.promoted.expect("Promoted=P2");
            let before_applied = before.applied.expect("Applied=P1");
            assert!(
                before_promoted.identity_eq(p2.as_ref()),
                "pre-transaction Promoted must be P2"
            );
            assert!(
                before_applied.identity_eq(p1.as_ref()),
                "pre-transaction Applied must be P1"
            );
            assert!(
                before_promoted.revision > before_applied.revision,
                "precondition: Promoted revision must be ahead of Applied"
            );

            let result = client
                .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                .await;
            assert!(result.is_err(), "change_core must surface compound error");

            let restored_bytes = tokio::fs::read(&product).await.unwrap_or_default();
            assert_eq!(
                restored_bytes, P2_BYTES,
                "deep product restore must rewrite injected product to P2"
            );

            let after = client.runtime_lifecycle_state().await;
            let after_promoted = after.promoted.expect("restored Promoted=P2");
            let after_applied = after
                .applied
                .expect("successful old restart must advance Applied to restored Promoted");
            assert!(
                after_promoted.identity_eq(p2.as_ref()),
                "Promoted must restore to pre-transaction P2"
            );
            assert!(
                after_applied.identity_eq(after_promoted.as_ref()),
                "Applied must advance to restored Promoted after successful old-core restart"
            );
            assert!(
                after_applied.identity_eq(p2.as_ref()),
                "Applied must equal restored P2, not stale P1"
            );
        });
    }

    /// Successful rollback rebuild restores Applied to the rebuilt old-core
    /// snapshot; product/promoted/applied stay coherent after the error.
    #[test]
    fn change_core_successful_rollback_publishes_applied_from_old_core() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("new core boom")));
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(
            client.change_core(crate::config::nyanpasu::ClashCore::ClashRs),
        );
        assert!(result.is_err());

        let lifecycle = tauri::async_runtime::block_on(client.runtime_lifecycle_state());
        let promoted = lifecycle.promoted.expect("rollback must leave Promoted");
        let applied = lifecycle
            .applied
            .expect("successful old restart publishes Applied");
        assert!(
            promoted.identity_eq(&applied),
            "Applied must match Promoted after successful rollback restart"
        );
        // Discard restored selected core to default (not ClashRs).
        assert_ne!(
            promoted.target_core,
            nyanpasu_config::application::ClashCore::ClashRs,
            "rollback rebuild must target the restored old selected core"
        );
    }

    /// Default fallback publishes Promoted only — Applied stays unset until a
    /// successful apply/start/restart (boot path uses start_promoted_runtime).
    #[test]
    fn promote_default_runtime_config_publishes_promoted_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = crate::client::tests::MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .times(1)
            .returning(|_, _| Ok(()));
        core.expect_on_profile_change().returning(|| ());
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();

        tauri::async_runtime::block_on(client.promote_default_runtime_config())
            .expect("default fallback promote");

        let lifecycle = tauri::async_runtime::block_on(client.runtime_lifecycle_state());
        assert!(
            lifecycle.promoted.is_some(),
            "fallback must publish Promoted"
        );
        assert!(
            lifecycle.applied.is_none(),
            "fallback must not advance Applied before core start"
        );
    }

    /// S01 contract (task §S01.9 / design goal #10): a second client graph
    /// constructed in the same process must install its own regeneration
    /// handler. Desired (S09): per-graph dispatcher, no process-global
    /// OnceCell first-install-wins.
    ///
    /// Current failure reason: `REGEN_BRIDGE` is a process-level `OnceCell`;
    /// `install_regen_bridge` keeps the first sender and silently drops the
    /// second (see FIXME above the static).
    #[test]
    fn s01_contract_second_client_graph_reuses_first_regen_handler() {
        // Ensure a first handler exists (no-op if an earlier test already installed).
        let first_installed = try_install_regen_bridge(|_kind| async { Ok(()) });
        // A second graph must be able to install its own handler under S09.
        // Current OnceCell first-install-wins always rejects the second install.
        let second_installed = try_install_regen_bridge(|_kind| async { Ok(()) });
        assert!(
            second_installed,
            "S01 FAILURE reason: second client graph reuses first REGEN_BRIDGE handler \
             (OnceCell first-install-wins; first_installed={first_installed}, \
             second_installed={second_installed}; install #2 is dropped and the first \
             handler remains process-global)"
        );
    }
}
