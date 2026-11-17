//! The system service: its status, installation and mode.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_paths::PathResolver;
use nyanpasu_rpc_protocol::types::RpcUnitMutationOutcome;
use nyanpasu_runtime_values::observation::ServiceStatusSummary;

use crate::{
    commands::types::ServiceStatusInfo, error::Result, ports::ServiceInstallPrompt,
    registry::RpcState,
};

nyanpasu_rpc::resource!(event "service-status-summary-changed-event", targets = [desktop, http]);

nyanpasu_rpc::resource!(event "service-status-changed-event", targets = [desktop, http, local], sensitive);

#[nyanpasu_rpc::rpc(query, expose = [http], sensitive)]
#[specta::specta]
pub async fn get_service_install_prompt(
    prompt: RpcState<'_, dyn ServiceInstallPrompt>,
    paths: RpcState<'_, PathResolver>,
) -> Result<String> {
    prompt.service_install_prompt(paths).await
}

/// The service daemon's version, install state and phase, without its server details.
#[nyanpasu_rpc::rpc(query, expose = [http])]
#[specta::specta]
pub fn get_service_status_summary(client: RpcState<'_, NyanpasuClient>) -> ServiceStatusSummary {
    ServiceStatusSummary::from(&client.service_status())
}

// See nyanpasu_config::application::ApplicationPreferencesPatch for the command-scope and domain-split rule.
#[nyanpasu_rpc::rpc(privacy, optional_capabilities(elevation, os_integration, presentation), expose = [])]
#[specta::specta]
pub async fn set_service_mode(
    client: RpcState<'_, NyanpasuClient>,
    enabled: bool,
) -> Result<RpcUnitMutationOutcome> {
    Ok(client.set_service_mode(enabled).await?.into())
}

#[nyanpasu_rpc::rpc(query, expose = [http], sensitive)]
#[specta::specta]
pub async fn status_service(client: RpcState<'_, NyanpasuClient>) -> Result<ServiceStatusInfo> {
    let info = client.service_status();
    Ok(ServiceStatusInfo {
        name: info.name,
        version: info.version,
        status: info.status,
        server: info.server,
        compat: info.compat,
        phase: info.phase,
        restart_attempts: info.restart_attempts,
    })
}
