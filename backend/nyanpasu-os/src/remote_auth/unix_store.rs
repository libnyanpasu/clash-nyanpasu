//! Unix handle-verified private directory/file protection and atomic replacement.

use std::{
    ffi::{CString, OsStr},
    fs::{self, DirBuilder, File},
    io::{self, Write},
    os::{
        fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd},
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, MetadataExt},
        },
    },
    path::{Component, Path, PathBuf},
};

use nyanpasu_contracts::auth::{LoadedSecretRecord, SecretStoreError, StorageProtection};

use super::secret_store::{
    generate_temp_file_name, normalize_lexical, read_bounded_bytes, split_target_path,
};

pub(super) fn probe_protection(
    secrets_path: PathBuf,
    _max_record_bytes: usize,
) -> Result<StorageProtection, SecretStoreError> {
    let (raw_dir, file_name) = split_target_path(&secrets_path)?;
    let euid = current_euid();

    let raw_ancestors_safe = validate_ancestors(&raw_dir, euid)?;
    let lexical_ancestors_safe = validate_ancestors(&normalize_lexical(&raw_dir), euid)?;
    let Some((dir_fd, dir_meta)) = try_open_dir_nofollow(&raw_dir)? else {
        return Ok(if raw_ancestors_safe && lexical_ancestors_safe {
            StorageProtection::Protected
        } else {
            StorageProtection::Unprotected
        });
    };

    let resolved_dir =
        fs::canonicalize(&raw_dir).map_err(map_io_err("canonicalizing private directory"))?;
    let ancestors_safe = raw_ancestors_safe && validate_ancestors(&resolved_dir, euid)?;
    let dir_safe = validate_dir_fd(&raw_dir, &dir_meta, euid)?;
    let file_cstr = to_cstring(&file_name)?;
    let file_safe = match try_open_file_at(&dir_fd, &file_cstr, libc::O_RDONLY)? {
        Some(file) => validate_regular_file(&file, &dir_fd, &file_cstr, euid, true)?.0,
        None => true,
    };

    Ok(if ancestors_safe && dir_safe && file_safe {
        StorageProtection::Protected
    } else {
        StorageProtection::Unprotected
    })
}

pub(super) fn load_record(
    secrets_path: PathBuf,
    max_record_bytes: usize,
) -> Result<Option<LoadedSecretRecord>, SecretStoreError> {
    let (raw_dir, file_name) = split_target_path(&secrets_path)?;
    let euid = current_euid();

    let Some((dir_fd, dir_meta)) = try_open_dir_nofollow(&raw_dir)? else {
        return Ok(None);
    };

    let resolved_dir =
        fs::canonicalize(&raw_dir).map_err(map_io_err("canonicalizing private directory"))?;
    let ancestors_safe =
        validate_ancestors(&raw_dir, euid)? && validate_ancestors(&resolved_dir, euid)?;
    let dir_safe = validate_dir_fd(&raw_dir, &dir_meta, euid)?;
    let file_cstr = to_cstring(&file_name)?;
    let Some(mut file) = try_open_file_at(&dir_fd, &file_cstr, libc::O_RDONLY)? else {
        return Ok(None);
    };

    let (file_safe, file_meta) = validate_regular_file(&file, &dir_fd, &file_cstr, euid, false)?;
    if file_meta.size() > max_record_bytes as u64 {
        return Err(SecretStoreError::Io {
            reason: "secret store file size exceeds maximum byte limit".to_owned(),
        });
    }

    let bytes = read_bounded_bytes(&mut file, max_record_bytes)?;
    let protection = if ancestors_safe && dir_safe && file_safe {
        StorageProtection::Protected
    } else {
        StorageProtection::Unprotected
    };
    Ok(Some(LoadedSecretRecord { bytes, protection }))
}

