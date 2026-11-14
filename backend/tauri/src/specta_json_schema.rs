//! Canonical JSON Schema Draft 2020-12 exporter over the exact `build_specta_builder()`
//! type/command collection and `COMMAND_CATALOG` metadata.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    path::Path,
    sync::{Arc, Mutex},
};

use nyanpasu_rpc::registry::CommandInputSchema;
use nyanpasu_rpc_protocol::{
    COMMAND_CATALOG, CommandMetadata, RpcError, RpcTarget,
    schemas::{
        COMMAND_SCHEMA_ARTIFACT_VERSION, CommandSchemaArtifact, CommandSchemaEntry,
        DEFAULT_VALIDATION_PROFILE, JSON_SCHEMA_2020_12_DIALECT,
    },
};
use serde_json::{Map, Value, json};
use specta::{
    Format as _, Type as _, Types,
    datatype::{
        DataType, Enum, Fields, Function, Generic, NamedDataType, NamedFields, NamedReference,
        NamedReferenceType, OpaqueReference, Primitive, Reference, Struct, UnnamedFields,
    },
};
use specta_serde::{Phase, PhasesFormat};
use tauri_specta::BuilderConfiguration;

/// Exports all HTTP and Local command schemas plus shared definitions to JSON.
pub(crate) fn build_command_schema_artifact(
    cfg: &BuilderConfiguration,
    input_schemas: &std::collections::HashMap<&'static str, CommandInputSchema>,
) -> Result<CommandSchemaArtifact, String> {
    let mut base_types = cfg.types.clone();
    let error_envelope_dt = RpcError::definition(&mut base_types);

    let catalog_commands = collect_exposed_catalog_commands()?;
    let specta_functions: BTreeMap<&str, &Function> = cfg
        .commands
        .iter()
        .map(|func| (func.name(), func))
        .collect();

    let mut pending_inputs = BTreeMap::new();
    for cmd in &catalog_commands {
        let input_entry = input_schemas.get(cmd.name).ok_or_else(|| {
            format!(
                "missing CommandInputSchema for exposed command `{}`",
                cmd.name
            )
        })?;
        let input_dt = (input_entry.datatype)(&mut base_types);
        pending_inputs.insert(cmd.name, (input_entry.has_params, input_dt));
        if !specta_functions.contains_key(cmd.name) {
            return Err(format!(
                "exposed command `{}` is missing from build_specta_builder()",
                cmd.name
            ));
        }
    }

    // Apply canonical Serde PhasesFormat EXACTLY once to the full type graph.
    let phased_types = PhasesFormat
        .map_types(&base_types)
        .map_err(|err| format!("specta_serde::PhasesFormat failed: {err}"))?;

    let mut exporter = SchemaExporter::new(phased_types.as_ref())?;

    let error_envelope = exporter.export_root(
        "RpcError",
        &error_envelope_dt,
        Phase::Serialize,
        false,
        None,
    )?;

    let mut commands = BTreeMap::new();
    for cmd in catalog_commands {
        let func = specta_functions[cmd.name];
        let (has_params, input_dt) = &pending_inputs[cmd.name];

        let input_schema = if !*has_params {
            if !matches!(input_dt, DataType::Tuple(t) if t.elements.is_empty()) {
                return Err(format!(
                    "zero-param command `{}` registered non-unit input datatype",
                    cmd.name
                ));
            }
            zero_param_input_schema()
        } else {
            exporter.export_root(
                &format!("{}.input", cmd.name),
                input_dt,
                Phase::Deserialize,
                true,
                None,
            )?
        };

        let success_dt = extract_command_success_datatype(cmd.name, func, &phased_types)?;
        let output_schema = exporter.export_root(
            &format!("{}.output", cmd.name),
            &success_dt,
            Phase::Serialize,
            false,
            None,
        )?;

        let docs = trim_non_empty(func.docs.as_ref());
        let deprecated = func.deprecated.as_ref().map(format_deprecated_message);

        commands.insert(
            cmd.name.to_owned(),
            CommandSchemaEntry {
                name: cmd.name.to_owned(),
                effect: cmd.effect,
                scope: cmd.scope,
                targets: cmd.targets.to_vec(),
                docs,
                deprecated,
                has_params: *has_params,
                input: input_schema,
                output: output_schema,
            },
        );
    }

    let artifact = CommandSchemaArtifact {
        version: COMMAND_SCHEMA_ARTIFACT_VERSION,
        schema_dialect: JSON_SCHEMA_2020_12_DIALECT.to_owned(),
        validation_profile: DEFAULT_VALIDATION_PROFILE.to_owned(),
        error_envelope,
        defs: exporter.defs,
        commands,
    };

    artifact
        .validate_all_references()
        .map_err(|err| format!("generated command schemas failed self-validation: {err}"))?;

    Ok(artifact)
}

/// Write canonical JSON schemas deterministically (pretty-printed with trailing newline).
pub(crate) fn write_command_schema_artifact(
    artifact: &CommandSchemaArtifact,
    path: &Path,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    let rendered = serialize_command_schema_artifact(artifact)?;
    std::fs::write(path, rendered)
        .map_err(|err| format!("failed to write {}: {err}", path.display()))
}

pub(crate) fn serialize_command_schema_artifact(
    artifact: &CommandSchemaArtifact,
) -> Result<String, String> {
    let mut json = serde_json::to_string_pretty(artifact)
        .map_err(|err| format!("failed to serialize command schema artifact: {err}"))?;
    json.push('\n');
    Ok(json)
}

/// Export a standalone resolved Draft 2020-12 schema for a single `DataType` and `Phase`.
#[cfg(test)]
pub(crate) fn export_probe_schema(
    dt: &DataType,
    types: &Types,
    phase: Phase,
) -> Result<Value, String> {
    let phased_types = PhasesFormat
        .map_types(types)
        .map_err(|err| format!("specta_serde::PhasesFormat failed: {err}"))?;
    let phased_dt = PhasesFormat
        .map_type(phased_types.as_ref(), dt)
        .map_err(|err| format!("specta_serde::PhasesFormat map_type failed: {err}"))?;
    let mut exporter = SchemaExporter::new(phased_types.as_ref())?;
    let root =
        exporter.datatype_to_schema("probe", phased_dt.as_ref(), phase, &GenericScope::new())?;
    let artifact = CommandSchemaArtifact {
        version: COMMAND_SCHEMA_ARTIFACT_VERSION,
        schema_dialect: JSON_SCHEMA_2020_12_DIALECT.to_owned(),
        validation_profile: DEFAULT_VALIDATION_PROFILE.to_owned(),
        error_envelope: json!({ "type": "object" }),
        defs: exporter.defs,
        commands: BTreeMap::new(),
    };
    artifact
        .resolve_root_schema("probe", &root)
        .map_err(|err| err.to_string())
}

#[derive(Debug)]
struct SchemaLanguageError(String);

impl fmt::Display for SchemaLanguageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SchemaLanguageError {}

impl From<std::io::Error> for SchemaLanguageError {
    fn from(err: std::io::Error) -> Self {
        Self(err.to_string())
    }
}

