use std::sync::atomic::{AtomicBool, Ordering};

use atomic_enum::atomic_enum;

use nyanpasu_ipc::types::{ServiceStatus, StatusInfo};
use nyanpasu_utils::runtime::block_on;
use serde::Serialize;
use tracing::instrument;

use crate::log_err;

use super::compat::ServiceCompat;

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[atomic_enum]
pub enum IpcState {
    Connected,
    Disconnected,
}

impl IpcState {
    pub fn is_connected(&self) -> bool {
        *self == IpcState::Connected
    }
}

static IPC_STATE: AtomicIpcState = AtomicIpcState::new(IpcState::Disconnected);
pub(super) static KILL_FLAG: AtomicBool = AtomicBool::new(false);
pub(super) static HEALTH_CHECK_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn get_ipc_state() -> IpcState {
    IPC_STATE.load(Ordering::Relaxed)
}

pub(super) fn set_ipc_state(state: IpcState) {
    IPC_STATE.store(state, Ordering::Relaxed);
    on_ipc_state_changed(state);
}

fn dispatch_disconnected() {
    // Strong CAS on purpose: a spurious `compare_exchange_weak` failure here
    // would leave a stale `Connected` past the accepted poll window and weaken
    // the fail-closed compat gate riding on this transition.
    if IPC_STATE
        .compare_exchange(
            IpcState::Connected,
            IpcState::Disconnected,
            Ordering::SeqCst,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        on_ipc_state_changed(IpcState::Disconnected)
    }
}

fn dispatch_connected() {
    if IPC_STATE
        .compare_exchange(
            IpcState::Disconnected,
            IpcState::Connected,
            Ordering::SeqCst,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        on_ipc_state_changed(IpcState::Connected)
    }
}

// TODO: it might be moved to outer scope?
#[instrument]
fn on_ipc_state_changed(state: IpcState) {
    tracing::info!("IPC state changed: {:?}", state);
    let enabled_service = {
        *crate::config::Config::verge()
            .latest()
            .enable_service_mode
            .as_ref()
            .unwrap_or(&false)
    };
    std::thread::spawn(move || {
        nyanpasu_utils::runtime::block_on(async move {
            if enabled_service {
                let (_, _, run_type) = crate::core::CoreManager::global().status().await;
                match (state, run_type) {
                    (IpcState::Connected, crate::core::RunType::Normal)
                    | (IpcState::Disconnected, crate::core::RunType::Service) => {
                        tracing::info!("Restarting core due to IPC state change");
                        log_err!(crate::core::CoreManager::global().run_core().await);
                    }
                    _ => {}
                }
            }
        })
    });
}

pub(super) fn spawn_health_check() {
    KILL_FLAG.store(false, Ordering::Relaxed);
    std::thread::spawn(|| {
        HEALTH_CHECK_RUNNING.store(true, Ordering::Release);
        block_on(async {
            loop {
                if KILL_FLAG.load(Ordering::Acquire) {
                    set_ipc_state(IpcState::Disconnected);
                    HEALTH_CHECK_RUNNING.store(false, Ordering::Release);
                    break;
                }
                health_check().await;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        })
    });
}

#[instrument]
async fn health_check() {
    match super::control::status().await {
        Ok(info) => {
            let (state, compat) = target_ipc_state(&info);
            if info.status == ServiceStatus::Running && !compat.allows_service_backend() {
                tracing::warn!(
                    ?compat,
                    "service daemon is incompatible; core will continue running on Local backend"
                );
            }
            match state {
                IpcState::Connected => dispatch_connected(),
                IpcState::Disconnected => dispatch_disconnected(),
            }
        }
        Err(e) => {
            tracing::error!("IPC health check failed: {}", e);
            dispatch_disconnected();
        }
    }
}

// TODO(actor-migration): compat gate lives on the legacy IpcState seam.
// Reason: CoreActor / CoreBackend do not exist until PR-5a.
// Remove when: PR-5c moves run-mode selection onto CoreClient::set_mode.
/// 纯函数：把一次 status 查询结果映射为目标 IpcState。
/// fail-closed —— 只有 daemon 在跑**且**通过兼容门禁才允许 Connected。
pub(super) fn target_ipc_state(info: &StatusInfo<'_>) -> (IpcState, ServiceCompat) {
    let compat = ServiceCompat::classify(info);
    let state = match info.status {
        ServiceStatus::Running if compat.allows_service_backend() => IpcState::Connected,
        _ => IpcState::Disconnected,
    };
    (state, compat)
}

#[cfg(test)]
mod tests {
    use nyanpasu_ipc::types::StatusInfo;

    use crate::core::{
        RunType,
        service::compat::{
            REQUIRED_SERVICE_MAJOR, STATUS_V1_4_5_FIXTURE, STATUS_V2_0_0_RC1_FIXTURE, ServiceCompat,
        },
    };

    use super::{IpcState, target_ipc_state};

    fn parse_fixture(fixture: &'static str) -> StatusInfo<'static> {
        serde_json::from_str(fixture).expect("status fixture must decode")
    }

    #[test]
    fn v1_daemon_never_reaches_service_backend() {
        let info = parse_fixture(STATUS_V1_4_5_FIXTURE);
        let (state, compat) = target_ipc_state(&info);

        assert!(state == IpcState::Disconnected);
        assert_eq!(
            compat,
            ServiceCompat::Incompatible {
                server_version: "1.4.5".to_owned(),
                required_major: REQUIRED_SERVICE_MAJOR,
            }
        );
        assert_eq!(RunType::classify(true, state), RunType::Normal);
    }

    #[test]
    fn v2_daemon_reaches_service_backend() {
        let info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        let (state, compat) = target_ipc_state(&info);

        assert!(state == IpcState::Connected);
        assert_eq!(
            compat,
            ServiceCompat::Compatible {
                server_version: "2.0.0-rc.1".to_owned(),
            }
        );
        assert_eq!(RunType::classify(true, state), RunType::Service);
    }
}
