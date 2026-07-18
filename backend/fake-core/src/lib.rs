//! Test-only fake clash core protocol helpers and binary discovery.
//!
//! This crate is never packaged as a production sidecar or resource. It exists so
//! process-lifecycle tests can inject deterministic check/start/apply failures
//! without real cores, OS proxy, or sleep-based ordering.
//!
//! # Binary discovery
//!
//! `CARGO_BIN_EXE_fake-core` is set **only** when compiling integration tests of
//! this package (the package that owns the `fake-core` binary). A
//! `dev-dependency` on `fake-core` does **not** build the binary and does **not**
//! set that env for the dependent package's tests.
//!
//! Discovery order for cross-crate consumers (e.g. tauri process-lifecycle tests):
//!
//! 1. Non-empty runtime override: `NYANPASU_FAKE_CORE` (empty values are ignored)
//! 2. Current test executable relative: if `current_exe` lives under
//!    `.../<profile>/deps` (or the profile dir itself), look for `fake-core` in the
//!    sibling profile directory — this picks up the same target/profile/triple
//!    Cargo used for the running test binary
//! 3. Workspace target fallback: `$CARGO_TARGET_DIR/{debug|release}/fake-core`
//!    (or `../target/...` relative to this crate when `CARGO_TARGET_DIR` is unset)
//!
//! Prebuild is still required for focused package tests that do not build the whole
//! workspace bin graph: `cargo build -p fake-core` (or `cargo test -p fake-core`).
//! Same-package tests should prefer `env!("CARGO_BIN_EXE_fake-core")` directly.
//!
//! # Start modes
//!
//! - `FAKE_CORE_START_EXIT=<code>`: **immediate** termination with that exit code
//!   (after optional start stdout/stderr). This is not a long-running start.
//! - Long-running start **requires** a ready/release control barrier via
//!   `FAKE_CORE_READY_ADDR`. Without `START_EXIT` and without a barrier, the
//!   process fails fast with exit 2 (no infinite park).

use std::{
    env,
    ffi::{OsStr, OsString},
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Output, Stdio},
    sync::{Mutex, MutexGuard, OnceLock},
    time::{Duration, Instant},
};

/// Binary artifact name without path (EXE_SUFFIX applied by discovery).
pub const BIN_NAME: &str = "fake-core";

/// Runtime path override for cross-crate consumers / harnesses.
pub const PATH_ENV: &str = "NYANPASU_FAKE_CORE";

/// Process-wide lock for tests that observe or mutate [`PATH_ENV`].
///
/// libtest may run tests on multiple threads; `set_var` / `remove_var` are not
/// atomic with concurrent readers. Hold this for the full set → resolve/require →
/// restore critical section (or for any read that must not race a mutation).
fn path_env_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_path_env_for_test() -> MutexGuard<'static, ()> {
    path_env_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// RAII lock that serializes concurrent test access to [`PATH_ENV`].
///
/// Use for read-only observations that must not race a concurrent mutation.
#[derive(Debug)]
pub struct PathEnvLock {
    _guard: MutexGuard<'static, ()>,
}

impl PathEnvLock {
    pub fn acquire() -> Self {
        Self {
            _guard: lock_path_env_for_test(),
        }
    }
}

/// RAII guard that sets [`PATH_ENV`] while holding the process-wide test lock.
///
/// Restores the previous value (or absence) on drop. The mutex stays held for the
/// full guard lifetime so set / resolve / restore cannot interleave with other tests.
#[derive(Debug)]
pub struct PathEnvGuard {
    previous: Option<OsString>,
    _guard: MutexGuard<'static, ()>,
}

impl PathEnvGuard {
    /// Set `PATH_ENV` to `value` until the guard is dropped.
    pub fn set(value: impl AsRef<OsStr>) -> Self {
        let guard = lock_path_env_for_test();
        let previous = env::var_os(PATH_ENV);
        // SAFETY: exclusive test lock is held for the full set/resolve/restore window.
        unsafe { env::set_var(PATH_ENV, value) };
        Self {
            previous,
            _guard: guard,
        }
    }

    /// Set `PATH_ENV` to the empty string until the guard is dropped.
    pub fn set_empty() -> Self {
        Self::set("")
    }
}

