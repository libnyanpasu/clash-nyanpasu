//! The command line grammar.
//!
//! Subcommands that reach the shell map to curated operations (see `commands.rs`) through
//! [`Command::plan`]; everything else is local to the CLI.

use std::{net::IpAddr, path::PathBuf, time::Duration};

use clap::{Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use nyanpasu_contracts::auth::TlsFingerprint;
use serde_json::{Value, json};

use nyanpasu_contracts::auth::Grant;

use crate::{
    describe::{Permission, Permissions, Resource, Role},
    error::CliError,
    target::TransportChoice,
};

/// Controls a running Clash Nyanpasu shell over its local socket or HTTP.
#[derive(Debug, Parser)]
#[command(
    name = "nyanpasu-cli",
    version = env!("NYANPASU_VERSION"),
    arg_required_else_help = true
)]
pub struct Cli {
    /// Use this context for this invocation without changing the default.
    #[arg(long, global = true, value_name = "NAME")]
    pub context: Option<String>,

    /// Narrow the resolved target to one transport; it never selects another target.
    #[arg(long, global = true, value_enum, default_value_t = TransportChoice::Auto)]
    pub transport: TransportChoice,

    /// Print the RPC result as JSON; streams print one JSON object per line.
    #[arg(long, global = true)]
    pub json: bool,

    /// The `cli.toml` file to use instead of the user config directory.
    #[arg(long, global = true, env = "NYANPASU_CLI_CONFIG", value_name = "FILE")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

/// Every subcommand.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show the Clash core status.
    Status,
    /// Control the Clash core.
    #[command(subcommand)]
    Core(CoreCommand),
    /// Read or change the proxy mode.
    #[command(subcommand)]
    Mode(ModeCommand),
    /// Inspect and select proxies.
    #[command(subcommand)]
    Proxy(ProxyCommand),
    /// Manage profiles and subscriptions.
    #[command(subcommand)]
    Profile(ProfileCommand),
    /// Read or change the operating system proxy.
    #[command(subcommand)]
    Sysproxy(SwitchCommand),
    /// Read or change TUN mode.
    #[command(subcommand)]
    Tun(SwitchCommand),
    /// Inspect and close connections.
    #[command(subcommand)]
    Conn(ConnCommand),
    /// Read core logs.
    #[command(subcommand)]
    Logs(LogsCommand),
    /// Show traffic.
    Traffic(TrafficArgs),
    /// Read or patch application settings.
    #[command(subcommand)]
    Config(ConfigCommand),
    /// Administer remote access: the HTTP server, the local socket, tokens and sessions.
    /// Only the local socket offers these commands.
    #[command(subcommand)]
    Remote(RemoteCommand),
    /// List the commands the shell offers this caller (`rpc_describe`).
    Methods,
    /// Call any allowed RPC method with a JSON parameter object.
    Call(CallArgs),
    /// Manage named targets in `cli.toml`.
    #[command(subcommand)]
    Context(ContextCommand),
    /// Link this executable into a directory on your `PATH`. Needed for the macOS app and an
    /// AppImage; the Windows installer and the Linux packages already put it there.
    Install(InstallArgs),
    /// Find the agent skill or link it into an agent's skill directory. In an AppImage the
    /// skill is first copied out of the temporary mount, which disappears with the app.
    #[command(subcommand)]
    Skill(SkillCommand),
    /// Print a shell completion script.
    Completion(CompletionArgs),
}

#[derive(Debug, Subcommand)]
pub enum CoreCommand {
    /// Restart the core. Active connections are dropped.
    Restart,
    /// Switch to another core and restart it.
    Switch {
        /// The core name, such as `mihomo`.
        core: String,
    },
    /// Show the version the running core reports.
    Version,
}

#[derive(Debug, Subcommand)]
pub enum ModeCommand {
    /// Show the proxy mode and the rest of the core configuration.
    Get,
    /// Switch the proxy mode.
    Set {
        /// The mode to switch to.
        mode: Mode,
    },
}