struct CaptureCommandSchemasLanguage<'a> {
    input_schemas: &'a std::collections::HashMap<&'static str, CommandInputSchema>,
    out: Arc<Mutex<Option<CommandSchemaArtifact>>>,
}

impl tauri_specta::LanguageExt for CaptureCommandSchemasLanguage<'_> {
    type Error = SchemaLanguageError;

    fn export(self, cfg: &BuilderConfiguration, _path: &Path) -> Result<(), Self::Error> {
        let artifact =
            build_command_schema_artifact(cfg, self.input_schemas).map_err(SchemaLanguageError)?;
        let mut guard = self
            .out
            .lock()
            .map_err(|_| SchemaLanguageError("schema export mutex poisoned".to_owned()))?;
        *guard = Some(artifact);
        Ok(())
    }
}

/// Captures `BuilderConfiguration` from the canonical `tauri_specta::Builder`
/// and builds the deterministic `CommandSchemaArtifact` in memory.
pub(crate) fn export_builder_command_schemas(
    builder: &tauri_specta::Builder<tauri::Wry>,
    input_schemas: &std::collections::HashMap<&'static str, CommandInputSchema>,
) -> Result<CommandSchemaArtifact, String> {
    let out = Arc::new(Mutex::new(None));
    let lang = CaptureCommandSchemasLanguage {
        input_schemas,
        out: Arc::clone(&out),
    };
    builder
        .export(lang, Path::new("unused"))
        .map_err(|err| err.0)?;
    let guard = out
        .lock()
        .map_err(|_| "schema export mutex poisoned".to_owned())?;
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| "LanguageExt::export did not produce a schema artifact".to_owned())
}

fn collect_exposed_catalog_commands() -> Result<Vec<&'static CommandMetadata>, String> {
    let mut seen = BTreeSet::new();
    let mut commands = Vec::new();
    for cmd in COMMAND_CATALOG {
        if !cmd.targets.contains(&RpcTarget::Http) && !cmd.targets.contains(&RpcTarget::Local) {
            continue;
        }
        if !seen.insert(cmd.name) {
            return Err(format!(
                "duplicate active catalog entry for command `{}`",
                cmd.name
            ));
        }
        commands.push(cmd);
    }
    commands.sort_by_key(|cmd| cmd.name);
    Ok(commands)
}

fn extract_command_success_datatype(
    command_name: &str,
    func: &Function,
    types: &Types,
) -> Result<DataType, String> {
    let Some(result_dt) = func.result() else {
        return Ok(DataType::Tuple(specta::datatype::Tuple::new(Vec::new())));
    };
    if let Some(ok_dt) = unwrap_std_result_ok(result_dt, types) {
        Ok(ok_dt)
    } else {
        if let DataType::Reference(Reference::Named(reference)) = result_dt
            && let Some(ndt) = types.get(reference)
            && is_std_result_ndt(ndt)
        {
            return Err(format!(
                "command `{command_name}` returned std::result::Result without concrete Ok generic"
            ));
        }
        Ok(result_dt.clone())
    }
}

fn unwrap_std_result_ok(dt: &DataType, types: &Types) -> Option<DataType> {
    let DataType::Reference(Reference::Named(reference)) = dt else {
        return None;
    };
    let ndt = types.get(reference)?;
    if !is_std_result_ndt(ndt) {
        return None;
    }
    let NamedReferenceType::Reference { generics, .. } = &reference.inner else {
        return None;
    };
    generics.first().map(|(_, ok_dt)| ok_dt.clone())
}

fn is_std_result_ndt(ndt: &NamedDataType) -> bool {
    ndt.module_path == "std::result" && (ndt.name == "Result" || ndt.name == "BaseResult")
}

fn zero_param_input_schema() -> Value {
    json!({
        "anyOf": [
            { "type": "null" },
            { "type": "object", "additionalProperties": false }
        ]
    })
}

fn trim_non_empty(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn format_deprecated_message(deprecated: &specta::datatype::Deprecated) -> String {
    match (&deprecated.note, &deprecated.since) {
        (Some(note), Some(since)) => format!("since {since}: {note}"),
        (Some(note), None) => note.as_ref().to_owned(),
        (None, Some(since)) => format!("since {since}"),
        (None, None) => "deprecated".to_owned(),
    }
}

type GenericScope = BTreeMap<String, DataType>;
type GenericBindings = Vec<(String, DataType)>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DefOrigin {
    file: String,
    line: u32,
    column: u32,
}

impl DefOrigin {
    fn from_ndt(ndt: &NamedDataType) -> Self {
        Self {
            file: ndt.location.file().to_owned(),
            line: ndt.location.line(),
            column: ndt.location.column(),
        }
    }
}

struct SchemaExporter<'a> {
    types: &'a Types,
    any_opaque: OpaqueReference,
    defs: BTreeMap<String, Value>,
    def_origins: BTreeMap<String, DefOrigin>,
    in_progress: BTreeSet<String>,
}

impl<'a> SchemaExporter<'a> {
    fn new(types: &'a Types) -> Result<Self, String> {
        let any_dt = <specta_typescript::Any as specta::Type>::definition(&mut Types::default());
        let DataType::Reference(Reference::Opaque(any_opaque)) = any_dt else {
            return Err("specta_typescript::Any did not produce an OpaqueReference".to_owned());
        };
        Ok(Self {
            types,
            any_opaque,
            defs: BTreeMap::new(),
            def_origins: BTreeMap::new(),
            in_progress: BTreeSet::new(),
        })
    }

    fn export_root(
        &mut self,
        path: &str,
        dt: &DataType,
        phase: Phase,
        inline_synthetic_args: bool,
        scope: Option<&GenericScope>,
    ) -> Result<Value, String> {
        let empty_scope = GenericScope::new();
        let scope = scope.unwrap_or(&empty_scope);

        if inline_synthetic_args
            && let DataType::Reference(Reference::Named(reference)) = dt
            && let Some(ndt) = self.types.get(reference)
            && ndt.name.starts_with("__UnifiedArgs_")
        {
            let target_ndt = self.unwrap_split_wrapper_if_needed(ndt, phase);
            let inner_ty = target_ndt.ty.as_ref().ok_or_else(|| {
                format!("synthetic args `{}` has no definition at {path}", ndt.name)
            })?;
            return self.datatype_to_schema(path, inner_ty, phase, scope);
        }

        self.datatype_to_schema(path, dt, phase, scope)
    }