impl Drop for PathEnvGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            // SAFETY: exclusive test lock still held; restore prior value/absence.
            Some(value) => unsafe { env::set_var(PATH_ENV, value) },
            None => unsafe { env::remove_var(PATH_ENV) },
        }
    }
}

/// Ready frame prefix (line may include `hold=` / `http=` fields).
pub const READY_PREFIX: &str = "READY";
/// Barrier protocol: parent -> child to request clean exit.
pub const RELEASE_LINE: &str = "RELEASE\n";

/// Environment keys that control fake-core behaviour (test-only, stable).
pub mod env_keys {
    pub const CHECK_EXIT: &str = "FAKE_CORE_CHECK_EXIT";
    pub const CHECK_STDOUT: &str = "FAKE_CORE_CHECK_STDOUT";
    pub const CHECK_STDERR: &str = "FAKE_CORE_CHECK_STDERR";
    /// Immediate start termination code. Mutually exclusive with long-running
    /// barrier mode: when set, the process exits before holding ports / READY.
    pub const START_EXIT: &str = "FAKE_CORE_START_EXIT";
    pub const EXIT_AFTER_RELEASE: &str = "FAKE_CORE_EXIT_AFTER_RELEASE";
    pub const START_STDOUT: &str = "FAKE_CORE_START_STDOUT";
    pub const START_STDERR: &str = "FAKE_CORE_START_STDERR";
    /// Optional TCP port to hold. `0` binds an ephemeral port; the actual port is
    /// reported in the READY frame.
    pub const HOLD_PORT: &str = "FAKE_CORE_HOLD_PORT";
    /// Parent control listener `host:port`. Required for long-running start.
    pub const READY_ADDR: &str = "FAKE_CORE_READY_ADDR";
    /// Optional HTTP port for status-injection-only apply endpoint. `0` binds
    /// ephemeral; the actual port is reported in the READY frame.
    pub const HTTP_PORT: &str = "FAKE_CORE_HTTP_PORT";
    pub const APPLY_STATUS: &str = "FAKE_CORE_APPLY_STATUS";
    pub const APPLY_BODY: &str = "FAKE_CORE_APPLY_BODY";
}

/// Stable prebuild command shown when the binary cannot be discovered.
pub const PREBUILD_COMMAND: &str = "cargo build -p fake-core";

/// Resolved run mode from real core argv shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// `fake-core -t -d <app_dir> -f <config>`
    Check { app_dir: PathBuf, config: PathBuf },
    /// Start argv (mihomo / clash-rs / premium shapes).
    Start { app_dir: PathBuf, config: PathBuf },
}

/// Ports announced by the child in the READY frame (actual bound values).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReadyAnnouncement {
    pub hold_port: Option<u16>,
    pub http_port: Option<u16>,
}

impl ReadyAnnouncement {
    pub fn to_line(&self) -> String {
        let mut line = String::from(READY_PREFIX);
        if let Some(port) = self.hold_port {
            line.push_str(&format!(" hold={port}"));
        }
        if let Some(port) = self.http_port {
            line.push_str(&format!(" http={port}"));
        }
        line.push('\n');
        line
    }

    /// Parse a READY line (`READY` or `READY hold=N http=M`).
    pub fn parse_line(line: &str) -> io::Result<Self> {
        let line = line.trim_end_matches(['\r', '\n']);
        let mut parts = line.split_whitespace();
        let head = parts.next().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "empty READY barrier line")
        })?;
        if head != READY_PREFIX {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("expected READY barrier, got {line:?}"),
            ));
        }

        let mut announcement = Self::default();
        for token in parts {
            if let Some(value) = token.strip_prefix("hold=") {
                announcement.hold_port = Some(parse_port_token("hold", value)?);
            } else if let Some(value) = token.strip_prefix("http=") {
                announcement.http_port = Some(parse_port_token("http", value)?);
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unexpected READY field: {token}"),
                ));
            }
        }
        Ok(announcement)
    }
}

fn parse_port_token(name: &str, value: &str) -> io::Result<u16> {
    value.parse::<u16>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid READY {name} port: {value:?}"),
        )
    })
}

