use std::{
    net::{IpAddr, Ipv4Addr},
    ops::Deref,
};

use crate::clash::config::DEFAULT_EXTERNAL_CONTROLLER_PORT;
use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, Type)))]
pub struct ClashStrategy {
    /// 外部控制器端口策略
    pub external_controller: ExternalControllerStrategy,
    /// 混合端口策略
    pub mixed_port: PortStrategy,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq, Type)))]
#[patch(attribute(serde(rename_all = "snake_case")))]
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
    pub fn try_pick(&self) -> Result<(IpAddr, PickedPort), PickPortError> {
        let port = self.port.pick_and_try_port()?;
        Ok((self.host, port))
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

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub struct PortStrategy {
    /// 外部控制器端口策略类型
    pub kind: PortStrategyKind,
    /// 外部控制器端口起始端口
    ///
    /// 用于固定或允许回退策略
    pub start_port: u16,
}

impl Eq for PortStrategy {}

impl PartialEq for PortStrategy {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind && self.start_port == other.start_port
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PickPortError {
    #[error("Port {port} is not available")]
    PortNotAvailable { port: u16 },
    #[error("Can't find an open port")]
    NoOpenPort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
pub enum PickedPort {
    Original(u16),
    Fallback(u16),
}

impl AsRef<u16> for PickedPort {
    fn as_ref(&self) -> &u16 {
        match self {
            PickedPort::Original(port) | PickedPort::Fallback(port) => port,
        }
    }
}

impl Deref for PickedPort {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl PortStrategy {
    /// 创建一个允许回退的端口策略
    pub fn new_allow_fallback(start_port: u16) -> Self {
        Self {
            kind: PortStrategyKind::AllowFallback,
            start_port,
        }
    }

    /// 选择并尝试端口
    #[tracing::instrument]
    pub fn pick_and_try_port(&self) -> Result<PickedPort, PickPortError> {
        let port = match self.kind {
            PortStrategyKind::Fixed => {
                if !port_scanner::local_port_available(self.start_port) {
                    return Err(PickPortError::PortNotAvailable {
                        port: self.start_port,
                    });
                }
                PickedPort::Original(self.start_port)
            }
            PortStrategyKind::Random => {
                let new_port =
                    port_scanner::request_open_port().ok_or(PickPortError::NoOpenPort)?;
                if !port_scanner::local_port_available(new_port) {
                    return Err(PickPortError::PortNotAvailable { port: new_port });
                }
                PickedPort::Fallback(new_port)
            }
            PortStrategyKind::AllowFallback => {
                if port_scanner::local_port_available(self.start_port) {
                    PickedPort::Original(self.start_port)
                } else {
                    let new_port =
                        port_scanner::request_open_port().ok_or(PickPortError::NoOpenPort)?;
                    PickedPort::Fallback(new_port)
                }
            }
        };

        Ok(port)
    }
}