    fn datatype_to_schema(
        &mut self,
        path: &str,
        dt: &DataType,
        phase: Phase,
        scope: &GenericScope,
    ) -> Result<Value, String> {
        match dt {
            DataType::Primitive(primitive) => Ok(primitive_schema(primitive)),
            DataType::List(list) => {
                let items =
                    self.datatype_to_schema(&format!("{path}[]"), &list.ty, phase, scope)?;
                let mut map = Map::new();
                map.insert("type".to_owned(), Value::String("array".to_owned()));
                map.insert("items".to_owned(), items);
                if let Some(length) = list.length {
                    map.insert("minItems".to_owned(), json!(length));
                    map.insert("maxItems".to_owned(), json!(length));
                }
                Ok(Value::Object(map))
            }
            DataType::Tuple(tuple) => {
                if tuple.elements.is_empty() {
                    return Ok(json!({ "type": "null" }));
                }
                let mut prefix_items = Vec::with_capacity(tuple.elements.len());
                for (index, elem) in tuple.elements.iter().enumerate() {
                    prefix_items.push(self.datatype_to_schema(
                        &format!("{path}[{index}]"),
                        elem,
                        phase,
                        scope,
                    )?);
                }
                Ok(json!({
                    "type": "array",
                    "prefixItems": prefix_items,
                    "minItems": tuple.elements.len(),
                    "maxItems": tuple.elements.len(),
                    "items": false
                }))
            }
            DataType::Map(map) => {
                let value_schema = self.datatype_to_schema(
                    &format!("{path}.<value>"),
                    map.value_ty(),
                    phase,
                    scope,
                )?;
                let mut obj = Map::new();
                obj.insert("type".to_owned(), Value::String("object".to_owned()));
                obj.insert("additionalProperties".to_owned(), value_schema);
                if let Some(property_names) = self.map_key_property_names_schema(
                    &format!("{path}.<key>"),
                    map.key_ty(),
                    phase,
                    scope,
                )? {
                    obj.insert("propertyNames".to_owned(), property_names);
                }
                Ok(Value::Object(obj))
            }
            DataType::Nullable(inner) => {
                let inner_schema = self.datatype_to_schema(path, inner, phase, scope)?;
                Ok(make_nullable_schema(inner_schema))
            }
            DataType::Intersection(parts) => {
                if parts.is_empty() {
                    return Err(format!("empty DataType::Intersection at {path}"));
                }
                let mut schemas = Vec::with_capacity(parts.len());
                for (index, part) in parts.iter().enumerate() {
                    schemas.push(self.datatype_to_schema(
                        &format!("{path}&{index}"),
                        part,
                        phase,
                        scope,
                    )?);
                }
                if schemas.len() == 1 {
                    Ok(schemas.into_iter().next().unwrap_or(Value::Null))
                } else {
                    Ok(json!({ "allOf": schemas }))
                }
            }
            DataType::Struct(strct) => self.struct_to_schema(path, strct, phase, scope),
            DataType::Enum(enm) => self.enum_to_schema(path, enm, phase, scope),
            DataType::Generic(generic) => {
                let bound = scope.get(generic.name().as_ref()).ok_or_else(|| {
                    format!(
                        "unbound generic `{}` at {path} (phase {phase:?})",
                        generic.name()
                    )
                })?;
                self.datatype_to_schema(path, bound, phase, scope)
            }
            DataType::Reference(Reference::Opaque(opaque)) => {
                if *opaque == self.any_opaque {
                    Ok(json!({}))
                } else {
                    Err(format!(
                        "unsupported opaque Specta reference `{}` at {path} (phase {phase:?})",
                        opaque.type_name()
                    ))
                }
            }
            DataType::Reference(Reference::Named(reference)) => {
                self.named_reference_to_schema(path, reference, phase, scope)
            }
        }
    }

    fn named_reference_to_schema(
        &mut self,
        path: &str,
        reference: &NamedReference,
        phase: Phase,
        scope: &GenericScope,
    ) -> Result<Value, String> {
        match &reference.inner {
            NamedReferenceType::Inline { dt, .. } => {
                self.datatype_to_schema(path, dt, phase, scope)
            }
            NamedReferenceType::Recursive(_) => {
                let ndt = self
                    .types
                    .get(reference)
                    .ok_or_else(|| format!("dangling recursive NamedReference at {path}"))?;
                let concrete_from_scope: Vec<(String, DataType)> = ndt
                    .generics
                    .iter()
                    .filter_map(|def| {
                        scope
                            .get(def.name.as_ref())
                            .map(|resolved| (def.name.to_string(), resolved.clone()))
                    })
                    .collect();
                let (target_ndt, concrete_generics) =
                    self.resolve_split_and_generics(ndt, &concrete_from_scope, phase, scope)?;
                let def_key =
                    self.canonical_def_key(path, target_ndt, &concrete_generics, phase, scope)?;
                Ok(json!({ "$ref": def_ref_pointer(&def_key) }))
            }
            NamedReferenceType::Reference { generics, .. } => {
                let ndt = self
                    .types
                    .get(reference)
                    .ok_or_else(|| format!("dangling NamedReference at {path}"))?;

                let mut substituted_generics = Vec::with_capacity(generics.len());
                for (param, arg_dt) in generics {
                    substituted_generics.push((param.clone(), substitute_datatype(arg_dt, scope)?));
                }

                // If `ndt` is a `PhasesFormat` split wrapper enum `{ Serialize(inner), Deserialize(inner) }`,
                // unwrap the phase-selected branch while keeping `substituted_generics` bound!
                if let Some(wrapper_inner) = select_split_wrapper_inner(ndt, phase) {
                    let child_scope =
                        bind_generic_scope(path, ndt, &substituted_generics, self.types, phase)?;
                    return self.datatype_to_schema(path, &wrapper_inner, phase, &child_scope);
                }

                let child_scope =
                    bind_generic_scope(path, ndt, &substituted_generics, self.types, phase)?;
                let concrete_ordered: Vec<(String, DataType)> = ndt
                    .generics
                    .iter()
                    .filter_map(|def| {
                        child_scope
                            .get(def.name.as_ref())
                            .map(|resolved| (def.name.to_string(), resolved.clone()))
                    })
                    .collect();

                let def_key =
                    self.canonical_def_key(path, ndt, &concrete_ordered, phase, &child_scope)?;
                let origin = DefOrigin::from_ndt(ndt);
                if let Some(existing_origin) = self.def_origins.get(&def_key) {
                    if existing_origin != &origin {
                        return Err(format!(
                            "JSON Schema $defs key collision `{def_key}` between {}:{} and {}:{}",
                            existing_origin.file, existing_origin.line, origin.file, origin.line
                        ));
                    }
                } else {
                    self.def_origins.insert(def_key.clone(), origin);
                }

                if !self.defs.contains_key(&def_key) && !self.in_progress.contains(&def_key) {
                    let inner_ty = ndt.ty.as_ref().ok_or_else(|| {
                        format!(
                            "referenced NamedDataType `{}::{}` has `ty: None` at {path}",
                            ndt.module_path, ndt.name
                        )
                    })?;
                    self.in_progress.insert(def_key.clone());
                    let def_path = format!("{path}->{def_key}");
                    let resolved_schema =
                        self.datatype_to_schema(&def_path, inner_ty, phase, &child_scope);
                    self.in_progress.remove(&def_key);

                    let mut schema_val = resolved_schema?;
                    attach_named_metadata(&mut schema_val, ndt);
                    self.defs.insert(def_key.clone(), schema_val);
                }

                Ok(json!({ "$ref": def_ref_pointer(&def_key) }))
            }
        }
    }

