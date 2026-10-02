use tauri_specta::Event;

pub mod api;
pub mod connection_details;
pub mod connection_rates;
pub mod proxies;
pub mod ws;

// TODO: support system path search via a config or flag
// FIXME: move this fn to nyanpasu-utils
/// Search the binary path of the core. See [`binary_candidates`] for the
/// search locations and their priority.
pub fn find_binary_path(
    core_type: &nyanpasu_utils::core::CoreType,
) -> std::io::Result<std::path::PathBuf> {
    binary_candidates(core_type)
        .into_iter()
        .find(|path| path.exists())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} not found", core_type.get_executable_name()),
            )
        })
}

/// Candidate core binary paths in priority order: data dir -> install dir.
/// Dev builds additionally register the downloaded `externalBin` sidecars
/// (`<crate>/sidecar/`): unlike `tauri build`, where the bundler copies the
/// sidecars next to the executable stripping the target-triple suffix,
/// `tauri dev` copies nothing, so neither the app's exe dir nor Tauri's own
/// sidecar lookup can find the cores there.
fn binary_candidates(core_type: &nyanpasu_utils::core::CoreType) -> Vec<std::path::PathBuf> {
    let name = core_type.get_executable_name();
    let mut candidates = Vec::new();
    if let Ok(data_dir) = crate::utils::dirs::app_data_dir() {
        candidates.push(data_dir.join(name));
    }
    if let Ok(app_dir) = crate::utils::dirs::app_install_dir() {
        candidates.push(app_dir.join(name));
    }
    #[cfg(debug_assertions)]
    if let Some(path) = dev_sidecar_binary_path(core_type) {
        candidates.push(path);
    }
    candidates
}

/// The sidecars keep the bundler's input naming `<name>-<target_triple>`
/// (see `tauri::utils::platform::target_triple`).
#[cfg(debug_assertions)]
fn dev_sidecar_binary_path(
    core_type: &nyanpasu_utils::core::CoreType,
) -> Option<std::path::PathBuf> {
    let name = core_type.get_executable_name();
    let stem = name
        .strip_suffix(std::env::consts::EXE_SUFFIX)
        .unwrap_or(name);
    let triple = tauri::utils::platform::target_triple().ok()?;
    Some(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("sidecar")
            .join(format!("{stem}-{triple}{}", std::env::consts::EXE_SUFFIX)),
    )
}

// Tauri owns only event-forwarding tasks; stream state lives in StreamsActor.
struct StreamEventBridge(Vec<tauri::async_runtime::JoinHandle<()>>);
impl Drop for StreamEventBridge {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(manager: &M) -> anyhow::Result<()> {
    use tokio::sync::broadcast::error::RecvError;
    manager.manage(connection_details::ConnectionDetailSubscriptions::new());
    let client = manager
        .state::<crate::client::NyanpasuClient>()
        .inner()
        .clone();
    let mut ws_rx = client.subscribe_clash_ws();
    let mut logs_rx = client.subscribe_core_logs();
    let logs_app = manager.app_handle().clone();
    let logs_token = client.shutdown_child_token();
    client.spawn_tracked(&logs_token, async move {
        while logs_rx.changed().await.is_ok() {
            let status = logs_rx.borrow_and_update().clone();
            if let Err(error) = (crate::core::logs::CoreLogsChanged { status }).emit(&logs_app) {
                tracing::warn!(%error, "failed to emit Core log status");
            }
        }
    });
    let app = manager.app_handle().clone();
    let ws_task = tauri::async_runtime::spawn(async move {
        loop {
            let event = match ws_rx.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => match client.clash_ws_snapshot().await {
                    Ok(snapshot) => ws::ClashWsEvent {
                        sequence: snapshot.sequence,
                        update: ws::ClashWsUpdate::Reset(Box::new(snapshot)),
                    },
                    Err(error) => {
                        tracing::warn!(%error, "failed to resync Clash streams");
                        continue;
                    }
                },
                Err(RecvError::Closed) => break,
            };
            if let Err(error) = event.emit(&app) {
                tracing::warn!(%error, "failed to emit Clash stream event");
            }
        }
    });
    manager.manage(StreamEventBridge(vec![ws_task]));
    Ok(())
}
