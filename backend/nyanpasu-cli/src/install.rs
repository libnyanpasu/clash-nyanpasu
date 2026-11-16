//! `install`: link this executable into a directory on `PATH`, and the standalone copy an
//! AppImage needs for that.
//!
//! An AppImage runs from a temporary mount that disappears with the app, so a link to the
//! executable or the skill folder inside it would dangle. The CLI copies both out to a
//! stable directory and links that copy instead.

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{
    error::CliError,
    placement::{Placed, copy_directory, io_failure, place_link},
    skill::SKILL_NAME,
};

/// The name the executable has on `PATH`.
pub const EXECUTABLE_NAME: &str = "nyanpasu-cli";

/// The system-wide directory whose link needs elevation; `install` only prints its command.
const SYSTEM_BIN_DIR: &str = "/usr/local/bin";

/// A running AppImage and the stable directory its standalone copy goes to.
#[derive(Debug, Clone)]
pub struct AppImage {
    /// The temporary mount (`APPDIR`) the bundled files live under.
    pub mount: PathBuf,
    /// Receives the copied executable and, below `skills/`, the copied skill folder, so
    /// that the copy finds its skill the same way a Windows or portable bundle does.
    pub standalone_dir: PathBuf,
}

impl AppImage {
    /// Whether `executable` runs from this AppImage's mount.
    pub fn runs(&self, executable: &Path) -> bool {
        let real = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_owned());
        real(executable).starts_with(real(&self.mount))
    }

    /// Copies the executable out of the mount, replacing an earlier copy. The copy is
    /// swapped in by a rename, so a copy that is running keeps working.
    ///
    /// # Errors
    /// Fails on a filesystem error.
    pub fn standalone_executable(&self, executable: &Path) -> Result<PathBuf, CliError> {
        let destination = self.standalone_dir.join(EXECUTABLE_NAME);
        let incoming = self.standalone_dir.join(format!("{EXECUTABLE_NAME}.new"));
        fs::create_dir_all(&self.standalone_dir)
            .and_then(|()| fs::copy(executable, &incoming))
            .and_then(|_| fs::rename(&incoming, &destination))
            .map_err(|error| io_failure("copy the executable to", &destination, error))?;
        Ok(destination)
    }

    /// Copies the skill folder out of the mount, replacing an earlier copy.
    ///
    /// # Errors
    /// Fails on a filesystem error.
    pub fn standalone_skill(&self, skill: &Path) -> Result<PathBuf, CliError> {
        let destination = self.standalone_dir.join("skills").join(SKILL_NAME);
        match fs::remove_dir_all(&destination) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
        .and_then(|()| copy_directory(skill, &destination))
        .map_err(|error| io_failure("copy the skill to", &destination, error))?;
        Ok(destination)
    }
}

/// What `install` placed, which the app's "install the command-line tool" action reads
/// from `--json`.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Installed {
    /// The link on `PATH`.
    pub link: PathBuf,
    /// The executable the link points at.
    pub target: PathBuf,
    /// The link already pointed there, so nothing changed.
    pub already_linked: bool,
    /// Whether the link's directory is on `PATH` of this process.
    pub on_path: bool,
    /// A command that links the executable for every user, which needs elevation.
    pub system_command: String,
}

/// Quotes a path as one POSIX shell word.
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', r"'\''"))
}

