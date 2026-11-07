//! Shared updater command adapters.

use nyanpasu_config::application::ClashCore;
use nyanpasu_core::client::{NyanpasuClient, agent_skills::CoreAgentSkill};
use nyanpasu_updater::ManifestVersionLatest;

use crate::{error::Result, registry::RpcState};

/// The agent skills a core ships. Their folders are on the host that runs
/// Nyanpasu, which is also where an agent driving the core has to run.
#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn get_core_agent_skills(
    client: RpcState<'_, NyanpasuClient>,
    core_type: ClashCore,
) -> Result<Vec<CoreAgentSkill>> {
    Ok(client.core_agent_skills(core_type).await)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn fetch_latest_core_versions(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<ManifestVersionLatest> {
    Ok(client.fetch_latest_core_versions().await?)
}

#[nyanpasu_macro::rpc(http)]
#[specta::specta]
pub async fn inspect_updater(
    client: RpcState<'_, NyanpasuClient>,
    updater_id: usize,
) -> Result<nyanpasu_updater::UpdaterSummary> {
    Ok(client.inspect_updater(updater_id).await?)
}
