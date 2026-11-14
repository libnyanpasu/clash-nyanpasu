//! Offline embedded canonical command JSON Schemas for CLI v0 and MCP consumers.
//!
//! Regenerate the committed schema catalog artifact with:
//! `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu export_rpc_command_schemas -- --ignored`

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use snafu::Snafu;

use crate::{CommandEffect, RpcTarget, Scope};

/// Format version for the committed command schema artifact.
pub const COMMAND_SCHEMA_ARTIFACT_VERSION: u32 = 1;

/// JSON Schema dialect used by the canonical artifact and resolved per-command schemas.
pub const JSON_SCHEMA_2020_12_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Concise fidelity notice describing what compiled structural v0 schemas guarantee and
/// what remains authoritative in the server decoder.
pub const DEFAULT_VALIDATION_PROFILE: &str = "Draft 2020-12 structural wire schemas generated from compiled Specta/Serde phase metadata; server decoders remain authoritative for missing #[serde(deny_unknown_fields)] flags, custom Deserialize implementations, JSON numeric lexemes, and domain/privilege checks.";

/// Canonical generated command schema catalog embedded as immutable data.
pub const EMBEDDED_COMMAND_SCHEMAS_JSON: &str = include_str!("../schemas/command-schemas.v1.json");

const DEFS_REF_PREFIX: &str = "#/$defs/";

fn default_validation_profile() -> String {
    DEFAULT_VALIDATION_PROFILE.to_owned()
}

/// Failures returned while loading or resolving canonical command JSON Schemas.
#[derive(Debug, Snafu)]
pub enum SchemaCatalogError {
    /// Parsing the JSON artifact failed.
    #[snafu(display("failed to parse canonical command schema artifact: {source}"))]
    DeserializeArtifact { source: serde_json::Error },

    /// Artifact version does not match the reader's supported schema format.
    #[snafu(display("unsupported command schema artifact version {version}"))]
    UnsupportedVersion { version: u32 },

    /// Requested command is absent from the exported HTTP/Local command catalog.
    #[snafu(display("command `{name}` has no exported JSON Schema"))]
    UnknownCommand { name: String },

    /// A schema `$ref` does not follow `#/$defs/<key>`.
    #[snafu(display("malformed JSON Schema $ref `{reference}`"))]
    MalformedReference { reference: String },

    /// A reachable `#/$defs/<key>` target is missing from `defs`.
    #[snafu(display("dangling JSON Schema reference `{def_key}` while resolving `{context}`"))]
    DanglingReference { context: String, def_key: String },

    /// A resolved root schema was expected to be a JSON Schema object.
    #[snafu(display("expected a JSON object root schema for `{context}`"))]
    NonObjectRootSchema { context: String },
}

/// Canonical command metadata plus input and success-output JSON Schema roots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSchemaEntry {
    /// Stable wire command name.
    pub name: String,
    /// Declared command effect (`query` or `mutation`) from [`crate::COMMAND_CATALOG`].
    pub effect: CommandEffect,
    /// Required authorization scope from [`crate::COMMAND_CATALOG`].
    pub scope: Scope,
    /// Declared transport targets from [`crate::COMMAND_CATALOG`].
    pub targets: Vec<RpcTarget>,
    /// Rust doc comment extracted from the Specta command declaration, if non-empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    /// Deprecation message extracted from Specta metadata, if deprecated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<String>,
    /// `false` for zero-parameter commands (`null | {}`); `true` for parameter objects.
    pub has_params: bool,
    /// Root JSON Schema for request parameters (Deserialization phase).
    pub input: Value,
    /// Root JSON Schema for the unwrapped success value (Serialization phase).
    pub output: Value,
}

