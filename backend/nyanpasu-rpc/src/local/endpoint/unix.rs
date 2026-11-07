//! Private-directory sockets with a persistent, OS-locked claim file.

use std::{
    fs::{self, DirBuilder, File, Metadata, OpenOptions, Permissions},
    io,
    os::{
        fd::AsRawFd as _,
        unix::fs::{
            DirBuilderExt as _, FileTypeExt as _, MetadataExt as _, OpenOptionsExt as _,
            PermissionsExt as _,
        },
    },
    path::{Component, Path, PathBuf},
};

use interprocess::local_socket::{GenericFilePath, ListenerOptions, Name, ToFsName as _};
use snafu::{ResultExt as _, ensure};
use socket2::{Domain, SockAddr, Socket, Type};

use super::{
    AccessControlSnafu, AlreadyServingSnafu, BindSnafu, ClaimSnafu, EndpointError,
    EndpointListener, InsecurePathSnafu, InspectSnafu, InvalidLocationSnafu, ReclaimSnafu,
    directory_chain::{self, DirectoryMetadata, FileType},
};

const MAX_SOCKET_PATH_LEN: usize = if cfg!(target_os = "linux") { 107 } else { 103 };

#[cfg(test)]
#[path = "unix_tests.rs"]
mod tests;

fn uid() -> libc::uid_t {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

pub(super) fn current_user() -> Result<String, EndpointError> {
    Ok(uid().to_string())
}

pub(super) fn default_path(user: &str, identity: &str) -> Result<PathBuf, EndpointError> {
    // A fixed, short root keeps discovery independent of shell environment/PID.
    // Canonicalize the system root because macOS's /tmp is /private/tmp.
    let root = Path::new("/tmp")
        .canonicalize()
        .context(InspectSnafu { path: "/tmp" })?;
    let directory = root.join(format!("nyanpasu-rpc-{user}"));
    match DirBuilder::new().mode(0o700).create(&directory) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(source) => {
            return Err(EndpointError::Claim {
                path: directory,
                source,
            });
        }
    }
    private_directory(&directory)?;
    Ok(directory.join(format!("{identity}.sock")))
}

pub(super) fn validate_location(path: &Path) -> Result<(), EndpointError> {
    ensure!(
        path.is_absolute()
            && path.file_name().is_some()
            && path.as_os_str().len() <= MAX_SOCKET_PATH_LEN
            && path
                .components()
                .all(|part| matches!(part, Component::RootDir | Component::Normal(_))),
        InvalidLocationSnafu { path }
    );
    let parent = path
        .parent()
        .ok_or_else(|| EndpointError::InvalidLocation { path: path.into() })?;
    // Inspect from root without following symlinks. A trusted owner and either
    // non-writable or sticky ancestors prevent other users from renaming the path.
    let mut component_path = PathBuf::new();
    let mut paths = Vec::new();
    let mut chain = Vec::new();
    for component in parent.components() {
        component_path.push(component);
        let metadata = fs::symlink_metadata(&component_path).context(InspectSnafu {
            path: &component_path,
        })?;
        paths.push(component_path.clone());
        chain.push(DirectoryMetadata {
            owner: metadata.uid(),
            mode: metadata.mode(),
            file_type: if metadata.is_dir() {
                FileType::Directory
            } else if metadata.file_type().is_symlink() {
                FileType::Symlink
            } else {
                FileType::Other
            },
        });
    }
    if let Some(index) = directory_chain::insecure_component(&chain, uid()) {
        return InsecurePathSnafu {
            path: &paths[index],
        }
        .fail();
    }
    Ok(())
}

fn private_directory(path: &Path) -> Result<Metadata, EndpointError> {
    private_directory_for_user(path, uid())
}

fn private_directory_for_user(path: &Path, user: libc::uid_t) -> Result<Metadata, EndpointError> {
    let metadata = fs::symlink_metadata(path).context(InspectSnafu { path })?;
    ensure!(
        metadata.is_dir() && metadata.uid() == user && metadata.mode() & 0o777 == 0o700,
        InsecurePathSnafu { path }
    );
    Ok(metadata)
}

fn socket_metadata(path: &Path) -> Result<Metadata, EndpointError> {
    socket_metadata_for_user(path, uid())
}

fn socket_metadata_for_user(path: &Path, user: libc::uid_t) -> Result<Metadata, EndpointError> {
    let metadata = fs::symlink_metadata(path).context(InspectSnafu { path })?;
    ensure!(
        metadata.file_type().is_socket()
            && metadata.uid() == user
            && metadata.mode() & 0o777 == 0o600
            && metadata.nlink() == 1,
        InsecurePathSnafu { path }
    );
    Ok(metadata)
}

pub(super) fn validate_connect(path: &Path) -> Result<(), EndpointError> {
    validate_location(path)?;
    socket_metadata(path)?;
    Ok(())
}

