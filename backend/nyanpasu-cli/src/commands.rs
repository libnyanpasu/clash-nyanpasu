//! The curated static command table shared by the CLI subcommands and the MCP tools.
//!
//! CLI v0 ships before the typed protocol, so each curated operation is written
//! down once here: the RPC method, its scope, a description, a compact JSON input
//! schema and the MCP annotations. A CLI subcommand turns its arguments into an
//! input object matching the schema; [`CuratedCommand::params`] maps that object to
//! the method's real parameters. `call <method>` covers every other command.
//!
//! Accuracy of every entry against the real handlers is verified by the core-backed
//! integration tests described in the design (§8.4), not by this crate's unit tests.

use serde_json::{Value, json};

use crate::{describe::Scope, error::CliError};

/// How a curated operation affects state, for MCP annotations and confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Reads state only (`readOnlyHint`).
    Query,
    /// Changes state. `destructive` marks drops of connections or core stops
    /// (`destructiveHint`).
    Mutation { destructive: bool },
}

/// How the operation reaches the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invocation {
    /// One request/reply RPC method.
    Method(&'static str),
    /// The connection-details stream; a one-shot read takes its first frame.
    ConnectionDetails,
}

/// One curated operation.
pub struct CuratedCommand {
    /// Stable identity: the CLI path joined by `_`, and the MCP tool name.
    pub name: &'static str,
    /// How the operation reaches the shell.
    pub invocation: Invocation,
    /// The scope the principal must hold.
    pub scope: Scope,
    /// Query or mutation.
    pub effect: Effect,
    /// A handwritten description for `--help` and MCP `tools/list`.
    pub description: &'static str,
    /// The compact JSON Schema of the input object, as JSON text.
    pub input_schema: &'static str,
    build: fn(&Value) -> Result<Value, CliError>,
}

impl CuratedCommand {
    /// The RPC method behind this operation, when it is a plain call.
    pub fn method(&self) -> Option<&'static str> {
        match self.invocation {
            Invocation::Method(method) => Some(method),
            Invocation::ConnectionDetails => None,
        }
    }

    /// The parsed input schema.
    ///
    /// # Panics
    /// Panics if the table holds invalid JSON; a unit test covers every entry.
    pub fn input_schema_value(&self) -> Value {
        serde_json::from_str(self.input_schema).expect("curated schemas are valid JSON")
    }

    /// Maps the schema-shaped `input` to the method's parameter object.
    ///
    /// # Errors
    /// A missing or mistyped required field is a usage error.
    pub fn params(&self, input: &Value) -> Result<Value, CliError> {
        (self.build)(input)
    }
}

fn required_str<'a>(input: &'a Value, field: &str) -> Result<&'a str, CliError> {
    input
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| CliError::usage(format!("`{field}` is required and must be a string")))
}

fn required_bool(input: &Value, field: &str) -> Result<bool, CliError> {
    input
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| CliError::usage(format!("`{field}` is required and must be a boolean")))
}

fn optional_field(input: &Value, field: &str) -> Value {
    input.get(field).cloned().unwrap_or(Value::Null)
}

fn no_params(_: &Value) -> Result<Value, CliError> {
    Ok(json!({}))
}

fn select_proxy(input: &Value) -> Result<Value, CliError> {
    Ok(json!({
        "group": required_str(input, "group")?,
        "name": required_str(input, "name")?,
    }))
}

fn proxy_delay(input: &Value) -> Result<Value, CliError> {
    Ok(json!({
        "name": required_str(input, "name")?,
        "provider": optional_field(input, "provider"),
        "url": optional_field(input, "url"),
        "expected": optional_field(input, "expected"),
    }))
}

fn group_delay(input: &Value) -> Result<Value, CliError> {
    Ok(json!({
        "group": required_str(input, "group")?,
        "url": optional_field(input, "url"),
        "expected": optional_field(input, "expected"),
    }))
}

fn switch_core(input: &Value) -> Result<Value, CliError> {
    Ok(json!({ "clashCore": required_str(input, "core")? }))
}

