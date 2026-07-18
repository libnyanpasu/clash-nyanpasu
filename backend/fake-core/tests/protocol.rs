//! Standalone protocol tests for the fake-core binary.
//!
//! Synchronization is barrier-based (`READY` / `RELEASE` over TCP). Timeouts are
//! deadlock safety nets only — tests never order steps with sleep.

use fake_core::{
    FakeCoreCommand, PATH_ENV, PREBUILD_COMMAND, PathEnvGuard, PathEnvLock, ReadyBarrier, env_keys,
    require_bin_path, resolve_bin_path,
};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::Stdio,
    time::Duration,
};

const SAFETY: Duration = Duration::from_secs(5);

fn bin() -> PathBuf {
    // Guaranteed for this package's integration tests only.
    PathBuf::from(env!("CARGO_BIN_EXE_fake-core"))
}

fn scratch_paths() -> (PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "fake-core-protocol-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::create_dir_all(&root);
    let config = root.join("config.yaml");
    std::fs::write(&config, "mixed-port: 0\n").expect("write config");
    (root, config)
}

#[test]
fn cargo_bin_exe_points_at_real_binary() {
    let path = bin();
    assert!(
        path.is_file(),
        "CARGO_BIN_EXE_fake-core must resolve to a file, got {}",
        path.display()
    );
}

#[test]
fn workspace_target_fallback_and_require_bin_path() {
    // Cross-crate consumers cannot rely on CARGO_BIN_EXE_fake-core. After this
    // package's tests build the binary, resolve/require must find it via
    // current_exe-relative discovery or the workspace target dir.
    // Hold the PATH_ENV lock so concurrent mutations cannot race this observation.
    let _lock = PathEnvLock::acquire();
    if std::env::var_os(PATH_ENV).is_some_and(|v| !v.is_empty()) {
        let resolved = resolve_bin_path();
        assert!(
            resolved.is_file(),
            "{PATH_ENV} points at missing binary: {}",
            resolved.display()
        );
    } else {
        let resolved = resolve_bin_path();
        assert!(
            resolved
                .file_name()
                .is_some_and(|n| { n.to_string_lossy().starts_with("fake-core") }),
            "fallback path should end with fake-core*, got {}",
            resolved.display()
        );
        assert!(
            resolved.is_file(),
            "resolved path should exist after cargo test -p fake-core: {}",
            resolved.display()
        );
    }
    let required = require_bin_path().unwrap_or_else(|err| {
        panic!("binary must exist after cargo test -p fake-core ({PREBUILD_COMMAND}): {err}")
    });
    assert!(required.is_file());
    // Same-package tests still use CARGO_BIN_EXE directly.
    assert_eq!(bin().file_name(), required.file_name());
}

#[test]
fn empty_nyanpasu_fake_core_is_ignored() {
    // Mutex-scoped set/resolve/restore — do not rely on libtest serial execution.
    let _guard = PathEnvGuard::set_empty();
    let resolved = resolve_bin_path();
    assert!(
        resolved.is_file()
            || resolved
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("fake-core")),
        "empty {PATH_ENV} must not become the resolved path, got {}",
        resolved.display()
    );
    assert_ne!(resolved.as_os_str(), "");
}

#[test]
fn check_failure_exit_code_and_streams() {
    let (app_dir, config) = scratch_paths();
    let output = FakeCoreCommand::new(bin())
        .check(&app_dir, &config)
        .env(env_keys::CHECK_EXIT, "7")
        .env(env_keys::CHECK_STDOUT, "check-stdout-line\n")
        .env(env_keys::CHECK_STDERR, "check-stderr-line\n")
        .output()
        .expect("run check");

    assert_eq!(output.status.code(), Some(7));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "check-stdout-line\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "check-stderr-line\n"
    );
}

#[test]
fn check_success_default_exit_zero() {
    let (app_dir, config) = scratch_paths();
    let output = FakeCoreCommand::new(bin())
        .check(&app_dir, &config)
        .output()
        .expect("run check");
    assert!(output.status.success());
}

