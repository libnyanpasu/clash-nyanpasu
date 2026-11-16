//! The real binary against a mock shell: argument handling, exit codes and one full
//! HTTP round trip, without any application core.

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::{Command, Output, Stdio},
    sync::{Arc, Mutex},
    thread,
};

use serde_json::{Value, json};

/// A request the mock shell received.
#[derive(Debug, Clone)]
struct Received {
    headers: String,
    body: Value,
}

/// Serves `/bridge/rpc` for one test: `rpc_describe` and `get_core_status_summary`.
struct MockShell {
    address: SocketAddr,
    received: Arc<Mutex<Vec<Received>>>,
}

fn read_request(stream: &mut TcpStream) -> Option<Received> {
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
    let head_end = loop {
        if let Some(position) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 {
            return None;
        }
        data.extend_from_slice(&buffer[..count]);
    };
    let headers = String::from_utf8_lossy(&data[..head_end]).into_owned();
    let length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    while data.len() < head_end + 4 + length {
        let count = stream.read(&mut buffer).ok()?;
        if count == 0 {
            return None;
        }
        data.extend_from_slice(&buffer[..count]);
    }
    let body = serde_json::from_slice(&data[head_end + 4..head_end + 4 + length]).ok()?;
    Some(Received { headers, body })
}

impl MockShell {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&received);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let log = Arc::clone(&log);
                thread::spawn(move || {
                    while let Some(request) = read_request(&mut stream) {
                        let reply = match request.body["method"].as_str() {
                            Some("rpc_describe") => json!({
                                "protocol": 1,
                                "server": {"kind": "desktop", "version": "0.0.0"},
                                "transport": "http",
                                "principal": {"kind": "token", "id": "t", "name": "test"},
                                "scopes": ["read"],
                                "commands": [
                                    {"name": "get_core_status_summary", "effect": "query",
                                     "scope": "read", "allowed": true}
                                ],
                            }),
                            Some("get_core_status_summary") => {
                                json!({"running": true, "core": "mihomo"})
                            }
                            _ => json!({"kind": "unknown_method", "message": "no"}),
                        };
                        log.lock().unwrap().push(request);
                        let body = reply.to_string();
                        let head = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                             content-length: {}\r\n\r\n",
                            body.len()
                        );
                        if stream.write_all(head.as_bytes()).is_err()
                            || stream.write_all(body.as_bytes()).is_err()
                        {
                            return;
                        }
                    }
                });
            }
        });
        Self { address, received }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }
}

fn cli(config: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_nyanpasu-cli"));
    command
        .arg("--config")
        .arg(config)
        .env_remove("NYANPASU_URL")
        .env_remove("NYANPASU_TOKEN")
        .env_remove("NYANPASU_CLI_CONFIG")
        // Tests never touch the developer's keychain.
        .env("NYANPASU_NO_KEYRING", "1")
        // A proxy in the environment must never be used for the target.
        .env("HTTP_PROXY", "http://127.0.0.1:1")
        .env("http_proxy", "http://127.0.0.1:1");
    command
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("the process exits normally")
}

#[test]
fn an_http_target_is_called_with_the_bearer_token_and_a_client_id() {
    let shell = MockShell::start();
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("cli.toml");

    let output = cli(&config)
        .args(["--json", "status"])
        .env("NYANPASU_URL", shell.url())
        .env("NYANPASU_TOKEN", "npk_1_secret")
        .output()
        .unwrap();
    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim_end(),
        r#"{"running":true,"core":"mihomo"}"#
    );

    let received = shell.received.lock().unwrap().clone();
    let methods: Vec<_> = received
        .iter()
        .map(|request| request.body["method"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(methods, ["rpc_describe", "get_core_status_summary"]);
    let client_ids: Vec<_> = received
        .iter()
        .map(|request| {
            let lower = request.headers.to_ascii_lowercase();
            assert!(
                lower.contains("authorization: bearer npk_1_secret"),
                "{lower}"
            );
            lower
                .lines()
                .find_map(|line| {
                    line.strip_prefix("x-nyanpasu-client-id: ")
                        .map(str::to_owned)
                })
                .expect("every request carries the client id")
        })
        .collect();
    assert_eq!(client_ids[0], client_ids[1], "one process, one client id");
}

#[test]
fn a_new_process_generates_a_new_client_id() {
    let shell = MockShell::start();
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("cli.toml");
    for _ in 0..2 {
        let output = cli(&config)
            .args(["--json", "status"])
            .env("NYANPASU_URL", shell.url())
            .output()
            .unwrap();
        assert_eq!(code(&output), 0);
    }
    let received = shell.received.lock().unwrap().clone();
    let id = |request: &Received| {
        request
            .headers
            .to_ascii_lowercase()
            .lines()
            .find_map(|line| {
                line.strip_prefix("x-nyanpasu-client-id: ")
                    .map(str::to_owned)
            })
            .unwrap()
    };
    assert_ne!(id(&received[0]), id(&received[2]));
    assert!(
        !received[0]
            .headers
            .to_ascii_lowercase()
            .contains("authorization:"),
        "no token, no authorization header"
    );
}

#[test]
fn an_unreachable_http_target_exits_three_without_a_fallback() {
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let directory = tempfile::tempdir().unwrap();
    let output = cli(&directory.path().join("cli.toml"))
        .arg("status")
        .env("NYANPASU_URL", format!("http://{closed}"))
        .output()
        .unwrap();
    assert_eq!(code(&output), 3);
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot reach"));
}

#[test]
fn a_denied_scope_exits_four_before_anything_is_sent() {
    let shell = MockShell::start();
    let directory = tempfile::tempdir().unwrap();
    let output = cli(&directory.path().join("cli.toml"))
        .args(["core", "restart"])
        .env("NYANPASU_URL", shell.url())
        .output()
        .unwrap();
    assert_eq!(
        code(&output),
        4,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        shell.received.lock().unwrap().len(),
        1,
        "only rpc_describe was sent"
    );
}

/// Runs `nyanpasu-cli mcp` with `requests` on standard input, then closes it.
fn mcp(config: &Path, url: &str, arguments: &[&str], requests: &[Value]) -> Output {
    let mut child = cli(config)
        .arg("mcp")
        .args(arguments)
        .env("NYANPASU_URL", url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for request in requests {
        writeln!(stdin, "{request}").unwrap();
    }
    drop(stdin);
    child.wait_with_output().unwrap()
}

#[test]
fn mcp_serves_stdio_with_nothing_but_protocol_on_standard_output() {
    let shell = MockShell::start();
    let directory = tempfile::tempdir().unwrap();
    let output = mcp(
        &directory.path().join("cli.toml"),
        &shell.url(),
        &[],
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                   "params": {"protocolVersion": "2025-06-18"}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                   "params": {"name": "status", "arguments": {}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                   "params": {"name": "core_restart", "arguments": {}}}),
        ],
    );
    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let replies: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|_| panic!("not JSON-RPC: {line}")))
        .collect();
    assert_eq!(replies.len(), 4, "the notification gets no reply");
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "nyanpasu-cli");
    // The mock shell allows only the status method for a read-only token.
    let tools: Vec<&str> = replies[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(tools, ["status", "conn_list"]);
    assert_eq!(
        replies[2]["result"]["content"][0]["text"],
        r#"{"running":true,"core":"mihomo"}"#
    );
    assert!(
        replies[3]["error"].is_object(),
        "a tool outside the session is not callable"
    );

    // One process, one connection identity, and nothing but describe and the one call.
    let received = shell.received.lock().unwrap().clone();
    let methods: Vec<_> = received
        .iter()
        .map(|request| request.body["method"].as_str().unwrap())
        .collect();
    assert_eq!(methods, ["rpc_describe", "get_core_status_summary"]);
}

