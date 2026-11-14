//! `nyanpasu-cli mcp`: an MCP server over standard input and output.
//!
//! It speaks newline-delimited JSON-RPC 2.0 (`initialize`, `ping`, `tools/list`,
//! `tools/call`) and reuses the CLI's connection, contexts and credentials. It is
//! read-only by default: it registers only the curated `read` tools. `--scope` adds
//! scopes, always intersected with the principal's scopes from `rpc_describe`, and
//! `remote_admin` is never offered. `--raw` adds a generic `call_method` tool that is
//! bounded by the same scopes.

use std::collections::BTreeSet;

use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt as _, AsyncWrite, AsyncWriteExt as _};

use crate::{
    commands::{COMMANDS, CuratedCommand, Effect, Invocation},
    describe::{Availability, Capabilities, Scope},
    error::CliError,
    output::degradations,
    transport::{StreamKind, Transport},
};

/// The MCP protocol revision this server answers with when the client's is unknown.
const PROTOCOL_VERSION: &str = "2025-06-18";
/// Protocol revisions this server can speak: its tools use nothing newer.
const SUPPORTED_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];
const RAW_TOOL: &str = "call_method";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// What `mcp` was asked to offer.
#[derive(Debug, Clone, Default)]
pub struct McpOptions {
    /// Scopes requested with `--scope`, beyond the default `read`.
    pub scopes: Vec<Scope>,
    /// Offer the generic `call_method` tool.
    pub raw: bool,
}

impl McpOptions {
    /// Parses `--scope` values.
    ///
    /// # Errors
    /// Unknown scope names and `remote_admin` are usage errors: it is never offered.
    pub fn parse(scopes: &[String], raw: bool) -> Result<Self, CliError> {
        let scopes = scopes
            .iter()
            .map(|name| {
                let scope: Scope = name.parse().map_err(|error| {
                    CliError::usage(format!(
                        "{error}; known scopes: {}",
                        Scope::ALL.map(Scope::as_str).join(", ")
                    ))
                })?;
                if scope == Scope::RemoteAdmin {
                    return Err(CliError::usage(
                        "remote_admin is never offered over MCP; manage remote access from the app",
                    ));
                }
                Ok(scope)
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { scopes, raw })
    }

    /// The scopes the server may offer: `read` plus the requested ones, intersected with
    /// the caller's own, and never `remote_admin`.
    pub fn effective_scopes(&self, capabilities: &Capabilities) -> BTreeSet<Scope> {
        std::iter::once(Scope::Read)
            .chain(self.scopes.iter().copied())
            .filter(|scope| *scope != Scope::RemoteAdmin && capabilities.holds(*scope))
            .collect()
    }
}

/// A curated operation offered as a tool.
fn offered(
    command: &CuratedCommand,
    scopes: &BTreeSet<Scope>,
    capabilities: &Capabilities,
) -> bool {
    if !scopes.contains(&command.scope) {
        return false;
    }
    match command.invocation {
        Invocation::Method(method) => capabilities.availability(method) == Availability::Allowed,
        Invocation::ConnectionDetails => true,
    }
}

/// The curated tools this session may offer.
pub fn visible_commands(
    options: &McpOptions,
    capabilities: &Capabilities,
) -> Vec<&'static CuratedCommand> {
    let scopes = options.effective_scopes(capabilities);
    COMMANDS
        .iter()
        .filter(|command| offered(command, &scopes, capabilities))
        .collect()
}

fn annotations(effect: Effect) -> Value {
    match effect {
        Effect::Query => json!({"readOnlyHint": true, "destructiveHint": false}),
        Effect::Mutation { destructive } => {
            json!({"readOnlyHint": false, "destructiveHint": destructive})
        }
    }
}

fn tool_definition(command: &CuratedCommand) -> Value {
    json!({
        "name": command.name,
        "description": command.description,
        "inputSchema": command.input_schema_value(),
        "annotations": annotations(command.effect),
    })
}

fn raw_tool_definition() -> Value {
    json!({
        "name": RAW_TOOL,
        "description": "Call any RPC method the caller may use, for operations without a curated tool. \
                        Use `rpc_describe` output from the CLI `methods` command to find names.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "method": {"type": "string"},
                "params": {"type": "object"}
            },
            "required": ["method"]
        },
        "annotations": {"readOnlyHint": false, "destructiveHint": true},
    })
}