/// A Clash proxy mode.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Mode {
    Rule,
    Global,
    Direct,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Global => "global",
            Self::Direct => "direct",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum ProxyCommand {
    /// List proxy groups and their selections.
    Groups,
    /// Select a proxy inside a group.
    Select {
        /// The proxy group.
        group: String,
        /// The proxy to select.
        proxy: String,
    },
    /// Run a latency test against a proxy, or against a whole group with `--group`.
    Test {
        /// A proxy name, or a group name with `--group`.
        name: String,
        /// Treat `name` as a proxy group.
        #[arg(long)]
        group: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProfileCommand {
    /// List profiles.
    List,
    /// Activate a profile.
    Use {
        /// The profile id.
        id: String,
    },
    /// Refresh a remote profile from its subscription URL.
    Update {
        /// The profile id.
        id: String,
    },
    /// Import a remote profile.
    Import {
        /// The subscription URL.
        url: String,
        /// A display name.
        #[arg(long)]
        name: Option<String>,
    },
    /// Show a profile's file content, which may contain credentials.
    Show {
        /// The profile id.
        id: String,
    },
}

/// A feature with a status and an on/off switch.
#[derive(Debug, Subcommand)]
pub enum SwitchCommand {
    /// Show whether it is on.
    Status,
    /// Turn it on.
    On,
    /// Turn it off.
    Off,
}

#[derive(Debug, Subcommand)]
pub enum ConnCommand {
    /// List connections.
    List {
        /// Keep printing the connection details as they change.
        #[arg(short, long)]
        follow: bool,
    },
    /// Close one connection, or all of them. Dropped connections are not restored.
    Close {
        /// The connection id.
        id: Option<String>,
        /// Close every connection.
        #[arg(long, conflicts_with = "id")]
        all: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum LogsCommand {
    /// Show the latest core log rows.
    Tail {
        /// Keep printing new log events.
        #[arg(short, long)]
        follow: bool,
        /// Only rows at this level.
        #[arg(long)]
        level: Option<String>,
        /// Only rows containing this text.
        #[arg(long)]
        keyword: Option<String>,
        /// How many rows to show.
        #[arg(long, default_value_t = 100)]
        limit: u64,
    },
}

#[derive(Debug, Args)]
pub struct TrafficArgs {
    /// Keep printing live traffic updates.
    #[arg(short, long)]
    pub follow: bool,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show the application settings, or one value selected by a dotted path.
    Get {
        /// A dotted path such as `theme_mode`.
        path: Option<String>,
    },
    /// Patch ordinary application settings with a JSON object.
    Patch {
        /// The patch, as a JSON object.
        #[arg(value_name = "JSON")]
        patch: String,
    },
}

/// Remote access administration. It needs `remote:write`, which only the desktop and the
/// same-user local socket hold, so it never runs over HTTP.
#[derive(Debug, Subcommand)]
pub enum RemoteCommand {
    /// Show the remote access settings and what each listener is doing.
    Status,
    /// Turn the HTTP server on or off, or choose how it authenticates.
    #[command(subcommand)]
    Http(HttpCommand),
    /// Turn the local socket on or off. Turning it off ends this connection's access.
    #[command(subcommand)]
    Socket(SwitchOnOff),
    /// Replace the certificate of the HTTPS server. Clients that pinned the old one must pin the
    /// new fingerprint, which `remote status` shows.
    ResetCertificate,
    /// Manage the access tokens that authenticate over HTTP.
    #[command(subcommand)]
    Token(TokenCommand),
    /// Manage browser sessions.
    #[command(subcommand)]
    Session(SessionCommand),
}

#[derive(Debug, Subcommand)]
pub enum HttpCommand {
    /// Start the HTTP server.
    On,
    /// Stop the HTTP server.
    Off,
    /// Choose how HTTP callers authenticate.
    Strategy {
        /// `local_authenticated` requires a credential; `local_open` lets every local
        /// process of every OS user call the API with the configured anonymous permissions;
        /// `lan_authenticated` listens on the LAN over HTTPS and requires a credential from
        /// every caller, so it also turns HTTPS on.
        #[arg(value_enum)]
        strategy: HttpStrategy,
    },
    /// Choose the address `lan_authenticated` listens on, or every IPv4 interface with `all`.
    LanBind {
        /// An IP address of this computer, or `all`.
        #[arg(value_parser = parse_lan_bind)]
        address: LanBind,
    },
    /// Serve plain HTTP or HTTPS with the self-signed certificate. The LAN strategy needs HTTPS.
    Tls {
        #[arg(value_enum)]
        mode: HttpTlsMode,
    },
}

/// Where `lan_authenticated` listens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanBind {
    /// Every IPv4 interface.
    All,
    /// One address of this computer.
    Address(IpAddr),
}

fn parse_lan_bind(text: &str) -> Result<LanBind, String> {
    if text.eq_ignore_ascii_case("all") {
        return Ok(LanBind::All);
    }
    text.parse()
        .map(LanBind::Address)
        .map_err(|_| format!("`{text}` is neither an IP address nor `all`"))
}

/// Whether the HTTP server speaks TLS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HttpTlsMode {
    /// Plain HTTP, loopback strategies only.
    #[value(name = "http")]
    Http,
    /// HTTPS with the self-signed certificate.
    #[value(name = "self_signed", alias = "self-signed")]
    SelfSigned,
}

impl HttpTlsMode {
    /// The persisted spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::SelfSigned => "self_signed",
        }
    }
}

