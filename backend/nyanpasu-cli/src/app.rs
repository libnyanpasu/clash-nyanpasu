//! Command execution: resolve one target, connect once, describe, then run the command.
//!
//! The connector is injected so the whole flow, including target precedence and exit
//! codes, is tested without a shell. There is exactly one connection attempt per
//! invocation: a failure ends the command and never selects another target.

use std::{
    future::Future,
    io::{Read, Write},
};

use clap::CommandFactory as _;
use serde_json::{Value, json};

use crate::{
    cli::{Cli, Command, ContextAdd, ContextCommand, Follow, Plan, parse_json_object},
    commands::{self, CuratedCommand, Invocation},
    config::{CliConfig, CliPaths, ContextConfig, ContextKind, validate_context_name},
    credentials::{TokenVault, clean_token},
    describe::{Capabilities, DESCRIBE_METHOD},
    endpoint,
    error::{CliError, ExitStatus},
    output::{Output, render_human},
    target::{Environment, ResolvedTarget, TargetKind, parse_http_url, resolve, resolve_token},
    transport::{
        Connection, EventFeed, HttpTransport, SocketTransport, StreamKind, Transport, event,
    },
};

/// Opens the one connection of an invocation.
pub trait Connector {
    /// The connection type.
    type Transport: Transport;

    /// Connects to exactly `target`, with `token` as its bearer credential when HTTP.
    ///
    /// # Errors
    /// An unreachable target is reported as such; the caller never tries another.
    fn connect(
        &self,
        target: &ResolvedTarget,
        token: Option<String>,
    ) -> impl Future<Output = Result<Self::Transport, CliError>> + Send;
}

/// The production connector: HTTP, or the validated local endpoint.
pub struct RealConnector;

impl Connector for RealConnector {
    type Transport = Connection;

    async fn connect(
        &self,
        target: &ResolvedTarget,
        token: Option<String>,
    ) -> Result<Connection, CliError> {
        match &target.kind {
            TargetKind::Http { url } => {
                Ok(Connection::Http(HttpTransport::new(url.clone(), token)?))
            }
            TargetKind::LocalSocket { endpoint: location } => {
                let endpoint = endpoint::locate(location.clone()).await?;
                Ok(Connection::Socket(
                    SocketTransport::connect(&endpoint).await?,
                ))
            }
        }
    }
}

/// Everything the commands need from the process, injected by the composition root.
pub struct Services<C> {
    /// Where `cli.toml` and file tokens live.
    pub paths: CliPaths,
    /// Token storage.
    pub vault: TokenVault,
    /// Opens connections.
    pub connector: C,
}

/// The standard streams of one invocation.
pub struct Io<'a> {
    /// Read for `--token-stdin`.
    pub stdin: &'a mut dyn Read,
    /// Results.
    pub stdout: &'a mut dyn Write,
    /// Diagnostics.
    pub stderr: &'a mut dyn Write,
}

/// Runs one invocation and returns the process exit status. Failures are printed to
/// standard error; nothing is printed twice.
pub async fn execute<C: Connector>(
    cli: Cli,
    environment: &Environment,
    services: &Services<C>,
    io: &mut Io<'_>,
) -> ExitStatus {
    match run(cli, environment, services, io).await {
        Ok(status) => status,
        Err(error) => {
            let _ = writeln!(io.stderr, "error: {error}");
            error.exit_status()
        }
    }
}

