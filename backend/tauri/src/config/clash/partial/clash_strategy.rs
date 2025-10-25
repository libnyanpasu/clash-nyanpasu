use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use super::super::config::DEFAULT_EXTERNAL_CONTROLLER_PORT;
use derive_builder::Builder;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::Arc;

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type, Builder)]
#[builder(default, derive(Debug, Serialize, Deserialize, Type))]
#[builder_struct_attr(serde_with::skip_serializing_none)]
pub struct ClashStrategy {
    /// 外部控制器端口策略
    pub external_controller_port: ExternalControllerStrategy,
    /// 混合端口策略
    pub mixed_port: PortStrategy,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Type, Builder)]
#[builder(default, derive(Debug, Serialize, Deserialize, Type))]
#[builder_struct_attr(serde_with::skip_serializing_none)]
#[serde(rename_all = "snake_case")]
pub struct ExternalControllerStrategy {
    pub host: IpAddr,
    pub port: PortStrategy,
}

impl Default for ExternalControllerStrategy {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: PortStrategy::new_allow_fallback(DEFAULT_EXTERNAL_CONTROLLER_PORT),
        }
    }
}

impl ExternalControllerStrategy {
    pub fn try_pick(&self) -> Result<SocketAddr, PickPortError> {
        let port = self.port.pick_and_try_port()?;
        Ok(SocketAddr::new(self.host, port))
    }

    pub fn try_pick_with_cached_port(&self) -> Option<SocketAddr> {
        self.port
            .cached_port()
            .map(|port| SocketAddr::new(self.host, port))
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum PortStrategyKind {
    Fixed,
    Random,
    #[default]
    AllowFallback,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub struct PortStrategy {
    /// 外部控制器端口策略类型
    pub kind: PortStrategyKind,
    /// 外部控制器端口起始端口
    ///
    /// 用于固定或允许回退策略
    pub start_port: u16,
    /// 此前缓存的 Port，用于避免重复选择相同的端口
    #[serde(skip)]
    cached_port: Arc<Mutex<Option<u16>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum PickPortError {
    #[error("Port {port} is not available")]
    PortNotAvailable { port: u16 },
    #[error("Can't find an open port")]
    NoOpenPort,
}

impl PortStrategy {
    /// 创建一个允许回退的端口策略
    pub fn new_allow_fallback(start_port: u16) -> Self {
        Self {
            kind: PortStrategyKind::AllowFallback,
            start_port,
            cached_port: Arc::new(Mutex::new(None)),
        }
    }

    /// 获取此前缓存的 Port
    pub fn cached_port(&self) -> Option<u16> {
        *self.cached_port.lock()
    }

    /// 选择并尝试端口
    #[tracing::instrument]
    pub fn pick_and_try_port(&self) -> Result<u16, PickPortError> {
        let port = match self.kind {
            PortStrategyKind::Fixed => {
                if !port_scanner::local_port_available(self.start_port) {
                    return Err(PickPortError::PortNotAvailable {
                        port: self.start_port,
                    });
                }
                self.start_port
            }
            PortStrategyKind::Random => {
                let new_port =
                    port_scanner::request_open_port().ok_or(PickPortError::NoOpenPort)?;
                if !port_scanner::local_port_available(new_port) {
                    return Err(PickPortError::PortNotAvailable { port: new_port });
                }
                new_port
            }
            PortStrategyKind::AllowFallback => {
                if port_scanner::local_port_available(self.start_port) {
                    self.start_port
                } else {
                    tracing::warn!(
                        "Port {} is not available, trying to find an open port",
                        self.start_port
                    );
                    let new_port =
                        port_scanner::request_open_port().ok_or(PickPortError::NoOpenPort)?;
                    new_port
                }
            }
        };

        self.cached_port.lock().insert(port);

        Ok(port)
    }
}