fn set_mode(input: &Value) -> Result<Value, CliError> {
    let mode = required_str(input, "mode")?;
    if !matches!(mode, "rule" | "global" | "direct") {
        return Err(CliError::usage(
            "`mode` must be one of rule, global or direct",
        ));
    }
    Ok(json!({ "patch": { "mode": mode } }))
}

fn uid_only(input: &Value) -> Result<Value, CliError> {
    Ok(json!({ "uid": required_str(input, "uid")? }))
}

fn update_profile(input: &Value) -> Result<Value, CliError> {
    Ok(json!({ "uid": required_str(input, "uid")?, "option": Value::Null }))
}

fn import_profile(input: &Value) -> Result<Value, CliError> {
    Ok(json!({
        "url": required_str(input, "url")?,
        "name": optional_field(input, "name"),
    }))
}

fn set_system_proxy(input: &Value) -> Result<Value, CliError> {
    Ok(json!({ "patch": { "enable_system_proxy": required_bool(input, "enabled")? } }))
}

fn set_tun(input: &Value) -> Result<Value, CliError> {
    Ok(json!({ "patch": { "enable_tun_mode": required_bool(input, "enabled")? } }))
}

fn close_connections(input: &Value) -> Result<Value, CliError> {
    Ok(json!({ "id": optional_field(input, "id") }))
}

fn tail_logs(input: &Value) -> Result<Value, CliError> {
    let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(100);
    Ok(json!({
        "query": {
            "direction": "latest",
            "cursor": Value::Null,
            "level": optional_field(input, "level"),
            "keyword": input.get("keyword").and_then(Value::as_str).unwrap_or(""),
            "limit": limit,
        }
    }))
}

fn patch_config(input: &Value) -> Result<Value, CliError> {
    match input.get("patch") {
        Some(patch @ Value::Object(_)) => Ok(json!({ "patch": patch })),
        _ => Err(CliError::usage("`patch` is required and must be an object")),
    }
}

