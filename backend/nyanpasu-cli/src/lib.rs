//! `nyanpasu-cli`: calls one command of a running Nyanpasu shell over its local
//! socket and prints the reply as JSON.
//!
//! The CLI is a client, not a composition root: it links only the wire contract
//! and never builds the application, so consecutive calls observe the one
//! session the server holds. The router form is LuCI, then this CLI, then the
//! server.
//!
//! Standard output carries exactly one JSON document and the exit code says
//! what it is:
//!
//! | Exit | Output                                                              |
//! | ---- | ------------------------------------------------------------------- |
//! | 0    | the command's result                                                |
//! | 1    | the `RpcError` the server returned                                  |
//! | 2    | a transport error object; a usage error prints to standard error    |

use std::{path::PathBuf, time::Duration};

use nyanpasu_rpc_protocol::{LocalCallError, LocalClient};
use serde_json::{Value, json};
use snafu::{ResultExt as _, Snafu};

pub const DEFAULT_SOCKET: &str = "/var/run/nyanpasu/rpc.sock";
/// A reconcile can take about 30 seconds, so the default leaves room for it.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(90);

pub const EXIT_SUCCESS: u8 = 0;
pub const EXIT_RPC_ERROR: u8 = 1;
pub const EXIT_FAILURE: u8 = 2;

/// The command `methods` calls, since the server is the source of truth for
/// what it serves.
const LIST_METHODS: &str = "list_methods";

pub const USAGE: &str = "usage: nyanpasu-cli [--socket PATH] [--timeout SECS] <command>

commands:
  call METHOD [--params JSON | --params-file FILE]
  methods";

#[derive(Debug, PartialEq, Eq)]
pub struct Cli {
    pub socket: PathBuf,
    pub timeout: Duration,
    pub command: Command,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Call { method: String, params: Params },
    Methods,
}

/// Where a call's parameters come from; a call without any sends `{}`.
#[derive(Debug, PartialEq, Eq)]
pub enum Params {
    Empty,
    Inline(String),
    File(PathBuf),
}

#[derive(Debug, Snafu)]
pub enum UsageError {
    #[snafu(display("{message}"))]
    Arguments { message: String },
    #[snafu(display("--params is not valid JSON"))]
    InlineParams { source: serde_json::Error },
    #[snafu(display("could not read {}", path.display()))]
    ReadParamsFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("{} is not valid JSON", path.display()))]
    FileParams {
        path: PathBuf,
        source: serde_json::Error,
    },
}

fn arguments(message: impl Into<String>) -> UsageError {
    UsageError::Arguments {
        message: message.into(),
    }
}

impl Cli {
    /// Parses the arguments after the program name.
    ///
    /// # Errors
    /// Returns a usage error for an unknown or incomplete argument.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, UsageError> {
        let mut args = args.into_iter();
        let mut socket = PathBuf::from(DEFAULT_SOCKET);
        let mut timeout = DEFAULT_TIMEOUT;
        let command = loop {
            let Some(arg) = args.next() else {
                return Err(arguments("a command is required"));
            };
            match arg.as_str() {
                "--socket" => {
                    socket = args
                        .next()
                        .ok_or_else(|| arguments("--socket needs a value"))?
                        .into();
                }
                "--timeout" => {
                    let seconds = args
                        .next()
                        .ok_or_else(|| arguments("--timeout needs a value"))?;
                    timeout = match seconds.parse::<u64>() {
                        Ok(seconds) if seconds > 0 => Duration::from_secs(seconds),
                        _ => return Err(arguments("--timeout is a whole number of seconds")),
                    };
                }
                "call" => break parse_call(&mut args)?,
                "methods" => break Command::Methods,
                other => return Err(arguments(format!("unknown argument {other:?}"))),
            }
        };
        if let Some(extra) = args.next() {
            return Err(arguments(format!("unexpected argument {extra:?}")));
        }
        Ok(Self {
            socket,
            timeout,
            command,
        })
    }
}

fn parse_call(args: &mut impl Iterator<Item = String>) -> Result<Command, UsageError> {
    let method = args
        .next()
        .ok_or_else(|| arguments("call needs a method name"))?;
    let mut params = Params::Empty;
    while let Some(flag) = args.next() {
        let source: fn(String) -> Params = match flag.as_str() {
            "--params" => Params::Inline,
            "--params-file" => |path| Params::File(path.into()),
            other => return Err(arguments(format!("unexpected argument {other:?}"))),
        };
        if params != Params::Empty {
            return Err(arguments(
                "give the parameters once, with --params or --params-file",
            ));
        }
        let value = args
            .next()
            .ok_or_else(|| arguments(format!("{flag} needs a value")))?;
        params = source(value);
    }
    Ok(Command::Call { method, params })
}

