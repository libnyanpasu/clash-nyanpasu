//! Stable shared event identities, independent of desktop wrappers.

use crate::Scope;

/// A shared event a shell publishes to its frontends. The names are the
/// Tauri event names the generated bindings listen on; the desktop shell
/// checks that the two lists agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventName {
    AppUpdateStateChanged,
    ClashWs,
    ConfigurationStatusChanged,
    ConfigurationStatusSummaryChanged,
    CoreLogsChanged,
    CoreLogsSummaryChanged,
    CoreStatusChanged,
    CoreStatusSummaryChanged,
    SchemeRequestReceived,
    ServiceStatusChanged,
    ServiceStatusSummaryChanged,
    StateChanged,
    StorageResync,
    StorageValueChanged,
    WindowMessage,
}

impl EventName {
    /// All shared identities in desktop bridge registration order.
    pub const ALL: [Self; 15] = [
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
        Self::ConfigurationStatusSummaryChanged,
        Self::CoreStatusSummaryChanged,
        Self::ServiceStatusSummaryChanged,
        Self::CoreLogsSummaryChanged,
    ];

    /// The existing event name used on every frontend transport.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppUpdateStateChanged => "app-update-state-changed",
            Self::ClashWs => "clash-ws-event",
            Self::ConfigurationStatusChanged => "configuration-status-changed",
            Self::ConfigurationStatusSummaryChanged => "configuration-status-summary-changed",
            Self::CoreLogsChanged => "core-logs-changed",
            Self::CoreLogsSummaryChanged => "core-logs-summary-changed",
            Self::CoreStatusChanged => "core-status-changed-event",
            Self::CoreStatusSummaryChanged => "core-status-summary-changed-event",
            Self::SchemeRequestReceived => "scheme-request-received-event",
            Self::ServiceStatusChanged => "service-status-changed-event",
            Self::ServiceStatusSummaryChanged => "service-status-summary-changed-event",
            Self::StateChanged => "state-changed-event",
            Self::StorageResync => "storage-resync-event",
            Self::StorageValueChanged => "storage-value-changed-event",
            Self::WindowMessage => "window-message-event",
        }
    }

    /// The scope a principal needs to receive this event's payload. Reviewed
    /// per variant against what the payload carries, not against its name.
    /// This is metadata only: event admission is decided by the common
    /// authorization layer, not here.
    pub fn scope(self) -> Scope {
        match self {
            // Structural values, invalidation signals and an empty deep-link
            // notice (it never carries the link) hold no free text. Whether a
            // desktop-only signal may reach another transport is the separate
            // question of event exposure, decided with admission.
            Self::ClashWs
            | Self::ConfigurationStatusSummaryChanged
            | Self::CoreLogsSummaryChanged
            | Self::CoreStatusSummaryChanged
            | Self::ServiceStatusSummaryChanged
            | Self::SchemeRequestReceived
            | Self::StateChanged
            | Self::StorageResync => Scope::Read,
            // Full status values carry owner diagnostic text, endpoints and
            // controller details; the update state carries unaudited error and
            // endpoint text; the core-log status carries an error message; a
            // storage change carries an arbitrary stored value; an inter-window
            // message carries an arbitrary payload.
            Self::AppUpdateStateChanged
            | Self::ConfigurationStatusChanged
            | Self::CoreLogsChanged
            | Self::CoreStatusChanged
            | Self::ServiceStatusChanged
            | Self::StorageValueChanged
            | Self::WindowMessage => Scope::SensitiveRead,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_scopes_are_pinned() {
        let by_scope = |scope| {
            EventName::ALL
                .into_iter()
                .filter(|event| event.scope() == scope)
                .map(EventName::as_str)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            by_scope(Scope::Read),
            [
                "clash-ws-event",
                "scheme-request-received-event",
                "storage-resync-event",
                "state-changed-event",
                "configuration-status-summary-changed",
                "core-status-summary-changed-event",
                "service-status-summary-changed-event",
                "core-logs-summary-changed",
            ]
        );
        assert_eq!(
            by_scope(Scope::SensitiveRead),
            [
                "app-update-state-changed",
                "core-logs-changed",
                "configuration-status-changed",
                "core-status-changed-event",
                "service-status-changed-event",
                "storage-value-changed-event",
                "window-message-event",
            ]
        );
        assert!(by_scope(Scope::System).is_empty());
    }

    #[test]
    fn event_names_are_unique() {
        let mut names: Vec<_> = EventName::ALL.into_iter().map(EventName::as_str).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), EventName::ALL.len());
    }
}
