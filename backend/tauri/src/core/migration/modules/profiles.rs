use super::super::{Ctx, MigrationStep, ModuleMigrator};
use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml::{
    Mapping, Value,
    value::{Tag, TaggedValue},
};

pub static MIGRATOR: ProfilesMigrator = ProfilesMigrator;

static VERSION_2_0_0: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());
static NULL_VALUE: MigrateProfilesNullValue = MigrateProfilesNullValue;
static SCRIPT_NEWTYPE: MigrateProfileScriptNewtype = MigrateProfileScriptNewtype;
static CLEAN_SCHEMA: MigrateProfilesCleanSchema = MigrateProfilesCleanSchema;
static STEPS: [&dyn MigrationStep; 3] = [&NULL_VALUE, &SCRIPT_NEWTYPE, &CLEAN_SCHEMA];

pub struct ProfilesMigrator;

impl ModuleMigrator for ProfilesMigrator {
    fn module(&self) -> &'static str {
        "profiles"
    }

    fn detect_baseline(&self, ctx: &Ctx) -> anyhow::Result<u64> {
        let profiles_path = ctx.profiles_path();
        if !profiles_path.exists() {
            return Ok(current_revision());
        }

        let raw = std::fs::read_to_string(&profiles_path)?;
        let profiles: Mapping = serde_yaml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;
        if is_clean_schema(&profiles) {
            return Ok(current_revision());
        }
        Ok(0)
    }

    fn steps(&self) -> &'static [&'static dyn MigrationStep] {
        &STEPS
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateProfilesNullValue;

impl MigrationStep for MigrateProfilesNullValue {
    fn id(&self) -> &'static str {
        "profiles/null_value"
    }

    fn module(&self) -> &'static str {
        "profiles"
    }

    fn revision(&self) -> u64 {
        1
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateProfilesNullValue"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let profiles_path = ctx.profiles_path();
        if !profiles_path.exists() {
            return Ok(());
        }
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        let mut profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;

        profiles.iter_mut().for_each(|(key, value)| {
            if value.is_null() {
                println!("detected null value in profiles {key:?} should be migrated");
                *value = serde_yaml::Value::Sequence(Vec::new());
            }
        });
        write_profiles_atomic(&profiles_path, &profiles, None)?;
        Ok(())
    }

    fn rollback(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let profiles_path = ctx.profiles_path();
        if !profiles_path.exists() {
            return Ok(());
        }
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        let mut profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;

        profiles.iter_mut().for_each(|(key, value)| {
            if key.is_string() && key.as_str().unwrap() == "chain" && value.is_sequence() {
                println!("detected sequence value in profiles {key:?} should be migrated");
                *value = serde_yaml::Value::Null;
            }
            if key.is_string() && key.as_str().unwrap() == "current" && value.is_sequence() {
                println!("detected sequence value in profiles {key:?} should be migrated");
                *value = serde_yaml::Value::Null;
            }
        });
        write_profiles_atomic(&profiles_path, &profiles, None)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateProfileScriptNewtype;

impl MigrationStep for MigrateProfileScriptNewtype {
    fn id(&self) -> &'static str {
        "profiles/script_newtype"
    }

    fn module(&self) -> &'static str {
        "profiles"
    }

    fn revision(&self) -> u64 {
        2
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateProfileScriptNewtype"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let profiles_path = ctx.profiles_path();
        if !profiles_path.exists() {
            eprintln!("profiles dir not found, skipping migration");
            return Ok(());
        }
        eprintln!("Trying to read profiles files...");
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        eprintln!("Trying to parse profiles files...");
        let profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;
        eprintln!("Trying to migrate profiles files...");
        let profiles = migrate_profile_data(profiles);
        eprintln!("Trying to write profiles files...");
        write_profiles_atomic(
            &profiles_path,
            &profiles,
            Some("# Profiles Config for Clash Nyanpasu"),
        )?;
        Ok(())
    }

    fn rollback(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let profiles_path = ctx.profiles_path();
        if !profiles_path.exists() {
            eprintln!("profiles dir not found, skipping discard");
            return Ok(());
        }
        eprintln!("Trying to read profiles files...");
        let profiles = std::fs::read_to_string(profiles_path.clone())?;
        eprintln!("Trying to parse profiles files...");
        let profiles: Mapping = serde_yaml::from_str(&profiles)
            .map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;
        eprintln!("Trying to discard profiles files...");
        let profiles = discard_profile_data(profiles);
        eprintln!("Trying to write profiles files...");
        write_profiles_atomic(
            &profiles_path,
            &profiles,
            Some("# Profiles Config for Clash Nyanpasu"),
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateProfilesCleanSchema;

impl MigrationStep for MigrateProfilesCleanSchema {
    fn id(&self) -> &'static str {
        "profiles/clean_schema"
    }

    fn module(&self) -> &'static str {
        "profiles"
    }

    fn revision(&self) -> u64 {
        3
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateProfilesCleanSchema"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        run_clean_schema(ctx)
    }

    fn rollback(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        rollback_clean_schema(ctx)
    }
}

/// New-schema marker: every item is a `config`/`transform` definition. A doc
/// with zero legacy markers (no legacy item types, no top-level `chain`) has
/// nothing to migrate and counts as clean.
fn is_clean_schema(doc: &Mapping) -> bool {
    if doc.contains_key("chain") {
        return false;
    }
    match doc.get("items").and_then(Value::as_sequence) {
        None => true,
        Some(items) => items.iter().all(|item| {
            item.as_mapping()
                .and_then(|item| item.get("type"))
                .and_then(Value::as_str)
                .is_some_and(|ty| matches!(ty, "config" | "transform"))
        }),
    }
}

