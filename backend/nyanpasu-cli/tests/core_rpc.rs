//! Core-backed verification of the curated command table.
//!
//! The table in `nyanpasu_cli::commands` is handwritten, so a renamed method, a changed
//! parameter or a command that left the local allowlist would otherwise surface only in a
//! user's terminal. These tests run the CLI's real connector against a real
//! `NyanpasuClient` served by the real local-socket server, and call every curated
//! operation with a schema-shaped input.
//!
//! The Unix endpoint rules (a private directory, a socket path) are what the fixture
//! builds, so the file is Unix-only; the Windows pipe has its own endpoint tests.
#![cfg(unix)]

use std::{os::unix::fs::DirBuilderExt as _, path::PathBuf, time::Duration};

use clap::Parser as _;
use nyanpasu_cli::{
    app::{Connector as _, Io, RealConnector, Services, execute},
    cli::Cli,
    commands::{self, Invocation},
    config::CliPaths,
    credentials::TokenVault,
    describe::{Capabilities, DESCRIBE_METHOD, Scope},
    error::{CliError, ExitStatus},
    mcp::{McpOptions, RAW_TOOL, serve},
    target::{Environment, ResolvedTarget, TargetKind, TargetSource},
    transport::{StreamKind, Transport, event},
};
use nyanpasu_rpc::{
    local::{
        ENDPOINT_NAMESPACE,
        server::{ConnectionLimits, LocalServer},
    },
    test_support::Fixture,
};
use nyanpasu_rpc_wire::RpcErrorKind;
use nyanpasu_single_instance::endpoint::{Endpoint, EndpointListener};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

const WATCHDOG: Duration = Duration::from_secs(30);

struct Shell {
    fixture: Fixture,
    shutdown: CancellationToken,
    server: tokio::task::JoinHandle<()>,
    socket: PathBuf,
    _directory: tempfile::TempDir,
}

impl Shell {
    async fn start() -> Self {
        let fixture = Fixture::new(false).await;
        let directory = tempfile::tempdir().unwrap();
        let run = directory.path().canonicalize().unwrap().join("run");
        std::fs::DirBuilder::new().mode(0o700).create(&run).unwrap();
        let socket = run.join("rpc.sock");
        let endpoint = Endpoint::discover(ENDPOINT_NAMESPACE, directory.path(), Some(&socket))
            .expect("a private directory is a valid endpoint location");
        let listener = EndpointListener::bind(&endpoint).await.unwrap();
        let shutdown = CancellationToken::new();
        let server = tokio::spawn({
            let server = LocalServer::new(fixture.rpc.clone(), ConnectionLimits::default());
            let shutdown = shutdown.clone();
            async move { server.serve(listener, shutdown).await.unwrap() }
        });
        Self {
            fixture,
            shutdown,
            server,
            socket,
            _directory: directory,
        }
    }

    fn target(&self) -> ResolvedTarget {
        ResolvedTarget {
            source: TargetSource::DefaultSocket,
            kind: TargetKind::LocalSocket {
                endpoint: Some(self.socket.clone()),
            },
            context: None,
            token_backend: None,
        }
    }

    async fn stop(self) {
        self.shutdown.cancel();
        tokio::time::timeout(WATCHDOG, self.server)
            .await
            .expect("the server drains")
            .unwrap();
        self.fixture.shutdown().await;
    }
}

/// One CLI invocation against the shell's socket, through the production code path.
struct CliRun {
    status: ExitStatus,
    stdout: String,
    stderr: String,
}

impl CliRun {
    fn json(&self) -> Value {
        serde_json::from_str(self.stdout.trim())
            .unwrap_or_else(|error| panic!("not JSON ({error}): {}", self.stdout))
    }
}

impl Shell {
    /// Runs `nyanpasu-cli <arguments>` with a context that names this shell's socket.
    async fn cli(&self, arguments: &[&str]) -> CliRun {
        let directory = self._directory.path();
        let config = directory.join("cli.toml");
        let paths = CliPaths::discover(Some(config.clone())).unwrap();
        let services = Services {
            vault: TokenVault::new(directory.join("tokens"), false),
            paths,
            connector: RealConnector,
            home: None,
            executable: None,
            skill_dir: None,
            appimage: None,
            search_path: None,
        };
        let socket = self.socket.display().to_string();
        let mut full = vec!["nyanpasu-cli", "--config"];
        let config_text = config.display().to_string();
        full.push(&config_text);
        full.extend(["context", "add", "test", "--socket", "--endpoint", &socket]);
        run_cli(&services, &full).await;
        let mut full = vec![
            "nyanpasu-cli",
            "--config",
            &config_text,
            "--context",
            "test",
        ];
        full.extend_from_slice(arguments);
        run_cli(&services, &full).await
    }
}

