//! Core and service status values published by their application owners.

use nyanpasu_ipc::types::ServiceStatus;

use crate::execution_host::ExecutionHost;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EndpointConnectivity {
    Connected,
    ShutDown,
    HandingOff {
        from: ExecutionHost,
        to: ExecutionHost,
    },
    /// The endpoint is unreachable. `desired` names the committed host; the
    /// router never falls back on its own.
    Degraded {
        desired: ExecutionHost,
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CoreStatusInfo {
    pub controller: Option<nyanpasu_ipc::api::status::CoreControllerInfo>,
    pub host: ExecutionHost,
    pub connectivity: EndpointConnectivity,
    pub generation: u64,
    pub state: Option<nyanpasu_ipc::api::status::CoreStateDetail>,
    pub state_changed_at: i64,
    pub revision: Option<nyanpasu_ipc::api::status::RevisionIdInfo>,
    pub healthy: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ServicePhase {
    Probing,
    NotInstalled,
    DaemonStopped,
    Installing,
    StartingDaemon,
    Ready,
    /// Version gate failed closed: upgrade required, never downgraded to.
    Incompatible,
    Restarting,
    /// Auto-restart budget spent; waits for `EnsureReady` or a successful explicit start.
    Exhausted,
    Uninstalling,
    /// The probe itself failed or timed out (F5): what the daemon actually
    /// is cannot be determined. Never treated as `DaemonStopped` -- an
    /// unreachable daemon might still be holding a core open, so callers
    /// that gate on "no core held" (the uninstall guard, `RecoverEndpoint`'s
    /// restart) must refuse rather than proceed.
    Unknown,
}

impl ServicePhase {
    /// Whether this phase settles if the daemon can serve: `None` for a
    /// transition still under way and for a probe that could not tell.
    pub fn readiness(self) -> Option<bool> {
        match self {
            Self::Ready => Some(true),
            Self::NotInstalled | Self::DaemonStopped | Self::Incompatible | Self::Exhausted => {
                Some(false)
            }
            Self::Probing
            | Self::Installing
            | Self::StartingDaemon
            | Self::Restarting
            | Self::Uninstalling
            | Self::Unknown => None,
        }
    }
}

/// The watch projection (UI settings page + facade). Daemon state, never a
/// second copy of core state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ServiceHostStatus {
    pub name: std::borrow::Cow<'static, str>,
    pub version: std::borrow::Cow<'static, str>,
    pub status: ServiceStatus,
    pub server: Option<nyanpasu_ipc::api::status::StatusResBody<'static>>,
    pub phase: ServicePhase,
    pub compat: ServiceCompat,
    pub restart_attempts: u8,
    /// The last settled [`ServicePhase::readiness`]: a transition or an
    /// unreadable probe leaves it as it was. Internal, like the generation.
    #[serde(skip)]
    pub settled_ready: bool,
    /// How many times `settled_ready` has turned true. A daemon that
    /// restarted between two reads is a new daemon even when both say
    /// `Ready`; this is what tells them apart. Internal: not part of the UI
    /// projection.
    #[serde(skip)]
    pub ready_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ServiceCompat {
    /// daemon 未安装 / 未运行 / 未上报 server 信息，没有可判定的版本。
    Unknown,
    /// 主版本匹配，允许进入 Service backend。
    Compatible { server_version: String },
    /// 主版本不匹配（典型：v1.4.5），或主版本对但低于最低版本（典型：
    /// v2.0.0-rc.1）。fail-closed。
    Incompatible {
        server_version: String,
        required_major: u64,
        /// 供 UI 展示：只说"需要 v2.x"无法解释一台 major 正确却被拒的
        /// daemon，它装的**就是** v2.x。
        required_min: String,
    },
    /// server 上报的版本不是合法 semver。fail-closed。
    Unparsable { server_version: String },
}
