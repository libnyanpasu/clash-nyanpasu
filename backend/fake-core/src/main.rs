//! Deterministic test-only fake clash core process.
//!
//! Accepts the real core argv shapes used by `nyanpasu-utils`:
//! - check: `-t -d <app_dir> -f <config>`
//! - start (mihomo): `-m -d <app_dir> -f <config>`
//! - start (clash-rs): `-d <app_dir> -c <config>`
//! - start (premium): `-d <app_dir> -f <config>`
//!
//! Behaviour is controlled by `FAKE_CORE_*` environment variables (see
//! `fake_core::env_keys`). Synchronization with the parent uses a TCP
//! ready/release barrier — never sleep-based ordering.
//!
//! # Start semantics
//!
//! - `FAKE_CORE_START_EXIT`: **immediate** termination after optional start
//!   stdout/stderr. Not a long-running process.
//! - Long-running start requires `FAKE_CORE_READY_ADDR` (ready/release barrier).
//!   Without either `START_EXIT` or a barrier, the process fails fast with exit 2.
//!
//! # HTTP contract
//!
//! Status-injection only for exact `PUT /configs` and `PATCH /configs` request
//! targets. Prefix paths such as `/configs/xxx` are not matched.

use fake_core::{
    Mode, ReadyAnnouncement, env_keys, env_var_os_string, parse_args, parse_env_u8_or,
    parse_env_u16_or, parse_optional_env_u8, parse_optional_env_u16, signal_ready_and_wait_release,
};
use std::{
    env,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

const DEFAULT_BARRIER_TIMEOUT: Duration = Duration::from_secs(10);

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    match parse_args(args.iter().map(|s| s.as_str())) {
        Ok(Mode::Check { .. }) => run_check(),
        Ok(Mode::Start { .. }) => run_start(),
        Err(err) => {
            eprintln!("fake-core: {err}");
            ExitCode::from(2)
        }
    }
}

fn run_check() -> ExitCode {
    if let Some(stdout) = env::var_os(env_keys::CHECK_STDOUT) {
        print!("{}", stdout.to_string_lossy());
        let _ = std::io::stdout().flush();
    }
    if let Some(stderr) = env::var_os(env_keys::CHECK_STDERR) {
        eprint!("{}", stderr.to_string_lossy());
        let _ = std::io::stderr().flush();
    }

    match env_u8_or(env_keys::CHECK_EXIT, 0) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(2)
        }
    }
}