/// The `tools/list` result.
pub fn tool_definitions(options: &McpOptions, capabilities: &Capabilities) -> Vec<Value> {
    let mut tools: Vec<Value> = visible_commands(options, capabilities)
        .into_iter()
        .map(tool_definition)
        .collect();
    if options.raw {
        tools.push(raw_tool_definition());
    }
    tools
}

/// Whether the raw tool may call `method` under the session's scopes.
fn raw_permitted(
    method: &str,
    scopes: &BTreeSet<Scope>,
    capabilities: &Capabilities,
) -> Result<(), CliError> {
    capabilities.require(method)?;
    let scope = capabilities
        .commands
        .iter()
        .find(|command| command.name == method)
        .and_then(|command| command.scope);
    match scope {
        Some(scope) if scope != Scope::RemoteAdmin && scopes.contains(&scope) => Ok(()),
        Some(scope) => Err(CliError::Unauthenticated {
            message: format!(
                "`{method}` needs the {scope} scope, which this MCP session does not offer"
            ),
        }),
        None => Err(CliError::Unauthenticated {
            message: format!(
                "`{method}` has no known scope, so this MCP session does not offer it"
            ),
        }),
    }
}

/// The text of a tool result: the JSON result, and the degradation when there is one.
fn result_content(value: &Value) -> Vec<Value> {
    let mut content = vec![json!({"type": "text", "text": value.to_string()})];
    let degraded = degradations(value);
    if !degraded.is_empty() {
        content.push(json!({
            "type": "text",
            "text": format!("warning: applied with degradation: {}", degraded.join("; ")),
        }));
    }
    content
}

fn tool_error(message: impl std::fmt::Display) -> Value {
    json!({"content": [{"type": "text", "text": message.to_string()}], "isError": true})
}

/// Runs one `tools/call`. Failures are tool errors; only an unknown tool is a protocol
/// error, reported by the caller through `Err`.
async fn call_tool<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
    options: &McpOptions,
    name: &str,
    arguments: &Value,
) -> Result<Value, String> {
    let scopes = options.effective_scopes(capabilities);
    if name == RAW_TOOL && options.raw {
        let outcome = async {
            let method = arguments
                .get("method")
                .and_then(Value::as_str)
                .ok_or_else(|| CliError::usage("`method` is required and must be a string"))?;
            let params = match arguments.get("params") {
                None | Some(Value::Null) => json!({}),
                Some(params @ Value::Object(_)) => params.clone(),
                Some(_) => return Err(CliError::usage("`params` must be an object")),
            };
            raw_permitted(method, &scopes, capabilities)?;
            transport.call(method, params).await
        }
        .await;
        return Ok(finish(outcome));
    }
    let command = visible_commands(options, capabilities)
        .into_iter()
        .find(|command| command.name == name)
        .ok_or_else(|| format!("unknown tool `{name}`"))?;
    let outcome = async {
        let params = command.params(arguments)?;
        match command.invocation {
            Invocation::Method(method) => transport.call(method, params).await,
            Invocation::ConnectionDetails => {
                let mut feed = transport.events(StreamKind::ConnectionDetails).await?;
                feed.next()
                    .await
                    .unwrap_or_else(|| Err(CliError::failed("the connection stream ended")))
            }
        }
    }
    .await;
    Ok(finish(outcome))
}

fn finish(outcome: Result<Value, CliError>) -> Value {
    match outcome {
        Ok(value) => json!({"content": result_content(&value), "isError": false}),
        Err(error) => tool_error(error),
    }
}

fn response(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn failure(id: &Value, code: i64, message: impl std::fmt::Display) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.to_string()}})
}

