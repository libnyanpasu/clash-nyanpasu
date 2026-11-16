//! CLI token storage: the OS keychain when available, otherwise an owner-only file
//! (`0600` on Unix and a current-user-only, protected DACL on Windows).
//! On Windows only the token directory is owner-only; existing parent permissions are
//! left unchanged, and an existing reparse point anywhere above it is refused.
//!
//! A token is never accepted as a command-line argument, so it cannot reach shell
//! history or the process list. It comes from standard input, from `NYANPASU_TOKEN`
//! when a context is added, or from `NYANPASU_TOKEN` at call time.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{config::TokenBackend, error::CliError};

/// The keychain service name of all CLI tokens.
const KEYRING_SERVICE: &str = "nyanpasu-cli";

/// One place a named secret can live.
pub trait SecretStore: Send + Sync {
    /// Reads a secret, or `None` when it was never stored.
    fn get(&self, name: &str) -> Result<Option<String>, CliError>;
    /// Stores or replaces a secret.
    fn set(&self, name: &str, secret: &str) -> Result<(), CliError>;
    /// Removes a secret; removing an absent one succeeds.
    fn delete(&self, name: &str) -> Result<(), CliError>;
}

/// The OS keychain through the `keyring` crate.
#[derive(Debug, Default)]
pub struct KeyringStore;

impl KeyringStore {
    fn entry(name: &str) -> Result<keyring::Entry, CliError> {
        keyring::Entry::new(KEYRING_SERVICE, name)
            .map_err(|error| CliError::failed(format!("keychain unavailable: {error}")))
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, name: &str) -> Result<Option<String>, CliError> {
        match Self::entry(name)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(CliError::failed(format!("keychain read failed: {error}"))),
        }
    }

    fn set(&self, name: &str, secret: &str) -> Result<(), CliError> {
        Self::entry(name)?
            .set_password(secret)
            .map_err(|error| CliError::failed(format!("keychain write failed: {error}")))
    }

    fn delete(&self, name: &str) -> Result<(), CliError> {
        match Self::entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(CliError::failed(format!("keychain delete failed: {error}"))),
        }
    }
}

/// A store that refuses every write, for `NYANPASU_NO_KEYRING`: tokens then go to files.
#[derive(Debug, Default)]
pub struct DisabledStore;

impl SecretStore for DisabledStore {
    fn get(&self, _name: &str) -> Result<Option<String>, CliError> {
        Ok(None)
    }

    fn set(&self, _name: &str, _secret: &str) -> Result<(), CliError> {
        Err(CliError::failed("the keychain is disabled"))
    }

    fn delete(&self, _name: &str) -> Result<(), CliError> {
        Ok(())
    }
}

/// Owner-only files in an owner-only directory, named after the validated context name.
#[derive(Debug)]
pub struct FileStore {
    directory: PathBuf,
}

impl FileStore {
    /// A store rooted at `directory`, which is created on first write.
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.join(format!("{name}.token"))
    }
}

#[cfg(unix)]
fn private_directory(directory: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;

    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
}

/// Creates the owner-only token directory, creating missing parents with inherited security.
/// Existing ancestors are validated but never re-secured.
#[cfg(windows)]
fn private_directory(directory: &Path) -> io::Result<()> {
    validate_existing_directory_chain(directory)?;
    let security = PrivateSecurity::new()?;
    let mut missing = Vec::new();
    let mut current = directory;
    loop {
        match fs::symlink_metadata(current) {
            Ok(metadata) => {
                validate_directory_metadata(current, &metadata)?;
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(current.to_owned());
                current = match current.parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => parent,
                    _ => break,
                };
            }
            Err(error) => return Err(error),
        }
    }
    if let Some((leaf, ancestors)) = missing.split_first() {
        // Only the token directory itself is owner-only. Its missing parents use the
        // inherited security, and existing parents are never re-secured here.
        for path in ancestors.iter().rev() {
            create_inherited_directory(path)?;
        }
        create_private_directory(leaf, &security)?;
    }
    validate_existing_directory_chain(directory)?;
    secure_existing_directory(directory, &security)
}

