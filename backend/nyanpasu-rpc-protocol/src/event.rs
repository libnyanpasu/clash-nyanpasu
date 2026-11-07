//! Stable shared event identities, independent of desktop wrappers.

/// A shared event a shell publishes to its frontends. The names are the
/// Tauri event names the generated bindings listen on; the desktop shell
/// checks that the two lists agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventName {
    AppUpdateStateChanged,
    ClashWs,
    ConfigurationStatusChanged,
    CoreLogsChanged,
    CoreStatusChanged,
    SchemeRequestReceived,
    ServiceStatusChanged,
    StateChanged,
    StorageResync,
    StorageValueChanged,
    WindowMessage,
}

impl EventName {
    /// All shared identities in desktop bridge registration order.
    pub const ALL: [Self; 11] = [
        Self::AppUpdateStateChanged,
        Self::ClashWs,
        Self::CoreLogsChanged,
        Self::ConfigurationStatusChanged,
        Self::CoreStatusChanged,
        Self::SchemeRequestReceived,
        Self::ServiceStatusChanged,
        Self::StorageValueChanged,
        Self::StorageResync,
        Self::WindowMessage,
        Self::StateChanged,
    ];

    /// The existing event name used on every frontend transport.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppUpdateStateChanged => "app-update-state-changed",
            Self::ClashWs => "clash-ws-event",
            Self::ConfigurationStatusChanged => "configuration-status-changed",
            Self::CoreLogsChanged => "core-logs-changed",
            Self::CoreStatusChanged => "core-status-changed-event",
            Self::SchemeRequestReceived => "scheme-request-received-event",
            Self::ServiceStatusChanged => "service-status-changed-event",
            Self::StateChanged => "state-changed-event",
            Self::StorageResync => "storage-resync-event",
            Self::StorageValueChanged => "storage-value-changed-event",
            Self::WindowMessage => "window-message-event",
        }
    }
}