    fn resolve_split_and_generics<'b>(
        &self,
        ndt: &'b NamedDataType,
        concrete: &[(String, DataType)],
        phase: Phase,
        scope: &GenericScope,
    ) -> Result<(&'b NamedDataType, GenericBindings), String>
    where
        'a: 'b,
    {
        if let Some(wrapper_inner) = select_split_wrapper_inner(ndt, phase)
            && let DataType::Reference(Reference::Named(inner_ref)) = &wrapper_inner
            && let Some(target_ndt) = self.types.get(inner_ref)
        {
            let mut substituted = Vec::new();
            if let NamedReferenceType::Reference { generics, .. } = &inner_ref.inner {
                let child = bind_generic_scope_from_pairs(ndt, concrete, scope)?;
                for (param, arg_dt) in generics {
                    substituted.push((
                        param.name().to_string(),
                        substitute_datatype(arg_dt, &child)?,
                    ));
                }
            }
            return Ok((target_ndt, substituted));
        }
        Ok((ndt, concrete.to_vec()))
    }

    fn unwrap_split_wrapper_if_needed<'b>(
        &self,
        ndt: &'b NamedDataType,
        phase: Phase,
    ) -> &'b NamedDataType
    where
        'a: 'b,
    {
        if let Some(DataType::Reference(Reference::Named(inner_ref))) =
            select_split_wrapper_inner(ndt, phase)
            && let Some(target) = self.types.get(&inner_ref)
        {
            return target;
        }
        ndt
    }

    fn canonical_def_key(
        &self,
        path: &str,
        ndt: &NamedDataType,
        concrete_generics: &[(String, DataType)],
        phase: Phase,
        scope: &GenericScope,
    ) -> Result<String, String> {
        let mut key = if ndt.module_path.is_empty() {
            format!("root.t.{}", encode_def_atom(ndt.name.as_ref()))
        } else {
            let mods = ndt
                .module_path
                .split("::")
                .map(encode_def_atom)
                .collect::<Vec<_>>()
                .join(".");
            format!("m.{mods}.t.{}", encode_def_atom(ndt.name.as_ref()))
        };

        if !concrete_generics.is_empty() {
            let mut arg_keys = Vec::with_capacity(concrete_generics.len());
            for (_, arg_dt) in concrete_generics {
                arg_keys.push(self.datatype_key_fragment(path, arg_dt, phase, scope)?);
            }
            key.push_str(".L.");
            key.push_str(&arg_keys.join(".C."));
            key.push_str(".R");
        }

        key.push_str(match phase {
            Phase::Serialize => ".ser",
            Phase::Deserialize => ".de",
        });
        Ok(key)
    }

    fn datatype_key_fragment(
        &self,
        path: &str,
        dt: &DataType,
        phase: Phase,
        scope: &GenericScope,
    ) -> Result<String, String> {
        match dt {
            DataType::Primitive(p) => Ok(format!("p_{p:?}")),
            DataType::Tuple(t) if t.elements.is_empty() => Ok("unit".to_owned()),
            DataType::Tuple(t) => {
                let mut parts = Vec::with_capacity(t.elements.len());
                for elem in &t.elements {
                    parts.push(self.datatype_key_fragment(path, elem, phase, scope)?);
                }
                Ok(format!("tuple.L.{}.R", parts.join(".C.")))
            }
            DataType::List(list) => {
                let elem = self.datatype_key_fragment(path, &list.ty, phase, scope)?;
                Ok(match list.length {
                    Some(len) => format!("array_{len}.L.{elem}.R"),
                    None => format!("list.L.{elem}.R"),
                })
            }
            DataType::Map(map) => {
                let k = self.datatype_key_fragment(path, map.key_ty(), phase, scope)?;
                let v = self.datatype_key_fragment(path, map.value_ty(), phase, scope)?;
                Ok(format!("map.L.{k}.C.{v}.R"))
            }
            DataType::Nullable(inner) => Ok(format!(
                "option.L.{}.R",
                self.datatype_key_fragment(path, inner, phase, scope)?
            )),
            DataType::Reference(Reference::Opaque(opaque)) if *opaque == self.any_opaque => {
                Ok("any".to_owned())
            }
            DataType::Reference(Reference::Named(reference)) => {
                let ndt = self
                    .types
                    .get(reference)
                    .ok_or_else(|| format!("dangling NamedReference in generic arg at {path}"))?;
                let args = match &reference.inner {
                    NamedReferenceType::Reference { generics, .. } => generics
                        .iter()
                        .map(|(g, d)| Ok((g.name().to_string(), substitute_datatype(d, scope)?)))
                        .collect::<Result<Vec<_>, String>>()?,
                    NamedReferenceType::Inline { dt, .. } => {
                        return self.datatype_key_fragment(path, dt, phase, scope);
                    }
                    NamedReferenceType::Recursive(_) => ndt
                        .generics
                        .iter()
                        .filter_map(|g| {
                            scope
                                .get(g.name.as_ref())
                                .map(|resolved| (g.name.to_string(), resolved.clone()))
                        })
                        .collect(),
                };
                let (resolved_ndt, resolved_args) =
                    self.resolve_split_and_generics(ndt, &args, phase, scope)?;
                self.canonical_def_key(path, resolved_ndt, &resolved_args, phase, scope)
            }
            DataType::Generic(g) => Err(format!(
                "unbound generic `{}` inside generic type argument at {path}",
                g.name()
            )),
            other => Err(format!(
                "unsupported datatype in generic key at {path}: {other:?}"
            )),
        }
    }

    fn struct_to_schema(
        &mut self,
        path: &str,
        strct: &Struct,
        phase: Phase,
        scope: &GenericScope,
    ) -> Result<Value, String> {
        match &strct.fields {
            Fields::Unit => Ok(json!({ "type": "null" })),
            Fields::Unnamed(unnamed) => self.unnamed_fields_to_schema(path, unnamed, phase, scope),
            Fields::Named(named) => self.named_fields_to_schema(path, named, phase, scope),
        }
    }

    fn unnamed_fields_to_schema(
        &mut self,
        path: &str,
        unnamed: &UnnamedFields,
        phase: Phase,
        scope: &GenericScope,
    ) -> Result<Value, String> {
        if unnamed.fields.is_empty() {
            return Ok(json!({
                "type": "array",
                "minItems": 0,
                "maxItems": 0
            }));
        }

        let live: Vec<&DataType> = unnamed
            .fields
            .iter()
            .filter_map(|field| field.ty.as_ref())
            .collect();

        if live.is_empty() {
            return Ok(json!({ "type": "null" }));
        }

        if live.len() == 1 && unnamed.fields.len() == 1 {
            return self.datatype_to_schema(path, live[0], phase, scope);
        }

        let mut prefix_items = Vec::with_capacity(live.len());
        for (idx, dt) in live.iter().enumerate() {
            prefix_items.push(self.datatype_to_schema(
                &format!("{path}[{idx}]"),
                dt,
                phase,
                scope,
            )?);
        }

        Ok(json!({
            "type": "array",
            "prefixItems": prefix_items,
            "minItems": live.len(),
            "maxItems": live.len(),
            "items": false
        }))
    }

    fn named_fields_to_schema(
        &mut self,
        path: &str,
        named: &NamedFields,
        phase: Phase,
        scope: &GenericScope,
    ) -> Result<Value, String> {
        let mut properties = Map::new();
        let mut required = Vec::new();

        for (field_name, field) in &named.fields {
            let Some(field_dt) = &field.ty else {
                continue;
            };
            let field_path = format!("{path}.{field_name}");
            let mut prop_schema = self.datatype_to_schema(&field_path, field_dt, phase, scope)?;
            if let Some(docs) = trim_non_empty(field.docs.as_ref()) {
                attach_description(&mut prop_schema, docs);
            }
            if let Some(deprecated) = &field.deprecated {
                attach_deprecated(&mut prop_schema, deprecated);
            }

            let is_omissible = field.optional
                || (phase == Phase::Deserialize && matches!(field_dt, DataType::Nullable(_)));
            if !is_omissible {
                required.push(Value::String(field_name.to_string()));
            }
            properties.insert(field_name.to_string(), prop_schema);
        }

        let mut obj = Map::new();
        obj.insert("type".to_owned(), Value::String("object".to_owned()));
        obj.insert("properties".to_owned(), Value::Object(properties));
        if !required.is_empty() {
            obj.insert("required".to_owned(), Value::Array(required));
        }
        Ok(Value::Object(obj))
    }

    fn enum_to_schema(
        &mut self,
        path: &str,
        enm: &Enum,
        phase: Phase,
        scope: &GenericScope,
    ) -> Result<Value, String> {
        let is_untagged = enm
            .attributes
            .get_named_as::<bool>("serde:container:untagged")
            == Some(&true);

        let live_variants: Vec<_> = enm
            .variants
            .iter()
            .filter(|(_, variant)| !variant.skip)
            .collect();

        if live_variants.is_empty() {
            return Ok(json!({ "not": {} }));
        }

        if !is_untagged
            && live_variants
                .iter()
                .all(|(_, variant)| matches!(variant.fields, Fields::Unit))
        {
            let mut literals = Vec::new();
            for (name, _) in live_variants {
                let lit = Value::String(name.to_string());
                if !literals.contains(&lit) {
                    literals.push(lit);
                }
            }
            return Ok(if literals.len() == 1 {
                json!({ "type": "string", "const": literals[0] })
            } else {
                json!({ "type": "string", "enum": literals })
            });
        }

        let mut branches = Vec::new();
        for (variant_name, variant) in live_variants {
            let branch = match &variant.fields {
                Fields::Unit => {
                    if is_untagged {
                        json!({ "type": "null" })
                    } else {
                        json!({ "type": "string", "const": variant_name })
                    }
                }
                Fields::Unnamed(unnamed) => self.unnamed_fields_to_schema(
                    &format!("{path}::{variant_name}"),
                    unnamed,
                    phase,
                    scope,
                )?,
                Fields::Named(named) => self.named_fields_to_schema(
                    &format!("{path}::{variant_name}"),
                    named,
                    phase,
                    scope,
                )?,
            };
            push_unique_any_of(&mut branches, branch);
        }

        if branches.len() == 1 {
            Ok(branches.into_iter().next().unwrap_or(Value::Null))
        } else {
            Ok(json!({ "anyOf": branches }))
        }
    }

    fn map_key_property_names_schema(
        &mut self,
        path: &str,
        key_dt: &DataType,
        phase: Phase,
        scope: &GenericScope,
    ) -> Result<Option<Value>, String> {
        match key_dt {
            DataType::Primitive(Primitive::str) => Ok(None),
            DataType::Primitive(Primitive::char) => Ok(Some(
                json!({ "type": "string", "minLength": 1, "maxLength": 1 }),
            )),
            DataType::Primitive(Primitive::i8) => Ok(Some(integer_key_pattern_schema(
                i8::MIN as i128,
                i8::MAX as i128,
            ))),
            DataType::Primitive(Primitive::i16) => Ok(Some(integer_key_pattern_schema(
                i16::MIN as i128,
                i16::MAX as i128,
            ))),
            DataType::Primitive(Primitive::i32) => Ok(Some(integer_key_pattern_schema(
                i32::MIN as i128,
                i32::MAX as i128,
            ))),
            DataType::Primitive(Primitive::i64) => Ok(Some(integer_key_pattern_schema(
                i64::MIN as i128,
                i64::MAX as i128,
            ))),
            DataType::Primitive(Primitive::u8) => {
                Ok(Some(integer_key_pattern_schema(0, u8::MAX as i128)))
            }
            DataType::Primitive(Primitive::u16) => {
                Ok(Some(integer_key_pattern_schema(0, u16::MAX as i128)))
            }
            DataType::Primitive(Primitive::u32) => {
                Ok(Some(integer_key_pattern_schema(0, u32::MAX as i128)))
            }
            DataType::Primitive(Primitive::u64) => {
                Ok(Some(integer_key_pattern_schema(0, u64::MAX as i128)))
            }
            DataType::Primitive(Primitive::bool) => Ok(Some(json!({ "enum": ["true", "false"] }))),
            DataType::Primitive(other) => Err(format!(
                "unsupported non-string map key primitive `{other:?}` at {path}"
            )),
            DataType::Generic(g) => {
                let bound = scope
                    .get(g.name().as_ref())
                    .ok_or_else(|| format!("unbound generic map key `{}` at {path}", g.name()))?;
                self.map_key_property_names_schema(path, bound, phase, scope)
            }
            DataType::Enum(enm) => {
                let schema = self.enum_to_schema(path, enm, phase, scope)?;
                ensure_string_key_enum_schema(path, &schema)?;
                Ok(Some(schema))
            }
            DataType::Reference(Reference::Named(reference)) => {
                if let NamedReferenceType::Inline { dt, .. } = &reference.inner {
                    return self.map_key_property_names_schema(path, dt, phase, scope);
                }
                let ndt = self
                    .types
                    .get(reference)
                    .ok_or_else(|| format!("dangling map key NamedReference at {path}"))?;
                if let Some(wrapper_inner) = select_split_wrapper_inner(ndt, phase) {
                    let generics = reference_generics(reference, scope)?;
                    let child_scope = bind_generic_scope(path, ndt, &generics, self.types, phase)?;
                    return self.map_key_property_names_schema(
                        path,
                        &wrapper_inner,
                        phase,
                        &child_scope,
                    );
                }
                let ty = ndt.ty.as_ref().ok_or_else(|| {
                    format!(
                        "map key NamedDataType `{}::{}` has `ty: None` at {path}",
                        ndt.module_path, ndt.name
                    )
                })?;
                let generics = reference_generics(reference, scope)?;
                let child_scope = bind_generic_scope(path, ndt, &generics, self.types, phase)?;
                match ty {
                    DataType::Struct(strct) => match &strct.fields {
                        Fields::Unnamed(unnamed) if unnamed.fields.len() == 1 => {
                            let inner = unnamed.fields[0].ty.as_ref().ok_or_else(|| {
                                format!("skipped newtype map key `{}` at {path}", ndt.name)
                            })?;
                            self.map_key_property_names_schema(path, inner, phase, &child_scope)
                        }
                        _ => Err(format!(
                            "unsupported struct map key `{}` (expected single-field newtype) at {path}",
                            ndt.name
                        )),
                    },
                    DataType::Enum(_) => {
                        let ref_schema =
                            self.named_reference_to_schema(path, reference, phase, scope)?;
                        let resolved_key = if let Some(ref_str) =
                            ref_schema.get("$ref").and_then(Value::as_str)
                            && let Some(raw_key) = ref_str.strip_prefix("#/$defs/")
                        {
                            let key = raw_key.replace("~1", "/").replace("~0", "~");
                            self.defs.get(&key).ok_or_else(|| {
                                format!("missing emitted map key enum `{key}` at {path}")
                            })?
                        } else {
                            &ref_schema
                        };
                        ensure_string_key_enum_schema(path, resolved_key)?;
                        Ok(Some(ref_schema))
                    }
                    DataType::Primitive(_) => {
                        self.map_key_property_names_schema(path, ty, phase, &child_scope)
                    }
                    other => Err(format!(
                        "unsupported named map key `{}` ({other:?}) at {path}",
                        ndt.name
                    )),
                }
            }
            other => Err(format!("unsupported map key datatype at {path}: {other:?}")),
        }
    }
}