async fn run<C: Connector>(
    cli: Cli,
    environment: &Environment,
    services: &Services<C>,
    io: &mut Io<'_>,
) -> Result<ExitStatus, CliError> {
    let json = cli.json;
    match &cli.command {
        Command::Context(command) => {
            return context(command, &cli, environment, services, io);
        }
        Command::Completion(arguments) => {
            clap_complete::generate(
                arguments.shell,
                &mut Cli::command(),
                "nyanpasu-cli",
                io.stdout,
            );
            return Ok(ExitStatus::Success);
        }
        _ => {}
    }

    // Validate every argument before connecting, so a typo never costs a connection.
    let plan = cli.command.plan()?;
    let call = match &cli.command {
        Command::Call(arguments) => Some((
            arguments.method.clone(),
            match &arguments.params {
                Some(text) => parse_json_object(text)?,
                None => json!({}),
            },
        )),
        _ => None,
    };

    let config = CliConfig::load(&services.paths.config_file)?;
    let target = resolve(&config, cli.context.as_deref(), cli.transport, environment)?;
    let token = resolve_token(&target, environment, &services.vault)?;
    let transport = services.connector.connect(&target, token).await?;
    let (raw, capabilities) = describe(&transport).await?;

    let mut output = Output {
        json,
        stdout: &mut *io.stdout,
        stderr: &mut *io.stderr,
    };
    if let Some((method, params)) = call {
        capabilities.require(&method)?;
        let value = transport.call(&method, params).await?;
        return Ok(output.result(&value)?);
    }
    if matches!(cli.command, Command::Methods) {
        return methods(&raw, json, &mut output);
    }
    match plan {
        Some(plan) => run_plan(&transport, &capabilities, &plan, &mut output).await,
        None => unreachable!("every connected subcommand has a plan, a call or methods"),
    }
}

/// Calls `rpc_describe` and decodes it, refusing an incompatible protocol.
async fn describe<T: Transport>(transport: &T) -> Result<(Value, Capabilities), CliError> {
    let raw = transport.call(DESCRIBE_METHOD, json!({})).await?;
    let capabilities = Capabilities::from_json(raw.clone())?;
    Ok((raw, capabilities))
}

fn methods(raw: &Value, json: bool, output: &mut Output<'_>) -> Result<ExitStatus, CliError> {
    if json {
        return Ok(output.result(raw)?);
    }
    let scopes = raw
        .get("scopes")
        .and_then(Value::as_array)
        .map(|scopes| {
            scopes
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    writeln!(output.stdout, "scopes: {scopes}")?;
    let commands = raw.get("commands").cloned().unwrap_or_else(|| json!([]));
    let table = render_human(&commands);
    writeln!(output.stdout, "{table}")?;
    Ok(ExitStatus::Success)
}

/// Checks a curated command against the caller's own capabilities.
fn permit(command: &CuratedCommand, capabilities: &Capabilities) -> Result<(), CliError> {
    if !capabilities.holds(command.scope) {
        return Err(CliError::Unauthenticated {
            message: format!(
                "`{}` needs the {} scope, which this caller does not hold",
                command.name, command.scope
            ),
        });
    }
    match command.invocation {
        Invocation::Method(method) => capabilities.require(method).map(drop),
        Invocation::ConnectionDetails => Ok(()),
    }
}

/// Selects a value by a dotted path; array segments are indexes.
pub fn select_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.')
        .filter(|segment| !segment.is_empty())
        .try_fold(value, |current, segment| match current {
            Value::Object(map) => map.get(segment),
            Value::Array(items) => segment
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get(index)),
            _ => None,
        })
}

async fn run_plan<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
    plan: &Plan,
    output: &mut Output<'_>,
) -> Result<ExitStatus, CliError> {
    let command = commands::find(plan.name).expect("a plan names a curated command");
    permit(command, capabilities)?;
    let params = command.params(&plan.input)?;

    let (value, details) = match command.invocation {
        Invocation::Method(method) => (transport.call(method, params).await?, None),
        Invocation::ConnectionDetails => {
            let mut feed = transport.events(StreamKind::ConnectionDetails).await?;
            let first = feed
                .next()
                .await
                .unwrap_or_else(|| Err(CliError::failed("the connection stream ended")))?;
            (first, Some(feed))
        }
    };
    let value = match &plan.select {
        Some(path) => select_path(&value, path)
            .cloned()
            .ok_or_else(|| CliError::usage(format!("no value at `{path}`")))?,
        None => value,
    };
    let status = output.result(&value)?;

    let feed = match (plan.follow, details) {
        (None, _) => return Ok(status),
        (Some(Follow::Connections), Some(feed)) => feed,
        (Some(Follow::Logs), _) => {
            transport
                .events(StreamKind::Event(event::CORE_LOGS_CHANGED))
                .await?
        }
        (Some(Follow::Traffic), _) => transport.events(StreamKind::Event(event::CLASH_WS)).await?,
        (Some(Follow::Connections), None) => {
            transport.events(StreamKind::ConnectionDetails).await?
        }
    };
    follow(feed, output).await?;
    Ok(status)
}

