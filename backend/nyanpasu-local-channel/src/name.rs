//! Where a server listens, and who may connect to it.
//!
//! Windows: a named pipe `\\.\pipe\<leaf>` whose DACL grants access to the
//! current user alone. Unix: a socket file `<leaf>.sock` in a directory only the
//! current user can enter. The Linux abstract namespace is avoided on purpose:
//! it has no permissions, so any local user could connect.
//!
//! Only the server, behind the `tokio` feature, makes names and listeners; a
//! blocking client only turns a name into a socket address.

#[cfg(feature = "tokio")]
use interprocess::local_socket::ListenerOptions;
use interprocess::local_socket::Name;
#[cfg(any(feature = "tokio", test))]
use snafu::{ResultExt as _, ensure};

#[cfg(any(feature = "tokio", test))]
use crate::error::{ServerError, server_error};

/// Short enough that the longest leaf fits `sockaddr_un` under macOS's
/// 49-byte `$TMPDIR`, the longest directory a socket lands in.
#[cfg(any(feature = "tokio", test))]
const MAX_PREFIX_LEN: usize = 16;

/// A fresh name for a server, in the form the bootstrap carries.
#[cfg(any(feature = "tokio", test))]
pub(crate) fn new_name(prefix: &str) -> Result<String, ServerError> {
    ensure!(
        !prefix.is_empty()
            && prefix.len() <= MAX_PREFIX_LEN
            && prefix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
        server_error::InvalidPrefixSnafu { prefix }
    );
    let mut random = [0_u8; 8];
    getrandom::fill(&mut random).context(server_error::RandomSnafu)?;
    platform::name_for(leaf(prefix, std::process::id(), &random))
}

/// `<prefix>-<pid>-<16 hex digits>`: the pid tells whose socket it is, the
/// random part keeps every name new.
#[cfg(any(feature = "tokio", test))]
fn leaf(prefix: &str, pid: u32, random: &[u8; 8]) -> String {
    let random: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{prefix}-{pid}-{random}")
}

#[cfg(feature = "tokio")]
pub(crate) use platform::listener_options;
pub(crate) use platform::socket_name;

#[cfg(windows)]
mod platform {
    use std::{ffi::OsString, io};

    use interprocess::local_socket::{GenericNamespaced, ToNsName as _};
    #[cfg(feature = "tokio")]
    use interprocess::os::windows::{
        local_socket::ListenerOptionsExt as _, security_descriptor::SecurityDescriptor,
    };
    #[cfg(feature = "tokio")]
    use widestring::U16CString;

    use super::*;

    #[cfg(any(feature = "tokio", test))]
    pub(crate) fn name_for(leaf: String) -> Result<String, ServerError> {
        Ok(leaf)
    }

    pub(crate) fn socket_name(name: &str) -> io::Result<Name<'static>> {
        OsString::from(name).to_ns_name::<GenericNamespaced>()
    }

    #[cfg(feature = "tokio")]
    pub(crate) fn listener_options(name: &str) -> Result<ListenerOptions<'static>, ServerError> {
        let sddl = current_user_sddl().context(server_error::AccessControlSnafu)?;
        let sddl = U16CString::from_str(&sddl)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
            .context(server_error::AccessControlSnafu)?;
        let descriptor =
            SecurityDescriptor::deserialize(&sddl).context(server_error::AccessControlSnafu)?;
        let name = socket_name(name).context(server_error::ListenSnafu { name })?;
        Ok(ListenerOptions::new()
            .name(name)
            .security_descriptor(descriptor))
    }

    /// A protected DACL with one entry: full access for the user this process runs as.
    #[cfg(any(feature = "tokio", test))]
    pub(crate) fn current_user_sddl() -> io::Result<String> {
        Ok(format!("D:P(A;;GA;;;{})", current_user_sid()?))
    }

    #[cfg(any(feature = "tokio", test))]
    fn current_user_sid() -> io::Result<String> {
        use std::{
            os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
            ptr,
        };

        use widestring::U16CStr;
        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY,
                TOKEN_USER, TokenUser,
            },
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };

        // SAFETY: every pointer passed below is either valid for the stated
        // length or an out-parameter the call fills; each handle and buffer the
        // system allocates is released before returning.
        unsafe {
            let mut raw_token = ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) == 0 {
                return Err(io::Error::last_os_error());
            }
            let token = OwnedHandle::from_raw_handle(raw_token);

            // The first call only reports the size; it fails by design.
            let mut len = 0_u32;
            GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                ptr::null_mut(),
                0,
                &mut len,
            );
            if len == 0 {
                return Err(io::Error::last_os_error());
            }
            // u64 elements keep the buffer aligned for TOKEN_USER.
            let mut buffer = vec![0_u64; (len as usize).div_ceil(size_of::<u64>())];
            if GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                buffer.as_mut_ptr().cast(),
                len,
                &mut len,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();

            let mut text = ptr::null_mut();
            if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
                return Err(io::Error::last_os_error());
            }
            let sid = U16CStr::from_ptr_str(text).to_string_lossy();
            LocalFree(text.cast());
            Ok(sid)
        }
    }
}

