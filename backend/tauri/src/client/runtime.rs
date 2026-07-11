//! Runtime derived state (PR-4): the read model the facade holds after each
//! rebuild, plus the product/candidate config file locations. Runtime is a
//! pure derivation — there is no writable runtime state anywhere else.

use std::path::PathBuf;

use nyanpasu_core::state::{SimpleStateManager, SimpleStateManagerSetup};
use serde_yaml::Mapping;

use crate::{enhance::PostProcessingOutput, utils::dirs};

pub const RUNTIME_CONFIG_DIR: &str = "runtime";
pub const RUNTIME_CONFIG: &str = "clash-config.yaml";

/// Read model of the current runtime derivation (former `IRuntime`, minus the
/// draft machinery). Derived once per rebuild while the profiles snapshot is
/// in hand; the four runtime read commands serve straight from this.
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
        .map_err(|_| anyhow::anyhow!("failed to initialize runtime state store"))?;
    Ok(tokio::sync::RwLock::new(manager))
}

/// The promoted (checked) product consumed by core start/hot-reload. Same
/// location the legacy `Config::runtime_config_path()` used.
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