/// The curated operations, in `--help` order.
pub const COMMANDS: &[CuratedCommand] = &[
    CuratedCommand {
        name: "status",
        invocation: Invocation::Method("get_core_status"),
        scope: Scope::Read,
        effect: Effect::Query,
        description: "Show the Clash core status: running state, core name and version.",
        input_schema: r#"{"type":"object","properties":{}}"#,
        build: no_params,
    },
    CuratedCommand {
        name: "core_version",
        invocation: Invocation::Method("clash_api_get_version"),
        scope: Scope::Read,
        effect: Effect::Query,
        description: "Show the version reported by the running Clash core.",
        input_schema: r#"{"type":"object","properties":{}}"#,
        build: no_params,
    },
    CuratedCommand {
        name: "core_restart",
        invocation: Invocation::Method("restart_sidecar"),
        scope: Scope::System,
        effect: Effect::Mutation { destructive: true },
        description: "Restart the Clash core. Active connections are dropped.",
        input_schema: r#"{"type":"object","properties":{}}"#,
        build: no_params,
    },
    CuratedCommand {
        name: "core_switch",
        invocation: Invocation::Method("change_clash_core"),
        scope: Scope::System,
        effect: Effect::Mutation { destructive: true },
        description: "Switch to another Clash core and restart. Active connections are dropped.",
        input_schema: r#"{"type":"object","properties":{"core":{"type":"string","description":"Core name, such as mihomo."}},"required":["core"]}"#,
        build: switch_core,
    },
    CuratedCommand {
        name: "mode_get",
        invocation: Invocation::Method("clash_api_get_configs"),
        scope: Scope::Read,
        effect: Effect::Query,
        description: "Show the Clash configuration reported by the core, including the proxy mode.",
        input_schema: r#"{"type":"object","properties":{}}"#,
        build: no_params,
    },
    CuratedCommand {
        name: "mode_set",
        invocation: Invocation::Method("patch_runtime_overrides"),
        scope: Scope::Proxy,
        effect: Effect::Mutation { destructive: false },
        description: "Switch the proxy mode to rule, global or direct.",
        input_schema: r#"{"type":"object","properties":{"mode":{"type":"string","enum":["rule","global","direct"]}},"required":["mode"]}"#,
        build: set_mode,
    },
    CuratedCommand {
        name: "proxy_groups",
        invocation: Invocation::Method("get_proxies"),
        scope: Scope::Read,
        effect: Effect::Query,
        description: "List proxy groups with their members and current selections.",
        input_schema: r#"{"type":"object","properties":{}}"#,
        build: no_params,
    },
    CuratedCommand {
        name: "proxy_select",
        invocation: Invocation::Method("select_proxy"),
        scope: Scope::Proxy,
        effect: Effect::Mutation { destructive: false },
        description: "Select a proxy inside a proxy group.",
        input_schema: r#"{"type":"object","properties":{"group":{"type":"string"},"name":{"type":"string","description":"Proxy to select."}},"required":["group","name"]}"#,
        build: select_proxy,
    },
    CuratedCommand {
        name: "proxy_test",
        invocation: Invocation::Method("clash_api_get_proxy_delay"),
        scope: Scope::Proxy,
        effect: Effect::Mutation { destructive: false },
        description: "Run a latency test against one proxy.",
        input_schema: r#"{"type":"object","properties":{"name":{"type":"string"},"provider":{"type":"string"},"url":{"type":"string"},"expected":{"type":"string"}},"required":["name"]}"#,
        build: proxy_delay,
    },
    CuratedCommand {
        name: "proxy_test_group",
        invocation: Invocation::Method("clash_api_get_group_delay"),
        scope: Scope::Proxy,
        effect: Effect::Mutation { destructive: false },
        description: "Run a latency test against every proxy of a group.",
        input_schema: r#"{"type":"object","properties":{"group":{"type":"string"},"url":{"type":"string"},"expected":{"type":"string"}},"required":["group"]}"#,
        build: group_delay,
    },
    CuratedCommand {
        name: "profile_list",
        invocation: Invocation::Method("get_profiles"),
        scope: Scope::Read,
        effect: Effect::Query,
        description: "List profiles and which of them are active.",
        input_schema: r#"{"type":"object","properties":{}}"#,
        build: no_params,
    },
    CuratedCommand {
        name: "profile_use",
        invocation: Invocation::Method("activate_profile"),
        scope: Scope::Profile,
        effect: Effect::Mutation { destructive: false },
        description: "Activate a profile and rebuild the runtime configuration.",
        input_schema: r#"{"type":"object","properties":{"uid":{"type":"string","description":"Profile id."}},"required":["uid"]}"#,
        build: uid_only,
    },
    CuratedCommand {
        name: "profile_update",
        invocation: Invocation::Method("update_profile"),
        scope: Scope::Profile,
        effect: Effect::Mutation { destructive: false },
        description: "Refresh a remote profile from its subscription URL.",
        input_schema: r#"{"type":"object","properties":{"uid":{"type":"string","description":"Profile id."}},"required":["uid"]}"#,
        build: update_profile,
    },
    CuratedCommand {
        name: "profile_import",
        invocation: Invocation::Method("import_profile"),
        scope: Scope::Profile,
        effect: Effect::Mutation { destructive: false },
        description: "Import a remote profile from a subscription URL.",
        input_schema: r#"{"type":"object","properties":{"url":{"type":"string"},"name":{"type":"string"}},"required":["url"]}"#,
        build: import_profile,
    },
    CuratedCommand {
        name: "profile_show",
        invocation: Invocation::Method("read_profile_file"),
        scope: Scope::SensitiveRead,
        effect: Effect::Query,
        description: "Show a profile's file content. It may contain credentials.",
        input_schema: r#"{"type":"object","properties":{"uid":{"type":"string","description":"Profile id."}},"required":["uid"]}"#,
        build: uid_only,
    },
    CuratedCommand {
        name: "sysproxy_status",
        invocation: Invocation::Method("get_sys_proxy"),
        scope: Scope::Read,
        effect: Effect::Query,
        description: "Show the operating system proxy settings.",
        input_schema: r#"{"type":"object","properties":{}}"#,
        build: no_params,
    },
    CuratedCommand {
        name: "sysproxy_set",
        invocation: Invocation::Method("patch_app_config"),
        scope: Scope::System,
        effect: Effect::Mutation { destructive: false },
        description: "Turn the operating system proxy on or off.",
        input_schema: r#"{"type":"object","properties":{"enabled":{"type":"boolean"}},"required":["enabled"]}"#,
        build: set_system_proxy,
    },
    CuratedCommand {
        name: "tun_status",
        invocation: Invocation::Method("get_clash_config"),
        scope: Scope::Read,
        effect: Effect::Query,
        description: "Show the Clash settings, including whether TUN mode is enabled.",
        input_schema: r#"{"type":"object","properties":{}}"#,
        build: no_params,
    },
    CuratedCommand {
        name: "tun_set",
        invocation: Invocation::Method("patch_clash_config"),
        scope: Scope::System,
        effect: Effect::Mutation { destructive: false },
        description: "Turn TUN mode on or off.",
        input_schema: r#"{"type":"object","properties":{"enabled":{"type":"boolean"}},"required":["enabled"]}"#,
        build: set_tun,
    },
    CuratedCommand {
        name: "conn_list",
        invocation: Invocation::ConnectionDetails,
        scope: Scope::Read,
        effect: Effect::Query,
        description: "Show the current connections.",
        input_schema: r#"{"type":"object","properties":{}}"#,
        build: no_params,
    },
    CuratedCommand {
        name: "conn_close",
        invocation: Invocation::Method("clash_api_delete_connections"),
        scope: Scope::Proxy,
        effect: Effect::Mutation { destructive: true },
        description: "Close one connection by id, or every connection when no id is given.",
        input_schema: r#"{"type":"object","properties":{"id":{"type":"string","description":"Connection id; omit to close all."}}}"#,
        build: close_connections,
    },
    CuratedCommand {
        name: "logs_tail",
        invocation: Invocation::Method("query_core_logs"),
        scope: Scope::Read,
        effect: Effect::Query,
        description: "Show the latest core log rows.",
        input_schema: r#"{"type":"object","properties":{"level":{"type":"string"},"keyword":{"type":"string"},"limit":{"type":"integer","minimum":1}}}"#,
        build: tail_logs,
    },
    CuratedCommand {
        name: "traffic",
        invocation: Invocation::Method("get_traffic_summary"),
        scope: Scope::Read,
        effect: Effect::Query,
        description: "Show the traffic summary.",
        input_schema: r#"{"type":"object","properties":{}}"#,
        build: no_params,
    },
    CuratedCommand {
        name: "config_get",
        invocation: Invocation::Method("get_app_config"),
        scope: Scope::Read,
        effect: Effect::Query,
        description: "Show the application settings.",
        input_schema: r#"{"type":"object","properties":{}}"#,
        build: no_params,
    },
    CuratedCommand {
        name: "config_patch",
        invocation: Invocation::Method("patch_app_config"),
        scope: Scope::Settings,
        effect: Effect::Mutation { destructive: false },
        description: "Patch ordinary application settings with a JSON object.",
        input_schema: r#"{"type":"object","properties":{"patch":{"type":"object"}},"required":["patch"]}"#,
        build: patch_config,
    },
];

