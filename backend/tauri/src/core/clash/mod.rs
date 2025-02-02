use backon::ExponentialBuilder;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

pub mod api;
pub mod core;
pub mod proxies;
pub mod ws;

pub static CLASH_API_DEFAULT_BACKOFF_STRATEGY: Lazy<ExponentialBuilder> = Lazy::new(|| {
    ExponentialBuilder::default()
        .with_min_delay(std::time::Duration::from_millis(50))
        .with_max_delay(std::time::Duration::from_secs(5))
        .with_max_times(5)
});

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
            ws_connector.start().await.unwrap();
        }
        let mut rx = ws_connector.subscribe();
        while let Ok(event) = rx.recv().await {
            ClashConnectionsEvent(event).emit(&app_handle).unwrap();
        }
    });
    Ok(())
}
