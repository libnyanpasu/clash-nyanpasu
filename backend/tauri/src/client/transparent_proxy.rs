//! Execution-host adapter for the privileged network owner. No system commands
//! run in the GUI process; the host verifies the complete applied revision.
use crate::core::actor_v2::{CoreClient, endpoint::ExecutionHost};
use nyanpasu_config::clash::config::TransparentProxyMode;
use nyanpasu_ipc::api::network::transparent_proxy::{
    NetworkTransparentProxyMode, NetworkTransparentProxyRequest, NetworkTransparentProxyStatus,
};

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
    let endpoint = core.connected_endpoint().await?;
    if endpoint.host() != ExecutionHost::Service {
        anyhow::ensure!(!enabled, "transparent capture requires service mode");
        return Ok(());
    }
    let snapshot = endpoint.status().await?;
    let expected_revision = match snapshot.revision {
        Some(revision) => revision,
        None if !enabled => nyanpasu_ipc::api::status::RevisionIdInfo {
            epoch: 0,
            generation: 0,
            effective_hash: String::new(),
        },
        None => anyhow::bail!("core has no applied runtime revision"),
    };
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
        expected_revision,
    };
    let status = endpoint.reconcile_transparent_proxy(&request).await?;
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

impl NyanpasuClient {
    pub async fn get_transparent_proxy_status(
        &self,
    ) -> anyhow::Result<NetworkTransparentProxyStatus> {
        let endpoint = self.inner.core_api.connected_endpoint().await?;
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
}
