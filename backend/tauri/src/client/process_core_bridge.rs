//! S09 test-only process-backed [`CoreLifecyclePort`] using hardened `fake-core`.
//!
//! Linked only under `cfg(test)` (see `client/mod.rs`). Must never call
//! `CoreManager::global()`, `find_binary_path`, real sidecars, user home/config
//! dirs, or production Clash API clients that read global `Config`.

use std::{
    collections::VecDeque,
    io,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use camino::Utf8Path;
use fake_core::{
    FakeCoreCommand, ReadyBarrier, ReadyConnection, ScopedChild, env_keys, require_bin_path,
};
use nyanpasu_config::application::ClashCore;
use nyanpasu_ipc::api::status::CoreState;
use sha2::Digest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{
    core_bridge::{CoreLifecycleLease, CoreLifecyclePort, CoreStatusSnapshot, restore_product},
    runtime::{CandidateFile, RuntimePaths},
};

const READY_OR_EXIT_TIMEOUT: Duration = Duration::from_secs(5);
const CHILD_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);
const APPLY_IO_TIMEOUT: Duration = Duration::from_secs(3);

/// Complete known `FAKE_CORE_*` child-behavior keys. Scrubbed from the inherited
/// parent environment before policy injection so developer/CI shells cannot
/// corrupt process-matrix semantics via stale exits, ports, barriers, or streams.
///
/// Binary selection (`NYANPASU_FAKE_CORE` / [`fake_core::PATH_ENV`]) is intentionally
/// excluded — it selects which binary runs, not how that binary behaves.
const FAKE_CORE_BEHAVIOR_ENV_KEYS: &[&str] = &[
    env_keys::CHECK_EXIT,
    env_keys::CHECK_STDOUT,
    env_keys::CHECK_STDERR,
    env_keys::START_EXIT,
    env_keys::EXIT_AFTER_RELEASE,
    env_keys::START_STDOUT,
    env_keys::START_STDERR,
    env_keys::HOLD_PORT,
    env_keys::READY_ADDR,
    env_keys::HTTP_PORT,
    env_keys::APPLY_STATUS,
    env_keys::APPLY_BODY,
];

/// Remove every known fake-core behavior key from the child command environment
/// before injecting the adapter policy for this operation.
fn scrub_inherited_fake_core_env(cmd: &mut FakeCoreCommand) {
    let command = cmd.command_mut();
    for key in FAKE_CORE_BEHAVIOR_ENV_KEYS {
        command.env_remove(key);
    }
}

/// Failure-injection / port policy private to process-adapter tests.
#[derive(Debug, Clone, Default)]
pub struct ProcessCorePolicy {
    /// When set, check mode exits with this code (`FAKE_CORE_CHECK_EXIT`).
    pub check_exit: Option<u8>,
    /// Optional stderr text for check mode.
    pub check_stderr: Option<String>,
    /// When set (and the per-restart queue is empty), start exits immediately
    /// with this code (`FAKE_CORE_START_EXIT`) instead of long-running mode.
    pub start_exit: Option<u8>,
    /// Per-restart overrides. `Some(code)` → immediate exit; `None` → long-run.
    /// Popped front-to-back; when empty, falls back to [`Self::start_exit`].
    pub start_exit_queue: VecDeque<Option<u8>>,
    /// Optional fixed/ephemeral hold port (`0` = ephemeral).
    pub hold_port: Option<u16>,
    /// Optional HTTP apply port (`0` = ephemeral). Defaults to `Some(0)` on
    /// long-running start so apply can target the announced port.
    pub http_port: Option<u16>,
    /// HTTP status for exact `PUT /configs` (default 204 inside fake-core).
    pub apply_status: Option<u16>,
    /// Exit code after RELEASE (default 0).
    pub exit_after_release: Option<u8>,
}

/// Snapshot of adapter-owned child process identity (test assertions).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessCoreSnapshot {
    pub pid: Option<u32>,
    pub hold_port: Option<u16>,
    pub http_port: Option<u16>,
}

struct ProcessCoreState {
    child: Option<ScopedChild>,
    ready: Option<ReadyConnection>,
    hold_port: Option<u16>,
    http_port: Option<u16>,
    target_core: ClashCore,
    state_changed_at: i64,
}

impl Default for ProcessCoreState {
    fn default() -> Self {
        Self {
            child: None,
            ready: None,
            hold_port: None,
            http_port: None,
            target_core: ClashCore::Mihomo,
            state_changed_at: unix_now(),
        }
    }
}

