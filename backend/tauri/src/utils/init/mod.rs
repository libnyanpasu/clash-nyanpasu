use crate::core::migration::modules::profiles::ProfilesFormat;
use fs_extra::dir::CopyOptions;
use nyanpasu_core::format::Format;
use nyanpasu_paths::PathResolver;
#[cfg(windows)]
use runas::Command as RunasCommand;
use snafu::ResultExt as _;
use std::{
    fs::{self, File},
    io::{self, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus},
    sync::Arc,
    thread::JoinHandle,
};
use tauri::utils::platform::current_exe;
pub mod logging;

#[cfg(test)]
mod tests;

/// The `migrate` subprocess exited unsuccessfully.
#[derive(Debug, thiserror::Error)]
#[error("child process failed: {status:?}, err: {stderr}")]
pub struct MigrationChildFailed {
    pub status: std::process::ExitStatus,
    pub stderr: String,
}

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to locate the executable"))]
    LocateExecutable { source: io::Error },
    #[snafu(display("failed to canonicalize executable"))]
    CanonicalizeExecutable { source: io::Error },
    #[snafu(display("failed to open migration log {}", path.display()))]
    OpenMigrationLog { path: PathBuf, source: io::Error },
    #[snafu(display("failed to create migration result directory"))]
    CreateResultDirectory { source: io::Error },
    #[snafu(display("failed to open migration stdout pipe"))]
    OpenStdoutPipe { source: io::Error },
    #[snafu(display("failed to open migration stderr pipe"))]
    OpenStderrPipe { source: io::Error },
    #[snafu(display("failed to spawn migration child"))]
    SpawnMigration { source: io::Error },
    #[snafu(display("failed to wait for migration child; stderr: {stderr}"))]
    WaitMigration { source: io::Error, stderr: String },
    #[snafu(display("migration child reported a failure"))]
    ReadChildFailure {
        #[snafu(source(from(crate::core::migration::child_result::Failure, Box::new)))]
        source: Box<crate::core::migration::child_result::Failure>,
    },
    #[snafu(display("migration child exited without a readable failure report"))]
    ChildExited { source: MigrationChildFailed },
    // The existing Format port returns anyhow; do not erase or reformat that source.
    #[snafu(display("failed to serialize initial profiles"))]
    SerializeProfiles { source: anyhow::Error },
    #[snafu(display("failed to save initial profiles {}", path.display()))]
    SaveProfiles { path: PathBuf, source: io::Error },
    #[snafu(display("the bundled resources directory is unknown"))]
    LocateResources,
    #[snafu(display("failed to create singleton directory"))]
    CreateSingletonDirectory {
        source: nyanpasu_paths::CreateDirError,
    },
    #[snafu(display("failed to create single instance"))]
    CreateSingleton {
        source: single_instance::error::SingleInstanceError,
    },
}

pub fn run_pending_migrations(paths: &PathResolver) -> std::result::Result<(), Error> {
    let current_exe = current_exe().context(LocateExecutableSnafu)?;
    let current_exe = dunce::canonicalize(current_exe).context(CanonicalizeExecutableSnafu)?;
    let log_path = paths
        .app_data_dir()
        .join("migration.log")
        .into_std_path_buf();
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&log_path)
        .context(OpenMigrationLogSnafu { path: log_path })?;
    let mut command = Command::new(current_exe);
    let result_dir = tempfile::tempdir().context(CreateResultDirectorySnafu)?;
    let result_path = result_dir.path().join("result.json");
    command
        .arg("migrate")
        .arg("--result-file")
        .arg(&result_path);
    run_migration_command(command, file, &result_path)
}

