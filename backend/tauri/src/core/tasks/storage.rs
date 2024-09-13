// store is a interface to save and restore task states
use super::{
    events::TaskEvent,
    task::{TaskEventID, TaskID, TaskManager},
    utils::Result,
};
use crate::core::{
    storage::{Storage, NYANPASU_TABLE},
    tasks::task::Task,
};
use log::debug;
use redb::{ReadableTable, TableDefinition};
use std::{
    str,
    sync::{Arc, OnceLock},
};

pub struct EventsGuard;

/// EventsGuard is a bridge between the task events and the storage
impl EventsGuard {
    pub fn global() -> &'static Arc<EventsGuard> {
        static EVENTS: OnceLock<Arc<EventsGuard>> = OnceLock::new();

        EVENTS.get_or_init(|| Arc::new(EventsGuard))
    }

    /// get_event get a task event by event id
    pub fn get_event(&self, event_id: TaskEventID) -> Result<Option<TaskEvent>> {
        let db = Storage::global().get_instance();
        let key = format!("task:event:id:{}", event_id);
        let read_txn = db.begin_read()?;
        let table = read_txn.open_table(NYANPASU_TABLE)?;
        let value = table.get(key.as_bytes())?;
        match value {
            Some(value) => {
                let mut value = value.value().to_owned();
                let event: TaskEvent = simd_json::from_slice(value.as_mut_slice())?;
                Ok(Some(event))
            }
            None => Ok(None),
        }
    }

    /// get_events get all events of a task
    #[allow(dead_code)]
    pub fn get_events(&self, task_id: TaskID) -> Result<Option<Vec<TaskEvent>>> {
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

    pub fn get_event_ids(&self, task_id: TaskID) -> Result<Option<Vec<TaskEventID>>> {
        let db = Storage::global().get_instance();
        let key = format!("task:events:task_id:{}", task_id);
        let read_txn = db.begin_read()?;
        let table = read_txn.open_table(NYANPASU_TABLE)?;
        let value = table.get(key.as_bytes())?;
        let value: Vec<TaskEventID> = match value {
            Some(value) => {
                let mut value = value.value().to_owned();
                simd_json::from_slice(value.as_mut_slice())?
            }
            None => return Ok(None),
        };
        Ok(Some(value))
    }

    /// add_event add a new event to the storage
    pub fn add_event(&self, event: &TaskEvent) -> Result<()> {
        let mut event_ids = (self.get_event_ids(event.task_id)?).unwrap_or_default();
        event_ids.push(event.id);

        let db = Storage::global().get_instance();
        let event_key = format!("task:event:id:{}", event.id);
        let event_ids_key = format!("task:events:task_id:{}", event.task_id);
        let event_value = simd_json::to_vec(event)?;
        let event_ids = simd_json::to_vec(&event_ids)?;
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(NYANPASU_TABLE)?;
            table.insert(event_key.as_bytes(), event_value.as_slice())?;
            table.insert(event_ids_key.as_bytes(), event_ids.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// update_event update a event in the storage
    pub fn update_event(&self, event: &TaskEvent) -> Result<()> {
        let db = Storage::global().get_instance();
        let event_key = format!("task:event:id:{}", event.id);
        let event_value = simd_json::to_vec(event)?;
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(NYANPASU_TABLE)?;
            table.insert(event_key.as_bytes(), event_value.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    /// remove_event remove a event from the storage
    #[allow(dead_code)]
    pub fn remove_event(&self, event_id: TaskEventID, task_id: TaskID) -> Result<()> {
        let event_ids: Vec<TaskEventID> = match self.get_event_ids(task_id)? {
            Some(value) => value.into_iter().filter(|v| v != &event_id).collect(),
            None => return Ok(()),
        };
        let db = Storage::global().get_instance();
        let event_key = format!("task:event:id:{}", event_id);
        let event_ids_key = format!("task:events:task_id:{}", event_id);
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(NYANPASU_TABLE)?;
            table.remove(event_key.as_bytes())?;
            if event_ids.is_empty() {
                table.remove(event_ids_key.as_bytes())?;
            } else {
                let event_ids = simd_json::to_vec(&event_ids)?;
                table.insert(event_ids_key.as_bytes(), event_ids.as_slice())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }
}

// pub struct TaskGuard;
pub trait TaskGuard {
    fn restore(&mut self) -> Result<()>;
    fn dump(&self) -> Result<()>;
}

/// TaskGuard is a bridge between the tasks and the storage
impl TaskGuard for TaskManager {
    fn restore(&mut self) -> Result<()> {
        let db = Storage::global().get_instance();
        let mut tasks = Vec::new();

        let read_txn = db.begin_read()?;
        let table = read_txn.open_table(NYANPASU_TABLE)?;
        for item in table.iter()? {
            let (key, value) = item?;
            let key = key.value();
            let mut value = value.value().to_owned();
            if key.starts_with(b"task:id:") {
                let task = simd_json::from_slice::<Task>(value.as_mut_slice())?;
                debug!(
                    "restore task: {:?} {:?}",
                    str::from_utf8(key).unwrap(),
                    str::from_utf8(value.as_slice()).unwrap()
                );
                tasks.push(task);
            }
        }
        self.restore_tasks(tasks);
        Ok(())
    }
    fn dump(&self) -> Result<()> {
        let tasks = self.list();
        let db = Storage::global().get_instance();
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(NYANPASU_TABLE)?;
            for task in tasks {
                let key = format!("task:id:{}", task.id);
                let value = simd_json::to_vec(&task)?;
                table.insert(key.as_bytes(), value.as_slice())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }
}
