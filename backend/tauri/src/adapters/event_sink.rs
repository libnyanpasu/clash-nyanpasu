//! Delivers committed state changes to every frontend through the shared
//! event bus.

use crate::adapters::events::StateChangedEvent;
use nyanpasu_core::client::{StateChanged, UiEventSink};
use nyanpasu_rpc::EventBus;

#[derive(Clone)]
pub struct BusUiEventSink {
    events: EventBus,
}

impl BusUiEventSink {
    pub fn new(events: EventBus) -> Self {
        Self { events }
    }
}

impl UiEventSink for BusUiEventSink {
    fn state_changed(&self, state: StateChanged) {
        // Every window: the tray menu and the editor read these queries too.
        crate::unified_rpc::publish(&self.events, &StateChangedEvent(state));
    }
}