fn reference_generics(
    reference: &NamedReference,
    scope: &GenericScope,
) -> Result<Vec<(Generic, DataType)>, String> {
    match &reference.inner {
        NamedReferenceType::Reference { generics, .. } => generics
            .iter()
            .map(|(g, dt)| Ok((g.clone(), substitute_datatype(dt, scope)?)))
            .collect(),
        _ => Ok(Vec::new()),
    }
}

fn select_split_wrapper_inner(ndt: &NamedDataType, phase: Phase) -> Option<DataType> {
    let DataType::Enum(wrapper) = ndt.ty.as_ref()? else {
        return None;
    };
    if wrapper.variants.len() != 2 {
        return None;
    }
    let has_serialize = wrapper.variants.iter().any(|(name, _)| name == "Serialize");
    let has_deserialize = wrapper
        .variants
        .iter()
        .any(|(name, _)| name == "Deserialize");
    if !has_serialize || !has_deserialize {
        return None;
    }
    let wanted = match phase {
        Phase::Serialize => "Serialize",
        Phase::Deserialize => "Deserialize",
    };
    let (_, variant) = wrapper.variants.iter().find(|(name, _)| name == wanted)?;
    let Fields::Unnamed(unnamed) = &variant.fields else {
        return None;
    };
    let [field] = unnamed.fields.as_slice() else {
        return None;
    };
    field.ty.clone()
}

