//! The core: its status, version, lifecycle, updates and directory.

use nyanpasu_config::{application::ClashCore, clash::features::ConfigFeatures};
use nyanpasu_core::client::{
    NyanpasuClient,
    agent_skills::{CoreAgentSkill, CoreAgentSkillSummary},
};
use nyanpasu_updater::{ManifestVersionLatest, UpdaterProgressSummary};

use crate::{error::Result, ports::CoreDirectoryResolver, registry::RpcState};

nyanpasu_rpc::resource!(event "core-status-summary-changed-event", targets = [desktop, http]);

nyanpasu_rpc::resource!(event "core-status-changed-event", targets = [desktop, http, local], sensitive);

#[nyanpasu_rpc::rpc(optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn change_clash_core(
    client: RpcState<'_, NyanpasuClient>,
    clash_core: ClashCore,
) -> Result {
    client.update_core(clash_core).await?;
    Ok(())
}

#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn clash_api_get_version(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_core::clash::api::ClashVersion> {
    Ok(client.clash_version().await?)
}

#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub async fn fetch_latest_core_versions(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<ManifestVersionLatest> {
    Ok(client.fetch_latest_core_versions().await?)
}

#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub async fn get_config_features(client: RpcState<'_, NyanpasuClient>) -> Result<ConfigFeatures> {
    Ok(client.config_features())
}

/// The agent skills a core ships. Their folders are on the host that runs
/// Nyanpasu, which is also where an agent driving the core has to run.
#[nyanpasu_rpc::rpc(query, expose = [http], sensitive)]
#[specta::specta]
pub async fn get_core_agent_skills(
    client: RpcState<'_, NyanpasuClient>,
    core_type: ClashCore,
) -> Result<Vec<CoreAgentSkill>> {
    Ok(client.core_agent_skills(core_type).await)
}

/// Safe agent skill names and descriptions without local filesystem directories.
/// The full skills (including `dir`) are returned by [`get_core_agent_skills`].
#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub async fn get_core_agent_skills_summary(
    client: RpcState<'_, NyanpasuClient>,
    core_type: ClashCore,
) -> Result<Vec<CoreAgentSkillSummary>> {
    Ok(client.core_agent_skills_summary(core_type).await)
}

#[nyanpasu_rpc::rpc(query, expose = [http], sensitive)]
#[specta::specta]
pub fn get_core_dir(resolver: RpcState<'_, dyn CoreDirectoryResolver>) -> Result<String> {
    resolver.core_directory()
}

#[nyanpasu_rpc::rpc(operation = nyanpasu_rpc_protocol::operations::GetCoreStatus, query, sensitive)]
#[specta::specta]
pub async fn get_core_status(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::core_status::CoreStatusInfo> {
    Ok(client.core_status().into())
}

/// Which core is running and in what state, without controller endpoints or failure text.
#[nyanpasu_rpc::rpc(query)]
#[specta::specta]
pub async fn get_core_status_summary(
    client: RpcState<'_, NyanpasuClient>,
) -> Result<nyanpasu_runtime_values::observation::CoreStatusSummary> {
    Ok(
        nyanpasu_runtime_values::observation::CoreStatusSummary::from(
            &nyanpasu_runtime_values::core_status::CoreStatusInfo::from(client.core_status()),
        ),
    )
}

#[nyanpasu_rpc::rpc(query, expose = [http], sensitive)]
#[specta::specta]
pub async fn inspect_updater(
    client: RpcState<'_, NyanpasuClient>,
    updater_id: usize,
) -> Result<nyanpasu_updater::UpdaterSummary> {
    Ok(client.inspect_updater(updater_id).await?)
}

/// Safe updater progress without raw failure diagnostics. The full summary is
/// [`inspect_updater`].
#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub async fn inspect_updater_summary(
    client: RpcState<'_, NyanpasuClient>,
    updater_id: usize,
) -> Result<UpdaterProgressSummary> {
    Ok(UpdaterProgressSummary::from(
        &client.inspect_updater(updater_id).await?,
    ))
}

/// restart the sidecar
#[nyanpasu_rpc::rpc(optional_capabilities(elevation, os_integration, presentation))]
#[specta::specta]
pub async fn restart_sidecar(client: RpcState<'_, NyanpasuClient>) -> Result {
    client.reconcile_core().await?;
    Ok(())
}