/// How the HTTP server authenticates a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HttpStrategy {
    #[value(name = "local_authenticated", alias = "local-authenticated")]
    LocalAuthenticated,
    #[value(name = "local_open", alias = "local-open")]
    LocalOpen,
    #[value(name = "lan_authenticated", alias = "lan-authenticated")]
    LanAuthenticated,
}

impl HttpStrategy {
    /// The persisted spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalAuthenticated => "local_authenticated",
            Self::LocalOpen => "local_open",
            Self::LanAuthenticated => "lan_authenticated",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum SwitchOnOff {
    /// Turn it on.
    On,
    /// Turn it off.
    Off,
}

#[derive(Debug, Subcommand)]
pub enum TokenCommand {
    /// Create a token. Its secret is printed once and cannot be shown again.
    Create(TokenCreate),
    /// List the tokens without their secrets.
    List,
    /// Revoke a token, closing the browser sessions it opened.
    Revoke {
        /// The token id shown by `token list`.
        id: String,
    },
}

#[derive(Debug, Args)]
pub struct TokenCreate {
    /// A name for the token, shown in lists and audit records.
    pub name: String,
    /// The role the token holds: `viewer`, `operator` or `admin`. A role follows later changes
    /// to its definition, so commands added by an update become available with it.
    #[arg(
        long,
        value_name = "ROLE",
        value_parser = parse_token_role,
        required_unless_present = "permissions",
        conflicts_with = "permissions"
    )]
    pub role: Option<Role>,
    /// A permission the token holds instead of a role, such as `proxy:write`; repeat for
    /// several. `write` includes `read`. A token never holds `remote`.
    #[arg(
        long = "permission",
        value_name = "RESOURCE:ACCESS",
        value_parser = parse_token_permission
    )]
    pub permissions: Vec<Permission>,
    /// How long the token stays valid, such as `90m`, `12h` or `30d`. It does not expire
    /// when omitted.
    #[arg(long, value_name = "DURATION", value_parser = parse_lifetime)]
    pub expires_in: Option<Duration>,
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// List the open browser sessions.
    List,
    /// Close a browser session.
    Revoke {
        /// The session id shown by `session list`.
        id: String,
    },
}

