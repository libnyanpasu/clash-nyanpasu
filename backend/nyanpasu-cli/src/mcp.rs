//! `nyanpasu-cli mcp`: an MCP server over standard input and output.
//!
//! It speaks newline-delimited JSON-RPC 2.0 (`initialize`, `ping`, `tools/list`,
//! `tools/call`) and reuses the CLI's target resolution, contexts and credentials, so an
//! agent sees exactly what `nyanpasu-cli` would. Each process owns one connection: a
//! dedicated local socket, or HTTP with its own per-process client id, so concurrent MCP
//! servers never share log cursors or stream lifetimes.
//!
//! The server is read-only by default: it registers only the curated `read` tools.
//! `--scope` adds scopes, always intersected with the scopes the shell reports for this
//! caller, and `remote_admin` is never offered, neither as a curated tool nor through the
//! opt-in `call_method` tool, whatever the shell says the caller holds.

use std::{collections::BTreeSet, time::Duration};

use serde_json::{Value, json};
use tokio::io::{AsyncBufRead, AsyncBufReadExt as _, AsyncWrite, AsyncWriteExt as _};

use crate::{
    commands::{COMMANDS, CuratedCommand, Effect, Invocation},
    describe::{Availability, Capabilities, Scope},
    error::CliError,
    output::degradations,
    transport::{StreamKind, Transport},
};

/// The protocol revision this server answers with when the client's is unknown.
const PROTOCOL_VERSION: &str = "2025-06-18";
/// Protocol revisions this server can speak: its tools use nothing newer.
const SUPPORTED_VERSIONS: [&str; 3] = ["2024-11-05", "2025-03-26", "2025-06-18"];
/// How long `conn_list` waits for the connection stream's first frame. A shell whose core is
/// not running never sends one, and the sequential server must not stall on it forever.
const CONNECTION_DETAILS_WAIT: Duration = Duration::from_secs(10);
/// The name of the opt-in generic tool.
pub const RAW_TOOL: &str = "call_method";

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// What `mcp` was asked to offer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McpOptions {
    /// Scopes requested with `--scope`, beyond the default `read`.
    pub scopes: Vec<Scope>,
    /// Offer the generic `call_method` tool.
    pub raw: bool,
}

impl McpOptions {
    /// The scopes the server may offer: `read` plus the requested ones, intersected with
    /// what the caller holds, and never `remote_admin`.
    pub fn effective_scopes(&self, capabilities: &Capabilities) -> BTreeSet<Scope> {
        std::iter::once(Scope::Read)
            .chain(self.scopes.iter().copied())
            .filter(|scope| *scope != Scope::RemoteAdmin && capabilities.holds(*scope))
            .collect()
    }
}

/// Whether a curated operation is offered under `scopes`.
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

/// The curated tools this session offers.
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

/// The methods `call_method` may call: exposed, permitted, inside the session's scopes and
/// without a curated tool of their own.
fn raw_methods(scopes: &BTreeSet<Scope>, capabilities: &Capabilities) -> Vec<String> {
    let curated: BTreeSet<&str> = COMMANDS
        .iter()
        .filter_map(|command| command.method())
        .collect();
    let mut methods: Vec<String> = capabilities
        .commands()
        .iter()
        .filter(|command| {
            command.allowed
                && command.scope != Scope::RemoteAdmin
                && scopes.contains(&command.scope)
                && !curated.contains(command.name.as_str())
        })
        .map(|command| command.name.clone())
        .collect();
    methods.sort_unstable();
    methods
}

