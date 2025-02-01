use super::super::{
    executor::{AsyncJobExecutor, TaskExecutor},
    task::{Task, TaskID, TaskManager, TaskSchedule},
};
use crate::{
    config::{Config, ProfileSharedGetter},
    feat,
};
use anyhow::Result;
use async_trait::async_trait;
use parking_lot::RwLock;
use std::{collections::HashMap, ops::Deref, sync::Arc, time::Duration};

const INITIAL_TASK_ID: TaskID = 10000000; // 留一个初始的 TaskID，避免和其他任务的 ID 冲突

type Minutes = u64;
type ProfileUID = String;

#[derive(Clone)]
pub struct ProfileUpdater(ProfileUID);

impl ProfileUpdater {
    #[allow(dead_code)]
    pub fn new(profile_uid: &str) -> Self {
        Self(profile_uid.to_string())
    }
}

#[async_trait]
impl AsyncJobExecutor for ProfileUpdater {
    async fn execute(&self) -> Result<()> {
        log::info!(target: "app", "running timer task `{}`", self.0);
        match feat::update_profile(self.0.clone(), None).await {
            Ok(_) => Ok(()),
            Err(err) => {
                log::error!(target: "app", "failed to update profile: {err:?}");
                Err(err)
            }
        }
    }
}

enum ProfileTaskOp {
    Add(TaskID, Minutes),
    Remove(TaskID),
    Update(TaskID, Minutes),
}

pub struct ProfilesJob {
    task_map: HashMap<ProfileUID, (TaskID, u64)>,
    task_manager: Arc<RwLock<TaskManager>>,
    // next_id: TaskID,
}

pub struct ProfilesJobGuard {
    job: Arc<RwLock<ProfilesJob>>,
}

impl ProfilesJobGuard {
    pub fn new(task_manager: Arc<RwLock<TaskManager>>) -> Self {
        Self {
            job: Arc::new(RwLock::new(ProfilesJob::new(task_manager))),
        }
    }
}

impl Deref for ProfilesJobGuard {
    type Target = Arc<RwLock<ProfilesJob>>;

    fn deref(&self) -> &Self::Target {
        &self.job
    }
}

impl ProfilesJob {
    pub fn new(task_manager: Arc<RwLock<TaskManager>>) -> Self {
        Self {
            task_map: HashMap::new(),
            task_manager,
        }
    }

    /// restore timer
    pub fn init(&mut self) -> Result<()> {
        self.refresh();
        let cur_timestamp = chrono::Local::now().timestamp();
        let task_map = &self.task_map;

        Config::profiles()
            .latest()
            .items
            .iter()
            .filter_map(|item| {
                if !item.is_remote() {
                    return None;
                }
                let item = item.as_remote().unwrap();
                // mins to seconds
                let interval = ((item.option.update_interval) as i64) * 60;
                let updated = item.updated() as i64;

                if interval > 0 && cur_timestamp - updated >= interval {
                    Some(item)
                } else {
                    None
                }
            })
            .for_each(|item| {
                if let Some((task_id, _)) = task_map.get(item.uid()) {
                    crate::log_err!(self.task_manager.write().advance_task(*task_id));
                }
            });

        Ok(())
    }

    /// Correctly update all cron tasks
    pub fn refresh(&mut self) {
        let diff_map = self.diff();
        for (uid, diff) in diff_map.into_iter() {
            match diff {
                ProfileTaskOp::Add(task_id, interval) => {
                    let task = new_task(task_id, &uid, interval);
                    crate::log_err!(self.task_manager.write().add_task(task));
                    self.task_map.insert(uid, (task_id, interval));
                }
                ProfileTaskOp::Remove(task_id) => {
                    crate::log_err!(self.task_manager.write().remove_task(task_id));
                    self.task_map.remove(&uid);
                }
                ProfileTaskOp::Update(task_id, interval) => {
                    crate::log_err!(self.task_manager.write().remove_task(task_id));
                    let task = new_task(task_id, &uid, interval);
                    crate::log_err!(self.task_manager.write().add_task(task));
                    self.task_map.insert(uid, (task_id, interval));
                }
            }
        }
    }
    // fn get_next_task_id(&mut self) -> TaskID {
    //     let id = self.next_id;
    //     self.next_id += 1;
    //     id
    // }

    /// generate the diff map for refresh
    fn diff(&self) -> HashMap<ProfileUID, ProfileTaskOp> {
        let mut diff_map = HashMap::new();

        let timer_map = &self.task_map;

        let new_map = gen_map();

        timer_map.iter().for_each(|(uid, (tid, val))| {
            let new_val = new_map.get(uid).unwrap_or(&0);

            if *new_val == 0 {
                diff_map.insert(uid.clone(), ProfileTaskOp::Remove(*tid));
            } else if new_val != val {
                diff_map.insert(uid.clone(), ProfileTaskOp::Update(*tid, *new_val));
            }
        });

        new_map.iter().for_each(|(uid, val)| {
            if timer_map.get(uid).is_none() {
                let task_id = get_task_id(uid);
                diff_map.insert(uid.clone(), ProfileTaskOp::Add(task_id, *val));
            }
        });

        diff_map
    }
}

/// generate a uid -> update_interval map
fn gen_map() -> HashMap<ProfileUID, Minutes> {
    let mut new_map = HashMap::new();

    Config::profiles()
        .latest()
        .get_items()
        .iter()
        .filter_map(|item| item.as_remote())
        .for_each(|item| {
            let interval = item.option.update_interval;
            if interval > 0 {
                new_map.insert(item.uid().to_string(), interval);
            }
        });

    new_map
}

/// get_task_id Get a u64 task id by profile uid
fn get_task_id(uid: &str) -> TaskID {
    let task_id = seahash::hash(uid.as_bytes());
    if task_id < INITIAL_TASK_ID {
        INITIAL_TASK_ID + task_id
    } else {
        task_id
    }
}

fn new_task(task_id: TaskID, profile_uid: &str, interval: Minutes) -> Task {
    Task {
        id: task_id,
        name: format!("profile-updater-{}", profile_uid),
        executor: TaskExecutor::Async(Box::new(ProfileUpdater(profile_uid.to_owned().to_string()))),
        schedule: TaskSchedule::Interval(Duration::from_secs(interval * 60)),
        ..Task::default()
    }
}
