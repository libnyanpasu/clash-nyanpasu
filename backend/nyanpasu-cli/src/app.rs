//! Command execution: resolve one target, connect once, describe, then run the command.
//!
//! The connector is injected so the whole flow, including target precedence and exit
//! codes, is tested without a shell. There is exactly one connection attempt per
//! invocation: a failure ends the command and never selects another target.

use std::{
    ffi::OsString,
    future::Future,
    io::{Read, Write},
    path::PathBuf,
};

use clap::CommandFactory as _;
use nyanpasu_contracts::auth::TlsFingerprint;
use reqwest::Url;
use serde_json::{Value, json};

use crate::{
    cli::{
        Cli, Command, ContextAdd, ContextCommand, Follow, InstallArgs, Plan, SkillCommand,
        parse_json_object,
    },
    commands::{self, CuratedCommand, Invocation},
    config::{CliConfig, CliPaths, ContextConfig, ContextKind, validate_context_name},
    credentials::{TokenVault, clean_token},
    describe::{Capabilities, DESCRIBE_METHOD},
    endpoint,
    error::{CliError, ExitStatus},
    install::{self, AppImage},
    mcp::{self, McpOptions},
    output::{Output, render_human},
    remote, skill,
    target::{Environment, ResolvedTarget, TargetKind, parse_http_url, resolve, resolve_token},
    transport::{
        Connection, EventFeed, HttpTransport, SocketTransport, StreamKind, Transport, event,
        probe_certificate,
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

    /// The fingerprint of the certificate the HTTPS server at `base` presents, which is not
    /// trusted yet. `context add` shows it so the user can compare and pin it.
    ///
    /// # Errors
    /// The server cannot be reached or does not speak TLS.
    fn probe_certificate(
        &self,
        base: &Url,
    ) -> impl Future<Output = Result<TlsFingerprint, CliError>> + Send;
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
            TargetKind::Http { url } => Ok(Connection::Http(HttpTransport::new(
                url.clone(),
                token,
                target.fingerprint,
            )?)),
            TargetKind::LocalSocket { endpoint: location } => {
                let endpoint = endpoint::locate(location.clone()).await?;
                Ok(Connection::Socket(
                    SocketTransport::connect(&endpoint).await?,
                ))
            }
        }
    }

    async fn probe_certificate(&self, base: &Url) -> Result<TlsFingerprint, CliError> {
        probe_certificate(base).await
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
    /// The user's home directory, for `skill install`.
    pub home: Option<PathBuf>,
    /// This executable, to find the skill folder shipped beside it.
    pub executable: Option<PathBuf>,
    /// An explicit skill folder, which wins over the shipped one.
    pub skill_dir: Option<PathBuf>,
    /// The AppImage this process runs from, whose temporary mount must not be linked to.
    pub appimage: Option<AppImage>,
    /// The `PATH` of this process, to tell whether `install` linked into it.
    pub search_path: Option<OsString>,
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
            return context(command, &cli, environment, services, io).await;
        }
        Command::Install(arguments) => return install_command(arguments, json, services, io),
        Command::Skill(command) => return skill_command(command, json, services, io),
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
    if matches!(cli.command, Command::Remote(_)) {
        remote::require_local_socket(&target)?;
    }
    let token = resolve_token(&target, environment, &services.vault)?;
    let transport = services.connector.connect(&target, token).await?;
    let (raw, capabilities) = describe(&transport).await?;

    if let Command::Mcp(arguments) = &cli.command {
        // Standard input and output belong to the protocol from here on: nothing else may
        // be printed to standard output, and `Io` is not used.
        let options = McpOptions {
            scopes: arguments.scopes.clone(),
            raw: arguments.raw,
        };
        mcp::serve(
            &transport,
            &capabilities,
            &options,
            tokio::io::BufReader::new(tokio::io::stdin()),
            tokio::io::stdout(),
        )
        .await?;
        return Ok(ExitStatus::Success);
    }

    let mut output = Output {
        json,
        stdout: &mut *io.stdout,
        stderr: &mut *io.stderr,
    };
    if let Command::Remote(command) = &cli.command {
        return remote::run(command, &transport, &capabilities, &mut output).await;
    }
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
        None => unreachable!(
            "every connected subcommand has a plan, a call, `remote`, `mcp` or methods"
        ),
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

