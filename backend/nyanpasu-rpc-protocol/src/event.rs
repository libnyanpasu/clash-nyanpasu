use std::{fmt, str::FromStr};

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
    pub const ALL: [Self; 11] = [
        Self::AppUpdateStateChanged,
        Self::ClashWs,
        Self::ConfigurationStatusChanged,
        Self::CoreLogsChanged,
        Self::CoreStatusChanged,
        Self::SchemeRequestReceived,
        Self::ServiceStatusChanged,
        Self::StateChanged,
        Self::StorageResync,
        Self::StorageValueChanged,
        Self::WindowMessage,
    ];

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

impl fmt::Display for EventName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The name is not a shared event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownEventName;

impl fmt::Display for UnknownEventName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown event name")
    }
}

impl std::error::Error for UnknownEventName {}

impl FromStr for EventName {
    type Err = UnknownEventName;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|event| event.as_str() == name)
            .ok_or(UnknownEventName)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_name_parses_back_to_its_event() {
        for event in EventName::ALL {
            assert_eq!(event.as_str().parse(), Ok(event));
        }
        assert_eq!("missing".parse::<EventName>(), Err(UnknownEventName));
    }
}