impl TokenCreate {
    /// What the token is granted: the role, or the listed permissions with the higher level
    /// kept where a resource is listed twice.
    pub fn grant(&self) -> Grant {
        match self.role {
            Some(role) => Grant::Role(role),
            None => Grant::Custom(self.permissions.iter().fold(
                Permissions::NONE,
                |held, permission| {
                    let access = held
                        .get(permission.resource)
                        .map_or(permission.access, |level| level.max(permission.access));
                    held.with(permission.resource, access)
                },
            )),
        }
    }
}

/// A role a token may hold: `viewer`, `operator` or `admin`.
fn parse_token_role(text: &str) -> Result<Role, String> {
    match serde_json::from_value::<Role>(Value::String(text.to_owned())) {
        Ok(role @ (Role::Viewer | Role::Operator | Role::Admin)) => Ok(role),
        _ => Err(format!(
            "`{text}` is not a role a token can hold; choose from viewer, operator, admin"
        )),
    }
}

/// A permission a token may hold, `resource:access`. `remote` stays with the owner, and every
/// caller already holds `meta:read`.
fn parse_token_permission(text: &str) -> Result<Permission, String> {
    const RESOURCES: &str = "proxy, profile, settings, system, logs, traffic";
    let invalid = || {
        format!("`{text}` is not `resource:access`; resources: {RESOURCES}; access: read, write")
    };
    let (resource, access) = text.split_once(':').ok_or_else(invalid)?;
    let resource = serde_json::from_value::<Resource>(Value::String(resource.to_owned()))
        .map_err(|_| invalid())?;
    let access = serde_json::from_value(Value::String(access.to_owned())).map_err(|_| invalid())?;
    match resource {
        Resource::Remote => Err(format!(
            "a token never holds remote; remote access is administered from the app or \
             `nyanpasu-cli remote`; resources: {RESOURCES}"
        )),
        Resource::Meta => Err(format!(
            "every caller already holds meta:read; resources: {RESOURCES}"
        )),
        _ => Ok(Permission::new(resource, access)),
    }
}

/// A positive duration such as `90s`, `15m`, `12h` or `30d`.
fn parse_lifetime(text: &str) -> Result<Duration, String> {
    let invalid = || format!("`{text}` is not a duration such as 90m, 12h or 30d");
    let (number, unit) = text.split_at(text.len().saturating_sub(1));
    let seconds_per_unit: u64 = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 60 * 60,
        "d" => 24 * 60 * 60,
        _ => return Err(invalid()),
    };
    let count: u64 = number.parse().map_err(|_| invalid())?;
    let seconds = count
        .checked_mul(seconds_per_unit)
        .filter(|seconds| *seconds > 0)
        .ok_or_else(invalid)?;
    Ok(Duration::from_secs(seconds))
}

#[derive(Debug, Args)]
pub struct CallArgs {
    /// The RPC method name; list them with `nyanpasu-cli methods`.
    pub method: String,
    /// The parameter object as JSON. Defaults to `{}`.
    #[arg(value_name = "JSON")]
    pub params: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    /// Add or replace a context. A token is read from standard input or `NYANPASU_TOKEN`,
    /// never from an argument.
    Add(ContextAdd),
    /// Make a context the default.
    Use {
        /// The context name.
        name: String,
    },
    /// List contexts.
    List,
    /// Remove a context and its stored token.
    Remove {
        /// The context name.
        name: String,
    },
}