#[cfg(unix)]
mod platform {
    #[cfg(any(feature = "tokio", test))]
    use std::{
        ffi::OsString,
        fs::DirBuilder,
        os::unix::fs::{DirBuilderExt as _, MetadataExt as _},
        path::Path,
    };
    use std::{io, path::PathBuf};

    use interprocess::local_socket::{GenericFilePath, ToFsName as _};

    use super::*;

    /// The longest path `sockaddr_un` holds, without its terminating NUL.
    #[cfg(any(feature = "tokio", test))]
    const MAX_SOCKET_PATH_LEN: usize = if cfg!(target_os = "linux") { 107 } else { 103 };

    #[cfg(any(feature = "tokio", test))]
    pub(crate) fn name_for(leaf: String) -> Result<String, ServerError> {
        // SAFETY: geteuid has no preconditions and cannot fail.
        let uid = unsafe { libc::geteuid() };
        let dir = choose_socket_dir(
            std::env::var_os("XDG_RUNTIME_DIR"),
            &std::env::temp_dir(),
            uid,
        )?;
        socket_path(&dir, &leaf)
    }

    pub(crate) fn socket_name(name: &str) -> io::Result<Name<'static>> {
        PathBuf::from(name).to_fs_name::<GenericFilePath>()
    }

    #[cfg(feature = "tokio")]
    pub(crate) fn listener_options(name: &str) -> Result<ListenerOptions<'static>, ServerError> {
        let name = socket_name(name).context(server_error::ListenSnafu { name })?;
        // Name reclamation, on by default, removes the socket file when the
        // listener is dropped.
        Ok(ListenerOptions::new().name(name))
    }

    #[cfg(any(feature = "tokio", test))]
    pub(crate) fn socket_path(dir: &Path, leaf: &str) -> Result<String, ServerError> {
        let path = dir.join(format!("{leaf}.sock"));
        ensure!(
            path.as_os_str().len() <= MAX_SOCKET_PATH_LEN,
            server_error::NameTooLongSnafu { path }
        );
        path.into_os_string()
            .into_string()
            .map_err(|path| ServerError::NonUtf8Path { path: path.into() })
    }

    /// The first private directory of: `$XDG_RUNTIME_DIR`, which the XDG spec
    /// makes private; the temporary directory, which macOS makes private per
    /// user; a directory of our own under the shared temporary directory.
    ///
    /// Using a private directory as it is keeps the path short: under macOS's
    /// `$TMPDIR` an extra directory level would overflow `sockaddr_un`.
    #[cfg(any(feature = "tokio", test))]
    pub(crate) fn choose_socket_dir(
        runtime_dir: Option<OsString>,
        temp_dir: &Path,
        uid: libc::uid_t,
    ) -> Result<PathBuf, ServerError> {
        if let Some(dir) = runtime_dir.map(PathBuf::from)
            && dir.is_absolute()
            && is_private(&dir, uid)
        {
            return Ok(dir);
        }
        if is_private(temp_dir, uid) {
            return Ok(temp_dir.to_owned());
        }
        let dir = temp_dir.join(format!("nyanpasu-local-channel-{uid}"));
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).context(server_error::SocketDirSnafu { path: dir }),
        }
        // An existing directory may have been planted by someone else, or
        // loosened later, so it is checked whether or not we just made it.
        ensure!(
            is_private(&dir, uid),
            server_error::InsecureSocketDirSnafu { path: dir }
        );
        Ok(dir)
    }

    /// A real directory (not a link to one), owned by `uid`, that no one else can access.
    #[cfg(any(feature = "tokio", test))]
    fn is_private(dir: &Path, uid: libc::uid_t) -> bool {
        dir.symlink_metadata().is_ok_and(|metadata| {
            metadata.file_type().is_dir() && metadata.uid() == uid && metadata.mode() & 0o077 == 0
        })
    }

    #[cfg(test)]
    mod tests {
        use std::os::unix::fs::PermissionsExt as _;

        use super::*;

        fn uid() -> libc::uid_t {
            // SAFETY: geteuid has no preconditions and cannot fail.
            unsafe { libc::geteuid() }
        }

        fn set_mode(path: &Path, mode: u32) {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
        }

        /// A temporary directory shared like `/tmp`.
        fn shared_temp() -> tempfile::TempDir {
            let temp = tempfile::tempdir().unwrap();
            set_mode(temp.path(), 0o1777);
            temp
        }

        #[test]
        fn a_private_runtime_dir_is_used_as_is() {
            let runtime = tempfile::tempdir().unwrap();
            set_mode(runtime.path(), 0o700);
            let temp = tempfile::tempdir().unwrap();
            let dir = choose_socket_dir(Some(runtime.path().into()), temp.path(), uid()).unwrap();
            assert_eq!(dir, runtime.path());
        }

        #[test]
        fn a_shared_or_relative_runtime_dir_falls_back_to_a_private_one() {
            let runtime = tempfile::tempdir().unwrap();
            set_mode(runtime.path(), 0o755);
            let temp = shared_temp();
            for runtime_dir in [Some(runtime.path().into()), Some("relative".into()), None] {
                let dir = choose_socket_dir(runtime_dir, temp.path(), uid()).unwrap();
                assert_eq!(
                    dir,
                    temp.path()
                        .join(format!("nyanpasu-local-channel-{}", uid()))
                );
                let mode = dir.metadata().unwrap().mode();
                assert_eq!(mode & 0o777, 0o700, "{mode:o}");
            }
        }

        #[test]
        fn a_private_temp_dir_is_used_as_is() {
            let temp = tempfile::tempdir().unwrap();
            set_mode(temp.path(), 0o700);
            let dir = choose_socket_dir(None, temp.path(), uid()).unwrap();
            assert_eq!(dir, temp.path());
        }

        #[test]
        fn a_loosened_fallback_dir_is_refused() {
            let temp = shared_temp();
            let dir = temp
                .path()
                .join(format!("nyanpasu-local-channel-{}", uid()));
            std::fs::create_dir(&dir).unwrap();
            set_mode(&dir, 0o777);
            let error = choose_socket_dir(None, temp.path(), uid()).unwrap_err();
            assert!(
                matches!(error, ServerError::InsecureSocketDir { .. }),
                "{error:?}"
            );
        }

        #[test]
        fn a_symlinked_fallback_dir_is_refused() {
            let temp = shared_temp();
            let target = tempfile::tempdir().unwrap();
            set_mode(target.path(), 0o700);
            let dir = temp
                .path()
                .join(format!("nyanpasu-local-channel-{}", uid()));
            std::os::unix::fs::symlink(target.path(), &dir).unwrap();
            let error = choose_socket_dir(None, temp.path(), uid()).unwrap_err();
            assert!(
                matches!(error, ServerError::InsecureSocketDir { .. }),
                "{error:?}"
            );
        }

        #[test]
        fn a_fallback_dir_of_another_user_is_refused() {
            let temp = shared_temp();
            let other = uid().wrapping_add(1);
            let dir = temp.path().join(format!("nyanpasu-local-channel-{other}"));
            std::fs::create_dir(&dir).unwrap();
            set_mode(&dir, 0o700);
            let error = choose_socket_dir(None, temp.path(), other).unwrap_err();
            assert!(
                matches!(error, ServerError::InsecureSocketDir { .. }),
                "{error:?}"
            );
        }

        #[test]
        fn a_path_longer_than_sockaddr_un_is_refused() {
            let long = PathBuf::from(format!("/{}", "d".repeat(MAX_SOCKET_PATH_LEN)));
            let error = socket_path(&long, "leaf").unwrap_err();
            assert!(
                matches!(error, ServerError::NameTooLong { .. }),
                "{error:?}"
            );
            assert!(socket_path(Path::new("/tmp"), "leaf").is_ok());
        }

        #[test]
        fn the_longest_leaf_fits_under_the_longest_temp_dir() {
            // macOS: /var/folders/<2>/<28>/T, and its largest pid.
            let tmpdir = Path::new("/var/folders/36/tjdph2t965j8snz9_vkdnw0r0000gn/T");
            let longest = leaf(&"x".repeat(MAX_PREFIX_LEN), 99_999, &[0xff; 8]);
            assert!(socket_path(tmpdir, &longest).is_ok());
            // Linux: the shared fallback with a 7-digit pid and a 10-digit uid.
            let fallback = Path::new("/tmp/nyanpasu-local-channel-4294967294");
            let longest = leaf(&"x".repeat(MAX_PREFIX_LEN), 4_194_304, &[0xff; 8]);
            assert!(socket_path(fallback, &longest).is_ok());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_are_validated() {
        for prefix in [
            "",
            "has space",
            "slash/",
            "dot.",
            &"x".repeat(MAX_PREFIX_LEN + 1),
        ] {
            let error = new_name(prefix).unwrap_err();
            assert!(
                matches!(error, ServerError::InvalidPrefix { .. }),
                "{prefix:?}: {error:?}"
            );
        }
        assert!(new_name(&"x".repeat(MAX_PREFIX_LEN)).is_ok());
        assert!(new_name("Tray_menu-2").is_ok());
    }

    #[test]
    fn every_name_is_new() {
        let first = new_name("unique").unwrap();
        let second = new_name("unique").unwrap();
        assert_ne!(first, second);
        assert!(
            first.contains(&format!("unique-{}-", std::process::id())),
            "{first}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_pipe_dacl_names_only_the_current_user() {
        let sddl = platform::current_user_sddl().unwrap();
        assert!(sddl.starts_with("D:P(A;;GA;;;S-1-5-"), "{sddl}");
        assert_eq!(sddl.matches('(').count(), 1, "{sddl}");
    }
}
