mod events_rotate;
mod logger;

use super::task::{Task, TaskManager};
use parking_lot::RwLock;
use std::sync::Arc;
pub trait JobExt {
    fn name(&self) -> &'static str;
    fn setup(&self) -> Option<Task>; // called when the app starts or the config changed
}

pub struct JobsManager {
    jobs: Vec<Box<dyn JobExt + Send + Sync>>,
    task_manager: Arc<RwLock<TaskManager>>,
}

impl JobsManager {
    pub fn new(task_manager: Arc<RwLock<TaskManager>>) -> Self {
        Self {
            jobs: Vec::new(),
            task_manager,
        }
    }

    pub fn setup(&mut self) -> anyhow::Result<()> {
        let jobs: Vec<Box<dyn JobExt + Send + Sync>> = vec![Box::new(
            events_rotate::EventsRotateJob::new(self.task_manager.read().get_inner_task_storage()),
        )];
        for job in jobs {
            let task = job.setup();
            if let Some(task) = task {
                self.task_manager.write().add_task(task)?;
            }
            self.jobs.push(job);
        }
        Ok(())
    }
}
