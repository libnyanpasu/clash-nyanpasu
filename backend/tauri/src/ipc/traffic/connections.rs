//! The desktop-only `traffic.connections` commands. The shared ones are in
//! `nyanpasu_rpc::commands::traffic::connections`.

use nyanpasu_core::client::NyanpasuClient;
use nyanpasu_rpc::error::Result;

// Subscription control uses UnifiedRpc; only frame delivery is a native Channel.
// HTTP consumers use the dedicated SSE adapter instead of a webview channel.
#[nyanpasu_rpc::rpc(owner, capabilities(caller_webview), access = read, expose = [])]
#[tauri::command]
#[specta::specta]
pub async fn subscribe_clash_connection_details(
    window: tauri::Window,
    client: tauri::State<'_, NyanpasuClient>,
    subscriptions: tauri::State<
        '_,
        crate::adapters::connection_details::ConnectionDetailSubscriptions,
    >,
    on_frame: tauri::ipc::Channel<std::sync::Arc<nyanpasu_core::clash::ws::ClashConnectionDetails>>,
) -> Result<crate::adapters::connection_details::SubscriptionId> {
    let receiver = client.subscribe_clash_connection_details().await?;
    let parent = client.shutdown_child_token();
    let (id, cancel) = subscriptions.register(&parent, window.label().to_string());
    tokio::spawn(client.track_until_shutdown(
        &cancel,
        crate::adapters::connection_details::forward_details(receiver, on_frame),
    ));
    Ok(id)
}

#[nyanpasu_rpc::rpc(owner, capabilities(caller_webview), access = read, expose = [])]
#[tauri::command]
#[specta::specta]
pub fn unsubscribe_clash_connection_details(
    window: tauri::Window,
    subscriptions: tauri::State<
        '_,
        crate::adapters::connection_details::ConnectionDetailSubscriptions,
    >,
    id: crate::adapters::connection_details::SubscriptionId,
) -> Result {
    subscriptions.unsubscribe(id, window.label())?;
    Ok(())
}