/// Looks up a curated operation by its stable name.
pub fn find(name: &str) -> Option<&'static CuratedCommand> {
    COMMANDS.iter().find(|command| command.name == name)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn names_are_unique_and_the_table_is_curated_size() {
        let names: HashSet<_> = COMMANDS.iter().map(|command| command.name).collect();
        assert_eq!(names.len(), COMMANDS.len(), "duplicate curated names");
        assert!((15..=30).contains(&COMMANDS.len()), "{}", COMMANDS.len());
    }

    #[test]
    fn every_entry_is_complete() {
        for command in COMMANDS {
            assert!(!command.name.is_empty());
            assert!(
                command.description.len() >= 10,
                "{} lacks a description",
                command.name
            );
            let schema = command.input_schema_value();
            assert_eq!(schema["type"], "object", "{}", command.name);
            assert!(schema["properties"].is_object(), "{}", command.name);
            if let Some(required) = schema.get("required") {
                for field in required.as_array().expect("required is an array") {
                    let field = field.as_str().expect("required names are strings");
                    assert!(
                        schema["properties"].get(field).is_some(),
                        "{} requires undeclared `{field}`",
                        command.name
                    );
                }
            }
        }
    }

    #[test]
    fn every_required_field_is_enforced_by_the_param_builder() {
        for command in COMMANDS {
            let schema = command.input_schema_value();
            let Some(required) = schema.get("required").and_then(Value::as_array) else {
                command
                    .params(&json!({}))
                    .unwrap_or_else(|error| panic!("{}: {error}", command.name));
                continue;
            };
            assert!(
                command.params(&json!({})).is_err(),
                "{} accepts empty input despite required fields",
                command.name
            );
            let sample: serde_json::Map<String, Value> = required
                .iter()
                .map(|field| {
                    let field = field.as_str().expect("required names are strings");
                    let sample = match schema["properties"][field]["type"].as_str() {
                        Some("boolean") => json!(true),
                        Some("object") => json!({"key": 1}),
                        Some("string") => match schema["properties"][field].get("enum") {
                            Some(values) => values[0].clone(),
                            None => json!("value"),
                        },
                        other => panic!("unexpected sample type {other:?}"),
                    };
                    (field.to_owned(), sample)
                })
                .collect();
            command
                .params(&Value::Object(sample))
                .unwrap_or_else(|error| panic!("{}: {error}", command.name));
        }
    }

    #[test]
    fn the_design_command_groups_are_covered() {
        for group in [
            "status",
            "core_",
            "mode_",
            "proxy_",
            "profile_",
            "sysproxy_",
            "tun_",
            "conn_",
            "logs_",
            "traffic",
            "config_",
        ] {
            assert!(
                COMMANDS
                    .iter()
                    .any(|command| command.name.starts_with(group)),
                "no curated command for `{group}`"
            );
        }
    }

    #[test]
    fn sensitive_and_destructive_entries_are_annotated() {
        assert_eq!(find("profile_show").unwrap().scope, Scope::SensitiveRead);
        assert!(matches!(
            find("core_restart").unwrap().effect,
            Effect::Mutation { destructive: true }
        ));
        assert!(matches!(
            find("conn_close").unwrap().effect,
            Effect::Mutation { destructive: true }
        ));
        assert_eq!(find("status").unwrap().effect, Effect::Query);
    }

    #[test]
    fn no_curated_command_requires_remote_admin() {
        assert!(
            COMMANDS
                .iter()
                .all(|command| command.scope != Scope::RemoteAdmin)
        );
    }

    #[test]
    fn queries_hold_read_scopes_and_mutations_do_not() {
        for command in COMMANDS {
            match command.effect {
                Effect::Query => assert!(
                    matches!(command.scope, Scope::Read | Scope::SensitiveRead),
                    "{}",
                    command.name
                ),
                Effect::Mutation { .. } => assert!(
                    !matches!(command.scope, Scope::Read | Scope::SensitiveRead),
                    "{}",
                    command.name
                ),
            }
        }
    }

    #[test]
    fn builders_produce_the_wire_parameter_shapes() {
        assert_eq!(
            find("proxy_select")
                .unwrap()
                .params(&json!({"group": "G", "name": "N"}))
                .unwrap(),
            json!({"group": "G", "name": "N"})
        );
        assert_eq!(
            find("sysproxy_set")
                .unwrap()
                .params(&json!({"enabled": false}))
                .unwrap(),
            json!({"patch": {"enable_system_proxy": false}})
        );
        assert_eq!(
            find("core_switch")
                .unwrap()
                .params(&json!({"core": "mihomo"}))
                .unwrap(),
            json!({"clashCore": "mihomo"})
        );
        assert!(
            find("mode_set")
                .unwrap()
                .params(&json!({"mode": "x"}))
                .is_err()
        );
    }
}
