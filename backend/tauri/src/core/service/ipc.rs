use std::sync::atomic::{AtomicBool, Ordering};

use atomic_enum::atomic_enum;

use nyanpasu_ipc::types::ServiceStatus;
use nyanpasu_utils::runtime::block_on;
use serde::Serialize;
use tracing::instrument;

use crate::log_err;

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
    if IPC_STATE
        .compare_exchange_weak(
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
        .compare_exchange_weak(
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
        *crate::config::ConfigService::verge()
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
        Ok(info) => match info.status {
            ServiceStatus::Running => {
                dispatch_connected();
            }
            ServiceStatus::Stopped | ServiceStatus::NotInstalled => {
                dispatch_disconnected();
            }
        },
        Err(e) => {
            tracing::error!("IPC health check failed: {}", e);
            dispatch_disconnected();
        }
    }
}
