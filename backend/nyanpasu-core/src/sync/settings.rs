//! `<config>/local/sync.yaml`: the device-scoped sync settings.
//!
//! The document holds no secret, so it is an ordinary atomic YAML write. It is
//! kept out of `application.yaml` because pointing sync at a storage endpoint
//! moves subscription tokens there, which is a different privilege class from
//! an ordinary preference.

use std::{fs, path::Path};

use nyanpasu_contracts::sync::SyncSettings;
use snafu::{ResultExt as _, Snafu};

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum SettingsError {
    #[snafu(display("failed to read the sync settings"))]
    Read { source: std::io::Error },
    #[snafu(display("the sync settings are not valid YAML"))]
    Decode { source: yaml_serde::Error },
    #[snafu(display("failed to write the sync settings"))]
    Write { source: std::io::Error },
}

/// Reads and atomically replaces the settings document.
pub trait SyncSettingsStore: Send + Sync + 'static {
    fn load(&self) -> Result<SyncSettings, SettingsError>;
    fn save(&self, settings: &SyncSettings) -> Result<(), SettingsError>;
}

/// A YAML file at a fixed path, replaced through a temporary file.
pub struct FileSyncSettingsStore {
    path: std::path::PathBuf,
}

impl FileSyncSettingsStore {
    #[must_use]
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl SyncSettingsStore for FileSyncSettingsStore {
    fn load(&self) -> Result<SyncSettings, SettingsError> {
        match fs::read(&self.path) {
            Ok(bytes) => yaml_serde::from_slice(&bytes).context(DecodeSnafu),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(SyncSettings::default())
            }
            Err(source) => Err(SettingsError::Read { source }),
        }
    }

    fn save(&self, settings: &SyncSettings) -> Result<(), SettingsError> {
        let parent = self.path.parent().ok_or_else(|| SettingsError::Write {
            source: std::io::Error::other("sync settings path has no parent"),
        })?;
        fs::create_dir_all(parent).context(WriteSnafu)?;
        let encoded = yaml_serde::to_string(settings).map_err(|error| SettingsError::Write {
            source: std::io::Error::other(error.to_string()),
        })?;
        let temporary = self.path.with_extension("yaml.partial");
        fs::write(&temporary, encoded).context(WriteSnafu)?;
        fs::rename(&temporary, &self.path).context(WriteSnafu)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nyanpasu_contracts::sync::{ProviderConfig, SyncSettings};

    use super::*;

    #[test]
    fn a_missing_document_loads_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileSyncSettingsStore::new(dir.path().join("local/sync.yaml"));
        assert_eq!(store.load().unwrap(), SyncSettings::default());
    }

    #[test]
    fn settings_round_trip_without_a_secret() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local/sync.yaml");
        let store = FileSyncSettingsStore::new(&path);
        let settings = SyncSettings {
            enabled: true,
            provider: Some(ProviderConfig::Fs {
                root: "/mnt/sync".to_owned(),
            }),
            device_name: "laptop".to_owned(),
            ..Default::default()
        };
        store.save(&settings).unwrap();
        assert_eq!(store.load().unwrap(), settings);
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("laptop"));
        assert!(!written.contains("password"));
    }
}