/// Prints pushed values as NDJSON until the stream ends or the user interrupts.
async fn follow(mut feed: EventFeed, output: &mut Output<'_>) -> Result<(), CliError> {
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            item = feed.next() => match item {
                None => return Ok(()),
                Some(Ok(value)) => output.stream_line(&value)?,
                Some(Err(error)) => return Err(error),
            },
        }
    }
}

fn context<C>(
    command: &ContextCommand,
    cli: &Cli,
    environment: &Environment,
    services: &Services<C>,
    io: &mut Io<'_>,
) -> Result<ExitStatus, CliError> {
    let path = &services.paths.config_file;
    let mut config = CliConfig::load(path)?;
    match command {
        ContextCommand::Add(arguments) => {
            let (context, token_backend) = build_context(arguments, environment, services, io)?;
            config.add(
                &arguments.name,
                ContextConfig {
                    token: token_backend,
                    ..context
                },
            )?;
            config.save(path)?;
            if cli.json {
                writeln!(io.stdout, "{}", json!({"added": arguments.name}))?;
            } else {
                writeln!(io.stdout, "added context `{}`", arguments.name)?;
            }
        }
        ContextCommand::Use { name } => {
            config.use_context(name)?;
            config.save(path)?;
            writeln!(io.stdout, "now using context `{name}`")?;
        }
        ContextCommand::List => {
            if cli.json {
                let value = serde_json::to_value(&config)
                    .map_err(|error| CliError::failed(error.to_string()))?;
                writeln!(io.stdout, "{value}")?;
            } else {
                let rows: Vec<Value> = config
                    .contexts
                    .iter()
                    .map(|(name, context)| {
                        json!({
                            "current": if config.current_context.as_deref() == Some(name) { "*" } else { "" },
                            "name": name,
                            "kind": context.kind,
                            "target": context.url.clone().or_else(|| {
                                context.endpoint.as_ref().map(|path| path.display().to_string())
                            }).unwrap_or_else(|| "default".to_owned()),
                            "token": context.token,
                        })
                    })
                    .collect();
                if rows.is_empty() {
                    writeln!(io.stdout, "no contexts; the default local socket is used")?;
                } else {
                    writeln!(io.stdout, "{}", render_human(&Value::Array(rows)))?;
                }
            }
        }
        ContextCommand::Remove { name } => {
            config.remove(name)?;
            services.vault.delete(name)?;
            config.save(path)?;
            writeln!(io.stdout, "removed context `{name}`")?;
        }
    }
    Ok(ExitStatus::Success)
}

