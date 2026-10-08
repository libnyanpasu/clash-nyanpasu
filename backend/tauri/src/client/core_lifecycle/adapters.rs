#[cfg(target_os = "windows")]
use super::ports::PathNotUtf8Snafu;
use super::ports::{
    BinaryInstaller, ElevatedCopyFailedSnafu, InstallCoreBinaryError, PreparedCoreBinary,
    StartElevatedCopySnafu,
};
use async_trait::async_trait;
#[cfg(target_os = "windows")]
use snafu::OptionExt;
use snafu::{ResultExt, ensure};

pub struct FsBinaryInstaller;

#[async_trait]
impl BinaryInstaller for FsBinaryInstaller {
    async fn install(&self, artifact: &PreparedCoreBinary) -> Result<(), InstallCoreBinaryError> {
        if let Err(error) = tokio::fs::copy(&artifact.source, &artifact.destination).await {
            tracing::warn!(%error, "core copy failed; requesting elevated installation");
            let source = artifact.source.clone();
            let destination = artifact.destination.clone();
            // The blocking task itself retains the staging files if its waiter dies.
            let staging = artifact.staging.clone();
            #[cfg(target_os = "windows")]
            let (source, destination) = (
                source
                    .to_str()
                    .context(PathNotUtf8Snafu { path: &source })?
                    .to_owned(),
                destination
                    .to_str()
                    .context(PathNotUtf8Snafu { path: &destination })?
                    .to_owned(),
            );
            let status = nyanpasu_core::tasks::blocking::join(
                tokio::task::spawn_blocking(move || {
                    let _staging = staging;
                    #[cfg(target_os = "windows")]
                    {
                        runas::Command::new("cmd")
                            .args(&[
                                "/C",
                                "copy",
                                "/Y",
                                source.as_str(),
                                destination.trim_start_matches(r"\?\"),
                            ])
                            .status()
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        runas::Command::new("cp")
                            .arg("-f")
                            .arg(source)
                            .arg(destination)
                            .status()
                    }
                })
                .await,
            )
            .context(StartElevatedCopySnafu {
                core: artifact.target,
                destination: &artifact.destination,
            })?;
            ensure!(
                status.success(),
                ElevatedCopyFailedSnafu {
                    core: artifact.target,
                    destination: &artifact.destination,
                    exit_code: status.code(),
                }
            );
        }
        Ok(())
    }
}