async fn context<C: Connector>(
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
            let (context, token_backend) =
                build_context(arguments, environment, services, io).await?;
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
                            "pinned": if context.fingerprint.is_some() { "yes" } else { "" },
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

/// The AppImage this invocation runs from, if it does.
fn running_appimage<C>(services: &Services<C>) -> Option<&AppImage> {
    let executable = services.executable.as_deref()?;
    services
        .appimage
        .as_ref()
        .filter(|appimage| appimage.runs(executable))
}

fn install_command<C>(
    arguments: &InstallArgs,
    json: bool,
    services: &Services<C>,
    io: &mut Io<'_>,
) -> Result<ExitStatus, CliError> {
    if cfg!(windows) {
        return Err(CliError::Unsupported {
            message: "the Windows installer puts nyanpasu-cli on PATH".to_owned(),
        });
    }
    let executable = services
        .executable
        .as_deref()
        .ok_or_else(|| CliError::failed("the path of this executable is unknown"))?;
    let mut target = executable
        .canonicalize()
        .unwrap_or_else(|_| executable.to_owned());
    if let Some(appimage) = running_appimage(services) {
        // The mount is gone once the app exits, so link a copy that stays.
        let skill = skill::locate(services.skill_dir.as_deref(), Some(&target)).ok();
        target = appimage.standalone_executable(&target)?;
        if let Some(skill) = skill {
            appimage.standalone_skill(&skill)?;
        }
    }
    let directory = match &arguments.dir {
        Some(directory) => directory.clone(),
        None => services
            .home
            .as_deref()
            .ok_or_else(|| CliError::failed("the home directory is unknown"))?
            .join(".local")
            .join("bin"),
    };
    let installed = install::install(
        &target,
        &directory,
        services.search_path.as_deref(),
        arguments.force,
    )?;
    if json {
        let text = serde_json::to_string(&installed)
            .map_err(|error| CliError::failed(format!("cannot encode the result: {error}")))?;
        writeln!(io.stdout, "{text}")?;
        return Ok(ExitStatus::Success);
    }
    let verb = if installed.already_linked {
        "already links"
    } else {
        "now links"
    };
    writeln!(
        io.stdout,
        "{} {verb} {}",
        installed.link.display(),
        installed.target.display()
    )?;
    if !installed.on_path {
        writeln!(
            io.stdout,
            "{} is not on your PATH. Add it to your shell profile, for example: \
             export PATH=\"{}:$PATH\"",
            directory.display(),
            directory.display()
        )?;
    }
    writeln!(
        io.stdout,
        "To install it for every user instead, run: {}",
        installed.system_command
    )?;
    Ok(ExitStatus::Success)
}

fn skill_command<C>(
    command: &SkillCommand,
    json: bool,
    services: &Services<C>,
    io: &mut Io<'_>,
) -> Result<ExitStatus, CliError> {
    let mut folder = skill::locate(
        services.skill_dir.as_deref(),
        services.executable.as_deref(),
    )?;
    if let Some(appimage) = running_appimage(services) {
        // Agents link the folder, so it must outlive the mount.
        folder = appimage.standalone_skill(&folder)?;
    }
    match command {
        SkillCommand::Path => {
            if json {
                writeln!(io.stdout, "{}", json!({"path": folder}))?;
            } else {
                writeln!(io.stdout, "{}", folder.display())?;
            }
        }
        SkillCommand::Install { agent, force } => {
            let home = services
                .home
                .as_deref()
                .ok_or_else(|| CliError::failed("the home directory is unknown"))?;
            let directory = skill::agent_directory(home, *agent);
            match skill::install(&folder, &directory, *force)? {
                skill::InstallOutcome::Linked(path) => {
                    writeln!(
                        io.stdout,
                        "linked {} -> {}",
                        path.display(),
                        folder.display()
                    )?;
                }
                skill::InstallOutcome::AlreadyLinked(path) => {
                    writeln!(io.stdout, "{} already links the skill", path.display())?;
                }
                skill::InstallOutcome::Copied(path) => {
                    writeln!(
                        io.stdout,
                        "copied the skill to {}; symlinks are not permitted here, so the copy does \
                         not follow app updates: delete that folder and rerun this command \
                         after updating",
                        path.display()
                    )?;
                }
            }
        }
    }
    Ok(ExitStatus::Success)
}

/// Builds the context of `context add`, storing its token first when one was given.
async fn build_context<C: Connector>(
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
                    fingerprint: None,
                },
                None,
            ))
        }
        Some(url) => {
            let url = parse_http_url(url)?;
            let fingerprint = choose_pin(arguments, &url, services, io).await?;
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
                    fingerprint,
                },
                backend,
            ))
        }
    }
}

