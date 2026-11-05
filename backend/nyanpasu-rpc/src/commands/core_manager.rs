//! The core binary: which one runs, its version, its agent skills and its
//! updates.

use nyanpasu_config::application::ClashCore;
use nyanpasu_core::{
    client::{NyanpasuClient, agent_skills::CoreAgentSkill},
    updater::ManifestVersionLatest,
};

use super::Result;
use crate::State;

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn change_clash_core(client: State<'_, NyanpasuClient>, clash_core: ClashCore) -> Result {
    client.update_core(clash_core).await?;
    Ok(())
}

/// restart the sidecar
#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn restart_sidecar(client: State<'_, NyanpasuClient>) -> Result {
    client.reconcile_core().await?;
    Ok(())
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn fetch_latest_core_versions(
    client: State<'_, NyanpasuClient>,
) -> Result<ManifestVersionLatest> {
    Ok(client.fetch_latest_core_versions().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn inspect_updater(
    client: State<'_, NyanpasuClient>,
    updater_id: usize,
) -> Result<nyanpasu_core::updater::UpdaterSummary> {
    Ok(client.inspect_updater(updater_id).await?)
}

/// The agent skills a core ships. Their folders are on the host that runs
/// Nyanpasu, which is also where an agent driving the core has to run.
#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_core_agent_skills(
    client: State<'_, NyanpasuClient>,
    core_type: ClashCore,
) -> Result<Vec<CoreAgentSkill>> {
    Ok(client.core_agent_skills(core_type).await)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_core_version(
    client: State<'_, NyanpasuClient>,
    core_type: ClashCore,
) -> Result<String> {
    Ok(client.get_core_version(core_type).await?)
}