fn run_clean_schema(ctx: &mut Ctx) -> anyhow::Result<()> {
    let path = ctx.profiles_path();
    if !path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)?;
    let doc: Mapping =
        serde_yaml::from_str(&raw).map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;
    if is_clean_schema(&doc) {
        return Ok(());
    }

    // R15: backup first, then transform (D3: mandatory .bak)
    let bak = path.with_extension("yaml.bak");
    crate::core::migration::fs::atomic_write(&bak, raw.as_bytes())?;

    let migrated = migrate_clean_schema(doc)?;

    // Typed round-trip: the only accepted output is a document the new domain
    // model can load AND validate (design §14.4). Duplicate uids are rejected
    // here by the items deserializer (R13).
    let profiles: nyanpasu_config::profile::Profiles =
        serde_yaml::from_value(Value::Mapping(migrated))
            .map_err(|e| anyhow::anyhow!("clean-schema output rejected by domain model: {e}"))?;
    profiles
        .validate()
        .map_err(|errors| anyhow::anyhow!("clean-schema output failed validation: {errors:?}"))?;

    let body = serde_yaml::to_string(&profiles)
        .map_err(|e| anyhow::anyhow!("failed to serialize migrated profiles: {e}"))?;
    let content = format!("# Profiles Config for Clash Nyanpasu\n\n{body}");
    crate::core::migration::fs::atomic_write(&path, content.as_bytes())?;
    Ok(())
}

fn rollback_clean_schema(ctx: &mut Ctx) -> anyhow::Result<()> {
    let path = ctx.profiles_path();
    let bak = path.with_extension("yaml.bak");
    if !bak.exists() {
        eprintln!("profiles.yaml.bak not found, nothing to roll back");
        return Ok(());
    }
    let raw = std::fs::read(&bak)?;
    crate::core::migration::fs::atomic_write(&path, &raw)
}

#[derive(Debug, thiserror::Error)]
#[error("profiles clean-schema migration failed (uid={uid:?}, field={field_path}): {reason}")]
pub struct CleanSchemaError {
    pub uid: Option<String>,
    pub field_path: String,
    pub reason: String,
}

impl CleanSchemaError {
    fn new(uid: Option<&str>, field_path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            uid: uid.map(str::to_owned),
            field_path: field_path.into(),
            reason: reason.into(),
        }
    }
}

fn str_value(v: &Value) -> Option<&str> {
    v.as_str()
}