pub(super) fn save_record(
    secrets_path: &Path,
    bytes: &[u8],
    max_record_bytes: usize,
    require_protected: bool,
) -> Result<(), SecretStoreError> {
    if bytes.len() > max_record_bytes || max_record_bytes == 0 {
        return Err(SecretStoreError::Io {
            reason: "secret store record exceeds maximum byte limit".to_owned(),
        });
    }
    let (raw_dir, file_name) = split_target_path(secrets_path)?;
    let euid = current_euid();

    // Conservative preflight before directory creation: check raw ancestry AND lexical collapse,
    // without ever redirecting kernel I/O away from `raw_dir` (which would alter `/var/../x` on macOS).
    let raw_ancestors_safe = validate_ancestors(&raw_dir, euid)?;
    let lexical_ancestors_safe = validate_ancestors(&normalize_lexical(&raw_dir), euid)?;
    if require_protected && !(raw_ancestors_safe && lexical_ancestors_safe) {
        return Err(SecretStoreError::UnprotectedMedium);
    }

    ensure_parent_ancestors(&raw_dir)?;
    let dir_fd = open_or_create_private_dir(&raw_dir, euid)?;

    // Full post-creation ancestry + directory + existing target verification BEFORE creating
    // any temp file or writing any plaintext bytes to disk.
    let resolved_dir =
        fs::canonicalize(&raw_dir).map_err(map_io_err("canonicalizing private directory"))?;
    let full_ancestors_safe =
        validate_ancestors(&raw_dir, euid)? && validate_ancestors(&resolved_dir, euid)?;
    let dir_meta = fd_metadata(&dir_fd)?;
    let dir_safe = validate_dir_fd(&raw_dir, &dir_meta, euid)?;
    if require_protected && !(full_ancestors_safe && dir_safe) {
        return Err(SecretStoreError::UnprotectedMedium);
    }

    // Check any pre-existing target file before creating a temp file so we never touch an
    // untrusted symlink/hardlink/FIFO/foreign target or clobber a relaxed existing record silently.
    let target_cstr = to_cstring(&file_name)?;
    if let Some(existing_file) = try_open_file_at(&dir_fd, &target_cstr, libc::O_RDONLY)? {
        let (existing_safe, _) =
            validate_regular_file(&existing_file, &dir_fd, &target_cstr, euid, false)?;
        if require_protected && !existing_safe {
            return Err(SecretStoreError::UnprotectedMedium);
        }
    }

    let temp_name = generate_temp_file_name(&file_name)?;
    let temp_cstr = to_cstring(&temp_name)?;
    let mut temp_file = create_temp_file_at(&dir_fd, &temp_cstr)?;

    let write_result = (|| -> Result<(), SecretStoreError> {
        let (temp_safe, _) = validate_regular_file(&temp_file, &dir_fd, &temp_cstr, euid, false)?;
        if require_protected && !temp_safe {
            return Err(SecretStoreError::UnprotectedMedium);
        }

        temp_file
            .write_all(bytes)
            .map_err(map_io_err("writing temp secret file"))?;
        temp_file
            .flush()
            .map_err(map_io_err("flushing temp secret file"))?;
        temp_file
            .sync_all()
            .map_err(map_io_err("syncing temp secret file"))?;

        // Re-verify protection immediately on the open handles before replacing target.
        let latest_ancestors =
            validate_ancestors(&raw_dir, euid)? && validate_ancestors(&resolved_dir, euid)?;
        let latest_dir_meta = fd_metadata(&dir_fd)?;
        let latest_dir = validate_dir_fd(&raw_dir, &latest_dir_meta, euid)?;
        let (latest_temp, _) = validate_regular_file(&temp_file, &dir_fd, &temp_cstr, euid, false)?;
        if require_protected && !(latest_ancestors && latest_dir && latest_temp) {
            return Err(SecretStoreError::UnprotectedMedium);
        }
        Ok(())
    })();

    drop(temp_file);
    if let Err(err) = write_result {
        let _ = unlink_at(&dir_fd, &temp_cstr);
        return Err(err);
    }

    // Atomic commit point: `renameat` atomically replaces `auth-secrets.json`. Prior errors abort
    // cleanly above and preserve the old file. Post-rename directory fsync is best-effort crash
    // durability only (see `sync_dir_best_effort`).
    if let Err(err) = rename_at(&dir_fd, &temp_cstr, &dir_fd, &target_cstr) {
        let _ = unlink_at(&dir_fd, &temp_cstr);
        return Err(err);
    }

    sync_dir_best_effort(&dir_fd);
    Ok(())
}