#[test]
fn invalid_numeric_env_exits_two_with_stable_error() {
    let (app_dir, config) = scratch_paths();

    // Previously these set-but-invalid values were silently ignored (false success).
    let check_cases = [
        (env_keys::CHECK_EXIT, "abc"),
        (env_keys::CHECK_EXIT, "256"),
        (env_keys::CHECK_EXIT, ""),
    ];
    for (key, value) in check_cases {
        let output = FakeCoreCommand::new(bin())
            .check(&app_dir, &config)
            .env(key, value)
            .output()
            .unwrap_or_else(|err| panic!("run invalid {key}={value:?}: {err}"));
        assert_eq!(output.status.code(), Some(2), "{key}={value:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("invalid configuration") && stderr.contains(key),
            "stderr for {key}={value:?}: {stderr}"
        );
    }

    let start_cases = [
        (env_keys::START_EXIT, "nope"),
        (env_keys::HOLD_PORT, "65536"),
        (env_keys::HOLD_PORT, ""),
        (env_keys::HTTP_PORT, "x"),
        (env_keys::APPLY_STATUS, "-1"),
        (env_keys::EXIT_AFTER_RELEASE, "999"),
    ];
    for (key, value) in start_cases {
        // Config is validated before the barrier connect, so a dummy READY_ADDR is fine.
        let output = FakeCoreCommand::new(bin())
            .start_mihomo(&app_dir, &config)
            .env(env_keys::READY_ADDR, "127.0.0.1:1")
            .env(key, value)
            .output()
            .unwrap_or_else(|err| panic!("run invalid {key}={value:?}: {err}"));
        assert_eq!(
            output.status.code(),
            Some(2),
            "invalid {key}={value:?} must exit 2, got {:?}",
            output.status.code()
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("invalid configuration") && stderr.contains(key),
            "stderr must be a stable configuration error for {key}={value:?}, got: {stderr}"
        );
    }
}

#[test]
fn start_immediate_failure_exit_code_and_streams() {
    let (app_dir, config) = scratch_paths();
    let output = FakeCoreCommand::new(bin())
        .start_mihomo(&app_dir, &config)
        .env(env_keys::START_EXIT, "9")
        .env(env_keys::START_STDOUT, "start-out\n")
        .env(env_keys::START_STDERR, "start-err\n")
        .output()
        .expect("run start failure");

    assert_eq!(output.status.code(), Some(9));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "start-out\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "start-err\n");
}

#[test]
fn start_without_barrier_or_start_exit_fails_fast() {
    let (app_dir, config) = scratch_paths();
    let output = FakeCoreCommand::new(bin())
        .start_mihomo(&app_dir, &config)
        .output()
        .expect("run start without barrier");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(env_keys::READY_ADDR) && stderr.contains(env_keys::START_EXIT),
        "stderr must explain barrier / START_EXIT requirement, got: {stderr}"
    );
}

#[test]
fn start_ready_release_barrier_and_exit_after_release() {
    let (app_dir, config) = scratch_paths();
    let barrier = ReadyBarrier::bind_local().expect("bind barrier");
    let mut child = FakeCoreCommand::new(bin())
        .start_mihomo(&app_dir, &config)
        .env(env_keys::READY_ADDR, barrier.addr_string())
        .env(env_keys::EXIT_AFTER_RELEASE, "3")
        .env(env_keys::START_STDOUT, "running\n")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_scoped()
        .expect("spawn start");

    let ready = barrier.accept_ready(SAFETY).expect("accept READY");
    assert_eq!(ready.announcement.hold_port, None);
    assert_eq!(ready.announcement.http_port, None);
    // Child is now running and waiting for RELEASE — no sleep ordering.
    ReadyBarrier::release(ready.stream).expect("send RELEASE");

    let status = child.wait_with_timeout(SAFETY).expect("wait child");
    assert_eq!(status.code(), Some(3));
}

