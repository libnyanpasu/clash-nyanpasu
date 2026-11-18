//! Declaration metadata, independent of linked command implementations.

/// Whether desktop dispatch needs a native wrapper or a shared handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// Requires a linked shell wrapper for desktop dispatch.
    Native,
    /// Uses explicit dependencies and a transport-supplied caller identity.
    Shared,
}

/// Reachable shell facilities; these declarations are not per-call authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellCapability {
    /// Clipboard reads and writes.
    Clipboard,
    /// Native notifications.
    Notification,
    /// Native message and file dialogs.
    Dialog,
    /// Native caller windows, webviews and bound channels.
    CallerWebview,
    /// Window creation and state.
    Window,
    /// Shell process restart, quit and crash operations.
    Lifecycle,
    /// Opening paths, URLs, editors and reporters.
    Opener,
    /// Tray and widget presentation.
    Presentation,
    /// The shell's deep-link queue.
    DeepLink,
    /// Application package updates.
    AppUpdate,
    /// Global shortcuts.
    Hotkey,
    /// Unprivileged system proxy and autostart integration.
    OsIntegration,
    /// Reachable elevation effects, governed by injected execution policy.
    Elevation,
    /// Host accent color queries.
    AccentColor,
    /// Host HTTP server control.
    HttpServer,
}
