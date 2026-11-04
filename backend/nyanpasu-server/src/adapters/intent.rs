//! The user's last run or stop request, kept in a file so it survives a restart.

use std::path::PathBuf;

use nyanpasu_contracts::BoxedCause;
use nyanpasu_core::client::core_lifecycle::ports::{CoreIntent, CoreIntentStore};
use nyanpasu_utils::io::atomic_fs;
use tokio::io::AsyncWriteExt as _;

const RUN: &str = "run\n";
const STOPPED: &str = "stopped\n";

/// Keeps the intent in one small file, replaced atomically and synced, so a
/// crash or power loss leaves either the old request or the new one.
pub struct FileCoreIntent {
    path: PathBuf,
}

impl FileCoreIntent {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait::async_trait]
impl CoreIntentStore for FileCoreIntent {
    async fn load(&self) -> Result<CoreIntent, BoxedCause> {
        let text = match tokio::fs::read_to_string(&self.path).await {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(CoreIntent::Run);
            }
            Err(error) => return Err(error.into()),
        };
        match text.trim() {
            "run" => Ok(CoreIntent::Run),
            "stopped" => Ok(CoreIntent::Stopped),
            other => Err(format!("{} holds {other:?}", self.path.display()).into()),
        }
    }

    async fn record(&self, intent: CoreIntent) -> Result<(), BoxedCause> {
        let staged = self.path.with_extension("tmp");
        let text = match intent {
            CoreIntent::Run => RUN,
            CoreIntent::Stopped => STOPPED,
        };
        let mut file = tokio::fs::File::create(&staged).await?;
        file.write_all(text.as_bytes()).await?;
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&staged, &self.path).await?;
        if let Some(directory) = self.path.parent() {
            atomic_fs::sync_dir(directory).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(directory: &tempfile::TempDir) -> FileCoreIntent {
        FileCoreIntent::new(directory.path().join("core-intent"))
    }

    #[tokio::test]
    async fn an_intent_that_was_never_recorded_is_to_run() {
        let directory = tempfile::tempdir().unwrap();

        assert_eq!(store(&directory).load().await.unwrap(), CoreIntent::Run);
    }

    #[tokio::test]
    async fn a_recorded_intent_is_read_back_by_the_next_process() {
        let directory = tempfile::tempdir().unwrap();

        store(&directory).record(CoreIntent::Stopped).await.unwrap();
        assert_eq!(store(&directory).load().await.unwrap(), CoreIntent::Stopped);

        store(&directory).record(CoreIntent::Run).await.unwrap();
        assert_eq!(store(&directory).load().await.unwrap(), CoreIntent::Run);
    }

    #[tokio::test]
    async fn an_unrecognised_file_is_an_error_not_a_guess() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("core-intent"), "maybe\n").unwrap();

        assert!(store(&directory).load().await.is_err());
    }
}