/// Result of accepting a child READY control connection.
#[derive(Debug)]
pub struct ReadyConnection {
    pub stream: TcpStream,
    pub announcement: ReadyAnnouncement,
}

/// Parse argv the same way real cores are invoked by `nyanpasu-utils`.
///
/// Check: `-t -d <app_dir> -f <config>`
/// Start (mihomo): `-m -d <app_dir> -f <config>`
/// Start (clash-rs): `-d <app_dir> -c <config>`
/// Start (premium): `-d <app_dir> -f <config>`
pub fn parse_args<I, S>(args: I) -> Result<Mode, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut is_check = false;
    let mut app_dir: Option<PathBuf> = None;
    let mut config: Option<PathBuf> = None;

    let mut iter = args.into_iter();
    // Skip program name when present.
    let first = iter.next();
    let mut rest: Vec<String> = Vec::new();
    if let Some(first) = first {
        let s = first.as_ref();
        if s.starts_with('-') {
            rest.push(s.to_string());
        }
    }
    rest.extend(iter.map(|s| s.as_ref().to_string()));

    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" => {
                is_check = true;
                i += 1;
            }
            "-m" => {
                i += 1;
            }
            "-d" => {
                i += 1;
                let value = rest
                    .get(i)
                    .ok_or_else(|| "missing value for -d".to_string())?;
                app_dir = Some(PathBuf::from(value));
                i += 1;
            }
            "-f" | "-c" => {
                i += 1;
                let value = rest
                    .get(i)
                    .ok_or_else(|| "missing value for -f/-c".to_string())?;
                config = Some(PathBuf::from(value));
                i += 1;
            }
            other => {
                return Err(format!("unexpected argument: {other}"));
            }
        }
    }

    let app_dir = app_dir.ok_or_else(|| "missing -d <app_dir>".to_string())?;
    let config = config.ok_or_else(|| "missing -f/-c <config>".to_string())?;

    if is_check {
        Ok(Mode::Check { app_dir, config })
    } else {
        Ok(Mode::Start { app_dir, config })
    }
}

/// Strict optional `u8` env parse. Unset → `Ok(None)`; set-but-invalid → `Err`.
pub fn parse_optional_env_u8(key: &str, raw: Option<&str>) -> Result<Option<u8>, String> {
    match raw {
        None => Ok(None),
        Some(value) => value.parse::<u8>().map(Some).map_err(|_| {
            format!("fake-core: invalid configuration: {key}={value:?} (expected integer 0-255)")
        }),
    }
}

/// Strict `u8` env parse with default when unset.
pub fn parse_env_u8_or(key: &str, raw: Option<&str>, default: u8) -> Result<u8, String> {
    Ok(parse_optional_env_u8(key, raw)?.unwrap_or(default))
}

/// Strict optional `u16` env parse. Unset → `Ok(None)`; set-but-invalid → `Err`.
pub fn parse_optional_env_u16(key: &str, raw: Option<&str>) -> Result<Option<u16>, String> {
    match raw {
        None => Ok(None),
        Some(value) => value.parse::<u16>().map(Some).map_err(|_| {
            format!("fake-core: invalid configuration: {key}={value:?} (expected integer 0-65535)")
        }),
    }
}

/// Strict `u16` env parse with default when unset.
pub fn parse_env_u16_or(key: &str, raw: Option<&str>, default: u16) -> Result<u16, String> {
    Ok(parse_optional_env_u16(key, raw)?.unwrap_or(default))
}

/// Read a process environment variable as UTF-8 text.
///
/// Returns `Ok(None)` when unset. Non-unicode values are configuration errors.
pub fn env_var_os_string(key: &str) -> Result<Option<String>, String> {
    match env::var(key) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(format!(
            "fake-core: invalid configuration: {key} is not valid UTF-8"
        )),
    }
}

fn bin_file_name() -> String {
    format!("{BIN_NAME}{}", std::env::consts::EXE_SUFFIX)
}

