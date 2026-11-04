use std::fmt;

use serde::{Deserialize, Serialize};

/// A transport a command can be exposed on. Each target has its own explicit
/// allowlist, which the registry enforces before dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RpcTarget {
    /// Tauri IPC from the desktop webview: the full desktop surface.
    Desktop,
    /// The WebUI over HTTP, opted into with `rpc(http)`.
    Http,
    /// The CLI over the local socket, opted into with `rpc(local)`.
    Local,
}

impl fmt::Display for RpcTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Desktop => "Tauri IPC",
            Self::Http => "HTTP",
            Self::Local => "the local socket",
        })
    }
}

/// A shell capability a command can reach, declared with
/// `rpc(capabilities(...))`. It is metadata about the command's reachable
/// effects, including effects its payload can trigger, not only what its
/// signature names; a shell that lacks a capability does not register the
/// command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellCapability {
    Clipboard,
    /// Native notifications.
    Notification,
    /// Native message and file dialogs.
    Dialog,
    /// The calling window or webview's identity, or a Channel bound to it.
    CallerWebview,
    /// Creating, sizing or otherwise managing windows.
    Window,
    /// Restarting, quitting or crashing the shell process.
    Lifecycle,
    /// Opening paths and URLs, editors and the issue reporter.
    Opener,
    /// The tray, the statistic widget and their icons.
    Presentation,
    /// The deep-link queue.
    DeepLink,
    /// Desktop package updates and their release channel.
    AppUpdate,
    /// Global shortcuts.
    Hotkey,
    /// Unprivileged OS integration: system proxy and autostart.
    OsIntegration,
    /// Anything that can end in a privilege prompt: the system service, DNS
    /// cache flushes, the app directory and installer fallbacks. A command that
    /// declares it can never be exposed over the local socket.
    Elevation,
}
