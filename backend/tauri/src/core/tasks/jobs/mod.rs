mod logger;
mod profiles;

use super::{
    task::Task,
    utils::{ConfigChangedNotifier, Result},
};
use anyhow::anyhow;
use parking_lot::Mutex;
pub use profiles::ProfilesJobGuard;
use std::sync::{Arc, OnceLock};
pub trait JobExt {
    fn name(&self) -> &'static str;
    fn setup(&self) -> Option<Task>; // called when the app starts or the config changed
}

pub struct JobsManager {
    jobs: Vec<Box<dyn JobExt + Send + Sync>>,
}

impl JobsManager {
    pub fn global() -> &'static Arc<Mutex<Self>> {
        static JOBS: OnceLock<Arc<Mutex<JobsManager>>> = OnceLock::new();
        JOBS.get_or_init(|| Arc::new(Mutex::new(Self { jobs: Vec::new() })))
    }

    pub fn global_register() -> Result<()> {
        let jobs: Vec<Box<dyn JobExt + Send + Sync>> = vec![
        // Box::<logger::ClearLogsJob>::default() as Box<dyn JobExt + Send + Sync>
        ];
        for job in jobs {
            let task = job.setup();
            if let Some(task) = task {
                super::task::TaskManager::global().write().add_task(task)?;
            }
            JobsManager::global().lock().jobs.push(job);
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
            let mut task_manager = super::task::TaskManager::global().write();
            task_manager.remove_task(task.id)?;
            task_manager.add_task(task)?;
        }
        Ok(())
    }
}