#[cfg(windows)]
fn validate_existing_directory_chain(directory: &Path) -> io::Result<()> {
    let absolute = std::path::absolute(directory)?;
    let ancestors: Vec<_> = absolute.ancestors().collect();
    for path in ancestors.into_iter().rev() {
        if path.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(path) {
            Ok(metadata) => validate_directory_metadata(path, &metadata)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(windows)]
fn validate_directory_metadata(path: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    if is_reparse_point(metadata) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "the token storage path {} is a reparse point",
                path.display()
            ),
        ));
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "the token storage path {} is not a directory",
                path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn validate_existing_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    validate_directory_metadata(path, &metadata)
}

#[cfg(not(any(unix, windows)))]
fn private_directory(directory: &std::path::Path) -> io::Result<()> {
    fs::create_dir_all(directory)
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, secret: &str) -> io::Result<()> {
    use std::{
        io::Write as _,
        os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _},
    };

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // `mode` applies only to a new file; tighten one that already existed.
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(secret.as_bytes())
}

#[cfg(windows)]
fn write_private(path: &Path, secret: &str) -> io::Result<()> {
    use std::io::Write as _;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "token path has no parent"))?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "token path has no file name")
    })?;
    let security = PrivateSecurity::new()?;
    let temporary = parent.join(temporary_token_name(file_name));
    let mut file = create_private_file(&temporary, &security)?;
    let written = file
        .write_all(secret.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = written {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    if let Err(error) = prepare_token_replacement(path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    let source = wide(temporary.as_os_str());
    let target = wide(path.as_os_str());
    // The replacement's protected DACL was applied at creation, and moving within the volume
    // keeps that descriptor. Reassert it by handle after publication so an old, permissive
    // target descriptor cannot survive a platform merge.
    // SAFETY: both paths are valid, null-terminated UTF-16 strings for the duration of the call.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        let error = io::Error::last_os_error();
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    secure_existing_file(path, &security)?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn write_private(path: &std::path::Path, secret: &str) -> io::Result<()> {
    fs::write(path, secret)
}

#[cfg(windows)]
fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt as _;

    value.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
struct PrivateSecurity {
    descriptor: *mut std::ffi::c_void,
}

#[cfg(windows)]
impl PrivateSecurity {
    fn new() -> io::Result<Self> {
        use std::ptr;

        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        };

        let sid = current_user_sid()?;
        let sddl = wide(std::ffi::OsStr::new(&format!("D:P(A;;FA;;;{sid})")));
        let mut descriptor = ptr::null_mut();
        // SAFETY: the SDDL is a valid null-terminated string and a successful conversion
        // allocates `descriptor`, which Drop releases with LocalFree.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { descriptor })
    }

    fn attributes(&self) -> windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
        windows_sys::Win32::Security::SECURITY_ATTRIBUTES {
            nLength: size_of::<windows_sys::Win32::Security::SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.descriptor,
            bInheritHandle: 0,
        }
    }

    fn dacl(&self) -> io::Result<*mut windows_sys::Win32::Security::ACL> {
        use std::ptr;

        use windows_sys::Win32::Security::GetSecurityDescriptorDacl;

        let mut present = 0;
        let mut dacl = ptr::null_mut();
        let mut defaulted = 0;
        // SAFETY: `descriptor` is a valid security descriptor owned by this value.
        if unsafe {
            GetSecurityDescriptorDacl(self.descriptor, &mut present, &mut dacl, &mut defaulted)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if present == 0 || dacl.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the private token ACL is absent",
            ));
        }
        Ok(dacl)
    }
}

#[cfg(windows)]
impl Drop for PrivateSecurity {
    fn drop(&mut self) {
        // SAFETY: `descriptor` was allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
        let _ = unsafe { windows_sys::Win32::Foundation::LocalFree(self.descriptor) };
    }
}

#[cfg(windows)]
fn current_user_sid() -> io::Result<String> {
    use std::{
        os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle},
        ptr, slice,
    };

    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    // SAFETY: GetCurrentProcess returns a pseudo-handle valid for this process. Every
    // out-parameter is initialized before use, and the token and SID string are closed/freed.
    unsafe {
        let mut raw_token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = OwnedHandle::from_raw_handle(raw_token);

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

        let mut raw_sid = ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut raw_sid) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut length = 0;
        while *raw_sid.add(length) != 0 {
            length += 1;
        }
        let sid = String::from_utf16_lossy(slice::from_raw_parts(raw_sid, length));
        LocalFree(raw_sid.cast());
        Ok(sid)
    }
}