/// Look for `fake-core` next to the running test binary's profile directory.
///
/// Cargo places integration/unit test executables under `.../<profile>/deps/`.
/// Built bins for the same profile/triple live in the parent profile directory.
fn resolve_from_current_exe() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?;
    let parent = exe.parent()?;
    let file_name = bin_file_name();

    if parent.file_name().and_then(|n| n.to_str()) == Some("deps") {
        let profile_dir = parent.parent()?;
        let candidate = profile_dir.join(&file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    let sibling = parent.join(&file_name);
    if sibling.is_file() {
        return Some(sibling);
    }

    None
}

fn target_fallback_path() -> PathBuf {
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target"));
    let profile = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    target_dir.join(profile).join(bin_file_name())
}

/// Resolve the fake-core binary path for process-lifecycle consumers.
///
/// See crate-level docs for discovery order and the `CARGO_BIN_EXE` limitation.
/// The returned path is not guaranteed to exist; use [`require_bin_path`].
pub fn resolve_bin_path() -> PathBuf {
    if let Ok(path) = env::var(PATH_ENV)
        && !path.is_empty()
    {
        return PathBuf::from(path);
    }

    if let Some(path) = resolve_from_current_exe() {
        return path;
    }

    target_fallback_path()
}

/// Same as [`resolve_bin_path`], but fails with an actionable error if missing.
pub fn require_bin_path() -> io::Result<PathBuf> {
    let path = resolve_bin_path();
    if path.is_file() {
        Ok(path)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "fake-core binary not found at `{}`. Prebuild with `{PREBUILD_COMMAND}` \
                 (or `cargo test -p fake-core`), then re-run the consumer tests. \
                 Optional override: set non-empty {PATH_ENV}. \
                 Note: a `dev-dependency` on fake-core does not build this binary and \
                 does not set `CARGO_BIN_EXE_fake-core` for the dependent package.",
                path.display()
            ),
        ))
    }
}

/// Parent-side ready/release barrier over a TCP control connection.
///
/// Ordering is barrier-based: the child connects and writes a READY line; the
/// parent later writes [`RELEASE_LINE`]. Timeouts are deadlock safety nets only.
#[derive(Debug)]
pub struct ReadyBarrier {
    listener: TcpListener,
    addr: SocketAddr,
}

impl ReadyBarrier {
    pub fn bind_local() -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let addr = listener.local_addr()?;
        Ok(Self { listener, addr })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn addr_string(&self) -> String {
        self.addr.to_string()
    }

    /// Accept the child control connection and wait until a READY line arrives.
    pub fn accept_ready(&self, timeout: Duration) -> io::Result<ReadyConnection> {
        self.listener.set_nonblocking(true)?;
        let deadline = Instant::now() + timeout;
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => {
                    stream.set_nonblocking(false)?;
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "timed out after accept before READY",
                        ));
                    }
                    stream.set_read_timeout(Some(remaining))?;
                    stream.set_write_timeout(Some(remaining))?;
                    let line = read_line(&mut stream)?;
                    let announcement = ReadyAnnouncement::parse_line(&line)?;
                    return Ok(ReadyConnection {
                        stream,
                        announcement,
                    });
                }
                Err(err)
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::Interrupted =>
                {
                    if Instant::now() >= deadline {
                        return Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "timed out waiting for READY control connection",
                        ));
                    }
                    // Poll interval only implements the accept timeout; ordering
                    // still depends on the READY barrier, not this sleep.
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(err) => return Err(err),
            }
        }
    }

    pub fn release(mut stream: TcpStream) -> io::Result<()> {
        stream.write_all(RELEASE_LINE.as_bytes())?;
        stream.flush()?;
        Ok(())
    }
}

fn read_line(stream: &mut TcpStream) -> io::Result<String> {
    let mut buf = Vec::with_capacity(64);
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte)?;
        buf.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
        if buf.len() > 512 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "READY line exceeded 512 bytes",
            ));
        }
    }
    String::from_utf8(buf).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

