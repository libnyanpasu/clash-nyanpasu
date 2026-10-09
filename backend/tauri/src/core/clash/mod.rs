use nyanpasu_core::clash::ws::{ClashWsEventPayload, ClashWsUpdate};
use tauri_specta::Event;

use crate::core::status_events::ClashWsEvent;

pub mod connection_details;

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
    let logs_token = manager
        .state::<tokio_util::sync::CancellationToken>()
        .child_token();
    let tasks = manager
        .state::<tokio_util::task::TaskTracker>()
        .inner()
        .clone();
    tauri::async_runtime::spawn(nyanpasu_core::tasks::track_until_shutdown(
        &tasks,
        &logs_token,
        async move {
            while logs_rx.changed().await.is_ok() {
                let status = logs_rx.borrow_and_update().clone();
                if let Err(error) =
                    (crate::core::status_events::CoreLogsChanged { status }).emit(&logs_app)
                {
                    tracing::warn!(%error, "failed to emit Core log status");
                }
            }
        },
    ));
    let app = manager.app_handle().clone();
    let ws_task = tauri::async_runtime::spawn(async move {
        loop {
            let event = match ws_rx.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(_)) => match client.clash_ws_snapshot().await {
                    Ok(snapshot) => ClashWsEventPayload {
                        sequence: snapshot.sequence,
                        update: ClashWsUpdate::Reset(Box::new(snapshot)),
                    },
                    Err(error) => {
                        tracing::warn!(%error, "failed to resync Clash streams");
                        continue;
                    }
                },
                Err(RecvError::Closed) => break,
            };
            if let Err(error) = ClashWsEvent(event).emit(&app) {
                tracing::warn!(%error, "failed to emit Clash stream event");
            }
        }
    });
    manager.manage(StreamEventBridge(vec![ws_task]));
    Ok(())
}