fn bind_generic_scope(
    path: &str,
    ndt: &NamedDataType,
    provided: &[(Generic, DataType)],
    _types: &Types,
    _phase: Phase,
) -> Result<GenericScope, String> {
    let mut scope = GenericScope::new();
    for (param, dt) in provided {
        scope.insert(param.name().to_string(), dt.clone());
    }
    for def in ndt.generics.iter() {
        if !scope.contains_key(def.name.as_ref())
            && let Some(default_dt) = &def.default
        {
            let resolved = substitute_datatype(default_dt, &scope).map_err(|err| {
                format!(
                    "failed to resolve generic default `{}` on `{}::{}` at {path}: {err}",
                    def.name, ndt.module_path, ndt.name
                )
            })?;
            scope.insert(def.name.to_string(), resolved);
        }
    }
    Ok(scope)
}

fn bind_generic_scope_from_pairs(
    ndt: &NamedDataType,
    provided: &[(String, DataType)],
    outer: &GenericScope,
) -> Result<GenericScope, String> {
    let mut scope = GenericScope::new();
    for (name, dt) in provided {
        scope.insert(name.clone(), substitute_datatype(dt, outer)?);
    }
    for def in ndt.generics.iter() {
        if !scope.contains_key(def.name.as_ref())
            && let Some(default_dt) = &def.default
        {
            scope.insert(
                def.name.to_string(),
                substitute_datatype(default_dt, &scope)?,
            );
        }
    }
    Ok(scope)
}

fn substitute_datatype(dt: &DataType, scope: &GenericScope) -> Result<DataType, String> {
    match dt {
        DataType::Generic(g) => Ok(scope
            .get(g.name().as_ref())
            .cloned()
            .unwrap_or_else(|| DataType::Generic(g.clone()))),
        DataType::List(list) => {
            let mut cloned = list.clone();
            cloned.ty = Box::new(substitute_datatype(&list.ty, scope)?);
            Ok(DataType::List(cloned))
        }
        DataType::Map(map) => {
            let mut cloned = map.clone();
            *cloned.key_ty_mut() = substitute_datatype(map.key_ty(), scope)?;
            *cloned.value_ty_mut() = substitute_datatype(map.value_ty(), scope)?;
            Ok(DataType::Map(cloned))
        }
        DataType::Nullable(inner) => Ok(DataType::Nullable(Box::new(substitute_datatype(
            inner, scope,
        )?))),
        DataType::Tuple(tuple) => {
            let mut elems = Vec::with_capacity(tuple.elements.len());
            for elem in &tuple.elements {
                elems.push(substitute_datatype(elem, scope)?);
            }
            Ok(DataType::Tuple(specta::datatype::Tuple::new(elems)))
        }
        DataType::Intersection(parts) => {
            let mut out = Vec::with_capacity(parts.len());
            for part in parts {
                out.push(substitute_datatype(part, scope)?);
            }
            Ok(DataType::Intersection(out))
        }
        DataType::Reference(Reference::Named(reference)) => {
            let mut cloned = reference.clone();
            match &mut cloned.inner {
                NamedReferenceType::Inline { dt, .. } => {
                    **dt = substitute_datatype(dt, scope)?;
                }
                NamedReferenceType::Reference { generics, .. } => {
                    for (_, arg_dt) in generics.iter_mut() {
                        *arg_dt = substitute_datatype(arg_dt, scope)?;
                    }
                }
                NamedReferenceType::Recursive(_) => {}
            }
            Ok(DataType::Reference(Reference::Named(cloned)))
        }
        other => Ok(other.clone()),
    }
}

fn primitive_schema(primitive: &Primitive) -> Value {
    match primitive {
        Primitive::i8 => json!({ "type": "integer", "minimum": i8::MIN, "maximum": i8::MAX }),
        Primitive::i16 => json!({ "type": "integer", "minimum": i16::MIN, "maximum": i16::MAX }),
        Primitive::i32 => json!({ "type": "integer", "minimum": i32::MIN, "maximum": i32::MAX }),
        Primitive::i64 => json!({ "type": "integer", "minimum": i64::MIN, "maximum": i64::MAX }),
        Primitive::i128 | Primitive::isize => json!({ "type": "integer" }),
        Primitive::u8 => json!({ "type": "integer", "minimum": 0, "maximum": u8::MAX }),
        Primitive::u16 => json!({ "type": "integer", "minimum": 0, "maximum": u16::MAX }),
        Primitive::u32 => json!({ "type": "integer", "minimum": 0, "maximum": u32::MAX }),
        Primitive::u64 => json!({ "type": "integer", "minimum": 0, "maximum": u64::MAX }),
        Primitive::u128 | Primitive::usize => json!({ "type": "integer", "minimum": 0 }),
        Primitive::f16 | Primitive::f32 | Primitive::f64 | Primitive::f128 => {
            json!({ "type": "number" })
        }
        Primitive::bool => json!({ "type": "boolean" }),
        Primitive::char => json!({ "type": "string", "minLength": 1, "maxLength": 1 }),
        Primitive::str => json!({ "type": "string" }),
    }
}