fn migrate_item(item: Mapping) -> Result<Mapping, CleanSchemaError> {
    let uid = item
        .get("uid")
        .and_then(str_value)
        .map(str::to_owned)
        .ok_or_else(|| CleanSchemaError::new(None, "uid", "missing item uid"))?;
    let fail = |field: &str, reason: &str| CleanSchemaError::new(Some(&uid), field, reason);

    let ty = item
        .get("type")
        .and_then(str_value)
        .ok_or_else(|| fail("type", "missing or non-string legacy type"))?
        .to_owned();

    let name = item
        .get("name")
        .and_then(str_value)
        .ok_or_else(|| fail("name", "missing profile name"))?
        .to_owned();
    let desc = match item.get("desc") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(fail("desc", "desc must be a string or null")),
    };
    let file = item
        .get("file")
        .and_then(str_value)
        .ok_or_else(|| fail("file", "missing materialized file"))?
        .to_owned();
    let updated = item.get("updated").cloned();

    let allowed: &[&str] = match ty.as_str() {
        "remote" => &[
            "uid", "type", "name", "desc", "file", "updated", "url", "extra", "option", "chain",
            "chains",
        ],
        "local" => &[
            "uid", "type", "name", "desc", "file", "updated", "symlinks", "chain", "chains",
        ],
        "merge" => &["uid", "type", "name", "desc", "file", "updated"],
        "script" => &[
            "uid",
            "type",
            "name",
            "desc",
            "file",
            "updated",
            "script_type",
        ],
        other => return Err(fail("type", &format!("unknown legacy type `{other}`"))),
    };
    for key in item.keys() {
        let Some(key) = key.as_str() else {
            return Err(fail("<non-string key>", "item keys must be strings"));
        };
        if !allowed.contains(&key) {
            return Err(fail(
                key,
                "unknown legacy field; refusing to drop it silently",
            ));
        }
    }

    let transforms = match (item.get("chain"), item.get("chains")) {
        (Some(_), Some(_)) => return Err(fail("chain", "both `chain` and alias `chains` present")),
        (Some(v), None) | (None, Some(v)) => match v {
            Value::Sequence(seq) => Some(seq.clone()),
            Value::Null => None,
            _ => return Err(fail("chain", "chain must be a sequence")),
        },
        (None, None) => None,
    };

    let is_url = file.starts_with("http://") || file.starts_with("https://");
    let materialized_file = if is_url {
        let ext = match ty.as_str() {
            "script" => match item.get("script_type").and_then(str_value) {
                Some("lua") => "lua",
                _ => "js",
            },
            _ => "yaml",
        };
        format!("{uid}.{ext}")
    } else {
        validate_managed_relative(&uid, &file)?;
        file.clone()
    };

    let mut materialized = Mapping::new();
    materialized.insert("file".into(), Value::String(materialized_file));
    if let Some(updated) = updated {
        match &updated {
            Value::Number(_) => {
                materialized.insert("updated_at".into(), updated);
            }
            Value::Null => {}
            _ => return Err(fail("updated", "updated must be an integer timestamp")),
        }
    }

    let source = if ty == "remote" || is_url {
        if ty == "local" && item.get("symlinks").is_some_and(|value| !value.is_null()) {
            return Err(fail(
                "symlinks",
                "remote source cannot preserve external symlink binding",
            ));
        }
        let url = if ty == "remote" {
            item.get("url")
                .and_then(str_value)
                .ok_or_else(|| fail("url", "missing subscription url"))?
                .to_owned()
        } else {
            file.clone()
        };
        let option_value = if ty == "remote" {
            item.get("option")
        } else {
            None
        };
        let option = migrate_remote_options(&uid, option_value)?;
        let subscription = if ty == "remote" {
            migrate_subscription(&uid, item.get("extra"))?
        } else {
            None
        };

        let mut source = Mapping::new();
        source.insert("type".into(), "remote".into());
        for (key, value) in materialized {
            source.insert(key, value);
        }
        source.insert("url".into(), Value::String(url));
        source.insert("option".into(), Value::Mapping(option));
        if let Some(subscription) = subscription {
            source.insert("subscription".into(), Value::Mapping(subscription));
        }
        source
    } else {
        let mut binding = Mapping::new();
        match item.get("symlinks") {
            Some(Value::String(target)) => {
                let is_absolute =
                    std::path::Path::new(target).is_absolute() || target.starts_with('/');
                if !is_absolute {
                    return Err(fail("symlinks", "external symlink target must be absolute"));
                }
                binding.insert("type".into(), "external".into());
                for (key, value) in materialized {
                    binding.insert(key, value);
                }
                binding.insert("target".into(), Value::String(target.clone()));
                binding.insert("mode".into(), "symlink".into());
            }
            None | Some(Value::Null) => {
                binding.insert("type".into(), "managed".into());
                for (key, value) in materialized {
                    binding.insert(key, value);
                }
            }
            Some(_) => return Err(fail("symlinks", "symlinks must be a string path")),
        }
        let mut source = Mapping::new();
        source.insert("type".into(), "local".into());
        source.insert("binding".into(), Value::Mapping(binding));
        source
    };

    let mut out = Mapping::new();
    out.insert("uid".into(), Value::String(uid.clone()));
    out.insert("name".into(), Value::String(name));
    if let Some(desc) = desc {
        out.insert("desc".into(), Value::String(desc));
    }
    match ty.as_str() {
        "remote" | "local" => {
            let mut config = Mapping::new();
            config.insert("type".into(), "file".into());
            config.insert("source".into(), Value::Mapping(source));
            if let Some(transforms) = transforms {
                config.insert("transforms".into(), Value::Sequence(transforms));
            }
            out.insert("type".into(), "config".into());
            out.insert("config".into(), Value::Mapping(config));
        }
        "merge" | "script" => {
            let mut transform = Mapping::new();
            if ty == "merge" {
                transform.insert("type".into(), "overlay".into());
                transform.insert("source".into(), Value::Mapping(source));
            } else {
                let runtime = item
                    .get("script_type")
                    .and_then(str_value)
                    .ok_or_else(|| fail("script_type", "missing script runtime"))?;
                if !matches!(runtime, "javascript" | "lua") {
                    return Err(fail("script_type", "unknown script runtime"));
                }
                transform.insert("type".into(), "script".into());
                transform.insert("source".into(), Value::Mapping(source));
                transform.insert("runtime".into(), runtime.into());
            }
            out.insert("type".into(), "transform".into());
            out.insert("transform".into(), Value::Mapping(transform));
        }
        _ => unreachable!("validated above"),
    }
    Ok(out)
}

fn validate_managed_relative(uid: &str, file: &str) -> Result<(), CleanSchemaError> {
    use std::path::{Component, Path};
    let path = Path::new(file);
    let bad = file.is_empty()
        || file.contains("://")
        || path.is_absolute()
        || file.starts_with('/')
        || path.components().any(|c| {
            matches!(
                c,
                Component::Prefix(_)
                    | Component::RootDir
                    | Component::ParentDir
                    | Component::CurDir
            )
        });
    if bad {
        return Err(CleanSchemaError::new(
            Some(uid),
            "file",
            "materialized file must be a plain relative path",
        ));
    }
    Ok(())
}

fn migrate_remote_options(uid: &str, option: Option<&Value>) -> Result<Mapping, CleanSchemaError> {
    let fail = |field: &str, reason: &str| CleanSchemaError::new(Some(uid), field, reason);
    let mut out = Mapping::new();
    match option {
        None => {
            out.insert("with_proxy".into(), Value::Bool(false));
            out.insert("self_proxy".into(), Value::Bool(true));
            out.insert("update_interval_minutes".into(), Value::from(120u64));
        }
        Some(Value::Null) => return Err(fail("option", "option must be a mapping")),
        Some(Value::Mapping(option)) => {
            for key in option.keys() {
                let Some(key) = key.as_str() else {
                    return Err(fail("option", "option keys must be strings"));
                };
                if !["user_agent", "with_proxy", "self_proxy", "update_interval"].contains(&key) {
                    return Err(fail(
                        &format!("option.{key}"),
                        "unknown legacy option field",
                    ));
                }
            }
            if let Some(user_agent) = option.get("user_agent")
                && !user_agent.is_null()
            {
                out.insert("user_agent".into(), user_agent.clone());
            }
            let flag = |key: &str| -> Result<bool, CleanSchemaError> {
                match option.get(key) {
                    None | Some(Value::Null) => Ok(false),
                    Some(Value::Bool(value)) => Ok(*value),
                    Some(_) => Err(fail(&format!("option.{key}"), "must be a boolean")),
                }
            };
            out.insert("with_proxy".into(), Value::Bool(flag("with_proxy")?));
            out.insert("self_proxy".into(), Value::Bool(flag("self_proxy")?));
            let interval = match option.get("update_interval") {
                None | Some(Value::Null) => 120,
                Some(Value::Number(n)) => n.as_u64().ok_or_else(|| {
                    fail("option.update_interval", "must be a non-negative integer")
                })?,
                Some(_) => return Err(fail("option.update_interval", "must be an integer")),
            };
            if interval == 0 {
                return Err(fail(
                    "option.update_interval",
                    "zero interval is not representable in the clean schema; fix the profile before migrating",
                ));
            }
            out.insert("update_interval_minutes".into(), Value::from(interval));
        }
        Some(_) => return Err(fail("option", "option must be a mapping")),
    }
    Ok(out)
}

