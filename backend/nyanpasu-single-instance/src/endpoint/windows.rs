//! Current-user ACLs and first-instance named-pipe ownership.

use std::{
    io,
    path::{Path, PathBuf},
};

use interprocess::{
    local_socket::{
        GenericFilePath, ListenerOptions, Name, ToFsName as _, tokio::Stream,
        traits::StreamCommon as _,
    },
    os::windows::{local_socket::ListenerOptionsExt as _, security_descriptor::SecurityDescriptor},
};
use snafu::{ResultExt as _, ensure};
use widestring::U16CString;

use super::{
    AccessControlSnafu, BindSnafu, CurrentUserSnafu, EndpointError, EndpointListener,
    InvalidLocationSnafu, Namespace, PeerMismatch, PeerRejection, verify_sid,
};

pub(super) fn current_user() -> Result<String, EndpointError> {
    current_user_sid().context(CurrentUserSnafu)
}

pub(super) fn verify_peer(stream: &Stream) -> Result<(), PeerRejection> {
    let creds = stream.peer_creds().map_err(PeerRejection::Credentials)?;
    let Some(pid) = creds.pid() else {
        return Err(PeerRejection::Mismatch(PeerMismatch::MissingIdentity));
    };
    let peer_sid = process_user_sid(pid).map_err(PeerRejection::Process)?;
    let expected_sid = current_user_sid().map_err(PeerRejection::CurrentUser)?;
    verify_sid(Some(&peer_sid), &expected_sid).map_err(PeerRejection::Mismatch)
}

pub(super) fn default_path(
    namespace: Namespace,
    user: &str,
    identity: &str,
) -> Result<PathBuf, EndpointError> {
    Ok(PathBuf::from(format!(
        r"\\.\pipe\{}-{user}-{identity}",
        namespace.as_str()
    )))
}

pub(super) fn validate(namespace: Namespace, path: &Path) -> Result<(), EndpointError> {
    let user = current_user()?;
    let prefix = format!(r"\\.\pipe\{}-{user}-", namespace.as_str());
    let leaf = path.to_str().and_then(|path| path.strip_prefix(&prefix));
    ensure!(
        leaf.is_some_and(|leaf| !leaf.is_empty()
            && leaf.len() <= 64
            && leaf
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')),
        InvalidLocationSnafu { path }
    );
    Ok(())
}

pub(super) fn validate_connect(namespace: Namespace, path: &Path) -> Result<(), EndpointError> {
    validate(namespace, path)
}

pub(super) fn socket_name(path: &Path) -> io::Result<Name<'static>> {
    path.to_owned().to_fs_name::<GenericFilePath>()
}

pub(super) fn bind(namespace: Namespace, path: &Path) -> Result<EndpointListener, EndpointError> {
    validate(namespace, path)?;
    let user = current_user()?;
    let sddl = U16CString::from_str(format!("D:P(A;;GA;;;{user})"))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
        .context(AccessControlSnafu)?;
    let security_descriptor = SecurityDescriptor::deserialize(&sddl).context(AccessControlSnafu)?;
    // Interprocess creates the first instance with FILE_FLAG_FIRST_PIPE_INSTANCE.
    // accept creates the next instance before handing out the connected one,
    // retaining both exclusivity and this protected DACL across reconnects.
    let listener = ListenerOptions::new()
        .name(socket_name(path).context(BindSnafu { path })?)
        .security_descriptor(security_descriptor)
        .create_tokio()
        .map_err(|source| match source.kind() {
            io::ErrorKind::PermissionDenied | io::ErrorKind::AddrInUse => {
                EndpointError::AlreadyServing {
                    path: path.to_owned(),
                }
            }
            _ => EndpointError::Bind {
                path: path.to_owned(),
                source,
            },
        })?;
    Ok(EndpointListener { listener, user })
}

fn current_user_sid() -> io::Result<String> {
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // SAFETY: GetCurrentProcess returns a pseudo-handle valid for the current process.
    let process = unsafe { GetCurrentProcess() };
    process_handle_user_sid(process)
}

fn process_user_sid(pid: u32) -> io::Result<String> {
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};

    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    // SAFETY: OpenProcess takes a numeric PID and returns an owned process handle on
    // success or null on failure; OwnedHandle closes the handle on drop.
    let raw_process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if raw_process.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: OpenProcess returned a non-null owned process handle.
    let process = unsafe { OwnedHandle::from_raw_handle(raw_process) };
    process_handle_user_sid(process.as_raw_handle())
}

fn process_handle_user_sid(process: windows_sys::Win32::Foundation::HANDLE) -> io::Result<String> {
    use std::{
        os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
        ptr,
    };

    use widestring::U16CStr;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::OpenProcessToken,
    };

    // SAFETY: `process` is a valid process handle (or GetCurrentProcess pseudo-handle);
    // every pointer passed below is either valid for the stated length or an
    // out-parameter the call fills; each handle and buffer the system allocates
    // is released before returning.
    unsafe {
        let mut raw_token = ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut raw_token) == 0 {
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
