//! Stable shared event identities, independent of desktop wrappers.

use crate::permission::{self, ResourceKind, ResourceMetadata};

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

    /// The resource table row that admits this event. Its scope, exposure and capability are
    /// declared with the other resources; a test keeps every identity paired with one row.
    pub fn resource(self) -> &'static ResourceMetadata {
        permission::find(ResourceKind::Event, self.as_str())
            .unwrap_or_else(|| panic!("event `{}` has no resource row", self.as_str()))
    }

    /// Look up an event identity by its wire event name.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|event| event.as_str() == name)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::{RpcTarget, Scope, permission::RESOURCES};

    /// Whether `rows` name every event identity exactly once and nothing else.
    fn rows_match_events(rows: &[&str]) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for row in rows {
            if EventName::from_name(row).is_none() {
                return Err(format!("`{row}` is not an event identity"));
            }
            if !seen.insert(*row) {
                return Err(format!("`{row}` is declared twice"));
            }
        }
        match EventName::ALL
            .into_iter()
            .find(|event| !seen.contains(event.as_str()))
        {
            Some(event) => Err(format!("`{}` has no resource row", event.as_str())),
            None => Ok(()),
        }
    }

    fn event_rows() -> Vec<&'static str> {
        RESOURCES
            .iter()
            .filter(|resource| resource.kind == ResourceKind::Event)
            .map(|resource| resource.name)
            .collect()
    }

    #[test]
    fn every_event_has_one_row() {
        assert_eq!(rows_match_events(&event_rows()), Ok(()));
    }

    #[test]
    fn event_rows_reject_missing_extra_and_duplicate_events() {
        let mut rows = event_rows();
        let removed = rows.pop().unwrap();
        assert_eq!(
            rows_match_events(&rows),
            Err(format!("`{removed}` has no resource row"))
        );
        rows.push(removed);
        rows.push("not-an-event");
        assert_eq!(
            rows_match_events(&rows),
            Err("`not-an-event` is not an event identity".to_owned())
        );
        rows.pop();
        rows.push(removed);
        assert_eq!(
            rows_match_events(&rows),
            Err(format!("`{removed}` is declared twice"))
        );
    }

    #[test]
    fn event_scopes_are_pinned() {
        let by_scope = |scope| {
            EventName::ALL
                .into_iter()
                .filter(|event| event.resource().scope == scope)
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
                .filter(|event| event.resource().targets.contains(&target))
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
