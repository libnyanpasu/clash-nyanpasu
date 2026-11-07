//! Core binary installation and best-effort companion directory publication.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
#[cfg(target_os = "windows")]
use nyanpasu_contracts::core_install::PathNotUtf8Snafu;
use nyanpasu_contracts::{
    core_install::{
        AuthorizeCoreCopySnafu, BinaryInstaller, CoreBinaryInstallRequest, ElevatedCopyFailedSnafu,
        InstallCoreBinaryError, StartElevatedCopySnafu,
    },
    elevation::{ElevationConsent, ElevationOperation},
};
#[cfg(target_os = "windows")]
use snafu::OptionExt;
use snafu::{ResultExt, Snafu, ensure};

use crate::elevation::runner::{CommandRunner, OsCommandRunner};

/// Copies a staged binary, falling back to elevation, then publishes its companion.
pub struct FsBinaryInstaller {
    consent: Arc<dyn ElevationConsent>,
}

impl FsBinaryInstaller {
    /// Uses the shell's policy only when an ordinary copy needs an elevated fallback.
    pub fn new(consent: Arc<dyn ElevationConsent>) -> Self {
        Self { consent }
    }
}

#[async_trait]
impl BinaryInstaller for FsBinaryInstaller {
    async fn install(
        &self,
        request: &CoreBinaryInstallRequest,
    ) -> Result<(), InstallCoreBinaryError> {
        install_binary(request, self.consent.as_ref(), Arc::new(OsCommandRunner)).await?;
        // Nothing Nyanpasu runs uses the companion, so failing to publish it fails no
        // core installation; the previous companion stays in place.
        if let Some(companion) = &request.companion
            && let Err(error) = {
                let (staged, destination) =
                    (companion.staged.clone(), companion.destination.clone());
                crate::blocking::join(
                    tokio::task::spawn_blocking(move || publish_companion(&staged, &destination))
                        .await,
                )
            }
        {
            tracing::warn!(
                error = %snafu::Report::from_error(error),
                core = %request.target,
                "could not publish the files the core archive ships beside the core"
            );
        }
        Ok(())
    }
}

async fn install_binary(
    request: &CoreBinaryInstallRequest,
    consent: &dyn ElevationConsent,
    runner: Arc<dyn CommandRunner>,
) -> Result<(), InstallCoreBinaryError> {
    if let Err(error) = tokio::fs::copy(&request.source, &request.destination).await {
        tracing::warn!(%error, "core copy failed; an elevated copy is needed");
        consent
            .authorize(ElevationOperation::CoreBinaryCopy)
            .context(AuthorizeCoreCopySnafu {
                core: request.target,
                destination: &request.destination,
            })?;
        let source = request.source.clone();
        let destination = request.destination.clone();
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
        let status = crate::blocking::join(
            spawn_staged_copy(&request.staging, move || {
                #[cfg(target_os = "windows")]
                {
                    runner.runas(runas::Command::new("cmd").args(&[
                        "/C",
                        "copy",
                        "/Y",
                        source.as_str(),
                        destination.trim_start_matches(r"\?\"),
                    ]))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    runner.runas(
                        runas::Command::new("cp")
                            .arg("-f")
                            .arg(source)
                            .arg(destination),
                    )
                }
            })
            .await,
        )
        .context(StartElevatedCopySnafu {
            core: request.target,
            destination: &request.destination,
        })?;
        ensure!(
            status.success(),
            ElevatedCopyFailedSnafu {
                core: request.target,
                destination: &request.destination,
                exit_code: status.code(),
            }
        );
    }
    Ok(())
}

/// The task, rather than its waiter, owns the files an elevated copy reads.
fn spawn_staged_copy(
    staging: &std::sync::Arc<tempfile::TempDir>,
    copy: impl FnOnce() -> std::io::Result<std::process::ExitStatus> + Send + 'static,
) -> tokio::task::JoinHandle<std::io::Result<std::process::ExitStatus>> {
    let staging = staging.clone();
    tokio::task::spawn_blocking(move || {
        let _staging = staging;
        copy()
    })
}

/// A failure of replacing a companion with the files of a newer core archive.
#[derive(Debug, Snafu)]
enum PublishCompanionError {
    #[snafu(display("{} is a symbolic link", destination.display()))]
    SymbolicLink { destination: PathBuf },
    #[snafu(display("could not remove the leftover {}", previous.display()))]
    RemoveLeftover {
        previous: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("could not restore the leftover {}", previous.display()))]
    RestoreLeftover {
        previous: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("could not move {} aside", destination.display()))]
    MoveAside {
        destination: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("could not move the new files to {}", destination.display()))]
    MoveNew {
        destination: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display(
        "could not restore {}; the previous files are kept at {}",
        destination.display(),
        previous.display()
    ))]
    Restore {
        destination: PathBuf,
        previous: PathBuf,
        source: std::io::Error,
    },
}