/// Links `target` as `dir/nyanpasu-cli`.
///
/// An existing entry is never overwritten, except that `force` replaces a symlink and a
/// link to an app that is gone is replaced without it.
///
/// # Errors
/// Fails on a conflicting entry, a filesystem error or a platform without symlinks.
pub fn install(
    target: &Path,
    dir: &Path,
    search_path: Option<&OsStr>,
    force: bool,
) -> Result<Installed, CliError> {
    fs::create_dir_all(dir).map_err(|error| io_failure("create", dir, error))?;
    let link = dir.join(EXECUTABLE_NAME);
    let already_linked = match place_link(target, &link, force)? {
        Placed::Linked => false,
        Placed::AlreadyLinked => true,
        Placed::Refused(error) => return Err(io_failure("link", &link, error)),
    };
    Ok(Installed {
        on_path: search_path
            .is_some_and(|paths| std::env::split_paths(paths).any(|entry| entry == dir)),
        system_command: format!(
            "sudo ln -sf {} {SYSTEM_BIN_DIR}/{EXECUTABLE_NAME}",
            shell_quote(target)
        ),
        link,
        target: target.to_owned(),
        already_linked,
    })
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    fn appimage(root: &Path) -> AppImage {
        AppImage {
            mount: root.join("mount"),
            standalone_dir: root.join("data/nyanpasu-cli"),
        }
    }

    #[test]
    fn quoting_survives_spaces_and_single_quotes() {
        assert_eq!(
            shell_quote(Path::new("/Applications/Clash Nyanpasu.app/x")),
            "'/Applications/Clash Nyanpasu.app/x'"
        );
        assert_eq!(shell_quote(Path::new("/a'b")), r"'/a'\''b'");
    }

    #[cfg(unix)]
    #[test]
    fn install_links_the_executable_and_reports_the_path() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("App.app/Contents/MacOS/nyanpasu-cli");
        let dir = root.path().join("home/.local/bin");
        let on_path: OsString = std::env::join_paths([Path::new("/usr/bin"), &dir]).unwrap();

        let first = install(&target, &dir, Some(&on_path), false).unwrap();
        assert_eq!(first.link, dir.join("nyanpasu-cli"));
        assert_eq!(fs::read_link(&first.link).unwrap(), target);
        assert!(!first.already_linked);
        assert!(first.on_path);
        assert!(first.system_command.starts_with("sudo ln -sf '"));
        assert!(
            first
                .system_command
                .ends_with("/usr/local/bin/nyanpasu-cli")
        );

        let second = install(&target, &dir, Some(OsStr::new("/usr/bin")), false).unwrap();
        assert!(second.already_linked);
        assert!(!second.on_path, "the directory is not on this PATH");
        assert!(!install(&target, &dir, None, false).unwrap().on_path);
    }

    #[cfg(unix)]
    #[test]
    fn install_never_overwrites_a_foreign_entry() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("bundle/nyanpasu-cli");
        let dir = root.path().join("bin");
        fs::create_dir_all(&dir).unwrap();
        let link = dir.join("nyanpasu-cli");

        let other = root.path().join("other");
        fs::write(&other, "other").unwrap();
        std::os::unix::fs::symlink(&other, &link).unwrap();
        assert!(install(&target, &dir, None, false).is_err());
        assert_eq!(fs::read_link(&link).unwrap(), other, "nothing changed");
        install(&target, &dir, None, true).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), target);

        fs::remove_file(&link).unwrap();
        fs::write(&link, "a real executable").unwrap();
        assert!(
            install(&target, &dir, None, true).is_err(),
            "a real file is never replaced, even with --force"
        );
        assert_eq!(fs::read_to_string(&link).unwrap(), "a real executable");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_an_unmounted_app_is_replaced_without_force() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("data/nyanpasu-cli/nyanpasu-cli");
        let dir = root.path().join("bin");
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink(
            root.path().join("tmp/.mount_gone/usr/bin/nyanpasu-cli"),
            dir.join("nyanpasu-cli"),
        )
        .unwrap();

        install(&target, &dir, None, false).unwrap();
        assert_eq!(fs::read_link(dir.join("nyanpasu-cli")).unwrap(), target);
    }

    #[test]
    fn an_appimage_owns_what_runs_from_its_mount() {
        let root = tempfile::tempdir().unwrap();
        let image = appimage(root.path());
        fs::create_dir_all(image.mount.join("usr/bin")).unwrap();
        fs::write(image.mount.join("usr/bin/nyanpasu-cli"), "binary").unwrap();

        assert!(image.runs(&image.mount.join("usr/bin/nyanpasu-cli")));
        assert!(!image.runs(&root.path().join("data/nyanpasu-cli/nyanpasu-cli")));
        assert!(!image.runs(&root.path().join("mount-other/nyanpasu-cli")));
    }

    #[test]
    fn the_standalone_copy_replaces_the_previous_one_and_finds_its_skill() {
        let root = tempfile::tempdir().unwrap();
        let image = appimage(root.path());
        let executable = image.mount.join("usr/bin/nyanpasu-cli");
        let skill = image
            .mount
            .join("usr/lib/Clash Nyanpasu/skills/nyanpasu-cli");
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::create_dir_all(skill.join("references")).unwrap();
        fs::write(&executable, "v1").unwrap();
        fs::write(skill.join("SKILL.md"), "skill v1").unwrap();
        fs::write(skill.join("references/commands.md"), "commands").unwrap();

        let copied = image.standalone_executable(&executable).unwrap();
        let copied_skill = image.standalone_skill(&skill).unwrap();
        assert_eq!(fs::read_to_string(&copied).unwrap(), "v1");
        assert_eq!(
            fs::read_to_string(copied_skill.join("references/commands.md")).unwrap(),
            "commands"
        );
        assert!(
            !image.runs(&copied),
            "the copy no longer depends on the mount"
        );
        assert_eq!(
            crate::skill::locate(None, Some(&copied)).unwrap(),
            copied_skill.canonicalize().unwrap(),
            "the copy finds the skill beside it"
        );

        // A newer app replaces both, and a file the new skill dropped does not linger.
        fs::write(&executable, "v2").unwrap();
        fs::write(skill.join("SKILL.md"), "skill v2").unwrap();
        fs::remove_file(skill.join("references/commands.md")).unwrap();
        image.standalone_executable(&executable).unwrap();
        image.standalone_skill(&skill).unwrap();
        assert_eq!(fs::read_to_string(&copied).unwrap(), "v2");
        assert_eq!(
            fs::read_to_string(copied_skill.join("SKILL.md")).unwrap(),
            "skill v2"
        );
        assert!(!copied_skill.join("references/commands.md").exists());
        assert!(!image.standalone_dir.join("nyanpasu-cli.new").exists());
    }

    #[cfg(unix)]
    #[test]
    fn the_standalone_executable_keeps_its_execute_bit() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = tempfile::tempdir().unwrap();
        let image = appimage(root.path());
        let executable = root.path().join("nyanpasu-cli");
        fs::write(&executable, "binary").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();

        let copied = image.standalone_executable(&executable).unwrap();
        assert_eq!(
            fs::metadata(copied).unwrap().permissions().mode() & 0o111,
            0o111
        );
    }
}