#[test]
fn start_holds_fixed_port_and_conflicts() {
    let (app_dir, config) = scratch_paths();

    // Holder binds HOLD_PORT=0, then announces the actual still-held port in READY.
    // Challenger targets that announced port (no TOCTOU via pre-probed free port).
    let barrier = ReadyBarrier::bind_local().expect("bind barrier");
    let mut holder = FakeCoreCommand::new(bin())
        .start_clash_rs(&app_dir, &config)
        .env(env_keys::READY_ADDR, barrier.addr_string())
        .env(env_keys::HOLD_PORT, "0")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn_scoped()
        .expect("spawn holder");

    let ready = barrier.accept_ready(SAFETY).expect("holder READY");
    let port = ready
        .announcement
        .hold_port
        .expect("holder must announce actual hold port");
    assert_ne!(port, 0, "ephemeral hold must report a non-zero port");

    // Port is held: a second bind must fail.
    let conflict = TcpListener::bind(("127.0.0.1", port));
    assert!(
        conflict.is_err(),
        "expected hold port {port} to be occupied"
    );

    // A second fake-core trying to hold the same still-held port must exit non-zero.
    // READY_ADDR is present so config validation passes; bind fails on the held port.
    let (app_dir2, config2) = scratch_paths();
    let output = FakeCoreCommand::new(bin())
        .start_premium(&app_dir2, &config2)
        .env(env_keys::HOLD_PORT, port.to_string())
        .env(env_keys::READY_ADDR, "127.0.0.1:1")
        .output()
        .expect("run conflict start");
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to hold port") || stderr.contains("failed to hold/http port"),
        "stderr should describe hold failure, got: {stderr}"
    );

    ReadyBarrier::release(ready.stream).expect("release holder");
    let status = holder.wait_with_timeout(SAFETY).expect("wait holder");
    assert!(status.success());
}

