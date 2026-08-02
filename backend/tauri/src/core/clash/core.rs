use crate::{config::Config, utils::dirs};
#[cfg(target_os = "macos")]
use anyhow::Result;
#[cfg(target_os = "macos")]
use nyanpasu_ipc::api::network::set_dns::NetworkSetDnsReq;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use specta::Type;
#[cfg(target_os = "macos")]
use std::borrow::Cow;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunType {
    /// Run as child process directly
    Normal,
    /// Run by Nyanpasu Service via a ipc call
    Service,
    // TODO: Not implemented yet
    /// Run as elevated process, if profile advice to run as elevated
    Elevated,
}

impl RunType {
    /// 纯分类：Service backend 的唯一判据。
    /// `IpcState::Connected` 只可能由通过 `ServiceCompat` 门禁的 daemon 产生
    /// （见 `core::service::ipc::target_ipc_state`），因此旧版 daemon 无法到达
    /// `RunType::Service`，也就无法选择会发 `/core/*` 请求的 Service backend。
    pub fn classify(enable_service: bool, ipc_state: crate::core::service::ipc::IpcState) -> Self {
        if enable_service && ipc_state.is_connected() {
            Self::Service
        } else {
            Self::Normal
        }
    }
}

impl Default for RunType {
    fn default() -> Self {
        let enable_service = {
            *Config::verge()
                .latest()
                .enable_service_mode
                .as_ref()
                .unwrap_or(&false)
        };
        let run_type = Self::classify(enable_service, crate::core::service::ipc::get_ipc_state());
        if run_type == Self::Service {
            tracing::info!("run core as service");
        } else {
            tracing::info!("run core as child process");
        }
        run_type
    }
}

#[derive(Debug)]
pub struct CoreManager {
    #[cfg(target_os = "macos")]
    previous_dns: tokio::sync::Mutex<Option<Vec<std::net::IpAddr>>>,
}

impl CoreManager {
    pub fn global() -> &'static CoreManager {
        static CORE_MANAGER: OnceCell<CoreManager> = OnceCell::new();
        CORE_MANAGER.get_or_init(|| CoreManager {
            #[cfg(target_os = "macos")]
            previous_dns: tokio::sync::Mutex::new(None),
        })
    }

    #[cfg(target_os = "macos")]
    pub async fn change_default_network_dns(&self, enabled: bool) -> Result<()> {
        use anyhow::Context;
        use nyanpasu_utils::network::macos::*;

        let run_type = RunType::default();

        log::debug!(target: "app", "try to set system dns");
        let default_device =
            get_default_network_hardware_port().context("failed to get default network device")?;
        log::debug!(target: "app", "current default network device: {:?}", default_device);
        let tun_device_ip = Config::clash()
            .clone()
            .latest()
            .get_tun_device_ip()
            .parse::<std::net::IpAddr>()
            .context("failed to parse tun device ip")?;
        log::debug!(target: "app", "current tun device ip: {:?}", tun_device_ip);

        let current_dns = get_dns(&default_device).context("failed to get current dns")?;
        log::debug!(target: "app", "current dns: {:?}", current_dns);
        let current_dns_contains_tun_device_ip = current_dns
            .as_ref()
            .is_some_and(|dns| dns.contains(&tun_device_ip));
        let mut previous_dns = self.previous_dns.lock().await;
        let previous_dns_clone = previous_dns.clone();
        let new_dns = match enabled {
            true if !current_dns_contains_tun_device_ip => {
                *previous_dns = current_dns;
                Some(Some(vec![tun_device_ip]))
            }
            false if current_dns_contains_tun_device_ip => Some(previous_dns.take()),
            _ => None,
        };
        if let Some(new_dns) = new_dns {
            log::debug!(target: "app", "set new dns: {:?}", new_dns);
            let result = match run_type {
                RunType::Service => nyanpasu_ipc::client::shortcuts::Client::service_default()
                    .set_dns(&NetworkSetDnsReq {
                        dns_servers: new_dns
                            .as_ref()
                            .map(|dns| dns.iter().map(|ip| Cow::Owned(*ip)).collect()),
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}")),
                _ => set_dns(&default_device, new_dns).map_err(anyhow::Error::from),
            };
            if let Err(e) = result.context("failed to set system dns") {
                *previous_dns = previous_dns_clone;
                return Err(e);
            }
        }
        Ok(())
    }
}

// TODO: support system path search via a config or flag
// FIXME: move this fn to nyanpasu-utils
/// Search the binary path of the core: Data Dir -> Sidecar Dir
pub fn find_binary_path(core_type: &nyanpasu_utils::core::CoreType) -> std::io::Result<PathBuf> {
    let data_dir = dirs::app_data_dir()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err.to_string()))?;
    let binary_path = data_dir.join(core_type.get_executable_name());
    if binary_path.exists() {
        return Ok(binary_path);
    }
    let app_dir = dirs::app_install_dir()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::NotFound, err.to_string()))?;
    let binary_path = app_dir.join(core_type.get_executable_name());
    if binary_path.exists() {
        return Ok(binary_path);
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{} not found", core_type.get_executable_name()),
    ))
}
