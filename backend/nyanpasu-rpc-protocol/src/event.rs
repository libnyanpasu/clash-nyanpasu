//! Stable shared event identities, independent of desktop wrappers.

use crate::{RpcTarget, Scope, ShellCapability};

const DESKTOP_ONLY: &[RpcTarget] = &[RpcTarget::Desktop];
const DESKTOP_AND_HTTP: &[RpcTarget] = &[RpcTarget::Desktop, RpcTarget::Http];
const ALL_TARGETS: &[RpcTarget] = &[RpcTarget::Desktop, RpcTarget::Http, RpcTarget::Local];

/// A shared event a shell publishes to its frontends. The names are the
/// Tauri event names the generated bindings listen on; the desktop shell
/// checks that the two lists agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventName {
    AppUpdateStateChanged,
    AppUpdateSummaryChanged,
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
    SyncStatusChanged,
    StorageResync,
    StorageValueChanged,
    WindowMessage,
}

impl EventName {
    /// All shared identities in desktop bridge registration order.
    pub const ALL: [Self; 17] = [
        Self::AppUpdateStateChanged,
        Self::AppUpdateSummaryChanged,
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
        Self::SyncStatusChanged,
    ];

    /// The existing event name used on every frontend transport.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AppUpdateStateChanged => "app-update-state-changed",
            Self::AppUpdateSummaryChanged => "app-update-summary-changed",
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
            Self::SyncStatusChanged => "sync-status-changed-event",
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
            | Self::AppUpdateSummaryChanged
            | Self::ConfigurationStatusSummaryChanged
            | Self::CoreLogsSummaryChanged
            | Self::CoreStatusSummaryChanged
            | Self::ServiceStatusSummaryChanged
            | Self::SchemeRequestReceived
            | Self::StateChanged
            | Self::StorageResync => Scope::Read,
            // The status carries blocked item paths, which are local names.
            Self::SyncStatusChanged => Scope::SensitiveRead,
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

    /// Look up an event identity by its wire event name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|event| event.as_str() == name)
    }

    /// Explicit transport exposure for this event identity.
    ///
    /// Window messages and deep-link notices belong strictly to the native desktop shell.
    pub fn targets(self) -> &'static [RpcTarget] {
        match self {
            Self::SchemeRequestReceived | Self::WindowMessage => DESKTOP_ONLY,
            Self::AppUpdateStateChanged
            | Self::ClashWs
            | Self::ConfigurationStatusChanged
            | Self::CoreLogsChanged
            | Self::CoreStatusChanged
            | Self::ServiceStatusChanged => ALL_TARGETS,
            Self::AppUpdateSummaryChanged
            | Self::ConfigurationStatusSummaryChanged
            | Self::CoreLogsSummaryChanged
            | Self::CoreStatusSummaryChanged
            | Self::ServiceStatusSummaryChanged
            | Self::StateChanged
            | Self::StorageResync
            | Self::StorageValueChanged => DESKTOP_AND_HTTP,
            // Sync is a device-local capability; a browser must not read it.
            Self::SyncStatusChanged => DESKTOP_ONLY,
        }
    }

    /// Shell capability required before a non-desktop transport may expose or deliver this event.
    pub fn capability(self) -> Option<ShellCapability> {
        match self {
            Self::AppUpdateStateChanged | Self::AppUpdateSummaryChanged => {
                Some(ShellCapability::AppUpdate)
            }
            Self::SchemeRequestReceived => Some(ShellCapability::DeepLink),
            Self::SyncStatusChanged => None,
            Self::WindowMessage => Some(ShellCapability::Window),
            Self::ClashWs
            | Self::ConfigurationStatusChanged
            | Self::ConfigurationStatusSummaryChanged
            | Self::CoreLogsChanged
            | Self::CoreLogsSummaryChanged
            | Self::CoreStatusChanged
            | Self::CoreStatusSummaryChanged
            | Self::ServiceStatusChanged
            | Self::ServiceStatusSummaryChanged
            | Self::StateChanged
            | Self::StorageResync
            | Self::StorageValueChanged => None,
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
                "app-update-summary-changed",
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
                "sync-status-changed-event",
            ]
        );
        assert!(by_scope(Scope::System).is_empty());
    }

    #[test]
    fn event_targets_keep_desktop_only_signals_off_remote_transports() {
        let by_target = |target| {
            EventName::ALL
                .into_iter()
                .filter(|event| event.targets().contains(&target))
                .map(EventName::as_str)
                .collect::<Vec<_>>()
        };
        assert_eq!(by_target(RpcTarget::Desktop).len(), EventName::ALL.len());
        let http = by_target(RpcTarget::Http);
        assert!(!http.contains(&"window-message-event"));
        assert!(!http.contains(&"scheme-request-received-event"));
        assert!(!http.contains(&"sync-status-changed-event"));
        assert_eq!(http.len(), EventName::ALL.len() - 3);

        let local = by_target(RpcTarget::Local);
        assert_eq!(
            local,
            [
                "app-update-state-changed",
                "clash-ws-event",
                "core-logs-changed",
                "configuration-status-changed",
                "core-status-changed-event",
                "service-status-changed-event",
            ]
        );
    }

    #[test]
    fn event_names_roundtrip_and_are_unique() {
        let mut names: Vec<_> = EventName::ALL.into_iter().map(EventName::as_str).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), EventName::ALL.len());
        for event in EventName::ALL {
            assert_eq!(EventName::from_name(event.as_str()), Some(event));
        }
        assert_eq!(EventName::from_name("not-an-event"), None);
    }
}
