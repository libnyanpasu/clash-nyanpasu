use async_trait::async_trait;
use nyanpasu_core::runtime::binary::{BinaryInstaller, InstallCoreBinaryError, PreparedCoreBinary};

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
                    .ok_or_else(|| InstallCoreBinaryError::PathNotUtf8 {
                        path: (&source).into(),
                    })?
                    .to_owned(),
                destination
                    .to_str()
                    .ok_or_else(|| InstallCoreBinaryError::PathNotUtf8 {
                        path: (&destination).into(),
                    })?
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
            .map_err(|source| InstallCoreBinaryError::StartElevatedCopy {
                core: artifact.target,
                destination: (&artifact.destination).into(),
                source,
            })?;
            if !status.success() {
                return Err(InstallCoreBinaryError::ElevatedCopyFailed {
                    core: artifact.target,
                    destination: (&artifact.destination).into(),
                    exit_code: status.code(),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::application::ClashCore;
    use nyanpasu_core::runtime::binary::BinaryInstallProgress;
    use std::sync::Arc;

    struct WorkflowProgress;

    impl BinaryInstallProgress for WorkflowProgress {
        fn restarting(&self) {
            panic!("only the workflow reports restarts");
        }

        fn finished(&self, _: Option<&str>) {
            panic!("only the workflow reports the terminal result");
        }
    }

    #[tokio::test]
    async fn installs_by_copy_without_reporting_workflow_progress() {
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let installed = tempfile::tempdir().unwrap();
        let source = staging.path().join("new-core");
        let destination = installed.path().join("installed-core");
        std::fs::write(&source, b"new binary").unwrap();
        std::fs::write(&destination, b"old binary").unwrap();
        let artifact = PreparedCoreBinary {
            target: ClashCore::Mihomo,
            source: source.clone(),
            destination: destination.clone(),
            staging,
            progress: Arc::new(WorkflowProgress),
        };
        FsBinaryInstaller.install(&artifact).await.unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"new binary");
        assert!(
            source.exists(),
            "installation retains staging until the workflow finishes"
        );
        drop(artifact);
        assert!(!source.exists());
    }
}