fn current_euid() -> u32 {
    // SAFETY: POSIX `geteuid` has no preconditions and always succeeds.
    unsafe { libc::geteuid() }
}

fn validate_ancestors(private_dir: &Path, euid: u32) -> Result<bool, SecretStoreError> {
    let Some(parent_of_private) = private_dir.parent() else {
        return Err(SecretStoreError::Io {
            reason: "remote-access secret directory has no parent".to_owned(),
        });
    };

    let mut current = PathBuf::new();
    let mut safe = true;
    for component in parent_of_private.components() {
        current.push(component);
        if component == Component::RootDir {
            // Also inspect `/`.
        }
        let meta = match fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(safe),
            Err(err) => return Err(map_io_err("inspecting secret directory ancestor")(err)),
        };

        let effective_meta = if meta.file_type().is_symlink() {
            // Allow OS-managed root-owned symlinks (for example `/var` -> `private/var` on macOS)
            // only if the symlink itself is root-owned and its resolved target is valid.
            if meta.uid() != 0 {
                return Err(SecretStoreError::Io {
                    reason: "refusing symlink in remote-access secret directory ancestor chain"
                        .to_owned(),
                });
            }
            fs::metadata(&current).map_err(map_io_err("inspecting root symlink ancestor target"))?
        } else {
            meta
        };

        if !effective_meta.is_dir() {
            return Err(SecretStoreError::Io {
                reason: "secret directory ancestor is not a directory".to_owned(),
            });
        }
        let uid = effective_meta.uid();
        let mode = effective_meta.mode();
        if (uid != 0 && uid != euid) || ((mode & 0o022) != 0 && (mode & 0o1000) == 0) {
            safe = false;
        }
    }

    Ok(safe)
}

fn ensure_parent_ancestors(private_dir: &Path) -> Result<(), SecretStoreError> {
    let Some(parent) = private_dir.parent() else {
        return Ok(());
    };
    let mut current = PathBuf::new();
    for component in parent.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    if meta.uid() != 0 {
                        return Err(SecretStoreError::Io {
                            reason:
                                "refusing symlink in remote-access secret directory ancestor chain"
                                    .to_owned(),
                        });
                    }
                } else if !meta.is_dir() {
                    return Err(SecretStoreError::Io {
                        reason: "secret directory ancestor is not a directory".to_owned(),
                    });
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                let mut builder = DirBuilder::new();
                builder.mode(0o700);
                match builder.create(&current) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(map_io_err("creating parent directory")(e)),
                }
            }
            Err(err) => return Err(map_io_err("checking parent directory")(err)),
        }
    }
    Ok(())
}

fn open_or_create_private_dir(private_dir: &Path, euid: u32) -> Result<OwnedFd, SecretStoreError> {
    if let Some((fd, _)) = try_open_dir_nofollow(private_dir)? {
        return Ok(fd);
    }

    let mut builder = DirBuilder::new();
    builder.mode(0o700);
    match builder.create(private_dir) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(map_io_err("creating private secret directory")(err)),
    }

    let (fd, meta) = try_open_dir_nofollow(private_dir)?.ok_or_else(|| SecretStoreError::Io {
        reason: "private secret directory vanished immediately after creation".to_owned(),
    })?;
    let _ = validate_dir_fd(private_dir, &meta, euid)?;
    Ok(fd)
}

fn try_open_dir_nofollow(dir: &Path) -> Result<Option<(OwnedFd, fs::Metadata)>, SecretStoreError> {
    let dir_cstr = CString::new(dir.as_os_str().as_bytes()).map_err(|_| SecretStoreError::Io {
        reason: "secret directory path contains interior NUL byte".to_owned(),
    })?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: `dir_cstr` is NUL-terminated and valid for this call.
    let raw_fd = unsafe { libc::open(dir_cstr.as_ptr(), flags) };
    if raw_fd < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(classify_open_error("opening private secret directory", err));
    }
    // SAFETY: `raw_fd` is newly opened and owned by us.
    let fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
    let meta = fd_metadata(&fd)?;
    Ok(Some((fd, meta)))
}

