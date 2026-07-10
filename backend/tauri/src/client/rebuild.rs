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
// Remove after PR-4/PR-5 migrate those flows onto injected clients.
// Known limitation (accepted): installation is first-install-wins — a second
// client constructed in the same process (tests) keeps the FIRST client's
// handler, including its paths. Production installs exactly once at setup.
type RegenRequest = oneshot::Sender<anyhow::Result<()>>;
static REGEN_BRIDGE: OnceCell<mpsc::UnboundedSender<RegenRequest>> = OnceCell::new();

pub(super) fn install_regen_bridge<F, Fut>(handler: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<RegenRequest>();
    if REGEN_BRIDGE.set(tx).is_err() {
        tracing::warn!("regeneration bridge already installed; keeping the first");
        return;
    }
    tauri::async_runtime::spawn(async move {
        while let Some(reply) = rx.recv().await {
            let _ = reply.send(handler().await);
        }
    });
}

/// Sequenced regeneration for legacy callers: awaits the facade rebuild of the
/// runtime draft (no core apply) before returning, preserving the legacy
/// `Config::generate().await?` ordering guarantees.
pub async fn regenerate() -> anyhow::Result<()> {
    let bridge = REGEN_BRIDGE
        .get()
        .ok_or_else(|| anyhow::anyhow!("regeneration bridge not installed"))?;
    let (tx, rx) = oneshot::channel();
    bridge
        .send(tx)
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
    // Remove when: PR-4/5/6 migrate the legacy writers onto typed clients.
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
        let (app, clash) = Self::legacy_regen_inputs()?;
        let profiles = self.inner.profiles.get().await?;
        self.regenerate_runtime_with(profiles, clash, app).await
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
}