impl Params {
    /// The JSON the call sends.
    ///
    /// # Errors
    /// Returns a usage error when the file cannot be read or is not JSON.
    pub fn load(&self) -> Result<Value, UsageError> {
        match self {
            Self::Empty => Ok(json!({})),
            Self::Inline(text) => serde_json::from_str(text).context(InlineParamsSnafu),
            Self::File(path) => {
                let text = std::fs::read_to_string(path).context(ReadParamsFileSnafu { path })?;
                serde_json::from_str(&text).context(FileParamsSnafu { path })
            }
        }
    }
}

/// What to print and how to exit.
#[derive(Debug, PartialEq)]
pub struct Reply {
    pub output: Value,
    pub exit_code: u8,
}

/// Runs the command against the server and returns its reply.
///
/// # Errors
/// Returns a usage error when the parameters cannot be loaded; every failure
/// after that is part of the [`Reply`].
pub async fn execute(cli: &Cli) -> Result<Reply, UsageError> {
    let (method, params) = match &cli.command {
        Command::Call { method, params } => (method.as_str(), params.load()?),
        Command::Methods => (LIST_METHODS, json!({})),
    };
    let mut client = match LocalClient::connect(&cli.socket).await {
        Ok(client) => client,
        Err(error) => return Ok(transport_failure(&error)),
    };
    Ok(call(&mut client, method, params, cli.timeout).await)
}