fn run_start() -> ExitCode {
    if let Some(stdout) = env::var_os(env_keys::START_STDOUT) {
        print!("{}", stdout.to_string_lossy());
        let _ = std::io::stdout().flush();
    }
    if let Some(stderr) = env::var_os(env_keys::START_STDERR) {
        eprint!("{}", stderr.to_string_lossy());
        let _ = std::io::stderr().flush();
    }

    // Parse every configured numeric env strictly up front. Unset may default;
    // set-but-invalid must fail with exit 2 before any long-running work.
    let start_exit = match env_optional_u8(env_keys::START_EXIT) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let hold_port = match env_optional_u16(env_keys::HOLD_PORT) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let http_port = match env_optional_u16(env_keys::HTTP_PORT) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let apply_status = match env_u16_or(env_keys::APPLY_STATUS, 204) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let exit_after_release = match env_u8_or(env_keys::EXIT_AFTER_RELEASE, 0) {
        Ok(v) => v,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    let apply_body = env::var(env_keys::APPLY_BODY).unwrap_or_default();
    let ready_addr = match env_var_os_string(env_keys::READY_ADDR) {
        Ok(Some(addr)) if !addr.is_empty() => Some(addr),
        Ok(_) => None,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };

    // START_EXIT means immediate termination — not a long-running start.
    if let Some(code) = start_exit {
        return ExitCode::from(code);
    }

    // Successful long-running start requires barrier mode.
    let Some(ready_addr) = ready_addr else {
        eprintln!(
            "fake-core: long-running start requires {} control barrier \
             (or {} for immediate exit)",
            env_keys::READY_ADDR,
            env_keys::START_EXIT
        );
        return ExitCode::from(2);
    };

    let stop = Arc::new(AtomicBool::new(false));
    let mut held_listeners: Vec<TcpListener> = Vec::new();
    let mut http_thread: Option<thread::JoinHandle<()>> = None;
    let mut announcement = ReadyAnnouncement::default();

    match (hold_port, http_port) {
        (Some(port), Some(http)) if port == http => match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => {
                let actual = match listener.local_addr() {
                    Ok(addr) => addr.port(),
                    Err(err) => {
                        eprintln!("fake-core: local_addr failed: {err}");
                        return ExitCode::from(1);
                    }
                };
                announcement.hold_port = Some(actual);
                announcement.http_port = Some(actual);
                if let Err(err) = listener.set_nonblocking(true) {
                    eprintln!("fake-core: set_nonblocking failed: {err}");
                    return ExitCode::from(1);
                }
                let stop_flag = Arc::clone(&stop);
                let body = apply_body.clone();
                http_thread = Some(thread::spawn(move || {
                    serve_http(listener, apply_status, body, stop_flag);
                }));
            }
            Err(err) => {
                eprintln!("fake-core: failed to hold/http port {port}: {err}");
                return ExitCode::from(1);
            }
        },
        (hold, http) => {
            if let Some(port) = hold {
                match TcpListener::bind(("127.0.0.1", port)) {
                    Ok(listener) => {
                        let actual = match listener.local_addr() {
                            Ok(addr) => addr.port(),
                            Err(err) => {
                                eprintln!("fake-core: local_addr failed: {err}");
                                return ExitCode::from(1);
                            }
                        };
                        announcement.hold_port = Some(actual);
                        held_listeners.push(listener);
                    }
                    Err(err) => {
                        eprintln!("fake-core: failed to hold port {port}: {err}");
                        return ExitCode::from(1);
                    }
                }
            }
            if let Some(port) = http {
                match TcpListener::bind(("127.0.0.1", port)) {
                    Ok(listener) => {
                        let actual = match listener.local_addr() {
                            Ok(addr) => addr.port(),
                            Err(err) => {
                                eprintln!("fake-core: local_addr failed: {err}");
                                return ExitCode::from(1);
                            }
                        };
                        announcement.http_port = Some(actual);
                        if let Err(err) = listener.set_nonblocking(true) {
                            eprintln!("fake-core: set_nonblocking failed: {err}");
                            return ExitCode::from(1);
                        }
                        let stop_flag = Arc::clone(&stop);
                        let body = apply_body.clone();
                        http_thread = Some(thread::spawn(move || {
                            serve_http(listener, apply_status, body, stop_flag);
                        }));
                    }
                    Err(err) => {
                        eprintln!("fake-core: failed to bind http port {port}: {err}");
                        return ExitCode::from(1);
                    }
                }
            }
        }
    }

    // Keep held_listeners alive until release.
    let _held = held_listeners;

    if let Err(err) =
        signal_ready_and_wait_release(&ready_addr, &announcement, DEFAULT_BARRIER_TIMEOUT)
    {
        eprintln!("fake-core: barrier failed: {err}");
        stop.store(true, Ordering::SeqCst);
        if let Some(handle) = http_thread {
            let _ = handle.join();
        }
        return ExitCode::from(1);
    }

    stop.store(true, Ordering::SeqCst);
    if let Some(handle) = http_thread {
        let _ = handle.join();
    }

    ExitCode::from(exit_after_release)
}

fn serve_http(listener: TcpListener, status: u16, body: String, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                if let Err(err) = handle_http_client(stream, status, &body) {
                    eprintln!("fake-core: http client error: {err}");
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::Interrupted =>
            {
                // Polling interval for stop flag / accept; not used for test ordering.
                thread::sleep(Duration::from_millis(5));
            }
            Err(err) => {
                eprintln!("fake-core: http accept error: {err}");
                break;
            }
        }
    }
}

fn handle_http_client(mut stream: TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;

    let mut buf = [0u8; 8192];
    let mut collected = Vec::new();
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                collected.extend_from_slice(&buf[..n]);
                if collected.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                if collected.len() >= 8192 {
                    break;
                }
            }
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                break;
            }
            Err(err) => return Err(err),
        }
    }

    let request = String::from_utf8_lossy(&collected);
    let first_line = request.lines().next().unwrap_or("");
    // Status-injection only for exact /configs — not prefix paths.
    let is_apply = is_exact_configs_apply(first_line);

    let (code, reason) = if is_apply {
        (
            status,
            match status {
                200 => "OK",
                204 => "No Content",
                400 => "Bad Request",
                500 => "Internal Server Error",
                _ if status >= 400 => "Error",
                _ => "OK",
            },
        )
    } else {
        (404, "Not Found")
    };

    let response_body = if is_apply { body } else { "" };
    let response = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
        response_body.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// Match exact `PUT /configs` or `PATCH /configs` request targets only.
fn is_exact_configs_apply(request_line: &str) -> bool {
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    matches!(method, "PUT" | "PATCH") && path == "/configs"
}

fn env_optional_u8(key: &str) -> Result<Option<u8>, String> {
    let raw = env_var_os_string(key)?;
    parse_optional_env_u8(key, raw.as_deref())
}

fn env_u8_or(key: &str, default: u8) -> Result<u8, String> {
    let raw = env_var_os_string(key)?;
    parse_env_u8_or(key, raw.as_deref(), default)
}

fn env_optional_u16(key: &str) -> Result<Option<u16>, String> {
    let raw = env_var_os_string(key)?;
    parse_optional_env_u16(key, raw.as_deref())
}

fn env_u16_or(key: &str, default: u16) -> Result<u16, String> {
    let raw = env_var_os_string(key)?;
    parse_env_u16_or(key, raw.as_deref(), default)
}
