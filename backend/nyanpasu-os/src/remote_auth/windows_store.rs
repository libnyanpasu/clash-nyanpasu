//! Windows handle-verified protected DACL enforcement and atomic replacement.

use std::{
    ffi::c_void,
    fs::{self, File},
    io::{self, Write},
    iter::once,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Path, PathBuf},
    ptr::{null, null_mut},
};

use nyanpasu_contracts::auth::{LoadedSecretRecord, SecretStoreError, StorageProtection};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_INVALID_FUNCTION, ERROR_NO_SECURITY_ON_OBJECT, ERROR_NOT_SUPPORTED,
        HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
    },
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            ConvertStringSidToSidW, GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,
        },
        DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation,
        GetSecurityDescriptorControl, GetTokenInformation, INHERITED_ACE, IsValidSid,
        OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SE_DACL_PRESENT, SE_DACL_PROTECTED,
        SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CREATE_NEW, CreateDirectoryW, CreateFileW,
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
        GetVolumeInformationByHandleW, OPEN_EXISTING, READ_CONTROL,
    },
    System::{
        SystemServices::{ACCESS_ALLOWED_ACE_TYPE, FILE_PERSISTENT_ACLS},
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

use super::secret_store::{
    WindowsSecurityAssessment, classify_ancestor_ace, classify_windows_security,
    collect_ancestor_dirs, generate_temp_file_name, normalize_lexical, owner_ace_grants_rw,
    read_bounded_bytes, split_target_path, windows_private_sddl,
};

const TRUSTED_ADMIN_SIDS: &[&str] = &[
    "S-1-5-18",                                                       // NT AUTHORITY\SYSTEM
    "S-1-5-32-544",                                                   // BUILTIN\Administrators
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464", // TrustedInstaller
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HandleIdentity {
    volume_serial: u32,
    index_high: u32,
    index_low: u32,
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: `self.0` is an owned Win32 handle.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

struct OwnedLocalMem(*mut c_void);

impl Drop for OwnedLocalMem {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` was allocated by `LocalAlloc`-family Win32 APIs.
            unsafe {
                let _ = LocalFree(self.0 as HLOCAL);
            }
        }
    }
}

struct CurrentUserSid {
    _token_buf: Vec<u8>,
    sid: PSID,
    sid_string: String,
    trusted_admin_sids: Vec<OwnedLocalMem>,
}

impl CurrentUserSid {
    fn query() -> Result<Self, SecretStoreError> {
        let mut token_raw: HANDLE = INVALID_HANDLE_VALUE;
        // SAFETY: querying current process token with `TOKEN_QUERY`.
        let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_raw) };
        if ok == 0 {
            return Err(map_win_err("opening current process token"));
        }
        let token = OwnedHandle(token_raw);

        let mut needed: u32 = 0;
        // SAFETY: first call retrieves required byte buffer length.
        unsafe {
            let _ = GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut needed);
        }
        if needed == 0 {
            return Err(map_win_err("sizing current process token user"));
        }

        let mut buf = vec![0u8; needed as usize];
        // SAFETY: `buf` is sized to `needed` bytes for `TOKEN_USER`.
        let ok = unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buf.as_mut_ptr().cast::<c_void>(),
                needed,
                &mut needed,
            )
        };
        if ok == 0 {
            return Err(map_win_err("reading current process token user"));
        }

        // SAFETY: `buf` contains an initialized `TOKEN_USER` pointing inside `buf`.
        let token_user = unsafe { &*(buf.as_ptr().cast::<TOKEN_USER>()) };
        let sid = token_user.User.Sid;
        // SAFETY: validating `sid` inside `buf`.
        if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
            return Err(SecretStoreError::Io {
                reason: "current process user SID is invalid".to_owned(),
            });
        }

        let mut wide_ptr: *mut u16 = null_mut();
        // SAFETY: `sid` is valid; Win32 allocates `wide_ptr` via LocalAlloc.
        let ok = unsafe { ConvertSidToStringSidW(sid, &mut wide_ptr) };
        if ok == 0 || wide_ptr.is_null() {
            return Err(map_win_err("formatting current user SID"));
        }
        let _wide_guard = OwnedLocalMem(wide_ptr.cast::<c_void>());
        let sid_string = unsafe { wide_ptr_to_string(wide_ptr) };

        let mut trusted_admin_sids = Vec::with_capacity(TRUSTED_ADMIN_SIDS.len());
        for sddl_sid in TRUSTED_ADMIN_SIDS {
            let wide = to_wide_null(std::ffi::OsStr::new(sddl_sid));
            let mut admin_sid: PSID = null_mut();
            // SAFETY: parsing well-known SID string; allocated via LocalAlloc and freed on Drop.
            if unsafe { ConvertStringSidToSidW(wide.as_ptr(), &mut admin_sid) } != 0
                && !admin_sid.is_null()
            {
                trusted_admin_sids.push(OwnedLocalMem(admin_sid));
            }
        }

        Ok(Self {
            _token_buf: buf,
            sid,
            sid_string,
            trusted_admin_sids,
        })
    }

    fn is_trusted_ancestor_sid(&self, candidate: PSID) -> bool {
        // SAFETY: `candidate` is a valid non-null SID pointer verified by caller.
        if candidate.is_null() || unsafe { IsValidSid(candidate) } == 0 {
            return false;
        }
        // SAFETY: `self.sid` stays alive as long as `&self`.
        if unsafe { EqualSid(candidate, self.sid) } != 0 {
            return true;
        }
        self.trusted_admin_sids.iter().any(|admin| {
            // SAFETY: each `admin.0` was validated by `ConvertStringSidToSidW`.
            unsafe { EqualSid(candidate, admin.0 as PSID) != 0 }
        })
    }
}

pub(super) fn probe_protection(
    secrets_path: PathBuf,
    _max_record_bytes: usize,
) -> Result<StorageProtection, SecretStoreError> {
    let (raw_dir, _) = split_target_path(&secrets_path)?;
    let user = CurrentUserSid::query()?;

    let raw_ancestors_safe = validate_windows_ancestors(&raw_dir, &user)?;
    let private_dir = normalize_lexical(&raw_dir);
    let norm_ancestors_safe = validate_windows_ancestors(&private_dir, &user)?;
    let ancestors_safe = raw_ancestors_safe && norm_ancestors_safe;

    let Some(dir_handle) = try_open_dir_handle(&private_dir)? else {
        return Ok(if ancestors_safe {
            StorageProtection::Protected
        } else {
            StorageProtection::Unprotected
        });
    };
    let dir_safe = validate_handle_security(dir_handle.0, true, &user)?;

    let file_safe = match try_open_file_handle(&secrets_path, FILE_READ_ATTRIBUTES | READ_CONTROL)?
    {
        Some(file_handle) => validate_handle_security(file_handle.0, false, &user)?,
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
    let (raw_dir, _) = split_target_path(&secrets_path)?;
    let user = CurrentUserSid::query()?;

    let raw_ancestors_safe = validate_windows_ancestors(&raw_dir, &user)?;
    let private_dir = normalize_lexical(&raw_dir);
    let norm_ancestors_safe = validate_windows_ancestors(&private_dir, &user)?;
    let ancestors_safe = raw_ancestors_safe && norm_ancestors_safe;

    let Some(dir_handle) = try_open_dir_handle(&private_dir)? else {
        return Ok(None);
    };
    let dir_safe = validate_handle_security(dir_handle.0, true, &user)?;

    let Some(file_handle) = try_open_file_handle(&secrets_path, FILE_GENERIC_READ | READ_CONTROL)?
    else {
        return Ok(None);
    };
    let file_safe = validate_handle_security(file_handle.0, false, &user)?;
    let info = query_handle_info(file_handle.0)?;
    let size = ((info.nFileSizeHigh as u64) << 32) | (info.nFileSizeLow as u64);
    if size > max_record_bytes as u64 {
        return Err(SecretStoreError::Io {
            reason: "secret store file size exceeds maximum byte limit".to_owned(),
        });
    }

    // SAFETY: transfer ownership of validated `file_handle` into `std::fs::File`.
    let raw = file_handle.0;
    std::mem::forget(file_handle);
    let mut file = unsafe { File::from_raw_handle(raw as _) };
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
    let user = CurrentUserSid::query()?;

    let raw_ancestors_safe = validate_windows_ancestors(&raw_dir, &user)?;
    let private_dir = normalize_lexical(&raw_dir);
    let norm_ancestors_safe = validate_windows_ancestors(&private_dir, &user)?;
    if require_protected && !(raw_ancestors_safe && norm_ancestors_safe) {
        return Err(SecretStoreError::UnprotectedMedium);
    }

    ensure_parent_dirs(&private_dir)?;
    // Retain `dir_handle` without `FILE_SHARE_DELETE` across temp creation, write, and rename.
    let dir_handle = open_or_create_private_dir(&private_dir, &user)?;
    let pinned_dir_identity = handle_identity(dir_handle.0)?;

    // Full post-creation ancestry + directory + existing file check BEFORE any temp file or
    // plaintext bytes are written to disk.
    let post_ancestors_safe = validate_windows_ancestors(&private_dir, &user)?;
    let dir_safe = validate_handle_security(dir_handle.0, true, &user)?;
    if require_protected && !(post_ancestors_safe && dir_safe) {
        return Err(SecretStoreError::UnprotectedMedium);
    }

    let cleaned_secrets_path = private_dir.join(&file_name);
    if let Some(existing_handle) =
        try_open_file_handle(&cleaned_secrets_path, FILE_READ_ATTRIBUTES | READ_CONTROL)?
    {
        let existing_safe = validate_handle_security(existing_handle.0, false, &user)?;
        if require_protected && !existing_safe {
            return Err(SecretStoreError::UnprotectedMedium);
        }
    }

    let temp_name = generate_temp_file_name(&file_name)?;
    let temp_path = private_dir.join(temp_name);
    let mut temp_file = create_protected_temp_file(&temp_path, &user)?;

    let write_result = (|| -> Result<(), SecretStoreError> {
        let temp_safe =
            validate_handle_security(temp_file.as_raw_handle() as HANDLE, false, &user)?;
        if require_protected && !temp_safe {
            return Err(SecretStoreError::UnprotectedMedium);
        }

        temp_file
            .write_all(bytes)
            .map_err(map_std_io("writing temp secret file"))?;
        temp_file
            .flush()
            .map_err(map_std_io("flushing temp secret file"))?;
        temp_file
            .sync_all()
            .map_err(map_std_io("syncing temp secret file"))?;

        // Re-verify path still points to the same pinned `private_dir` identity (volume serial +
        // file index) and retains owner-only ACL protection before replacing target.
        let latest_dir =
            try_open_dir_handle(&private_dir)?.ok_or_else(|| SecretStoreError::Io {
                reason: "private secret directory vanished during write".to_owned(),
            })?;
        if handle_identity(latest_dir.0)? != pinned_dir_identity {
            return Err(SecretStoreError::Io {
                reason: "private secret directory identity changed during write".to_owned(),
            });
        }
        let post_write_ancestors = validate_windows_ancestors(&private_dir, &user)?;
        let dir_still_safe = validate_handle_security(latest_dir.0, true, &user)?;
        let temp_still_safe =
            validate_handle_security(temp_file.as_raw_handle() as HANDLE, false, &user)?;
        if require_protected && !(post_write_ancestors && dir_still_safe && temp_still_safe) {
            return Err(SecretStoreError::UnprotectedMedium);
        }
        Ok(())
    })();

    drop(temp_file);
    if let Err(err) = write_result {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }

    // Re-check pinned directory path immediately before `fs::rename` while `dir_handle` remains
    // open without `FILE_SHARE_DELETE`.
    let pre_rename_dir =
        try_open_dir_handle(&private_dir)?.ok_or_else(|| SecretStoreError::Io {
            reason: "private secret directory vanished before atomic replacement".to_owned(),
        })?;
    if handle_identity(pre_rename_dir.0)? != pinned_dir_identity {
        let _ = fs::remove_file(&temp_path);
        return Err(SecretStoreError::Io {
            reason: "private secret directory identity changed before atomic replacement"
                .to_owned(),
        });
    }

    if let Err(err) = fs::rename(&temp_path, &cleaned_secrets_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(map_std_io("atomically replacing secret store file")(err));
    }
    Ok(())
}

fn validate_windows_ancestors(
    private_dir: &Path,
    user: &CurrentUserSid,
) -> Result<bool, SecretStoreError> {
    let ancestors = collect_ancestor_dirs(private_dir)?;

    let mut all_safe = true;
    for ancestor in ancestors {
        let Some(handle) = try_open_dir_handle(&ancestor)? else {
            continue;
        };
        let info = query_handle_info(handle.0)?;
        if (info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
            return Err(SecretStoreError::Io {
                reason: "refusing reparse point in remote-access secret directory ancestor"
                    .to_owned(),
            });
        }
        if !validate_ancestor_acl(handle.0, user)? {
            all_safe = false;
        }
    }
    Ok(all_safe)
}

fn validate_ancestor_acl(handle: HANDLE, user: &CurrentUserSid) -> Result<bool, SecretStoreError> {
    if !volume_supports_persistent_acls(handle) {
        return Ok(false);
    }

    let mut owner_sid: PSID = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `sd` freed by `OwnedLocalMem`.
    let rc = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner_sid,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut sd,
        )
    };
    let _sd_guard = OwnedLocalMem(sd);
    if is_unsupported_security_rc(rc) {
        return Ok(false);
    }
    if rc != 0 || sd.is_null() {
        return Err(SecretStoreError::Io {
            reason: format!("GetSecurityInfo on ancestor failed with error code {rc}"),
        });
    }

    if !user.is_trusted_ancestor_sid(owner_sid) {
        return Ok(false);
    }

    let mut control: u16 = 0;
    let mut revision: u32 = 0;
    // SAFETY: `sd` is valid.
    if unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut revision) } == 0 {
        return Ok(false);
    }
    if (control & SE_DACL_PRESENT) == 0 || dacl.is_null() {
        return Ok(false);
    }

    let mut size_info = ACL_SIZE_INFORMATION::default();
    // SAFETY: `dacl` is a valid non-null ACL inside `sd`.
    if unsafe {
        GetAclInformation(
            dacl,
            (&mut size_info as *mut ACL_SIZE_INFORMATION).cast::<c_void>(),
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
    {
        return Ok(false);
    }

    for idx in 0..size_info.AceCount {
        let mut ace_ptr: *mut c_void = null_mut();
        // SAFETY: `idx < size_info.AceCount`.
        if unsafe { GetAce(dacl, idx, &mut ace_ptr) } == 0 || ace_ptr.is_null() {
            return Ok(false);
        }
        // SAFETY: every ACE begins with `ACE_HEADER`.
        let header = unsafe { *(ace_ptr.cast::<ACE_HEADER>()) };
        if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
            // Fail closed on unknown/object/callback ACEs; only recognize allow ACEs.
            return Ok(false);
        }
        // SAFETY: `AceType == ACCESS_ALLOWED_ACE_TYPE` guarantees layout `ACCESS_ALLOWED_ACE`.
        let ace = unsafe { &*(ace_ptr.cast::<ACCESS_ALLOWED_ACE>()) };
        let ace_sid = (&ace.SidStart as *const u32).cast_mut().cast::<c_void>();
        match classify_ancestor_ace(
            header.AceType,
            header.AceFlags,
            ace.Mask,
            user.is_trusted_ancestor_sid(ace_sid),
        ) {
            Ok(false) => {}
            Ok(true) | Err(()) => return Ok(false),
        }
    }

    Ok(true)
}

fn ensure_parent_dirs(private_dir: &Path) -> Result<(), SecretStoreError> {
    if let Some(parent) = private_dir.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent).map_err(map_std_io("creating secret parent directory"))?;
    }
    Ok(())
}

fn open_or_create_private_dir(
    private_dir: &Path,
    user: &CurrentUserSid,
) -> Result<OwnedHandle, SecretStoreError> {
    if let Some(handle) = try_open_dir_handle(private_dir)? {
        return Ok(handle);
    }

    let sddl = windows_private_sddl(&user.sid_string, true);
    let (_sd_guard, mut sa) = build_security_attributes(&sddl)?;
    let wide = to_wide_null(private_dir.as_os_str());
    // SAFETY: `wide` is NUL-terminated and `sa` points to a valid security descriptor for this call.
    let created =
        unsafe { CreateDirectoryW(wide.as_ptr(), &mut sa as *mut SECURITY_ATTRIBUTES as _) };
    if created == 0 {
        let last = io::Error::last_os_error();
        if last.kind() != io::ErrorKind::AlreadyExists {
            return Err(map_std_io("creating protected private directory")(last));
        }
    }

    try_open_dir_handle(private_dir)?.ok_or_else(|| SecretStoreError::Io {
        reason: "private secret directory vanished immediately after creation".to_owned(),
    })
}

fn create_protected_temp_file(
    temp_path: &Path,
    user: &CurrentUserSid,
) -> Result<File, SecretStoreError> {
    let sddl = windows_private_sddl(&user.sid_string, false);
    let (_sd_guard, sa) = build_security_attributes(&sddl)?;
    let wide = to_wide_null(temp_path.as_os_str());
    // SAFETY: `wide` is NUL-terminated and `sa` holds a protected owner-only DACL.
    let raw_handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_GENERIC_READ | FILE_GENERIC_WRITE | READ_CONTROL,
            0,
            &sa,
            CREATE_NEW,
            FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if raw_handle == INVALID_HANDLE_VALUE {
        return Err(map_win_err("creating protected temp secret file"));
    }
    // SAFETY: `raw_handle` is valid and uniquely owned.
    Ok(unsafe { File::from_raw_handle(raw_handle as _) })
}

fn try_open_dir_handle(path: &Path) -> Result<Option<OwnedHandle>, SecretStoreError> {
    let wide = to_wide_null(path.as_os_str());
    // Do NOT include `FILE_SHARE_DELETE`: pinning this handle prevents concurrent delete/rename of
    // the validated directory while leaving child file creation/renames unaffected.
    // SAFETY: `wide` is NUL-terminated; opens without following reparse points.
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_READ_ATTRIBUTES | READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        let last = io::Error::last_os_error();
        if last.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(map_std_io("opening secret directory handle")(last));
    }
    Ok(Some(OwnedHandle(raw)))
}

fn try_open_file_handle(path: &Path, access: u32) -> Result<Option<OwnedHandle>, SecretStoreError> {
    let wide = to_wide_null(path.as_os_str());
    // SAFETY: `wide` is NUL-terminated; opens without following reparse points and without
    // granting `FILE_SHARE_DELETE`.
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        let last = io::Error::last_os_error();
        if last.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(map_std_io("opening secret file handle")(last));
    }
    Ok(Some(OwnedHandle(raw)))
}

fn query_handle_info(handle: HANDLE) -> Result<BY_HANDLE_FILE_INFORMATION, SecretStoreError> {
    let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    // SAFETY: `handle` is an open Win32 file/dir handle and `info` is valid writable memory.
    let ok = unsafe { GetFileInformationByHandle(handle, info.as_mut_ptr()) };
    if ok == 0 {
        return Err(map_win_err("querying secret handle information"));
    }
    // SAFETY: initialized by `GetFileInformationByHandle`.
    Ok(unsafe { info.assume_init() })
}

fn handle_identity(handle: HANDLE) -> Result<HandleIdentity, SecretStoreError> {
    let info = query_handle_info(handle)?;
    Ok(HandleIdentity {
        volume_serial: info.dwVolumeSerialNumber,
        index_high: info.nFileIndexHigh,
        index_low: info.nFileIndexLow,
    })
}

fn volume_supports_persistent_acls(handle: HANDLE) -> bool {
    let mut fs_flags: u32 = 0;
    // SAFETY: `handle` is an open file or directory handle; only queries `fs_flags`.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            handle,
            null_mut(),
            0,
            null_mut(),
            null_mut(),
            &mut fs_flags,
            null_mut(),
            0,
        )
    };
    ok != 0 && (fs_flags & FILE_PERSISTENT_ACLS) != 0
}

fn is_unsupported_security_rc(rc: u32) -> bool {
    rc == ERROR_NOT_SUPPORTED || rc == ERROR_INVALID_FUNCTION || rc == ERROR_NO_SECURITY_ON_OBJECT
}

pub(super) fn validate_handle_security(
    handle: HANDLE,
    expect_dir: bool,
    user: &CurrentUserSid,
) -> Result<bool, SecretStoreError> {
    let info = query_handle_info(handle)?;
    if (info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) != 0 {
        return classify_windows_security(WindowsSecurityAssessment::IntegrityViolation(
            "refusing reparse point on remote-access secret object",
        ));
    }
    let is_dir = (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
    if is_dir != expect_dir {
        return classify_windows_security(WindowsSecurityAssessment::IntegrityViolation(
            if expect_dir {
                "remote-access secret directory target is not a directory"
            } else {
                "remote-access secret store target is not a regular file"
            },
        ));
    }
    if !expect_dir && info.nNumberOfLinks > 1 {
        return classify_windows_security(WindowsSecurityAssessment::IntegrityViolation(
            "refusing hardlinked remote-access secret file",
        ));
    }

    if !volume_supports_persistent_acls(handle) {
        return classify_windows_security(WindowsSecurityAssessment::UnsupportedAclMedium);
    }

    let mut owner_sid: PSID = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut sd: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: querying owner SID and DACL from the already-opened handle; `sd` is freed via `OwnedLocalMem`.
    let rc = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner_sid,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut sd,
        )
    };
    let _sd_guard = OwnedLocalMem(sd);
    if is_unsupported_security_rc(rc) {
        return classify_windows_security(WindowsSecurityAssessment::UnsupportedAclMedium);
    }
    if rc != 0 || sd.is_null() {
        return Err(SecretStoreError::Io {
            reason: format!("GetSecurityInfo failed with error code {rc}"),
        });
    }

    // SAFETY: `owner_sid` and `user.sid` point into valid descriptors alive during this call.
    let owner_is_current_user = !owner_sid.is_null()
        && unsafe { IsValidSid(owner_sid) } != 0
        && unsafe { EqualSid(owner_sid, user.sid) } != 0;

    let mut control: u16 = 0;
    let mut revision: u32 = 0;
    // SAFETY: `sd` is valid.
    if unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut revision) } == 0 {
        return Err(map_win_err("inspecting security descriptor control flags"));
    }
    let dacl_present = (control & SE_DACL_PRESENT) != 0;
    let dacl_protected = (control & SE_DACL_PROTECTED) != 0;
    let dacl_is_null = dacl.is_null();

    let mut size_info = ACL_SIZE_INFORMATION::default();
    if !dacl_is_null {
        // SAFETY: `dacl` is a valid non-null ACL inside `sd`.
        if unsafe {
            GetAclInformation(
                dacl,
                (&mut size_info as *mut ACL_SIZE_INFORMATION).cast::<c_void>(),
                std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        } == 0
        {
            return Ok(false);
        }
    }

    let mut has_inherited_ace = false;
    let mut has_unsupported_ace = false;
    let mut has_non_owner_allow_ace = false;
    let mut has_effective_owner_rw_allow = false;

    for idx in 0..size_info.AceCount {
        let mut ace_ptr: *mut c_void = null_mut();
        // SAFETY: `idx < size_info.AceCount`.
        if unsafe { GetAce(dacl, idx, &mut ace_ptr) } == 0 || ace_ptr.is_null() {
            has_unsupported_ace = true;
            break;
        }
        // SAFETY: every ACE begins with `ACE_HEADER`.
        let header = unsafe { *(ace_ptr.cast::<ACE_HEADER>()) };
        if (u32::from(header.AceFlags) & INHERITED_ACE) != 0 {
            has_inherited_ace = true;
        }
        if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
            has_unsupported_ace = true;
            continue;
        }
        // SAFETY: `AceType == ACCESS_ALLOWED_ACE_TYPE` guarantees layout `ACCESS_ALLOWED_ACE`.
        let ace = unsafe { &*(ace_ptr.cast::<ACCESS_ALLOWED_ACE>()) };
        let ace_sid = (&ace.SidStart as *const u32).cast_mut().cast::<c_void>();
        // SAFETY: checking `ace_sid` against `user.sid`.
        if unsafe { IsValidSid(ace_sid) } == 0 || unsafe { EqualSid(ace_sid, user.sid) } == 0 {
            has_non_owner_allow_ace = true;
            continue;
        }
        if owner_ace_grants_rw(header.AceFlags, ace.Mask) {
            has_effective_owner_rw_allow = true;
        }
    }

    classify_windows_security(WindowsSecurityAssessment::Descriptor {
        owner_is_current_user,
        dacl_present,
        dacl_protected,
        dacl_is_null,
        ace_count: size_info.AceCount,
        has_inherited_ace,
        has_unsupported_ace,
        has_non_owner_allow_ace,
        has_effective_owner_rw_allow,
    })
}

fn build_security_attributes(
    sddl: &str,
) -> Result<(OwnedLocalMem, SECURITY_ATTRIBUTES), SecretStoreError> {
    let wide = to_wide_null(std::ffi::OsStr::new(sddl));
    let mut sd: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: `wide` is NUL-terminated SDDL string; Win32 allocates `sd` freed by `OwnedLocalMem`.
    let ok = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            SDDL_REVISION_1,
            &mut sd,
            null_mut(),
        )
    };
    if ok == 0 || sd.is_null() {
        return Err(map_win_err("building protected security descriptor"));
    }
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd,
        bInheritHandle: 0,
    };
    Ok((OwnedLocalMem(sd), sa))
}

fn to_wide_null(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(once(0)).collect()
}

unsafe fn wide_ptr_to_string(ptr: *const u16) -> String {
    let mut len = 0usize;
    // SAFETY: caller guarantees `ptr` points to a NUL-terminated UTF-16 string.
    unsafe {
        while *ptr.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
    }
}

fn map_win_err(action: &'static str) -> SecretStoreError {
    let err = io::Error::last_os_error();
    SecretStoreError::Io {
        reason: format!("{action} ({})", err.kind()),
    }
}

fn map_std_io(action: &'static str) -> impl FnOnce(io::Error) -> SecretStoreError {
    move |err| SecretStoreError::Io {
        reason: format!("{action} ({})", err.kind()),
    }
}

#[cfg(test)]
pub(super) mod test_helpers {
    use std::{
        path::Path,
        ptr::{null, null_mut},
    };

    use windows_sys::Win32::Security::{
        ACL,
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
            SetSecurityInfo,
        },
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, UNPROTECTED_DACL_SECURITY_INFORMATION,
    };

    use super::{
        CurrentUserSid, OwnedHandle, OwnedLocalMem, create_protected_temp_file,
        open_or_create_private_dir, to_wide_null, try_open_dir_handle, validate_ancestor_acl,
        validate_handle_security, validate_windows_ancestors,
    };

    fn open_test_dir_for_sddl(path: &Path) -> OwnedHandle {
        use windows_sys::Win32::{
            Foundation::INVALID_HANDLE_VALUE,
            Storage::FileSystem::{
                CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
                FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
                READ_CONTROL, WRITE_DAC, WRITE_OWNER,
            },
        };

        let wide = to_wide_null(path.as_os_str());
        // Test-only fixture handle: request `WRITE_DAC | WRITE_OWNER` only on temp directories so
        // `SetSecurityInfo` never fails with `ACCESS_DENIED` while production handles stay read-only.
        let raw = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_READ_ATTRIBUTES | READ_CONTROL | WRITE_DAC | WRITE_OWNER,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                null_mut(),
            )
        };
        assert_ne!(
            raw, INVALID_HANDLE_VALUE,
            "opening test directory for SDDL failed"
        );
        OwnedHandle(raw)
    }

    pub fn current_user_sid_string() -> String {
        CurrentUserSid::query()
            .expect("current user SID must resolve")
            .sid_string
    }

    pub fn dir_ancestor_acl_is_safe(path: &Path) -> bool {
        let user = CurrentUserSid::query().expect("current user SID must resolve");
        let handle = try_open_dir_handle(path)
            .unwrap()
            .expect("target dir must exist");
        validate_ancestor_acl(handle.0, &user).unwrap()
    }

    pub fn path_ancestors_are_safe(private_dir: &Path) -> bool {
        let user = CurrentUserSid::query().expect("current user SID must resolve");
        validate_windows_ancestors(private_dir, &user).unwrap()
    }

    pub fn verify_created_private_objects_owned_by_token_user(
        private_dir: &Path,
        temp_file_path: &Path,
    ) {
        use std::os::windows::io::AsRawHandle;

        let user = CurrentUserSid::query().expect("current user SID must resolve");
        let dir_handle = open_or_create_private_dir(private_dir, &user).unwrap();
        assert!(validate_handle_security(dir_handle.0, true, &user).unwrap());

        let temp_file = create_protected_temp_file(temp_file_path, &user).unwrap();
        assert!(validate_handle_security(temp_file.as_raw_handle() as _, false, &user).unwrap());
    }

    pub fn apply_sddl_to_dir(path: &Path, sddl: &str, protected: bool) {
        use windows_sys::Win32::Security::OWNER_SECURITY_INFORMATION;

        let user = CurrentUserSid::query().expect("current user SID must resolve");
        let handle = open_test_dir_for_sddl(path);
        let wide = to_wide_null(std::ffi::OsStr::new(sddl));
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                null_mut(),
            )
        };
        assert_ne!(ok, 0);
        let _guard = OwnedLocalMem(sd);

        let mut dacl_present = 0;
        let mut dacl: *mut ACL = null_mut();
        let mut defaulted = 0;
        let ok =
            unsafe { GetSecurityDescriptorDacl(sd, &mut dacl_present, &mut dacl, &mut defaulted) };
        assert_ne!(ok, 0);

        let info = if protected {
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            OWNER_SECURITY_INFORMATION
                | DACL_SECURITY_INFORMATION
                | UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        let rc = unsafe {
            SetSecurityInfo(
                handle.0,
                SE_FILE_OBJECT,
                info,
                user.sid,
                null(),
                dacl,
                null(),
            )
        };
        assert_eq!(rc, 0, "SetSecurityInfo failed with {rc}");
    }
}