/// Calls `method` on a connected client and turns the outcome into a reply.
///
/// A deadline reports an unknown outcome and does not cancel the call: the
/// server finishes work it has started.
pub async fn call(
    client: &mut LocalClient,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Reply {
    match client.call_with_timeout(method, params, timeout).await {
        Ok(Ok(result)) => Reply {
            output: result,
            exit_code: EXIT_SUCCESS,
        },
        Ok(Err(error)) => Reply {
            output: serde_json::to_value(&error).expect("an RpcError is plain JSON"),
            exit_code: EXIT_RPC_ERROR,
        },
        Err(error) => transport_failure(&error),
    }
}

fn transport_failure(error: &LocalCallError) -> Reply {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    Reply {
        output: json!({
            "kind": "transport_error",
            "message": message,
            "outcome_unknown": error.is_outcome_unknown(),
        }),
        exit_code: EXIT_FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use nyanpasu_rpc_protocol::{
        RpcError, RpcErrorKind,
        frame::{self, LocalRequest, LocalResponse},
    };

    use super::*;

    fn parse(args: &[&str]) -> Result<Cli, UsageError> {
        Cli::parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn a_call_defaults_to_the_router_socket_and_a_long_enough_timeout() {
        let cli = parse(&["call", "get_core_status"]).unwrap();

        assert_eq!(
            cli,
            Cli {
                socket: PathBuf::from(DEFAULT_SOCKET),
                timeout: DEFAULT_TIMEOUT,
                command: Command::Call {
                    method: "get_core_status".into(),
                    params: Params::Empty,
                },
            }
        );
        assert!(DEFAULT_TIMEOUT >= Duration::from_secs(60));
    }

    #[test]
    fn the_global_options_and_either_parameter_source_are_accepted() {
        let cli = parse(&[
            "--socket",
            "/tmp/s",
            "--timeout",
            "5",
            "call",
            "activate_profile",
            "--params",
            r#"{"uid":"a"}"#,
        ])
        .unwrap();
        assert_eq!(cli.socket, PathBuf::from("/tmp/s"));
        assert_eq!(cli.timeout, Duration::from_secs(5));
        assert_eq!(
            cli.command,
            Command::Call {
                method: "activate_profile".into(),
                params: Params::Inline(r#"{"uid":"a"}"#.into()),
            }
        );

        let cli = parse(&["call", "create_profile", "--params-file", "/tmp/p.json"]).unwrap();
        assert_eq!(
            cli.command,
            Command::Call {
                method: "create_profile".into(),
                params: Params::File("/tmp/p.json".into()),
            }
        );
        assert_eq!(parse(&["methods"]).unwrap().command, Command::Methods);
    }

    #[test]
    fn malformed_command_lines_are_usage_errors() {
        for args in [
            &[][..],
            &["--socket"],
            &["--timeout", "0"],
            &["--timeout", "soon", "methods"],
            &["call"],
            &["call", "m", "--params"],
            &["call", "m", "--params", "{}", "--params-file", "f"],
            &["call", "m", "--verbose", "1"],
            &["call", "m", "stray"],
            &["methods", "stray"],
            &["frobnicate"],
        ] {
            assert!(
                matches!(parse(args), Err(UsageError::Arguments { .. })),
                "{args:?}"
            );
        }
    }

    #[test]
    fn parameters_are_json_from_the_command_line_a_file_or_nothing() {
        assert_eq!(Params::Empty.load().unwrap(), json!({}));
        assert_eq!(
            Params::Inline(r#"{"uid":"a"}"#.into()).load().unwrap(),
            json!({"uid": "a"})
        );
        assert!(matches!(
            Params::Inline("{".into()).load(),
            Err(UsageError::InlineParams { .. })
        ));

        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("params.json");
        std::fs::write(&file, r#"{"ids":[]}"#).unwrap();
        assert_eq!(
            Params::File(file.clone()).load().unwrap(),
            json!({"ids": []})
        );
        std::fs::write(&file, "nope").unwrap();
        assert!(matches!(
            Params::File(file).load(),
            Err(UsageError::FileParams { .. })
        ));
        assert!(matches!(
            Params::File(directory.path().join("missing.json")).load(),
            Err(UsageError::ReadParamsFile { .. })
        ));
    }

    /// Answers the next request on `server` with `reply`.
    async fn answer(server: &mut tokio::io::DuplexStream, reply: Result<Value, RpcError>) {
        let body = frame::read_frame(server).await.unwrap().unwrap();
        let request: LocalRequest = serde_json::from_slice(&body).unwrap();
        let frame = frame::encode(&LocalResponse::new(Some(request.id), reply)).unwrap();
        frame::write_frame(server, &frame).await.unwrap();
    }

    #[tokio::test]
    async fn a_result_prints_as_is_and_exits_zero() {
        let (client, mut server) = tokio::io::duplex(4096);
        let mut client = LocalClient::from_stream(client);
        let server = tokio::spawn(async move {
            answer(&mut server, Ok(json!({"state": "running"}))).await;
        });

        let reply = call(&mut client, "get_core_status", json!({}), DEFAULT_TIMEOUT).await;

        assert_eq!(
            reply,
            Reply {
                output: json!({"state": "running"}),
                exit_code: EXIT_SUCCESS,
            }
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn an_rpc_error_prints_the_envelope_and_exits_one() {
        let (client, mut server) = tokio::io::duplex(4096);
        let mut client = LocalClient::from_stream(client);
        let server = tokio::spawn(async move {
            answer(&mut server, Err(RpcError::unknown_method("nope"))).await;
        });

        let reply = call(&mut client, "nope", json!({}), DEFAULT_TIMEOUT).await;

        assert_eq!(reply.exit_code, EXIT_RPC_ERROR);
        assert_eq!(reply.output["kind"], RpcErrorKind::UnknownMethod.as_str());
        assert_eq!(reply.output["message"], "unknown command `nope`");
        server.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn a_deadline_reports_an_unknown_outcome_and_exits_two() {
        let (client, _server) = tokio::io::duplex(4096);
        let mut client = LocalClient::from_stream(client);

        let reply = call(&mut client, "stop_core", json!({}), Duration::from_secs(90)).await;

        assert_eq!(reply.exit_code, EXIT_FAILURE);
        assert_eq!(reply.output["kind"], "transport_error");
        assert_eq!(reply.output["outcome_unknown"], true);
        assert!(
            reply.output["message"]
                .as_str()
                .unwrap()
                .contains("outcome is unknown")
        );
    }

    #[tokio::test]
    async fn a_connection_closed_before_the_reply_is_an_unknown_outcome() {
        let (client, mut server) = tokio::io::duplex(4096);
        let mut client = LocalClient::from_stream(client);
        let server = tokio::spawn(async move {
            frame::read_frame(&mut server).await.unwrap().unwrap();
        });

        let reply = call(&mut client, "reconcile_core", json!({}), DEFAULT_TIMEOUT).await;

        assert_eq!(reply.exit_code, EXIT_FAILURE);
        assert_eq!(reply.output["outcome_unknown"], true);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn no_server_is_a_transport_error_where_the_call_never_ran() {
        let cli = Cli {
            socket: std::env::temp_dir().join("nyanpasu-cli-no-such-socket.sock"),
            timeout: DEFAULT_TIMEOUT,
            command: Command::Methods,
        };

        let reply = execute(&cli).await.unwrap();

        assert_eq!(reply.exit_code, EXIT_FAILURE);
        assert_eq!(reply.output["kind"], "transport_error");
        assert_eq!(reply.output["outcome_unknown"], false);
    }
}