#[test]
fn mcp_refuses_remote_admin_before_connecting_and_reports_an_unreachable_target() {
    let shell = MockShell::start();
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("cli.toml");
    for arguments in [
        &["--scope", "remote_admin"][..],
        &["--raw", "--scope", "read", "--scope", "remote_admin"],
    ] {
        let output = mcp(&config, &shell.url(), arguments, &[]);
        assert_eq!(code(&output), 2, "{arguments:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("never offered over MCP"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(
        shell.received.lock().unwrap().is_empty(),
        "a usage error costs no connection"
    );

    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let output = mcp(&config, &format!("http://{closed}"), &[], &[]);
    assert_eq!(code(&output), 3);
    assert!(
        output.stdout.is_empty(),
        "no protocol output without a shell"
    );
}

#[test]
fn usage_errors_exit_two() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("cli.toml");
    for arguments in [
        &["--transport", "carrier-pigeon", "status"][..],
        &[
            "context",
            "add",
            "x",
            "--url",
            "http://h:1",
            "--token",
            "secret",
        ],
        &["config", "patch", "not json"],
        &["conn", "close"],
        &["mode", "set", "sideways"],
    ] {
        let output = cli(&config).args(arguments).output().unwrap();
        assert_eq!(
            code(&output),
            2,
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = cli(&config)
        .args(["--transport", "socket", "status"])
        .env("NYANPASU_URL", "http://127.0.0.1:9")
        .output()
        .unwrap();
    assert_eq!(code(&output), 2, "a transport that does not fit the target");
}

#[test]
fn help_version_and_completion_exit_zero() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("cli.toml");
    for arguments in [&["--help"][..], &["--version"], &["completion", "zsh"]] {
        let output = cli(&config).args(arguments).output().unwrap();
        assert_eq!(code(&output), 0, "{arguments:?}");
        assert!(!output.stdout.is_empty());
    }
}

/// The CLI ships inside the app bundle, so it reports the application's version rather than
/// its own crate version.
#[test]
fn the_version_is_the_applications() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../package.json");
    let package: Value = serde_json::from_slice(&std::fs::read(package).unwrap()).unwrap();
    let version = package["version"].as_str().unwrap();
    let directory = tempfile::tempdir().unwrap();
    let output = cli(&directory.path().join("cli.toml"))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim_end(),
        format!("nyanpasu-cli {version}")
    );
}

#[test]
fn contexts_survive_between_invocations_and_hold_no_secret_in_the_file() {
    let shell = MockShell::start();
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("cli.toml");

    let mut add = cli(&config)
        .args([
            "context",
            "add",
            "mock",
            "--url",
            &shell.url(),
            "--token-stdin",
        ])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    add.stdin
        .take()
        .unwrap()
        .write_all(b"npk_9_filetoken\n")
        .unwrap();
    assert!(add.wait().unwrap().success());
    assert!(
        !std::fs::read_to_string(&config)
            .unwrap()
            .contains("filetoken"),
        "cli.toml holds no secret"
    );

    let output = cli(&config).args(["--json", "status"]).output().unwrap();
    assert_eq!(
        code(&output),
        0,
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let received = shell.received.lock().unwrap().clone();
    assert!(
        received.iter().all(|request| request
            .headers
            .to_ascii_lowercase()
            .contains("authorization: bearer npk_9_filetoken")),
        "the file-stored token authenticates every request"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;

        let token = directory.path().join("tokens").join("mock.token");
        let mode = std::fs::metadata(token).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
