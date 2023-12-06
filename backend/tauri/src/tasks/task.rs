use super::executor::{AsyncJob, Job};
use crate::error;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use delay_timer::{
    entity::{DelayTimer, DelayTimerBuilder},
    timer::task::{Task as TimerTask, TaskBuilder as TimerTaskBuilder},
    utils::convenience::cron_expression_grammatical_candy::{CandyCronStr, CandyFrequency},
};
use once_cell::sync::OnceCell;
use std::{
    borrow::BorrowMut,
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, RwLock as RW},
};

pub type TaskID = u64;

#[derive(Debug, Clone)]
pub enum TaskState {
    Cancelled, // 任务已取消，不再执行
    Idle,      // 空闲
    Running,   // 任务执行中
}

#[derive(Debug, Clone)]
pub enum TaskRunResult {
    Ok,
    Err(String),
}

#[derive(Debug, Clone)]
pub enum TaskType {
    Once,     // 一次性执行
    Interval, // 按间隔执行
    Cron,     // 按 cron 表达式执行
}

// TODO: 如果需要的话，未来可以添加执行日记（历史记录）
#[derive(Debug, Clone)]
pub struct Task {
    id: u64,
    name: String,
    r#type: TaskType,
    cron: Option<String>, // cron expression
    state: TaskState,
    interval: Option<i64>, // seconds
    last_run: Option<(Timestamp, TaskRunResult)>,
    next_run: Option<Timestamp>, // timestamp
    created_at: Timestamp,
}

impl Default for Task {
    fn default() -> Self {
        Task {
            id: 0,
            name: String::new(),
            r#type: TaskType::Once,
            cron: None,
            state: TaskState::Idle,
            interval: None,
            last_run: None,
            next_run: None,
            created_at: 0,
        }
    }
}

pub type Timestamp = i64;

// 检查任务输入
macro_rules! check_task_input {
    ($task:ident) => {
        if $task.name.is_empty() {
            return Err(anyhow!("task name is empty"));
        }

        match $task.r#type {
            TaskType::Cron => {
                if $task.cron.is_none() {
                    return Err(anyhow!("cron expression is empty"));
                }
            }
            TaskType::Interval | TaskType::Once => {
                if $task.interval.is_none() {
                    return Err(anyhow!("interval is empty"));
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

    match task.r#type {
        TaskType::Cron => {
            // NOTE: 由于 DelayTimer 的垃圾设计，因此继续使用弃用的 candy 方法
            // NOTE: 请注意一定需要回收内存，否则会造成内存泄漏
            let cron = task.cron.clone().unwrap().clone();
            builder.set_frequency_by_candy(CandyFrequency::Repeated(CandyCronStr(cron)));
        }
        TaskType::Interval => {
            builder.set_frequency_repeated_by_seconds(task.interval.unwrap() as u64);
        }
        // 一次性任务，目前设计只支持 Interval
        TaskType::Once => {
            builder.set_frequency_once_by_seconds(task.interval.unwrap() as u64);
        }
    }

    builder.set_maximum_parallel_runnable_num(5); // 最大同时并发数

    (task, builder)
}

fn wrap_job(list: TaskList, task_id: TaskID, job: Job) {
    list.set_task_state(task_id, TaskState::Running, None);
    let res = job.execute();
    list.set_task_state(
        task_id,
        TaskState::Idle,
        Some(match res {
            Ok(_) => TaskRunResult::Ok,
            Err(e) => {
                error!(format!("task error: {}", e.to_string()));
                TaskRunResult::Err(e.to_string())
            }
        }),
    );
}

async fn wrap_async_job(list: TaskList, task_id: TaskID, async_job: AsyncJob) {
    list.set_task_state(task_id, TaskState::Running, None);
    let res = async_job.execute().await;
    list.set_task_state(
        task_id,
        TaskState::Idle,
        Some(match res {
            Ok(_) => TaskRunResult::Ok,
            Err(e) => {
                error!(format!("task error: {}", e.to_string()));
                TaskRunResult::Err(e.to_string())
            }
        }),
    );
}

// TaskList 语法糖
type TaskList = Arc<RW<Vec<Task>>>;
trait TaskListOps {
    fn set_task_state(
        &self,
        task_id: u64,
        state: TaskState,
        result: Option<TaskRunResult>,
    ) -> Result<()>;
}
impl TaskListOps for TaskList {
    fn set_task_state(
        &self,
        task_id: u64,
        state: TaskState,
        result: Option<TaskRunResult>,
    ) -> Result<()> {
        let mut list = self.write().unwrap();
        let item = list
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or(anyhow!("task {} not found", task_id))?;
        match state {
            TaskState::Running => {
                item.state = TaskState::Running;
            }
            TaskState::Idle => {
                match item.state {
                    TaskState::Running => {
                        item.last_run = Some((
                            Utc::now().timestamp(),
                            result.ok_or(anyhow!(
                                "change task {} state from running to idle, but result is none",
                                task_id
                            ))?,
                        ));
                    }
                    _ => {}
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

    /// task list
    list: TaskList,
}

impl TaskManager {
    pub fn global() -> &'static Self {
        static TASK_MANAGER: OnceCell<TaskManager> = OnceCell::new();

        TASK_MANAGER.get_or_init(|| TaskManager {
            timer: Arc::new(Mutex::new(DelayTimerBuilder::default().build())),
            list: Arc::new(RW::new(Vec::new())),
        })
    }

    /// add sync task
    fn add_task(&mut self, task: Task, job: Job) -> Result<()> {
        check_task_input!(task);
        let (task, mut builder) = {
            let list = self.list.read().unwrap();
            build_task(task, list.len())
        };

        let task_id = task.id;
        let list_ref = self.list.clone();
        let body = move || {
            let list = list_ref.clone();
            wrap_job(list, task_id, job.clone())
        };

        let timer_task = builder.spawn_routine(body);
        {
            builder.free(); // 在错误处理之前，先释放内存
        }
        let timer = self.timer.lock().unwrap();
        let mut list = self.list.write().unwrap();
        timer
            .add_task(timer_task.context("failed to build a task")?)
            .context("failed to add a task to scheduler")?;
        list.push(task);
        Ok(())
    }

    fn add_async_task(&mut self, task: Task, async_job: AsyncJob) -> Result<()> {
        check_task_input!(task);
        let (task, mut builder) = {
            let list = self.list.read().unwrap();
            build_task(task, list.len())
        };

        let task_id = task.id;
        let list_ref = self.list.clone();
        let body = move || {
            let list = list_ref.clone();
            let async_job = async_job.clone();
            async move { wrap_async_job(list, task_id, async_job).await }
        };

        let timer_task = builder.spawn_async_routine(body);
        {
            builder.free(); // 在错误处理之前，先释放内存
        }
        let timer = self.timer.lock().unwrap();
        let mut list = self.list.write().unwrap();
        timer
            .add_task(timer_task.context("failed to build a task")?)
            .context("failed to add a task to scheduler")?;
        list.push(task);
        Ok(())
    }

    pub fn add_cron_task(&mut self, task_name: String, job: Job) -> Result<()> {
        let task = Task {
            id: 0,
            name: task_name,
            r#type: TaskType::Cron,
            cron: None,
            state: TaskState::Idle,
            interval: None,
            last_run: None,
            next_run: None,
            created_at: 0,
        };

        self.add_task(task, job)
    }
}