/// Immutable canonical JSON Schema catalog for all HTTP and Local RPC commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSchemaArtifact {
    /// Artifact format version (`1`).
    pub version: u32,
    /// JSON Schema dialect URI (`https://json-schema.org/draft/2020-12/schema`).
    pub schema_dialect: String,
    /// Concise statement of schema fidelity bounds vs. server-authoritative rules.
    #[serde(default = "default_validation_profile")]
    pub validation_profile: String,
    /// Root JSON Schema for the canonical [`crate::RpcError`] failure envelope.
    pub error_envelope: Value,
    /// Shared named type definitions referenced via `#/$defs/<key>`.
    pub defs: BTreeMap<String, Value>,
    /// Commands exposed on `RpcTarget::Http` or `RpcTarget::Local`, keyed by command name.
    pub commands: BTreeMap<String, CommandSchemaEntry>,
}

impl CommandSchemaArtifact {
    /// Load and validate the immutable catalog embedded inside `nyanpasu-rpc-protocol`.
    pub fn from_embedded() -> Result<Self, SchemaCatalogError> {
        Self::parse_json(EMBEDDED_COMMAND_SCHEMAS_JSON)
    }

    /// Parse a serialized schema catalog JSON document and verify version consistency.
    pub fn parse_json(raw: &str) -> Result<Self, SchemaCatalogError> {
        let artifact: Self = serde_json::from_str(raw)
            .map_err(|source| SchemaCatalogError::DeserializeArtifact { source })?;
        if artifact.version != COMMAND_SCHEMA_ARTIFACT_VERSION {
            return Err(SchemaCatalogError::UnsupportedVersion {
                version: artifact.version,
            });
        }
        Ok(artifact)
    }

    /// Look up a single command entry by exact command name.
    #[must_use]
    pub fn command(&self, name: &str) -> Option<&CommandSchemaEntry> {
        self.commands.get(name)
    }

    /// Iterate over all exported command names in deterministic sorted order.
    pub fn command_names(&self) -> impl Iterator<Item = &str> {
        self.commands.keys().map(String::as_str)
    }

    /// Iterate over exported commands that admit the requested transport `target`.
    pub fn commands_for_target(
        &self,
        target: RpcTarget,
    ) -> impl Iterator<Item = &CommandSchemaEntry> {
        self.commands
            .values()
            .filter(move |entry| entry.targets.contains(&target))
    }

    /// Resolve a command's request input JSON Schema into a self-contained Draft 2020-12 document
    /// bundling only its transitive `#/$defs/...` closure.
    pub fn resolve_input_schema(&self, name: &str) -> Result<Value, SchemaCatalogError> {
        let entry = self
            .commands
            .get(name)
            .ok_or_else(|| SchemaCatalogError::UnknownCommand {
                name: name.to_owned(),
            })?;
        self.resolve_root_schema(&format!("{name}.input"), &entry.input)
    }

    /// Resolve a command's success output JSON Schema into a self-contained Draft 2020-12 document
    /// bundling only its transitive `#/$defs/...` closure.
    pub fn resolve_output_schema(&self, name: &str) -> Result<Value, SchemaCatalogError> {
        let entry = self
            .commands
            .get(name)
            .ok_or_else(|| SchemaCatalogError::UnknownCommand {
                name: name.to_owned(),
            })?;
        self.resolve_root_schema(&format!("{name}.output"), &entry.output)
    }

    /// Resolve the canonical [`crate::RpcError`] envelope JSON Schema into a self-contained
    /// Draft 2020-12 document.
    pub fn resolve_error_envelope_schema(&self) -> Result<Value, SchemaCatalogError> {
        self.resolve_root_schema("errorEnvelope", &self.error_envelope)
    }