pub(super) fn socket_name(path: &Path) -> io::Result<Name<'static>> {
    path.to_owned().to_fs_name::<GenericFilePath>()
}

fn same_file(left: &Metadata, right: &Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

/// The lock file is deliberately never unlinked: unlinking an advisory lock would
/// let a new binder lock a different inode while the old lock is still held.
#[derive(Debug)]
pub(super) struct Claim {
    path: PathBuf,
    directory: Metadata,
    lock: File,
    lock_path: PathBuf,
    socket: Option<Metadata>,
}

impl Claim {
    fn acquire(path: &Path) -> Result<Self, EndpointError> {
        validate_location(path)?;
        let directory = private_directory(
            path.parent()
                .expect("validated absolute socket has a parent"),
        )?;
        let mut lock_path = path.as_os_str().to_owned();
        lock_path.push(".lock");
        let lock_path = PathBuf::from(lock_path);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&lock_path)
            .context(ClaimSnafu { path: &lock_path })?;
        let metadata = lock.metadata().context(InspectSnafu { path: &lock_path })?;
        ensure!(
            metadata.is_file()
                && metadata.uid() == uid()
                && metadata.mode() & 0o777 == 0o600
                && metadata.nlink() == 1,
            InsecurePathSnafu { path: &lock_path }
        );
        // SAFETY: lock owns a live descriptor; nonblocking flock never waits for another binder.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            let source = io::Error::last_os_error();
            if source.kind() == io::ErrorKind::WouldBlock {
                return AlreadyServingSnafu { path }.fail();
            }
            return Err(EndpointError::Claim {
                path: lock_path,
                source,
            });
        }
        let claim = Self {
            path: path.into(),
            directory,
            lock,
            lock_path,
            socket: None,
        };
        claim.validate_identity()?;
        Ok(claim)
    }

    fn validate_identity(&self) -> Result<(), EndpointError> {
        validate_location(&self.path)?;
        let parent = self
            .path
            .parent()
            .expect("validated absolute socket has a parent");
        let directory = private_directory(parent)?;
        let lock = fs::symlink_metadata(&self.lock_path).context(InspectSnafu {
            path: &self.lock_path,
        })?;
        let held = self.lock.metadata().context(InspectSnafu {
            path: &self.lock_path,
        })?;
        ensure!(
            same_file(&self.directory, &directory) && same_file(&held, &lock),
            InsecurePathSnafu { path: &self.path }
        );
        Ok(())
    }

    fn reclaim_stale(
        &self,
        connect: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<(), EndpointError> {
        let metadata = match socket_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(EndpointError::Inspect { source, .. })
                if source.kind() == io::ErrorKind::NotFound =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        match connect(&self.path) {
            Ok(()) => return AlreadyServingSnafu { path: &self.path }.fail(),
            Err(source) if source.kind() == io::ErrorKind::ConnectionRefused => {}
            Err(source) => {
                return Err(EndpointError::Inspect {
                    path: self.path.clone(),
                    source,
                });
            }
        }
        self.validate_identity()?;
        let current = socket_metadata(&self.path)?;
        ensure!(
            same_file(&metadata, &current),
            InsecurePathSnafu { path: &self.path }
        );
        // The claim serializes binders and the validated directory chain excludes
        // replacement by other users. The same user and root are trusted during
        // the interval between this identity check and unlink.
        fs::remove_file(&self.path).context(ReclaimSnafu { path: &self.path })
    }
}

fn probe(path: &Path) -> io::Result<()> {
    // A full listen backlog must not block binding or be mistaken for a crash.
    let probe = Socket::new(Domain::UNIX, Type::STREAM, None)?;
    probe.set_nonblocking(true)?;
    probe.connect(&SockAddr::unix(path)?)
}

impl Drop for Claim {
    fn drop(&mut self) {
        if let Some(owned) = &self.socket
            && self.validate_identity().is_ok()
            && let Ok(current) = fs::symlink_metadata(&self.path)
            && same_file(owned, &current)
        {
            // Keep replacements detected by the identity check. The claim serializes
            // binders; path permissions exclude other users. The same user and root
            // are trusted between this check and unlink.
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub(super) fn bind(path: &Path) -> Result<EndpointListener, EndpointError> {
    let mut claim = Claim::acquire(path)?;
    claim.reclaim_stale(probe)?;
    let listener = ListenerOptions::new()
        .name(socket_name(path).context(BindSnafu { path })?)
        .reclaim_name(false)
        .create_tokio()
        .context(BindSnafu { path })?;
    claim.socket = Some(fs::symlink_metadata(path).context(InspectSnafu { path })?);
    // The containing directory is already private. chmod after bind also works
    // on macOS, which does not support interprocess's pre-bind fchmod technique.
    fs::set_permissions(path, Permissions::from_mode(0o600)).context(AccessControlSnafu)?;
    claim.validate_identity()?;
    socket_metadata(path)?;
    Ok(EndpointListener {
        listener,
        _claim: claim,
    })
}
