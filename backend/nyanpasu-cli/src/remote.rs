//! `nyanpasu-cli remote`: remote access administration over the local socket.
//!
//! `remote_admin` is held only by the desktop and the same-user local socket: no bearer token
//! or browser session can carry it and HTTP never offers its commands. These subcommands
//! therefore refuse any other target before connecting. They are deliberately not part of the
//! curated table in `commands.rs`, so an agent cannot be handed remote administration by
//! accident.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nyanpasu_rpc_wire::RpcErrorKind;
use serde_json::{Map, Value, json};

use crate::{
    cli::{
        HttpCommand, HttpStrategy, LanBind, RemoteCommand, SessionCommand, SwitchOnOff,
        TokenCommand, TokenCreate,
    },
    describe::{Capabilities, Scope},
    error::{CliError, ExitStatus},
    output::Output,
    target::{ResolvedTarget, TargetKind},
    transport::Transport,
};

const APP_SETTINGS: &str = "get_app_settings";
const HTTP_STATUS: &str = "get_http_server_status";
const SOCKET_STATUS: &str = "get_local_socket_status";
const SET_REMOTE_ACCESS: &str = "set_remote_access";
const RESET_TLS: &str = "reset_remote_access_tls";
const LIST_TOKENS: &str = "list_remote_access_tokens";
const CREATE_TOKEN: &str = "create_remote_access_token";
const REVOKE_TOKEN: &str = "revoke_remote_access_token";
const LIST_SESSIONS: &str = "list_remote_browser_sessions";
const REVOKE_SESSION: &str = "revoke_remote_browser_session";

/// Refuses every target but the local socket, before any connection is made.
///
/// # Errors
/// An HTTP target is `Unsupported`: remote administration is never offered over HTTP.
pub fn require_local_socket(target: &ResolvedTarget) -> Result<(), CliError> {
    match target.kind {
        TargetKind::LocalSocket { .. } => Ok(()),
        TargetKind::Http { .. } => Err(CliError::Unsupported {
            message: format!(
                "remote administration is available only over the local socket, not {}; \
                 use `--context` or `--transport socket` to select it",
                target.describe()
            ),
        }),
    }
}

/// Runs one `remote` subcommand on a connection to the local socket.
///
/// # Errors
/// A command the shell does not offer this principal is `Unsupported` or `Unauthenticated`;
/// shell errors keep their own exit status.
pub async fn run<T: Transport>(
    command: &RemoteCommand,
    transport: &T,
    capabilities: &Capabilities,
    output: &mut Output<'_>,
) -> Result<ExitStatus, CliError> {
    if !capabilities.holds(Scope::RemoteAdmin) {
        return Err(CliError::Unauthenticated {
            message: "remote administration needs the remote_admin scope, which this caller \
                      does not hold"
                .to_owned(),
        });
    }
    match command {
        RemoteCommand::Status => status(transport, capabilities, output).await,
        RemoteCommand::Http(HttpCommand::On) => {
            change(transport, capabilities, output, |config| {
                set(config, "/http/enabled", json!(true))
            })
            .await
        }
        RemoteCommand::Http(HttpCommand::Off) => {
            change(transport, capabilities, output, |config| {
                set(config, "/http/enabled", json!(false))
            })
            .await
        }
        RemoteCommand::Http(HttpCommand::Strategy { strategy }) => {
            if *strategy == HttpStrategy::LocalOpen {
                writeln!(
                    output.stderr,
                    "warning: local_open lets every local process of every OS user call the \
                     HTTP API with the configured open scopes"
                )?;
            }
            if *strategy == HttpStrategy::LanAuthenticated {
                writeln!(
                    output.stderr,
                    "warning: lan_authenticated lets every device that can reach this \
                     computer's network try to sign in over HTTPS; keep tokens narrow and \
                     compare the certificate fingerprint before pinning it"
                )?;
            }
            change(transport, capabilities, output, |config| {
                set(config, "/http/strategy", json!(strategy.as_str()))?;
                // Credentials must not cross the LAN in the clear, so this strategy brings TLS.
                if *strategy == HttpStrategy::LanAuthenticated {
                    set(config, "/http/tls", json!("self_signed"))?;
                }
                Ok(())
            })
            .await
        }
        RemoteCommand::Http(HttpCommand::LanBind { address }) => {
            let value = match address {
                LanBind::All => Value::Null,
                LanBind::Address(address) => json!(address.to_string()),
            };
            change(transport, capabilities, output, |config| {
                set(config, "/http/lan_bind", value)
            })
            .await
        }
        RemoteCommand::Http(HttpCommand::Tls { mode }) => {
            change(transport, capabilities, output, |config| {
                set(config, "/http/tls", json!(mode.as_str()))
            })
            .await
        }
        RemoteCommand::ResetCertificate => {
            capabilities.require(RESET_TLS)?;
            let value = call(transport, capabilities, RESET_TLS, json!({})).await?;
            Ok(output.result(&value)?)
        }
        RemoteCommand::Socket(switch) => {
            let enabled = matches!(switch, SwitchOnOff::On);
            change(transport, capabilities, output, |config| {
                set(config, "/local_socket/enabled", json!(enabled))
            })
            .await
        }
        RemoteCommand::Token(TokenCommand::Create(arguments)) => {
            create_token(transport, capabilities, arguments, output).await
        }
        RemoteCommand::Token(TokenCommand::List) => {
            read(transport, capabilities, LIST_TOKENS, output).await
        }
        RemoteCommand::Token(TokenCommand::Revoke { id }) => {
            revoke(transport, capabilities, REVOKE_TOKEN, id, output).await
        }
        RemoteCommand::Session(SessionCommand::List) => {
            read(transport, capabilities, LIST_SESSIONS, output).await
        }
        RemoteCommand::Session(SessionCommand::Revoke { id }) => {
            revoke(transport, capabilities, REVOKE_SESSION, id, output).await
        }
    }
}