    /// Bundle a root schema with `$schema` and only its reachable `$defs` subgraph.
    pub fn resolve_root_schema(
        &self,
        context: &str,
        root: &Value,
    ) -> Result<Value, SchemaCatalogError> {
        let Value::Object(root_obj) = root else {
            return Err(SchemaCatalogError::NonObjectRootSchema {
                context: context.to_owned(),
            });
        };

        let mut refs = Vec::new();
        collect_refs(root, &mut refs)?;

        let mut collected_defs = BTreeMap::new();
        let mut queue: VecDeque<String> = refs.into_iter().collect();
        let mut seen = BTreeSet::new();

        while let Some(def_key) = queue.pop_front() {
            if !seen.insert(def_key.clone()) {
                continue;
            }
            let Some(def_schema) = self.defs.get(&def_key) else {
                return Err(SchemaCatalogError::DanglingReference {
                    context: context.to_owned(),
                    def_key,
                });
            };
            let mut nested_refs = Vec::new();
            collect_refs(def_schema, &mut nested_refs)?;
            queue.extend(nested_refs);
            collected_defs.insert(def_key, def_schema.clone());
        }

        let mut resolved = Map::with_capacity(root_obj.len() + 3);
        resolved.insert(
            "$schema".to_owned(),
            Value::String(self.schema_dialect.clone()),
        );
        if !root_obj.contains_key("$comment") && !self.validation_profile.is_empty() {
            resolved.insert(
                "$comment".to_owned(),
                Value::String(self.validation_profile.clone()),
            );
        }
        if !collected_defs.is_empty() {
            let defs_map: Map<String, Value> = collected_defs.into_iter().collect();
            resolved.insert("$defs".to_owned(), Value::Object(defs_map));
        }
        for (key, value) in root_obj {
            if key == "$schema" || key == "$defs" {
                continue;
            }
            resolved.insert(key.clone(), value.clone());
        }
        Ok(Value::Object(resolved))
    }

    /// Verify every command input/output and `errorEnvelope` schema resolves without dangling
    /// or malformed `$ref` pointers.
    pub fn validate_all_references(&self) -> Result<(), SchemaCatalogError> {
        let _ = self.resolve_error_envelope_schema()?;
        for name in self.commands.keys() {
            let _ = self.resolve_input_schema(name)?;
            let _ = self.resolve_output_schema(name)?;
        }
        Ok(())
    }
}

const SINGLE_SUBSCHEMA_KEYWORDS: &[&str] = &[
    "not",
    "if",
    "then",
    "else",
    "propertyNames",
    "contains",
    "additionalProperties",
    "unevaluatedProperties",
    "items",
    "unevaluatedItems",
    "contentSchema",
];

const ARRAY_SUBSCHEMA_KEYWORDS: &[&str] = &["allOf", "anyOf", "oneOf", "prefixItems"];

const MAP_SUBSCHEMA_KEYWORDS: &[&str] = &[
    "properties",
    "patternProperties",
    "$defs",
    "definitions",
    "dependentSchemas",
];

fn collect_refs(schema: &Value, out: &mut Vec<String>) -> Result<(), SchemaCatalogError> {
    let Value::Object(map) = schema else {
        return Ok(());
    };

    if let Some(ref_val) = map.get("$ref") {
        let reference = ref_val
            .as_str()
            .ok_or_else(|| SchemaCatalogError::MalformedReference {
                reference: ref_val.to_string(),
            })?;
        let key = decode_defs_ref_key(reference)?;
        out.push(key);
    }

    for keyword in SINGLE_SUBSCHEMA_KEYWORDS {
        if let Some(child) = map.get(*keyword) {
            collect_refs(child, out)?;
        }
    }

    for keyword in ARRAY_SUBSCHEMA_KEYWORDS {
        if let Some(Value::Array(items)) = map.get(*keyword) {
            for item in items {
                collect_refs(item, out)?;
            }
        }
    }

    for keyword in MAP_SUBSCHEMA_KEYWORDS {
        if let Some(Value::Object(entries)) = map.get(*keyword) {
            for subschema in entries.values() {
                collect_refs(subschema, out)?;
            }
        }
    }

    Ok(())
}

