// store is a interface to save and restore task states
use super::task::{TaskEventID, TaskManager};
use super::{events::TaskEvent, task::TaskID};
use crate::core::storage::Storage;
use crate::core::tasks::task::Task;
use log::debug;
use std::sync::{Arc, OnceLock};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TaskStorageError {
    #[error("storage operation failed: {0:?}")]
    StorageOperationFailed(#[from] rocksdb::Error),

    #[error("json parse failed: {0:?}")]
    JsonParseFailed(#[from] simd_json::Error),
}

pub struct EventsGuard;

/// EventsGuard is a bridge between the task events and the storage
impl EventsGuard {
    pub fn global() -> &'static Arc<EventsGuard> {
        static EVENTS: OnceLock<Arc<EventsGuard>> = OnceLock::new();

        EVENTS.get_or_init(|| Arc::new(EventsGuard))
    }

    /// get_event get a task event by event id
    pub fn get_event(&self, event_id: TaskEventID) -> Result<Option<TaskEvent>, TaskStorageError> {
        let db = Storage::global().get_instance();
        let key = format!("task:event:id:{}", event_id);
        let value = db.get(key.as_bytes())?;
        match value {
            Some(mut value) => {
                let event: TaskEvent = simd_json::from_slice(&mut value)?;
                Ok(Some(event))
            }
            None => Ok(None),
        }
    }

    /// get_events get all events of a task
    pub fn get_events(&self, task_id: TaskID) -> Result<Option<Vec<TaskEvent>>, TaskStorageError> {
        let mut value = match self.get_event_ids(task_id)? {
            Some(value) => value,
            None => return Ok(None),
        };

        let mut events = Vec::with_capacity(value.len());
        for event_id in value.drain(..) {
            let event = self.get_event(event_id)?.unwrap(); // unwrap because it should be exist here, if not, it's a bug
            events.push(event);
        }
        Ok(Some(events))
    }

    pub fn get_event_ids(
        &self,
        task_id: TaskID,
    ) -> Result<Option<Vec<TaskEventID>>, TaskStorageError> {
        let db = Storage::global().get_instance();
        let key = format!("task:events:task_id:{}", task_id);
        let value = db.get(key.as_bytes())?;
        let value: Vec<TaskEventID> = match value {
            Some(mut value) => simd_json::from_slice(&mut value)?,
            None => return Ok(None),
        };
        Ok(Some(value))
    }

    /// add_event add a new event to the storage
    pub fn add_event(&self, event: &TaskEvent) -> Result<(), TaskStorageError> {
        let mut event_ids = match self.get_event_ids(event.task_id)? {
            Some(value) => value,
            None => Vec::new(),
        };
        event_ids.push(event.id);

        let db = Storage::global().get_instance();
        let tx = db.transaction();
        let event_key = format!("task:event:id:{}", event.id);
        let event_ids_key = format!("task:events:task_id:{}", event.task_id);
        let event_value = simd_json::to_vec(event)?;
        let event_ids = simd_json::to_vec(&event_ids)?;
        let _ = tx.put(event_key.as_bytes(), event_value);
        let _ = tx.put(event_ids_key.as_bytes(), event_ids);
        tx.commit()?;
        Ok(())
    }

    /// update_event update a event in the storage
    pub fn update_event(&self, event: &TaskEvent) -> Result<(), TaskStorageError> {
        let db = Storage::global().get_instance();
        let event_key = format!("task:event:id:{}", event.id);
        let event_value = simd_json::to_vec(event)?;
        db.put(event_key.as_bytes(), event_value)?;
        Ok(())
    }

    /// remove_event remove a event from the storage
    pub fn remove_event(
        &self,
        event_id: TaskEventID,
        task_id: TaskID,
    ) -> Result<(), TaskStorageError> {
        let event_ids: Vec<TaskEventID> = match self.get_event_ids(task_id)? {
            Some(value) => value.into_iter().filter(|v| v != &event_id).collect(),
            None => return Ok(()),
        };
        let db = Storage::global().get_instance();
        let tx = db.transaction();
        let event_key = format!("task:event:id:{}", event_id);
        let event_ids_key = format!("task:events:task_id:{}", event_id);
        tx.delete(event_key.as_bytes())?;
        if event_ids.is_empty() {
            tx.delete(event_ids_key.as_bytes())?
        } else {
            let event_ids = simd_json::to_vec(&event_ids)?;
            tx.put(event_ids_key.as_bytes(), event_ids)?
        }
        tx.commit()?;
        Ok(())
    }
}

// pub struct TaskGuard;
pub trait TaskGuard {
    fn restore(&mut self) -> Result<(), TaskStorageError>;
    fn dump(&self) -> Result<(), TaskStorageError>;
}

/// TaskGuard is a bridge between the tasks and the storage
impl TaskGuard for TaskManager {
    fn restore(&mut self) -> Result<(), TaskStorageError> {
        let db = Storage::global().get_instance();
        let iter = db.iterator(rocksdb::IteratorMode::From(
            b"task:id:",
            rocksdb::Direction::Forward,
        ));
        let mut tasks = Vec::new();
        for item in iter {
            let (key, mut value) = item?;
            debug!("restore task: {:?} {:?}", key, value);
            let task = simd_json::from_slice::<Task>(&mut value)?;
            tasks.push(task);
        }
        self.restore_tasks(tasks);
        Ok(())
    }

    fn dump(&self) -> Result<(), TaskStorageError> {
        let tasks = self.list();
        let db = Storage::global().get_instance();
        let tx = db.transaction();
        for task in tasks {
            let key = format!("task:id:{}", task.id);
            let value = simd_json::to_vec(&task)?;
            tx.put(key.as_bytes(), value)?;
        }
        tx.commit()?;
        Ok(())
    }
}
