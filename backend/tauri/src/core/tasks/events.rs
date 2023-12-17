use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::storage::EventsGuard;
use super::task::{TaskEventID, TaskID, TaskRunResult, Timestamp};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
pub struct TaskEvents;

pub trait TaskEventsDispatcher {
    fn new() -> Self;
    fn new_event(&self, task_id: TaskID, event_id: TaskEventID) -> TaskEventID;
    fn dispatch(&self, event_id: TaskEventID, state: TaskEventState);
}

impl TaskEvents {
    pub fn global() -> &'static Arc<Self> {
        static EVENTS: OnceLock<Arc<TaskEvents>> = OnceLock::new();

        EVENTS.get_or_init(|| Arc::new(Self::new()))
    }
}

impl TaskEventsDispatcher for TaskEvents {
    fn new() -> Self {
        TaskEvents {}
    }

    fn new_event(&self, task_id: TaskID, event_id: TaskEventID) -> TaskEventID {
        let mut event = TaskEvent {
            id: event_id,
            task_id,
            ..TaskEvent::default()
        };
        event.dispatch(TaskEventState::Pending);
        EventsGuard::global().add_event(&event).unwrap();
        event_id
    }

    fn dispatch(&self, event_id: TaskEventID, state: TaskEventState) {
        let mut event = EventsGuard::global().get_event(event_id).unwrap().unwrap(); // unwrap because it should be exist here, if not, it's a bug
        event.dispatch(state);
        EventsGuard::global().update_event(&event).unwrap();
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TaskEvent {
    pub id: TaskEventID,
    pub task_id: TaskID,
    pub state: TaskEventState,
    pub timeline: HashMap<String, Timestamp>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum TaskEventState {
    Pending, // added to the queue, alias of created
    Running,
    Finished(TaskRunResult),
    Cancelled,
}

impl TaskEventState {
    pub fn fmt(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Finished(_) => "finished",
            Self::Cancelled => "cancelled",
        }
    }
}

impl Default for TaskEvent {
    fn default() -> Self {
        TaskEvent {
            id: 0,
            task_id: 0,
            state: TaskEventState::Pending,
            timeline: HashMap::with_capacity(4), // 4 states
        }
    }
}

impl TaskEvent {
    fn dispatch(&mut self, state: TaskEventState) {
        self.state = state;
        self.timeline
            .insert(self.state.fmt().into(), Utc::now().timestamp_millis());
    }
}