/// Child-side control connection used by the fake-core binary.
pub fn signal_ready_and_wait_release(
    addr: &str,
    announcement: &ReadyAnnouncement,
    timeout: Duration,
) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    let mut stream = loop {
        match TcpStream::connect(addr) {
            Ok(stream) => break stream,
            Err(err) => {
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("connect READY_ADDR timed out: {err}"),
                    ));
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    };

    let remaining = deadline.saturating_duration_since(Instant::now());
    stream.set_read_timeout(Some(remaining.max(Duration::from_millis(1))))?;
    stream.set_write_timeout(Some(remaining.max(Duration::from_millis(1))))?;
    let ready_line = announcement.to_line();
    stream.write_all(ready_line.as_bytes())?;
    stream.flush()?;

    let mut buf = vec![0u8; RELEASE_LINE.len()];
    match stream.read_exact(&mut buf) {
        Ok(()) => {
            if buf == RELEASE_LINE.as_bytes() {
                Ok(())
            } else {
                Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "expected RELEASE barrier, got {:?}",
                        String::from_utf8_lossy(&buf)
                    ),
                ))
            }
        }
        // Parent dropped the socket: treat as release.
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => Ok(()),
        Err(err) => Err(err),
    }
}

/// RAII wrapper that kills and waits the child on drop if still running.
///
/// Use this in tests so assertion failures cannot leak processes or held ports.
#[derive(Debug)]
pub struct ScopedChild {
    child: Option<Child>,
}

impl ScopedChild {
    pub fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    pub fn child(&mut self) -> &mut Child {
        self.child
            .as_mut()
            .expect("ScopedChild already taken or dropped")
    }

    pub fn id(&self) -> u32 {
        self.child
            .as_ref()
            .expect("ScopedChild already taken or dropped")
            .id()
    }

    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child().try_wait()
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child().wait()
    }

    pub fn wait_with_timeout(&mut self, timeout: Duration) -> io::Result<ExitStatus> {
        wait_with_timeout(self.child(), timeout)
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.child().kill()
    }

    /// Disarm kill-on-drop and return the inner child.
    pub fn into_inner(mut self) -> Child {
        self.child
            .take()
            .expect("ScopedChild already taken or dropped")
    }
}

impl Drop for ScopedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            match child.try_wait() {
                Ok(Some(_)) => {}
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

/// Builder for spawning the fake-core binary from tests.
#[derive(Debug)]
pub struct FakeCoreCommand {
    command: Command,
}

impl FakeCoreCommand {
    pub fn new(bin: impl AsRef<Path>) -> Self {
        let mut command = Command::new(bin.as_ref());
        command.stdin(Stdio::null());
        Self { command }
    }

    pub fn check(mut self, app_dir: impl AsRef<Path>, config: impl AsRef<Path>) -> Self {
        self.command
            .arg("-t")
            .arg("-d")
            .arg(app_dir.as_ref())
            .arg("-f")
            .arg(config.as_ref());
        self
    }

    pub fn start_mihomo(mut self, app_dir: impl AsRef<Path>, config: impl AsRef<Path>) -> Self {
        self.command
            .arg("-m")
            .arg("-d")
            .arg(app_dir.as_ref())
            .arg("-f")
            .arg(config.as_ref());
        self
    }

    pub fn start_clash_rs(mut self, app_dir: impl AsRef<Path>, config: impl AsRef<Path>) -> Self {
        self.command
            .arg("-d")
            .arg(app_dir.as_ref())
            .arg("-c")
            .arg(config.as_ref());
        self
    }

    pub fn start_premium(mut self, app_dir: impl AsRef<Path>, config: impl AsRef<Path>) -> Self {
        self.command
            .arg("-d")
            .arg(app_dir.as_ref())
            .arg("-f")
            .arg(config.as_ref());
        self
    }

    pub fn env(mut self, key: impl AsRef<str>, value: impl AsRef<std::ffi::OsStr>) -> Self {
        self.command.env(key.as_ref(), value.as_ref());
        self
    }

    pub fn stdout<T: Into<Stdio>>(mut self, cfg: T) -> Self {
        self.command.stdout(cfg);
        self
    }

    pub fn stderr<T: Into<Stdio>>(mut self, cfg: T) -> Self {
        self.command.stderr(cfg);
        self
    }

    pub fn spawn(mut self) -> io::Result<Child> {
        self.command.spawn()
    }

    pub fn spawn_scoped(self) -> io::Result<ScopedChild> {
        Ok(ScopedChild::new(self.spawn()?))
    }

    pub fn output(mut self) -> io::Result<Output> {
        self.command.output()
    }