#[test]
fn start_port_zero_reports_actual_ports_in_ready() {
    let (app_dir, config) = scratch_paths();

    // Equal ports (including both 0) share one listener — report the same actual port.
    let barrier = ReadyBarrier::bind_local().expect("bind barrier shared");
    let mut shared = FakeCoreCommand::new(bin())
        .start_mihomo(&app_dir, &config)
        .env(env_keys::READY_ADDR, barrier.addr_string())
        .env(env_keys::HOLD_PORT, "0")
        .env(env_keys::HTTP_PORT, "0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn_scoped()
        .expect("spawn shared port0");
    let ready = barrier.accept_ready(SAFETY).expect("READY shared");
    let hold = ready.announcement.hold_port.expect("hold");
    let http = ready.announcement.http_port.expect("http");
    assert_ne!(hold, 0);
    assert_eq!(hold, http, "equal configured ports share one bound port");
    assert!(TcpListener::bind(("127.0.0.1", hold)).is_err());
    let mut stream = connect_with_timeout(("127.0.0.1", http), SAFETY).expect("http");
    stream
        .write_all(
            b"PUT /configs HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        )
        .expect("write");
    let mut resp = String::new();
    stream.read_to_string(&mut resp).expect("read");
    assert!(resp.starts_with("HTTP/1.1 204"), "got: {resp}");
    ReadyBarrier::release(ready.stream).expect("release shared");
    assert!(
        shared
            .wait_with_timeout(SAFETY)
            .expect("wait shared")
            .success()
    );

    // Separate hold=0 only: actual hold announced, no http.
    let barrier = ReadyBarrier::bind_local().expect("bind barrier hold");
    let mut hold_only = FakeCoreCommand::new(bin())
        .start_clash_rs(&app_dir, &config)
        .env(env_keys::READY_ADDR, barrier.addr_string())
        .env(env_keys::HOLD_PORT, "0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn_scoped()
        .expect("spawn hold0");
    let ready = barrier.accept_ready(SAFETY).expect("READY hold");
    let hold = ready.announcement.hold_port.expect("hold only");
    assert_ne!(hold, 0);
    assert_eq!(ready.announcement.http_port, None);
    assert!(TcpListener::bind(("127.0.0.1", hold)).is_err());
    ReadyBarrier::release(ready.stream).expect("release hold");
    assert!(
        hold_only
            .wait_with_timeout(SAFETY)
            .expect("wait hold")
            .success()
    );
}

#[test]
fn apply_http_put_and_patch_configs_exact_paths_only() {
    let (app_dir, config) = scratch_paths();
    let barrier = ReadyBarrier::bind_local().expect("bind barrier");

    let mut child = FakeCoreCommand::new(bin())
        .start_mihomo(&app_dir, &config)
        .env(env_keys::READY_ADDR, barrier.addr_string())
        .env(env_keys::HTTP_PORT, "0")
        .env(env_keys::APPLY_STATUS, "204")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn_scoped()
        .expect("spawn http core");

    let ready = barrier.accept_ready(SAFETY).expect("READY");
    let http_port = ready.announcement.http_port.expect("http port");

    let mut stream = connect_with_timeout(("127.0.0.1", http_port), SAFETY).expect("connect http");
    stream
        .write_all(
            b"PUT /configs HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 15\r\nConnection: close\r\n\r\n{\"path\":\"x.yaml\"}",
        )
        .expect("write put");
    let mut resp = String::new();
    stream.read_to_string(&mut resp).expect("read put resp");
    assert!(
        resp.starts_with("HTTP/1.1 204"),
        "expected 204 apply success, got: {resp}"
    );

    let mut stream = connect_with_timeout(("127.0.0.1", http_port), SAFETY).expect("connect patch");
    stream
        .write_all(
            b"PATCH /configs HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        )
        .expect("write patch");
    let mut resp = String::new();
    stream.read_to_string(&mut resp).expect("read patch resp");
    assert!(
        resp.starts_with("HTTP/1.1 204"),
        "expected 204 patch success, got: {resp}"
    );

    // Prefix path must not receive the injected status.
    let mut stream =
        connect_with_timeout(("127.0.0.1", http_port), SAFETY).expect("connect prefix");
    stream
        .write_all(
            b"PUT /configs/extra HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .expect("write prefix");
    let mut resp = String::new();
    stream.read_to_string(&mut resp).expect("read prefix");
    assert!(
        resp.starts_with("HTTP/1.1 404"),
        "prefix /configs/extra must 404, got: {resp}"
    );

    ReadyBarrier::release(ready.stream).expect("release");
    let status = child.wait_with_timeout(SAFETY).expect("wait");
    assert!(status.success());

    // Failure mode: restart with apply status 500.
    let (app_dir, config) = scratch_paths();
    let barrier = ReadyBarrier::bind_local().expect("bind barrier 2");
    let mut child = FakeCoreCommand::new(bin())
        .start_mihomo(&app_dir, &config)
        .env(env_keys::READY_ADDR, barrier.addr_string())
        .env(env_keys::HTTP_PORT, "0")
        .env(env_keys::APPLY_STATUS, "500")
        .env(env_keys::APPLY_BODY, "apply failed")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn_scoped()
        .expect("spawn failing apply core");
    let ready = barrier.accept_ready(SAFETY).expect("READY fail mode");
    let http_port = ready.announcement.http_port.expect("http port fail mode");

    let mut stream = connect_with_timeout(("127.0.0.1", http_port), SAFETY).expect("connect");
    stream
        .write_all(
            b"PUT /configs HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
        )
        .expect("write");
    let mut resp = String::new();
    stream.read_to_string(&mut resp).expect("read");
    assert!(
        resp.starts_with("HTTP/1.1 500"),
        "expected 500 apply failure, got: {resp}"
    );
    assert!(resp.contains("apply failed"));

    ReadyBarrier::release(ready.stream).expect("release fail mode");
    let status = child.wait_with_timeout(SAFETY).expect("wait fail mode");
    assert!(status.success());
}

#[test]
fn scoped_child_kills_on_drop() {
    let (app_dir, config) = scratch_paths();
    let barrier = ReadyBarrier::bind_local().expect("bind barrier");
    let mut child = FakeCoreCommand::new(bin())
        .start_mihomo(&app_dir, &config)
        .env(env_keys::READY_ADDR, barrier.addr_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn_scoped()
        .expect("spawn");
    let pid = child.id();
    let _ready = barrier.accept_ready(SAFETY).expect("READY");
    // Still running (no RELEASE yet).
    assert!(child.try_wait().expect("try_wait").is_none());
    // Drop without release: RAII must kill + wait so assertion failures cannot leak.
    drop(child);

    #[cfg(target_os = "linux")]
    {
        let proc = PathBuf::from(format!("/proc/{pid}"));
        assert!(
            !proc.exists(),
            "child pid {pid} should be gone after ScopedChild drop"
        );
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
    }
}

fn connect_with_timeout(addr: (&str, u16), timeout: Duration) -> std::io::Result<TcpStream> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match TcpStream::connect(addr) {
            Ok(stream) => {
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                stream.set_write_timeout(Some(Duration::from_secs(2)))?;
                return Ok(stream);
            }
            Err(err) => {
                if std::time::Instant::now() >= deadline {
                    return Err(err);
                }
                // Connect retry is a readiness safety net; READY barrier already
                // guaranteed the server thread is up.
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}