#[derive(Debug, Args)]
pub struct ContextAdd {
    /// The context name.
    pub name: String,
    /// Target the local socket.
    #[arg(long, conflicts_with = "url", required_unless_present = "url")]
    pub socket: bool,
    /// Override the discovered socket path.
    #[arg(long, requires = "socket", value_name = "PATH")]
    pub endpoint: Option<PathBuf>,
    /// Target an HTTP base URL.
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    /// Read the token from standard input.
    #[arg(long, requires = "url", conflicts_with = "token_from_env")]
    pub token_stdin: bool,
    /// Take the token from `NYANPASU_TOKEN`.
    #[arg(long, requires = "url")]
    pub token_from_env: bool,
    /// Pin the server certificate by its SHA-256 fingerprint, as the app's remote access
    /// settings show it. Only an https URL can be pinned.
    #[arg(
        long,
        requires = "url",
        conflicts_with = "system_trust",
        value_name = "SHA256"
    )]
    pub fingerprint: Option<TlsFingerprint>,
    /// Verify an https server with the system trust store, for a certificate from a real
    /// authority such as a reverse proxy's, instead of pinning its certificate.
    #[arg(long, requires = "url")]
    pub system_trust: bool,
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// The directory to link into, which should be on `PATH` [default: ~/.local/bin].
    #[arg(long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
    /// Replace an existing link that points somewhere else.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Subcommand)]
pub enum SkillCommand {
    /// Print the folder that holds the agent skill.
    Path,
    /// Link the skill into an agent's skill directory.
    Install {
        /// The agent to install it for.
        #[arg(long, value_enum)]
        agent: Agent,
        /// Replace an existing link that points somewhere else.
        #[arg(long)]
        force: bool,
    },
}

/// An agent with a known skill directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Agent {
    /// Claude Code: `~/.claude/skills`.
    Claude,
    /// Codex: `~/.codex/skills`.
    Codex,
    /// The shared `~/.agents/skills` directory.
    Agents,
}

#[derive(Debug, Args)]
pub struct CompletionArgs {
    /// The shell to generate for.
    #[arg(value_enum)]
    pub shell: Shell,
}

/// What a connected subcommand asks for: one curated operation and how to present it.
#[derive(Debug, PartialEq)]
pub struct Plan {
    /// The curated operation name.
    pub name: &'static str,
    /// The schema-shaped input.
    pub input: Value,
    /// A follow-up stream to print after the result.
    pub follow: Option<Follow>,
    /// A dotted path selecting part of the result.
    pub select: Option<String>,
}

/// A stream a `-f` flag attaches to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow {
    /// Core log events.
    Logs,
    /// Live traffic.
    Traffic,
    /// Connection details.
    Connections,
}

fn plan(name: &'static str, input: Value) -> Plan {
    Plan {
        name,
        input,
        follow: None,
        select: None,
    }
}

fn switch(command: &SwitchCommand, status: &'static str, set: &'static str) -> Plan {
    match command {
        SwitchCommand::Status => plan(status, json!({})),
        SwitchCommand::On => plan(set, json!({"enabled": true})),
        SwitchCommand::Off => plan(set, json!({"enabled": false})),
    }
}