fn migrate_subscription(
    uid: &str,
    extra: Option<&Value>,
) -> Result<Option<Mapping>, CleanSchemaError> {
    let fail = |field: &str, reason: &str| CleanSchemaError::new(Some(uid), field, reason);
    match extra {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Mapping(extra)) => {
            let mut out = Mapping::new();
            for (key, value) in extra {
                let Some(key) = key.as_str() else {
                    return Err(fail("extra", "extra keys must be strings"));
                };
                match key {
                    "upload" | "download" | "total" => {
                        out.insert(key.into(), value.clone());
                    }
                    "expire" => {
                        if value.as_u64() != Some(0) && !value.is_null() {
                            out.insert("expire".into(), value.clone());
                        }
                    }
                    other => {
                        return Err(fail(
                            &format!("extra.{other}"),
                            "unknown legacy extra field",
                        ));
                    }
                }
            }
            Ok((!out.is_empty()).then_some(out))
        }
        Some(_) => Err(fail("extra", "extra must be a mapping")),
    }
}

/// R7/R10/R11/R12: whole-document mapping. Input is the post-rev2 doc.
fn migrate_clean_schema(doc: Mapping) -> Result<Mapping, CleanSchemaError> {
    for key in doc.keys() {
        let Some(key) = key.as_str() else {
            return Err(CleanSchemaError::new(
                None,
                "<non-string key>",
                "top-level keys must be strings",
            ));
        };
        if !["current", "chain", "valid", "items"].contains(&key) {
            return Err(CleanSchemaError::new(
                None,
                key,
                "unknown legacy top-level field",
            ));
        }
    }

    let items: Vec<Mapping> = match doc.get("items") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Sequence(seq)) => seq
            .iter()
            .map(|v| {
                v.as_mapping().cloned().ok_or_else(|| {
                    CleanSchemaError::new(None, "items", "every item must be a mapping")
                })
            })
            .collect::<Result<_, _>>()?,
        Some(_) => {
            return Err(CleanSchemaError::new(
                None,
                "items",
                "items must be a sequence",
            ));
        }
    };

    let mut new_items: Vec<Value> = Vec::with_capacity(items.len() + 1);
    let mut uids: Vec<String> = Vec::with_capacity(items.len());
    let mut direct_file_config: Vec<String> = Vec::new();
    for item in items {
        let migrated = migrate_item(item)?;
        let uid = migrated["uid"]
            .as_str()
            .expect("set by migrate_item")
            .to_owned();
        if migrated.get("type") == Some(&Value::from("config")) {
            direct_file_config.push(uid.clone());
        }
        uids.push(uid);
        new_items.push(Value::Mapping(migrated));
    }

    // R10: current forms
    let old_current: Vec<String> = match doc.get("current") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Sequence(seq)) => seq
            .iter()
            .map(|v| {
                v.as_str().map(str::to_owned).ok_or_else(|| {
                    CleanSchemaError::new(None, "current", "current entries must be strings")
                })
            })
            .collect::<Result<_, _>>()?,
        Some(_) => {
            return Err(CleanSchemaError::new(
                None,
                "current",
                "current must be a string or sequence",
            ));
        }
    };

    let current: Option<String> = match old_current.len() {
        0 => None,
        1 => Some(old_current[0].clone()),
        _ => {
            for member in &old_current {
                if !uids.contains(member) {
                    return Err(CleanSchemaError::new(
                        Some(member),
                        "current",
                        "current member does not exist",
                    ));
                }
                if !direct_file_config.contains(member) {
                    return Err(CleanSchemaError::new(
                        Some(member),
                        "current",
                        "multi-current member cannot be represented as a direct FileConfig",
                    ));
                }
            }
            let uid = {
                let mut candidate = "combined-profile".to_owned();
                let mut n = 1;
                while uids.contains(&candidate) {
                    n += 1;
                    candidate = format!("combined-profile-{n}");
                }
                candidate
            };
            let mut config = Mapping::new();
            config.insert("type".into(), "composition".into());
            config.insert("base".into(), Value::String(old_current[0].clone()));
            config.insert(
                "extend_proxies_from".into(),
                Value::Sequence(
                    old_current[1..]
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect(),
                ),
            );
            let mut combined = Mapping::new();
            combined.insert("uid".into(), Value::String(uid.clone()));
            combined.insert("name".into(), "Combined Profile".into());
            combined.insert("type".into(), "config".into());
            combined.insert("config".into(), Value::Mapping(config));
            new_items.push(Value::Mapping(combined));
            Some(uid)
        }
    };

    let mut out = Mapping::new();
    if let Some(current) = current {
        out.insert("current".into(), Value::String(current));
    }
    match doc.get("chain") {
        None | Some(Value::Null) => {}
        Some(Value::Sequence(seq)) if seq.is_empty() => {}
        Some(Value::Sequence(seq)) => {
            out.insert("global_transforms".into(), Value::Sequence(seq.clone()));
        }
        Some(_) => {
            return Err(CleanSchemaError::new(
                None,
                "chain",
                "chain must be a sequence",
            ));
        }
    }
    if let Some(valid) = doc.get("valid")
        && !valid.is_null()
    {
        out.insert("valid".into(), valid.clone());
    }
    out.insert("items".into(), Value::Sequence(new_items));
    Ok(out)
}

