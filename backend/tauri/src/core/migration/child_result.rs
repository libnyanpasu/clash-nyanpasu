//! Failure report exchanged only between the migration subprocess and its parent.
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use snafu::ResultExt as _;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailureStage {
    RunnerConstruction,
    Backup,
    Migration,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RevisionFailure {
    NewerStoreSchema {
        path: PathBuf,
        read: u64,
        supported: u64,
    },
    OlderStoreSchema {
        path: PathBuf,
        read: u64,
        supported: u64,
    },
    NewerDocument {
        path: PathBuf,
        read: u64,
        supported: u64,
    },
    NewerAppliedModule {
        path: PathBuf,
        module: String,
        read: u64,
        supported: u64,
    },
}

#[derive(Debug, Serialize, Deserialize, snafu::Snafu)]
#[snafu(display("migration child failed during {stage:?}: {}", chain.join(": ")))]
pub(crate) struct Failure {
    pub stage: FailureStage,
    pub config_dir: PathBuf,
    pub backups_dir: PathBuf,
    pub paths: Vec<PathBuf>,
    pub revision: Option<RevisionFailure>,
    pub chain: Vec<String>,
    pub backup_available: bool,
    #[serde(default)]
    pub io_failure: Option<nyanpasu_utils::io::IoFailure>,
}

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to encode migration result"))]
    Encode { source: serde_json::Error },
    #[snafu(display("failed to write migration result {}", path.display()))]
    Write {
        path: PathBuf,
        source: super::fs::Error,
    },
    #[snafu(display("failed to read migration result {}", path.display()))]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to decode migration result {}", path.display()))]
    Decode {
        path: PathBuf,
        source: serde_json::Error,
    },
}

impl Failure {
    pub fn from_error(
        error: &(dyn std::error::Error + 'static),
        stage: FailureStage,
        paths: &nyanpasu_paths::PathResolver,
        backup_available: bool,
    ) -> Self {
        let mut chain = Vec::new();
        let mut revision = None;
        let mut related_paths = Vec::new();
        let mut current = Some(error);
        while let Some(error) = current {
            chain.push(error.to_string());
            if let Some(error) = error.downcast_ref::<super::store::Error>() {
                revision = match error {
                    super::store::Error::NewerSchema {
                        path,
                        read,
                        supported,
                    } => Some(RevisionFailure::NewerStoreSchema {
                        path: path.clone(),
                        read: *read,
                        supported: *supported,
                    }),
                    super::store::Error::OlderSchema {
                        path,
                        read,
                        supported,
                    } => Some(RevisionFailure::OlderStoreSchema {
                        path: path.clone(),
                        read: *read,
                        supported: *supported,
                    }),
                    _ => revision,
                };
            }
            if let Some(error) = error.downcast_ref::<super::runner::Error>() {
                revision = match error {
                    super::runner::Error::NewerDocument {
                        path,
                        read,
                        supported,
                    } => Some(RevisionFailure::NewerDocument {
                        path: path.clone(),
                        read: *read,
                        supported: *supported,
                    }),
                    super::runner::Error::NewerAppliedRevision {
                        path,
                        module,
                        read,
                        supported,
                    } => Some(RevisionFailure::NewerAppliedModule {
                        path: path.clone(),
                        module: module.clone(),
                        read: *read,
                        supported: *supported,
                    }),
                    _ => revision,
                };
            }
            let path = if let Some(error) = error.downcast_ref::<super::MigrationCheckError>() {
                match error {
                    super::MigrationCheckError::Io { path, .. }
                    | super::MigrationCheckError::Parse { path, .. }
                    | super::MigrationCheckError::Storage { path, .. }
                    | super::MigrationCheckError::Stamp { path, .. }
                    | super::MigrationCheckError::MigrateClashOverrides { path, .. }
                    | super::MigrationCheckError::ReadSessionState { path, .. } => Some(path),
                    super::MigrationCheckError::Unrecognized(_) => None,
                }
            } else if let Some(crate::core::backup::BackupError::Io { path, .. }) =
                error.downcast_ref::<crate::core::backup::BackupError>()
            {
                Some(path)
            } else if let Some(error) = error.downcast_ref::<super::store::Error>() {
                match error {
                    super::store::Error::ReadState { path, .. }
                    | super::store::Error::ParseState { path, .. }
                    | super::store::Error::NewerSchema { path, .. }
                    | super::store::Error::OlderSchema { path, .. } => Some(path),
                    _ => None,
                }
            } else if let Some(error) = error.downcast_ref::<super::fs::Error>() {
                match error {
                    super::fs::Error::StampDocument { path, .. }
                    | super::fs::Error::SerializeDocument { path, .. }
                    | super::fs::Error::ValidateDestination { path, .. }
                    | super::fs::Error::CreateDirectory { path, .. }
                    | super::fs::Error::WriteFile { path, .. } => Some(path),
                }
            } else if let Some(crate::core::session_state::Error::ReadState { path, .. }) =
                error.downcast_ref::<crate::core::session_state::Error>()
            {
                Some(path)
            } else if let Some(crate::core::corrupt::Error::PreserveEvidence { path, .. }) =
                error.downcast_ref::<crate::core::corrupt::Error>()
            {
                Some(path)
            } else {
                None
            };
            if let Some(path) = path
                && !related_paths.contains(path)
            {
                related_paths.push(path.clone());
            }
            current = error.source();
        }
        if let Some(revision) = &revision {
            let path = match revision {
                RevisionFailure::NewerStoreSchema { path, .. }
                | RevisionFailure::OlderStoreSchema { path, .. }
                | RevisionFailure::NewerDocument { path, .. }
                | RevisionFailure::NewerAppliedModule { path, .. } => path,
            };
            if !related_paths.contains(path) {
                related_paths.push(path.clone());
            }
        }
        Self {
            stage,
            config_dir: paths.app_config_dir().as_std_path().to_owned(),
            backups_dir: paths.backups_dir().into_std_path_buf(),
            paths: related_paths,
            revision,
            chain,
            backup_available,
            io_failure: crate::core::corrupt::error_io(error),
        }
    }

    pub fn write(&self, path: &Path) -> Result<(), Error> {
        let contents = serde_json::to_vec(self).context(EncodeSnafu)?;
        super::fs::atomic_write(path, &contents).context(WriteSnafu { path })
    }

    pub fn read(path: &Path) -> Result<Self, Error> {
        let contents = std::fs::read(path).context(ReadSnafu { path })?;
        serde_json::from_slice(&contents).context(DecodeSnafu { path })
    }
}
