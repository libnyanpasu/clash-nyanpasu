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
            // Latched in the poll loop rather than in a static: entering the
            // incompatible state is worth one line, staying in it is not, and
            // the loop is the only thing that lives as long as the state does.
            let mut warned_incompatible = false;
            loop {
                if KILL_FLAG.load(Ordering::Acquire) {
                    set_ipc_state(IpcState::Disconnected);
                    HEALTH_CHECK_RUNNING.store(false, Ordering::Release);
                    break;
                }
                warned_incompatible = health_check(warned_incompatible).await;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        })
    });
}

/// What level (if any) `health_check` should log the "daemon incompatible"
/// message at this poll.
#[derive(Debug, PartialEq, Eq)]
enum WarnLevel {
    Warn,
    Debug,
    Silent,
}

/// Pure decision behind the warning latch: given the latch carried from the
/// previous poll and whether this poll found the daemon incompatible-but-
/// running, decide the log level for this poll and the latch to carry into
/// the next one. A probe that can't reach the daemon at all is fed `false`
/// here, the same as a compatible daemon would be: it stays silent and clears
/// the latch, so a later recurrence of the incompatible state warns again.
fn next_incompatible_warning_state(
    warned: bool,
    incompatible_but_running: bool,
) -> (WarnLevel, bool) {
    match (incompatible_but_running, warned) {
        (true, false) => (WarnLevel::Warn, true),
        (true, true) => (WarnLevel::Debug, true),
        (false, _) => (WarnLevel::Silent, false),
    }
}

/// `warned` says whether the previous poll already reported an incompatible
/// daemon; the return value carries that forward. Without it a permanently
/// incompatible daemon warns once per 5s poll for as long as the app runs,
/// which buries every other warning in the log.
#[instrument]
async fn health_check(warned: bool) -> bool {
    match super::control::status().await {
        Ok(info) => {
            let (state, compat) = target_ipc_state(&info);
            let incompatible_but_running =
                info.status == ServiceStatus::Running && !compat.allows_service_backend();
            let (level, next_warned) =
                next_incompatible_warning_state(warned, incompatible_but_running);
            match level {
                WarnLevel::Warn => tracing::warn!(
                    ?compat,
                    "service daemon is incompatible; core will continue running on Local backend"
                ),
                WarnLevel::Debug => tracing::debug!(
                    ?compat,
                    "service daemon is incompatible; core will continue running on Local backend"
                ),
                WarnLevel::Silent => {}
            }
            match state {
                IpcState::Connected => dispatch_connected(),
                IpcState::Disconnected => dispatch_disconnected(),
            }
            next_warned
        }
        Err(e) => {
            tracing::error!("IPC health check failed: {}", e);
            dispatch_disconnected();
            // The daemon stopped answering, so route it through the same
            // decision as a compatible probe: the latch clears, and a
            // recurring incompatible reading warns again.
            let (_, next_warned) = next_incompatible_warning_state(warned, false);
            next_warned
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

    use super::{IpcState, WarnLevel, next_incompatible_warning_state, target_ipc_state};

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

    #[test]
    fn entering_incompatible_state_warns_once() {
        let (level, warned) = next_incompatible_warning_state(false, true);

        assert_eq!(level, WarnLevel::Warn);
        assert!(warned);
    }

    #[test]
    fn staying_incompatible_only_debug_logs() {
        let (level, warned) = next_incompatible_warning_state(true, true);

        assert_eq!(level, WarnLevel::Debug);
        assert!(warned);
    }

    #[test]
    fn leaving_incompatible_state_clears_latch() {
        let (level, warned) = next_incompatible_warning_state(true, false);

        assert_eq!(level, WarnLevel::Silent);
        assert!(!warned);
    }

    #[test]
    fn unreachable_probe_clears_latch_so_recurrence_warns_again() {
        // health_check's Err arm feeds `false` regardless of the prior
        // latch, mirroring an unreachable daemon.
        let (level, warned) = next_incompatible_warning_state(true, false);
        assert_eq!(level, WarnLevel::Silent);
        assert!(!warned);

        // The next poll finding the daemon incompatible again must warn,
        // not debug-log, because the latch was cleared above.
        let (level, warned) = next_incompatible_warning_state(warned, true);
        assert_eq!(level, WarnLevel::Warn);
        assert!(warned);
    }

    #[test]
    fn compatible_daemon_never_warns() {
        assert_eq!(
            next_incompatible_warning_state(false, false),
            (WarnLevel::Silent, false)
        );
        assert_eq!(
            next_incompatible_warning_state(true, false),
            (WarnLevel::Silent, false)
        );
    }
}