/// Process-backed lifecycle adapter for S09 failure-matrix tests.
pub struct ProcessCoreLifecycleAdapter {
    bin: PathBuf,
    runtime_paths: RuntimePaths,
    /// `-d` app dir for fake-core argv (TempDir data root — never user dirs).
    app_dir: PathBuf,
    lifecycle: Arc<tokio::sync::Mutex<ProcessCoreState>>,
    policy: Arc<StdMutex<ProcessCorePolicy>>,
    /// Private oneshot fired immediately before the lifecycle mutex is acquired.
    before_begin_lock: StdMutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Private oneshot fired immediately after the lifecycle mutex is acquired.
    after_begin_lock: StdMutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl ProcessCoreLifecycleAdapter {
    /// Resolve `fake-core` via [`require_bin_path`] (preserves prebuild error).
    pub fn try_new(runtime_paths: RuntimePaths, app_dir: PathBuf) -> anyhow::Result<Self> {
        let bin = require_bin_path().map_err(|error| {
            anyhow::anyhow!("ProcessCoreLifecycleAdapter cannot start without fake-core: {error}")
        })?;
        std::fs::create_dir_all(&app_dir)?;
        if let Some(parent) = runtime_paths.product().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::create_dir_all(runtime_paths.candidate_dir())?;
        Ok(Self {
            bin,
            runtime_paths,
            app_dir,
            lifecycle: Arc::new(tokio::sync::Mutex::new(ProcessCoreState::default())),
            policy: Arc::new(StdMutex::new(ProcessCorePolicy::default())),
            before_begin_lock: StdMutex::new(None),
            after_begin_lock: StdMutex::new(None),
        })
    }

    pub fn runtime_paths(&self) -> &RuntimePaths {
        &self.runtime_paths
    }

    pub fn bin_path(&self) -> &Path {
        &self.bin
    }

    pub fn set_policy(&self, policy: ProcessCorePolicy) {
        *self
            .policy
            .lock()
            .expect("process core policy lock must not poison") = policy;
    }

    /// Install private before/after lifecycle-lock oneshots for the next `begin`.
    /// Each sender fires at most once and is consumed by `begin`.
    pub fn arm_begin_lock_hooks(
        &self,
        before_lock: tokio::sync::oneshot::Sender<()>,
        after_lock: tokio::sync::oneshot::Sender<()>,
    ) {
        *self
            .before_begin_lock
            .lock()
            .expect("before_begin_lock must not poison") = Some(before_lock);
        *self
            .after_begin_lock
            .lock()
            .expect("after_begin_lock must not poison") = Some(after_lock);
    }

    pub async fn process_snapshot(&self) -> ProcessCoreSnapshot {
        let state = self.lifecycle.lock().await;
        ProcessCoreSnapshot {
            pid: state.child.as_ref().map(ScopedChild::id),
            hold_port: state.hold_port,
            http_port: state.http_port,
        }
    }
}

struct ProcessCoreLifecycleLease {
    bin: PathBuf,
    runtime_paths: RuntimePaths,
    app_dir: PathBuf,
    policy: Arc<StdMutex<ProcessCorePolicy>>,
    state: tokio::sync::OwnedMutexGuard<ProcessCoreState>,
}

#[async_trait]
impl CoreLifecyclePort for ProcessCoreLifecycleAdapter {
    async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
        if let Some(tx) = self
            .before_begin_lock
            .lock()
            .expect("before_begin_lock must not poison")
            .take()
        {
            let _ = tx.send(());
        }
        let state = self.lifecycle.clone().lock_owned().await;
        if let Some(tx) = self
            .after_begin_lock
            .lock()
            .expect("after_begin_lock must not poison")
            .take()
        {
            let _ = tx.send(());
        }
        Ok(Box::new(ProcessCoreLifecycleLease {
            bin: self.bin.clone(),
            runtime_paths: self.runtime_paths.clone(),
            app_dir: self.app_dir.clone(),
            policy: Arc::clone(&self.policy),
            state,
        }))
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let mut state = self.lifecycle.lock().await;
        reap_if_exited(&mut state);
        let running = state.child.is_some();
        Ok(CoreStatusSnapshot {
            state: if running {
                CoreState::Running
            } else {
                CoreState::Stopped(None)
            },
            state_changed_at: state.state_changed_at,
            // Never call RunType::default() — it reads Config::verge().
            run_type: crate::core::RunType::Normal,
        })
    }

    async fn on_profile_change(&self) {
        // Process adapter has no connection-interruption side effects.
    }
}

#[async_trait]
impl CoreLifecycleLease for ProcessCoreLifecycleLease {
    async fn check_and_promote(
        &mut self,
        candidate: &CandidateFile,
        target_core: ClashCore,
        product: &Utf8Path,
    ) -> anyhow::Result<[u8; 32]> {
        anyhow::ensure!(
            product == self.runtime_paths.product(),
            "product path must match the lifecycle adapter runtime product"
        );
        let bytes = tokio::fs::read(candidate.path()).await?;
        let candidate_hash: [u8; 32] = sha2::Sha256::digest(&bytes).into();
        anyhow::ensure!(
            candidate_hash == candidate.bytes_sha256(),
            "candidate config hash changed before check"
        );

        self.run_check(candidate.path()).await?;

        let after = tokio::fs::read(candidate.path().as_std_path()).await?;
        if after != bytes {
            anyhow::bail!("candidate config changed between check and promote");
        }
        let after_hash: [u8; 32] = sha2::Sha256::digest(&after).into();
        if after_hash != candidate.bytes_sha256() {
            anyhow::bail!("candidate config hash changed before promotion");
        }

        restore_product(product.as_std_path(), &bytes).await?;
        let promoted = tokio::fs::read(product.as_std_path()).await?;
        let promoted_hash: [u8; 32] = sha2::Sha256::digest(&promoted).into();
        if promoted_hash != candidate.bytes_sha256() {
            anyhow::bail!("promoted runtime product hash does not match candidate");
        }
        self.state.target_core = target_core;
        Ok(promoted_hash)
    }

    async fn apply_candidate(
        &mut self,
        candidate: &CandidateFile,
        target_core: ClashCore,
    ) -> anyhow::Result<()> {
        let bytes = tokio::fs::read(candidate.path()).await?;
        let candidate_hash: [u8; 32] = sha2::Sha256::digest(&bytes).into();
        anyhow::ensure!(
            candidate_hash == candidate.bytes_sha256(),
            "candidate config hash changed before check"
        );
        self.run_check(candidate.path()).await?;
        let after = tokio::fs::read(candidate.path()).await?;
        anyhow::ensure!(
            after == bytes,
            "candidate config changed between check and apply"
        );
        self.state.target_core = target_core;
        self.put_configs(candidate.path().as_str()).await
    }

    async fn apply_promoted(&mut self, product: &Utf8Path) -> anyhow::Result<()> {
        anyhow::ensure!(
            product == self.runtime_paths.product(),
            "product path must match the lifecycle adapter runtime product"
        );
        self.put_configs(product.as_str()).await
    }

    async fn restart(&mut self) -> anyhow::Result<()> {
        self.stop_inner().await?;

        let product = self.runtime_paths.product().to_owned();
        // Never invent a placeholder product — promote/seed must write real bytes.
        if !product.as_std_path().is_file() {
            anyhow::bail!("runtime product is missing; cannot restart");
        }

        // Atomic snapshot: one lock covers start-mode pop + ports/apply fields so
        // concurrent set_policy cannot pair start mode from A with ports from B.
        let (start_exit, policy) = self.take_restart_policy();

        if let Some(code) = start_exit {
            return self.spawn_immediate_start_exit(&product, code).await;
        }

        self.spawn_long_running(&product, &policy).await
    }

    async fn stop(&mut self) -> anyhow::Result<()> {
        self.stop_inner().await
    }
}

impl ProcessCoreLifecycleLease {
    /// Snapshot policy under a single lock. Pops `start_exit_queue` when non-empty.
    fn take_restart_policy(&self) -> (Option<u8>, ProcessCorePolicy) {
        let mut policy = self
            .policy
            .lock()
            .expect("process core policy lock must not poison");
        let start_exit = if !policy.start_exit_queue.is_empty() {
            policy.start_exit_queue.pop_front().flatten()
        } else {
            policy.start_exit
        };
        let snapshot = policy.clone();
        (start_exit, snapshot)
    }

