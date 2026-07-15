use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use nyanpasu_ipc::client::{ClientError, shortcuts::Client};
use tempfile::TempDir;
use tokio::{process::Child, time::Instant};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

struct ServiceProcess {
    child: Child,
}

impl ServiceProcess {
    async fn shutdown(mut self) -> Result<()> {
        if self
            .child
            .try_wait()
            .context("failed to inspect nyanpasu-service during shutdown")?
            .is_none()
        {
            self.child
                .kill()
                .await
                .context("failed to terminate nyanpasu-service")?;
        }
        self.child
            .wait()
            .await
            .context("failed to reap nyanpasu-service")?;
        cleanup_global_runtime_files();
        Ok(())
    }
}

impl Drop for ServiceProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
        cleanup_global_runtime_files();
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    ensure_no_service_is_running().await?;

    let binary = service_binary()?;
    let dirs = SmokeDirs::new()?;
    let daemon_log = fs::File::create(&dirs.output)
        .with_context(|| format!("failed to create {}", dirs.output.display()))?;
    let daemon_stderr = daemon_log
        .try_clone()
        .context("failed to clone the daemon log handle")?;
    let child = tokio::process::Command::new(&binary)
        .arg("server")
        .arg("--nyanpasu-config-dir")
        .arg(&dirs.config)
        .arg("--nyanpasu-data-dir")
        .arg(&dirs.data)
        .arg("--nyanpasu-app-dir")
        .arg(&dirs.app)
        .stdin(Stdio::null())
        .stdout(Stdio::from(daemon_log))
        .stderr(Stdio::from(daemon_stderr))
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("failed to start {}", binary.display()))?;
    let mut service = ServiceProcess { child };

    let result = async {
        let status = wait_for_status(&mut service.child).await?;
        let logs = Client::service_default()
            .inspect_logs()
            .await
            .context("read-only inspect_logs request failed")?;
        Result::<_>::Ok((
            status.version.into_owned(),
            status.core_infos.state,
            logs.logs.len(),
        ))
    }
    .await;
    let shutdown_result = service.shutdown().await;

    let (version, core_state, log_count) = match result {
        Ok(summary) => summary,
        Err(error) => {
            let output = fs::read_to_string(&dirs.output)
                .unwrap_or_else(|read_error| format!("<failed to read daemon log: {read_error}>"));
            shutdown_result.context("failed to stop daemon after smoke-test failure")?;
            bail!("{error:#}\nnyanpasu-service output:\n{output}");
        }
    };
    shutdown_result?;

    println!(
        "IPC smoke test passed: service_version={}, core_state={:?}, logs={}",
        version, core_state, log_count
    );
    Ok(())
}

async fn ensure_no_service_is_running() -> Result<()> {
    match tokio::time::timeout(CONNECT_TIMEOUT, Client::service_default().status()).await {
        Ok(Ok(_)) => bail!(
            "another nyanpasu-service is already reachable; stop it before running the smoke test"
        ),
        Ok(Err(error)) if is_endpoint_not_ready(&error) => Ok(()),
        Ok(Err(error)) => Err(error.into()),
        Err(error) => Err(error).context("timed out probing the global nyanpasu-service endpoint"),
    }
}

async fn wait_for_status(
    child: &mut Child,
) -> Result<nyanpasu_ipc::api::status::StatusResBody<'static>> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        if let Some(status) = child
            .try_wait()
            .context("failed to inspect nyanpasu-service")?
        {
            bail!("nyanpasu-service exited before IPC was ready: {status}");
        }

        match tokio::time::timeout(CONNECT_TIMEOUT, Client::service_default().status()).await {
            Ok(Ok(status)) => return Ok(status),
            Ok(Err(error)) if is_endpoint_not_ready(&error) => {}
            Ok(Err(error)) => return Err(error.into()),
            Err(error) => {
                return Err(error).context("nyanpasu-service status request timed out");
            }
        }

        if Instant::now() >= deadline {
            bail!("timed out waiting for nyanpasu-service IPC");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn is_endpoint_not_ready(error: &ClientError<'_>) -> bool {
    // ClientError::Io does not retain whether an I/O failure happened while connecting or while
    // reading a response. Only these endpoint-absence errors are safe to classify as startup races;
    // all other I/O, HTTP, server-response, and decode errors fail the smoke test immediately.
    matches!(
        error,
        ClientError::Io(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            )
    )
}

struct SmokeDirs {
    _root: TempDir,
    config: PathBuf,
    data: PathBuf,
    app: PathBuf,
    output: PathBuf,
}

impl SmokeDirs {
    fn new() -> Result<Self> {
        let root = tempfile::tempdir().context("failed to create smoke-test directory")?;
        let config = root.path().join("config");
        let data = root.path().join("data");
        let app = root.path().join("app");
        let output = root.path().join("nyanpasu-service.log");
        for dir in [&config, &data, &app] {
            fs::create_dir_all(dir)
                .with_context(|| format!("failed to create {}", dir.display()))?;
        }
        Ok(Self {
            _root: root,
            config,
            data,
            app,
            output,
        })
    }
}

fn service_binary() -> Result<PathBuf> {
    if let Some(path) = env::var_os("NYANPASU_SERVICE_SMOKE_BINARY") {
        return existing_binary(path.into());
    }

    let filename = format!(
        "nyanpasu-service-{}{}",
        sidecar_host(),
        env::consts::EXE_SUFFIX
    );
    existing_binary(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tauri/sidecar")
            .join(filename),
    )
}

fn existing_binary(path: PathBuf) -> Result<PathBuf> {
    if !path.is_file() {
        bail!(
            "nyanpasu-service smoke binary not found: {}",
            path.display()
        );
    }
    path.canonicalize()
        .with_context(|| format!("failed to canonicalize {}", path.display()))
}

fn cleanup_global_runtime_files() {
    // The frozen daemon does not derive these paths from --nyanpasu-data-dir: service.pid lives in
    // suggest_service_data_dir("nyanpasu-service"), and IPC is the fixed nyanpasu_ipc pipe/socket.
    // The smoke process therefore owns global runtime paths while it runs. This is why
    // ensure_no_service_is_running() is mandatory and a local installed service must be stopped.
    let data_dir = nyanpasu_utils::dirs::suggest_service_data_dir("nyanpasu-service");
    let _ = fs::remove_file(data_dir.join("service.pid"));
    #[cfg(unix)]
    let _ = fs::remove_file("/var/run/nyanpasu_ipc.sock");
}

#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
fn sidecar_host() -> &'static str {
    "x86_64-pc-windows-msvc"
}

#[cfg(all(target_arch = "aarch64", target_os = "windows"))]
fn sidecar_host() -> &'static str {
    "aarch64-pc-windows-msvc"
}

#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
fn sidecar_host() -> &'static str {
    "x86_64-apple-darwin"
}

#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
fn sidecar_host() -> &'static str {
    "aarch64-apple-darwin"
}

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
fn sidecar_host() -> &'static str {
    "x86_64-unknown-linux-gnu"
}

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
fn sidecar_host() -> &'static str {
    "aarch64-unknown-linux-gnu"
}

#[cfg(not(any(
    all(target_arch = "x86_64", target_os = "windows"),
    all(target_arch = "aarch64", target_os = "windows"),
    all(target_arch = "x86_64", target_os = "macos"),
    all(target_arch = "aarch64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "linux"),
    all(target_arch = "aarch64", target_os = "linux"),
)))]
compile_error!("the nyanpasu-service smoke test supports only distributed desktop targets");
