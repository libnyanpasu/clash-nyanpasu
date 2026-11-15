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

use nyanpasu_cli::{
    app::{Connector as _, RealConnector},
    commands::{self, Invocation},
    describe::{Capabilities, DESCRIBE_METHOD, Scope},
    error::CliError,
    target::{ResolvedTarget, TargetKind, TargetSource},
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

/// Curated operations the local allowlist deliberately does not expose. Each is still a
/// valid HTTP command; over the socket the CLI reports `unsupported` instead of calling.
const NOT_ON_THE_LOCAL_SOCKET: &[&str] = &["config_patch"];

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
        if NOT_ON_THE_LOCAL_SOCKET.contains(&command.name) {
            assert!(
                matches!(
                    capabilities.require(method),
                    Err(CliError::Unsupported { .. })
                ),
                "{} is expected to be unsupported over the socket",
                command.name
            );
            continue;
        }
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
    for method in ["set_remote_access", "patch_app_config", "get_app_config"] {
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
async fn the_local_principal_describes_itself_without_exposing_remote_admin() {
    let shell = Shell::start().await;
    let transport = RealConnector.connect(&shell.target(), None).await.unwrap();
    let described = transport.call(DESCRIBE_METHOD, json!({})).await.unwrap();
    assert_eq!(described["transport"], "local");
    assert_eq!(described["principal"]["kind"], "local_socket");
    let capabilities = Capabilities::from_json(described).unwrap();
    for command in capabilities.commands() {
        assert_ne!(
            command.scope,
            Scope::RemoteAdmin,
            "{} is remote_admin and must stay off the local socket",
            command.name
        );
    }
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