/// Builds the context of `context add`, storing its token first when one was given.
fn build_context<C>(
    arguments: &ContextAdd,
    environment: &Environment,
    services: &Services<C>,
    io: &mut Io<'_>,
) -> Result<(ContextConfig, Option<crate::config::TokenBackend>), CliError> {
    validate_context_name(&arguments.name)?;
    match &arguments.url {
        None => {
            // Replacing a context must not leave its old credential behind.
            services.vault.delete(&arguments.name)?;
            Ok((
                ContextConfig {
                    kind: ContextKind::LocalSocket,
                    endpoint: arguments.endpoint.clone(),
                    url: None,
                    token: None,
                },
                None,
            ))
        }
        Some(url) => {
            let url = parse_http_url(url)?;
            let token = if arguments.token_stdin {
                let mut text = String::new();
                io.stdin.read_to_string(&mut text)?;
                Some(clean_token(&text)?)
            } else if arguments.token_from_env {
                let text = environment
                    .token
                    .as_deref()
                    .ok_or_else(|| CliError::usage("NYANPASU_TOKEN is not set"))?;
                Some(clean_token(text)?)
            } else {
                None
            };
            let backend = match token {
                Some(token) => Some(services.vault.save(&arguments.name, &token)?),
                None => {
                    services.vault.delete(&arguments.name)?;
                    None
                }
            };
            Ok((
                ContextConfig {
                    kind: ContextKind::Http,
                    endpoint: None,
                    url: Some(url.to_string()),
                    token: None,
                },
                backend,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use clap::Parser as _;

    use super::*;
    use crate::{
        credentials::fakes::MemoryStore,
        describe::{Scope, fixtures},
        transport::fakes::FakeTransport,
    };

    /// Records every connect, then hands out one scripted transport or a failure.
    struct FakeConnector {
        targets: Mutex<Vec<(ResolvedTarget, Option<String>)>>,
        outcome: Mutex<Option<Result<FakeTransport, CliError>>>,
    }

    impl FakeConnector {
        fn new(outcome: Result<FakeTransport, CliError>) -> Self {
            Self {
                targets: Mutex::default(),
                outcome: Mutex::new(Some(outcome)),
            }
        }
    }

    impl Connector for FakeConnector {
        type Transport = FakeTransport;

        async fn connect(
            &self,
            target: &ResolvedTarget,
            token: Option<String>,
        ) -> Result<FakeTransport, CliError> {
            self.targets.lock().unwrap().push((target.clone(), token));
            self.outcome
                .lock()
                .unwrap()
                .take()
                .expect("one connection attempt per invocation")
        }
    }

    fn describe_reply(scopes: &[Scope], methods: &[(&str, Scope)]) -> Result<Value, CliError> {
        Ok(fixtures::describe_json(scopes, methods))
    }

    struct Harness {
        directory: tempfile::TempDir,
        services: Services<FakeConnector>,
    }

    fn harness(outcome: Result<FakeTransport, CliError>) -> Harness {
        let directory = tempfile::tempdir().unwrap();
        let paths = CliPaths::discover(Some(directory.path().join("cli.toml"))).unwrap();
        let services = Services {
            vault: TokenVault::with_stores(
                Box::new(MemoryStore::new(false)),
                Box::new(MemoryStore::new(true)),
            ),
            paths,
            connector: FakeConnector::new(outcome),
        };
        Harness {
            directory,
            services,
        }
    }

    struct Run {
        status: ExitStatus,
        stdout: String,
        stderr: String,
    }

    async fn invoke(
        harness: &Harness,
        environment: &Environment,
        arguments: &[&str],
        stdin: &str,
    ) -> Run {
        let cli =
            Cli::try_parse_from(std::iter::once("nyanpasu-cli").chain(arguments.iter().copied()))
                .unwrap_or_else(|error| panic!("{arguments:?}: {error}"));
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut stdin = stdin.as_bytes();
        let status = execute(
            cli,
            environment,
            &harness.services,
            &mut Io {
                stdin: &mut stdin,
                stdout: &mut stdout,
                stderr: &mut stderr,
            },
        )
        .await;
        Run {
            status,
            stdout: String::from_utf8(stdout).unwrap(),
            stderr: String::from_utf8(stderr).unwrap(),
        }
    }

    fn all_scopes() -> Vec<Scope> {
        Scope::ALL
            .into_iter()
            .filter(|scope| *scope != Scope::RemoteAdmin)
            .collect()
    }

    fn describe_all() -> Result<Value, CliError> {
        let allowed: Vec<(&str, Scope)> = commands::COMMANDS
            .iter()
            .filter_map(|command| command.method().map(|method| (method, command.scope)))
            .collect();
        describe_reply(&all_scopes(), &allowed)
    }

    #[tokio::test]
    async fn a_query_prints_a_table_and_exits_zero() {
        let transport = FakeTransport::new(vec![
            describe_all(),
            Ok(json!({"running": true, "core": "mihomo"})),
        ]);
        let harness = harness(Ok(transport));
        let run = invoke(&harness, &Environment::default(), &["status"], "").await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        assert!(run.stdout.contains("running"), "{}", run.stdout);

        let (target, token) = harness.services.connector.targets.lock().unwrap()[0].clone();
        assert_eq!(target.kind, TargetKind::LocalSocket { endpoint: None });
        assert_eq!(token, None);
    }

    #[tokio::test]
    async fn json_prints_the_result_unchanged() {
        let transport = FakeTransport::new(vec![describe_all(), Ok(json!({"running": true}))]);
        let harness = harness(Ok(transport));
        let run = invoke(&harness, &Environment::default(), &["--json", "status"], "").await;
        assert_eq!(run.stdout.trim_end(), r#"{"running":true}"#);
    }

    #[tokio::test]
    async fn the_environment_url_selects_an_http_target_with_the_environment_token() {
        let transport = FakeTransport::new(vec![describe_all(), Ok(json!({}))]);
        let harness = harness(Ok(transport));
        let environment = Environment {
            url: Some("http://127.0.0.1:9090".to_owned()),
            token: Some("npk_1_secret".to_owned()),
        };
        let run = invoke(&harness, &environment, &["status"], "").await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        let (target, token) = harness.services.connector.targets.lock().unwrap()[0].clone();
        assert!(matches!(target.kind, TargetKind::Http { .. }));
        assert_eq!(token.as_deref(), Some("npk_1_secret"));
    }

    #[tokio::test]
    async fn a_failed_connection_never_tries_another_target() {
        let harness = harness(Err(CliError::Unreachable {
            target: "the local socket".into(),
            detail: "refused".into(),
        }));
        let environment = Environment {
            url: Some("http://127.0.0.1:1".to_owned()),
            token: None,
        };
        // A current context exists, and so does the default socket; neither is tried.
        invoke(
            &harness,
            &Environment::default(),
            &["context", "add", "other", "--socket"],
            "",
        )
        .await;
        let run = invoke(&harness, &environment, &["status"], "").await;
        assert_eq!(run.status, ExitStatus::Unreachable);
        assert_eq!(harness.services.connector.targets.lock().unwrap().len(), 1);
        assert!(run.stderr.contains("refused"), "{}", run.stderr);
    }

    #[tokio::test]
    async fn an_incompatible_transport_fails_before_connecting() {
        let harness = harness(Err(CliError::failed("must not connect")));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["--transport", "http", "status"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Usage);
        assert!(
            harness
                .services
                .connector
                .targets
                .lock()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn malformed_arguments_fail_before_connecting() {
        let harness = harness(Err(CliError::failed("must not connect")));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["config", "patch", "not json"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Usage);
        assert!(
            harness
                .services
                .connector
                .targets
                .lock()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_degraded_mutation_exits_six_even_with_json() {
        let degraded = json!({
            "status": "committed_degraded", "value": null, "commits": [],
            "notifications_pending": false,
            "degradations": [{"phase": "runtime_build", "message": "late"}]
        });
        for arguments in [
            &["proxy", "select", "G", "N"][..],
            &["--json", "proxy", "select", "G", "N"],
        ] {
            let transport = FakeTransport::new(vec![describe_all(), Ok(degraded.clone())]);
            let harness = harness(Ok(transport));
            let run = invoke(&harness, &Environment::default(), arguments, "").await;
            assert_eq!(run.status, ExitStatus::Degraded, "{arguments:?}");
            assert!(run.stderr.contains("runtime_build: late"), "{}", run.stderr);
        }
    }

    #[tokio::test]
    async fn shell_errors_map_to_their_exit_codes() {
        let remote = |kind: &str| {
            Err(CliError::from_remote(nyanpasu_rpc_wire::RpcError {
                kind: kind.to_owned(),
                message: "nope".to_owned(),
                code: None,
                retryable: None,
                operation_id: None,
                domain_error: None,
            }))
        };
        for (kind, expected) in [
            ("application_error", ExitStatus::Application),
            ("unsupported", ExitStatus::Unsupported),
            ("forbidden", ExitStatus::Unauthenticated),
        ] {
            let transport = FakeTransport::new(vec![describe_all(), remote(kind)]);
            let harness = harness(Ok(transport));
            let run = invoke(&harness, &Environment::default(), &["status"], "").await;
            assert_eq!(run.status, expected, "{kind}");
        }
    }

    #[tokio::test]
    async fn a_denied_scope_never_reaches_the_shell() {
        let transport = FakeTransport::new(vec![describe_reply(
            &[Scope::Read],
            &[
                ("get_core_status", Scope::Read),
                ("restart_sidecar", Scope::System),
            ],
        )]);
        let harness = harness(Ok(transport));
        let run = invoke(&harness, &Environment::default(), &["core", "restart"], "").await;
        assert_eq!(run.status, ExitStatus::Unauthenticated, "{}", run.stderr);
    }

    #[tokio::test]
    async fn a_method_the_transport_does_not_expose_is_unsupported() {
        let transport = FakeTransport::new(vec![describe_reply(
            &[Scope::Read, Scope::System],
            &[("get_core_status", Scope::Read)],
        )]);
        let harness = harness(Ok(transport));
        let run = invoke(&harness, &Environment::default(), &["core", "restart"], "").await;
        assert_eq!(run.status, ExitStatus::Unsupported, "{}", run.stderr);
    }

    #[tokio::test]
    async fn an_incompatible_describe_protocol_is_unsupported() {
        let transport = FakeTransport::new(vec![Ok(json!({"protocol": 9}))]);
        let harness = harness(Ok(transport));
        let run = invoke(&harness, &Environment::default(), &["status"], "").await;
        assert_eq!(run.status, ExitStatus::Unsupported);
    }

    #[tokio::test]
    async fn call_sends_the_given_json_to_an_allowed_method() {
        let transport = FakeTransport::new(vec![describe_all(), Ok(json!(["a"]))]);
        let harness = harness(Ok(transport));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["--json", "call", "get_profiles", r#"{"x":1}"#],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        assert_eq!(run.stdout.trim_end(), r#"["a"]"#);

        let transport = FakeTransport::new(vec![describe_all()]);
        let harness = self::harness(Ok(transport));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["call", "remote_access_status"],
            "",
        )
        .await;
        assert_eq!(
            run.status,
            ExitStatus::Unsupported,
            "not exposed, so never sent"
        );
    }

    #[tokio::test]
    async fn config_get_selects_a_dotted_path() {
        let transport = FakeTransport::new(vec![describe_all(), Ok(json!({"a": {"b": [10, 20]}}))]);
        let harness = harness(Ok(transport));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["--json", "config", "get", "a.b.1"],
            "",
        )
        .await;
        assert_eq!(run.stdout.trim_end(), "20");
    }

    #[tokio::test]
    async fn conn_list_follow_prints_the_first_frame_then_ndjson() {
        let transport = FakeTransport::new(vec![describe_all()])
            .with_feed(vec![Ok(json!({"n": 1})), Ok(json!({"n": 2}))]);
        let harness = harness(Ok(transport));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["--json", "conn", "list", "-f"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        assert_eq!(run.stdout, "{\"n\":1}\n{\"n\":2}\n");
    }

    #[tokio::test]
    async fn traffic_follow_streams_ndjson_after_the_summary() {
        let transport = FakeTransport::new(vec![describe_all(), Ok(json!({"total": 1}))])
            .with_feed(vec![Ok(json!({"up": 5}))]);
        let harness = harness(Ok(transport));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["--json", "traffic", "-f"],
            "",
        )
        .await;
        assert_eq!(run.stdout, "{\"total\":1}\n{\"up\":5}\n");
    }

    #[tokio::test]
    async fn contexts_round_trip_and_tokens_stay_out_of_the_config_file() {
        let harness = harness(Err(CliError::failed("never connects")));
        let environment = Environment::default();

        let run = invoke(
            &harness,
            &environment,
            &[
                "context",
                "add",
                "work",
                "--url",
                "https://work.example:9443",
                "--token-stdin",
            ],
            "npk_1_topsecret\n",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);

        let text = std::fs::read_to_string(&harness.services.paths.config_file).unwrap();
        assert!(!text.contains("topsecret"), "{text}");
        assert!(text.contains("token = \"file\""), "{text}");

        invoke(
            &harness,
            &environment,
            &["context", "add", "local", "--socket"],
            "",
        )
        .await;
        let run = invoke(&harness, &environment, &["context", "list"], "").await;
        assert!(run.stdout.contains("work"), "{}", run.stdout);
        assert!(run.stdout.contains("local"), "{}", run.stdout);

        let run = invoke(&harness, &environment, &["context", "use", "local"], "").await;
        assert_eq!(run.status, ExitStatus::Success);
        let run = invoke(&harness, &environment, &["context", "remove", "work"], "").await;
        assert_eq!(run.status, ExitStatus::Success);
        let run = invoke(&harness, &environment, &["context", "use", "work"], "").await;
        assert_eq!(run.status, ExitStatus::Usage);
    }

    #[tokio::test]
    async fn a_context_token_reaches_the_connector_for_its_http_target() {
        let transport = FakeTransport::new(vec![describe_all(), Ok(json!({}))]);
        let harness = harness(Ok(transport));
        let environment = Environment::default();
        invoke(
            &harness,
            &environment,
            &[
                "context",
                "add",
                "work",
                "--url",
                "http://10.0.0.5:9090",
                "--token-stdin",
            ],
            "npk_1_stored",
        )
        .await;
        let run = invoke(&harness, &environment, &["--context", "work", "status"], "").await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        let (target, token) = harness.services.connector.targets.lock().unwrap()[0].clone();
        assert_eq!(
            target.source,
            crate::target::TargetSource::ContextFlag("work".into())
        );
        assert_eq!(token.as_deref(), Some("npk_1_stored"));
    }

    #[tokio::test]
    async fn token_from_env_requires_the_variable() {
        let harness = harness(Err(CliError::failed("never connects")));
        let run = invoke(
            &harness,
            &Environment::default(),
            &[
                "context",
                "add",
                "x",
                "--url",
                "http://h:1",
                "--token-from-env",
            ],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Usage);
        let environment = Environment {
            url: None,
            token: Some("npk_2_env".to_owned()),
        };
        let run = invoke(
            &harness,
            &environment,
            &[
                "context",
                "add",
                "x",
                "--url",
                "http://h:1",
                "--token-from-env",
            ],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
    }

    #[tokio::test]
    async fn completion_prints_a_script_for_the_shell() {
        let harness = harness(Err(CliError::failed("never connects")));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["completion", "bash"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success);
        assert!(run.stdout.contains("nyanpasu-cli"), "{}", run.stdout);
        drop(harness.directory);
    }

    #[test]
    fn select_path_walks_objects_and_arrays() {
        let value = json!({"a": {"b": [1, {"c": true}]}});
        assert_eq!(select_path(&value, "a.b.1.c"), Some(&json!(true)));
        assert_eq!(select_path(&value, "a.x"), None);
        assert_eq!(select_path(&value, ""), Some(&value));
    }
}