#[cfg(windows)]
fn create_private_directory(path: &Path, security: &PrivateSecurity) -> io::Result<()> {
    use windows_sys::Win32::{
        Foundation::ERROR_ALREADY_EXISTS, Storage::FileSystem::CreateDirectoryW,
    };

    let path_wide = wide(path.as_os_str());
    let attributes = security.attributes();
    // SAFETY: the path is a valid null-terminated UTF-16 string, and the security attributes
    // point to a descriptor that outlives the call.
    if unsafe { CreateDirectoryW(path_wide.as_ptr(), &attributes) } != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32) {
        return validate_existing_directory(path);
    }
    Err(error)
}

#[cfg(windows)]
fn create_inherited_directory(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            validate_existing_directory(path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn secure_existing_directory(path: &Path, security: &PrivateSecurity) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, READ_CONTROL, WRITE_DAC,
    };

    let handle = open_existing_handle(
        path,
        READ_CONTROL | WRITE_DAC,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
    )?
    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "token directory disappeared"))?;
    let attributes = file_attributes(&handle)?;
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the token directory is a reparse point",
        ));
    }
    if attributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the token directory path is not a directory",
        ));
    }
    set_private_dacl(handle.as_raw_handle(), security)
}

#[cfg(windows)]
fn secure_existing_file(path: &Path, security: &PrivateSecurity) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
        READ_CONTROL, WRITE_DAC,
    };

    let handle =
        open_existing_handle(path, READ_CONTROL | WRITE_DAC, FILE_FLAG_OPEN_REPARSE_POINT)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "token file disappeared"))?;
    let attributes = file_attributes(&handle)?;
    if attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the token target is not a regular file",
        ));
    }
    set_private_dacl(handle.as_raw_handle(), security)
}

#[cfg(windows)]
fn set_private_dacl(
    handle: std::os::windows::io::RawHandle,
    security: &PrivateSecurity,
) -> io::Result<()> {
    use std::ptr;

    use windows_sys::Win32::Security::{
        Authorization::{SE_FILE_OBJECT, SetSecurityInfo},
        DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };

    let dacl = security.dacl()?;
    let result = unsafe {
        SetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}

#[cfg(windows)]
fn create_private_file(path: &Path, security: &PrivateSecurity) -> io::Result<fs::File> {
    use std::{
        os::windows::io::{FromRawHandle as _, OwnedHandle},
        ptr,
    };

    use windows_sys::Win32::{
        Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE},
    };

    let path = wide(path.as_os_str());
    let attributes = security.attributes();
    // SAFETY: the path and security descriptor are valid for the call. The returned handle is
    // converted to an owner that closes it.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_NONE,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a valid owned handle.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    Ok(fs::File::from(handle))
}

#[cfg(windows)]
fn open_existing_handle(
    path: &Path,
    access: u32,
    flags: u32,
) -> io::Result<Option<std::os::windows::io::OwnedHandle>> {
    use std::{
        os::windows::io::{FromRawHandle as _, OwnedHandle},
        ptr,
    };

    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        Storage::FileSystem::{
            CreateFileW, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };

    let path = wide(path.as_os_str());
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            flags,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        };
    }
    // SAFETY: CreateFileW returned a valid owned handle.
    Ok(Some(unsafe { OwnedHandle::from_raw_handle(handle) }))
}

#[cfg(windows)]
fn file_attributes(handle: &std::os::windows::io::OwnedHandle) -> io::Result<u32> {
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is valid and `information` is a correctly sized out-parameter.
    if unsafe { GetFileInformationByHandle(handle.as_raw_handle(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(information.dwFileAttributes)
}

#[cfg(windows)]
fn open_regular_token_file(
    path: &Path,
    access: u32,
) -> io::Result<Option<std::os::windows::io::OwnedHandle>> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
    };

    let Some(handle) = open_existing_handle(
        path,
        access | FILE_READ_ATTRIBUTES,
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
    )?
    else {
        return Ok(None);
    };
    let attributes = file_attributes(&handle)?;
    if attributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the token target is not a regular file",
        ));
    }
    Ok(Some(handle))
}