/// Handles one JSON-RPC message; `None` for a notification, which gets no reply.
pub async fn handle_message<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
    options: &McpOptions,
    message: &Value,
) -> Option<Value> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return message
            .get("id")
            .map(|id| failure(id, INVALID_REQUEST, "a request needs a method"));
    };
    let id = message.get("id")?;
    let params = message.get("params").unwrap_or(&Value::Null);
    Some(match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str);
            let version = requested
                .filter(|version| SUPPORTED_VERSIONS.contains(version))
                .unwrap_or(PROTOCOL_VERSION);
            response(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "nyanpasu-cli", "version": env!("CARGO_PKG_VERSION")},
                }),
            )
        }
        "ping" => response(id, json!({})),
        "tools/list" => response(
            id,
            json!({"tools": tool_definitions(options, capabilities)}),
        ),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(failure(id, INVALID_PARAMS, "`name` is required"));
            };
            let no_arguments = json!({});
            let arguments = match params.get("arguments") {
                None | Some(Value::Null) => &no_arguments,
                Some(arguments) => arguments,
            };
            match call_tool(transport, capabilities, options, name, arguments).await {
                Ok(result) => response(id, result),
                Err(message) => failure(id, INVALID_PARAMS, message),
            }
        }
        other => failure(id, METHOD_NOT_FOUND, format!("unknown method `{other}`")),
    })
}