fn is_open_any_schema(value: &Value) -> bool {
    matches!(value, Value::Object(map) if map.is_empty())
}

fn make_nullable_schema(inner: Value) -> Value {
    if is_open_any_schema(&inner) {
        return inner;
    }
    let null_schema = json!({ "type": "null" });
    if inner == null_schema {
        return inner;
    }
    let mut branches = Vec::new();
    push_unique_any_of(&mut branches, inner);
    push_unique_any_of(&mut branches, null_schema);
    if branches.len() == 1 {
        branches.into_iter().next().unwrap_or(Value::Null)
    } else {
        json!({ "anyOf": branches })
    }
}

fn push_unique_any_of(branches: &mut Vec<Value>, item: Value) {
    if let Value::Object(ref map) = item
        && map.len() == 1
        && let Some(Value::Array(inner_branches)) = map.get("anyOf")
    {
        for sub in inner_branches {
            push_unique_any_of(branches, sub.clone());
        }
        return;
    }
    if !branches.contains(&item) {
        branches.push(item);
    }
}

fn attach_named_metadata(schema: &mut Value, ndt: &NamedDataType) {
    if let Some(docs) = trim_non_empty(ndt.docs.as_ref()) {
        attach_description(schema, docs);
    }
    if let Some(deprecated) = &ndt.deprecated {
        attach_deprecated(schema, deprecated);
    }
}

fn attach_description(schema: &mut Value, docs: String) {
    if let Value::Object(map) = schema
        && !map.contains_key("$ref")
        && !map.contains_key("description")
    {
        map.insert("description".to_owned(), Value::String(docs));
    }
}

fn attach_deprecated(schema: &mut Value, deprecated: &specta::datatype::Deprecated) {
    if let Value::Object(map) = schema
        && !map.contains_key("$ref")
        && !map.contains_key("deprecated")
    {
        map.insert("deprecated".to_owned(), Value::Bool(true));
        if let Some(note) = &deprecated.note
            && !map.contains_key("description")
        {
            map.insert(
                "description".to_owned(),
                Value::String(format!("Deprecated: {note}")),
            );
        }
    }
}

/// Prefix-free injective encoder over `[A-Za-z0-9_-]`.
/// Literal `-` becomes `--`, `[A-Za-z0-9_]` stay literal, and any other UTF-8 byte
/// becomes `-<hex2>` (leaving `.` free as an unambiguous structural separator).
fn encode_def_atom(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'-' => out.push_str("--"),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' => out.push(byte as char),
            other => {
                use std::fmt::Write as _;
                let _ = write!(&mut out, "-{other:02x}");
            }
        }
    }
    out
}

fn def_ref_pointer(def_key: &str) -> String {
    let mut escaped = String::with_capacity(def_key.len());
    for ch in def_key.chars() {
        match ch {
            '~' => escaped.push_str("~0"),
            '/' => escaped.push_str("~1"),
            _ => escaped.push(ch),
        }
    }
    format!("#/$defs/{escaped}")
}

fn positive_decimal_range_regex(max_inclusive: u128) -> String {
    let max_str = max_inclusive.to_string();
    let bytes = max_str.as_bytes();
    let len = bytes.len();
    if len == 1 {
        return format!("[1-{}]", bytes[0] as char);
    }

    let mut alts = Vec::new();
    if len == 2 {
        alts.push("[1-9]".to_owned());
    } else {
        alts.push(format!("[1-9][0-9]{{0,{}}}", len - 2));
    }

    for i in 0..len {
        let min_digit = if i == 0 { b'1' } else { b'0' };
        let max_digit = bytes[i];
        if min_digit < max_digit {
            let prefix = &max_str[..i];
            let hi = (max_digit - 1) as char;
            let lo = min_digit as char;
            let digit_cls = if lo == hi {
                format!("{lo}")
            } else {
                format!("[{lo}-{hi}]")
            };
            let rem = len - 1 - i;
            let tail = match rem {
                0 => String::new(),
                1 => "[0-9]".to_owned(),
                n => format!("[0-9]{{{n}}}"),
            };
            alts.push(format!("{prefix}{digit_cls}{tail}"));
        }
    }

    alts.push(max_str);
    alts.join("|")
}

fn integer_key_pattern_schema(min: i128, max: i128) -> Value {
    let mut branches = vec!["0".to_owned()];
    if max > 0 {
        branches.push(positive_decimal_range_regex(max as u128));
    }
    if min < 0 {
        let neg_abs = min.unsigned_abs();
        branches.push(format!("-(?:{})", positive_decimal_range_regex(neg_abs)));
    }
    json!({ "pattern": format!("^(?:{})$", branches.join("|")) })
}