async fn run_cli(services: &Services<RealConnector>, arguments: &[&str]) -> CliRun {
    let cli =
        Cli::try_parse_from(arguments).unwrap_or_else(|error| panic!("{arguments:?}: {error}"));
    let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
    let mut stdin: &[u8] = &[];
    let status = tokio::time::timeout(
        WATCHDOG,
        execute(
            cli,
            &Environment::default(),
            services,
            &mut Io {
                stdin: &mut stdin,
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        ),
    )
    .await
    .expect("the command finishes");
    CliRun {
        status,
        stdout: String::from_utf8(stdout).unwrap(),
        stderr: String::from_utf8(stderr).unwrap(),
    }
}

/// An input object that satisfies the entry's `required` fields.
fn sample_input(name: &str, schema: &Value) -> Value {
    let mut input = serde_json::Map::new();
    let properties = schema["properties"].as_object();
    for field in schema["required"].as_array().into_iter().flatten() {
        let field = field.as_str().unwrap();
        let kind = properties
            .and_then(|properties| properties.get(field))
            .and_then(|property| property["type"].as_str());
        let value = match (name, field, kind) {
            ("mode_set", "mode", _) => json!("rule"),
            ("core_switch", "core", _) => json!("mihomo"),
            (_, _, Some("boolean")) => json!(true),
            (_, _, Some("integer")) => json!(1),
            (_, _, Some("array")) => json!(["x"]),
            (_, _, Some("object")) => json!({}),
            _ => json!("x"),
        };
        input.insert(field.to_owned(), value);
    }
    Value::Object(input)
}

#[tokio::test]
async fn the_cli_and_the_shell_agree_on_the_endpoint_namespace() {
    assert_eq!(nyanpasu_cli::endpoint::RPC_NAMESPACE, ENDPOINT_NAMESPACE);
}

#[tokio::test]
async fn every_curated_command_reaches_a_real_local_handler() {
    let shell = Shell::start().await;
    let transport = RealConnector
        .connect(&shell.target(), None)
        .await
        .expect("the CLI connects to the real local socket");
    let capabilities =
        Capabilities::from_json(transport.call(DESCRIBE_METHOD, json!({})).await.unwrap()).unwrap();

    let mut called = 0;
    for command in commands::COMMANDS {
        let Invocation::Method(method) = command.invocation else {
            continue;
        };
        capabilities
            .require(method)
            .unwrap_or_else(|error| panic!("{} ({method}): {error}", command.name));

        let input = sample_input(command.name, &command.input_schema_value());
        let params = command.params(&input).unwrap();
        match transport.call(method, params).await {
            Ok(_) => {}
            // A fixture core without a running Clash may refuse; the command was still
            // admitted, decoded and run by its real handler.
            Err(CliError::Remote { error })
                if error.error_kind() == RpcErrorKind::ApplicationError => {}
            Err(error) => panic!(
                "{} ({method}) did not reach its handler: {error}",
                command.name
            ),
        }
        called += 1;
    }
    assert!(called > 20, "only {called} curated commands were exercised");
    shell.stop().await;
}

#[tokio::test]
async fn an_uncurated_command_outside_the_allowlist_is_refused_by_the_shell() {
    let shell = Shell::start().await;
    let transport = RealConnector.connect(&shell.target(), None).await.unwrap();
    let capabilities =
        Capabilities::from_json(transport.call(DESCRIBE_METHOD, json!({})).await.unwrap()).unwrap();
    for method in [
        "get_app_config",
        "patch_clash_config",
        "create_http_launch_url",
        // The network rules name the user's Wi-Fi networks and gateways, and applying them
        // switches the proxy, TUN and transparent proxy: they stay with the desktop.
        "get_network_automation_status",
        "apply_network_automation",
        "request_wifi_name_access",
        // The settings action that links the CLI onto `PATH` runs the bundled executable.
        "install_cli",
    ] {
        assert!(
            matches!(
                capabilities.require(method),
                Err(CliError::Unsupported { .. })
            ),
            "{method} must not be advertised on the local socket"
        );
        match transport.call(method, json!({})).await {
            Err(CliError::Remote { error }) => {
                assert_eq!(error.error_kind(), RpcErrorKind::Unsupported, "{method}");
            }
            other => panic!("{method} must be refused, got {other:?}"),
        }
    }
    shell.stop().await;
}

#[tokio::test]
async fn the_local_principal_holds_remote_admin_for_its_reviewed_commands_only() {
    let shell = Shell::start().await;
    let transport = RealConnector.connect(&shell.target(), None).await.unwrap();
    let described = transport.call(DESCRIBE_METHOD, json!({})).await.unwrap();
    assert_eq!(described["transport"], "local");
    assert_eq!(described["principal"]["kind"], "local_socket");
    let capabilities = Capabilities::from_json(described).unwrap();
    assert!(capabilities.holds(Scope::RemoteAdmin));
    let mut administration: Vec<_> = capabilities
        .commands()
        .iter()
        .filter(|command| command.scope == Scope::RemoteAdmin)
        .map(|command| command.name.as_str())
        .collect();
    administration.sort_unstable();
    assert_eq!(
        administration,
        [
            "create_remote_access_token",
            "get_local_socket_status",
            "list_remote_access_tokens",
            "list_remote_browser_sessions",
            "revoke_remote_access_token",
            "revoke_remote_browser_session",
            "set_remote_access",
        ],
        "the launch URL is a browser credential and stays with the desktop"
    );
    shell.stop().await;
}

#[tokio::test]
async fn following_the_clash_stream_starts_from_a_full_reset() {
    let shell = Shell::start().await;
    let transport = RealConnector.connect(&shell.target(), None).await.unwrap();
    let mut feed = transport
        .events(StreamKind::Event(event::CLASH_WS))
        .await
        .expect("the local socket serves the Clash stream");
    let first = tokio::time::timeout(WATCHDOG, feed.next())
        .await
        .expect("an initial value arrives")
        .expect("the stream is open")
        .expect("the initial value is not an error");
    assert!(first.get("sequence").is_some(), "{first}");
    drop(feed);
    shell.stop().await;
}

#[tokio::test]
async fn remote_administration_runs_end_to_end_against_a_real_core() {
    let shell = Shell::start().await;

    let status = shell.cli(&["--json", "remote", "status"]).await;
    assert_eq!(status.status, ExitStatus::Success, "{}", status.stderr);
    let status = status.json();
    assert_eq!(status["settings"]["http"]["enabled"], false);
    assert_eq!(status["settings"]["local_socket"]["enabled"], false);
    // This fixture has no local-socket owner to report, which the status says outright.
    assert!(
        status["local_socket"]["unavailable"].is_string(),
        "{status}"
    );

    // A token's secret is shown once, and listings never carry it.
    let created = shell
        .cli(&[
            "--json",
            "remote",
            "token",
            "create",
            "ci",
            "--scope",
            "read",
            "--scope",
            "proxy",
            "--expires-in",
            "1h",
        ])
        .await;
    assert_eq!(created.status, ExitStatus::Success, "{}", created.stderr);
    let created = created.json();
    let secret = created["token"].as_str().unwrap().to_owned();
    assert!(secret.starts_with("npk_"), "{secret}");
    let id = created["summary"]["id"].as_str().unwrap().to_owned();
    assert_eq!(created["summary"]["scopes"], json!(["read", "proxy"]));
    assert!(created["summary"]["expires_at_ms"].is_i64(), "{created}");
    let listed = shell.cli(&["--json", "remote", "token", "list"]).await;
    assert_eq!(listed.json().as_array().unwrap().len(), 1);
    assert!(!listed.stdout.contains(&secret));
    let revoked = shell.cli(&["remote", "token", "revoke", &id]).await;
    assert_eq!(revoked.status, ExitStatus::Success, "{}", revoked.stderr);
    let listed = shell.cli(&["--json", "remote", "token", "list"]).await;
    assert_eq!(listed.json(), json!([]));
    let again = shell.cli(&["remote", "token", "revoke", &id]).await;
    assert_eq!(again.status, ExitStatus::Application, "{}", again.stderr);

    let sessions = shell.cli(&["--json", "remote", "session", "list"]).await;
    assert_eq!(sessions.json(), json!([]));

    // The fixture's HTTP listener cannot start: the setting is committed and the exit says so.
    let on = shell.cli(&["remote", "http", "on"]).await;
    assert_eq!(on.status, ExitStatus::Degraded, "{}", on.stderr);
    let status = shell.cli(&["--json", "remote", "status"]).await.json();
    assert_eq!(status["settings"]["http"]["enabled"], true, "{status}");
    let off = shell.cli(&["remote", "http", "off"]).await;
    assert_eq!(off.status, ExitStatus::Success, "{}", off.stderr);
    let open = shell
        .cli(&["remote", "http", "strategy", "local_open"])
        .await;
    assert_eq!(open.status, ExitStatus::Success, "{}", open.stderr);
    assert!(open.stderr.contains("every OS user"), "{}", open.stderr);
    let status = shell.cli(&["--json", "remote", "status"]).await.json();
    assert_eq!(status["settings"]["http"]["enabled"], false, "{status}");
    assert_eq!(status["settings"]["http"]["strategy"], "local_open");

    // Without a local-socket owner the switch cannot follow; nothing is committed.
    let socket = shell.cli(&["remote", "socket", "on"]).await;
    assert_eq!(socket.status, ExitStatus::Application, "{}", socket.stderr);
    shell.stop().await;
}

/// `config patch` takes the same ordinary-preferences contract as on the other transports, so
/// the socket can change ordinary settings and still cannot reach another class.
#[tokio::test]
async fn config_patch_works_over_the_socket_and_keeps_its_privilege_separation() {
    let shell = Shell::start().await;
    let patched = shell
        .cli(&["config", "patch", r#"{"proxy_layout_column": 5}"#])
        .await;
    assert_eq!(patched.status, ExitStatus::Success, "{}", patched.stderr);
    let column = shell
        .cli(&["--json", "config", "get", "proxy_layout_column"])
        .await;
    assert_eq!(column.json(), json!(5), "{}", column.stderr);

    for refused in [
        r#"{"remote_access": {"local_socket": {"enabled": true}}}"#,
        r#"{"enable_service_mode": true}"#,
        r#"{"hotkeys": []}"#,
        // Network rules are a `system` setting: a rule writes the proxy switches.
        r#"{"network_automation": {"enabled": true}}"#,
    ] {
        let run = shell.cli(&["config", "patch", refused]).await;
        assert_eq!(run.status, ExitStatus::Usage, "{refused}: {}", run.stderr);
    }
    shell.stop().await;
}

/// Runs `nyanpasu-cli mcp` against the shell: the production connector and describe, then the
/// stdio server over in-memory streams. Returns one reply per request that has an id.
async fn mcp_session(shell: &Shell, options: &McpOptions, requests: &[Value]) -> Vec<Value> {
    let transport = RealConnector.connect(&shell.target(), None).await.unwrap();
    let capabilities =
        Capabilities::from_json(transport.call(DESCRIBE_METHOD, json!({})).await.unwrap()).unwrap();
    let input: String = requests
        .iter()
        .map(|request| format!("{request}\n"))
        .collect();
    let mut output = Vec::new();
    tokio::time::timeout(
        WATCHDOG,
        serve(
            &transport,
            &capabilities,
            options,
            input.as_bytes(),
            &mut output,
        ),
    )
    .await
    .expect("the MCP session ends with its input")
    .unwrap();
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn mcp_over_the_local_socket_exits_when_the_shell_closes_it() {
    let shell = Shell::start().await;
    let transport = RealConnector.connect(&shell.target(), None).await.unwrap();
    let capabilities =
        Capabilities::from_json(transport.call(DESCRIBE_METHOD, json!({})).await.unwrap()).unwrap();
    let (reader, _stdin) = tokio::io::duplex(64);
    let session = tokio::spawn(async move {
        let mut output = Vec::new();
        serve(
            &transport,
            &capabilities,
            &McpOptions::default(),
            tokio::io::BufReader::new(reader),
            &mut output,
        )
        .await?;
        Ok::<_, std::io::Error>(output)
    });

    let Shell {
        fixture,
        shutdown,
        server,
        ..
    } = shell;
    shutdown.cancel();
    tokio::time::timeout(WATCHDOG, server)
        .await
        .expect("the shell socket closes")
        .unwrap();
    let output = tokio::time::timeout(WATCHDOG, session)
        .await
        .expect("the MCP session observes the socket close")
        .expect("the MCP task does not panic")
        .expect("the MCP session exits normally");
    assert!(output.is_empty());
    fixture.shutdown().await;
}

fn tool_names(reply: &Value) -> Vec<String> {
    reply["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("not a tools/list reply: {reply}"))
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect()
}

fn tool_call(id: u64, name: &str, arguments: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
           "params": {"name": name, "arguments": arguments}})
}

/// The tool list is the curated table intersected with what the real shell allows, and every
/// offered tool reaches its real handler: a stale method name or schema fails here.
#[tokio::test]
async fn every_mcp_tool_reaches_a_real_local_handler() {
    let shell = Shell::start().await;
    let options = McpOptions {
        scopes: Scope::ALL
            .into_iter()
            .filter(|scope| *scope != Scope::RemoteAdmin)
            .collect(),
        raw: false,
    };
    let list = json!({"jsonrpc": "2.0", "id": 0, "method": "tools/list"});
    let replies = mcp_session(&shell, &options, &[list]).await;
    let mut offered = tool_names(&replies[0]);
    offered.sort_unstable();
    let mut curated: Vec<String> = commands::COMMANDS
        .iter()
        .map(|command| command.name.to_owned())
        .collect();
    curated.sort_unstable();
    assert_eq!(offered, curated, "the shell allows every curated tool");

    for (command, id) in commands::COMMANDS.iter().zip(1..) {
        // The connection stream sends nothing while the fixture has no running Clash; the
        // bounded wait is covered by a unit test.
        if command.name == "conn_list" {
            continue;
        }
        let input = sample_input(command.name, &command.input_schema_value());
        let replies = tokio::time::timeout(
            Duration::from_secs(10),
            mcp_session(&shell, &options, &[tool_call(id, command.name, input)]),
        )
        .await
        .unwrap_or_else(|_| panic!("{} never answered", command.name));
        let reply = &replies[0];
        assert!(reply.get("error").is_none(), "{}: {reply}", command.name);
        // A fixture core without a running Clash may refuse the operation. The tool was
        // still admitted, decoded and run, which is what a stale table would break.
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        if reply["result"]["isError"] == true {
            for stale in [
                "unsupported on this target",
                "not authenticated",
                "unknown_method",
            ] {
                assert!(!text.contains(stale), "{} is stale: {text}", command.name);
            }
        }
    }
    shell.stop().await;
}

#[tokio::test]
async fn the_default_mcp_session_offers_only_read_tools_and_refuses_the_rest() {
    let shell = Shell::start().await;
    let replies = mcp_session(
        &shell,
        &McpOptions::default(),
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
            tool_call(2, "core_restart", json!({})),
            tool_call(3, "profile_show", json!({"uid": "x"})),
            tool_call(
                4,
                RAW_TOOL,
                json!({"method": "get_configuration_status_summary"}),
            ),
        ],
    )
    .await;
    let mut offered = tool_names(&replies[0]);
    offered.sort_unstable();
    let mut read_only: Vec<String> = commands::COMMANDS
        .iter()
        .filter(|command| command.scope == Scope::Read)
        .map(|command| command.name.to_owned())
        .collect();
    read_only.sort_unstable();
    assert_eq!(offered, read_only);
    for refused in &replies[1..] {
        assert!(
            refused["error"].is_object(),
            "not offered, not callable: {refused}"
        );
    }
    shell.stop().await;
}

/// The desktop-only network commands are not on the local allowlist, so no MCP session can
/// offer them, with every scope requested and `--raw`; the settings view an agent reads
/// carries a flag instead of the rules.
#[tokio::test]
async fn mcp_never_offers_the_desktop_only_network_commands() {
    let shell = Shell::start().await;
    let options = McpOptions {
        scopes: Scope::ALL.to_vec(),
        raw: true,
    };
    let desktop_only = [
        "get_network_automation_status",
        "apply_network_automation",
        "request_wifi_name_access",
        "install_cli",
    ];
    let mut requests = vec![json!({"jsonrpc": "2.0", "id": 0, "method": "tools/list"})];
    for (id, method) in (1..).zip(desktop_only) {
        requests.push(tool_call(id, RAW_TOOL, json!({"method": method})));
    }
    let replies = mcp_session(&shell, &options, &requests).await;

    let tools = replies[0]["result"]["tools"].as_array().unwrap();
    let raw = tools
        .iter()
        .find(|tool| tool["name"] == RAW_TOOL)
        .expect("call_method is offered with --raw");
    let description = raw["description"].as_str().unwrap();
    for method in desktop_only {
        assert!(!description.contains(method), "{method} is advertised");
        assert!(
            !tool_names(&replies[0]).iter().any(|name| name == method),
            "{method} has a tool of its own"
        );
    }
    for (method, reply) in desktop_only.iter().zip(&replies[1..]) {
        assert_eq!(reply["result"]["isError"], true, "{method}: {reply}");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("unsupported on this target"),
            "{method} was refused for the wrong reason: {text}"
        );
    }

    let settings = shell.cli(&["--json", "config", "get"]).await.json();
    assert_eq!(settings["network_automation_enabled"], false, "{settings}");
    for private in ["network_automation\"", "rules", "wifi_ssid", "gateway_mac"] {
        assert!(
            !settings.to_string().contains(private),
            "{private} reached the read view: {settings}"
        );
    }
    shell.stop().await;
}

/// `--raw` is bounded by the session's scopes and can never administer remote access, even
/// when every scope is requested and the local principal holds `remote_admin`.
#[tokio::test]
async fn mcp_raw_never_reaches_remote_administration_on_a_real_core() {
    let shell = Shell::start().await;
    let transport = RealConnector.connect(&shell.target(), None).await.unwrap();
    let described = transport.call(DESCRIBE_METHOD, json!({})).await.unwrap();
    let capabilities = Capabilities::from_json(described).unwrap();
    assert!(
        capabilities.holds(Scope::RemoteAdmin),
        "the premise of this test"
    );
    let administration: Vec<String> = capabilities
        .commands()
        .iter()
        .filter(|command| command.scope == Scope::RemoteAdmin)
        .map(|command| command.name.clone())
        .collect();
    assert!(administration.len() >= 7, "{administration:?}");

    let options = McpOptions {
        scopes: Scope::ALL.to_vec(),
        raw: true,
    };
    let mut requests = vec![json!({"jsonrpc": "2.0", "id": 0, "method": "tools/list"})];
    for (id, method) in (1..).zip(&administration) {
        let params = match method.as_str() {
            "set_remote_access" => json!({"patch": {"local_socket": {"enabled": true}}}),
            "create_remote_access_token" => json!({"name": "agent", "scopes": ["read"]}),
            _ => json!({}),
        };
        requests.push(tool_call(
            id,
            RAW_TOOL,
            json!({"method": method, "params": params}),
        ));
    }
    let replies = mcp_session(&shell, &options, &requests).await;

    let raw = replies[0]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == RAW_TOOL)
        .expect("call_method is offered with --raw");
    let description = raw["description"].as_str().unwrap();
    for method in &administration {
        assert!(
            !description.contains(method.as_str()),
            "{method} is advertised"
        );
        assert!(
            !tool_names(&replies[0]).contains(method),
            "{method} has a tool of its own"
        );
    }
    for (method, reply) in administration.iter().zip(&replies[1..]) {
        assert_eq!(reply["result"]["isError"], true, "{method}: {reply}");
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("this MCP session does not offer"),
            "{method} was refused for the wrong reason: {text}"
        );
    }

    // Nothing was changed behind the refusals.
    let status = shell.cli(&["--json", "remote", "status"]).await.json();
    assert_eq!(
        status["settings"]["local_socket"]["enabled"], false,
        "{status}"
    );
    let tokens = shell.cli(&["--json", "remote", "token", "list"]).await;
    assert_eq!(tokens.json(), json!([]));

    // The same session can still use the raw tool for what its scopes allow.
    let replies = mcp_session(
        &shell,
        &options,
        &[tool_call(
            1,
            RAW_TOOL,
            json!({"method": "get_configuration_status_summary"}),
        )],
    )
    .await;
    assert_eq!(replies[0]["result"]["isError"], false, "{}", replies[0]);
    shell.stop().await;
}