fn decode_defs_ref_key(reference: &str) -> Result<String, SchemaCatalogError> {
    let raw_token = reference.strip_prefix(DEFS_REF_PREFIX).ok_or_else(|| {
        SchemaCatalogError::MalformedReference {
            reference: reference.to_owned(),
        }
    })?;
    if raw_token.is_empty() || raw_token.contains('/') {
        return Err(SchemaCatalogError::MalformedReference {
            reference: reference.to_owned(),
        });
    }

    let mut decoded = String::with_capacity(raw_token.len());
    let mut chars = raw_token.chars();
    while let Some(ch) = chars.next() {
        if ch == '~' {
            match chars.next() {
                Some('0') => decoded.push('~'),
                Some('1') => decoded.push('/'),
                _ => {
                    return Err(SchemaCatalogError::MalformedReference {
                        reference: reference.to_owned(),
                    });
                }
            }
        } else {
            decoded.push(ch);
        }
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{COMMAND_CATALOG, CommandEffect, RpcTarget};

    use super::*;

    #[test]
    fn embedded_artifact_matches_catalog_union_offline() {
        let artifact = CommandSchemaArtifact::from_embedded().expect("embedded artifact parses");
        assert_eq!(artifact.version, COMMAND_SCHEMA_ARTIFACT_VERSION);
        assert_eq!(artifact.schema_dialect, JSON_SCHEMA_2020_12_DIALECT);
        assert!(!artifact.validation_profile.is_empty());
        artifact
            .validate_all_references()
            .expect("all command and errorEnvelope $defs references must resolve");

        let expected: BTreeMap<&str, _> = COMMAND_CATALOG
            .iter()
            .filter(|cmd| {
                cmd.targets.contains(&RpcTarget::Http) || cmd.targets.contains(&RpcTarget::Local)
            })
            .map(|cmd| (cmd.name, cmd))
            .collect();

        let actual_names: BTreeSet<&str> = artifact.command_names().collect();
        let expected_names: BTreeSet<&str> = expected.keys().copied().collect();
        assert_eq!(actual_names, expected_names);

        for (name, catalog_cmd) in expected {
            let entry = artifact.command(name).expect("command entry exists");
            assert_eq!(entry.effect, catalog_cmd.effect);
            assert_eq!(entry.scope, catalog_cmd.scope);
            assert_eq!(entry.targets, catalog_cmd.targets);
        }

        // Desktop-only commands must stay excluded from HTTP/Local schemas.
        for desktop_only in COMMAND_CATALOG.iter().filter(|cmd| {
            !cmd.targets.contains(&RpcTarget::Http) && !cmd.targets.contains(&RpcTarget::Local)
        }) {
            assert!(
                artifact.command(desktop_only.name).is_none(),
                "desktop-only command `{}` must not be exported",
                desktop_only.name
            );
        }

        let http_names: BTreeSet<&str> = artifact
            .commands_for_target(RpcTarget::Http)
            .map(|cmd| cmd.name.as_str())
            .collect();
        let local_names: BTreeSet<&str> = artifact
            .commands_for_target(RpcTarget::Local)
            .map(|cmd| cmd.name.as_str())
            .collect();
        assert!(!http_names.is_empty());
        assert!(!local_names.is_empty());
        assert!(http_names.is_subset(&actual_names));
        assert!(local_names.is_subset(&actual_names));
        assert!(artifact.command("get_profiles").is_some());
        assert_eq!(
            artifact.command("get_profiles").unwrap().effect,
            CommandEffect::Query
        );
    }

    #[test]
    fn resolve_schemas_bundle_minimal_defs_and_comments() {
        let artifact = CommandSchemaArtifact::from_embedded().expect("embedded artifact parses");

        let input = artifact
            .resolve_input_schema("get_profiles")
            .expect("get_profiles input resolves");
        assert_eq!(
            input.get("$schema").and_then(Value::as_str),
            Some(JSON_SCHEMA_2020_12_DIALECT)
        );
        assert!(input.get("$comment").and_then(Value::as_str).is_some());
        assert!(
            input.get("$defs").is_none(),
            "zero-arg get_profiles input should not bundle unrelated $defs"
        );

        let output = artifact
            .resolve_output_schema("get_profiles")
            .expect("get_profiles output resolves");
        let output_defs = output
            .get("$defs")
            .and_then(Value::as_object)
            .expect("get_profiles output needs $defs");
        assert!(
            output_defs.len() < artifact.defs.len(),
            "per-command resolved schema must include only reachable $defs ({} < {})",
            output_defs.len(),
            artifact.defs.len()
        );

        let err_env = artifact
            .resolve_error_envelope_schema()
            .expect("errorEnvelope resolves");
        assert_eq!(
            err_env.get("$schema").and_then(Value::as_str),
            Some(JSON_SCHEMA_2020_12_DIALECT)
        );
        let err_key = decode_defs_ref_key(
            err_env
                .get("$ref")
                .and_then(Value::as_str)
                .expect("errorEnvelope resolves via $ref"),
        )
        .expect("errorEnvelope $ref decodes");
        let err_props = err_env["$defs"][&err_key]["properties"]
            .as_object()
            .expect("RpcError def exposes properties");
        assert!(err_props.contains_key("code") && err_props.contains_key("message"));
    }

    #[test]
    fn resolver_handles_rfc6901_and_non_keyword_dollar_ref_and_errors() {
        let artifact = CommandSchemaArtifact {
            version: COMMAND_SCHEMA_ARTIFACT_VERSION,
            schema_dialect: JSON_SCHEMA_2020_12_DIALECT.to_owned(),
            validation_profile: DEFAULT_VALIDATION_PROFILE.to_owned(),
            error_envelope: json!({ "type": "object" }),
            defs: BTreeMap::from([
                ("a/b~c".to_owned(), json!({ "type": "string" })),
                ("unused".to_owned(), json!({ "type": "integer" })),
            ]),
            commands: BTreeMap::from([(
                "probe".to_owned(),
                CommandSchemaEntry {
                    name: "probe".to_owned(),
                    effect: CommandEffect::Query,
                    scope: Scope::Read,
                    targets: vec![RpcTarget::Http],
                    docs: None,
                    deprecated: None,
                    has_params: true,
                    input: json!({
                        "type": "object",
                        "properties": {
                            "$ref": { "$ref": "#/$defs/a~1b~0c" },
                            "example_holder": {
                                "type": "object",
                                "const": { "$ref": "not-a-pointer" },
                                "default": { "$ref": "still-literal" },
                                "examples": [{ "$ref": "also-literal" }]
                            }
                        },
                        "required": ["$ref"]
                    }),
                    output: json!({ "type": "null" }),
                },
            )]),
        };

        let resolved = artifact
            .resolve_input_schema("probe")
            .expect("RFC 6901 ref and property literally named $ref must resolve cleanly");
        let defs = resolved["$defs"].as_object().expect("$defs object");
        assert!(defs.contains_key("a/b~c"));
        assert!(!defs.contains_key("unused"));

        assert!(matches!(
            artifact.resolve_input_schema("missing"),
            Err(SchemaCatalogError::UnknownCommand { .. })
        ));
        assert!(matches!(
            CommandSchemaArtifact::parse_json(
                &json!({
                    "version": 999,
                    "schemaDialect": JSON_SCHEMA_2020_12_DIALECT,
                    "errorEnvelope": {},
                    "defs": {},
                    "commands": {}
                })
                .to_string()
            ),
            Err(SchemaCatalogError::UnsupportedVersion { version: 999 })
        ));
        assert!(matches!(
            artifact.resolve_root_schema("dangling", &json!({ "$ref": "#/$defs/missing" })),
            Err(SchemaCatalogError::DanglingReference { .. })
        ));
        assert!(matches!(
            artifact.resolve_root_schema("bad_ref", &json!({ "$ref": "https://example.com/a" })),
            Err(SchemaCatalogError::MalformedReference { .. })
        ));
        assert!(matches!(
            artifact.resolve_root_schema("bool_root", &json!(true)),
            Err(SchemaCatalogError::NonObjectRootSchema { .. })
        ));
    }
}
