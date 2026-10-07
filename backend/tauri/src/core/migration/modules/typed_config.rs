use super::{
    super::{
        Ctx, MigrationCheckError, MigrationStep, ModuleMigrator, StepCheck,
        fs::try_exists,
        legacy_schema::{IClashTemp, IVerge, typed_config_from_legacy_parts},
    },
    application::ApplicationFormat,
    clash_config::{self, ClashConfigFormat},
};
use crate::utils::help;
use anyhow::Context as _;
use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, state::PersistentState,
};
use nyanpasu_core::format::Format as _;
use once_cell::sync::Lazy;
use semver::Version;
use serde::{Serialize, de::DeserializeOwned};
use serde_yaml::{Mapping, Value};
use std::path::Path;

pub static MIGRATOR: TypedConfigMigrator = TypedConfigMigrator;

static VERSION_2_0_0: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());
static SPLIT_LEGACY_CONFIG: SplitLegacyConfig = SplitLegacyConfig;
static REPAIR_CLASH_CONFIG_PATH: RepairClashConfigPath = RepairClashConfigPath;
static STEPS: [&dyn MigrationStep; 2] = [&SPLIT_LEGACY_CONFIG, &REPAIR_CLASH_CONFIG_PATH];

const PREVIOUS_TYPED_CLASH_FILE: &str = "clash.yaml";

pub struct TypedConfigMigrator;

