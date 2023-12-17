use std::fmt::{self, Formatter};

use anyhow::Result;
use async_trait::async_trait;
use dyn_clone::{clone_trait_object, DynClone};

/// JobExecutor is a trait for job executor
/// It is used to define a sync job
///
/// For example, you can define a job to print hello.
/// ``` rust
/// use anyhow::Result;
/// #[derive(Clone)]
/// pub struct HelloJob {}
/// impl JobExecutor for HelloJob {
///     fn execute(&self) -> Result<()> {
///        println!("hello");
///       Ok(())
///    }
/// }
/// ```
/// Then you can pass it to the task manager to execute it.
///
///
pub trait JobExecutor: DynClone {
    fn execute(&self) -> Result<()>;
}

clone_trait_object!(JobExecutor);

pub type Job = Box<dyn JobExecutor + Send + Sync>;

#[async_trait]
pub trait AsyncJobExecutor: DynClone {
    async fn execute(&self) -> Result<()>;
}

clone_trait_object!(AsyncJobExecutor);

pub type AsyncJob = Box<dyn AsyncJobExecutor + Send + Sync>;

#[derive(Clone)]
pub enum TaskExecutor {
    Sync(Job),
    Async(AsyncJob),
}

impl fmt::Debug for TaskExecutor {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sync(_) => write!(f, "Sync"),
            Self::Async(_) => write!(f, "Async"),
        }
    }
}

impl Default for TaskExecutor {
    fn default() -> Self {
        Self::Sync(Job::default()) // default job executor
    }
}

impl From<Job> for TaskExecutor {
    fn from(job: Job) -> Self {
        Self::Sync(job)
    }
}

impl From<AsyncJob> for TaskExecutor {
    fn from(job: AsyncJob) -> Self {
        Self::Async(job)
    }
}

#[derive(Clone, Debug)]
struct DefaultJobExecutor {}

impl JobExecutor for DefaultJobExecutor {
    fn execute(&self) -> Result<()> {
        unimplemented!("not implemented");
    }
}

#[async_trait]
impl AsyncJobExecutor for DefaultJobExecutor {
    async fn execute(&self) -> Result<()> {
        unimplemented!("not implemented");
    }
}

impl Default for Job {
    fn default() -> Self {
        Box::new(DefaultJobExecutor {})
    }
}

impl Default for AsyncJob {
    fn default() -> Self {
        Box::new(DefaultJobExecutor {})
    }
}
