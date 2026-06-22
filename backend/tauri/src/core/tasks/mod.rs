mod events;
pub mod executor;
pub mod jobs;
mod storage;
pub mod task;
mod utils;

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(
    app: &M,
    storage: super::storage::Storage,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use parking_lot::RwLock;

    let task_storage = storage::TaskStorage::new(storage);
    let task_manager = task::TaskManager::new(task_storage);
    let task_manager = std::sync::Arc::new(RwLock::new(task_manager));

    // job manager
    let mut job_manager = jobs::JobsManager::new(task_manager.clone());
    job_manager.setup().context("failed to setup job manager")?;
    let job_manager = std::sync::Arc::new(RwLock::new(job_manager));
    app.manage(job_manager);
    // profiles job — funnel scheduled updates through the client (actor + Config stay in sync)
    let gate: std::sync::Arc<dyn jobs::ProfilesUpdateGate> =
        std::sync::Arc::new(app.state::<crate::client::NyanpasuClient>().inner().clone());
    let profiles_job = jobs::ProfilesJobGuard::new(task_manager.clone(), gate);
    {
        let mut profiles_job = profiles_job.write();
        profiles_job.init()?;
    }
    app.manage(profiles_job);
    app.manage(task_manager);
    Ok(())
}
