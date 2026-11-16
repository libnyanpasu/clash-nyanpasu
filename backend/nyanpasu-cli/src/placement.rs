//! Placing the CLI's own files where the user or an agent looks for them: a symlink to
//! the executable on `PATH`, a symlink to the skill folder in an agent's directory.

use std::{fs, io, path::Path};

use crate::error::CliError;

/// What [`place_link`] found or did.
#[derive(Debug)]
pub enum Placed {
    /// A new link now points at the source.
    Linked,
    /// The link already pointed there.
    AlreadyLinked,
    /// The platform refused to create the link; nothing was placed.
    Refused(io::Error),
}

/// Reports a filesystem failure with the action and path.
pub fn io_failure(action: &str, path: &Path, error: io::Error) -> CliError {
    CliError::failed(format!("cannot {action} {}: {error}", path.display()))
}

#[cfg(unix)]
fn link(source: &Path, destination: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(windows)]
fn link(source: &Path, destination: &Path) -> io::Result<()> {
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, destination)
    } else {
        std::os::windows::fs::symlink_file(source, destination)
    }
}

#[cfg(unix)]
fn remove_link(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

#[cfg(windows)]
fn remove_link(path: &Path) -> io::Result<()> {
    // A directory symlink is removed as a directory on Windows.
    fs::remove_dir(path).or_else(|_| fs::remove_file(path))
}

/// Copies a directory tree.
pub fn copy_directory(source: &Path, destination: &Path) -> io::Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Links `destination` to `source`, whose parent directory already exists.
///
/// An existing entry is never overwritten, except that `force` replaces a symlink. A link
/// whose target is gone belongs to an app that was moved, updated or, for an AppImage,
/// unmounted: it is stale rather than anyone's choice, so it is replaced without `force`.
///
/// # Errors
/// Fails on a conflicting entry or a filesystem error other than the platform refusing
/// symlinks, which is [`Placed::Refused`].
pub fn place_link(source: &Path, destination: &Path, force: bool) -> Result<Placed, CliError> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if fs::read_link(destination).is_ok_and(|current| current == source) {
                return Ok(Placed::AlreadyLinked);
            }
            if !force && destination.exists() {
                return Err(CliError::failed(format!(
                    "{} is a link to somewhere else; pass --force to replace it",
                    destination.display()
                )));
            }
            remove_link(destination).map_err(|error| io_failure("replace", destination, error))?;
        }
        Ok(_) => {
            return Err(CliError::failed(format!(
                "{} already exists and is not a link; remove it first",
                destination.display()
            )));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_failure("inspect", destination, error)),
    }
    match link(source, destination) {
        Ok(()) => Ok(Placed::Linked),
        Err(error) => Ok(Placed::Refused(error)),
    }
}
