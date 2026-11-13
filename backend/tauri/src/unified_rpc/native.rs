//! Native wrappers and desktop binding over the shared audited registry.

use std::{collections::HashMap, sync::Arc};

use nyanpasu_rpc_protocol::{RpcError, RpcValue};
use serde_json::Value;
use tauri::Manager as _;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use nyanpasu_rpc::{
    audit::RpcTransport,
    registry::{RpcCaller, RpcFuture, SetupError, UnifiedRpc},
};

pub type TauriRpcHandler =
    fn(tauri::AppHandle, tauri::Window, tauri::Webview, Value) -> Result<RpcFuture, RpcError>;

pub struct TauriCommandEntry {
    pub name: &'static str,
    pub handler: TauriRpcHandler,
}

inventory::collect!(TauriCommandEntry);

/// Native handler inventory is separate from the neutral registry and HTTP routes.
#[derive(Clone)]
pub struct DesktopRpc {
    rpc: UnifiedRpc,
    handlers: Arc<HashMap<&'static str, TauriRpcHandler>>,
    pub(super) tasks: TaskTracker,
    pub(super) shutdown: CancellationToken,
}

pub(super) fn register_native_commands(
    entries: impl IntoIterator<Item = &'static TauriCommandEntry>,
) -> Result<Arc<HashMap<&'static str, TauriRpcHandler>>, SetupError> {
    let mut handlers = HashMap::new();
    for entry in entries {
        if handlers.insert(entry.name, entry.handler).is_some() {
            return Err(SetupError::RegisterCommand { name: entry.name });
        }
    }
    Ok(Arc::new(handlers))
}

/// Return the names of all native Tauri wrappers linked into this desktop binary.
pub(crate) fn linked_command_names() -> impl Iterator<Item = &'static str> {
    inventory::iter::<TauriCommandEntry>
        .into_iter()
        .map(|entry| entry.name)
}

impl DesktopRpc {
    pub fn new(
        mut rpc: UnifiedRpc,
        tasks: TaskTracker,
        shutdown: CancellationToken,
    ) -> Result<Self, SetupError> {
        let handlers = register_native_commands(inventory::iter::<TauriCommandEntry>)?;
        rpc.mark_native_linked(handlers.keys().copied());
        Ok(Self {
            rpc,
            tasks,
            shutdown,
            handlers,
        })
    }

    pub(super) fn icon_dependencies(&self) -> Arc<nyanpasu_rpc::registry::RpcDependencies> {
        self.rpc.dependencies()
    }

    pub(super) async fn call(
        &self,
        app: tauri::AppHandle,
        window: tauri::Window,
        webview: tauri::Webview,
        method: &str,
        params: Value,
    ) -> Result<Value, RpcError> {
        let caller = RpcCaller::desktop(window.label());
        if let Some(handler) = self.handlers.get(method) {
            return self
                .rpc
                .call_native(method, caller, || handler(app, window, webview, params))
                .await;
        }
        self.rpc
            .call(RpcTransport::TauriIpc, method, caller, params)
            .await
    }
}

// This is the transport entrypoint, not an application operation. All
// application commands must use #[nyanpasu_macro::rpc] and its registry.
#[tauri::command]
#[specta::specta]
pub async fn call_rpc(
    app: tauri::AppHandle,
    window: tauri::Window,
    webview: tauri::Webview,
    method: String,
    params: RpcValue,
) -> Result<RpcValue, RpcError> {
    let rpc = app.state::<DesktopRpc>();
    rpc.call(app.clone(), window, webview, &method, params.0)
        .await
        .map(RpcValue)
}