/// Calls a method the principal must be allowed to call.
async fn call<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
    method: &str,
    params: Value,
) -> Result<Value, CliError> {
    capabilities.require(method)?;
    transport.call(method, params).await
}

async fn read<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
    method: &str,
    output: &mut Output<'_>,
) -> Result<ExitStatus, CliError> {
    let value = call(transport, capabilities, method, json!({})).await?;
    Ok(output.result(&value)?)
}

async fn revoke<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
    method: &str,
    id: &str,
    output: &mut Output<'_>,
) -> Result<ExitStatus, CliError> {
    let value = call(transport, capabilities, method, json!({ "id": id })).await?;
    Ok(output.result(&value)?)
}

/// The committed settings, the HTTP server and the local socket, side by side.
///
/// A host that cannot report one listener, such as a headless server without a local-socket
/// owner, says so for that part and still shows the rest.
async fn status<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
    output: &mut Output<'_>,
) -> Result<ExitStatus, CliError> {
    let settings = committed_config(transport, capabilities).await?;
    let http = listener_status(call(transport, capabilities, HTTP_STATUS, json!({})).await)?;
    let local_socket =
        listener_status(call(transport, capabilities, SOCKET_STATUS, json!({})).await)?;
    Ok(output.result(&json!({
        "settings": settings,
        "http": http,
        "local_socket": local_socket,
    }))?)
}

/// A listener status, or why the host cannot report it.
fn listener_status(reply: Result<Value, CliError>) -> Result<Value, CliError> {
    match reply {
        Err(CliError::Remote { error }) if error.error_kind() == RpcErrorKind::ApplicationError => {
            Ok(json!({ "unavailable": error.message }))
        }
        other => other,
    }
}

async fn committed_config<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
) -> Result<Value, CliError> {
    let settings = call(transport, capabilities, APP_SETTINGS, json!({})).await?;
    settings
        .get("remote_access")
        .cloned()
        .ok_or_else(|| CliError::failed("the shell returned settings without `remote_access`"))
}

/// Edits the committed `remote_access` section and writes it back as a whole.
///
/// `set_remote_access` replaces the section, as the desktop settings page does, so a change
/// made between the read and the write by another administrator is overwritten.
async fn change<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
    output: &mut Output<'_>,
    edit: impl FnOnce(&mut Value) -> Result<(), CliError>,
) -> Result<ExitStatus, CliError> {
    capabilities.require(SET_REMOTE_ACCESS)?;
    let mut config = committed_config(transport, capabilities).await?;
    edit(&mut config)?;
    let value = call(
        transport,
        capabilities,
        SET_REMOTE_ACCESS,
        json!({ "config": config }),
    )
    .await?;
    Ok(output.result(&value)?)
}

/// Sets a field the committed settings must already have.
fn set(config: &mut Value, pointer: &str, value: Value) -> Result<(), CliError> {
    *config
        .pointer_mut(pointer)
        .ok_or_else(|| CliError::failed(format!("the settings have no `{pointer}`")))? = value;
    Ok(())
}

async fn create_token<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
    arguments: &TokenCreate,
    output: &mut Output<'_>,
) -> Result<ExitStatus, CliError> {
    let mut request = Map::new();
    request.insert("name".to_owned(), json!(arguments.name));
    request.insert("scopes".to_owned(), json!(arguments.scopes));
    if let Some(lifetime) = arguments.expires_in {
        request.insert("expires_at_ms".to_owned(), json!(expiry_ms(lifetime)?));
    }
    let created = call(
        transport,
        capabilities,
        CREATE_TOKEN,
        json!({ "request": request }),
    )
    .await?;
    if output.json {
        return Ok(output.result(&created)?);
    }
    // The secret exists only in this reply: put it first and say so.
    writeln!(
        output.stderr,
        "note: this token is shown once and cannot be retrieved again"
    )?;
    Ok(output.result(&flatten_created_token(&created))?)
}