    fn policy_clone(&self) -> ProcessCorePolicy {
        self.policy
            .lock()
            .expect("process core policy lock must not poison")
            .clone()
    }

    /// Reap exited children under the lease. Returns a stable error when the core
    /// is not running so apply never targets a stale/reused HTTP port.
    fn ensure_core_running(&mut self) -> anyhow::Result<()> {
        reap_if_exited(&mut self.state);
        if self.state.child.is_none() || self.state.http_port.is_none() {
            clear_running(&mut self.state);
            anyhow::bail!("core not running");
        }
        Ok(())
    }

    async fn run_check(&self, config: &Utf8Path) -> anyhow::Result<()> {
        let policy = self.policy_clone();
        let bin = self.bin.clone();
        let app_dir = self.app_dir.clone();
        let config = config.as_std_path().to_path_buf();
        let output = tokio::task::spawn_blocking(move || {
            let mut cmd = FakeCoreCommand::new(&bin)
                .check(&app_dir, &config)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            scrub_inherited_fake_core_env(&mut cmd);
            if let Some(code) = policy.check_exit {
                cmd = cmd.env(env_keys::CHECK_EXIT, code.to_string());
            }
            if let Some(stderr) = policy.check_stderr.as_ref() {
                cmd = cmd.env(env_keys::CHECK_STDERR, stderr);
            }
            cmd.output()
        })
        .await
        .map_err(|error| anyhow::anyhow!("check join failed: {error}"))?
        .map_err(|error| anyhow::anyhow!("failed to spawn fake-core check: {error}"))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            anyhow::bail!(
                "fake-core check failed (status={}): stderr={stderr} stdout={stdout}",
                output.status
            )
        }
    }

    async fn put_configs(&mut self, config_path: &str) -> anyhow::Result<()> {
        self.ensure_core_running()?;
        let http_port = self
            .state
            .http_port
            .expect("http_port present after ensure_core_running");
        match put_configs_direct(http_port, config_path).await {
            Ok(()) => Ok(()),
            Err(error) => {
                // Child may die between ensure and I/O completion. Re-reap under
                // the lease and never leave callers talking to a stale port.
                reap_if_exited(&mut self.state);
                if self.state.child.is_none() {
                    clear_running(&mut self.state);
                    anyhow::bail!("core not running");
                }
                Err(error)
            }
        }
    }

    async fn spawn_immediate_start_exit(
        &mut self,
        product: &Utf8Path,
        code: u8,
    ) -> anyhow::Result<()> {
        let bin = self.bin.clone();
        let app_dir = self.app_dir.clone();
        let product = product.as_std_path().to_path_buf();
        let target = self.state.target_core;
        let status = tokio::task::spawn_blocking(move || {
            let mut cmd = start_command(&bin, &app_dir, &product, target);
            scrub_inherited_fake_core_env(&mut cmd);
            let mut child = cmd
                .env(env_keys::START_EXIT, code.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn_scoped()?;
            let status = child.wait_with_timeout(CHILD_DRAIN_TIMEOUT)?;
            Ok::<_, io::Error>(status)
        })
        .await
        .map_err(|error| anyhow::anyhow!("start-exit join failed: {error}"))?
        .map_err(|error| anyhow::anyhow!("fake-core immediate start failed: {error}"))?;

        // Child fully reaped inside spawn_blocking; nothing stored → no leak.
        clear_running(&mut self.state);
        anyhow::bail!("fake-core start failed with immediate exit status {status}")
    }

    async fn spawn_long_running(
        &mut self,
        product: &Utf8Path,
        policy: &ProcessCorePolicy,
    ) -> anyhow::Result<()> {
        let barrier = ReadyBarrier::bind_local()
            .map_err(|error| anyhow::anyhow!("failed to bind READY barrier: {error}"))?;
        let ready_addr = barrier.addr_string();

        let bin = self.bin.clone();
        let app_dir = self.app_dir.clone();
        let product_path = product.as_std_path().to_path_buf();
        let target = self.state.target_core;
        let hold_port = policy.hold_port;
        // Always expose HTTP for apply; 0 = ephemeral when unset.
        let http_port = policy.http_port.or(Some(0));
        let apply_status = policy.apply_status;
        let exit_after_release = policy.exit_after_release;

        let mut child = tokio::task::spawn_blocking(move || {
            let mut cmd = start_command(&bin, &app_dir, &product_path, target);
            scrub_inherited_fake_core_env(&mut cmd);
            cmd = cmd
                .env(env_keys::READY_ADDR, ready_addr)
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            if let Some(port) = hold_port {
                cmd = cmd.env(env_keys::HOLD_PORT, port.to_string());
            }
            if let Some(port) = http_port {
                cmd = cmd.env(env_keys::HTTP_PORT, port.to_string());
            }
            if let Some(status) = apply_status {
                cmd = cmd.env(env_keys::APPLY_STATUS, status.to_string());
            }
            if let Some(code) = exit_after_release {
                cmd = cmd.env(env_keys::EXIT_AFTER_RELEASE, code.to_string());
            }
            cmd.spawn_scoped()
        })
        .await
        .map_err(|error| anyhow::anyhow!("start spawn join failed: {error}"))?
        .map_err(|error| anyhow::anyhow!("failed to spawn fake-core: {error}"))?;

        // Barrier wait + child poll run on a blocking thread so the async
        // runtime is not stalled by the READY poll interval.
        let ready_result = tokio::task::spawn_blocking(move || {
            accept_ready_or_child_exit(barrier, &mut child, READY_OR_EXIT_TIMEOUT)
                .map(|ready| (ready, child))
        })
        .await
        .map_err(|error| anyhow::anyhow!("READY wait join failed: {error}"))?;

        let (ready, child) = match ready_result {
            Ok(pair) => pair,
            Err(error) => {
                clear_running(&mut self.state);
                return Err(error);
            }
        };

        self.state.hold_port = ready.announcement.hold_port;
        self.state.http_port = ready.announcement.http_port;
        self.state.ready = Some(ready);
        self.state.child = Some(child);
        self.state.state_changed_at = unix_now();
        Ok(())
    }

    async fn stop_inner(&mut self) -> anyhow::Result<()> {
        if let Some(ready) = self.state.ready.take() {
            // Best-effort clean release; kill path still runs below.
            let _ = ReadyBarrier::release(ready.stream);
        }

        if let Some(mut child) = self.state.child.take() {
            let wait_result = tokio::task::spawn_blocking(move || {
                match child.wait_with_timeout(CHILD_DRAIN_TIMEOUT) {
                    Ok(status) => Ok::<_, io::Error>((child, status)),
                    Err(_) => {
                        let _ = child.kill();
                        let status = child.wait()?;
                        Ok((child, status))
                    }
                }
            })
            .await
            .map_err(|error| anyhow::anyhow!("stop join failed: {error}"))?;

            // Drop ScopedChild after wait — already exited, Drop is a no-op.
            let _ =
                wait_result.map_err(|error| anyhow::anyhow!("fake-core stop failed: {error}"))?;
        }

        self.state.hold_port = None;
        self.state.http_port = None;
        self.state.state_changed_at = unix_now();
        Ok(())
    }
}

