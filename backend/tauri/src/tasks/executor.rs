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