/// Serves MCP until the input ends.
///
/// # Errors
/// Reports a failed read or write on the standard streams.
pub async fn serve<T, R, W>(
    transport: &T,
    capabilities: &Capabilities,
    options: &McpOptions,
    mut input: R,
    mut output: W,
) -> std::io::Result<()>
where
    T: Transport,
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line).await? == 0 {
            return Ok(());
        }
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle_message(transport, capabilities, options, &message).await,
            Err(error) => Some(failure(&Value::Null, PARSE_ERROR, error)),
        };
        if let Some(reply) = reply {
            output.write_all(reply.to_string().as_bytes()).await?;
            output.write_all(b"\n").await?;
            output.flush().await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{describe::fixtures, transport::fakes::FakeTransport};

    /// A describe response that allows every curated method, holding `scopes`.
    fn capabilities(scopes: &[&str]) -> Capabilities {
        let allowed: Vec<(&str, &str)> = COMMANDS
            .iter()
            .filter_map(|command| {
                command
                    .method()
                    .map(|method| (method, command.scope.as_str()))
            })
            .collect();
        fixtures::capabilities(scopes, &allowed)
    }

    fn names(options: &McpOptions, capabilities: &Capabilities) -> BTreeSet<&'static str> {
        visible_commands(options, capabilities)
            .into_iter()
            .map(|command| command.name)
            .collect()
    }

    fn all_scopes() -> Vec<&'static str> {
        Scope::ALL.map(Scope::as_str).to_vec()
    }

    #[test]
    fn the_default_session_offers_only_read_tools() {
        let capabilities = capabilities(&all_scopes());
        let offered = names(&McpOptions::default(), &capabilities);
        assert!(!offered.is_empty());
        for command in COMMANDS {
            assert_eq!(
                offered.contains(command.name),
                command.scope == Scope::Read,
                "{} ({})",
                command.name,
                command.scope
            );
        }
        assert!(
            !offered.contains("profile_show"),
            "sensitive_read is opt-in"
        );
    }

    #[test]
    fn scope_flags_add_tools_but_only_within_the_principals_scopes() {
        let options = McpOptions::parse(&["proxy".into(), "profile".into()], false).unwrap();

        let full = names(&options, &capabilities(&all_scopes()));
        assert!(full.contains("proxy_select"));
        assert!(full.contains("profile_use"));
        assert!(!full.contains("core_restart"), "system was not requested");

        let read_only = names(&options, &capabilities(&["read"]));
        assert!(
            !read_only.contains("proxy_select"),
            "the principal lacks proxy"
        );
        assert!(read_only.contains("status"));

        let proxy_only = names(&options, &capabilities(&["read", "proxy"]));
        assert!(proxy_only.contains("proxy_select"));
        assert!(
            !proxy_only.contains("profile_use"),
            "requested but not held"
        );
    }

    #[test]
    fn remote_admin_is_never_offered() {
        let error = McpOptions::parse(&["remote_admin".into()], false).unwrap_err();
        assert!(matches!(error, CliError::Usage { .. }));

        let mut capabilities = capabilities(&all_scopes());
        capabilities
            .commands
            .push(crate::describe::DescribedCommand {
                name: "set_remote_access".into(),
                kind: None,
                scope: Some(Scope::RemoteAdmin),
                allowed: true,
            });
        let options = McpOptions {
            scopes: Scope::ALL.to_vec(),
            raw: true,
        };
        assert!(
            !options
                .effective_scopes(&capabilities)
                .contains(&Scope::RemoteAdmin)
        );
        assert!(
            raw_permitted(
                "set_remote_access",
                &options.effective_scopes(&capabilities),
                &capabilities
            )
            .is_err()
        );
    }

    #[test]
    fn unknown_scope_names_are_usage_errors() {
        assert!(matches!(
            McpOptions::parse(&["bogus".into()], false),
            Err(CliError::Usage { .. })
        ));
    }

    #[test]
    fn a_method_the_shell_does_not_expose_is_not_offered() {
        let capabilities = fixtures::capabilities(&["read"], &[("get_core_status", "read")]);
        assert_eq!(
            names(&McpOptions::default(), &capabilities),
            BTreeSet::from(["status", "conn_list"])
        );
    }

    #[test]
    fn tool_definitions_carry_schemas_and_annotations() {
        let capabilities = capabilities(&all_scopes());
        let options = McpOptions::parse(&["system".into(), "proxy".into()], false).unwrap();
        let tools = tool_definitions(&options, &capabilities);
        let tool = |name: &str| tools.iter().find(|tool| tool["name"] == name).unwrap();

        assert_eq!(tool("status")["annotations"]["readOnlyHint"], true);
        assert_eq!(tool("core_restart")["annotations"]["destructiveHint"], true);
        assert_eq!(tool("core_restart")["annotations"]["readOnlyHint"], false);
        assert_eq!(
            tool("proxy_select")["annotations"]["destructiveHint"],
            false
        );
        assert_eq!(
            tool("proxy_select")["inputSchema"]["required"],
            json!(["group", "name"])
        );
        assert!(
            !tools.iter().any(|tool| tool["name"] == RAW_TOOL),
            "call_method is opt-in"
        );
    }

    #[test]
    fn raw_adds_the_generic_tool_and_bounds_it_by_scope() {
        let capabilities = fixtures::capabilities(
            &["read", "system"],
            &[("get_core_status", "read"), ("restart_sidecar", "system")],
        );
        let options = McpOptions {
            scopes: vec![],
            raw: true,
        };
        assert!(
            tool_definitions(&options, &capabilities)
                .iter()
                .any(|tool| tool["name"] == RAW_TOOL)
        );
        let scopes = options.effective_scopes(&capabilities);
        assert!(raw_permitted("get_core_status", &scopes, &capabilities).is_ok());
        assert!(
            raw_permitted("restart_sidecar", &scopes, &capabilities).is_err(),
            "system was not requested"
        );
        assert!(
            raw_permitted("get_profiles", &scopes, &capabilities).is_err(),
            "not exposed"
        );
    }

    async fn rpc(
        transport: &FakeTransport,
        capabilities: &Capabilities,
        options: &McpOptions,
        message: Value,
    ) -> Value {
        handle_message(transport, capabilities, options, &message)
            .await
            .expect("a request gets a reply")
    }

    #[tokio::test]
    async fn initialize_lists_and_calls_tools() {
        let capabilities = capabilities(&all_scopes());
        let options = McpOptions::default();
        let transport = FakeTransport::new(vec![Ok(json!({"running": true}))]);

        let reply = rpc(
            &transport,
            &capabilities,
            &options,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                   "params": {"protocolVersion": "2025-03-26"}}),
        )
        .await;
        assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(reply["result"]["serverInfo"]["name"], "nyanpasu-cli");
        assert!(reply["result"]["capabilities"]["tools"].is_object());

        let reply = rpc(
            &transport,
            &capabilities,
            &options,
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        )
        .await;
        assert!(
            reply["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["name"] == "status")
        );

        let reply = rpc(
            &transport,
            &capabilities,
            &options,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
                   "params": {"name": "status", "arguments": {}}}),
        )
        .await;
        assert_eq!(reply["result"]["isError"], false);
        assert_eq!(reply["result"]["content"][0]["text"], r#"{"running":true}"#);
        assert_eq!(
            transport.recorded(),
            vec![("get_core_status".to_owned(), json!({}))]
        );
    }

    #[tokio::test]
    async fn an_unlisted_tool_cannot_be_called() {
        let capabilities = capabilities(&all_scopes());
        let transport = FakeTransport::new(vec![]);
        let reply = rpc(
            &transport,
            &capabilities,
            &McpOptions::default(),
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                   "params": {"name": "core_restart", "arguments": {}}}),
        )
        .await;
        assert_eq!(reply["error"]["code"], INVALID_PARAMS);
        assert!(transport.recorded().is_empty(), "nothing reached the shell");

        let reply = rpc(
            &transport,
            &capabilities,
            &McpOptions::default(),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                   "params": {"name": RAW_TOOL, "arguments": {"method": "get_core_status"}}}),
        )
        .await;
        assert_eq!(reply["error"]["code"], INVALID_PARAMS, "raw is opt-in");
    }

    #[tokio::test]
    async fn tool_failures_and_degradation_are_reported_in_the_result() {
        let capabilities = capabilities(&all_scopes());
        let options = McpOptions::parse(&["proxy".into()], false).unwrap();
        let degraded = json!({
            "status": "committed_degraded", "value": null, "commits": [],
            "notifications_pending": false,
            "degradations": [{"phase": "runtime_build", "message": "late"}]
        });
        let transport = FakeTransport::new(vec![
            Ok(degraded),
            Err(CliError::Unreachable {
                target: "t".into(),
                detail: "refused".into(),
            }),
        ]);
        let call = |id| {
            json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
                   "params": {"name": "proxy_select", "arguments": {"group": "G", "name": "N"}}})
        };

        let reply = rpc(&transport, &capabilities, &options, call(1)).await;
        assert_eq!(reply["result"]["isError"], false);
        assert!(
            reply["result"]["content"][1]["text"]
                .as_str()
                .unwrap()
                .contains("runtime_build: late")
        );

        let reply = rpc(&transport, &capabilities, &options, call(2)).await;
        assert_eq!(reply["result"]["isError"], true);
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("refused")
        );
    }

    #[tokio::test]
    async fn invalid_tool_arguments_are_tool_errors_without_a_call() {
        let capabilities = capabilities(&all_scopes());
        let options = McpOptions::parse(&["proxy".into()], false).unwrap();
        let transport = FakeTransport::new(vec![]);
        let reply = rpc(
            &transport,
            &capabilities,
            &options,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                   "params": {"name": "proxy_select", "arguments": {"group": "G"}}}),
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        assert!(transport.recorded().is_empty());
    }

    #[tokio::test]
    async fn the_connection_tool_takes_the_first_stream_frame() {
        let capabilities = capabilities(&all_scopes());
        let transport = FakeTransport::new(vec![]).with_feed(vec![
            Ok(json!({"connections": []})),
            Ok(json!({"connections": [1]})),
        ]);
        let reply = rpc(
            &transport,
            &capabilities,
            &McpOptions::default(),
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                   "params": {"name": "conn_list"}}),
        )
        .await;
        assert_eq!(
            reply["result"]["content"][0]["text"],
            r#"{"connections":[]}"#
        );
    }

    #[tokio::test]
    async fn serve_answers_lines_and_ignores_notifications() {
        let capabilities = capabilities(&all_scopes());
        let transport = FakeTransport::new(vec![]);
        let input = concat!(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#,
            "\n",
            "not json\n",
            r#"{"jsonrpc":"2.0","id":8,"method":"nope"}"#,
            "\n",
        );
        let mut output = Vec::new();
        serve(
            &transport,
            &capabilities,
            &McpOptions::default(),
            input.as_bytes(),
            &mut output,
        )
        .await
        .unwrap();

        let replies: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(replies.len(), 3, "the notification gets no reply");
        assert_eq!(replies[0]["id"], 7);
        assert_eq!(replies[1]["error"]["code"], PARSE_ERROR);
        assert_eq!(replies[2]["error"]["code"], METHOD_NOT_FOUND);
    }
}