/// The certificate an `https` context pins: the given fingerprint, none when the system trust
/// store should decide, or the one the server presents once the user confirms it.
async fn choose_pin<C: Connector>(
    arguments: &ContextAdd,
    url: &Url,
    services: &Services<C>,
    io: &mut Io<'_>,
) -> Result<Option<TlsFingerprint>, CliError> {
    if url.scheme() != "https" {
        if arguments.fingerprint.is_some() || arguments.system_trust {
            return Err(CliError::usage(
                "--fingerprint and --system-trust apply to an https URL only",
            ));
        }
        return Ok(None);
    }
    if arguments.fingerprint.is_some() || arguments.system_trust {
        return Ok(arguments.fingerprint);
    }
    // Standard input would have to carry the token and the answer at once.
    if arguments.token_stdin {
        return Err(CliError::usage(
            "pass --fingerprint (or --system-trust) when the token is read from standard input",
        ));
    }
    let presented = services.connector.probe_certificate(url).await?;
    write!(
        io.stderr,
        "{url} presents a certificate with the SHA-256 fingerprint\n  {presented}\n\
         Compare it with the fingerprint in the app's remote access settings.\n\
         Pin this certificate? [y/N] "
    )?;
    io.stderr.flush()?;
    if !confirmed(io.stdin)? {
        return Err(CliError::usage(
            "the certificate was not pinned and nothing was saved; verify the fingerprint and \
             pass it with --fingerprint",
        ));
    }
    Ok(Some(presented))
}