/// Clears the read-only attribute on an already validated token file.
///
/// The attribute is not an access-control boundary, and a token that another tool marked
/// read-only must still be replaceable or removable by the CLI that owns it.
#[cfg(windows)]
fn clear_read_only(handle: &std::os::windows::io::OwnedHandle) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, FILE_BASIC_INFO, FileBasicInfo,
        GetFileInformationByHandleEx, SetFileInformationByHandle,
    };

    let mut information = FILE_BASIC_INFO::default();
    let size = size_of::<FILE_BASIC_INFO>() as u32;
    // SAFETY: the handle is valid and `information` is a correctly sized out-parameter.
    if unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw_handle(),
            FileBasicInfo,
            (&mut information as *mut FILE_BASIC_INFO).cast(),
            size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if information.FileAttributes & FILE_ATTRIBUTE_READONLY == 0 {
        return Ok(());
    }
    information.FileAttributes &= !FILE_ATTRIBUTE_READONLY;
    if information.FileAttributes == 0 {
        information.FileAttributes = FILE_ATTRIBUTE_NORMAL;
    }
    // SAFETY: the handle is valid and `information` remains alive for the call.
    if unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle(),
            FileBasicInfo,
            (&information as *const FILE_BASIC_INFO).cast(),
            size,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn prepare_token_replacement(path: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;

    validate_token_parent(path)?;
    if let Some(handle) = open_regular_token_file(path, FILE_WRITE_ATTRIBUTES)? {
        clear_read_only(&handle)?;
    }
    Ok(())
}

#[cfg(windows)]
fn validate_token_parent(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "token path has no parent"))?;
    validate_existing_directory_chain(parent)
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;

    metadata.file_attributes()
        & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
        != 0
}

#[cfg(windows)]
fn temporary_token_name(file_name: &std::ffi::OsStr) -> std::ffi::OsString {
    use std::ffi::OsString;

    let mut name = OsString::from(".");
    name.push(file_name);
    name.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    name
}

#[cfg(windows)]
fn read_private(path: &Path) -> io::Result<Option<String>> {
    use std::io::Read as _;

    use windows_sys::Win32::Foundation::GENERIC_READ;

    validate_token_parent(path)?;
    let Some(handle) = open_regular_token_file(path, GENERIC_READ)? else {
        return Ok(None);
    };
    let mut file = fs::File::from(handle);
    let mut secret = String::new();
    file.read_to_string(&mut secret)?;
    Ok(Some(secret.trim().to_owned()))
}