fn run_migration_command(
    mut command: Command,
    file: File,
    result_path: &Path,
) -> std::result::Result<(), Error> {
    let file = Arc::new(parking_lot::Mutex::new(file));
    let (stdout_reader, stdout_writer) = os_pipe::pipe().context(OpenStdoutPipeSnafu)?;
    let (stderr_reader, stderr_writer) = os_pipe::pipe().context(OpenStderrPipeSnafu)?;
    let errs = Arc::new(parking_lot::Mutex::new(String::new()));
    let mut child = command
        .stderr(stderr_writer)
        .stdout(stdout_writer)
        .spawn()
        .context(SpawnMigrationSnafu)?;
    // Command owns the parent's write handles until dropped; readers need them closed for EOF.
    drop(command);
    let file_ = file.clone();
    let errs_ = errs.clone();
    let stdout_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout_reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match nyanpasu_utils::io::read_line(&mut reader, &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    let mut file = file_.lock();
                    let _ = file.write_all(&buf);
                }
                Err(e) => {
                    eprintln!("failed to read stdout: {e:?}");
                    let mut errs = errs_.lock();
                    errs.push_str(&format!("failed to read stdout: {e:?}\n"));
                    break;
                }
            }
        }
    });
    let errs_ = errs.clone();
    let stderr_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr_reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match nyanpasu_utils::io::read_line(&mut reader, &mut buf) {
                Ok(0) => break,
                Ok(_) => {
                    let mut file = file.lock();
                    let _ = file.write_all(&buf);
                    let mut errs = errs_.lock();
                    errs.push_str(unsafe { std::str::from_utf8_unchecked(&buf) });
                }
                Err(e) => {
                    eprintln!("failed to read stderr: {e:?}");
                    let mut errs = errs_.lock();
                    errs.push_str(&format!("failed to read stderr: {e:?}\n"));
                    break;
                }
            }
        }
    });
    let result = wait_for_migration_output(&mut child, [stdout_thread, stderr_thread]);
    let err = errs.lock();
    let status = result.context(WaitMigrationSnafu {
        stderr: err.clone(),
    })?;
    if status.success() {
        return Ok(());
    }
    match crate::core::migration::child_result::Failure::read(result_path) {
        Ok(source) => Err(Error::ReadChildFailure {
            source: Box::new(source),
        }),
        Err(error) => {
            tracing::warn!(%error, "migration failure report unavailable; using exit status and stderr");
            Err(Error::ChildExited {
                source: MigrationChildFailed {
                    status,
                    stderr: err.clone(),
                },
            })
        }
    }
}

fn wait_for_migration_output(
    child: &mut Child,
    readers: [JoinHandle<()>; 2],
) -> io::Result<ExitStatus> {
    let result = child.wait();
    // Join both readers before returning or resuming a reader's panic.
    let reader_results = readers.map(JoinHandle::join);
    for result in reader_results {
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
    result
}

/// Initialize all the config files
/// before tauri setup
pub fn init_config(paths: &PathResolver) -> std::result::Result<(), Error> {
    // Check if old config dir exist and new config dir is not exist
    // let mut old_app_dir: Option<PathBuf> = None;
    // let mut app_dir: Option<PathBuf> = None;
    // crate::dialog_err!(dirs::old_app_home_dir().map(|_old_app_dir| {
    //     old_app_dir = Some(_old_app_dir);
    // }));

    // crate::dialog_err!(dirs::app_home_dir().map(|_app_dir| {
    //     app_dir = Some(_app_dir);
    // }));

    // if let (Some(app_dir), Some(old_app_dir)) = (app_dir, old_app_dir) {
    //     let msg = t!("dialog.migrate");
    //     if !app_dir.exists() && old_app_dir.exists() && migrate_dialog(msg.to_string().as_str()) {
    //         if let Err(e) = do_config_migration(&old_app_dir, &app_dir) {
    //             super::dialog::error_dialog(format!("failed to do migration: {:?}", e))
    //         }
    //     }
    //     if !app_dir.exists() {
    //         let _ = fs::create_dir_all(app_dir);
    //     }
    // }

    let path = paths.profiles_path();
    if !path.exists() {
        // Stamped like every later write, since the app refuses to load
        // an unstamped profiles.yaml.
        let mut content = Vec::new();
        ProfilesFormat::default()
            .serialize(
                &mut content,
                &nyanpasu_config::profile::Profiles::default(),
                Some("# Clash Nyanpasu"),
            )
            .context(SerializeProfilesSnafu)?;
        fs::write(&path, content).context(SaveProfilesSnafu {
            path: path.as_std_path(),
        })?;
    }

    Ok(())
}

/// The bundled `resources` dir, which only Tauri can locate.
pub fn bundled_resources_dir(manager: &impl tauri::Manager<tauri::Wry>) -> tauri::Result<PathBuf> {
    Ok(manager.path().resource_dir()?.join("resources"))
}

/// initialize app resources
pub fn init_resources(app_dir: &Path, res_dir: Option<&Path>) -> std::result::Result<(), Error> {
    let res_dir = res_dir.ok_or(Error::LocateResources)?;

    if !app_dir.exists() {
        let _ = fs::create_dir_all(app_dir);
    }
    if !res_dir.exists() {
        let _ = fs::create_dir_all(res_dir);
    }

    #[cfg(target_os = "windows")]
    let file_list = ["Country.mmdb", "geoip.dat", "geosite.dat", "wintun.dll"];
    #[cfg(not(target_os = "windows"))]
    let file_list = ["Country.mmdb", "geoip.dat", "geosite.dat"];

    // copy the resource file
    // if the source file is newer than the destination file, copy it over
    for file in file_list.iter() {
        let src_path = res_dir.join(file);
        let dest_path = app_dir.join(file);

        let handle_copy = || {
            match fs::copy(&src_path, &dest_path) {
                Ok(_) => log::debug!(target: "app", "resources copied '{file}'"),
                Err(err) => {
                    log::error!(target: "app", "failed to copy resources '{file}', {err:?}")
                }
            };
        };

        if src_path.exists() && !dest_path.exists() {
            handle_copy();
            continue;
        }

        let src_modified = fs::metadata(&src_path).and_then(|m| m.modified());
        let dest_modified = fs::metadata(&dest_path).and_then(|m| m.modified());

        match (src_modified, dest_modified) {
            (Ok(src_modified), Ok(dest_modified)) => {
                if src_modified > dest_modified {
                    handle_copy();
                } else {
                    log::debug!(target: "app", "skipping resource copy '{file}'");
                }
            }
            _ => {
                log::debug!(target: "app", "failed to get modified '{file}'");
                handle_copy();
            }
        };
    }

    Ok(())
}

/// The name the instances of one user contend for: a lock file in the config dir, or on
/// Windows a session-local name carrying the user's SID.
pub fn single_instance_placeholder(app_name: &str, config_dir: &Path) -> String {
    #[cfg(windows)]
    {
        let sid = current_user_sid().unwrap_or_else(|| config_hash(config_dir));
        format!("Local\\{app_name}-{sid}")
    }
    #[cfg(not(windows))]
    {
        let _ = app_name;
        config_dir
            .join("instance.lock")
            .to_string_lossy()
            .to_string()
    }
}

/// The current user's SID, from PowerShell or else WMIC.
#[cfg(windows)]
fn current_user_sid() -> Option<String> {
    use std::os::windows::process::CommandExt;

    let output = Command::new("powershell")
        .args([
            "-Command",
            "[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value",
        ])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();
    if let Ok(output) = output
        && output.status.success()
    {
        let sid = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !sid.is_empty() {
            return Some(sid);
        }
    }

    let output = Command::new("wmic")
        .args([
            "useraccount",
            "where",
            "name='%username%'",
            "get",
            "sid",
            "/value",
        ])
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .output();
    if let Ok(output) = output
        && output.status.success()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(sid) = line.strip_prefix("SID=") {
                let sid = sid.trim();
                if !sid.is_empty() {
                    return Some(sid.to_string());
                }
            }
        }
    }
    None
}

