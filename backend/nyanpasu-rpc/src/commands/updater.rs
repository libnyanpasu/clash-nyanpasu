//! Shared updater command adapters.

use nyanpasu_config::application::ClashCore;
use nyanpasu_core::client::{
    NyanpasuClient,
    agent_skills::{CoreAgentSkill, CoreAgentSkillSummary},
};
use nyanpasu_updater::{ManifestVersionLatest, UpdaterProgressSummary};

use crate::{error::Result, registry::RpcState};

/// The agent skills a core ships. Their folders are on the host that runs
/// Nyanpasu, which is also where an agent driving the core has to run.
#[nyanpasu_rpc::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn get_core_agent_skills(
    client: RpcState<'_, NyanpasuClient>,
    core_type: ClashCore,
) -> Result<Vec<CoreAgentSkill>> {
    Ok(client.core_agent_skills(core_type).await)
}

/// Safe agent skill names and descriptions without local filesystem directories.
/// The full skills (including `dir`) are returned by [`get_core_agent_skills`].
#[nyanpasu_rpc::rpc(http, query)]
#[specta::specta]
pub async fn get_core_agent_skills_summary(
    client: RpcState<'_, NyanpasuClient>,
    core_type: ClashCore,
) -> Result<Vec<CoreAgentSkillSummary>> {
    Ok(client.core_agent_skills_summary(core_type).await)
}

#[nyanpasu_rpc::rpc(http, query)]
#[specta::specta]
pub async fn fetch_latest_core_versions(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<ManifestVersionLatest> {
    Ok(client.fetch_latest_core_versions().await?)
}

#[nyanpasu_rpc::rpc(http, query, scope = sensitive_read)]
#[specta::specta]
pub async fn inspect_updater(
    client: RpcState<'_, NyanpasuClient>,
    updater_id: usize,
) -> Result<nyanpasu_updater::UpdaterSummary> {
    Ok(client.inspect_updater(updater_id).await?)
}

/// Safe updater progress without raw failure diagnostics. The full summary is
/// [`inspect_updater`].
#[nyanpasu_rpc::rpc(http, query)]
#[specta::specta]
pub async fn inspect_updater_summary(
    client: RpcState<'_, NyanpasuClient>,
    updater_id: usize,
) -> Result<UpdaterProgressSummary> {
    Ok(UpdaterProgressSummary::from(
        &client.inspect_updater(updater_id).await?,
    ))
}