/// Atomically persist a profiles mapping, mirroring [`crate::utils::help::save_yaml`]
/// but writing through a temp file + rename so a crash mid-write can never
/// truncate the user's `profiles.yaml`.
fn write_profiles_atomic(
    path: &std::path::Path,
    profiles: &Mapping,
    prefix: Option<&str>,
) -> anyhow::Result<()> {
    let body = serde_yaml::to_string(profiles)
        .map_err(|e| anyhow::anyhow!("failed to serialize profiles: {e}"))?;
    let content = match prefix {
        Some(prefix) => format!("{prefix}\n\n{body}"),
        None => body,
    };
    crate::core::migration::fs::atomic_write(path, content.as_bytes())
}

fn current_revision() -> u64 {
    STEPS.last().map(|step| step.revision()).unwrap_or_default()
}

fn migrate_profile_data(mut mapping: Mapping) -> Mapping {
    if let Some(items) = mapping.get_mut("items")
        && let Some(items) = items.as_sequence_mut()
    {
        for item in items {
            if let Some(item) = item.as_mapping_mut()
                && let Some(ty) = item.get("type").cloned()
                && let Value::Tagged(tag) = ty
                && tag.tag == "script"
                && let Some(script_kind) = tag.value.as_str()
            {
                item.insert(
                    "type".into(),
                    serde_yaml::Value::String("script".to_string()),
                );
                item.insert(
                    "script_type".into(),
                    serde_yaml::Value::String(script_kind.to_string()),
                );
            }
        }
    }

    mapping
}

fn discard_profile_data(mut mapping: Mapping) -> Mapping {
    if let Some(items) = mapping.get_mut("items")
        && let Some(items) = items.as_sequence_mut()
    {
        for item in items {
            if let Some(item) = item.as_mapping_mut()
                && let Some(ty) = item.get("type").cloned()
                && let Value::String(ty) = ty
                && ty == "script"
                && let Some(script_kind) = item.get("script_type").cloned()
            {
                item.insert(
                    "type".into(),
                    serde_yaml::Value::Tagged(Box::new(TaggedValue {
                        tag: Tag::new("script"),
                        value: script_kind,
                    })),
                );
                item.remove("script_type");
            }
        }
    }

    mapping
}

#[cfg(test)]
mod tests {
    use crate::config::Profiles;

    use super::*;
    use pretty_assertions::assert_str_eq;

    const ORIGINAL_SAMPLE: &str = r#"current:
- rIWXPHuafvEM
chain: []
valid:
- dns
- unified-delay
- tcp-concurrent
- tun
- profile
items:
- uid: rIWXPHuafvEM
  type: remote
  name: 🌸云
  file: rIWXPHuafvEM.yaml
  desc: null
  updated: 1758110672
  url: https://example.com
  extra:
    upload: 3641183914
    download: 39111158992
    total: 42946719600
    expire: 1769123200
  option:
    with_proxy: false
    self_proxy: true
    update_interval: 1440
  chain:
  - siL1cvjnvLB6
  - sxI0dHKeqSNg
- uid: siL1cvjnvLB6
  type: !script javascript
  name: 花☁️处理
  file: siL1cvjnvLB6.js
  desc: ''
  updated: 1720954186
- uid: sxI0dHKeqSNg
  type: !script javascript
  name: 🌸☁️图标
  file: sxI0dHKeqSNg.js
  desc: ''
  updated: 1722656540
- uid: sZYZe33w7RKV
  type: !script lua
  name: 图标
  file: sZYZe33w7RKV.lua
  desc: ''
  updated: 1724082226
- uid: lkvV5JXfzO34
  type: local
  name: New Profile
  file: lkvV5JXfzO34.yaml
  desc: ''
  updated: 1725587682
  chain: []
- uid: lJynXCoMMIUd
  type: local
  name: New Profile
  file: lJynXCoMMIUd.yaml
  desc: ''
  updated: 1726252304
  chain: []
- uid: lBtaVEaMAR97
  type: local
  name: Test
  file: lBtaVEaMAR97.yaml
  desc: ''
  updated: 1727621893
  chain: []
"#;

    const MIGRATED_SAMPLE: &str = r#"current:
- rIWXPHuafvEM
chain: []
valid:
- dns
- unified-delay
- tcp-concurrent
- tun
- profile
items:
- uid: rIWXPHuafvEM
  type: remote
  name: 🌸云
  file: rIWXPHuafvEM.yaml
  desc: null
  updated: 1758110672
  url: https://example.com
  extra:
    upload: 3641183914
    download: 39111158992
    total: 42946719600
    expire: 1769123200
  option:
    with_proxy: false
    self_proxy: true
    update_interval: 1440
  chain:
  - siL1cvjnvLB6
  - sxI0dHKeqSNg
- uid: siL1cvjnvLB6
  type: script
  name: 花☁️处理
  file: siL1cvjnvLB6.js
  desc: ''
  updated: 1720954186
  script_type: javascript
- uid: sxI0dHKeqSNg
  type: script
  name: 🌸☁️图标
  file: sxI0dHKeqSNg.js
  desc: ''
  updated: 1722656540
  script_type: javascript
- uid: sZYZe33w7RKV
  type: script
  name: 图标
  file: sZYZe33w7RKV.lua
  desc: ''
  updated: 1724082226
  script_type: lua
- uid: lkvV5JXfzO34
  type: local
  name: New Profile
  file: lkvV5JXfzO34.yaml
  desc: ''
  updated: 1725587682
  chain: []
- uid: lJynXCoMMIUd
  type: local
  name: New Profile
  file: lJynXCoMMIUd.yaml
  desc: ''
  updated: 1726252304
  chain: []
- uid: lBtaVEaMAR97
  type: local
  name: Test
  file: lBtaVEaMAR97.yaml
  desc: ''
  updated: 1727621893
  chain: []
"#;