    pub fn command_mut(&mut self) -> &mut Command {
        &mut self.command
    }
}

/// Wait until `child` exits or `timeout` elapses (deadlock safety net).
pub fn wait_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> io::Result<std::process::ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => return Ok(status),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "child did not exit before timeout",
                ));
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn parse_check_argv() {
        let mode =
            parse_args(["fake-core", "-t", "-d", "/tmp/app", "-f", "/tmp/cfg.yaml"]).unwrap();
        assert_eq!(
            mode,
            Mode::Check {
                app_dir: PathBuf::from("/tmp/app"),
                config: PathBuf::from("/tmp/cfg.yaml"),
            }
        );
    }

    #[test]
    fn parse_mihomo_start_argv() {
        let mode =
            parse_args(["fake-core", "-m", "-d", "/tmp/app", "-f", "/tmp/cfg.yaml"]).unwrap();
        assert_eq!(
            mode,
            Mode::Start {
                app_dir: PathBuf::from("/tmp/app"),
                config: PathBuf::from("/tmp/cfg.yaml"),
            }
        );
    }

    #[test]
    fn parse_clash_rs_start_argv() {
        let mode = parse_args(["fake-core", "-d", "/tmp/app", "-c", "/tmp/cfg.yaml"]).unwrap();
        assert_eq!(
            mode,
            Mode::Start {
                app_dir: PathBuf::from("/tmp/app"),
                config: PathBuf::from("/tmp/cfg.yaml"),
            }
        );
    }

    #[test]
    fn parse_premium_start_argv() {
        let mode = parse_args(["fake-core", "-d", "/tmp/app", "-f", "/tmp/cfg.yaml"]).unwrap();
        assert_eq!(
            mode,
            Mode::Start {
                app_dir: PathBuf::from("/tmp/app"),
                config: PathBuf::from("/tmp/cfg.yaml"),
            }
        );
    }

    #[test]
    fn numeric_env_unset_defaults_and_invalid_rejected() {
        assert_eq!(parse_optional_env_u8("K", None).unwrap(), None);
        assert_eq!(parse_env_u8_or("K", None, 0).unwrap(), 0);
        assert_eq!(parse_optional_env_u8("K", Some("7")).unwrap(), Some(7));
        assert_eq!(parse_env_u16_or("K", None, 204).unwrap(), 204);

        let err = parse_optional_env_u8(env_keys::CHECK_EXIT, Some("abc")).unwrap_err();
        assert!(err.contains("invalid configuration"));
        assert!(err.contains(env_keys::CHECK_EXIT));

        let err = parse_optional_env_u8(env_keys::CHECK_EXIT, Some("256")).unwrap_err();
        assert!(err.contains("0-255"));

        let err = parse_optional_env_u16(env_keys::HOLD_PORT, Some("-1")).unwrap_err();
        assert!(err.contains("0-65535"));

        let err = parse_optional_env_u16(env_keys::HOLD_PORT, Some("65536")).unwrap_err();
        assert!(err.contains("0-65535"));

        let err = parse_optional_env_u16(env_keys::HOLD_PORT, Some("")).unwrap_err();
        assert!(err.contains("invalid configuration"));
    }

    #[test]
    fn ready_announcement_roundtrip() {
        let bare = ReadyAnnouncement::default();
        assert_eq!(bare.to_line(), "READY\n");
        assert_eq!(ReadyAnnouncement::parse_line("READY\n").unwrap(), bare);

        let both = ReadyAnnouncement {
            hold_port: Some(18080),
            http_port: Some(19090),
        };
        let line = both.to_line();
        assert_eq!(line, "READY hold=18080 http=19090\n");
        assert_eq!(ReadyAnnouncement::parse_line(&line).unwrap(), both);
        assert_eq!(
            ReadyAnnouncement::parse_line("READY hold=1")
                .unwrap()
                .hold_port,
            Some(1)
        );
    }

    #[test]
    fn empty_path_env_is_ignored_by_resolve() {
        // Mutex-scoped set/resolve/restore — do not rely on libtest serial execution.
        let _guard = PathEnvGuard::set_empty();
        let resolved = resolve_bin_path();
        assert!(
            resolved
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("fake-core")),
            "empty {PATH_ENV} must fall through, got {}",
            resolved.display()
        );
    }
}
