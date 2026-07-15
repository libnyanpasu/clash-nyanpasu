//! Rebuild plumbing (PR-3 T07): the actor-side fire-and-forget notifier, the
//! receiver-side debounced listener (design §6.4: debouncing is the receiver's
//! concern), and the legacy regeneration bridge.

use std::future::Future;

use nyanpasu_config::{application::NyanpasuAppConfig, clash::config::ClashConfig};
use once_cell::sync::OnceCell;
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
        self.regenerate_for_legacy_inner().await
    }

    async fn regenerate_for_legacy_inner(&self) -> Result<()> {
        let (app, clash) = Self::legacy_regen_inputs()?;
        let profiles = self.inner.profiles.get().await?;
        self.regenerate_runtime_with(profiles, clash, app).await
    }

    pub(crate) async fn regenerate_and_apply_for_legacy(&self) -> Result<()> {
        // P0-2: one gate hold covers regenerate AND apply — a concurrent rebuild
        // cannot replace the product between the two steps.
        let _rebuild = self.inner.rebuild_gate.lock().await;
        self.regenerate_for_legacy_inner().await?;
        self.inner
            .core
            .apply_config()
            .await
            .map_err(ClientError::Anyhow)
    }

    pub(crate) async fn regenerate_and_restart_for_legacy(&self) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        self.regenerate_for_legacy_inner().await?;
        self.inner
            .core
            .restart_core()
            .await
            .map_err(ClientError::Anyhow)
    }

    /// Core-switch transaction (spec §5.4). The WHOLE draft→rebuild→restart→
    /// commit/rollback sequence holds the rebuild gate, so no concurrent
    /// rebuild can replace the checked product between check and start (P0-2).
    pub async fn change_core(&self, new_core: crate::config::nyanpasu::ClashCore) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;

        // Last-resort rollback material: the previous product passed its own
        // check when it was promoted, so restoring its bytes needs no re-check.
        let product = crate::client::runtime::runtime_config_path().map_err(ClientError::Anyhow)?;
        let old_product = tokio::fs::read(&product).await.ok();

        // TODO(actor-migration): core selection still drafts the legacy verge.
        // Reason: verge feature flows migrate in PR-5/6.
        // Remove when: core selection patches the typed app config.
        crate::config::Config::verge().draft().clash_core = Some(new_core);

        if let Err(error) = self.regenerate_for_legacy_inner().await {
            crate::config::Config::verge().discard();
            return Err(error); // 产物 / manager 零变化(P0-1 管线保证)
        }

        // TODO(actor-migration): legacy log sink clear on core switch (C7).
        // Remove when: PR-5 injects the LogSink into CoreActor.
        crate::core::logger::Logger::global().clear_log();

        match self.inner.core.restart_core().await {
            Ok(()) => {
                crate::config::Config::verge().apply();
                if let Err(error) = crate::config::Config::verge().latest().save_file() {
                    tracing::error!(%error, "failed to persist verge after core switch");
                }
                Ok(())
            }
            Err(new_core_error) => {
                tracing::error!("failed to change core: {new_core_error:?}");
                crate::config::Config::verge().discard();
                // Rollback = rebuild from committed state. A rollback failure is
                // NEVER swallowed (P0-4): degrade to restoring the previous
                // checked product bytes; the old core must not start on a
                // product built for the new core.
                if let Err(rebuild_error) = self.regenerate_for_legacy_inner().await {
                    let restored: anyhow::Result<()> = match &old_product {
                        Some(bytes) => {
                            crate::client::core_bridge::restore_product(&product, bytes).await
                        }
                        None => tokio::fs::remove_file(&product)
                            .await
                            .map_err(|e| anyhow::anyhow!(e)),
                    };
                    // 注:此分支下 manager 仍持有新核 RuntimeState(发布已随新核
                    // regenerate 完成)——按 spec §5.2 语义:产物权威,下次成功重建自愈。
                    if let Err(restore_error) = restored {
                        return Err(ClientError::Anyhow(
                            new_core_error
                                .context(format!("rollback rebuild failed: {rebuild_error}"))
                                .context(format!(
                                    "product restore failed: {restore_error}; core left stopped"
                                )),
                        ));
                    }
                    if let Err(restart_error) = self.inner.core.restart_core().await {
                        return Err(ClientError::Anyhow(
                            new_core_error
                                .context(format!("rollback rebuild failed: {rebuild_error}"))
                                .context(format!("old core restart failed: {restart_error}")),
                        ));
                    }
                    return Err(ClientError::Anyhow(new_core_error.context(format!(
                        "rollback rebuild failed: {rebuild_error}; restored previous product"
                    ))));
                }
                if let Err(restart_error) = self.inner.core.restart_core().await {
                    return Err(ClientError::Anyhow(new_core_error.context(format!(
                        "old core restart failed after rollback: {restart_error}"
                    ))));
                }
                Err(ClientError::Anyhow(new_core_error))
            }
        }
    }

    /// Boot fallback (spec §5.6, D8): the default config is ALSO routed through
    /// candidate -> check -> promote — D5 has no exceptions. A failed check
    /// leaves no product; boot continues and the core start fails visibly.
    pub(crate) async fn promote_default_runtime_config(&self) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
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
        let candidate = crate::client::runtime::candidate_config_path();
        {
            // Exclusive create (create_new): the unique candidate path must not
            // already exist. A pre-existing file/symlink now fails the pipeline
            // visibly instead of being followed (TOCTOU hardening, PR-4 re-review).
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
                .await
                .map_err(|error| {
                    ClientError::Custom(format!("failed to create candidate: {error}"))
                })?;
            file.write_all(yaml.as_bytes()).await.map_err(|error| {
                ClientError::Custom(format!("failed to write candidate: {error}"))
            })?;
            file.flush().await.map_err(|error| {
                ClientError::Custom(format!("failed to flush candidate: {error}"))
            })?;
        }
        let candidate = super::utf8_path(candidate).map_err(ClientError::Anyhow)?;
        let checked = self
            .inner
            .core
            .check_and_promote(&candidate, app.core)
            .await;
        if let Err(error) = tokio::fs::remove_file(candidate.as_std_path()).await {
            tracing::warn!(%error, ?candidate, "failed to remove candidate config");
        }
        checked.map_err(ClientError::Anyhow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use camino::Utf8Path;
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
    /// The product path (`runtime_config_path`/`app_config_dir`) is process-global
    /// and not env-injectable, so this asserts control flow via the mock sequence
    /// rather than integration-testing the restored bytes. It stays self-contained:
    /// the pre-test product bytes are captured and restored regardless of outcome.
    #[test]
    fn change_core_rollback_rebuild_failure_restores_product_and_errors() {
        let dir = tempfile::tempdir().unwrap();
        // Seed the global product so change_core reads Some(old_product): the
        // rollback restore path then runs restore_product (real fs) and attempts
        // the old-core restart, giving a deterministic sequence. Original bytes are
        // restored below before any assertion can unwind.
        let product = crate::client::runtime::runtime_config_path().unwrap();
        let original = std::fs::read(&product).ok();
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

        // Restore the global product to its pre-test state before asserting.
        match &original {
            Some(bytes) => {
                let _ = std::fs::write(&product, bytes);
            }
            None => {
                let _ = std::fs::remove_file(&product);
            }
        }

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
            _candidate: &Utf8Path,
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

    /// S01 contract (task §S01.2 / design failure matrix change_core row): when
    /// rollback rebuild fails and product bytes are restored, the runtime read
    /// model (Promoted) must also be restored to the old snapshot. Desired
    /// (S03): product restore + promoted restore are atomic.
    ///
    /// Current failure reason: product restore only rewrites the file; the
    /// RuntimeState store still holds the new-core publish from the successful
    /// new-core regenerate (see rebuild.rs comment at the restore branch).
    #[test]
    fn s01_contract_product_restore_leaves_runtime_read_model_on_new_core() {
        let dir = tempfile::tempdir().unwrap();
        let product = crate::client::runtime::runtime_config_path().unwrap();
        let original = std::fs::read(&product).ok();
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
        let result = tauri::async_runtime::block_on(
            client.change_core(crate::config::nyanpasu::ClashCore::ClashRs),
        );
        assert!(result.is_err(), "change_core must surface compound error");

        // Product bytes should have been restored to OLD_PRODUCT by the
        // last-resort restore path (control-flow pin already covered above).
        let restored_bytes = std::fs::read(&product).unwrap_or_default();

        // Read model after the deep rollback branch.
        let state = tauri::async_runtime::block_on(client.runtime_state());

        // Cleanup global product before assertions that may panic.
        match &original {
            Some(bytes) => {
                let _ = std::fs::write(&product, bytes);
            }
            None => {
                let _ = std::fs::remove_file(&product);
            }
        }

        // Desired (S03): after product restore, Promoted is None or the old
        // snapshot — never the new-core publish. Current defect: regenerate
        // for the new core published RuntimeState, and the restore branch
        // never rolls the store back (explicit comment in change_core).
        let read_model_still_new = state.as_ref().is_some();
        assert!(
            !read_model_still_new,
            "S01 FAILURE reason: product restore after rollback-rebuild failure leaves \
             runtime read model on the new-core publish (store still Some); product \
             bytes restored={} store_is_some={}",
            restored_bytes == OLD_PRODUCT,
            read_model_still_new
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
