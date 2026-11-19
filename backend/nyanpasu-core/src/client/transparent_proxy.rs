//! Execution-host adapter for the privileged network owner. No system commands
//! run in the GUI process; the host verifies the complete applied revision.
use crate::actor_v2::CoreClient;
use nyanpasu_config::clash::config::{
    ClashConfig, ClashConfigPatch, TransparentProxyConfigPatch, TransparentProxyMode,
    clash_strategy::{PortStrategy, PortStrategyKind},
};
use nyanpasu_ipc::api::network::transparent_proxy::{
    NetworkTransparentProxyMode, NetworkTransparentProxyRequest, NetworkTransparentProxyStatus,
};
use nyanpasu_runtime_values::execution_host::ExecutionHost;

use super::{NyanpasuClient, effects::plan::TransparentProxyDesired};

pub(crate) async fn reconcile(
    core: &CoreClient,
    desired: &TransparentProxyDesired,
) -> anyhow::Result<()> {
    let enabled = desired.config.mode != TransparentProxyMode::Disabled;
    let port = if enabled {
        let port = desired
            .port
            .ok_or_else(|| anyhow::anyhow!("transparent proxy listener port is not resolved"))?;
        anyhow::ensure!(port != 0, "transparent proxy listener port must be nonzero");
        Some(port)
    } else {
        None
    };
    if !cfg!(target_os = "linux") {
        anyhow::ensure!(!enabled, "transparent capture is supported only on Linux");
        return Ok(());
    }
    let mode = match desired.config.mode {
        TransparentProxyMode::Disabled => NetworkTransparentProxyMode::Disabled,
        TransparentProxyMode::Redir => NetworkTransparentProxyMode::Redir,
        TransparentProxyMode::Tproxy => NetworkTransparentProxyMode::Tproxy,
    };
    let request = NetworkTransparentProxyRequest {
        mode,
        port: port.unwrap_or_default(),
        local: desired.config.local,
        interfaces: desired.config.interfaces.clone(),
        ipv6: desired.config.ipv6,
        expected_revision: nyanpasu_ipc::api::status::RevisionIdInfo {
            epoch: 0,
            generation: 0,
            effective_hash: String::new(),
        },
    };
    let status = core.reconcile_transparent_proxy(request).await?;
    if !status.supported {
        anyhow::ensure!(
            !enabled && !status.active,
            "the service does not support transparent capture"
        );
        return Ok(());
    }
    if let Some(error) = status.error {
        anyhow::bail!("{error}");
    }
    anyhow::ensure!(
        status.active == enabled,
        "transparent capture did not reach the requested state"
    );
    Ok(())
}

/// What the execution host says about transparent capture; a host other than
/// the service does not support it.
pub(super) async fn status(core: &CoreClient) -> anyhow::Result<NetworkTransparentProxyStatus> {
    let endpoint = core.connected_endpoint().await?;
    if endpoint.host() != ExecutionHost::Service {
        return Ok(NetworkTransparentProxyStatus {
            supported: false,
            active: false,
            mode: None,
            revision: None,
            error: None,
        });
    }
    endpoint
        .transparent_proxy_status()
        .await
        .map_err(Into::into)
}

/// Diagnostics are available only from a Linux privileged service. A local
/// core, another OS, or an older service reports its typed availability.
pub(super) async fn diagnostics(
    core: &CoreClient,
) -> anyhow::Result<nyanpasu_runtime_values::transparent_proxy::TransparentProxyDiagnostics> {
    use nyanpasu_runtime_values::transparent_proxy::TransparentProxyDiagnostics;

    if !cfg!(target_os = "linux") {
        return Ok(TransparentProxyDiagnostics::Unsupported);
    }
    let endpoint = core.connected_endpoint().await?;
    if endpoint.host() != ExecutionHost::Service {
        return Ok(TransparentProxyDiagnostics::Unsupported);
    }
    endpoint
        .transparent_proxy_diagnostics()
        .await
        .map_err(Into::into)
}

/// The patch that switches transparent capture on, keeping a mode the user
/// already chose, or off.
pub(super) fn switch_patch(config: &ClashConfig, enabled: bool) -> ClashConfigPatch {
    let mode = if enabled {
        match config.transparent_proxy.mode {
            TransparentProxyMode::Disabled => TransparentProxyMode::Tproxy,
            mode => mode,
        }
    } else {
        TransparentProxyMode::Disabled
    };
    ClashConfigPatch {
        transparent_proxy: TransparentProxyConfigPatch {
            mode: Some(mode),
            ..Default::default()
        },
        tproxy_port: (mode == TransparentProxyMode::Tproxy && config.tproxy_port.is_none())
            .then_some(Some(PortStrategy {
                kind: PortStrategyKind::Fixed,
                start_port: 7894,
            })),
        ..Default::default()
    }
}

impl NyanpasuClient {
    pub async fn get_transparent_proxy_status(
        &self,
    ) -> anyhow::Result<NetworkTransparentProxyStatus> {
        status(&self.inner.core_api).await
    }

    pub async fn get_transparent_proxy_diagnostics(
        &self,
    ) -> anyhow::Result<nyanpasu_runtime_values::transparent_proxy::TransparentProxyDiagnostics>
    {
        diagnostics(&self.inner.core_api).await
    }
}