fn raw_tool_definition(scopes: &BTreeSet<Scope>, capabilities: &Capabilities) -> Value {
    let methods = raw_methods(scopes, capabilities);
    json!({
        "name": RAW_TOOL,
        "description": format!(
            "Call an RPC method that has no curated tool. Only methods the caller may use within \
             this session's scopes are accepted; parameters are the method's own JSON object. \
             Available: {}.",
            methods.join(", ")
        ),
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
        let scopes = options.effective_scopes(capabilities);
        tools.push(raw_tool_definition(&scopes, capabilities));
    }
    tools
}

/// Whether `call_method` may call `method` under the session's scopes.
///
/// # Errors
/// An unexposed method is `Unsupported`; one the principal may not call, outside the
/// session's scopes, or holding `remote_admin` is `Unauthenticated`.
fn raw_permitted(
    method: &str,
    scopes: &BTreeSet<Scope>,
    capabilities: &Capabilities,
) -> Result<(), CliError> {
    let scope = capabilities.require(method)?.scope;
    if scope != Scope::RemoteAdmin && scopes.contains(&scope) {
        Ok(())
    } else {
        Err(CliError::Unauthenticated {
            message: format!(
                "`{method}` needs the {scope} scope, which this MCP session does not offer"
            ),
        })
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

fn finish(outcome: Result<Value, CliError>) -> Value {
    match outcome {
        Ok(value) => json!({"content": result_content(&value), "isError": false}),
        Err(error) => {
            json!({"content": [{"type": "text", "text": error.to_string()}], "isError": true})
        }
    }
}

/// Runs one `tools/call`. Failures are tool errors; only an unknown tool is a protocol
/// error, reported through `Err`.
async fn call_tool<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
    options: &McpOptions,
    name: &str,
    arguments: &Value,
) -> Result<Value, String> {
    if name == RAW_TOOL && options.raw {
        let scopes = options.effective_scopes(capabilities);
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
                tokio::time::timeout(CONNECTION_DETAILS_WAIT, feed.next())
                    .await
                    .map_err(|_| {
                        CliError::failed("no connection details arrived; is the core running?")
                    })?
                    .unwrap_or_else(|| Err(CliError::failed("the connection stream ended")))
            }
        }
    }
    .await;
    Ok(finish(outcome))
}

fn response(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn failure(id: &Value, code: i64, message: impl std::fmt::Display) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message.to_string()}})
}

/// Handles one JSON-RPC message; `None` for a notification or a client response, which
/// get no reply.
pub async fn handle_message<T: Transport>(
    transport: &T,
    capabilities: &Capabilities,
    options: &McpOptions,
    message: &Value,
) -> Option<Value> {
    if !message.is_object() {
        return Some(failure(
            &Value::Null,
            INVALID_REQUEST,
            "expected one JSON-RPC request object",
        ));
    }
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // This server sends no requests, so a message without a method is either a stray
        // response, which is ignored, or a malformed request.
        if message.get("result").is_some() || message.get("error").is_some() {
            return None;
        }
        return message
            .get("id")
            .map(|id| failure(id, INVALID_REQUEST, "a request needs a method"));
    };
    let id = message.get("id")?;
    if id.is_null() {
        return Some(failure(
            id,
            INVALID_REQUEST,
            "a request id must not be null",
        ));
    }
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
                    "serverInfo": {"name": "nyanpasu-cli", "version": env!("NYANPASU_VERSION")},
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

