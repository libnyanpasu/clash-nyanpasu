mod events_rotate;
mod logger;
mod profiles;

use super::{
    task::{Task, TaskManager},
    utils::{ConfigChangedNotifier, Result},
};
use anyhow::anyhow;
use parking_lot::RwLock;
pub use profiles::ProfilesJobGuard;
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

impl ConfigChangedNotifier for JobsManager {
    fn notify_config_changed(&self, job_name: &str) -> Result<()> {
        let job = self
            .jobs
            .iter()
            .find(|job| job.name() == job_name)
            .ok_or(anyhow!("job not exist"))?;
        let task = job.setup();
        if let Some(task) = task {
            let mut task_manager = self.task_manager.write();
            task_manager.remove_task(task.id)?;
            task_manager.add_task(task)?;
        }
        Ok(())
    }
}
