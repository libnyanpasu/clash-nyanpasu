use super::{
    events::{TaskEventState, TaskEvents, TaskEventsDispatcher},
    executor::{Job, TaskExecutor},
    storage::TaskStorage,
    utils::{Error, Result, TaskCreationError},
};
use crate::error;
use chrono::Utc;
use delay_timer::{
    entity::{DelayTimer, DelayTimerBuilder},
    timer::task::TaskBuilder as TimerTaskBuilder,
    utils::convenience::cron_expression_grammatical_candy::{CandyCronStr, CandyFrequency},
};
use parking_lot::{Mutex, RwLock as RW};
use serde::{Deserialize, Serialize};
use snowflake::SnowflakeIdGenerator;
use std::{sync::Arc, time::Duration};

pub type TaskID = u64;
pub type TaskEventID = i64; // 任务事件 ID，适用于任务并发执行，区分不同的执行事件

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum TaskState {
    Cancelled, // 任务已取消，不再执行
    #[default]
    Idle, // 空闲
    Running(TaskEventID), // 任务执行中，存储最新执行的事件 ID
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TaskRunResult {
    Ok,
    Err(String),
}

#[derive(Debug, Clone)]
pub enum TaskSchedule {
    Once(Duration),     // 一次性执行
    Interval(Duration), // 按间隔执行
    #[allow(dead_code)]
    Cron(String), // 按 cron 表达式执行
}

impl Default for TaskSchedule {
    fn default() -> Self {
        Self::Once(Duration::from_secs(0))
    }
}

// TODO: 如果需要的话，未来可以添加执行日记（历史记录）
#[derive(Debug, Clone)]
pub struct TaskOptions {
    pub maximum_parallel_runnable_num: u64, // 最大同时并发数
}

impl Default for TaskOptions {
    fn default() -> Self {
        Self {
            maximum_parallel_runnable_num: 5,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskID,
    pub name: String,
    #[serde(skip_serializing, skip_deserializing)]
    pub(super) schedule: TaskSchedule,
    #[serde(skip_serializing, skip_deserializing)]
    pub(super) state: TaskState,
    #[serde(skip_serializing, skip_deserializing)]
    pub(super) opts: TaskOptions,
    pub(super) last_run: Option<(Timestamp, TaskRunResult)>,
    pub(super) next_run: Option<Timestamp>, // timestamp
    #[serde(skip_serializing, skip_deserializing)]
    pub(super) executor: TaskExecutor,
    pub created_at: Timestamp,
}

impl Default for Task {
    fn default() -> Self {
        Task {
            id: 0,
            name: String::new(),
            schedule: TaskSchedule::Once(Duration::from_secs(0)),
            state: TaskState::Idle,
            opts: TaskOptions::default(),
            executor: TaskExecutor::Sync(Job::default()), // a unimplemented job
            last_run: None,
            next_run: None,
            created_at: 0,
        }
    }
}

pub type Timestamp = i64;

// 参数校验失败
macro_rules! params_validated_failed {
    ($fmt:expr) => {
        Err(Error::ParamsValidationFailed($fmt))
    };
}

// 检查任务输入
macro_rules! check_task_input {
    ($task:ident) => {
        if $task.name.is_empty() {
            return params_validated_failed!("task name is empty");
        }

        match &$task.schedule {
            TaskSchedule::Once(duration) => {
                if duration.as_secs() <= 0 {
                    return params_validated_failed!("task interval must be greater than 0");
                }
            }
            TaskSchedule::Interval(duration) => {
                if duration.as_secs() <= 0 {
                    return params_validated_failed!("task interval must be greater than 0");
                }
            }
            TaskSchedule::Cron(cron) => {
                if cron.is_empty() {
                    return params_validated_failed!("task cron is empty");
                }
            }
        }
    };
}

// 构建任务
fn build_task<'a>(task: Task, len: usize) -> (Task, TimerTaskBuilder<'a>) {
    let task = Task {
        id: match task.id {
            0 => len as u64 + 1,
            _ => task.id,
        },
        created_at: match task.created_at {
            0 => Utc::now().timestamp(),
            _ => task.created_at,
        },
        ..task
    };

    let mut builder = TimerTaskBuilder::default();
    builder.set_task_id(task.id);

    match &task.schedule {
        TaskSchedule::Cron(cron) => {
            // NOTE: 由于 DelayTimer 的设计，因此继续使用弃用的 candy 方法
            // NOTE: 请注意一定需要回收内存，否则会造成内存泄漏
            let cron = cron.clone();
            #[allow(deprecated)]
            builder.set_frequency_by_candy(CandyFrequency::Repeated(CandyCronStr(cron)));
        }
        TaskSchedule::Interval(duration) => {
            builder.set_frequency_repeated_by_seconds(duration.as_secs());
        }
        // 一次性延迟任务，目前设计只支持 Interval
        // TODO: 支持即时任务？
        TaskSchedule::Once(duration) => {
            builder.set_frequency_once_by_seconds(duration.as_secs());
        }
    }

    builder.set_maximum_parallel_runnable_num(task.opts.maximum_parallel_runnable_num); // 最大同时并发数

    (task, builder)
}

macro_rules! wrap_job {
    ($exec:expr, $list:expr, $id_generator:expr, $task_id:expr, $task_events:expr) => {{
        let event_id = $id_generator.generate();

        let _ = $list.set_task_state($task_id, TaskState::Running(event_id), None);
        $task_events.new_event($task_id, event_id).unwrap();
        $task_events
            .dispatch(event_id, TaskEventState::Running)
            .unwrap();

        let res = $exec;

        let res = match res {
            Ok(_) => TaskRunResult::Ok,
            Err(e) => {
                error!(format!("task error: {}", e.to_string()));
                TaskRunResult::Err(e.to_string())
            }
        };

        if let TaskState::Running(latest_event_id) = $list.get_task_state($task_id).unwrap() {
            if latest_event_id == event_id {
                let _ = $list.set_task_state($task_id, TaskState::Idle, Some(res.clone()));
            }
        }
        $task_events
            .dispatch(event_id, TaskEventState::Finished(res))
            .unwrap();
    }};
}

// TaskList 语法糖
type TaskList = Arc<RW<Vec<Task>>>;
trait TaskListOps {
    fn get_task_state(&self, task_id: TaskID) -> Result<TaskState>;
    fn set_task_state(
        &self,
        task_id: TaskID,
        state: TaskState,
        result: Option<TaskRunResult>,
    ) -> Result<()>;
}
impl TaskListOps for TaskList {
    fn get_task_state(&self, task_id: TaskID) -> Result<TaskState> {
        let list = self.read();
        let item = list
            .iter()
            .find(|t| t.id == task_id)
            .ok_or(Error::CreateTaskFailed(TaskCreationError::NotFound))?;
        Ok(item.state.clone())
    }

    fn set_task_state(
        &self,
        task_id: TaskID,
        state: TaskState,
        result: Option<TaskRunResult>,
    ) -> Result<()> {
        let mut list = self.write();
        let item = list
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or(Error::CreateTaskFailed(TaskCreationError::NotFound))?;
        match state {
            TaskState::Running(event_id) => {
                item.state = TaskState::Running(event_id);
            }
            TaskState::Idle => {
                if let TaskState::Running(_) = item.state {
                    item.last_run = Some((
                        Utc::now().timestamp(),
                        result.ok_or(Error::CreateTaskFailed(TaskCreationError::NotFound))?,
                    ));
                }
                item.state = TaskState::Idle;
            }
            TaskState::Cancelled => {
                item.state = TaskState::Cancelled;
            }
        }
        Ok(())
    }
}

pub struct TaskManager {
    /// cron manager
    timer: Arc<Mutex<DelayTimer>>,
    // Add a mutex to protect the concurrency of the storage
    pub(super) storage: Arc<Mutex<TaskStorage>>,
    task_events: Arc<TaskEvents>,
    /// task list
    list: TaskList,
    restore_list: TaskList,
    id_generator: SnowflakeIdGenerator,
}

impl TaskManager {
    pub fn new(storage: TaskStorage) -> Self {
        let storage = Arc::new(Mutex::new(storage));
        let task_events = TaskEvents::new(storage.clone());
        Self {
            timer: Arc::new(Mutex::new(DelayTimerBuilder::default().build())),
            storage,
            task_events: Arc::new(task_events),
            restore_list: Arc::new(RW::new(Vec::new())),
            list: Arc::new(RW::new(Vec::new())),
            id_generator: SnowflakeIdGenerator::new(1, 1),
        }
    }

    #[doc(hidden)]
    /// a hidden method to get the inner task storage
    /// just for internal jobs to use
    pub(super) fn get_inner_task_storage(&self) -> Arc<Mutex<TaskStorage>> {
        self.storage.clone()
    }

    pub fn restore_tasks(&mut self, tasks: Vec<Task>) {
        let mut list = self.restore_list.write();
        list.clear();
        for task in tasks {
            list.push(task);
        }
    }

    /// add task
    ///
    /// # Example
    /// ```rust
    /// let task = Task {
    ///    name: "test".to_string(),
    ///    schedule: TaskSchedule::Once(Duration::from_secs(1)),
    ///   ..Task::default()
    /// };
    /// let job = Job::default();
    /// task_manager.add_task(task, job.into());
    pub fn add_task(&mut self, task: Task) -> Result<()> {
        check_task_input!(task);

        let (mut task, mut builder) = {
            let list = self.list.read();
            build_task(task, list.len())
        };
        let restored_task = self.get_task_from_restored(task.id);
        if let Some(restored_task) = restored_task {
            if restored_task.name == task.name {
                task.last_run = restored_task.last_run;
                task.created_at = restored_task.created_at;
            }
        }

        let task_id = task.id;
        let id_generator = self.id_generator;
        let list_ref = self.list.clone();
        let executor = task.executor.clone();
        let task_events = self.task_events.clone();
        let timer_task = match executor {
            TaskExecutor::Sync(job) => {
                let body = move || {
                    let list = list_ref.clone();
                    let mut id_generator = id_generator;
                    wrap_job!(job.execute(), list, id_generator, task_id, task_events);
                };
                builder.spawn_routine(body)
            }
            TaskExecutor::Async(async_job) => {
                let body = move || {
                    let list = list_ref.clone();
                    let async_job = async_job.clone();
                    let mut id_generator = id_generator;
                    let task_events = task_events.clone();
                    async move {
                        wrap_job!(
                            async_job.execute().await,
                            list,
                            id_generator,
                            task_id,
                            task_events
                        );
                    }
                };

                builder.spawn_async_routine(body)
            }
        };

        {
            builder.free(); // 在错误处理之前，先释放内存
        }

        let timer = self.timer.lock();
        let mut list = self.list.write();
        timer
            .add_task(timer_task.map_err(|e| {
                Error::new_task_error("failed to create a delay task instance".to_string(), e)
            })?)
            .map_err(|e| {
                Error::new_task_error("failed to add a task to scheduler".to_string(), e)
            })?;
        let storage = self.storage.lock();
        let task_id = task.id;
        list.push(task);
        storage.add_task(task_id)?;
        Ok(())
    }

    fn get_task_from_restored(&self, task_id: TaskID) -> Option<Task> {
        let list = self.restore_list.read();
        list.iter().find(|t| t.id == task_id).cloned()
    }

    #[allow(dead_code)]
    pub fn pick_task(&self, task_id: TaskID) -> Result<Task> {
        let list = self.list.read();
        list.iter()
            .find(|t| t.id == task_id)
            .cloned()
            .ok_or(Error::CreateTaskFailed(TaskCreationError::NotFound))
    }

    #[allow(dead_code)]
    pub fn total(&self) -> usize {
        let list = self.list.read();
        list.len()
    }

    // get current task list
    // note: this method will clone the task list
    pub fn list(&self) -> Vec<Task> {
        let list = self.list.read();
        list.clone()
    }

    pub fn remove_task(&mut self, task_id: TaskID) -> Result<()> {
        let mut list = self.list.write();
        let index = list
            .iter()
            .position(|t| t.id == task_id)
            .ok_or(Error::CreateTaskFailed(TaskCreationError::NotFound))?;
        self.timer
            .lock()
            .remove_task(task_id)
            .map_err(|e| Error::new_task_error("failed to remove task".to_string(), e))?;
        list.remove(index);
        let storage = self.storage.lock();
        storage.remove_task(task_id)?;
        Ok(())
    }

    pub fn advance_task(&mut self, task_id: TaskID) -> Result<()> {
        let timer = self.timer.lock();
        timer
            .advance_task(task_id)
            .map_err(|e| Error::new_task_error("failed to advance a task".to_string(), e))?;
        Ok(())
    }
}