/// Replaces `destination` with the `staged` files. Agents link into the destination,
/// so it is swapped as a whole and never emptied in place.
fn publish_companion(staged: &Path, destination: &Path) -> Result<(), PublishCompanionError> {
    let mut previous = destination.as_os_str().to_os_string();
    previous.push(".previous");
    let previous = PathBuf::from(previous);
    // A swap renames the directory, which would abandon what a link points to.
    ensure!(
        !std::fs::symlink_metadata(destination).is_ok_and(|meta| meta.file_type().is_symlink()),
        SymbolicLinkSnafu { destination }
    );
    // Left by an interrupted swap. Without a destination it is the only copy.
    if previous.exists() {
        if destination.exists() {
            std::fs::remove_dir_all(&previous).context(RemoveLeftoverSnafu {
                previous: &previous,
            })?;
        } else {
            std::fs::rename(&previous, destination).context(RestoreLeftoverSnafu {
                previous: &previous,
            })?;
        }
    }
    let replaces = destination.exists();
    if replaces {
        std::fs::rename(destination, &previous).context(MoveAsideSnafu { destination })?;
    }
    if let Err(error) = std::fs::rename(staged, destination) {
        if replaces {
            std::fs::rename(&previous, destination).context(RestoreSnafu {
                destination,
                previous: &previous,
            })?;
        }
        return Err(error).context(MoveNewSnafu { destination });
    }
    // A running executable keeps its directory on Windows; the next swap removes it.
    if replaces && let Err(error) = std::fs::remove_dir_all(&previous) {
        tracing::warn!(%error, path = %previous.display(), "could not remove the previous files");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_contracts::core_install::CoreBinaryCompanionPaths;
    use std::sync::Arc;
    use tempfile::TempDir;

    struct CountingConsent(std::sync::atomic::AtomicUsize);

    impl ElevationConsent for CountingConsent {
        fn authorize(
            &self,
            operation: ElevationOperation,
        ) -> Result<(), nyanpasu_contracts::elevation::ElevationConsentError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            crate::elevation::RefuseElevation.authorize(operation)
        }
    }

    fn copy_request() -> CoreBinaryInstallRequest {
        let staging = Arc::new(TempDir::new().unwrap());
        let source = staging.path().join("staged");
        std::fs::write(&source, "core binary").unwrap();
        CoreBinaryInstallRequest {
            target: nyanpasu_config::application::ClashCore::Mihomo,
            source,
            destination: staging.path().join("installed"),
            staging,
            companion: None,
        }
    }

    #[tokio::test]
    async fn refused_copy_fallback_never_spawns() {
        use crate::elevation::runner::CountingRunner;
        use nyanpasu_contracts::elevation::ElevationConsentError;
        use std::sync::atomic::Ordering;

        let request = copy_request();
        // A directory is a deterministic copy failure, without changing permissions.
        std::fs::create_dir(&request.destination).unwrap();
        let runner = Arc::new(CountingRunner::default());
        let consent = CountingConsent(Default::default());
        let error = install_binary(&request, &consent, runner.clone())
            .await
            .unwrap_err();
        assert_eq!(runner.0.load(Ordering::SeqCst), 0);
        assert_eq!(consent.0.load(Ordering::SeqCst), 1);
        assert!(matches!(
            error,
            InstallCoreBinaryError::AuthorizeCoreCopy {
                core: nyanpasu_config::application::ClashCore::Mihomo,
                source: ElevationConsentError::AuthorizeElevation {
                    operation: ElevationOperation::CoreBinaryCopy
                },
                ..
            }
        ));
        assert!(request.destination.is_dir());
        assert_eq!(
            std::fs::read_to_string(&request.source).unwrap(),
            "core binary"
        );
    }

    #[tokio::test]
    async fn successful_plain_copy_does_not_request_elevation() {
        use crate::elevation::runner::CountingRunner;
        use std::sync::atomic::Ordering;

        let request = copy_request();
        let runner = Arc::new(CountingRunner::default());
        let consent = CountingConsent(Default::default());
        install_binary(&request, &consent, runner.clone())
            .await
            .unwrap();
        assert_eq!(consent.0.load(Ordering::SeqCst), 0);
        assert_eq!(runner.0.load(Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::read_to_string(&request.destination).unwrap(),
            "core binary"
        );
    }

    #[tokio::test]
    async fn install_request_keeps_staging_when_waiter_is_dropped() {
        use futures::FutureExt as _;

        let staging = Arc::new(TempDir::new().unwrap());
        let source = staging.path().join("core");
        std::fs::write(&source, "staged binary").unwrap();
        let request = CoreBinaryInstallRequest {
            target: nyanpasu_config::application::ClashCore::Mihomo,
            source: source.clone(),
            destination: staging.path().join("installed"),
            staging,
            companion: None,
        };
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let copy_source = source.clone();
        let mut task = spawn_staged_copy(&request.staging, move || {
            entered.send(()).unwrap();
            released.recv().unwrap();
            assert_eq!(
                std::fs::read_to_string(copy_source).unwrap(),
                "staged binary"
            );
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        });
        started.await.unwrap();
        let waiter = async {
            let request = request;
            let result = (&mut task).await;
            drop(request);
            result
        };
        assert!(waiter.now_or_never().is_none());
        let exists_after_drop = source.exists();
        // Always unblock the task, even when the lifetime assertion will fail.
        release.send(()).unwrap();
        let result = task.await;
        assert!(
            exists_after_drop,
            "the blocking copy must retain the staging lease"
        );
        assert_eq!(
            result.unwrap().unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert!(
            !source.exists(),
            "the completed task releases its staging lease"
        );
    }

    /// A companion staged beside `destination` with a `SKILL.md` that says `skill`.
    fn staged(destination: &Path, skill: &str) -> (TempDir, PathBuf) {
        let staging = tempfile::Builder::new()
            .prefix("meow-alpha.staging-")
            .tempdir_in(destination.parent().unwrap())
            .unwrap();
        let staged = staging.path().join("new");
        std::fs::create_dir_all(staged.join("skills/meow-cli")).unwrap();
        std::fs::write(staged.join("skills/meow-cli/SKILL.md"), skill).unwrap();
        (staging, staged)
    }

    fn skill(destination: &Path) -> String {
        std::fs::read_to_string(destination.join("skills/meow-cli/SKILL.md")).unwrap()
    }

    #[tokio::test]
    async fn the_companion_is_published_once_the_binary_is_installed() {
        let dir = TempDir::new().unwrap();
        let destination = dir.path().join("meow-alpha");
        std::fs::create_dir_all(destination.join("skills/meow-cli")).unwrap();
        std::fs::write(destination.join("skills/meow-cli/stale.md"), "stale").unwrap();
        let staging = Arc::new(TempDir::new().unwrap());
        let source = staging.path().join("meow");
        std::fs::write(&source, "new core").unwrap();
        let (companion, staged) = staged(&destination, "new skill");
        let request = CoreBinaryInstallRequest {
            target: nyanpasu_config::application::ClashCore::MeowAlpha,
            source,
            destination: dir.path().join("installed-core"),
            staging,
            companion: Some(CoreBinaryCompanionPaths {
                staged,
                destination: destination.clone(),
            }),
        };
        FsBinaryInstaller::new(Arc::new(crate::elevation::RefuseElevation))
            .install(&request)
            .await
            .unwrap();
        drop(request);

        assert_eq!(
            std::fs::read_to_string(dir.path().join("installed-core")).unwrap(),
            "new core"
        );
        assert_eq!(skill(&destination), "new skill");
        assert!(!destination.join("skills/meow-cli/stale.md").exists());
        drop(companion);
        // Only the installed core and the companion remain.
        let mut names = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, ["installed-core", "meow-alpha"]);
    }

    #[test]
    fn a_first_companion_is_published_where_none_was() {
        let dir = TempDir::new().unwrap();
        let destination = dir.path().join("meow-alpha");
        let (_companion, staged) = staged(&destination, "skill");

        publish_companion(&staged, &destination).unwrap();

        assert_eq!(skill(&destination), "skill");
    }

    #[test]
    fn the_previous_files_left_by_an_interrupted_swap_are_restored_or_removed() {
        let dir = TempDir::new().unwrap();
        let destination = dir.path().join("meow-alpha");
        let previous = dir.path().join("meow-alpha.previous");
        // Interrupted between moving the old files aside and moving the new ones in.
        std::fs::create_dir_all(previous.join("skills/meow-cli")).unwrap();
        std::fs::write(previous.join("skills/meow-cli/SKILL.md"), "old skill").unwrap();
        let (_failed, failed) = staged(&destination, "unused");
        std::fs::remove_dir_all(&failed).unwrap();

        // The new files are gone, so the swap fails after restoring the only copy.
        publish_companion(&failed, &destination).unwrap_err();
        assert_eq!(skill(&destination), "old skill");
        assert!(!previous.exists());

        // With a destination in place, a leftover is only in the way.
        std::fs::create_dir_all(previous.join("skills")).unwrap();
        let (_companion, staged) = staged(&destination, "new skill");
        publish_companion(&staged, &destination).unwrap();
        assert_eq!(skill(&destination), "new skill");
        assert!(!previous.exists());
    }

    #[test]
    fn a_failed_swap_restores_the_previous_files() {
        let dir = TempDir::new().unwrap();
        let destination = dir.path().join("meow-alpha");
        std::fs::create_dir_all(destination.join("skills/meow-cli")).unwrap();
        std::fs::write(destination.join("skills/meow-cli/SKILL.md"), "old skill").unwrap();

        let error = publish_companion(&dir.path().join("missing"), &destination).unwrap_err();

        assert!(
            matches!(error, PublishCompanionError::MoveNew { .. }),
            "{error}"
        );
        assert_eq!(skill(&destination), "old skill");
        assert!(!dir.path().join("meow-alpha.previous").exists());
    }

    #[cfg(target_family = "unix")]
    #[test]
    fn a_linked_destination_is_left_alone() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("elsewhere");
        std::fs::create_dir_all(target.join("skills/meow-cli")).unwrap();
        std::fs::write(target.join("skills/meow-cli/SKILL.md"), "linked skill").unwrap();
        let destination = dir.path().join("meow-alpha");
        std::os::unix::fs::symlink(&target, &destination).unwrap();
        let (_companion, staged) = staged(&destination, "new skill");

        let error = publish_companion(&staged, &destination).unwrap_err();

        assert!(
            matches!(error, PublishCompanionError::SymbolicLink { .. }),
            "{error}"
        );
        assert_eq!(skill(&destination), "linked skill");
    }
}