fn ensure_string_key_enum_schema(path: &str, schema: &Value) -> Result<(), String> {
    let Some(obj) = schema.as_object() else {
        return Err(format!(
            "unsupported non-object map key enum schema at {path}"
        ));
    };
    if let Some(Value::String(_)) = obj.get("const") {
        return Ok(());
    }
    if let Some(Value::Array(vals)) = obj.get("enum")
        && vals.iter().all(Value::is_string)
    {
        return Ok(());
    }
    if obj.get("type").and_then(Value::as_str) == Some("string") {
        return Ok(());
    }
    if let Some(Value::Array(branches)) = obj.get("anyOf")
        && branches
            .iter()
            .all(|branch| ensure_string_key_enum_schema(path, branch).is_ok())
    {
        return Ok(());
    }
    Err(format!(
        "unsupported non-string enum map key schema `{schema}` at {path}"
    ))
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, collections::BTreeMap};

    use specta::{
        Types,
        datatype::{DataType, Generic, NamedDataType, Reference},
    };

    use super::*;

    #[test]
    fn canonical_def_keys_are_injective_for_renamed_types_across_phases() {
        let mut types = Types::default();

        fn build_slash(_: &mut Types, ndt: &mut NamedDataType) {
            ndt.name = Cow::Borrowed("A/B");
            ndt.module_path = Cow::Borrowed("test_mod");
            ndt.ty = Some(DataType::Primitive(Primitive::u8));
        }
        fn build_under(_: &mut Types, ndt: &mut NamedDataType) {
            ndt.name = Cow::Borrowed("A_B");
            ndt.module_path = Cow::Borrowed("test_mod");
            ndt.ty = Some(DataType::Primitive(Primitive::u8));
        }
        fn build_tilde(_: &mut Types, ndt: &mut NamedDataType) {
            ndt.name = Cow::Borrowed("A~B");
            ndt.module_path = Cow::Borrowed("test_mod");
            ndt.ty = Some(DataType::Primitive(Primitive::u8));
        }

        let slash_dt = DataType::Reference(NamedDataType::init_with_sentinel(
            "injective_test_slash",
            &[],
            false,
            false,
            &mut types,
            build_slash,
            |_| DataType::Primitive(Primitive::u8),
        ));
        let under_dt = DataType::Reference(NamedDataType::init_with_sentinel(
            "injective_test_under",
            &[],
            false,
            false,
            &mut types,
            build_under,
            |_| DataType::Primitive(Primitive::u8),
        ));
        let tilde_dt = DataType::Reference(NamedDataType::init_with_sentinel(
            "injective_test_tilde",
            &[],
            false,
            false,
            &mut types,
            build_tilde,
            |_| DataType::Primitive(Primitive::u8),
        ));

        #[derive(specta::Type)]
        #[allow(dead_code)]
        struct Wrapper<T> {
            inner: T,
        }

        let DataType::Reference(Reference::Named(wrapper_ref)) =
            Wrapper::<u8>::definition(&mut types)
        else {
            panic!("Wrapper<u8> did not produce NamedReference");
        };
        let wrapper_ndt = types
            .get(&wrapper_ref)
            .expect("Wrapper NDT registered")
            .clone();

        let emitter = SchemaExporter::new(&types).expect("schema exporter initializes");
        let empty = GenericScope::new();
        let mut seen = BTreeSet::new();
        for arg in [&slash_dt, &under_dt, &tilde_dt] {
            for phase in [Phase::Serialize, Phase::Deserialize] {
                let key = emitter
                    .canonical_def_key(
                        "Wrapper",
                        &wrapper_ndt,
                        &[("T".to_owned(), arg.clone())],
                        phase,
                        &empty,
                    )
                    .expect("canonical key generated");
                assert!(seen.insert(key), "duplicate canonical def key detected");
            }
        }
        assert_eq!(seen.len(), 6);
        assert_ne!(encode_def_atom("A/B"), encode_def_atom("A_B"));
        assert_ne!(encode_def_atom("A/B"), encode_def_atom("A~B"));
        assert_ne!(encode_def_atom("A_B"), encode_def_atom("A~B"));
    }

    #[test]
    fn primitive_char_int64_and_map_keys_emit_exact_structural_constraints() {
        #[derive(specta::Type)]
        #[allow(dead_code)]
        enum KeyEnum {
            Alpha,
            Beta,
        }

        #[derive(specta::Type)]
        #[allow(dead_code)]
        struct PrimitivesAndMaps {
            ch: char,
            signed64: i64,
            unsigned64: u64,
            char_map: BTreeMap<char, bool>,
            i32_map: BTreeMap<i32, bool>,
            enum_map: BTreeMap<KeyEnum, bool>,
        }

        let mut types = Types::default();
        let dt = PrimitivesAndMaps::definition(&mut types);
        let mut emitter = SchemaExporter::new(&types).expect("schema exporter initializes");
        let root_ref = emitter
            .datatype_to_schema("root", &dt, Phase::Serialize, &BTreeMap::new())
            .expect("PrimitivesAndMaps emits cleanly");
        let def_key = root_ref["$ref"]
            .as_str()
            .unwrap()
            .strip_prefix("#/$defs/")
            .unwrap();
        let props = emitter.defs[def_key]["properties"].as_object().unwrap();

        assert_eq!(
            props["ch"],
            json!({ "type": "string", "minLength": 1, "maxLength": 1 })
        );
        assert_eq!(
            props["signed64"],
            json!({ "type": "integer", "minimum": i64::MIN, "maximum": i64::MAX })
        );
        assert_eq!(
            props["unsigned64"],
            json!({ "type": "integer", "minimum": 0, "maximum": u64::MAX })
        );
        assert_eq!(
            props["char_map"]["propertyNames"],
            json!({ "type": "string", "minLength": 1, "maxLength": 1 })
        );
        assert!(
            props["i32_map"]["propertyNames"]["pattern"]
                .as_str()
                .is_some_and(|p| p.contains("2147483647") && p.contains("-(?:")),
            "i32 map propertyNames should encode bounded decimal pattern: {}",
            props["i32_map"]
        );
        assert!(
            props["enum_map"]["propertyNames"]["$ref"]
                .as_str()
                .is_some(),
            "enum map propertyNames should reference string enum: {}",
            props["enum_map"]
        );
    }

    #[test]
    fn exporter_fails_closed_on_unsupported_types_and_duplicate_def_origins() {
        let types = Types::default();
        let mut emitter = SchemaExporter::new(&types).expect("schema exporter initializes");

        let unbound = DataType::Generic(Generic::new(Cow::Borrowed("T")));
        let err = emitter
            .datatype_to_schema("unbound", &unbound, Phase::Serialize, &BTreeMap::new())
            .expect_err("unbound generic must fail closed");
        assert!(err.contains("unbound generic"), "{err}");

        let opaque = DataType::Reference(Reference::opaque::<&'static str>("custom-opaque"));
        let err = emitter
            .datatype_to_schema("opaque", &opaque, Phase::Serialize, &BTreeMap::new())
            .expect_err("unknown OpaqueReference must fail closed");
        assert!(err.contains("unsupported opaque Specta reference"), "{err}");

        #[derive(specta::Type)]
        #[allow(dead_code)]
        struct MultiFieldKey {
            a: u8,
            b: u8,
        }

        let mut struct_key_types = Types::default();
        let key_dt = MultiFieldKey::definition(&mut struct_key_types);
        let mut key_emitter =
            SchemaExporter::new(&struct_key_types).expect("schema exporter initializes");
        let err = key_emitter
            .map_key_property_names_schema("map.<key>", &key_dt, Phase::Serialize, &BTreeMap::new())
            .expect_err("multi-field struct map key must fail closed");
        assert!(err.contains("unsupported struct map key"), "{err}");

        let bad_enum_schema = json!({ "anyOf": [{ "type": "integer" }] });
        let err = ensure_string_key_enum_schema("enum.<key>", &bad_enum_schema)
            .expect_err("non-string enum map key must fail closed");
        assert!(err.contains("unsupported non-string enum map key"), "{err}");

        let mut dup_types = Types::default();
        fn build_dup_a(_: &mut Types, ndt: &mut NamedDataType) {
            ndt.name = Cow::Borrowed("DupType");
            ndt.module_path = Cow::Borrowed("mod_x");
            ndt.ty = Some(DataType::Primitive(Primitive::u8));
        }
        fn build_dup_b(_: &mut Types, ndt: &mut NamedDataType) {
            ndt.name = Cow::Borrowed("DupType");
            ndt.module_path = Cow::Borrowed("mod_x");
            ndt.ty = Some(DataType::Primitive(Primitive::u16));
        }
        let first_dt = DataType::Reference(NamedDataType::init_with_sentinel(
            "dup_test_sentinel_a",
            &[],
            false,
            false,
            &mut dup_types,
            build_dup_a,
            |_| DataType::Primitive(Primitive::u8),
        ));
        let second_dt = DataType::Reference(NamedDataType::init_with_sentinel(
            "dup_test_sentinel_b",
            &[],
            false,
            false,
            &mut dup_types,
            build_dup_b,
            |_| DataType::Primitive(Primitive::u16),
        ));
        let mut dup_emitter = SchemaExporter::new(&dup_types).expect("schema exporter initializes");
        dup_emitter
            .datatype_to_schema("dup1", &first_dt, Phase::Serialize, &BTreeMap::new())
            .expect("first DupType succeeds");
        let err = dup_emitter
            .datatype_to_schema("dup2", &second_dt, Phase::Serialize, &BTreeMap::new())
            .expect_err("second distinct struct with identical module+wire name must collide");
        assert!(err.contains("JSON Schema $defs key collision"), "{err}");
    }
}