fn start_command(bin: &Path, app_dir: &Path, config: &Path, core: ClashCore) -> FakeCoreCommand {
    match core {
        ClashCore::ClashRs | ClashCore::ClashRsAlpha => {
            FakeCoreCommand::new(bin).start_clash_rs(app_dir, config)
        }
        ClashCore::ClashPremium => FakeCoreCommand::new(bin).start_premium(app_dir, config),
        ClashCore::Mihomo | ClashCore::MihomoAlpha | ClashCore::Meow => {
            FakeCoreCommand::new(bin).start_mihomo(app_dir, config)
        }
    }
}

fn clear_running(state: &mut ProcessCoreState) {
    state.child = None;
    state.ready = None;
    state.hold_port = None;
    state.http_port = None;
    state.state_changed_at = unix_now();
}

fn reap_if_exited(state: &mut ProcessCoreState) {
    let exited = match state.child.as_mut() {
        Some(child) => matches!(child.try_wait(), Ok(Some(_))),
        None => false,
    };
    if exited {
        clear_running(state);
    }
}

/// Race READY accept against early child exit. The short sleep is only a poll
/// interval for `try_wait` / channel — ordering is barrier-based.
fn accept_ready_or_child_exit(
    barrier: ReadyBarrier,
    child: &mut ScopedChild,
    timeout: Duration,
) -> anyhow::Result<ReadyConnection> {
    let (tx, rx) = std::sync::mpsc::channel::<io::Result<ReadyConnection>>();
    std::thread::spawn(move || {
        let _ = tx.send(barrier.accept_ready(timeout));
    });

    let deadline = Instant::now() + timeout;
    loop {
        match rx.try_recv() {
            Ok(Ok(ready)) => return Ok(ready),
            Ok(Err(error)) => {
                if let Some(status) = child.try_wait().ok().flatten() {
                    anyhow::bail!(
                        "fake-core exited before READY (status={status}); barrier error: {error}"
                    );
                }
                anyhow::bail!("READY barrier failed: {error}");
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                anyhow::bail!("READY acceptor thread disconnected");
            }
        }

        if let Some(status) = child
            .try_wait()
            .map_err(|error| anyhow::anyhow!("try_wait failed: {error}"))?
        {
            anyhow::bail!("fake-core exited before READY with status {status}");
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("timed out waiting for fake-core READY");
        }

        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Exact HTTP PUT /configs against the adapter-owned fake-core port.
/// Does not use production Clash API clients.
async fn put_configs_direct(http_port: u16, config_path: &str) -> anyhow::Result<()> {
    let body = serde_json::json!({ "path": config_path }).to_string();
    let request = format!(
        "PUT /configs HTTP/1.1\r\n\
         Host: 127.0.0.1:{http_port}\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );

    let connect = tokio::time::timeout(
        APPLY_IO_TIMEOUT,
        tokio::net::TcpStream::connect(("127.0.0.1", http_port)),
    )
    .await
    .map_err(|_| anyhow::anyhow!("connect fake-core http://127.0.0.1:{http_port} timed out"))?
    .map_err(|error| anyhow::anyhow!("connect fake-core http://127.0.0.1:{http_port}: {error}"))?;
    let mut stream = connect;

    tokio::time::timeout(APPLY_IO_TIMEOUT, stream.write_all(request.as_bytes()))
        .await
        .map_err(|_| anyhow::anyhow!("write apply request timed out"))?
        .map_err(|error| anyhow::anyhow!("write apply request: {error}"))?;

    let mut response = Vec::new();
    tokio::time::timeout(APPLY_IO_TIMEOUT, stream.read_to_end(&mut response))
        .await
        .map_err(|_| anyhow::anyhow!("read apply response timed out"))?
        .map_err(|error| anyhow::anyhow!("read apply response: {error}"))?;

    let text = String::from_utf8_lossy(&response);
    let status = parse_http_status(&text)?;
    if status >= 400 {
        anyhow::bail!("fake-core apply returned HTTP {status}");
    }
    Ok(())
}

/// Require a real HTTP status line (`HTTP/x.y <code> ...`). Rejects bare codes
/// or non-HTTP first tokens so apply never mis-parses garbage as success.
fn parse_http_status(response: &str) -> anyhow::Result<u16> {
    let first = response
        .lines()
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid HTTP response: empty body"))?;
    let mut parts = first.split_whitespace();
    let version = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid HTTP response: missing status line"))?;
    if !version.starts_with("HTTP/") {
        anyhow::bail!("invalid HTTP response: status line must start with HTTP/: {first:?}");
    }
    let code = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("invalid HTTP response: missing status code: {first:?}"))?;
    code.parse::<u16>().map_err(|_| {
        anyhow::anyhow!("invalid HTTP response: non-numeric status code {code:?} in {first:?}")
    })
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

mod tests {
    use super::*;
    use crate::client::{NyanpasuClient, tests::test_client_args_with_lifecycle};
    use tempfile::TempDir;
    use tokio::sync::{Notify, oneshot};

    struct ProcessTestEnv {
        _dir: TempDir,
        adapter: Arc<ProcessCoreLifecycleAdapter>,
    }

    impl ProcessTestEnv {
        fn new() -> anyhow::Result<Self> {
            let dir = tempfile::tempdir()?;
            let paths = crate::utils::path::PathResolver::with_base_dirs(
                dir.path().into(),
                dir.path().join("data"),
            );
            let runtime_paths = RuntimePaths::from_resolver(&paths)?;
            let adapter = Arc::new(ProcessCoreLifecycleAdapter::try_new(
                runtime_paths,
                paths.app_data_dir().to_path_buf(),
            )?);
            Ok(Self { _dir: dir, adapter })
        }

        fn dir(&self) -> &TempDir {
            &self._dir
        }

        fn client(&self) -> anyhow::Result<NyanpasuClient> {
            let mut args =
                test_client_args_with_lifecycle(self.dir(), self.adapter.clone() as Arc<_>);
            args.runtime_paths = self.adapter.runtime_paths().clone();
            args.paths = crate::utils::path::PathResolver::with_base_dirs(
                self.dir().path().into(),
                self.dir().path().join("data"),
            );
            NyanpasuClient::try_new_with_args(args)
        }
    }

    async fn seed_product(adapter: &ProcessCoreLifecycleAdapter, bytes: &[u8]) {
        let product = adapter.runtime_paths().product();
        if let Some(parent) = product.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(product.as_std_path(), bytes)
            .await
            .unwrap();
    }

    fn process_exists(pid: u32) -> bool {
        #[cfg(target_os = "linux")]
        {
            std::path::Path::new(&format!("/proc/{pid}")).exists()
        }
        #[cfg(all(unix, not(target_os = "linux")))]
        {
            std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
            false
        }
    }

    // try_new_with_args uses tauri::async_runtime::block_on internally, so
    // construct the client outside any outer block_on / #[tokio::test].
    #[test]
    fn s09_process_check_fail_leaves_product_promoted_applied_unchanged() {
        let env = ProcessTestEnv::new().expect("process env");
        const OLD: &[u8] = b"# s09-old\nmode: rule\n";
        tauri::async_runtime::block_on(seed_product(&env.adapter, OLD));

        env.adapter.set_policy(ProcessCorePolicy {
            http_port: Some(0),
            apply_status: Some(204),
            ..Default::default()
        });
        let client = env.client().expect("client");

        tauri::async_runtime::block_on(async {
            let promoted = client
                .promote_existing_runtime_product()
                .await
                .expect("seed promote");
            client
                .start_promoted_runtime()
                .await
                .expect("seed apply/start");
            let before = client.runtime_lifecycle_state().await;
            assert!(before.promoted.is_some());
            assert!(before.applied.is_some());
            assert!(
                before
                    .promoted
                    .as_ref()
                    .unwrap()
                    .identity_eq(promoted.as_ref())
            );

            env.adapter.set_policy(ProcessCorePolicy {
                check_exit: Some(1),
                check_stderr: Some("injected check failure".into()),
                ..Default::default()
            });
            let err = client
                .regenerate_runtime()
                .await
                .expect_err("check failure must surface");
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("check failed") || rendered.contains("fake-core"),
                "unexpected error: {rendered}"
            );

            let product_after = tokio::fs::read(env.adapter.runtime_paths().product())
                .await
                .unwrap();
            assert_eq!(
                product_after, OLD,
                "product must stay unchanged on check fail"
            );

            let after = client.runtime_lifecycle_state().await;
            assert!(
                after
                    .promoted
                    .as_ref()
                    .unwrap()
                    .identity_eq(before.promoted.as_ref().unwrap()),
                "Promoted must stay unchanged on check fail"
            );
            assert!(
                after
                    .applied
                    .as_ref()
                    .unwrap()
                    .identity_eq(before.applied.as_ref().unwrap()),
                "Applied must stay unchanged on check fail"
            );

            let mut lease = env.adapter.begin().await.unwrap();
            lease.stop().await.unwrap();
            client.shutdown().await;
        });
    }

    #[tokio::test]
    async fn s09_process_restart_immediate_failure_observed_no_child_leak() {
        let env = ProcessTestEnv::new().expect("process env");
        seed_product(&env.adapter, b"mode: rule\n").await;

        env.adapter.set_policy(ProcessCorePolicy {
            start_exit: Some(7),
            ..Default::default()
        });

        let mut lease = env.adapter.begin().await.unwrap();
        let err = lease
            .restart()
            .await
            .expect_err("START_EXIT must fail restart");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("immediate exit") || msg.contains("start failed"),
            "unexpected error: {msg}"
        );
        drop(lease);

        let snap = env.adapter.process_snapshot().await;
        assert!(snap.pid.is_none(), "failed start must not retain a child");
        assert!(snap.hold_port.is_none());
        assert!(snap.http_port.is_none());

        let status = env.adapter.status().await.unwrap();
        assert!(matches!(status.state, CoreState::Stopped(_)));
    }

    #[test]
    fn s09_process_apply_500_after_promote_leaves_promoted_new_applied_old_or_none() {
        let env = ProcessTestEnv::new().expect("process env");
        const OLD: &[u8] = b"# s09-applied-old\nmode: direct\n";
        tauri::async_runtime::block_on(seed_product(&env.adapter, OLD));

        env.adapter.set_policy(ProcessCorePolicy {
            http_port: Some(0),
            apply_status: Some(204),
            ..Default::default()
        });
        let client = env.client().expect("client");

        tauri::async_runtime::block_on(async {
            let old_snap = client
                .promote_existing_runtime_product()
                .await
                .expect("promote old");
            client
                .start_promoted_runtime()
                .await
                .expect("start old applied");
            let before = client.runtime_lifecycle_state().await;
            assert!(
                before
                    .applied
                    .as_ref()
                    .unwrap()
                    .identity_eq(old_snap.as_ref())
            );

            {
                let mut lease = env.adapter.begin().await.unwrap();
                lease.stop().await.unwrap();
            }
            env.adapter.set_policy(ProcessCorePolicy {
                http_port: Some(0),
                apply_status: Some(500),
                ..Default::default()
            });
            {
                let mut lease = env.adapter.begin().await.unwrap();
                lease.restart().await.expect("restart with apply=500");
            }

            let err = client
                .rebuild_running_config()
                .await
                .expect_err("apply 500 must fail rebuild");
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("500") || rendered.contains("apply"),
                "unexpected error: {rendered}"
            );

            let after = client.runtime_lifecycle_state().await;
            let promoted = after
                .promoted
                .expect("Promoted must advance after successful check/promote");
            assert!(
                !promoted.identity_eq(old_snap.as_ref()),
                "Promoted must be the new snapshot after promote-ok/apply-fail"
            );
            let applied = after.applied.expect("Applied was seeded");
            assert!(
                applied.identity_eq(old_snap.as_ref()),
                "Applied must remain the pre-apply snapshot on apply failure"
            );

            let mut lease = env.adapter.begin().await.unwrap();
            lease.stop().await.unwrap();
            client.shutdown().await;
        });
    }

    #[tokio::test]
    async fn s09_process_fixed_port_hold_conflict_and_frees_after_stop() {
        let env_a = ProcessTestEnv::new().expect("env a");
        let env_b = ProcessTestEnv::new().expect("env b");
        seed_product(&env_a.adapter, b"mode: rule\n").await;
        seed_product(&env_b.adapter, b"mode: rule\n").await;

        // Holder binds hold_port=0 (ephemeral). Challenger targets the
        // READY-announced port while A still holds it — no TOCTOU via
        // pre-selected free ephemeral ports that can be released/rebound.
        env_a.adapter.set_policy(ProcessCorePolicy {
            hold_port: Some(0),
            http_port: Some(0),
            ..Default::default()
        });
        {
            let mut lease = env_a.adapter.begin().await.unwrap();
            lease.restart().await.expect("A starts with ephemeral hold");
        }
        let snap_a = env_a.adapter.process_snapshot().await;
        let fixed = snap_a.hold_port.expect("A must announce held port");
        assert_ne!(fixed, 0, "READY must announce a concrete hold port");

        env_b.adapter.set_policy(ProcessCorePolicy {
            hold_port: Some(fixed),
            http_port: Some(0),
            ..Default::default()
        });
        {
            let mut lease = env_b.adapter.begin().await.unwrap();
            let err = lease
                .restart()
                .await
                .expect_err("B must fail while A holds the port");
            let msg = format!("{err:#}");
            assert!(
                msg.contains("exited before READY")
                    || msg.contains("hold")
                    || msg.contains("READY"),
                "unexpected conflict error: {msg}"
            );
        }
        assert!(
            env_b.adapter.process_snapshot().await.pid.is_none(),
            "B must not retain a child after bind conflict"
        );

        {
            let mut lease = env_a.adapter.begin().await.unwrap();
            lease.stop().await.unwrap();
        }
        let listener = std::net::TcpListener::bind(("127.0.0.1", fixed))
            .expect("hold port must free after stop");
        drop(listener);

        {
            let mut lease = env_b.adapter.begin().await.unwrap();
            lease
                .restart()
                .await
                .expect("B starts after A released port");
            lease.stop().await.unwrap();
        }
    }

    #[tokio::test]
    async fn s09_process_lifecycle_lease_serialization_with_barrier() {
        let env = ProcessTestEnv::new().expect("process env");
        let adapter = env.adapter.clone();

        let (entered_tx, entered_rx) = oneshot::channel::<()>();
        let release = Arc::new(Notify::new());
        let release_wait = release.clone();

        let first = tokio::spawn({
            let adapter = adapter.clone();
            async move {
                let _lease = adapter.begin().await.expect("first begin");
                let _ = entered_tx.send(());
                release_wait.notified().await;
            }
        });

        entered_rx.await.expect("first lease entered");

        let (before_tx, before_rx) = oneshot::channel::<()>();
        let (after_tx, mut after_rx) = oneshot::channel::<()>();
        adapter.arm_begin_lock_hooks(before_tx, after_tx);

        let second = tokio::spawn({
            let adapter = adapter.clone();
            async move {
                let _lease = adapter.begin().await.expect("second begin");
            }
        });

        // Second task reached begin and fired before-lock; still blocked on mutex.
        before_rx.await.expect("second begin reached before-lock");
        match after_rx.try_recv() {
            Err(oneshot::error::TryRecvError::Empty) => {}
            Ok(()) => panic!("after-lock must not fire while first lease is held"),
            Err(oneshot::error::TryRecvError::Closed) => {
                panic!("after-lock sender dropped before first lease released")
            }
        }

        release.notify_one();
        first.await.expect("first task");
        after_rx
            .await
            .expect("after-lock must fire once first lease drops");
        second.await.expect("second task");
    }

    #[tokio::test]
    async fn s09_process_clean_stop_scoped_child_cleanup() {
        let env = ProcessTestEnv::new().expect("process env");
        seed_product(&env.adapter, b"mode: rule\n").await;
        env.adapter.set_policy(ProcessCorePolicy {
            hold_port: Some(0),
            http_port: Some(0),
            ..Default::default()
        });

        {
            let mut lease = env.adapter.begin().await.unwrap();
            lease.restart().await.expect("start long-running");
        }
        let running = env.adapter.process_snapshot().await;
        assert!(running.pid.is_some(), "child must be running before stop");
        let hold = running.hold_port.expect("hold port announced");
        let http = running.http_port.expect("http port announced");
        let pid = running.pid.unwrap();
        assert_ne!(hold, 0);
        assert_ne!(http, 0);

        {
            let mut lease = env.adapter.begin().await.unwrap();
            lease.stop().await.expect("clean stop");
        }

        let after = env.adapter.process_snapshot().await;
        assert!(after.pid.is_none());
        assert!(after.hold_port.is_none());
        assert!(after.http_port.is_none());

        let _ =
            std::net::TcpListener::bind(("127.0.0.1", hold)).expect("hold port free after stop");
        let _ =
            std::net::TcpListener::bind(("127.0.0.1", http)).expect("http port free after stop");
        assert!(!process_exists(pid), "pid {pid} must not remain after stop");
    }

    #[test]
    fn s09_process_two_client_graphs_isolated_pids_ports_paths() {
        let env_a = ProcessTestEnv::new().expect("env a");
        let env_b = ProcessTestEnv::new().expect("env b");
        tauri::async_runtime::block_on(async {
            seed_product(&env_a.adapter, b"# graph-a\nmode: rule\n").await;
            seed_product(&env_b.adapter, b"# graph-b\nmode: direct\n").await;
        });

        for env in [&env_a, &env_b] {
            env.adapter.set_policy(ProcessCorePolicy {
                hold_port: Some(0),
                http_port: Some(0),
                apply_status: Some(204),
                ..Default::default()
            });
        }

        let client_a = env_a.client().expect("client a");
        let client_b = env_b.client().expect("client b");

        tauri::async_runtime::block_on(async {
            client_a
                .promote_existing_runtime_product()
                .await
                .expect("promote a");
            client_b
                .promote_existing_runtime_product()
                .await
                .expect("promote b");
            client_a.start_promoted_runtime().await.expect("start a");
            client_b.start_promoted_runtime().await.expect("start b");

            let snap_a = env_a.adapter.process_snapshot().await;
            let snap_b = env_b.adapter.process_snapshot().await;
            assert!(snap_a.pid.is_some() && snap_b.pid.is_some());
            assert_ne!(snap_a.pid, snap_b.pid, "graphs must own distinct PIDs");
            assert_ne!(
                snap_a.http_port, snap_b.http_port,
                "graphs must own distinct HTTP ports"
            );
            assert_ne!(
                env_a.adapter.runtime_paths().product(),
                env_b.adapter.runtime_paths().product(),
                "graphs must use distinct RuntimePaths products"
            );

            {
                let mut lease = env_a.adapter.begin().await.unwrap();
                lease.stop().await.unwrap();
            }
            assert!(env_a.adapter.process_snapshot().await.pid.is_none());

            {
                let mut lease = env_b.adapter.begin().await.unwrap();
                lease
                    .apply_promoted(env_b.adapter.runtime_paths().product())
                    .await
                    .expect("B apply still works after A stop");
                lease.stop().await.unwrap();
            }

            client_a.shutdown().await;
            client_b.shutdown().await;
        });
    }

    /// Process-level `change_core` failure + successful old-core rollback.
    ///
    /// Uses a start-exit queue so new-core restart fails and rollback old-core
    /// restart long-runs. Touches legacy Config globals via `change_core`
    /// (PR-5 residual); run under `--test-threads=1` with the rest of the
    /// process matrix. Does not re-enumerate every mock branch.
    #[test]
    fn s09_process_change_core_new_start_exit_rollback_old_restart_succeeds() {
        let env = ProcessTestEnv::new().expect("process env");
        // Seed a real product so restart never invents a placeholder.
        tauri::async_runtime::block_on(seed_product(
            &env.adapter,
            b"# s09-change-core-seed\nmode: rule\n",
        ));

        let mut start_exit_queue = VecDeque::new();
        // New-core restart fails immediately; rollback old-core restart runs.
        start_exit_queue.push_back(Some(3));
        start_exit_queue.push_back(None);
        env.adapter.set_policy(ProcessCorePolicy {
            start_exit_queue,
            hold_port: Some(0),
            http_port: Some(0),
            apply_status: Some(204),
            ..Default::default()
        });
        let client = env.client().expect("client");

        tauri::async_runtime::block_on(async {
            // Explicit promote so product/promoted are established before switch.
            let seeded = client
                .promote_existing_runtime_product()
                .await
                .expect("seed promote");
            assert!(
                env.adapter
                    .runtime_paths()
                    .product()
                    .as_std_path()
                    .is_file(),
                "product must exist after promote"
            );

            let selected_before = crate::config::Config::verge()
                .latest()
                .clash_core
                .unwrap_or_default();
            assert_ne!(
                selected_before,
                crate::config::nyanpasu::ClashCore::ClashRs,
                "baseline selected core must not already be the target"
            );

            let err = client
                .change_core(crate::config::nyanpasu::ClashCore::ClashRs)
                .await
                .expect_err("new-core start-exit must surface");
            let rendered = format!("{err:?}");
            assert!(
                rendered.contains("start failed")
                    || rendered.contains("immediate exit")
                    || rendered.contains("exit"),
                "unexpected change_core error: {rendered}"
            );

            let after = client.runtime_lifecycle_state().await;
            let promoted = after
                .promoted
                .expect("rollback must leave Promoted published");
            let applied = after
                .applied
                .expect("successful old restart must publish Applied");
            assert!(
                promoted.identity_eq(&applied),
                "Applied must match Promoted after successful rollback restart"
            );
            assert_ne!(
                promoted.target_core,
                ClashCore::ClashRs,
                "rollback rebuild must target the restored old selected core"
            );
            // Seeded promote may be superseded by the rollback rebuild product;
            // product bytes must still exist and selected core restored.
            assert!(
                env.adapter
                    .runtime_paths()
                    .product()
                    .as_std_path()
                    .is_file(),
                "product must remain after rollback"
            );
            let selected_after = crate::config::Config::verge()
                .latest()
                .clash_core
                .unwrap_or_default();
            assert_eq!(
                selected_after, selected_before,
                "selected core must restore via draft discard"
            );
            let _ = seeded;

            let snap = env.adapter.process_snapshot().await;
            assert!(
                snap.pid.is_some(),
                "old-core child must be running after rollback restart"
            );
            assert!(
                snap.hold_port.is_some(),
                "hold port announced after rollback"
            );
            assert!(
                snap.http_port.is_some(),
                "http port announced after rollback"
            );

            {
                let mut lease = env.adapter.begin().await.unwrap();
                lease.stop().await.expect("cleanup stop");
            }
            let cleaned = env.adapter.process_snapshot().await;
            assert!(cleaned.pid.is_none(), "child cleaned after stop");
            assert!(cleaned.hold_port.is_none());
            assert!(cleaned.http_port.is_none());

            client.shutdown().await;
        });
    }

    #[tokio::test]
    async fn s09_process_restart_fails_when_product_missing() {
        let env = ProcessTestEnv::new().expect("process env");
        // Intentionally do not seed/promote product.
        let product = env
            .adapter
            .runtime_paths()
            .product()
            .as_std_path()
            .to_path_buf();
        if product.exists() {
            let _ = tokio::fs::remove_file(&product).await;
        }

        let mut lease = env.adapter.begin().await.unwrap();
        let err = lease
            .restart()
            .await
            .expect_err("missing product must fail restart");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("runtime product is missing"),
            "unexpected missing-product error: {msg}"
        );
        drop(lease);
        assert!(
            env.adapter.process_snapshot().await.pid.is_none(),
            "no child after missing-product restart"
        );
    }

    #[tokio::test]
    async fn s09_process_apply_after_external_exit_returns_core_not_running() {
        let env = ProcessTestEnv::new().expect("process env");
        seed_product(&env.adapter, b"mode: rule\n").await;
        env.adapter.set_policy(ProcessCorePolicy {
            hold_port: Some(0),
            http_port: Some(0),
            apply_status: Some(204),
            ..Default::default()
        });

        {
            let mut lease = env.adapter.begin().await.unwrap();
            lease.restart().await.expect("start long-running");
        }
        let snap = env.adapter.process_snapshot().await;
        let pid = snap.pid.expect("child running");
        assert!(snap.http_port.is_some(), "http announced before kill");

        // External kill leaves adapter-owned child/http stale until lease reaps.
        #[cfg(unix)]
        {
            let status = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .status()
                .expect("kill -9");
            assert!(status.success());
        }
        #[cfg(not(unix))]
        {
            // Platform without reliable external kill in this harness.
            // Unix path covers the reap/apply contract.
            let _ = pid;
            return;
        }

        {
            let mut lease = env.adapter.begin().await.unwrap();
            // Apply itself must reap under the lease and return the stable error
            // (no sleep-based wait for process death — I/O failure + try_wait).
            let err = lease
                .apply_promoted(env.adapter.runtime_paths().product())
                .await
                .expect_err("apply must refuse after child exit");
            let msg = format!("{err:#}");
            assert!(
                msg.contains("core not running"),
                "stable error required, got: {msg}"
            );
        }

        let after = env.adapter.process_snapshot().await;
        assert!(after.pid.is_none(), "reap must clear child");
        assert!(after.http_port.is_none(), "reap must clear stale http port");
        assert!(after.hold_port.is_none(), "reap must clear hold port");
    }

    #[test]
    fn s09_process_parse_http_status_requires_http_prefix() {
        assert_eq!(
            parse_http_status("HTTP/1.1 204 No Content").expect("204"),
            204
        );
        assert_eq!(
            parse_http_status("HTTP/1.0 500 Internal Server Error").expect("500"),
            500
        );
        let bare = parse_http_status("200 OK").expect_err("bare code");
        assert!(
            bare.to_string().contains("HTTP/"),
            "error must mention HTTP/ requirement: {bare}"
        );
        let empty = parse_http_status("").expect_err("empty");
        assert!(
            empty.to_string().contains("empty"),
            "error must mention empty: {empty}"
        );
        let bad_code = parse_http_status("HTTP/1.1 xyz Oops").expect_err("non-numeric");
        assert!(
            bad_code.to_string().contains("non-numeric")
                || bad_code.to_string().contains("status code"),
            "error must mention bad code: {bad_code}"
        );
    }

    #[tokio::test]
    async fn s09_process_require_bin_path_prebuild_error_is_preserved() {
        match require_bin_path() {
            Ok(path) => {
                assert!(
                    path.is_file(),
                    "require_bin_path returned non-file {}",
                    path.display()
                );
                let env = ProcessTestEnv::new().expect("adapter constructs when bin exists");
                assert_eq!(env.adapter.bin_path(), path.as_path());
            }
            Err(error) => {
                let msg = error.to_string();
                assert!(
                    msg.contains(fake_core::PREBUILD_COMMAND) || msg.contains("fake-core"),
                    "missing-bin error must mention prebuild: {msg}"
                );
            }
        }
    }

    /// Serializes parent `FAKE_CORE_*` contamination across tests and restores
    /// previous values on drop. Does not touch `NYANPASU_FAKE_CORE`.
    struct ContaminatedFakeCoreEnv {
        _lock: std::sync::MutexGuard<'static, ()>,
        previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    fn parent_fake_core_env_lock() -> &'static StdMutex<()> {
        static LOCK: std::sync::OnceLock<StdMutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| StdMutex::new(()))
    }

    impl ContaminatedFakeCoreEnv {
        fn apply(pairs: &[(&'static str, &str)]) -> Self {
            let lock = parent_fake_core_env_lock()
                .lock()
                .expect("parent fake-core env lock must not poison");
            let mut previous = Vec::with_capacity(pairs.len());
            for &(key, value) in pairs {
                previous.push((key, std::env::var_os(key)));
                // SAFETY: held under process-local mutex; restored in Drop.
                unsafe { std::env::set_var(key, value) };
            }
            Self {
                _lock: lock,
                previous,
            }
        }
    }

    impl Drop for ContaminatedFakeCoreEnv {
        fn drop(&mut self) {
            for (key, previous) in self.previous.drain(..) {
                // SAFETY: same mutex-scoped restoration as apply.
                match previous {
                    Some(value) => unsafe { std::env::set_var(key, value) },
                    None => unsafe { std::env::remove_var(key) },
                }
            }
        }
    }

    // Contaminates the parent process with representative FAKE_CORE_* values that
    // would break check / immediate start / long-running start / apply if inherited.
    // Policy deliberately sets only the values this operation needs; scrub must win.
    #[test]
    fn s09_process_child_env_ignores_parent_fake_core_contamination() {
        let _contaminate = ContaminatedFakeCoreEnv::apply(&[
            (env_keys::CHECK_EXIT, "7"),
            (env_keys::CHECK_STDERR, "parent-check-err"),
            (env_keys::START_EXIT, "9"),
            (env_keys::START_STDERR, "parent-start-err"),
            (env_keys::EXIT_AFTER_RELEASE, "3"),
            (env_keys::HOLD_PORT, "1"),
            (env_keys::HTTP_PORT, "1"),
            (env_keys::READY_ADDR, "127.0.0.1:1"),
            (env_keys::APPLY_STATUS, "500"),
            (env_keys::APPLY_BODY, "parent-apply-fail"),
        ]);

        let env = ProcessTestEnv::new().expect("process env");
        const PRODUCT: &[u8] = b"# s09-env-scrub\nmode: rule\n";
        tauri::async_runtime::block_on(seed_product(&env.adapter, PRODUCT));

        // Policy only: long-running with ephemeral HTTP + successful apply.
        // No check_exit / start_exit — parent contamination must not supply them.
        env.adapter.set_policy(ProcessCorePolicy {
            http_port: Some(0),
            apply_status: Some(204),
            ..Default::default()
        });
        let client = env.client().expect("client");

        tauri::async_runtime::block_on(async {
            client
                .promote_existing_runtime_product()
                .await
                .expect("promote");
            client
                .start_promoted_runtime()
                .await
                .expect("start must ignore parent START_EXIT/READY_ADDR/ports");

            let snap = env.adapter.process_snapshot().await;
            assert!(snap.pid.is_some(), "child must stay running after start");
            assert!(
                snap.http_port.is_some_and(|p| p != 1),
                "http port must come from policy ephemeral bind, not parent HTTP_PORT=1: {snap:?}"
            );

            // regenerate triggers check + apply; parent CHECK_EXIT=7 / APPLY_STATUS=500
            // must not win over scrub + policy defaults / apply_status=204.
            client
                .regenerate_runtime()
                .await
                .expect("regenerate check+apply must ignore parent contamination");

            let status = env.adapter.status().await.expect("adapter status");
            assert!(
                matches!(status.state, CoreState::Running),
                "core must remain running after regenerate: {status:?}"
            );
            let after = env.adapter.process_snapshot().await;
            assert!(after.pid.is_some(), "child must remain after regenerate");
        });
    }
}
