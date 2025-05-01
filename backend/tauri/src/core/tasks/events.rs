use anyhow::Context;
use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use super::{
    storage::TaskStorage,
    task::{TaskEventID, TaskID, TaskRunResult, Timestamp},
    utils::Result,
};
use std::{collections::HashMap, sync::Arc};
pub struct TaskEvents {
    storage: Arc<Mutex<TaskStorage>>,
}

/// TaskEventDispatcher is a dispatcher for a task event,
/// currently, it's designed for a single thread task to dispatch event.
pub struct TaskEventDispatcher {
    storage: Arc<Mutex<TaskStorage>>,
    event: TaskEvent,
}

impl TaskEvents {
    pub fn new(storage: Arc<Mutex<TaskStorage>>) -> Self {
        TaskEvents { storage }
    }

    pub fn new_event(&self, task_id: TaskID, event_id: TaskEventID) -> Result<TaskEventDispatcher> {
        tracing::debug!("create new event: {:?} for task: {:?}", event_id, task_id);
        let mut dispatcher = {
            let storage = self.storage.lock();
            let event = TaskEvent {
                id: event_id,
                task_id,
                ..TaskEvent::default()
            };
            storage.add_event(&event).context("failed to add event")?;
            TaskEventDispatcher::new(self.storage.clone(), event)
        };
        dispatcher
            .dispatch(TaskEventState::Pending)
            .context("failed to dispatch pending event")?;
        Ok(dispatcher)
    }
}

impl TaskEventDispatcher {
    pub fn new(storage: Arc<Mutex<TaskStorage>>, event: TaskEvent) -> Self {
        TaskEventDispatcher { storage, event }
    }
    pub fn dispatch(&mut self, state: TaskEventState) -> Result<()> {
        tracing::debug!(
            "dispatch state: {:?} for event: {:?} of task: {:?}",
            state,
            self.event.id,
            self.event.task_id
        );
        self.event.dispatch(state);
        let storage = self.storage.lock();
        storage.update_event(&self.event)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TaskEvent {
    pub id: TaskEventID,
    pub task_id: TaskID,
    pub state: TaskEventState,
    pub timeline: HashMap<String, Timestamp>,
    pub updated_at: Timestamp,
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
            updated_at: Utc::now().timestamp_millis(),
        }
    }
}

impl TaskEvent {
    fn dispatch(&mut self, state: TaskEventState) {
        let now = Utc::now().timestamp_millis();
        self.state = state;
        self.timeline.insert(self.state.fmt().into(), now);
        self.updated_at = now;
    }
}