    const CLEAN_SAMPLE: &str = r#"valid:
- dns
items:
- uid: aaa
  name: A
  type: config
  config:
    type: file
    source:
      type: local
      binding:
        type: managed
        file: aaa.yaml
"#;

    #[test]
    fn clean_schema_detection() {
        let clean: Mapping = serde_yaml::from_str(CLEAN_SAMPLE).unwrap();
        assert!(is_clean_schema(&clean));
        let legacy: Mapping = serde_yaml::from_str(MIGRATED_SAMPLE).unwrap();
        assert!(!is_clean_schema(&legacy));
        assert!(is_clean_schema(&Mapping::new()));
    }

    fn item(yaml: &str) -> Mapping {
        serde_yaml::from_str(yaml).unwrap()
    }

    fn migrated(yaml: &str) -> Mapping {
        migrate_item(item(yaml)).unwrap()
    }

    fn yaml_eq(actual: &Mapping, expected: &str) {
        let expected: Mapping = serde_yaml::from_str(expected).unwrap();
        pretty_assertions::assert_eq!(
            serde_yaml::to_value(actual).unwrap(),
            serde_yaml::to_value(&expected).unwrap()
        );
    }

    #[test]
    fn remote_item_full_mapping() {
        let out = migrated(
            r#"uid: r1
type: remote
name: Cloud
file: r1.yaml
desc: hello
updated: 1758110672
url: https://example.com
extra: {upload: 1, download: 2, total: 3, expire: 1769123200}
option: {with_proxy: false, self_proxy: true, update_interval: 1440, user_agent: ua}
chain: [t1, t2]
"#,
        );
        yaml_eq(
            &out,
            r#"uid: r1
name: Cloud
desc: hello
type: config
config:
  type: file
  source:
    type: remote
    file: r1.yaml
    updated_at: 1758110672
    url: https://example.com
    option: {user_agent: ua, with_proxy: false, self_proxy: true, update_interval_minutes: 1440}
    subscription: {upload: 1, download: 2, total: 3, expire: 1769123200}
  transforms: [t1, t2]
"#,
        );
    }

    #[test]
    fn remote_option_absent_defaults_to_legacy_effective_values() {
        let out = migrated("uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\n");
        let option = out["config"]["source"]["option"].as_mapping().unwrap();
        assert_eq!(option["with_proxy"], Value::Bool(false));
        assert_eq!(option["self_proxy"], Value::Bool(true));
        assert_eq!(option["update_interval_minutes"], Value::from(120));
    }

    #[test]
    fn remote_option_partial_uses_apply_default_semantics() {
        let out = migrated(
            "uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\noption: {update_interval: 60}\n",
        );
        let option = out["config"]["source"]["option"].as_mapping().unwrap();
        assert_eq!(option["with_proxy"], Value::Bool(false));
        assert_eq!(option["self_proxy"], Value::Bool(false));
        assert_eq!(option["update_interval_minutes"], Value::from(60));
    }

    #[test]
    fn remote_option_null_fails_explicitly() {
        let err = migrate_item(item(
            "uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\noption: null\n",
        ))
        .unwrap_err();
        assert_eq!(err.uid.as_deref(), Some("r1"));
        assert_eq!(err.field_path, "option");
        assert_eq!(err.reason, "option must be a mapping");
    }

    #[test]
    fn remote_extra_expire_zero_becomes_absent() {
        let out = migrated(
            "uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\nextra: {upload: 0, download: 0, total: 0, expire: 0}\n",
        );
        let subscription = out["config"]["source"]["subscription"]
            .as_mapping()
            .unwrap();
        assert!(!subscription.contains_key("expire"));
        assert_eq!(subscription["upload"], Value::from(0));
    }

    #[test]
    fn remote_failures_carry_uid_and_field() {
        let err =
            migrate_item(item("uid: r1\ntype: remote\nname: A\nfile: r1.yaml\n")).unwrap_err();
        assert_eq!(err.uid.as_deref(), Some("r1"));
        assert_eq!(err.field_path, "url");

        let err = migrate_item(item(
            "uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\noption: {update_interval: 0}\n",
        ))
        .unwrap_err();
        assert_eq!(err.field_path, "option.update_interval");

        let err = migrate_item(item(
            "uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\noption: {bogus: 1}\n",
        ))
        .unwrap_err();
        assert_eq!(err.field_path, "option.bogus");

        let err = migrate_item(item(
            "uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\nwhatever: 1\n",
        ))
        .unwrap_err();
        assert_eq!(err.field_path, "whatever");
    }

    #[test]
    fn local_item_managed_mapping() {
        let out =
            migrated("uid: l1\ntype: local\nname: L\nfile: l1.yaml\nupdated: 5\nchains: [t1]\n");
        yaml_eq(
            &out,
            r#"uid: l1
name: L
type: config
config:
  type: file
  source:
    type: local
    binding: {type: managed, file: l1.yaml, updated_at: 5}
  transforms: [t1]
"#,
        );
    }

