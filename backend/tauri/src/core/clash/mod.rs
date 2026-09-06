use backon::ExponentialBuilder;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

pub mod api;
pub mod proxies;
pub mod ws;

pub static CLASH_API_DEFAULT_BACKOFF_STRATEGY: Lazy<ExponentialBuilder> = Lazy::new(|| {
    ExponentialBuilder::default()
        .with_min_delay(std::time::Duration::from_millis(50))
        .with_max_delay(std::time::Duration::from_secs(5))
        .with_max_times(5)
});

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

#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct ClashConnectionsEvent(pub ws::ClashConnectionsConnectorEvent);

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(manager: &M) -> anyhow::Result<()> {
    let ws_connector = ws::ClashConnectionsConnector::new();
    manager.manage(ws_connector.clone());
    let app_handle = manager.app_handle().clone();

    tauri::async_runtime::spawn(async move {
        // TODO: refactor it while clash core manager use tauri event dispatcher to notify the core state changed
        {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;

            // TODO: clash-rs ws authorization is not working
            match ws_connector.start().await {
                Ok(_) => {
                    tracing::info!(
                        "ws_connector started successfully clash-rs may be errored here."
                    );
                }
                // TODO: wait for clash-rs to fix
                Err(e) => {
                    tracing::error!("ws_connector failed to start: {:?}", e);
                }
            }
        }
        let mut rx = ws_connector.subscribe();
        let mut ws_rx = ws_connector.subscribe_ws();
        let ws_app_handle = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok(event) = ws_rx.recv().await {
                event.emit(&ws_app_handle).unwrap();
            }
        });
        while let Ok(event) = rx.recv().await {
            ClashConnectionsEvent(event).emit(&app_handle).unwrap();
        }
    });
    Ok(())
}
