//! Runtime derived state (PR-4): the read model the facade holds after each
//! rebuild, plus the product/candidate config file locations. Runtime is a
//! pure derivation — there is no writable runtime state anywhere else.

use std::path::PathBuf;

use nyanpasu_core::state::{SimpleStateManager, SimpleStateManagerSetup};
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::{enhance::PostProcessingOutput, utils::dirs};

pub const RUNTIME_CONFIG_DIR: &str = "runtime";
pub const RUNTIME_CONFIG: &str = "clash-config.yaml";

/// Read model of the current runtime derivation (replaces the old
/// draft-based config type, minus the draft machinery). Derived once per
/// rebuild while the profiles snapshot is in hand; the four runtime read
/// commands serve straight from this.
///
/// Semantics (spec §5.1, r2): the latest TARGET config that passed the core
/// binary's check and was promoted to the product. It does NOT promise the
/// running core accepted it — a failed apply is reported as
/// `RebuildOutcome::Degraded`, not reflected here.
#[derive(Debug, Clone, Default)]
pub struct RuntimeState {
    pub config: Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
}

/// Facade-held runtime store. The RwLock is a narrowly scoped implementation
/// detail (CLAUDE.md §8 exception): `upsert` needs `&mut`, writers are already
/// serialized by the facade `rebuild_gate`, readers take `snapshot()`.
/// SimpleStateManager (not a bare RwLock<Option<..>>) is deliberate: its
/// StateCoordinator ack subscribers are the landing point for the
/// TODO(post-PR-7) ack-driven rollback direction (spec D2).
pub type RuntimeStateStore = tokio::sync::RwLock<SimpleStateManager<Option<RuntimeState>>>;

pub async fn new_runtime_state_store() -> anyhow::Result<RuntimeStateStore> {
    let manager = SimpleStateManagerSetup::builder()
        .initial_state(None)
        .assemble()
        .initialize()
        .await
        .map_err(|error| anyhow::anyhow!("failed to initialize runtime state store: {error:?}"))?;
    Ok(tokio::sync::RwLock::new(manager))
}

/// D6 (spec §6.4): previous values of the keys a clash patch touches, taken
/// from the published runtime state. Used to push the running core BACK when
/// the post-patch rebuild fails — the IPC applies the patch API-first, so a
/// failed rebuild would otherwise leave the core ahead of the persisted state.
pub(crate) fn compensation_for(patch: &Mapping, prev: Option<&Mapping>) -> Option<Mapping> {
    let prev = prev?;
    let comp: Mapping = patch
        .iter()
        .filter_map(|(k, _)| prev.get(k).map(|v| (k.clone(), v.clone())))
        .collect();
    (!comp.is_empty()).then_some(comp)
}

/// The promoted (checked) product consumed by core start/hot-reload. Same
/// location the old (now-deleted) runtime config path helper used.
pub fn runtime_config_path() -> anyhow::Result<PathBuf> {
    Ok(dirs::app_config_dir()?
        .join(RUNTIME_CONFIG_DIR)
        .join(RUNTIME_CONFIG))
}

/// Where a rebuild writes the unchecked candidate before check + promote
/// (spec D5: the product only ever holds configs that passed the check).
/// Unique per attempt (spec §5.2, r2): a fixed temp path is a TOCTOU /
/// multi-instance / parallel-test clobber hazard. The pipeline best-effort
/// deletes the candidate after `check_and_promote`.
pub fn candidate_config_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "clash-nyanpasu-candidate-{}-{seq}.yaml",
        std::process::id()
    ))
}

/// Post-commit rebuild result for mutation IPC (spec §6.2, decision D2):
/// state is committed first; a failed rebuild degrades instead of erroring.
// TODO(post-PR-7): degraded outcome is transitional. State managers already
// expose async commit acks; the end-state is ack-driven rollback when config
// application fails, replacing this degraded-report model. Tracked in
// actor-migration-roadmap §6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RebuildOutcome {
    Ok,
    Degraded { error: String },
}

impl RebuildOutcome {
    /// Combine sequential outcomes; the first degradation wins.
    pub fn merge(self, other: RebuildOutcome) -> RebuildOutcome {
        match self {
            RebuildOutcome::Degraded { .. } => self,
            RebuildOutcome::Ok => other,
        }
    }
}

/// Mutation payload + rebuild outcome for data-carrying commands (import).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CommitOutcome<T> {
    pub value: T,
    pub rebuild: RebuildOutcome,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compensation_restores_previous_values_of_patched_keys() {
        let mut prev = Mapping::new();
        prev.insert("mode".into(), "rule".into());
        prev.insert("allow-lan".into(), false.into());
        let mut patch = Mapping::new();
        patch.insert("mode".into(), "direct".into());
        patch.insert("ipv6".into(), true.into()); // prev 无该键 → 略过
        let comp = compensation_for(&patch, Some(&prev)).expect("some");
        assert_eq!(comp.get("mode"), Some(&"rule".into()));
        assert!(comp.get("ipv6").is_none());
        assert!(compensation_for(&patch, None).is_none());
    }

    /// S01 contract (task §S01.8 / design D6): compensation must be able to
    /// `Remove` keys that exist only on the running core (absent from Applied).
    ///
    /// Current failure reason: `compensation_for` only copies previous values
    /// for keys present in `prev` and silently drops brand-new patch keys, so
    /// API-first patches cannot be fully rolled back. It also reads the single
    /// published runtime store (Promoted semantics) rather than a distinct
    /// Applied snapshot.
    #[test]
    fn s01_contract_compensation_cannot_remove_keys_absent_from_applied() {
        let mut applied = Mapping::new();
        applied.insert("mode".into(), "rule".into());

        let mut patch = Mapping::new();
        patch.insert("mode".into(), "direct".into());
        // New key introduced by the API-first patch — Applied has no prior value.
        patch.insert("ipv6".into(), true.into());

        let comp = compensation_for(&patch, Some(&applied))
            .expect("compensation must exist when Applied has at least one overlapping key");

        assert_eq!(
            comp.get("mode"),
            Some(&"rule".into()),
            "existing Applied keys must be restored via Set"
        );

        // Desired (S05): brand-new keys produce an explicit Remove op so the
        // running core drops them. Current helper cannot express Remove at all.
        assert!(
            comp.contains_key("ipv6"),
            "S01 FAILURE reason: compensation cannot Remove keys absent from Applied \
             (new patch keys are dropped; helper only emits Set-from-prev and reads \
             the single promoted runtime store rather than Applied)"
        );
    }

    /// S01 contract (task §S01.7 / design §8.4): runtime product path resolution
    /// used by tests and production must not escape into the real user app
    /// config directory. Desired (S02): every path comes from an injected
    /// `RuntimePaths` rooted at a TempDir.
    ///
    /// Current failure reason: `runtime_config_path` delegates to
    /// `dirs::app_config_dir()`, so unit tests resolve and may mutate the real
    /// product location.
    #[test]
    fn s01_contract_runtime_product_path_resolves_real_app_config_dir() {
        let product = runtime_config_path().expect("runtime product path must resolve");
        let real_app_config =
            dirs::app_config_dir().expect("real app config dir must resolve for comparison");

        let under_real = product.starts_with(&real_app_config);
        assert!(
            !under_real,
            "S01 FAILURE reason: runtime_config_path resolves the real product path \
             under {} (got {}); tests/production still depend on process-global \
             dirs::app_config_dir instead of injected RuntimePaths/TempDir",
            real_app_config.display(),
            product.display()
        );
    }
}