/// Serves MCP until the input ends. Messages are handled one at a time.
///
/// # Errors
/// Reports a failed read or write on the streams.
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
    use nyanpasu_rpc_wire::{CommandEffect, DescribedCommand};

    use super::*;
    use crate::{describe::fixtures, transport::fakes::FakeTransport};

    /// A describe response that allows every curated method, holding `scopes`.
    fn capabilities(scopes: &[Scope]) -> Capabilities {
        let allowed: Vec<(&str, Scope)> = COMMANDS
            .iter()
            .filter_map(|command| command.method().map(|method| (method, command.scope)))
            .collect();
        fixtures::capabilities(scopes, &allowed)
    }

    fn names(options: &McpOptions, capabilities: &Capabilities) -> BTreeSet<&'static str> {
        visible_commands(options, capabilities)
            .into_iter()
            .map(|command| command.name)
            .collect()
    }

    fn all_scopes() -> Vec<Scope> {
        Scope::ALL.to_vec()
    }

    fn options(scopes: &[Scope], raw: bool) -> McpOptions {
        McpOptions {
            scopes: scopes.to_vec(),
            raw,
        }
    }

    fn calls(transport: &FakeTransport) -> Vec<(String, Value)> {
        transport.calls.lock().unwrap().clone()
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
        let options = options(&[Scope::Proxy, Scope::Profile], false);

        let full = names(&options, &capabilities(&all_scopes()));
        assert!(full.contains("proxy_select"));
        assert!(full.contains("profile_use"));
        assert!(!full.contains("core_restart"), "system was not requested");

        let read_only = names(&options, &capabilities(&[Scope::Read]));
        assert!(
            !read_only.contains("proxy_select"),
            "the principal lacks proxy"
        );
        assert!(read_only.contains("status"));

        let proxy_only = names(&options, &capabilities(&[Scope::Read, Scope::Proxy]));
        assert!(proxy_only.contains("proxy_select"));
        assert!(
            !proxy_only.contains("profile_use"),
            "requested but not held"
        );
    }

    #[test]
    fn remote_admin_is_never_offered_even_when_requested_and_held() {
        let mut capabilities = capabilities(&all_scopes());
        capabilities.response.commands.push(DescribedCommand {
            name: "set_remote_access".into(),
            effect: CommandEffect::Mutation,
            scope: Scope::RemoteAdmin,
            allowed: true,
        });
        let options = options(&Scope::ALL, true);
        let scopes = options.effective_scopes(&capabilities);
        assert!(!scopes.contains(&Scope::RemoteAdmin));
        assert!(matches!(
            raw_permitted("set_remote_access", &scopes, &capabilities),
            Err(CliError::Unauthenticated { .. })
        ));
        let raw = raw_tool_definition(&scopes, &capabilities);
        assert!(
            !raw["description"]
                .as_str()
                .unwrap()
                .contains("set_remote_access"),
            "the raw tool does not even advertise it"
        );
        assert!(
            tool_definitions(&options, &capabilities)
                .iter()
                .all(|tool| tool["name"] != "set_remote_access")
        );
    }

    #[test]
    fn a_method_the_shell_does_not_expose_is_not_offered() {
        let capabilities =
            fixtures::capabilities(&[Scope::Read], &[("get_core_status_summary", Scope::Read)]);
        assert_eq!(
            names(&McpOptions::default(), &capabilities),
            BTreeSet::from(["status", "conn_list"])
        );
    }

    #[test]
    fn tool_definitions_carry_schemas_and_annotations() {
        let capabilities = capabilities(&all_scopes());
        let tools = tool_definitions(
            &options(&[Scope::System, Scope::Proxy], false),
            &capabilities,
        );
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
            &[Scope::Read, Scope::System],
            &[
                ("get_core_status_summary", Scope::Read),
                ("get_app_config", Scope::Read),
                ("restart_sidecar", Scope::System),
            ],
        );
        let options = options(&[], true);
        let tools = tool_definitions(&options, &capabilities);
        let raw = tools
            .iter()
            .find(|tool| tool["name"] == RAW_TOOL)
            .expect("call_method is offered");
        let description = raw["description"].as_str().unwrap();
        assert!(description.contains("get_app_config"), "{description}");
        assert!(
            !description.contains("get_core_status_summary"),
            "curated methods have their own tools"
        );
        assert!(
            !description.contains("restart_sidecar"),
            "system was not requested"
        );

        let scopes = options.effective_scopes(&capabilities);
        assert!(raw_permitted("get_app_config", &scopes, &capabilities).is_ok());
        assert!(
            raw_permitted("restart_sidecar", &scopes, &capabilities).is_err(),
            "system was not requested"
        );
        assert!(matches!(
            raw_permitted("get_profiles", &scopes, &capabilities),
            Err(CliError::Unsupported { .. })
        ));
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
            json!({"jsonrpc": "2.0", "id": 2, "method": "initialize",
                   "params": {"protocolVersion": "1999-01-01"}}),
        )
        .await;
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSION);

        let reply = rpc(
            &transport,
            &capabilities,
            &options,
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
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
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                   "params": {"name": "status", "arguments": {}}}),
        )
        .await;
        assert_eq!(reply["result"]["isError"], false);
        assert_eq!(reply["result"]["content"][0]["text"], r#"{"running":true}"#);
        assert_eq!(
            calls(&transport),
            vec![("get_core_status_summary".to_owned(), json!({}))]
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
        assert!(calls(&transport).is_empty(), "nothing reached the shell");

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
    async fn the_raw_tool_never_reaches_a_remote_admin_method() {
        let mut capabilities = capabilities(&all_scopes());
        for name in ["set_remote_access", "create_remote_access_token"] {
            capabilities.response.commands.push(DescribedCommand {
                name: name.into(),
                effect: CommandEffect::Mutation,
                scope: Scope::RemoteAdmin,
                allowed: true,
            });
        }
        let options = options(&Scope::ALL, true);
        let transport = FakeTransport::new(vec![]);
        for method in ["set_remote_access", "create_remote_access_token"] {
            let reply = rpc(
                &transport,
                &capabilities,
                &options,
                json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                       "params": {"name": RAW_TOOL, "arguments": {"method": method}}}),
            )
            .await;
            assert_eq!(reply["result"]["isError"], true, "{method}");
        }
        assert!(calls(&transport).is_empty(), "nothing reached the shell");
    }

    #[tokio::test]
    async fn tool_failures_and_degradation_are_reported_in_the_result() {
        let capabilities = capabilities(&all_scopes());
        let options = options(&[Scope::Proxy], false);
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
        let options = options(&[Scope::Proxy], false);
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
        assert!(calls(&transport).is_empty());
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

    /// A shell whose connection stream opens and then never sends a frame.
    struct SilentStream {
        senders: std::sync::Mutex<Vec<tokio::sync::mpsc::Sender<Result<Value, CliError>>>>,
    }

    impl Transport for SilentStream {
        async fn call(&self, _method: &str, _params: Value) -> Result<Value, CliError> {
            Err(CliError::failed("no calls expected"))
        }

        async fn events(
            &self,
            _stream: StreamKind,
        ) -> Result<crate::transport::EventFeed, CliError> {
            let (sender, receiver) = tokio::sync::mpsc::channel(1);
            self.senders.lock().unwrap().push(sender);
            Ok(crate::transport::EventFeed::new(receiver, None))
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_connection_stream_is_a_tool_error_not_a_stall() {
        let capabilities = capabilities(&all_scopes());
        let transport = SilentStream {
            senders: Default::default(),
        };
        let reply = handle_message(
            &transport,
            &capabilities,
            &McpOptions::default(),
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": {"name": "conn_list"}}),
        )
        .await
        .expect("a request gets a reply");
        assert_eq!(reply["result"]["isError"], true);
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("is the core running"),
            "{reply}"
        );
    }

    #[tokio::test]
    async fn serve_answers_requests_and_ignores_notifications_and_responses() {
        let capabilities = capabilities(&all_scopes());
        let transport = FakeTransport::new(vec![]);
        let input = concat!(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":9,"result":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#,
            "\n",
            "\n",
            "not json\n",
            "[1]\n",
            r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
            "\n",
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
        assert_eq!(replies.len(), 5, "notifications and responses get no reply");
        assert_eq!(replies[0]["id"], 7);
        assert_eq!(replies[1]["error"]["code"], PARSE_ERROR);
        assert_eq!(replies[2]["error"]["code"], INVALID_REQUEST);
        assert_eq!(replies[3]["error"]["code"], INVALID_REQUEST, "a null id");
        assert_eq!(replies[4]["error"]["code"], METHOD_NOT_FOUND);
    }
}
