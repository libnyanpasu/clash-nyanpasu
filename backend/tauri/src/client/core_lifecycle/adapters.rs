#[cfg(target_os = "windows")]
use super::ports::PathNotUtf8Snafu;
use super::ports::{
    BinaryInstaller, ElevatedCopyFailedSnafu, InstallCoreBinaryError, PreparedCoreBinary,
    StartElevatedCopySnafu,
};
use async_trait::async_trait;
#[cfg(target_os = "windows")]
use snafu::OptionExt;
use snafu::{ResultExt, Snafu, ensure};
use std::path::{Path, PathBuf};

pub struct FsBinaryInstaller;

#[async_trait]
impl BinaryInstaller for FsBinaryInstaller {
    async fn install(&self, artifact: &PreparedCoreBinary) -> Result<(), InstallCoreBinaryError> {
        install_binary(artifact).await?;
        // Nothing Nyanpasu runs uses the companion, so failing to publish it fails no
        // core installation; the previous companion stays in place.
        if let Some(companion) = &artifact.companion
            && let Err(error) = {
                let (staged, destination) = (companion.staged(), companion.destination.clone());
                crate::utils::blocking::join(
                    tokio::task::spawn_blocking(move || publish_companion(&staged, &destination))
                        .await,
                )
            }
        {
            tracing::warn!(
                error = %snafu::Report::from_error(error),
                core = %artifact.target,
                "could not publish the files the core archive ships beside the core"
            );
        }
        Ok(())
    }
}

async fn install_binary(artifact: &PreparedCoreBinary) -> Result<(), InstallCoreBinaryError> {
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
        let status = crate::utils::blocking::join(
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
    use crate::client::core_lifecycle::ports::{BinaryInstallProgress, PreparedCoreCompanion};
    use std::sync::Arc;
    use tempfile::TempDir;

    struct NoProgress;

    impl BinaryInstallProgress for NoProgress {
        fn restarting(&self) {}
        fn finished(&self, _: Option<&str>) {}
    }

    /// A companion staged beside `destination` with a `SKILL.md` that says `skill`.
    fn staged(destination: &Path, skill: &str) -> PreparedCoreCompanion {
        let companion = PreparedCoreCompanion {
            staging: tempfile::Builder::new()
                .prefix("meow-alpha.staging-")
                .tempdir_in(destination.parent().unwrap())
                .unwrap(),
            destination: destination.to_path_buf(),
        };
        std::fs::create_dir_all(companion.staged().join("skills/meow-cli")).unwrap();
        std::fs::write(companion.staged().join("skills/meow-cli/SKILL.md"), skill).unwrap();
        companion
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
        let artifact = PreparedCoreBinary {
            target: nyanpasu_config::application::ClashCore::MeowAlpha,
            source,
            destination: dir.path().join("installed-core"),
            staging,
            progress: Arc::new(NoProgress),
            companion: Some(staged(&destination, "new skill")),
        };

        FsBinaryInstaller.install(&artifact).await.unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.path().join("installed-core")).unwrap(),
            "new core"
        );
        assert_eq!(skill(&destination), "new skill");
        assert!(!destination.join("skills/meow-cli/stale.md").exists());
        drop(artifact);
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
        let companion = staged(&destination, "skill");

        publish_companion(&companion.staged(), &destination).unwrap();

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
        let failed = staged(&destination, "unused");
        std::fs::remove_dir_all(failed.staged()).unwrap();

        // The new files are gone, so the swap fails after restoring the only copy.
        publish_companion(&failed.staged(), &destination).unwrap_err();
        assert_eq!(skill(&destination), "old skill");
        assert!(!previous.exists());

        // With a destination in place, a leftover is only in the way.
        std::fs::create_dir_all(previous.join("skills")).unwrap();
        let companion = staged(&destination, "new skill");
        publish_companion(&companion.staged(), &destination).unwrap();
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
        let companion = staged(&destination, "new skill");

        let error = publish_companion(&companion.staged(), &destination).unwrap_err();

        assert!(
            matches!(error, PublishCompanionError::SymbolicLink { .. }),
            "{error}"
        );
        assert_eq!(skill(&destination), "linked skill");
    }
}