/// What stands in for the SID when it cannot be read.
#[cfg(windows)]
fn config_hash(config_dir: &Path) -> String {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };
    let mut hasher = DefaultHasher::new();
    config_dir.to_string_lossy().hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

pub fn check_singleton(
    paths: &PathResolver,
) -> std::result::Result<Option<single_instance::SingleInstance>, Error> {
    // The config dir is created first, as the lock file and the hash fallback live in it.
    nyanpasu_paths::create_dir_all(paths.app_config_dir())
        .context(CreateSingletonDirectorySnafu)?;
    let placeholder = single_instance_placeholder(
        crate::host_paths::APP_NAME,
        paths.app_config_dir().as_std_path(),
    );
    for i in 0..5 {
        let instance =
            single_instance::SingleInstance::new(&placeholder).context(CreateSingletonSnafu)?;
        if instance.is_single() {
            return Ok(Some(instance));
        }
        if i != 4 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    Ok(None)
}

pub fn do_config_migration(old_app_dir: &PathBuf, app_dir: &PathBuf) -> anyhow::Result<()> {
    let copy_option = CopyOptions::new();
    let copy_option = copy_option.overwrite(true);
    let copy_option = copy_option.content_only(true);
    if let Err(e) = fs_extra::dir::move_dir(old_app_dir, app_dir, &copy_option) {
        match e.kind {
            #[cfg(windows)]
            fs_extra::error::ErrorKind::PermissionDenied => {
                // It seems that clash-verge-service is running, so kill it.
                let status = RunasCommand::new("cmd")
                    .args(&["/C", "taskkill", "/IM", "clash-verge-service.exe", "/F"])
                    .status()?;
                if !status.success() {
                    anyhow::bail!("failed to kill clash-verge-service.exe")
                }
                fs::rename(old_app_dir, app_dir)?;
            }
            _ => return Err(e.into()),
        };
    }
    Ok(())
}
