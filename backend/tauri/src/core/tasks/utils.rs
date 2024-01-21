use thiserror::Error;

#[derive(Debug)]
pub enum TaskCreationError {
    #[allow(unused)]
    AlreadyExist,
    NotFound,
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("create task failed: {0:?}")]
    CreateTaskFailed(TaskCreationError),

    #[error("params validation failed: {0}")]
    ParamsValidationFailed(&'static str),

    #[error("storage operation failed: {0:?}")]
    StorageOperationFailed(#[from] rocksdb::Error),

    #[error("json parse failed: {0:?}")]
    JsonParseFailed(#[from] simd_json::Error),

    #[error("task issue failed: {message:?}")]
    InnerTask {
        message: String,
        #[source]
        source: delay_timer::error::TaskError,
    },

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn new_task_error(message: String, source: delay_timer::error::TaskError) -> Self {
        Self::InnerTask { message, source }
    }
}

pub trait ConfigChangedNotifier {
    fn notify_config_changed(&self, task_name: &str) -> Result<()>;
}