impl Command {
    /// The curated operation a subcommand stands for, or `None` for local subcommands,
    /// `remote`, `methods` and `call`, which the application layer handles itself.
    ///
    /// # Errors
    /// Malformed JSON arguments are usage errors.
    pub fn plan(&self) -> Result<Option<Plan>, CliError> {
        let plan = match self {
            Self::Status => plan("status", json!({})),
            Self::Core(CoreCommand::Restart) => plan("core_restart", json!({})),
            Self::Core(CoreCommand::Switch { core }) => plan("core_switch", json!({"core": core})),
            Self::Core(CoreCommand::Version) => plan("core_version", json!({})),
            Self::Mode(ModeCommand::Get) => plan("mode_get", json!({})),
            Self::Mode(ModeCommand::Set { mode }) => {
                plan("mode_set", json!({"mode": mode.as_str()}))
            }
            Self::Proxy(ProxyCommand::Groups) => plan("proxy_groups", json!({})),
            Self::Proxy(ProxyCommand::Select { group, proxy }) => {
                plan("proxy_select", json!({"group": group, "name": proxy}))
            }
            Self::Proxy(ProxyCommand::Test { name, group: true }) => {
                plan("proxy_test_group", json!({"group": name}))
            }
            Self::Proxy(ProxyCommand::Test { name, group: false }) => {
                plan("proxy_test", json!({"name": name}))
            }
            Self::Profile(ProfileCommand::List) => plan("profile_list", json!({})),
            Self::Profile(ProfileCommand::Use { id }) => plan("profile_use", json!({"uid": id})),
            Self::Profile(ProfileCommand::Update { id }) => {
                plan("profile_update", json!({"uid": id}))
            }
            Self::Profile(ProfileCommand::Import { url, name }) => {
                plan("profile_import", json!({"url": url, "name": name}))
            }
            Self::Profile(ProfileCommand::Show { id }) => plan("profile_show", json!({"uid": id})),
            Self::Sysproxy(command) => switch(command, "sysproxy_status", "sysproxy_set"),
            Self::Tun(command) => switch(command, "tun_status", "tun_set"),
            Self::Conn(ConnCommand::List { follow }) => Plan {
                follow: follow.then_some(Follow::Connections),
                ..plan("conn_list", json!({}))
            },
            Self::Conn(ConnCommand::Close {
                id: Some(id),
                all: false,
            }) => plan("conn_close", json!({"id": id})),
            Self::Conn(ConnCommand::Close {
                id: None,
                all: true,
            }) => plan("conn_close", json!({})),
            Self::Conn(ConnCommand::Close { .. }) => {
                return Err(CliError::usage(
                    "give a connection id, or --all to close every connection",
                ));
            }
            Self::Logs(LogsCommand::Tail {
                follow,
                level,
                keyword,
                limit,
            }) => Plan {
                follow: follow.then_some(Follow::Logs),
                ..plan(
                    "logs_tail",
                    json!({"level": level, "keyword": keyword, "limit": limit}),
                )
            },
            Self::Traffic(args) => Plan {
                follow: args.follow.then_some(Follow::Traffic),
                ..plan("traffic", json!({}))
            },
            Self::Config(ConfigCommand::Get { path }) => Plan {
                select: path.clone(),
                ..plan("config_get", json!({}))
            },
            Self::Config(ConfigCommand::Patch { patch: text }) => {
                plan("config_patch", json!({"patch": parse_json_object(text)?}))
            }
            Self::Remote(_)
            | Self::Methods
            | Self::Call(_)
            | Self::Context(_)
            | Self::Install(_)
            | Self::Skill(_)
            | Self::Completion(_) => return Ok(None),
        };
        Ok(Some(plan))
    }
}

