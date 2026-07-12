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
    let (tx, mut rx) = mpsc::unbounded_channel::<RegenRequest>();
    if REGEN_BRIDGE.set(tx).is_err() {
        tracing::warn!("regeneration bridge already installed; keeping the first");
        return;
    }
    tauri::async_runtime::spawn(async move {
        while let Some(request) = rx.recv().await {
            let result = handler(request.kind).await;
            let _ = request.reply.send(result);
        }
    });
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
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

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
}