    #[test]
    fn local_symlinks_becomes_external_symlink_binding() {
        let out = migrated(
            "uid: l1\ntype: local\nname: L\nfile: l1.yaml\nsymlinks: /outside/real.yaml\n",
        );
        yaml_eq(
            &out,
            r#"uid: l1
name: L
type: config
config:
  type: file
  source:
    type: local
    binding: {type: external, file: l1.yaml, target: /outside/real.yaml, mode: symlink}
"#,
        );
        // 相对 target 显式失败
        let err = migrate_item(item(
            "uid: l1\ntype: local\nname: L\nfile: l1.yaml\nsymlinks: not/absolute.yaml\n",
        ))
        .unwrap_err();
        assert_eq!(err.field_path, "symlinks");
    }

    #[test]
    fn merge_and_script_become_transforms() {
        let out = migrated("uid: m1\ntype: merge\nname: M\nfile: m1.yaml\n");
        yaml_eq(
            &out,
            r#"uid: m1
name: M
type: transform
transform:
  type: overlay
  source:
    type: local
    binding: {type: managed, file: m1.yaml}
"#,
        );
        let out = migrated("uid: s1\ntype: script\nname: S\nfile: s1.lua\nscript_type: lua\n");
        yaml_eq(
            &out,
            r#"uid: s1
name: S
type: transform
transform:
  type: script
  source:
    type: local
    binding: {type: managed, file: s1.lua}
  runtime: lua
"#,
        );
    }

    #[test]
    fn url_in_file_converts_to_remote_source_per_legacy_type() {
        // design §14.2: 定义按旧 type,Source 改 Remote,file 重新生成
        let out = migrated("uid: l1\ntype: local\nname: L\nfile: https://e.com/sub.yaml\n");
        assert_eq!(out["type"], Value::from("config"));
        let source = out["config"]["source"].as_mapping().unwrap();
        assert_eq!(source["type"], Value::from("remote"));
        assert_eq!(source["url"], Value::from("https://e.com/sub.yaml"));
        assert_eq!(source["file"], Value::from("l1.yaml"));
        let option = source["option"].as_mapping().unwrap();
        assert_eq!(option["self_proxy"], Value::Bool(true)); // R5 absent 语义

        let err = migrate_item(item(
            "uid: l1\ntype: local\nname: L\nfile: https://e.com/sub.yaml\nsymlinks: /outside/real.yaml\n",
        ))
        .unwrap_err();
        assert_eq!(err.uid.as_deref(), Some("l1"));
        assert_eq!(err.field_path, "symlinks");

        let out = migrated(
            "uid: s1\ntype: script\nname: S\nfile: https://e.com/x.js\nscript_type: javascript\n",
        );
        assert_eq!(out["transform"]["source"]["file"], Value::from("s1.js"));
    }

    #[test]
    fn item_failures_for_non_remote_kinds() {
        // merge/script 不允许 chain(R8 → 未知键)
        let err = migrate_item(item(
            "uid: m1\ntype: merge\nname: M\nfile: m1.yaml\nchain: []\n",
        ))
        .unwrap_err();
        assert_eq!(err.field_path, "chain");
        // 未知 type(R1)
        let err = migrate_item(item("uid: x\ntype: banana\nname: X\nfile: x.yaml\n")).unwrap_err();
        assert_eq!(err.field_path, "type");
        // 路径穿越(R3)
        let err =
            migrate_item(item("uid: l1\ntype: local\nname: L\nfile: ../up.yaml\n")).unwrap_err();
        assert_eq!(err.field_path, "file");
        // chain 与 chains 同存(R8)
        let err = migrate_item(item(
            "uid: l1\ntype: local\nname: L\nfile: l1.yaml\nchain: []\nchains: []\n",
        ))
        .unwrap_err();
        assert_eq!(err.field_path, "chain");
    }