impl ModuleMigrator for TypedConfigMigrator {
    fn module(&self) -> &'static str {
        "typed_config"
    }

    fn detect_baseline(&self, ctx: &Ctx) -> anyhow::Result<u64> {
        match typed_file_state(ctx)? {
            TypedFileState::All => Ok(current_revision()),
            TypedFileState::None => Ok(0),
            TypedFileState::NeedsClashRepair => Ok(1),
        }
    }

    fn steps(&self) -> &'static [&'static dyn MigrationStep] {
        &STEPS
    }

    fn files_behind(&self, ctx: &Ctx, applied: u64) -> anyhow::Result<Option<String>> {
        Ok(match typed_file_state(ctx)? {
            TypedFileState::All => None,
            TypedFileState::None if applied >= SPLIT_LEGACY_CONFIG.revision() => Some(format!(
                "application.yaml and session-state.yaml are missing from {}",
                ctx.paths().app_config_dir()
            )),
            TypedFileState::NeedsClashRepair if applied >= REPAIR_CLASH_CONFIG_PATH.revision() => {
                Some(format!(
                    "{} is not a typed clash config",
                    ctx.clash_config_path().display()
                ))
            }
            TypedFileState::None | TypedFileState::NeedsClashRepair => None,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SplitLegacyConfig;

impl MigrationStep for SplitLegacyConfig {
    fn id(&self) -> &'static str {
        "typed_config/split_legacy_config"
    }

    fn module(&self) -> &'static str {
        "typed_config"
    }

    fn revision(&self) -> u64 {
        1
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "SplitLegacyConfig"
    }

    fn check(&self, ctx: &Ctx) -> Result<Option<StepCheck>, MigrationCheckError> {
        Ok(Some(match typed_file_state(ctx)? {
            TypedFileState::None => StepCheck::Needed,
            TypedFileState::All | TypedFileState::NeedsClashRepair => StepCheck::Satisfied,
        }))
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let (application, session_state, clash_config) = if has_legacy_inputs(ctx)? {
            let legacy = read_legacy_verge(&ctx.nyanpasu_config_path())?;
            let legacy_clash = read_legacy_clash_inputs(ctx)?;
            typed_config_from_legacy_parts(&legacy, &legacy_clash)?
        } else {
            (
                NyanpasuAppConfig::default(),
                PersistentState::default(),
                ClashConfig::default(),
            )
        };

        let application_yaml = serialize_application(&application)
            .context("failed to serialize migrated application config")?;
        let session_yaml =
            serialize_yaml(&session_state).context("failed to serialize migrated session state")?;
        let clash_yaml =
            serialize_clash(&clash_config).context("failed to serialize migrated clash config")?;

        write_typed_files(ctx, &application_yaml, &session_yaml, &clash_yaml)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RepairClashConfigPath;

impl MigrationStep for RepairClashConfigPath {
    fn id(&self) -> &'static str {
        "typed_config/repair_clash_config_path"
    }

    fn module(&self) -> &'static str {
        "typed_config"
    }

    fn revision(&self) -> u64 {
        2
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "RepairClashConfigPath"
    }

    fn check(&self, ctx: &Ctx) -> Result<Option<StepCheck>, MigrationCheckError> {
        match typed_file_state(ctx)? {
            TypedFileState::All => Ok(Some(StepCheck::Satisfied)),
            TypedFileState::NeedsClashRepair => Ok(Some(StepCheck::Needed)),
            TypedFileState::None => Err(MigrationCheckError::Unrecognized(
                "cannot repair typed clash config before split_legacy_config has completed"
                    .to_string(),
            )),
        }
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let previous_typed_path = ctx.paths().app_config_dir().join(PREVIOUS_TYPED_CLASH_FILE);
        let clash_config = if previous_typed_path.exists() {
            clash_config::read_typed(previous_typed_path.as_std_path())
                .context("failed to read previous typed clash config")?
        } else if has_legacy_inputs(ctx)? {
            let legacy = read_legacy_verge(&ctx.nyanpasu_config_path())?;
            let legacy_clash = read_legacy_clash_inputs(ctx)?;
            let (_, _, clash_config) = typed_config_from_legacy_parts(&legacy, &legacy_clash)?;
            clash_config
        } else {
            ClashConfig::default()
        };

        let clash_yaml =
            serialize_clash(&clash_config).context("failed to serialize repaired clash config")?;
        crate::core::migration::fs::atomic_write(&ctx.clash_config_path(), clash_yaml.as_bytes())
            .context("failed to write repaired clash config")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypedFileState {
    All,
    None,
    NeedsClashRepair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SharedClashFileState {
    Missing,
    Typed,
    LegacyRuntime,
    Unrecognized,
}

fn typed_file_state(ctx: &Ctx) -> Result<TypedFileState, MigrationCheckError> {
    let application_exists = try_exists(&ctx.application_config_path())?;
    let session_exists = try_exists(&ctx.session_state_path())?;
    let clash_state = classify_shared_clash_file(ctx)?;

    if !application_exists && !session_exists {
        return match clash_state {
            SharedClashFileState::Missing => Ok(TypedFileState::None),
            SharedClashFileState::LegacyRuntime => Ok(TypedFileState::None),
            SharedClashFileState::Typed => partial_typed_file_state(
                vec!["clash-config.yaml"],
                vec!["application.yaml", "session-state.yaml"],
            ),
            SharedClashFileState::Unrecognized => Err(unrecognized_clash_file(ctx)),
        };
    }

    if application_exists && session_exists {
        match clash_state {
            SharedClashFileState::Typed => {
                validate_existing_typed_files(ctx)?;
                return Ok(TypedFileState::All);
            }
            SharedClashFileState::Missing | SharedClashFileState::LegacyRuntime => {
                validate_existing_application_and_session(ctx)?;
                return Ok(TypedFileState::NeedsClashRepair);
            }
            SharedClashFileState::Unrecognized => {}
        }
    }

    if clash_state == SharedClashFileState::Unrecognized {
        return Err(unrecognized_clash_file(ctx));
    }

    let mut existing = Vec::new();
    let mut missing = Vec::new();
    push_file_state(
        &mut existing,
        &mut missing,
        application_exists,
        "application.yaml",
    );
    push_file_state(
        &mut existing,
        &mut missing,
        session_exists,
        "session-state.yaml",
    );
    match clash_state {
        SharedClashFileState::Typed => existing.push("clash-config.yaml"),
        SharedClashFileState::Missing | SharedClashFileState::LegacyRuntime => {
            missing.push("clash-config.yaml")
        }
        SharedClashFileState::Unrecognized => unreachable!("handled above"),
    }

    partial_typed_file_state(existing, missing)
}

fn unrecognized_clash_file(ctx: &Ctx) -> MigrationCheckError {
    MigrationCheckError::Unrecognized(format!(
        "unrecognized typed config migration state: existing {} is neither \
         a valid typed clash config nor a recognized legacy runtime config; restore or \
         remove it before retrying",
        ctx.clash_config_path().display()
    ))
}

fn classify_shared_clash_file(ctx: &Ctx) -> Result<SharedClashFileState, MigrationCheckError> {
    let path = ctx.clash_config_path();
    if !try_exists(&path)? {
        return Ok(SharedClashFileState::Missing);
    }

    if clash_config::read_typed(&path).is_ok() {
        return Ok(SharedClashFileState::Typed);
    }

    let value: Value = read_yaml(&path)?;

    if looks_like_legacy_runtime_clash_mapping(&value) {
        return Ok(SharedClashFileState::LegacyRuntime);
    }

    Ok(SharedClashFileState::Unrecognized)
}

fn looks_like_legacy_runtime_clash_mapping(value: &Value) -> bool {
    let Some(map) = value.as_mapping() else {
        return false;
    };

    [
        "mixed-port",
        "port",
        "socks-port",
        "redir-port",
        "tproxy-port",
        "external-controller",
        "allow-lan",
        "log-level",
        "mode",
        "ipv6",
        "dns",
        "tun",
        "listeners",
        "proxies",
        "proxy-groups",
        "rules",
        "proxy-providers",
        "rule-providers",
    ]
    .iter()
    .any(|key| map.contains_key(Value::String((*key).to_string())))
}

fn push_file_state(
    existing: &mut Vec<&'static str>,
    missing: &mut Vec<&'static str>,
    exists: bool,
    name: &'static str,
) {
    if exists {
        existing.push(name);
    } else {
        missing.push(name);
    }
}

fn partial_typed_file_state(
    existing: Vec<&'static str>,
    missing: Vec<&'static str>,
) -> Result<TypedFileState, MigrationCheckError> {
    Err(MigrationCheckError::Unrecognized(format!(
        "partial typed config migration state: existing [{}], missing [{}]; \
         restore or remove the typed config files before retrying",
        existing.join(", "),
        missing.join(", ")
    )))
}

fn validate_existing_typed_files(ctx: &Ctx) -> Result<(), MigrationCheckError> {
    validate_existing_application_and_session(ctx)?;
    clash_config::read_typed(&ctx.clash_config_path())?;
    Ok(())
}

fn validate_existing_application_and_session(ctx: &Ctx) -> Result<(), MigrationCheckError> {
    read_yaml::<nyanpasu_config::application::NyanpasuAppConfig>(&ctx.application_config_path())?;
    read_yaml::<nyanpasu_config::state::PersistentState>(&ctx.session_state_path())?;
    Ok(())
}

/// Whether any legacy file is left to convert. Without one this is a fresh
/// install, which starts from the typed defaults: the legacy templates only
/// describe what the legacy app assumed for fields its files left out.
fn has_legacy_inputs(ctx: &Ctx) -> Result<bool, MigrationCheckError> {
    Ok(try_exists(&ctx.nyanpasu_config_path())?
        || try_exists(&ctx.clash_guard_overrides_path())?
        || classify_shared_clash_file(ctx)? == SharedClashFileState::LegacyRuntime)
}

fn read_legacy_verge(path: &Path) -> anyhow::Result<IVerge> {
    let mut merged = IVerge::template();
    if !path.exists() {
        return Ok(merged);
    }

    let mut legacy: IVerge = read_yaml(path)
        .with_context(|| format!("failed to read legacy config {}", path.display()))?;
    legacy.migrate_auto_close_connection();
    merged.patch_config(legacy);
    Ok(merged)
}

fn read_legacy_clash_inputs(ctx: &Ctx) -> anyhow::Result<Mapping> {
    let mut merged = IClashTemp::template().0;

    if classify_shared_clash_file(ctx)? == SharedClashFileState::LegacyRuntime {
        merge_legacy_clash_file(&mut merged, &ctx.clash_config_path())?;
    }
    merge_legacy_clash_file(&mut merged, &ctx.clash_guard_overrides_path())?;

    Ok(merged)
}

fn merge_legacy_clash_file(merged: &mut Mapping, path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let legacy = help::read_merge_mapping(&path.to_path_buf())
        .with_context(|| format!("failed to read legacy clash overrides {}", path.display()))?;
    for (key, value) in legacy {
        if !matches!(value, Value::Null) {
            merged.insert(key, value);
        }
    }
    Ok(())
}

fn read_yaml<T: DeserializeOwned>(path: &Path) -> Result<T, MigrationCheckError> {
    let raw = std::fs::read_to_string(path).map_err(|source| MigrationCheckError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    serde_yaml::from_str(&raw).map_err(|source| MigrationCheckError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn serialize_yaml<T: Serialize>(value: &T) -> anyhow::Result<String> {
    serde_yaml::to_string(value).map_err(Into::into)
}

/// `application.yaml` is written stamped from the start, at the revision its
/// own module is at: the app refuses an unstamped file, and an unstamped one
/// at that revision would look like a lost stamp if the run stopped before the
/// runner adopted it.
fn serialize_application(application: &NyanpasuAppConfig) -> anyhow::Result<String> {
    let mut content = Vec::new();
    ApplicationFormat::default().serialize(&mut content, application, None)?;
    Ok(String::from_utf8(content)?)
}

/// `clash-config.yaml` is written stamped for the same reason as
/// `application.yaml`.
fn serialize_clash(clash: &ClashConfig) -> anyhow::Result<String> {
    let mut content = Vec::new();
    ClashConfigFormat::default().serialize(&mut content, clash, None)?;
    Ok(String::from_utf8(content)?)
}

fn write_typed_files(
    ctx: &Ctx,
    application_yaml: &str,
    session_yaml: &str,
    clash_yaml: &str,
) -> anyhow::Result<()> {
    let outputs = [
        (
            "application config",
            ctx.application_config_path(),
            application_yaml.as_bytes(),
        ),
        (
            "session state",
            ctx.session_state_path(),
            session_yaml.as_bytes(),
        ),
        (
            "clash config",
            ctx.clash_config_path(),
            clash_yaml.as_bytes(),
        ),
    ];
    let mut written = Vec::new();

    for (name, path, contents) in outputs {
        if let Err(error) = crate::core::migration::fs::atomic_write(&path, contents) {
            for created in written.iter().rev() {
                let _ = std::fs::remove_file(created);
            }
            return Err(error).with_context(|| format!("failed to write migrated {name}"));
        }
        written.push(path);
    }

    Ok(())
}

fn current_revision() -> u64 {
    STEPS.last().map(|step| step.revision()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::migration::legacy_schema::WindowState as LegacyWindowState;
    use nyanpasu_config::state::window::{WindowLabel, WindowState};

    fn test_ctx() -> (Ctx, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        (Ctx::new(config_dir, data_dir), temp)
    }

    fn write_yaml<T: Serialize>(path: &Path, value: &T) {
        let raw = serde_yaml::to_string(value).unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, raw).unwrap();
    }

    fn read_typed<T: DeserializeOwned>(path: &Path) -> T {
        let raw = std::fs::read_to_string(path).unwrap();
        serde_yaml::from_str(&raw).unwrap()
    }

    fn write_legacy_clash(path: &Path) {
        let mut legacy = IClashTemp::template().0;
        legacy.insert("mixed-port".into(), 8123.into());
        legacy.insert("external-controller".into(), "127.0.0.1:19090".into());
        legacy.insert("allow-lan".into(), true.into());
        legacy.insert("mode".into(), "global".into());
        legacy.insert("ipv6".into(), true.into());
        write_yaml(path, &legacy);
    }

    /// Every default mints its own controller secret, so it is left out.
    fn without_secret(clash: &ClashConfig) -> Value {
        let mut value = serde_yaml::to_value(clash).unwrap();
        value["overrides"]
            .as_mapping_mut()
            .unwrap()
            .remove("secret");
        value
    }

    #[test]
    fn split_legacy_config_on_fresh_install_writes_the_typed_defaults() {
        let (mut ctx, _temp) = test_ctx();

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let application: NyanpasuAppConfig = read_typed(&ctx.application_config_path());
        let session: PersistentState = read_typed(&ctx.session_state_path());
        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        assert_eq!(
            serde_yaml::to_value(&application).unwrap(),
            serde_yaml::to_value(NyanpasuAppConfig::default()).unwrap()
        );
        assert_eq!(
            serde_yaml::to_value(&session).unwrap(),
            serde_yaml::to_value(PersistentState::default()).unwrap()
        );
        assert_eq!(
            without_secret(&clash),
            without_secret(&ClashConfig::default())
        );
    }

    #[test]
    fn repair_clash_config_path_without_legacy_inputs_writes_the_typed_default() {
        let (mut ctx, _temp) = test_ctx();
        write_yaml(
            &ctx.application_config_path(),
            &NyanpasuAppConfig::default(),
        );
        write_yaml(&ctx.session_state_path(), &PersistentState::default());

        REPAIR_CLASH_CONFIG_PATH.run(&mut ctx).unwrap();

        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        assert_eq!(
            without_secret(&clash),
            without_secret(&ClashConfig::default())
        );
    }

    #[test]
    fn legacy_runtime_clash_config_alone_detects_baseline_zero() {
        let (ctx, _temp) = test_ctx();
        write_legacy_clash(&ctx.clash_config_path());

        assert_eq!(MIGRATOR.detect_baseline(&ctx).unwrap(), 0);
    }

    #[test]
    fn split_legacy_config_accepts_legacy_runtime_clash_config_alone() {
        let (mut ctx, _temp) = test_ctx();
        write_legacy_clash(&ctx.clash_config_path());
        write_yaml(&ctx.nyanpasu_config_path(), &IVerge::template());

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let _: NyanpasuAppConfig = read_typed(&ctx.application_config_path());
        let _: PersistentState = read_typed(&ctx.session_state_path());
        let _: ClashConfig = read_typed(&ctx.clash_config_path());
    }

    #[test]
    fn split_legacy_config_migrates_a_non_random_mixed_port_as_fixed() {
        use nyanpasu_config::clash::config::clash_strategy::{PortStrategy, PortStrategyKind};

        let (mut ctx, _temp) = test_ctx();
        write_yaml(
            &ctx.nyanpasu_config_path(),
            &IVerge {
                enable_random_port: Some(false),
                verge_mixed_port: Some(7891),
                ..IVerge::template()
            },
        );

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        assert_eq!(
            clash.mixed_port,
            PortStrategy {
                kind: PortStrategyKind::Fixed,
                start_port: 7891,
            }
        );
    }

    #[test]
    fn legacy_runtime_with_existing_typed_state_detects_repair_baseline() {
        let (ctx, _temp) = test_ctx();
        write_yaml(
            &ctx.application_config_path(),
            &NyanpasuAppConfig::default(),
        );
        write_yaml(&ctx.session_state_path(), &PersistentState::default());
        write_legacy_clash(&ctx.clash_config_path());

        assert_eq!(MIGRATOR.detect_baseline(&ctx).unwrap(), 1);
    }

    #[test]
    fn repair_clash_config_path_preserves_previous_typed_state() {
        let (mut ctx, _temp) = test_ctx();
        write_yaml(
            &ctx.application_config_path(),
            &NyanpasuAppConfig::default(),
        );
        write_yaml(&ctx.session_state_path(), &PersistentState::default());
        write_legacy_clash(&ctx.clash_config_path());
        let previous = ClashConfig {
            enable_tun_mode: true,
            enable_clash_fields: false,
            ..ClashConfig::default()
        };
        write_yaml(
            ctx.paths()
                .app_config_dir()
                .join(PREVIOUS_TYPED_CLASH_FILE)
                .as_std_path(),
            &previous,
        );

        REPAIR_CLASH_CONFIG_PATH.run(&mut ctx).unwrap();

        let repaired: ClashConfig = read_typed(&ctx.clash_config_path());
        assert!(repaired.enable_tun_mode);
        assert!(!repaired.enable_clash_fields);
    }

    #[test]
    fn repair_clash_config_path_rebuilds_when_previous_typed_file_is_missing() {
        let (mut ctx, _temp) = test_ctx();
        write_yaml(
            &ctx.application_config_path(),
            &NyanpasuAppConfig::default(),
        );
        write_yaml(&ctx.session_state_path(), &PersistentState::default());
        write_yaml(&ctx.nyanpasu_config_path(), &IVerge::template());
        write_legacy_clash(&ctx.clash_config_path());

        REPAIR_CLASH_CONFIG_PATH.run(&mut ctx).unwrap();

        let repaired: ClashConfig = read_typed(&ctx.clash_config_path());
        let overrides = serde_yaml::to_value(&repaired.overrides).unwrap();
        let overrides = overrides.as_mapping().unwrap();
        assert_eq!(overrides.get("allow-lan"), Some(&Value::Bool(true)));
        assert_eq!(overrides.get("mode"), Some(&Value::String("global".into())));
        assert_eq!(overrides.get("ipv6"), Some(&Value::Bool(true)));
    }

    #[test]
    fn typed_clash_config_alone_still_fails_as_partial_typed_state() {
        let (ctx, _temp) = test_ctx();
        write_yaml(&ctx.clash_config_path(), &ClashConfig::default());

        let err = SPLIT_LEGACY_CONFIG.check(&ctx).unwrap_err();

        assert!(
            err.to_string()
                .contains("partial typed config migration state"),
            "{err:#}"
        );
        assert!(err.to_string().contains("existing [clash-config.yaml]"));
    }

    #[test]
    fn all_existing_valid_typed_files_detect_current_baseline() {
        let (ctx, _temp) = test_ctx();
        write_yaml(
            &ctx.application_config_path(),
            &NyanpasuAppConfig::default(),
        );
        write_yaml(&ctx.session_state_path(), &PersistentState::default());
        write_yaml(&ctx.clash_config_path(), &ClashConfig::default());

        assert_eq!(MIGRATOR.detect_baseline(&ctx).unwrap(), current_revision());
    }

    #[test]
    fn unrecognized_clash_config_alone_fails_without_overwrite() {
        let (ctx, _temp) = test_ctx();
        std::fs::write(ctx.clash_config_path(), "unexpected: true\n").unwrap();

        let err = MIGRATOR.detect_baseline(&ctx).unwrap_err();

        assert!(
            err.to_string()
                .contains("unrecognized typed config migration state"),
            "{err:#}"
        );
        assert_eq!(
            std::fs::read_to_string(ctx.clash_config_path()).unwrap(),
            "unexpected: true\n"
        );
    }

    #[test]
    fn typed_config_files_are_created_from_legacy_files() {
        let (mut ctx, _temp) = test_ctx();
        let legacy = IVerge {
            enable_system_proxy: Some(true),
            enable_tun_mode: Some(true),
            verge_mixed_port: Some(7899),
            ..IVerge::template()
        };
        write_yaml(&ctx.nyanpasu_config_path(), &legacy);
        write_legacy_clash(&ctx.clash_guard_overrides_path());

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let application: NyanpasuAppConfig = read_typed(&ctx.application_config_path());
        assert!(application.enable_system_proxy);

        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        assert!(clash.enable_tun_mode);
        assert_eq!(clash.mixed_port.start_port, 7899);
        assert_eq!(clash.external_controller.port.start_port, 19090);

        let session: PersistentState = read_typed(&ctx.session_state_path());
        assert!(session.window_state.is_empty());
    }

    /// `auto_close_connection` predates `break_when_proxy_change`. A document
    /// with only the old field migrates by it, although the template it is
    /// merged onto carries the new one; the new field wins when both are
    /// present, and a document with neither keeps the default.
    #[test]
    fn split_legacy_config_migrates_the_deprecated_proxy_change_policy() {
        use nyanpasu_config::clash::config::clash_strategy::ProxyChangeBreakMode;

        let cases = [
            ("auto_close_connection: false\n", ProxyChangeBreakMode::Off),
            ("auto_close_connection: true\n", ProxyChangeBreakMode::All),
            (
                "auto_close_connection: true\nbreak_when_proxy_change: none\n",
                ProxyChangeBreakMode::Off,
            ),
            ("{}\n", ProxyChangeBreakMode::All),
        ];
        for (verge, expected) in cases {
            let (mut ctx, _temp) = test_ctx();
            std::fs::write(ctx.nyanpasu_config_path(), verge).unwrap();

            SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

            let clash: ClashConfig = read_typed(&ctx.clash_config_path());
            assert_eq!(clash.break_connection.on_proxy_change, expected, "{verge}");
        }
    }

    #[test]
    fn all_existing_valid_typed_files_satisfy_the_split() {
        let (ctx, _temp) = test_ctx();
        write_yaml(
            &ctx.application_config_path(),
            &NyanpasuAppConfig::default(),
        );
        write_yaml(&ctx.session_state_path(), &PersistentState::default());
        write_yaml(&ctx.clash_config_path(), &ClashConfig::default());
        write_yaml(&ctx.nyanpasu_config_path(), &IVerge::template());

        assert_eq!(
            SPLIT_LEGACY_CONFIG.check(&ctx).unwrap(),
            Some(StepCheck::Satisfied)
        );
    }

    #[test]
    fn all_existing_invalid_typed_files_fail_validation() {
        let (ctx, _temp) = test_ctx();
        std::fs::write(ctx.application_config_path(), "application sentinel").unwrap();
        write_yaml(&ctx.session_state_path(), &PersistentState::default());
        write_yaml(&ctx.clash_config_path(), &ClashConfig::default());

        let err = SPLIT_LEGACY_CONFIG.check(&ctx).unwrap_err();

        assert!(
            matches!(
                &err,
                MigrationCheckError::Parse { path, .. } if *path == ctx.application_config_path()
            ),
            "{err:#}"
        );
    }

    #[test]
    fn partial_typed_files_fail_with_clear_error() {
        let (ctx, _temp) = test_ctx();
        std::fs::write(ctx.application_config_path(), "application sentinel").unwrap();

        let err = SPLIT_LEGACY_CONFIG.check(&ctx).unwrap_err();

        assert!(
            err.to_string()
                .contains("partial typed config migration state"),
            "{err:#}"
        );
    }

    #[test]
    fn write_failure_rolls_back_files_created_in_this_run() {
        let (ctx, _temp) = test_ctx();
        std::fs::create_dir_all(ctx.session_state_path()).unwrap();

        let err =
            write_typed_files(&ctx, "app: true\n", "session: true\n", "clash: true\n").unwrap_err();

        assert!(
            err.to_string()
                .contains("failed to write migrated session state"),
            "{err:#}"
        );
        assert!(
            !ctx.application_config_path().exists(),
            "application.yaml created before the failure must be rolled back"
        );
        assert!(ctx.session_state_path().is_dir());
        assert!(
            !ctx.clash_config_path().exists(),
            "files after the failure must not be written"
        );
    }

    #[test]
    fn legacy_window_state_migrates_to_session_state() {
        let (mut ctx, _temp) = test_ctx();
        let legacy = IVerge {
            window_size_state: Some(LegacyWindowState {
                width: 1024,
                height: 768,
                x: 11,
                y: 22,
                maximized: true,
                fullscreen: false,
            }),
            ..IVerge::template()
        };
        write_yaml(&ctx.nyanpasu_config_path(), &legacy);

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let session: PersistentState = read_typed(&ctx.session_state_path());
        let migrated = session
            .window_state
            .get(&WindowLabel("main".into()))
            .expect("main window state should migrate");
        assert_eq!(
            migrated,
            &WindowState {
                width: 1024,
                height: 768,
                x: 11,
                y: 22,
                maximized: true,
                fullscreen: false,
            }
        );
    }

    #[test]
    fn legacy_window_position_fallback_migrates_to_session_state() {
        let (mut ctx, _temp) = test_ctx();
        #[allow(deprecated)]
        let legacy = IVerge {
            window_size_position: Some(vec![900.0, 700.0, 30.0, 40.0]),
            ..IVerge::template()
        };
        write_yaml(&ctx.nyanpasu_config_path(), &legacy);

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let session: PersistentState = read_typed(&ctx.session_state_path());
        let migrated = session
            .window_state
            .get(&WindowLabel("main".into()))
            .expect("main window state should migrate");
        assert_eq!(migrated.width, 900);
        assert_eq!(migrated.height, 700);
        assert_eq!(migrated.x, 30);
        assert_eq!(migrated.y, 40);
        assert!(!migrated.maximized);
        assert!(!migrated.fullscreen);
    }

    #[test]
    fn legacy_guard_overrides_preserve_override_fields() {
        let (mut ctx, _temp) = test_ctx();
        write_yaml(&ctx.nyanpasu_config_path(), &IVerge::template());
        std::fs::write(
            ctx.clash_guard_overrides_path(),
            "ipv6: true\nallow-lan: true\nmode: global\nlog-level: debug\n",
        )
        .unwrap();

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        let overrides = serde_yaml::to_value(&clash.overrides).unwrap();
        let overrides = overrides.as_mapping().unwrap();
        assert_eq!(overrides.get("ipv6"), Some(&Value::Bool(true)));
        assert_eq!(overrides.get("allow-lan"), Some(&Value::Bool(true)));
        assert_eq!(overrides.get("mode"), Some(&Value::String("global".into())));
        assert_eq!(
            overrides.get("log-level"),
            Some(&Value::String("debug".into()))
        );
    }

    #[test]
    fn legacy_runtime_clash_config_alone_preserves_override_fields() {
        let (mut ctx, _temp) = test_ctx();
        write_yaml(&ctx.nyanpasu_config_path(), &IVerge::template());
        write_legacy_clash(&ctx.clash_config_path());

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        let overrides = serde_yaml::to_value(&clash.overrides).unwrap();
        let overrides = overrides.as_mapping().unwrap();
        assert_eq!(overrides.get("ipv6"), Some(&Value::Bool(true)));
        assert_eq!(overrides.get("allow-lan"), Some(&Value::Bool(true)));
        assert_eq!(overrides.get("mode"), Some(&Value::String("global".into())));
        assert_eq!(
            overrides.get("log-level"),
            Some(&Value::String("info".into()))
        );
    }

    #[test]
    fn legacy_guard_overrides_take_precedence_over_runtime_clash_config() {
        let (mut ctx, _temp) = test_ctx();
        write_yaml(&ctx.nyanpasu_config_path(), &IVerge::template());
        let mut runtime = IClashTemp::template().0;
        runtime.insert("ipv6".into(), false.into());
        runtime.insert("allow-lan".into(), false.into());
        runtime.insert("mode".into(), "direct".into());
        runtime.insert("log-level".into(), "warning".into());
        write_yaml(&ctx.clash_config_path(), &runtime);
        std::fs::write(
            ctx.clash_guard_overrides_path(),
            "ipv6: true\nallow-lan: true\nmode: global\nlog-level: debug\n",
        )
        .unwrap();

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        let overrides = serde_yaml::to_value(&clash.overrides).unwrap();
        let overrides = overrides.as_mapping().unwrap();
        assert_eq!(overrides.get("ipv6"), Some(&Value::Bool(true)));
        assert_eq!(overrides.get("allow-lan"), Some(&Value::Bool(true)));
        assert_eq!(overrides.get("mode"), Some(&Value::String("global".into())));
        assert_eq!(
            overrides.get("log-level"),
            Some(&Value::String("debug".into()))
        );
    }

    #[test]
    fn legacy_clash_overrides_seed_modeled_clash_config() {
        let (mut ctx, _temp) = test_ctx();
        write_yaml(&ctx.nyanpasu_config_path(), &IVerge::template());
        write_legacy_clash(&ctx.clash_guard_overrides_path());

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        let overrides = serde_yaml::to_value(&clash.overrides).unwrap();
        let overrides = overrides.as_mapping().unwrap();
        assert_eq!(clash.mixed_port.start_port, 7890);
        assert_eq!(clash.external_controller.host.to_string(), "127.0.0.1");
        assert_eq!(clash.external_controller.port.start_port, 19090);
        assert_eq!(overrides.get("ipv6"), Some(&Value::Bool(true)));
    }

    #[test]
    fn legacy_clash_null_overrides_keep_defaults() {
        let (mut ctx, _temp) = test_ctx();
        write_yaml(&ctx.nyanpasu_config_path(), &IVerge::template());
        std::fs::write(
            ctx.clash_guard_overrides_path(),
            "mode:\nlog-level:\nallow-lan:\nipv6:\n",
        )
        .unwrap();

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        let overrides = serde_yaml::to_value(&clash.overrides).unwrap();
        let overrides = overrides.as_mapping().unwrap();
        assert_eq!(overrides.get("mode"), Some(&Value::String("rule".into())));
        assert_eq!(
            overrides.get("log-level"),
            Some(&Value::String("info".into()))
        );
        assert_eq!(overrides.get("allow-lan"), Some(&Value::Bool(false)));
        assert_eq!(overrides.get("ipv6"), Some(&Value::Bool(false)));
    }
}