fn validate_dir_fd(
    dir_path: &Path,
    fd_meta: &fs::Metadata,
    euid: u32,
) -> Result<bool, SecretStoreError> {
    if !fd_meta.is_dir() {
        return Err(SecretStoreError::Io {
            reason: "remote-access secret directory is not a directory".to_owned(),
        });
    }
    if fd_meta.uid() != euid {
        return Err(SecretStoreError::Io {
            reason: "remote-access secret directory is not owned by current user".to_owned(),
        });
    }
    let path_lstat = fs::symlink_metadata(dir_path).map_err(map_io_err("stat secret directory"))?;
    if path_lstat.file_type().is_symlink() {
        return Err(SecretStoreError::Io {
            reason: "refusing symlink for remote-access secret directory".to_owned(),
        });
    }
    if path_lstat.dev() != fd_meta.dev() || path_lstat.ino() != fd_meta.ino() {
        return Err(SecretStoreError::Io {
            reason: "remote-access secret directory changed during validation".to_owned(),
        });
    }
    Ok((fd_meta.mode() & 0o7777) == 0o700)
}

fn try_open_file_at(
    dir_fd: &OwnedFd,
    file_cstr: &CString,
    access_flags: libc::c_int,
) -> Result<Option<File>, SecretStoreError> {
    // Pass `O_NONBLOCK` so opening a FIFO or special node never blocks before `fstat` rejects it.
    let flags = access_flags | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: `dir_fd` is an open directory fd and `file_cstr` is a NUL-terminated component name.
    let raw_fd = unsafe { libc::openat(dir_fd.as_raw_fd(), file_cstr.as_ptr(), flags) };
    if raw_fd < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(classify_open_error("opening secret store file", err));
    }
    // SAFETY: `raw_fd` is newly opened and owned by us.
    Ok(Some(unsafe { File::from_raw_fd(raw_fd) }))
}