/// The token's secret ahead of its metadata, as one table.
fn flatten_created_token(created: &Value) -> Value {
    let (Some(token), Some(Value::Object(summary))) =
        (created.get("token"), created.get("summary"))
    else {
        return created.clone();
    };
    let mut flat = Map::new();
    flat.insert("token".to_owned(), token.clone());
    flat.extend(summary.clone());
    Value::Object(flat)
}

/// The wall-clock expiry, in the milliseconds since the Unix epoch the shell expects.
fn expiry_ms(lifetime: Duration) -> Result<i64, CliError> {
    let too_far = || CliError::usage("the expiry is too far in the future");
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CliError::failed("the system clock is before 1970"))?;
    let expiry = now.checked_add(lifetime).ok_or_else(too_far)?;
    i64::try_from(expiry.as_millis()).map_err(|_| too_far())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        describe::fixtures,
        target::{TargetSource, parse_http_url},
        transport::fakes::FakeTransport,
    };

    fn target(kind: TargetKind) -> ResolvedTarget {
        ResolvedTarget {
            source: TargetSource::DefaultSocket,
            kind,
            context: None,
            token_backend: None,
            fingerprint: None,
        }
    }

    #[test]
    fn only_the_local_socket_may_administer_remote_access() {
        assert!(
            require_local_socket(&target(TargetKind::LocalSocket {
                endpoint: Some(PathBuf::from("/run/rpc.sock"))
            }))
            .is_ok()
        );
        let http = target(TargetKind::Http {
            url: parse_http_url("http://127.0.0.1:9090").unwrap(),
        });
        let error = require_local_socket(&http).unwrap_err();
        assert!(matches!(error, CliError::Unsupported { .. }), "{error}");
        assert_eq!(error.exit_status(), ExitStatus::Unsupported);
    }

    #[test]
    fn a_secret_is_listed_before_its_metadata() {
        let flat = flatten_created_token(&json!({
            "summary": {"id": "tok_1", "name": "laptop"},
            "token": "npk_tok_1_secret",
        }));
        let keys: Vec<_> = flat.as_object().unwrap().keys().collect();
        assert_eq!(keys.first().map(|key| key.as_str()), Some("token"));
        assert_eq!(flat["id"], "tok_1");
        let untouched = json!({"unexpected": true});
        assert_eq!(flatten_created_token(&untouched), untouched);
    }

    #[test]
    fn expiry_is_a_future_instant_in_milliseconds() {
        let before = i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        let expiry = expiry_ms(Duration::from_secs(60)).unwrap();
        assert!(expiry >= before + 60_000, "{expiry} vs {before}");
        assert!(expiry < before + 120_000);
        assert!(matches!(
            expiry_ms(Duration::MAX),
            Err(CliError::Usage { .. })
        ));
    }

    #[test]
    fn a_listener_the_host_cannot_report_is_marked_and_other_failures_still_fail() {
        let remote = |kind: &str| {
            Err(CliError::from_remote(nyanpasu_rpc_wire::RpcError {
                kind: kind.to_owned(),
                message: "not supported".to_owned(),
                code: None,
                retryable: None,
                operation_id: None,
                domain_error: None,
            }))
        };
        assert_eq!(
            listener_status(remote("application_error")).unwrap(),
            json!({"unavailable": "not supported"})
        );
        assert!(listener_status(remote("forbidden")).is_err());
        assert_eq!(
            listener_status(Ok(json!({"enabled": false}))).unwrap(),
            json!({"enabled": false})
        );
    }

    #[test]
    fn editing_requires_the_field_to_exist() {
        let mut config = json!({"http": {"enabled": false}});
        set(&mut config, "/http/enabled", json!(true)).unwrap();
        assert_eq!(config["http"]["enabled"], true);
        assert!(set(&mut config, "/local_socket/enabled", json!(true)).is_err());
    }

    #[tokio::test]
    async fn a_caller_without_remote_admin_is_refused_before_any_call() {
        let capabilities = fixtures::capabilities(&[Scope::Read], &[(APP_SETTINGS, Scope::Read)]);
        let transport = FakeTransport::new(Vec::new());
        let (mut stdout, mut stderr) = (Vec::new(), Vec::new());
        let mut output = Output {
            json: false,
            stdout: &mut stdout,
            stderr: &mut stderr,
        };
        let error = run(
            &RemoteCommand::Status,
            &transport,
            &capabilities,
            &mut output,
        )
        .await
        .unwrap_err();
        assert_eq!(error.exit_status(), ExitStatus::Unauthenticated);
        assert!(transport.calls.lock().unwrap().is_empty());
    }
}