    #[test]
    fn top_level_current_forms() {
        // 缺失/[] → 无 current 键
        let out = migrate_clean_schema(item("items: []\n")).unwrap();
        assert!(!out.contains_key("current"));
        let out = migrate_clean_schema(item("current: []\nitems: []\n")).unwrap();
        assert!(!out.contains_key("current"));

        // 标量与单元素(rev1 前旧数据可能是标量,防御性支持)
        let doc = "current: l1\nitems:\n- {uid: l1, type: local, name: L, file: l1.yaml}\n";
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["current"], Value::from("l1"));
        let doc = "current: [l1]\nitems:\n- {uid: l1, type: local, name: L, file: l1.yaml}\n";
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["current"], Value::from("l1"));
    }

    #[test]
    fn multi_current_synthesizes_composition() {
        let doc = r#"current: [a, b, c]
items:
- {uid: a, type: local, name: A, file: a.yaml}
- {uid: b, type: remote, name: B, file: b.yaml, url: "https://e.com"}
- {uid: c, type: local, name: C, file: c.yaml}
"#;
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["current"], Value::from("combined-profile"));
        let items = out["items"].as_sequence().unwrap();
        let combined = items
            .iter()
            .find(|i| i["uid"] == Value::from("combined-profile"))
            .expect("synthesized composition present");
        assert_eq!(combined["name"], Value::from("Combined Profile"));
        yaml_eq(
            combined["config"].as_mapping().unwrap(),
            "type: composition\nbase: a\nextend_proxies_from: [b, c]\n",
        );
    }

    #[test]
    fn combined_profile_uid_is_collision_safe_and_deterministic() {
        let doc = r#"current: [a, b]
items:
- {uid: a, type: local, name: A, file: a.yaml}
- {uid: b, type: local, name: B, file: b.yaml}
- {uid: combined-profile, type: local, name: Taken, file: t.yaml}
"#;
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["current"], Value::from("combined-profile-2"));
    }

    #[test]
    fn multi_current_with_transform_member_fails() {
        let doc = r#"current: [a, s]
items:
- {uid: a, type: local, name: A, file: a.yaml}
- {uid: s, type: script, name: S, file: s.js, script_type: javascript}
"#;
        let err = migrate_clean_schema(item(doc)).unwrap_err();
        assert_eq!(err.uid.as_deref(), Some("s"));
        assert_eq!(err.field_path, "current");
    }

    #[test]
    fn chain_becomes_global_transforms_and_unknown_top_level_fails() {
        let doc = r#"chain: [t1]
items:
- {uid: t1, type: merge, name: T, file: t1.yaml}
"#;
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["global_transforms"], item("x: [t1]")["x"]);

        let err = migrate_clean_schema(item("bogus: 1\nitems: []\n")).unwrap_err();
        assert_eq!(err.field_path, "bogus");
        assert!(err.uid.is_none());
    }

    #[test]
    fn valid_carries_verbatim() {
        let doc = "valid: [dns, tun]\nitems: []\n";
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["valid"], item("x: [dns, tun]")["x"]);
    }

    fn temp_ctx() -> (tempfile::TempDir, Ctx) {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let ctx = Ctx::new(config, data);
        (temp, ctx)
    }

    #[test]
    fn clean_schema_run_writes_bak_and_validated_new_schema() {
        let (_temp, mut ctx) = temp_ctx();
        let path = ctx.profiles_path();
        std::fs::write(&path, MIGRATED_SAMPLE).unwrap();

        run_clean_schema(&mut ctx).unwrap();

        // .bak = 迁移前原文
        let bak = std::fs::read_to_string(path.with_extension("yaml.bak")).unwrap();
        assert_eq!(bak, MIGRATED_SAMPLE);

        // 输出带头注释,且能被新类型加载并通过 validate
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.starts_with("# Profiles Config for Clash Nyanpasu\n\n"));
        let profiles: nyanpasu_config::profile::Profiles = serde_yaml::from_str(&raw).unwrap();
        profiles.validate().unwrap();
        assert_eq!(profiles.items.len(), 7);
        assert!(profiles.current.is_some());

        // 幂等重入:再跑一遍 no-op,内容不变
        run_clean_schema(&mut ctx).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
    }

    #[test]
    fn clean_schema_with_null_current_baselines_to_head_and_is_noop() {
        let (_temp, mut ctx) = temp_ctx();
        let path = ctx.profiles_path();
        let clean = r#"current: null
items:
  - uid: l1
    name: L
    type: config
    config:
      type: file
      source:
        type: local
        binding:
          type: managed
          file: l1.yaml
"#;
        std::fs::write(&path, clean).unwrap();

        assert_eq!(MIGRATOR.detect_baseline(&ctx).unwrap(), current_revision());
        run_clean_schema(&mut ctx).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), clean);
    }

    #[test]
    fn clean_schema_run_failure_leaves_file_untouched() {
        let (_temp, mut ctx) = temp_ctx();
        let path = ctx.profiles_path();
        // 引用不存在 transform 的 chain → validate 失败
        let bad = "chain: [ghost]\nitems: []\n";
        std::fs::write(&path, bad).unwrap();
        assert!(run_clean_schema(&mut ctx).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
    }

    #[test]
    fn clean_schema_run_rejects_duplicate_item_uid() {
        let (_temp, mut ctx) = temp_ctx();
        let path = ctx.profiles_path();
        let duplicate = r#"items:
  - uid: same
    type: local
    name: A
    file: a.yaml
  - uid: same
    type: local
    name: B
    file: b.yaml
"#;
        std::fs::write(&path, duplicate).unwrap();

        let err = run_clean_schema(&mut ctx).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("clean-schema output rejected by domain model"));
        assert!(message.contains("duplicate profile id: same"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), duplicate);
    }

    #[test]
    fn clean_schema_rollback_restores_bak() {
        let (_temp, mut ctx) = temp_ctx();
        let path = ctx.profiles_path();
        std::fs::write(&path, MIGRATED_SAMPLE).unwrap();
        run_clean_schema(&mut ctx).unwrap();
        rollback_clean_schema(&mut ctx).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), MIGRATED_SAMPLE);
    }

    #[test]
    fn test_migrate_existing_data() {
        let original_data = serde_yaml::from_str::<Mapping>(ORIGINAL_SAMPLE).unwrap();
        let migrated_data = migrate_profile_data(original_data);
        let output_data = serde_yaml::to_string(&migrated_data).unwrap();
        assert_str_eq!(output_data, MIGRATED_SAMPLE);
    }

    #[test]
    fn test_discard_existing_data() {
        let migrated_data = serde_yaml::from_str::<Mapping>(MIGRATED_SAMPLE).unwrap();
        let original_data = discard_profile_data(migrated_data);
        let output_data = serde_yaml::to_string(&original_data).unwrap();
        assert_str_eq!(output_data, ORIGINAL_SAMPLE);
    }

    #[test]
    #[ignore]
    fn test_profile_parse_migrated_data() {
        let profiles = serde_yaml::from_str::<Profiles>(MIGRATED_SAMPLE).unwrap();
        eprintln!("{profiles:#?}");
    }
}