fn create_temp_file_at(dir_fd: &OwnedFd, temp_cstr: &CString) -> Result<File, SecretStoreError> {
    let flags = libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: `dir_fd` is open and `temp_cstr` is NUL-terminated; mode `0o600` is passed for O_CREAT.
    let raw_fd = unsafe {
        libc::openat(
            dir_fd.as_raw_fd(),
            temp_cstr.as_ptr(),
            flags,
            0o600 as libc::mode_t,
        )
    };
    if raw_fd < 0 {
        return Err(map_io_err("creating temp secret file")(
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: `raw_fd` is newly opened and owned by us.
    Ok(unsafe { File::from_raw_fd(raw_fd) })
}

fn validate_regular_file(
    file: &File,
    dir_fd: &OwnedFd,
    file_cstr: &CString,
    euid: u32,
    allow_missing_lstat: bool,
) -> Result<(bool, fs::Metadata), SecretStoreError> {
    let meta = file.metadata().map_err(map_io_err("fstat secret file"))?;
    if !meta.is_file() {
        return Err(SecretStoreError::Io {
            reason: "secret store target is not a regular file".to_owned(),
        });
    }
    if meta.uid() != euid {
        return Err(SecretStoreError::Io {
            reason: "secret store file is not owned by current user".to_owned(),
        });
    }
    if meta.nlink() > 1 {
        return Err(SecretStoreError::Io {
            reason: "refusing hardlinked secret store file".to_owned(),
        });
    }

    match lstat_at(dir_fd, file_cstr)? {
        Some(lstat) => {
            // `st_mode`/`S_IF*` are `u16` on macOS and `u32` on Linux; `st_dev`/`st_ino` widths vary.
            #[allow(clippy::useless_conversion)]
            {
                if (u32::from(lstat.st_mode) & u32::from(libc::S_IFMT)) == u32::from(libc::S_IFLNK)
                {
                    return Err(SecretStoreError::Io {
                        reason: "refusing symlinked secret store file".to_owned(),
                    });
                }
                if u64::try_from(lstat.st_dev) != Ok(meta.dev())
                    || u64::try_from(lstat.st_ino) != Ok(meta.ino())
                {
                    return Err(SecretStoreError::Io {
                        reason: "secret store file changed during validation".to_owned(),
                    });
                }
            }
        }
        None if allow_missing_lstat => {}
        None => {
            return Err(SecretStoreError::Io {
                reason: "secret store file vanished during validation".to_owned(),
            });
        }
    }

    Ok(((meta.mode() & 0o7777) == 0o600, meta))
}

fn fd_metadata(fd: &OwnedFd) -> Result<fs::Metadata, SecretStoreError> {
    // Borrow into a `ManuallyDrop<File>` to query metadata without duplicating or closing `fd`.
    // SAFETY: `fd.as_raw_fd()` stays open for the lifetime of `borrowed`.
    let borrowed = std::mem::ManuallyDrop::new(unsafe { File::from_raw_fd(fd.as_raw_fd()) });
    borrowed
        .metadata()
        .map_err(map_io_err("fstat directory handle"))
}

fn lstat_at(dir_fd: &OwnedFd, file_cstr: &CString) -> Result<Option<libc::stat>, SecretStoreError> {
    let mut stat_buf = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: `dir_fd` is open, `file_cstr` is NUL-terminated, and `stat_buf` is valid writable memory.
    let rc = unsafe {
        libc::fstatat(
            dir_fd.as_raw_fd(),
            file_cstr.as_ptr(),
            stat_buf.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(map_io_err("lstat secret file")(err));
    }
    // SAFETY: `fstatat` initialized `stat_buf` on success.
    Ok(Some(unsafe { stat_buf.assume_init() }))
}

fn rename_at(
    old_dir: &OwnedFd,
    old_name: &CString,
    new_dir: &OwnedFd,
    new_name: &CString,
) -> Result<(), SecretStoreError> {
    // SAFETY: both directory fds are open and both names are valid NUL-terminated strings.
    let rc = unsafe {
        libc::renameat(
            old_dir.as_raw_fd(),
            old_name.as_ptr(),
            new_dir.as_raw_fd(),
            new_name.as_ptr(),
        )
    };
    if rc < 0 {
        return Err(map_io_err("atomically replacing secret store file")(
            io::Error::last_os_error(),
        ));
    }
    Ok(())
}

fn unlink_at(dir_fd: &OwnedFd, name: &CString) -> Result<(), SecretStoreError> {
    // SAFETY: `dir_fd` is open and `name` is NUL-terminated.
    let rc = unsafe { libc::unlinkat(dir_fd.as_raw_fd(), name.as_ptr(), 0) };
    if rc < 0 {
        return Err(map_io_err("removing temp secret file")(
            io::Error::last_os_error(),
        ));
    }
    Ok(())
}

fn sync_dir_best_effort(dir_fd: &OwnedFd) {
    let borrowed: BorrowedFd<'_> = unsafe { BorrowedFd::borrow_raw(dir_fd.as_raw_fd()) };
    // Best-effort crash durability only: `renameat` has already atomically replaced the file in the
    // directory namespace. If directory `fsync` fails (e.g., unsupported filesystem error or I/O
    // failure after commit), we log at debug level rather than returning an error that would
    // contradict the already-committed replacement; sudden power loss may still lose the new
    // directory entry on filesystems that fail directory `fsync`.
    // SAFETY: `borrowed` is an open directory descriptor.
    if unsafe { libc::fsync(borrowed.as_raw_fd()) } < 0 {
        let err = io::Error::last_os_error();
        tracing::debug!(kind = ?err.kind(), "best-effort post-rename directory fsync did not complete");
    }
}

fn to_cstring(name: &OsStr) -> Result<CString, SecretStoreError> {
    CString::new(name.as_bytes()).map_err(|_| SecretStoreError::Io {
        reason: "secret store filename contains interior NUL byte".to_owned(),
    })
}

fn classify_open_error(action: &'static str, error: io::Error) -> SecretStoreError {
    if let Some(code) = error.raw_os_error()
        && code == libc::ELOOP
    {
        return SecretStoreError::Io {
            reason: format!("{action}: refusing symlink"),
        };
    }
    map_io_err(action)(error)
}

fn map_io_err(action: &'static str) -> impl FnOnce(io::Error) -> SecretStoreError {
    move |err| SecretStoreError::Io {
        reason: format!("{action} ({})", err.kind()),
    }
}
