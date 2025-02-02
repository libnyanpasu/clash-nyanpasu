use crate::core::tasks::{
    executor::{AsyncJobExecutor, TaskExecutor},
    storage::TaskStorage,
    task::TaskSchedule,
};
use anyhow::Context;
use parking_lot::Mutex;
use std::sync::Arc;

use super::JobExt;

const CLEAR_EVENTS_TASK_NAME: &str = "Task Events Rotate";

#[derive(Clone)]
pub struct EventsRotateJob {
    task_storage: Arc<Mutex<TaskStorage>>,
}

impl EventsRotateJob {
    pub fn new(task_storage: Arc<Mutex<TaskStorage>>) -> Self {
        Self { task_storage }
    }
}

#[async_trait::async_trait]
impl AsyncJobExecutor for EventsRotateJob {
    // TODO: optimize performance if we got reported that this job is slow
    async fn execute(&self) -> anyhow::Result<()> {
        let storage = self.task_storage.lock();
        let task_ids = storage.list_tasks().context("failed to list tasks")?;
        for task_id in task_ids {
            let event_ids = storage
                .get_event_ids(task_id)
                .context(format!("failed to get event ids for task {}", task_id))?
                .unwrap_or_default();
            let mut events_to_remove = Vec::new();
            let mut events = event_ids
                .into_iter()
                .filter_map(|id| {
                    let event = storage.get_event(id).ok().flatten();
                    if event.is_none() {
                        events_to_remove.push(id);
                    }
                    event
                })
                .collect::<Vec<_>>();
            // DESC sort events by updated_at
            events.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            // keep max 10 events
            let events = events
                .into_iter()
                .skip(10)
                .map(|e| e.id)
                .collect::<Vec<_>>();
            events_to_remove.extend(events);
            // remove events
            for event_id in events_to_remove {
                log::debug!("removing event {} for task {}", event_id, task_id);
                storage
                    .remove_event(event_id, task_id)
                    .context(format!("failed to remove event {}", event_id))?;
            }
        }
        Ok(())
    }
}

impl JobExt for EventsRotateJob {
    fn name(&self) -> &'static str {
        CLEAR_EVENTS_TASK_NAME
    }

    fn setup(&self) -> Option<crate::core::tasks::task::Task> {
        Some(crate::core::tasks::task::Task {
            name: CLEAR_EVENTS_TASK_NAME.to_string(),
            schedule: TaskSchedule::Cron("@hourly".to_string()),
            executor: TaskExecutor::Async(Box::new(self.clone())),
            ..Default::default()
        })
    }
}
