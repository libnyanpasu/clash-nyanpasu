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
        self.regenerate_for_legacy_inner().await.map(|_| ())
    }

    async fn regenerate_for_legacy_inner(
        &self,
    ) -> Result<std::sync::Arc<crate::client::runtime::RuntimeSnapshot>> {
        let revision = self
            .inner
            .runtime_revisions
            .allocate()
            .map_err(ClientError::Anyhow)?;
        let (app, clash) = Self::legacy_regen_inputs()?;
        let profiles = self.inner.profiles.get().await?;
        self.regenerate_runtime_with(revision, profiles, clash, app)
            .await
    }

    pub(crate) async fn regenerate_and_apply_for_legacy(&self) -> Result<()> {
        // P0-2: one gate hold covers regenerate AND apply — a concurrent rebuild
        // cannot replace the product between the two steps.
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let promoted = self.regenerate_for_legacy_inner().await?;
        self.inner
            .core
            .apply_config()
            .await
            .map_err(ClientError::Anyhow)?;
        self.publish_applied(promoted).await
    }

    pub(crate) async fn regenerate_and_restart_for_legacy(&self) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        let promoted = self.regenerate_for_legacy_inner().await?;
        self.inner
            .core
            .restart_core()
            .await
            .map_err(ClientError::Anyhow)?;
        self.publish_applied(promoted).await
    }

    /// Core-switch transaction (spec §5.4 / S03). The WHOLE
    /// draft→rebuild→restart→commit/rollback sequence holds the rebuild gate,
    /// so no concurrent rebuild can replace the checked product between check
    /// and start (P0-2). Rollback restores product, Promoted, Applied, and the
    /// selected core according to the captured transaction snapshot.
    pub async fn change_core(&self, new_core: crate::config::nyanpasu::ClashCore) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;

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

        let new_snapshot = match self.regenerate_for_legacy_inner().await {
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

        match self.inner.core.restart_core().await {
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
                match self.regenerate_for_legacy_inner().await {
                    Ok(rollback_snapshot) => {
                        // 5. Start old core off the rebuilt product.
                        if let Err(restart_error) = self.inner.core.restart_core().await {
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
                        if let Err(restart_error) = self.inner.core.restart_core().await {
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
        let checked = self
            .inner
            .core
            .check_and_promote(&candidate, app.core)
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
        let mut core = crate::client::core_bridge::MockRunningCoreBridge::new();
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

        let mut core = crate::client::core_bridge::MockRunningCoreBridge::new();
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

    // ── S01 failure contracts (task §S01.1 / §S01.2 / §S01.9) ──────────────
    // These pin CURRENT defective behaviour with explicit failure reasons so
    // later S03/S04/S09 fixes turn them green without rewriting the assertion
    // intent. Production code is intentionally untouched.

    /// Barrier-gated core double used by the concurrent-restart contract.
    /// First `restart_core` (new-core attempt) parks until the concurrent
    /// restart has been observed; subsequent restarts complete immediately.
    struct BarrierCore {
        entered_restart: Arc<AtomicUsize>,
        /// Set by the test once the concurrent restart has entered.
        concurrent_entered: Arc<AtomicBool>,
        /// Signalled by the first restart after it has marked itself entered,
        /// so the test can spawn the concurrent call.
        first_entered_tx: Mutex<Option<tokio_oneshot::Sender<()>>>,
        /// Released by the concurrent call (or test) so the first restart can
        /// finish and let change_core continue into rollback.
        release_first_rx: Mutex<Option<tokio_oneshot::Receiver<()>>>,
        check_calls: AtomicUsize,
    }

    #[async_trait]
    impl crate::client::RunningCoreBridge for BarrierCore {
        async fn check_and_promote(
            &self,
            _candidate: &crate::client::runtime::CandidateFile,
            _target_core: ClashCore,
        ) -> anyhow::Result<()> {
            // First check = new-core promote (ok); second = rollback rebuild (ok).
            let n = self.check_calls.fetch_add(1, Ordering::SeqCst);
            if n <= 1 { Ok(()) } else { Ok(()) }
        }

        async fn apply_config(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn restart_core(&self) -> anyhow::Result<()> {
            let n = self.entered_restart.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                // New-core restart: announce entry, then wait for the concurrent
                // restart to prove it can enter while we hold the rebuild gate.
                if let Some(tx) = self.first_entered_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                // Wait until the concurrent restart has been observed, or the
                // release channel is dropped (test teardown).
                let first_rx = self.release_first_rx.lock().unwrap().take();
                if let Some(rx) = first_rx {
                    let _ = rx.await;
                }
                // Still fail the new core so change_core enters rollback.
                return Err(anyhow::anyhow!("new core boom (barrier)"));
            }
            // Concurrent / rollback restarts: mark concurrent entry and succeed.
            self.concurrent_entered.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn on_profile_change(&self) {}
    }

    /// S01 contract (task §S01.1 / design §6.6): while `change_core` is parked
    /// inside the new-core restart (still holding `rebuild_gate` only — not a
    /// full core lifecycle lease), a concurrent restart must NOT be able to
    /// enter the core restart path. Desired (S04): shared CoreLifecycleLease
    /// blocks concurrent restart until rollback finishes.
    ///
    /// Current failure reason: `RunningCoreBridge::restart_core` is not under
    /// a shared lifecycle lease; only the facade `rebuild_gate` serialises
    /// regenerate paths that go through the same client. A direct concurrent
    /// restart on the core bridge (as tray/service paths can do) can enter
    /// during the rollback window.
    ///
    /// Plain `#[test]` (not `#[tokio::test]`): `try_new_with_args` itself
    /// `block_on`s setup, so nesting inside a tokio runtime panics.
    #[test]
    fn s01_contract_change_core_rollback_window_allows_concurrent_restart() {
        let dir = tempfile::tempdir().unwrap();
        let (first_entered_tx, first_entered_rx) = tokio_oneshot::channel();
        let (release_first_tx, release_first_rx) = tokio_oneshot::channel();
        let concurrent_entered = Arc::new(AtomicBool::new(false));
        let entered_restart = Arc::new(AtomicUsize::new(0));

        let core = Arc::new(BarrierCore {
            entered_restart: entered_restart.clone(),
            concurrent_entered: concurrent_entered.clone(),
            first_entered_tx: Mutex::new(Some(first_entered_tx)),
            release_first_rx: Mutex::new(Some(release_first_rx)),
            check_calls: AtomicUsize::new(0),
        });
        // Keep a second Arc so the concurrent task can call restart without
        // going through the client (simulates tray/service lifecycle entry
        // that does not share the facade rebuild_gate).
        let core_for_concurrent: Arc<dyn crate::client::RunningCoreBridge> = core.clone();

        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, core),
        )
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

            // Wait until change_core is parked in the new-core restart.
            first_entered_rx
                .await
                .expect("first restart must signal entry");

            // Concurrent restart while change_core still holds only rebuild_gate.
            let concurrent =
                tauri::async_runtime::spawn(
                    async move { core_for_concurrent.restart_core().await },
                );
            concurrent
                .await
                .expect("concurrent task join")
                .expect("concurrent restart currently completes");

            // Release the parked new-core restart so change_core can finish rollback.
            let _ = release_first_tx.send(());
            let change_result = change.await.expect("change_core task join");
            assert!(
                change_result.is_err(),
                "change_core must still surface the new-core failure"
            );

            // Desired (S04): concurrent restart is blocked until rollback ends.
            // Current defect: concurrent restart entered while first restart was
            // still parked inside the rollback window (no CoreLifecycleLease).
            assert!(
                !concurrent_entered.load(Ordering::SeqCst),
                "S01 FAILURE reason: change_core rollback window allows concurrent restart \
                 (no shared CoreLifecycleLease; concurrent restart_core entered while \
                 new-core restart was still in flight; entered_restart={})",
                entered_restart.load(Ordering::SeqCst)
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

        let mut core = crate::client::core_bridge::MockRunningCoreBridge::new();
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
        let mut core = crate::client::core_bridge::MockRunningCoreBridge::new();
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
        let mut core = crate::client::core_bridge::MockRunningCoreBridge::new();
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
        let mut core = crate::client::core_bridge::MockRunningCoreBridge::new();
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