/// Parses a JSON object argument.
///
/// # Errors
/// Anything that is not a JSON object is a usage error.
pub fn parse_json_object(text: &str) -> Result<Value, CliError> {
    match serde_json::from_str::<Value>(text) {
        Ok(value @ Value::Object(_)) => Ok(value),
        Ok(_) => Err(CliError::usage("expected a JSON object")),
        Err(error) => Err(CliError::usage(format!("invalid JSON: {error}"))),
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    use super::*;

    fn parse(arguments: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("nyanpasu-cli").chain(arguments.iter().copied()))
            .unwrap_or_else(|error| panic!("{arguments:?}: {error}"))
    }

    #[test]
    fn the_grammar_is_internally_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn subcommands_map_to_curated_operations() {
        for (arguments, name) in [
            (&["status"][..], "status"),
            (&["core", "restart"], "core_restart"),
            (&["core", "version"], "core_version"),
            (&["mode", "get"], "mode_get"),
            (&["mode", "set", "global"], "mode_set"),
            (&["proxy", "groups"], "proxy_groups"),
            (&["proxy", "select", "G", "N"], "proxy_select"),
            (&["proxy", "test", "N"], "proxy_test"),
            (&["proxy", "test", "G", "--group"], "proxy_test_group"),
            (&["profile", "list"], "profile_list"),
            (&["profile", "use", "p1"], "profile_use"),
            (&["profile", "update", "p1"], "profile_update"),
            (&["profile", "import", "https://x"], "profile_import"),
            (&["profile", "show", "p1"], "profile_show"),
            (&["sysproxy", "status"], "sysproxy_status"),
            (&["sysproxy", "on"], "sysproxy_set"),
            (&["tun", "off"], "tun_set"),
            (&["conn", "list"], "conn_list"),
            (&["conn", "close", "--all"], "conn_close"),
            (&["logs", "tail"], "logs_tail"),
            (&["traffic"], "traffic"),
            (&["config", "get"], "config_get"),
            (&["config", "patch", "{}"], "config_patch"),
        ] {
            let plan = parse(arguments).command.plan().unwrap().expect("a plan");
            assert_eq!(plan.name, name, "{arguments:?}");
            assert!(
                crate::commands::find(plan.name).is_some(),
                "{name} is not curated"
            );
        }
    }

    #[test]
    fn local_and_generic_subcommands_have_no_plan() {
        for arguments in [
            &["methods"][..],
            &["remote", "status"],
            &["call", "get_core_status"],
            &["context", "list"],
            &["install"],
            &["skill", "path"],
            &["completion", "bash"],
        ] {
            assert!(parse(arguments).command.plan().unwrap().is_none());
        }
    }

    #[test]
    fn follow_flags_attach_streams() {
        let plan = |arguments: &[&str]| parse(arguments).command.plan().unwrap().unwrap();
        assert_eq!(plan(&["logs", "tail", "-f"]).follow, Some(Follow::Logs));
        assert_eq!(plan(&["traffic", "-f"]).follow, Some(Follow::Traffic));
        assert_eq!(
            plan(&["conn", "list", "-f"]).follow,
            Some(Follow::Connections)
        );
        assert_eq!(plan(&["traffic"]).follow, None);
    }

    #[test]
    fn closing_connections_needs_an_id_or_all() {
        let error = parse(&["conn", "close"]).command.plan().unwrap_err();
        assert!(matches!(error, CliError::Usage { .. }));
        assert!(Cli::try_parse_from(["nyanpasu-cli", "conn", "close", "x", "--all"]).is_err());
    }

    #[test]
    fn json_arguments_must_be_objects() {
        assert!(parse_json_object(r#"{"a":1}"#).is_ok());
        for bad in ["[1]", "3", "not json"] {
            assert!(parse_json_object(bad).is_err(), "{bad}");
        }
        let error = parse(&["config", "patch", "[]"])
            .command
            .plan()
            .unwrap_err();
        assert!(matches!(error, CliError::Usage { .. }));
    }

    #[test]
    fn a_token_is_never_a_command_line_value() {
        for arguments in [
            &[
                "context", "add", "x", "--url", "http://h", "--token", "secret",
            ][..],
            &["context", "add", "x", "--url", "http://h", "--token=secret"],
            &["--token", "secret", "status"],
        ] {
            assert!(
                Cli::try_parse_from(
                    std::iter::once("nyanpasu-cli").chain(arguments.iter().copied())
                )
                .is_err(),
                "{arguments:?}"
            );
        }
        assert!(
            Cli::try_parse_from([
                "nyanpasu-cli",
                "context",
                "add",
                "x",
                "--url",
                "http://h",
                "--token-stdin"
            ])
            .is_ok()
        );
    }

    #[test]
    fn context_add_needs_exactly_one_target() {
        assert!(Cli::try_parse_from(["nyanpasu-cli", "context", "add", "x"]).is_err());
        assert!(
            Cli::try_parse_from([
                "nyanpasu-cli",
                "context",
                "add",
                "x",
                "--socket",
                "--url",
                "http://h"
            ])
            .is_err()
        );
        assert!(Cli::try_parse_from(["nyanpasu-cli", "context", "add", "x", "--socket"]).is_ok());
    }

    #[test]
    fn remote_subcommands_parse() {
        for arguments in [
            &["remote", "status"][..],
            &["remote", "http", "on"],
            &["remote", "http", "off"],
            &["remote", "http", "strategy", "local_open"],
            &["remote", "http", "strategy", "local-authenticated"],
            &["remote", "socket", "on"],
            &["remote", "socket", "off"],
            &["remote", "token", "create", "laptop", "--role", "viewer"],
            &[
                "remote",
                "token",
                "create",
                "ci",
                "--permission",
                "proxy:read",
                "--permission",
                "logs:write",
                "--expires-in",
                "30d",
            ],
            &["remote", "token", "list"],
            &["remote", "token", "revoke", "tok_1"],
            &["remote", "session", "list"],
            &["remote", "session", "revoke", "sess_1"],
        ] {
            assert!(
                Cli::try_parse_from(
                    std::iter::once("nyanpasu-cli").chain(arguments.iter().copied())
                )
                .is_ok(),
                "{arguments:?}"
            );
        }
    }

    #[test]
    fn a_token_is_created_with_a_role_or_permissions_and_never_remote() {
        for arguments in [
            &["remote", "token", "create", "laptop"][..],
            &["remote", "token", "create", "laptop", "--role", "owner"],
            &["remote", "token", "create", "laptop", "--role", "anonymous"],
            &[
                "remote",
                "token",
                "create",
                "laptop",
                "--permission",
                "remote:read",
            ],
            &[
                "remote",
                "token",
                "create",
                "laptop",
                "--permission",
                "meta:read",
            ],
            &[
                "remote",
                "token",
                "create",
                "laptop",
                "--permission",
                "proxy",
            ],
            &[
                "remote",
                "token",
                "create",
                "laptop",
                "--role",
                "viewer",
                "--permission",
                "proxy:write",
            ],
            &[
                "remote",
                "token",
                "create",
                "laptop",
                "--role",
                "viewer",
                "--expires-in",
                "0d",
            ],
            &[
                "remote",
                "token",
                "create",
                "laptop",
                "--role",
                "viewer",
                "--expires-in",
                "soon",
            ],
            &["remote", "http", "strategy", "internet_open"],
            &["remote", "http", "lan-bind", "not-an-address"],
            &["remote", "http", "tls", "plain"],
        ] {
            assert!(
                Cli::try_parse_from(
                    std::iter::once("nyanpasu-cli").chain(arguments.iter().copied())
                )
                .is_err(),
                "{arguments:?}"
            );
        }
        let Command::Remote(RemoteCommand::Token(TokenCommand::Create(create))) = parse(&[
            "remote",
            "token",
            "create",
            "ci",
            "--permission",
            "proxy:read",
            "--permission",
            "logs:read",
            "--permission",
            "proxy:write",
        ])
        .command
        else {
            panic!("a token create command");
        };
        assert_eq!(
            serde_json::to_value(create.grant()).unwrap(),
            serde_json::json!({ "permissions": { "proxy": "write", "logs": "read" } })
        );
        assert_eq!(create.expires_in, None);

        let Command::Remote(RemoteCommand::Token(TokenCommand::Create(create))) =
            parse(&["remote", "token", "create", "ci", "--role", "operator"]).command
        else {
            panic!("a token create command");
        };
        assert_eq!(
            serde_json::to_value(create.grant()).unwrap(),
            serde_json::json!({ "role": "operator" })
        );
    }

    #[test]
    fn lifetimes_are_positive_whole_units() {
        for (text, seconds) in [
            ("90s", 90),
            ("15m", 900),
            ("12h", 43_200),
            ("30d", 2_592_000),
        ] {
            assert_eq!(
                parse_lifetime(text),
                Ok(Duration::from_secs(seconds)),
                "{text}"
            );
        }
        for bad in ["", "d", "0s", "-5m", "1.5h", "10w", "18446744073709551615d"] {
            assert!(parse_lifetime(bad).is_err(), "{bad}");
        }
    }
}