/// Whether the first line of `input` is `y` or `yes`. End of input is a no.
fn confirmed(input: &mut dyn Read) -> Result<bool, CliError> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while line.len() < 16 && input.read(&mut byte)? == 1 && byte[0] != b'\n' {
        line.push(byte[0]);
    }
    let answer = String::from_utf8_lossy(&line);
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

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
        /// What the scripted transport was asked, kept after it moves into the command.
        calls: Arc<Mutex<Vec<(String, Value)>>>,
        /// The certificate an HTTPS server presents, or `None` for an unreachable one.
        presented: Mutex<Option<TlsFingerprint>>,
        /// Every URL whose certificate was probed.
        probed: Mutex<Vec<Url>>,
    }

    impl FakeConnector {
        fn new(outcome: Result<FakeTransport, CliError>) -> Self {
            let calls = outcome
                .as_ref()
                .map(|transport| transport.calls.clone())
                .unwrap_or_default();
            Self {
                targets: Mutex::default(),
                outcome: Mutex::new(Some(outcome)),
                calls,
                presented: Mutex::default(),
                probed: Mutex::default(),
            }
        }

        fn calls(&self) -> Vec<(String, Value)> {
            self.calls.lock().unwrap().clone()
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

        async fn probe_certificate(&self, base: &Url) -> Result<TlsFingerprint, CliError> {
            self.probed.lock().unwrap().push(base.clone());
            (*self.presented.lock().unwrap()).ok_or_else(|| CliError::Unreachable {
                target: base.to_string(),
                detail: "refused".to_owned(),
            })
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
            home: None,
            executable: None,
            skill_dir: None,
            appimage: None,
            search_path: None,
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
            fingerprint: None,
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
            fingerprint: None,
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
            &["--json", "call", "get_proxies", r#"{"x":1}"#],
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
                "--system-trust",
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

    fn pinned(text: &str) -> TlsFingerprint {
        TlsFingerprint::of_certificate(text.as_bytes())
    }

    fn context_file(harness: &Harness) -> CliConfig {
        CliConfig::load(&harness.services.paths.config_file).unwrap()
    }

    #[tokio::test]
    async fn an_https_context_pins_the_given_fingerprint_without_probing() {
        let harness = harness(Err(CliError::failed("never connects")));
        let fingerprint = pinned("lan certificate");
        let text = fingerprint.to_string();
        let run = invoke(
            &harness,
            &Environment::default(),
            &[
                "context",
                "add",
                "lan",
                "--url",
                "https://192.168.1.20:9443",
                "--fingerprint",
                &text,
            ],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        assert!(harness.services.connector.probed.lock().unwrap().is_empty());
        assert_eq!(
            context_file(&harness).contexts["lan"].fingerprint,
            Some(fingerprint)
        );
    }

    #[tokio::test]
    async fn an_unpinned_https_context_shows_the_fingerprint_and_pins_it_on_confirmation() {
        let harness = harness(Err(CliError::failed("never connects")));
        let presented = pinned("what the server sent");
        *harness.services.connector.presented.lock().unwrap() = Some(presented);
        let add = [
            "context",
            "add",
            "lan",
            "--url",
            "https://192.168.1.20:9443",
        ];

        let declined = invoke(&harness, &Environment::default(), &add, "n\n").await;
        assert_eq!(declined.status, ExitStatus::Usage);
        assert!(
            declined.stderr.contains(&presented.to_string()),
            "{}",
            declined.stderr
        );
        assert!(
            declined.stderr.contains("Pin this certificate?"),
            "{}",
            declined.stderr
        );
        assert!(
            context_file(&harness).contexts.is_empty(),
            "nothing is saved"
        );

        // End of input is not consent either.
        let silent = invoke(&harness, &Environment::default(), &add, "").await;
        assert_eq!(silent.status, ExitStatus::Usage);

        let accepted = invoke(&harness, &Environment::default(), &add, "yes\n").await;
        assert_eq!(accepted.status, ExitStatus::Success, "{}", accepted.stderr);
        assert_eq!(
            context_file(&harness).contexts["lan"].fingerprint,
            Some(presented)
        );
    }

    #[tokio::test]
    async fn pinning_needs_a_decision_that_standard_input_cannot_make_twice() {
        let harness = harness(Err(CliError::failed("never connects")));
        let environment = Environment::default();
        for arguments in [
            // The token is on standard input, so the answer cannot be.
            &[
                "context",
                "add",
                "a",
                "--url",
                "https://h:1",
                "--token-stdin",
            ][..],
            // Pinning and the system trust store exclude each other.
            &[
                "context",
                "add",
                "a",
                "--url",
                "https://h:1",
                "--system-trust",
                "--fingerprint",
                &pinned("x").to_string(),
            ],
        ] {
            let result = Cli::try_parse_from(
                std::iter::once("nyanpasu-cli").chain(arguments.iter().copied()),
            );
            match result {
                Err(_) => {}
                Ok(_) => {
                    let run = invoke(&harness, &environment, arguments, "npk_1_x\n").await;
                    assert_eq!(run.status, ExitStatus::Usage, "{arguments:?}");
                }
            }
        }
        // A plain http URL has no certificate to pin or to trust.
        let run = invoke(
            &harness,
            &environment,
            &[
                "context",
                "add",
                "a",
                "--url",
                "http://127.0.0.1:1",
                "--system-trust",
            ],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Usage);
        assert!(context_file(&harness).contexts.is_empty());
        assert!(harness.services.connector.probed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_pin_of_a_context_reaches_the_connector() {
        let transport = FakeTransport::new(vec![describe_all(), Ok(json!({}))]);
        let harness = harness(Ok(transport));
        let fingerprint = pinned("lan certificate");
        invoke(
            &harness,
            &Environment::default(),
            &[
                "context",
                "add",
                "lan",
                "--url",
                "https://192.168.1.20:9443",
                "--fingerprint",
                &fingerprint.to_string(),
            ],
            "",
        )
        .await;
        let run = invoke(&harness, &Environment::default(), &["status"], "").await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        let (target, _) = harness.services.connector.targets.lock().unwrap()[0].clone();
        assert_eq!(target.fingerprint, Some(fingerprint));
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
            fingerprint: None,
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

    /// A local principal holding every scope, with the remote administration commands allowed.
    fn describe_remote() -> Result<Value, CliError> {
        let methods: Vec<(&str, Scope)> = [
            "get_app_settings",
            "get_http_server_status",
            "get_local_socket_status",
            "set_remote_access",
            "reset_remote_access_tls",
            "list_remote_access_tokens",
            "create_remote_access_token",
            "revoke_remote_access_token",
            "list_remote_browser_sessions",
            "revoke_remote_browser_session",
        ]
        .into_iter()
        .map(|method| (method, Scope::RemoteAdmin))
        .collect();
        describe_reply(&Scope::ALL, &methods)
    }

    fn committed_settings() -> Value {
        json!({"remote_access": {
            "http": {"enabled": false, "strategy": "local_authenticated", "port": null,
                     "serve_webui": true, "local_open_scopes": ["read"],
                     "lan_bind": null, "tls": "http"},
            "local_socket": {"enabled": true},
        }})
    }

    #[tokio::test]
    async fn remote_status_reads_the_settings_and_both_listeners() {
        let transport = FakeTransport::new(vec![
            describe_remote(),
            Ok(committed_settings()),
            Ok(json!({"enabled": true, "url": "http://127.0.0.1:41234/", "port": 41234})),
            Ok(json!({"enabled": true, "endpoint": "/run/user/1000/rpc.sock"})),
        ]);
        let harness = harness(Ok(transport));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["--json", "remote", "status"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        let value: Value = serde_json::from_str(run.stdout.trim()).unwrap();
        assert_eq!(value["settings"]["local_socket"]["enabled"], true);
        assert_eq!(value["http"]["port"], 41234);
        assert_eq!(value["local_socket"]["endpoint"], "/run/user/1000/rpc.sock");
        let calls = harness.services.connector.calls();
        let methods: Vec<_> = calls.iter().map(|(method, _)| method.as_str()).collect();
        assert_eq!(
            methods,
            [
                "rpc_describe",
                "get_app_settings",
                "get_http_server_status",
                "get_local_socket_status"
            ]
        );
    }

    #[tokio::test]
    async fn a_remote_switch_writes_the_whole_section_back_with_one_field_changed() {
        for (arguments, pointer, expected) in [
            (&["remote", "http", "on"][..], "/http/enabled", json!(true)),
            (&["remote", "http", "off"], "/http/enabled", json!(false)),
            (
                &["remote", "http", "strategy", "local_open"],
                "/http/strategy",
                json!("local_open"),
            ),
            (
                &["remote", "socket", "off"],
                "/local_socket/enabled",
                json!(false),
            ),
            (
                &["remote", "http", "lan-bind", "192.168.1.20"],
                "/http/lan_bind",
                json!("192.168.1.20"),
            ),
            (
                &["remote", "http", "lan-bind", "all"],
                "/http/lan_bind",
                Value::Null,
            ),
            (
                &["remote", "http", "tls", "self_signed"],
                "/http/tls",
                json!("self_signed"),
            ),
        ] {
            let transport = FakeTransport::new(vec![
                describe_remote(),
                Ok(committed_settings()),
                Ok(json!({"status": "committed", "value": null, "commits": [],
                          "notifications_pending": false, "degradations": []})),
            ]);
            let harness = harness(Ok(transport));
            let run = invoke(&harness, &Environment::default(), arguments, "").await;
            assert_eq!(
                run.status,
                ExitStatus::Success,
                "{arguments:?}: {}",
                run.stderr
            );
            let calls = harness.services.connector.calls();
            let methods: Vec<_> = calls.iter().map(|(method, _)| method.as_str()).collect();
            assert_eq!(
                methods,
                ["rpc_describe", "get_app_settings", "set_remote_access"]
            );
            let mut sent = calls[2].1["config"].clone();
            assert_eq!(sent.pointer(pointer), Some(&expected), "{arguments:?}");
            // Everything else is the committed value, unchanged.
            let mut original = committed_settings()["remote_access"].clone();
            *sent.pointer_mut(pointer).unwrap() = Value::Null;
            *original.pointer_mut(pointer).unwrap() = Value::Null;
            assert_eq!(sent, original, "{arguments:?}");
        }
    }

    #[tokio::test]
    async fn the_lan_strategy_brings_https_and_warns() {
        let transport = FakeTransport::new(vec![
            describe_remote(),
            Ok(committed_settings()),
            Ok(json!({"status": "committed", "value": null})),
        ]);
        let harness = harness(Ok(transport));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["remote", "http", "strategy", "lan_authenticated"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        assert!(
            run.stderr.contains("compare the certificate fingerprint"),
            "{}",
            run.stderr
        );
        let calls = harness.services.connector.calls();
        let sent = &calls[2].1["config"]["http"];
        assert_eq!(sent["strategy"], "lan_authenticated");
        assert_eq!(
            sent["tls"], "self_signed",
            "credentials never cross the LAN in the clear"
        );
    }

    #[tokio::test]
    async fn resetting_the_certificate_calls_its_command_and_reports_degradation() {
        let transport = FakeTransport::new(vec![
            describe_remote(),
            Ok(json!({
                "status": "committed_degraded", "value": null, "commits": [],
                "notifications_pending": false,
                "degradations": [{"phase": "system_effect", "message": "the HTTP server is not running"}]
            })),
        ]);
        let harness = harness(Ok(transport));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["remote", "reset-certificate"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Degraded, "{}", run.stderr);
        let calls = harness.services.connector.calls();
        assert_eq!(calls[1].0, "reset_remote_access_tls");
    }

    #[tokio::test]
    async fn opening_the_http_api_to_local_processes_is_warned_about() {
        let transport = FakeTransport::new(vec![
            describe_remote(),
            Ok(committed_settings()),
            Ok(json!({"status": "committed", "value": null})),
        ]);
        let harness = harness(Ok(transport));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["remote", "http", "strategy", "local_open"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        assert!(run.stderr.contains("every OS user"), "{}", run.stderr);
    }

    #[tokio::test]
    async fn a_listener_that_cannot_follow_exits_six() {
        let transport = FakeTransport::new(vec![
            describe_remote(),
            Ok(committed_settings()),
            Ok(json!({
                "status": "committed_degraded", "value": null, "commits": [],
                "notifications_pending": false,
                "degradations": [{"phase": "system_effect", "message": "the HTTP server is not running: PortInUse"}]
            })),
        ]);
        let harness = harness(Ok(transport));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["remote", "http", "on"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Degraded);
        assert!(run.stderr.contains("PortInUse"), "{}", run.stderr);
    }

    #[tokio::test]
    async fn token_creation_sends_the_scopes_and_prints_the_secret_once() {
        let created = json!({
            "summary": {"id": "tok_1", "name": "ci", "scopes": ["read", "proxy"]},
            "token": "npk_tok_1_secret",
        });
        let transport = FakeTransport::new(vec![describe_remote(), Ok(created.clone())]);
        let human = harness(Ok(transport));
        let run = invoke(
            &human,
            &Environment::default(),
            &[
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
            ],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        assert!(run.stdout.contains("npk_tok_1_secret"), "{}", run.stdout);
        assert!(run.stderr.contains("shown once"), "{}", run.stderr);
        let calls = human.services.connector.calls();
        let request = &calls[1].1["request"];
        assert_eq!(calls[1].0, "create_remote_access_token");
        assert_eq!(request["name"], "ci");
        assert_eq!(request["scopes"], json!(["read", "proxy"]));
        assert!(request["expires_at_ms"].as_i64().unwrap() > 0);

        let transport = FakeTransport::new(vec![describe_remote(), Ok(created.clone())]);
        let machine = harness(Ok(transport));
        let run = invoke(
            &machine,
            &Environment::default(),
            &[
                "--json", "remote", "token", "create", "ci", "--scope", "read",
            ],
            "",
        )
        .await;
        assert_eq!(
            serde_json::from_str::<Value>(run.stdout.trim()).unwrap(),
            created,
            "--json prints the shell result unchanged"
        );
        assert!(!run.stderr.contains("shown once"), "{}", run.stderr);
    }

    #[tokio::test]
    async fn remote_administration_never_connects_to_an_http_target() {
        let harness = harness(Err(CliError::failed("must not connect")));
        let environment = Environment {
            url: Some("http://127.0.0.1:9090".to_owned()),
            token: Some("npk_1_secret".to_owned()),
            fingerprint: None,
        };
        for arguments in [
            &["remote", "status"][..],
            &["remote", "token", "list"],
            &["remote", "http", "on"],
        ] {
            let run = invoke(&harness, &environment, arguments, "").await;
            assert_eq!(run.status, ExitStatus::Unsupported, "{arguments:?}");
            assert!(run.stderr.contains("local socket"), "{}", run.stderr);
        }
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
    async fn a_shell_that_does_not_offer_remote_administration_is_unsupported() {
        // A shell older than this CLI, or one that exposes none of the commands.
        let transport = FakeTransport::new(vec![describe_reply(&Scope::ALL, &[])]);
        let harness = harness(Ok(transport));
        let run = invoke(
            &harness,
            &Environment::default(),
            &["remote", "token", "list"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Unsupported, "{}", run.stderr);
    }

    /// A skill folder and a home directory, with no connection to any shell.
    fn skill_harness() -> Harness {
        let mut harness = harness(Err(CliError::failed("the skill commands never connect")));
        let folder = harness.directory.path().join("bundle/skills/nyanpasu-cli");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("SKILL.md"), "skill").unwrap();
        harness.services.skill_dir = Some(folder);
        harness.services.home = Some(harness.directory.path().join("home"));
        harness
    }

    #[tokio::test]
    async fn skill_path_prints_the_folder_without_connecting() {
        let harness = skill_harness();
        let folder = harness.services.skill_dir.clone().unwrap();
        let plain = invoke(&harness, &Environment::default(), &["skill", "path"], "").await;
        assert_eq!(plain.status, ExitStatus::Success, "{}", plain.stderr);
        assert_eq!(
            plain.stdout.trim_end(),
            folder.canonicalize().unwrap().display().to_string()
        );
        let json = invoke(
            &harness,
            &Environment::default(),
            &["--json", "skill", "path"],
            "",
        )
        .await;
        assert_eq!(
            serde_json::from_str::<Value>(json.stdout.trim()).unwrap()["path"],
            json!(folder.canonicalize().unwrap())
        );
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

    #[cfg(unix)]
    #[tokio::test]
    async fn skill_install_links_into_the_agents_directory_once() {
        let harness = skill_harness();
        let home = harness.services.home.clone().unwrap();
        for expected in ["linked", "already links"] {
            let run = invoke(
                &harness,
                &Environment::default(),
                &["skill", "install", "--agent", "claude"],
                "",
            )
            .await;
            assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
            assert!(run.stdout.contains(expected), "{}", run.stdout);
        }
        let link = home.join(".claude/skills/nyanpasu-cli");
        assert!(link.join("SKILL.md").is_file(), "the link resolves");

        std::fs::remove_file(&link).unwrap();
        std::fs::create_dir(&link).unwrap();
        let run = invoke(
            &harness,
            &Environment::default(),
            &["skill", "install", "--agent", "claude", "--force"],
            "",
        )
        .await;
        assert_eq!(
            run.status,
            ExitStatus::Application,
            "never replaces real data"
        );
    }

    /// A macOS-style bundle: the executable and its skill, with no connection to any shell.
    #[cfg(unix)]
    fn bundle_harness() -> (Harness, PathBuf) {
        let mut harness = skill_harness();
        let executable = harness.directory.path().join("bundle/nyanpasu-cli");
        std::fs::write(&executable, "binary").unwrap();
        harness.services.executable = Some(executable.clone());
        (harness, executable)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_links_the_bundled_executable_and_prints_the_result_as_json() {
        let (mut harness, executable) = bundle_harness();
        let bin = harness.directory.path().join("home/.local/bin");
        harness.services.home = Some(harness.directory.path().join("home"));
        harness.services.search_path = Some(bin.clone().into_os_string());

        let run = invoke(
            &harness,
            &Environment::default(),
            &["--json", "install"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        let result: Value = serde_json::from_str(run.stdout.trim()).unwrap();
        let target = executable.canonicalize().unwrap();
        assert_eq!(result["link"], json!(bin.join("nyanpasu-cli")));
        assert_eq!(result["target"], json!(target));
        assert_eq!(result["already_linked"], json!(false));
        assert_eq!(result["on_path"], json!(true));
        assert!(
            result["system_command"]
                .as_str()
                .is_some_and(|command| command.contains("/usr/local/bin/nyanpasu-cli"))
        );
        assert_eq!(
            std::fs::read_link(bin.join("nyanpasu-cli")).unwrap(),
            target
        );

        let again = invoke(&harness, &Environment::default(), &["install"], "").await;
        assert_eq!(again.status, ExitStatus::Success, "{}", again.stderr);
        assert!(again.stdout.contains("already links"), "{}", again.stdout);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn install_says_when_the_directory_is_not_on_path() {
        let (harness, _) = bundle_harness();
        let dir = harness.directory.path().join("elsewhere");
        let argument = dir.display().to_string();

        let run = invoke(
            &harness,
            &Environment::default(),
            &["install", "--dir", &argument],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        assert!(run.stdout.contains("is not on your PATH"), "{}", run.stdout);
        assert!(dir.join("nyanpasu-cli").exists());
    }

    /// An AppImage whose executable and skill sit in a mount that is about to vanish.
    #[cfg(unix)]
    fn appimage_harness() -> (Harness, AppImage) {
        let mut harness = skill_harness();
        let root = harness.directory.path().to_owned();
        let mount = root.join("mount");
        let skill = mount.join("usr/lib/Clash Nyanpasu/skills/nyanpasu-cli");
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), "skill").unwrap();
        let executable = mount.join("usr/bin/nyanpasu-cli");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, "binary").unwrap();
        let appimage = AppImage {
            mount,
            standalone_dir: root.join("data/nyanpasu-cli"),
        };
        harness.services.executable = Some(executable);
        harness.services.skill_dir = None;
        harness.services.appimage = Some(appimage.clone());
        (harness, appimage)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_appimage_links_a_copy_that_outlives_its_mount() {
        let (harness, appimage) = appimage_harness();
        let home = harness.services.home.clone().unwrap();

        let run = invoke(&harness, &Environment::default(), &["install"], "").await;
        assert_eq!(run.status, ExitStatus::Success, "{}", run.stderr);
        let link = home.join(".local/bin/nyanpasu-cli");
        let copy = appimage.standalone_dir.join("nyanpasu-cli");
        assert_eq!(
            std::fs::read_link(&link).unwrap().canonicalize().unwrap(),
            copy.canonicalize().unwrap()
        );
        std::fs::remove_dir_all(&appimage.mount).unwrap();
        assert!(link.exists(), "the link survives the unmounted app");
        assert!(
            appimage
                .standalone_dir
                .join("skills/nyanpasu-cli/SKILL.md")
                .is_file(),
            "the copy keeps its skill beside it"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_appimage_skill_is_linked_from_a_copy_not_from_the_mount() {
        let (harness, appimage) = appimage_harness();
        let home = harness.services.home.clone().unwrap();

        let path = invoke(&harness, &Environment::default(), &["skill", "path"], "").await;
        assert_eq!(path.status, ExitStatus::Success, "{}", path.stderr);
        let copy = appimage.standalone_dir.join("skills/nyanpasu-cli");
        assert_eq!(
            std::fs::canonicalize(path.stdout.trim_end()).unwrap(),
            copy.canonicalize().unwrap()
        );

        let install = invoke(
            &harness,
            &Environment::default(),
            &["skill", "install", "--agent", "claude"],
            "",
        )
        .await;
        assert_eq!(install.status, ExitStatus::Success, "{}", install.stderr);
        std::fs::remove_dir_all(&appimage.mount).unwrap();
        let link = home.join(".claude/skills/nyanpasu-cli");
        assert!(
            link.join("SKILL.md").is_file(),
            "the agent still finds the skill"
        );
    }

    #[tokio::test]
    async fn skill_install_needs_a_home_directory() {
        let mut harness = skill_harness();
        harness.services.home = None;
        let run = invoke(
            &harness,
            &Environment::default(),
            &["skill", "install", "--agent", "codex"],
            "",
        )
        .await;
        assert_eq!(run.status, ExitStatus::Application);
        assert!(run.stderr.contains("home directory"), "{}", run.stderr);
    }

    #[test]
    fn select_path_walks_objects_and_arrays() {
        let value = json!({"a": {"b": [1, {"c": true}]}});
        assert_eq!(select_path(&value, "a.b.1.c"), Some(&json!(true)));
        assert_eq!(select_path(&value, "a.x"), None);
        assert_eq!(select_path(&value, ""), Some(&value));
    }
}
