//! Sidecar version probes using a shell-provided executable source.

use std::sync::Arc;

use async_trait::async_trait;
use nyanpasu_config::application::ClashCore;
use snafu::{ResultExt, ensure};

use nyanpasu_contracts::core_version::{
    self, CoreVersionError, CoreVersionExecutableSource, CoreVersionExitSnafu, CoreVersionReader,
    RunCoreVersionSnafu,
};

pub struct SidecarCoreVersionReader {
    executable_source: Arc<dyn CoreVersionExecutableSource>,
}

impl SidecarCoreVersionReader {
    pub fn new(executable_source: Arc<dyn CoreVersionExecutableSource>) -> Self {
        Self { executable_source }
    }
}

#[async_trait]
impl CoreVersionReader for SidecarCoreVersionReader {
    async fn read(&self, core: ClashCore) -> Result<String, CoreVersionError> {
        let argument = match core {
            ClashCore::ClashRs | ClashCore::ClashRsAlpha => "-V",
            _ => "-v",
        };
        let executable = self.executable_source.current_exe(core)?;
        let directory = executable.parent().expect("the executable has a parent");
        let directory = if directory.ends_with("deps") {
            directory.parent().unwrap_or(directory)
        } else {
            directory
        };
        let mut binary = directory.join(core.binary_name());
        #[cfg(windows)]
        if binary.extension().is_none_or(|ext| ext != "exe") {
            binary.as_mut_os_string().push(".exe");
        }
        #[cfg(not(windows))]
        if binary.extension().is_some_and(|ext| ext == "exe") {
            binary.set_extension("");
        }
        let mut command = tokio::process::Command::new(binary);
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW, as tauri-plugin-shell does.
        let output = command
            .arg(argument)
            .output()
            .await
            .map_err(anyhow::Error::from)
            .context(RunCoreVersionSnafu { core })?;
        ensure!(output.status.success(), CoreVersionExitSnafu { core });
        core_version::parse_version(core, &String::from_utf8_lossy(&output.stdout))
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use nyanpasu_contracts::core_version::MockCoreVersionExecutableSource;

    use super::*;

    #[tokio::test]
    async fn version_probe_resolves_executable_on_each_read() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first/app.exe");
        let second = dir.path().join("second/deps/app.exe");
        let mut sequence = mockall::Sequence::new();
        let mut source = MockCoreVersionExecutableSource::new();
        source
            .expect_current_exe()
            .with(mockall::predicate::eq(ClashCore::Mihomo))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(move |_| Ok(first));
        source
            .expect_current_exe()
            .with(mockall::predicate::eq(ClashCore::Mihomo))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(move |_| Ok(second));
        let reader = SidecarCoreVersionReader::new(Arc::new(source));
        // The same core twice: a reader that cached the path per core would
        // make only the first call.
        for core in [ClashCore::Mihomo; 2] {
            let error = reader.read(core).await.unwrap_err();
            assert!(
                matches!(error, CoreVersionError::RunCoreVersion { core: actual, .. } if actual == core)
            );
        }
    }

    #[tokio::test]
    async fn version_probe_resolution_failure_stays_a_read_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing: PathBuf = dir.path().join("missing");
        let cause = std::fs::read(missing).unwrap_err();
        let expected = cause.to_string();
        let mut source = MockCoreVersionExecutableSource::new();
        source
            .expect_current_exe()
            .times(1)
            .return_once(move |core| {
                Err(anyhow::Error::from(cause)).context(RunCoreVersionSnafu { core })
            });
        let reader = SidecarCoreVersionReader::new(Arc::new(source));
        let error = reader.read(ClashCore::Mihomo).await.unwrap_err();
        match error {
            CoreVersionError::RunCoreVersion { core, source } => {
                assert_eq!(core, ClashCore::Mihomo);
                assert_eq!(source.to_string(), expected);
                assert!(source.downcast_ref::<std::io::Error>().is_some());
            }
            other => panic!("resolution failure changed: {other:?}"),
        }
    }
}
