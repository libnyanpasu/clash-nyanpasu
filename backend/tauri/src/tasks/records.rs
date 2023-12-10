use chrono::Utc;

use super::task::{TaskEventID, TaskID, TaskRunResult, Timestamp};
use std::collections::HashMap;
pub type TaskEvents = HashMap<TaskEventID, TaskEvent>;

pub trait TaskEventsDispatcher {
    fn new() -> Self;
    fn new_event(&mut self, task_id: TaskID, event_id: TaskEventID) -> TaskEventID;
    fn dispatch(&mut self, event_id: TaskEventID, state: TaskEventState);
}

impl TaskEventsDispatcher for TaskEvents {
    fn new() -> Self {
        HashMap::new()
    }

    fn new_event(&mut self, task_id: TaskID, event_id: TaskEventID) -> TaskEventID {
        let mut event = TaskEvent {
            id: event_id,
            task_id,
            ..TaskEvent::default()
        };
        event.dispatch(TaskEventState::Pending);
        self.insert(event_id, event);
        event_id
    }

    fn dispatch(&mut self, event_id: TaskEventID, state: TaskEventState) {
        let event = self.get_mut(&event_id).unwrap();
        event.dispatch(state);
    }
}

pub struct TaskEvent {
    id: TaskEventID,
    task_id: TaskID,
    state: TaskEventState,
    timeline: HashMap<&'static str, Timestamp>,
}

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
            .insert(self.state.fmt(), Utc::now().timestamp_millis());
    }
}