#[cfg(windows)]
fn delete_private(path: &Path) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle as _;

    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_DISPOSITION_INFO, FILE_WRITE_ATTRIBUTES, FileDispositionInfo,
        SetFileInformationByHandle,
    };

    match fs::symlink_metadata(path) {
        Ok(_) => {
            validate_token_parent(path)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    let Some(handle) = open_regular_token_file(path, DELETE | FILE_WRITE_ATTRIBUTES)? else {
        return Ok(());
    };
    clear_read_only(&handle)?;
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the handle is valid and `disposition` remains alive for the call.
    if unsafe {
        SetFileInformationByHandle(
            handle.as_raw_handle(),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(windows))]
fn read_private(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(secret) => Ok(Some(secret.trim().to_owned())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(not(windows))]
fn delete_private(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

impl SecretStore for FileStore {
    fn get(&self, name: &str) -> Result<Option<String>, CliError> {
        read_private(&self.path(name))
            .map_err(|error| CliError::failed(format!("cannot read token file: {error}")))
    }

    fn set(&self, name: &str, secret: &str) -> Result<(), CliError> {
        private_directory(&self.directory)
            .and_then(|()| write_private(&self.path(name), secret))
            .map_err(|error| CliError::failed(format!("cannot write token file: {error}")))
    }

    fn delete(&self, name: &str) -> Result<(), CliError> {
        match delete_private(&self.path(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(CliError::failed(format!(
                "cannot delete token file: {error}"
            ))),
        }
    }
}

/// Both stores, so a token can prefer the keychain and fall back to a file.
pub struct TokenVault {
    keyring: Box<dyn SecretStore>,
    file: Box<dyn SecretStore>,
}

impl TokenVault {
    /// The production vault: the OS keychain unless `use_keyring` is false, then a file
    /// under `token_dir`.
    pub fn new(token_dir: PathBuf, use_keyring: bool) -> Self {
        let keyring: Box<dyn SecretStore> = if use_keyring {
            Box::new(KeyringStore)
        } else {
            Box::new(DisabledStore)
        };
        Self::with_stores(keyring, Box::new(FileStore::new(token_dir)))
    }

    /// A vault over explicit stores.
    pub fn with_stores(keyring: Box<dyn SecretStore>, file: Box<dyn SecretStore>) -> Self {
        Self { keyring, file }
    }

    /// Stores a token, preferring the keychain, and reports where it went.
    ///
    /// # Errors
    /// Fails when neither store accepts the secret.
    pub fn save(&self, name: &str, token: &str) -> Result<TokenBackend, CliError> {
        // A secret stored earlier elsewhere must not outlive its replacement.
        self.delete(name)?;
        if self.keyring.set(name, token).is_ok() {
            return Ok(TokenBackend::Keyring);
        }
        self.file.set(name, token)?;
        Ok(TokenBackend::File)
    }

    /// Reads the token from the store the context recorded. There is no fallback to
    /// the other store, so a context never resolves a credential it did not choose.
    ///
    /// # Errors
    /// Fails on a store read failure.
    pub fn load(&self, name: &str, backend: TokenBackend) -> Result<Option<String>, CliError> {
        match backend {
            TokenBackend::Keyring => self.keyring.get(name),
            TokenBackend::File => self.file.get(name),
        }
    }

    /// Removes the token from both stores; an unavailable keychain is not an error.
    ///
    /// # Errors
    /// Fails only when the file store cannot delete.
    pub fn delete(&self, name: &str) -> Result<(), CliError> {
        let _ = self.keyring.delete(name);
        self.file.delete(name)
    }
}

/// Normalizes a pasted token: surrounding whitespace is dropped, the rest must be
/// one non-empty word.
///
/// # Errors
/// An empty token or one containing whitespace is a usage error.
pub fn clean_token(raw: &str) -> Result<String, CliError> {
    let token = raw.trim();
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        return Err(CliError::usage("the token is empty or contains whitespace"));
    }
    Ok(token.to_owned())
}

#[cfg(test)]
pub(crate) mod fakes {
    use std::{collections::BTreeMap, sync::Mutex};

    use super::*;

    /// An in-memory store; `available: false` makes writes fail like a missing keychain.
    pub struct MemoryStore {
        pub available: bool,
        pub secrets: Mutex<BTreeMap<String, String>>,
    }

    impl MemoryStore {
        pub fn new(available: bool) -> Self {
            Self {
                available,
                secrets: Mutex::default(),
            }
        }
    }

    impl SecretStore for MemoryStore {
        fn get(&self, name: &str) -> Result<Option<String>, CliError> {
            Ok(self.secrets.lock().unwrap().get(name).cloned())
        }

        fn set(&self, name: &str, secret: &str) -> Result<(), CliError> {
            if !self.available {
                return Err(CliError::failed("unavailable"));
            }
            self.secrets
                .lock()
                .unwrap()
                .insert(name.to_owned(), secret.to_owned());
            Ok(())
        }

        fn delete(&self, name: &str) -> Result<(), CliError> {
            self.secrets.lock().unwrap().remove(name);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fakes::MemoryStore, *};

    #[test]
    fn prefers_the_keyring_and_falls_back_to_a_file() {
        let vault = TokenVault::with_stores(
            Box::new(MemoryStore::new(true)),
            Box::new(MemoryStore::new(true)),
        );
        assert_eq!(vault.save("a", "npk_1_x").unwrap(), TokenBackend::Keyring);
        assert_eq!(
            vault.load("a", TokenBackend::Keyring).unwrap().as_deref(),
            Some("npk_1_x")
        );

        let vault = TokenVault::with_stores(
            Box::new(MemoryStore::new(false)),
            Box::new(MemoryStore::new(true)),
        );
        assert_eq!(vault.save("a", "npk_1_x").unwrap(), TokenBackend::File);
        assert_eq!(
            vault.load("a", TokenBackend::File).unwrap().as_deref(),
            Some("npk_1_x")
        );
    }

    #[test]
    fn load_reads_only_the_recorded_backend() {
        let vault = TokenVault::with_stores(
            Box::new(MemoryStore::new(false)),
            Box::new(MemoryStore::new(true)),
        );
        vault.save("a", "secret").unwrap();
        assert_eq!(vault.load("a", TokenBackend::Keyring).unwrap(), None);
    }

    #[test]
    fn delete_clears_both_stores() {
        let vault = TokenVault::with_stores(
            Box::new(MemoryStore::new(true)),
            Box::new(MemoryStore::new(true)),
        );
        vault.save("a", "secret").unwrap();
        vault.delete("a").unwrap();
        assert_eq!(vault.load("a", TokenBackend::Keyring).unwrap(), None);
        assert_eq!(vault.load("a", TokenBackend::File).unwrap(), None);
    }

    #[test]
    fn the_file_store_round_trips_and_is_owner_only() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileStore::new(directory.path().join("tokens"));
        assert_eq!(store.get("a").unwrap(), None);
        store.set("a", "npk_1_x").unwrap();
        store.set("a", "npk_2_y").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("npk_2_y"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            let file_mode = fs::metadata(store.path("a")).unwrap().permissions().mode();
            assert_eq!(file_mode & 0o777, 0o600);
            let directory_mode = fs::metadata(directory.path().join("tokens"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(directory_mode & 0o777, 0o700);
        }

        #[cfg(windows)]
        {
            let expected = canonical_sddl(&format!("D:P(A;;FA;;;{})", current_user_sid().unwrap()));
            // Windows can report the auto-inherited flag even for a protected DACL.
            // It does not change the ACEs, which must still match exactly.
            let comparable = |sddl: String| sddl.replacen("D:PAI(", "D:P(", 1);
            assert_eq!(comparable(file_sddl(&store.path("a"))), expected);
            assert_eq!(
                comparable(file_sddl(&directory.path().join("tokens"))),
                expected,
                "the token directory must not inherit broader grants"
            );
        }

        store.delete("a").unwrap();
        store.delete("a").unwrap();
        assert_eq!(store.get("a").unwrap(), None);
    }

    #[cfg(windows)]
    #[test]
    fn an_existing_handle_keeps_the_old_secret_when_the_file_is_replaced() {
        use std::io::Read as _;

        use windows_sys::Win32::{
            Foundation::GENERIC_READ, Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
        };

        let directory = tempfile::tempdir().unwrap();
        let tokens = directory.path().join("tokens");
        fs::create_dir_all(&tokens).unwrap();
        let path = tokens.join("a.token");
        fs::write(&path, "old").unwrap();
        set_named_sddl(
            &path,
            &format!("D:P(A;;FA;;;{})(A;;FR;;;WD)", current_user_sid().unwrap()),
        );
        let mut old_handle = fs::File::from(
            open_existing_handle(&path, GENERIC_READ, FILE_FLAG_OPEN_REPARSE_POINT)
                .unwrap()
                .unwrap(),
        );

        let store = FileStore::new(tokens);
        store.set("a", "new").unwrap();

        let mut old = String::new();
        old_handle.read_to_string(&mut old).unwrap();
        assert_eq!(old, "old", "an open handle must not observe the new secret");
        assert_eq!(store.get("a").unwrap().as_deref(), Some("new"));
        let comparable = |sddl: String| sddl.replacen("D:PAI(", "D:P(", 1);
        assert_eq!(
            comparable(file_sddl(&path)),
            canonical_sddl(&format!("D:P(A;;FA;;;{})", current_user_sid().unwrap())),
            "the replacement must not inherit the old permissive DACL"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_created_token_directory_is_owner_only() {
        let directory = tempfile::tempdir().unwrap();
        let tokens = directory.path().join("nyanpasu-cli").join("tokens");
        let store = FileStore::new(tokens.clone());
        store.set("a", "secret").unwrap();

        let expected = canonical_sddl(&format!("D:P(A;;FA;;;{})", current_user_sid().unwrap()));
        let comparable = |sddl: String| sddl.replacen("D:PAI(", "D:P(", 1);
        assert_eq!(
            comparable(file_sddl(&tokens)),
            expected,
            "the token directory must be owner-only"
        );
        assert!(tokens.parent().unwrap().is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn an_existing_parent_directory_keeps_its_acl() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("nyanpasu-cli");
        fs::create_dir(&parent).unwrap();
        set_named_sddl(
            &parent,
            &format!("D:P(A;;FA;;;{})(A;;FR;;;WD)", current_user_sid().unwrap()),
        );
        let before = file_sddl(&parent);

        let tokens = parent.join("tokens");
        let store = FileStore::new(tokens.clone());
        store.set("a", "secret").unwrap();

        assert_eq!(
            file_sddl(&parent),
            before,
            "an existing parent ACL must not be rewritten"
        );
        let expected = canonical_sddl(&format!("D:P(A;;FA;;;{})", current_user_sid().unwrap()));
        let comparable = |sddl: String| sddl.replacen("D:PAI(", "D:P(", 1);
        assert_eq!(comparable(file_sddl(&tokens)), expected);
    }

    #[cfg(windows)]
    #[test]
    fn reading_a_token_does_not_rewrite_its_acl() {
        let directory = tempfile::tempdir().unwrap();
        let tokens = directory.path().join("tokens");
        fs::create_dir_all(&tokens).unwrap();
        let path = tokens.join("a.token");
        fs::write(&path, "secret").unwrap();
        set_named_sddl(
            &path,
            &format!("D:P(A;;FA;;;{})(A;;FR;;;WD)", current_user_sid().unwrap()),
        );
        let before = file_sddl(&path);

        let store = FileStore::new(tokens);
        assert_eq!(store.get("a").unwrap().as_deref(), Some("secret"));
        assert_eq!(file_sddl(&path), before, "reading must not change the ACL");
    }

    #[cfg(windows)]
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "this Windows-only test clears the attribute so the temporary directory can be removed"
    )]
    #[test]
    fn a_read_only_token_file_is_still_readable() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileStore::new(directory.path().join("tokens"));
        store.set("a", "secret").unwrap();
        let path = store.path("a");
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).unwrap();

        assert_eq!(store.get("a").unwrap().as_deref(), Some("secret"));

        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_read_only_token_file_can_be_replaced_and_deleted() {
        let directory = tempfile::tempdir().unwrap();
        let store = FileStore::new(directory.path().join("tokens"));
        store.set("a", "old").unwrap();
        mark_read_only(&store.path("a"));

        store.set("a", "new").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("new"));
        let comparable = |sddl: String| sddl.replacen("D:PAI(", "D:P(", 1);
        assert_eq!(
            comparable(file_sddl(&store.path("a"))),
            canonical_sddl(&format!("D:P(A;;FA;;;{})", current_user_sid().unwrap())),
            "replacing a read-only token must keep the owner-only DACL"
        );

        mark_read_only(&store.path("a"));
        store.delete("a").unwrap();
        assert_eq!(store.get("a").unwrap(), None);
    }

    #[cfg(windows)]
    #[test]
    fn a_read_only_file_does_not_block_a_vault_save() {
        let directory = tempfile::tempdir().unwrap();
        let tokens = directory.path().join("tokens");
        let vault = TokenVault::with_stores(
            Box::new(MemoryStore::new(false)),
            Box::new(FileStore::new(tokens.clone())),
        );
        assert_eq!(vault.save("a", "old").unwrap(), TokenBackend::File);
        let path = tokens.join("a.token");
        mark_read_only(&path);

        assert_eq!(vault.save("a", "new").unwrap(), TokenBackend::File);
        assert_eq!(
            vault.load("a", TokenBackend::File).unwrap().as_deref(),
            Some("new")
        );

        mark_read_only(&path);
        vault.delete("a").unwrap();
        assert_eq!(vault.load("a", TokenBackend::File).unwrap(), None);
    }

    #[cfg(windows)]
    #[test]
    fn a_file_above_the_token_directory_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().join("parent");
        fs::write(&parent, "not a directory").unwrap();
        let store = FileStore::new(parent.join("tokens"));

        let error = store.set("a", "secret").unwrap_err();
        assert!(error.to_string().contains("not a directory"), "{error}");
    }

    #[cfg(windows)]
    #[test]
    fn a_reparse_point_ancestor_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("target");
        let link = directory.path().join("link");
        fs::create_dir(&target).unwrap();
        if !create_directory_reparse(&link, &target) {
            eprintln!("SKIPPED reparse-point fixture: reparse creation unavailable");
            return;
        }

        let store = FileStore::new(link.join("nyanpasu-cli").join("tokens"));
        let error = store.set("a", "secret").unwrap_err();
        assert!(error.to_string().contains("reparse point"), "{error}");
        assert!(
            !target.join("nyanpasu-cli").exists(),
            "the token directory must not be created through the reparse point"
        );

        let outside = target.join("nyanpasu-cli").join("tokens");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("a.token"), "do not delete").unwrap();
        let error = store.get("a").unwrap_err();
        assert!(error.to_string().contains("reparse point"), "{error}");
        let error = store.delete("a").unwrap_err();
        assert!(error.to_string().contains("reparse point"), "{error}");
        assert!(outside.join("a.token").exists());
    }

    #[test]
    fn clean_token_rejects_whitespace_and_empty_input() {
        assert_eq!(clean_token(" npk_1_x\n").unwrap(), "npk_1_x");
        assert!(clean_token("").is_err());
        assert!(clean_token("two words").is_err());
    }

    #[cfg(windows)]
    fn mark_read_only(path: &Path) {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(windows)]
    fn create_directory_reparse(link: &Path, target: &Path) -> bool {
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return fs::symlink_metadata(link).is_ok_and(|metadata| is_reparse_point(&metadata));
        }
        let created = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .is_ok_and(|output| output.status.success());
        created && fs::symlink_metadata(link).is_ok_and(|metadata| is_reparse_point(&metadata))
    }

    #[cfg(windows)]
    fn set_named_sddl(path: &Path, sddl: &str) {
        use std::ptr;

        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::{
                    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
                    SE_FILE_OBJECT, SetNamedSecurityInfoW,
                },
                DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
                PROTECTED_DACL_SECURITY_INFORMATION,
            },
        };

        let sddl = wide(std::ffi::OsStr::new(sddl));
        let path = wide(path.as_os_str());
        let mut descriptor = ptr::null_mut();
        // SAFETY: the SDDL is null-terminated and the descriptor is released below.
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    ptr::null_mut(),
                )
            },
            0
        );
        let mut present = 0;
        let mut dacl = ptr::null_mut();
        let mut defaulted = 0;
        // SAFETY: descriptor is valid and every out-parameter is initialized by the API.
        assert_ne!(
            unsafe {
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted)
            },
            0
        );
        assert_ne!(present, 0);
        // SAFETY: the path and DACL are valid for the call; the descriptor remains alive.
        let result = unsafe {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                dacl,
                ptr::null_mut(),
            )
        };
        // SAFETY: the conversion allocated this descriptor with LocalAlloc.
        unsafe { LocalFree(descriptor) };
        assert_eq!(result, 0, "SetNamedSecurityInfoW failed: {result}");
    }

    #[cfg(windows)]
    fn file_sddl(path: &std::path::Path) -> String {
        use std::ptr;

        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT},
                DACL_SECURITY_INFORMATION,
            },
        };

        let path = wide(path.as_os_str());
        let mut descriptor = ptr::null_mut();
        // SAFETY: The path is null-terminated and GetNamedSecurityInfoW initializes the
        // descriptor on success. The allocation is freed after conversion below.
        let result = unsafe {
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        assert_eq!(result, 0, "GetNamedSecurityInfoW failed: {result}");
        let sddl = descriptor_sddl(descriptor);
        // SAFETY: GetNamedSecurityInfoW allocated this descriptor with LocalAlloc.
        unsafe { LocalFree(descriptor) };
        sddl
    }

    #[cfg(windows)]
    fn canonical_sddl(sddl: &str) -> String {
        use std::ptr;

        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
        };

        let sddl = wide(std::ffi::OsStr::new(sddl));
        let mut descriptor = ptr::null_mut();
        // SAFETY: The input is a valid null-terminated SDDL string, and the returned descriptor
        // is freed after conversion.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        assert_ne!(converted, 0, "the expected ACL is invalid");
        let sddl = descriptor_sddl(descriptor);
        // SAFETY: the conversion allocated this descriptor with LocalAlloc.
        unsafe { LocalFree(descriptor) };
        sddl
    }

    #[cfg(windows)]
    fn descriptor_sddl(descriptor: *mut std::ffi::c_void) -> String {
        use std::ptr;

        use windows_sys::Win32::{
            Foundation::LocalFree,
            Security::{
                Authorization::{
                    ConvertSecurityDescriptorToStringSecurityDescriptorW, SDDL_REVISION_1,
                },
                DACL_SECURITY_INFORMATION,
            },
        };

        let mut text = ptr::null_mut();
        // SAFETY: `descriptor` is a valid security descriptor supplied by the caller, and the
        // returned string is allocated by the API and freed below.
        let converted = unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                ptr::null_mut(),
            )
        };
        assert_ne!(converted, 0, "cannot serialize the file ACL");
        let mut length = 0;
        // SAFETY: a successful conversion returns a null-terminated UTF-16 string.
        unsafe {
            while *text.add(length) != 0 {
                length += 1;
            }
            let value = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
            LocalFree(text.cast());
            value
        }
    }
}
