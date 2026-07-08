use super::super::{Ctx, MigrationStep, ModuleMigrator};
use crate::{
    bridge::typed_config_from_legacy_parts,
    config::{IClashTemp, IVerge},
    utils::help,
};
use anyhow::{Context as _, bail};
use once_cell::sync::Lazy;
use semver::Version;
use serde::{Serialize, de::DeserializeOwned};
use serde_yaml::{Mapping, Value};
use std::path::Path;

pub static MIGRATOR: TypedConfigMigrator = TypedConfigMigrator;

static VERSION_2_0_0: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());
static SPLIT_LEGACY_CONFIG: SplitLegacyConfig = SplitLegacyConfig;
static STEPS: [&dyn MigrationStep; 1] = [&SPLIT_LEGACY_CONFIG];

pub struct TypedConfigMigrator;

impl ModuleMigrator for TypedConfigMigrator {
    fn module(&self) -> &'static str {
        "typed_config"
    }

    fn detect_baseline(&self, ctx: &Ctx) -> anyhow::Result<u64> {
        match typed_file_state(ctx)? {
            TypedFileState::All => Ok(current_revision()),
            TypedFileState::None => Ok(0),
        }
    }

    fn steps(&self) -> &'static [&'static dyn MigrationStep] {
        &STEPS
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

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        match typed_file_state(ctx)? {
            TypedFileState::All => return Ok(()),
            TypedFileState::None => {}
        }

        let legacy = read_legacy_verge(&ctx.nyanpasu_config_path())?;
        let legacy_clash = read_legacy_clash_inputs(ctx)?;
        let (application, session_state, clash_config) =
            typed_config_from_legacy_parts(&legacy, &legacy_clash)?;

        let application_yaml = serialize_yaml(&application)
            .context("failed to serialize migrated application config")?;
        let session_yaml =
            serialize_yaml(&session_state).context("failed to serialize migrated session state")?;
        let clash_yaml =
            serialize_yaml(&clash_config).context("failed to serialize migrated clash config")?;

        write_typed_files(ctx, &application_yaml, &session_yaml, &clash_yaml)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypedFileState {
    All,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SharedClashFileState {
    Missing,
    Typed,
    LegacyRuntime,
    Unrecognized,
}

fn typed_file_state(ctx: &Ctx) -> anyhow::Result<TypedFileState> {
    let application_exists = typed_path_exists(&ctx.application_config_path())?;
    let session_exists = typed_path_exists(&ctx.session_state_path())?;
    let clash_state = classify_shared_clash_file(ctx)?;

    if !application_exists && !session_exists {
        return match clash_state {
            SharedClashFileState::Missing => Ok(TypedFileState::None),
            SharedClashFileState::LegacyRuntime => Ok(TypedFileState::None),
            SharedClashFileState::Typed => partial_typed_file_state(
                vec!["clash-config.yaml"],
                vec!["application.yaml", "session-state.yaml"],
            ),
            SharedClashFileState::Unrecognized => bail!(
                "unrecognized typed config migration state: existing {} is neither \
                 a valid typed clash config nor a recognized legacy runtime config; restore or \
                 remove it before retrying",
                ctx.clash_config_path().display()
            ),
        };
    }

    if application_exists && session_exists && clash_state == SharedClashFileState::Typed {
        validate_existing_typed_files(ctx)?;
        return Ok(TypedFileState::All);
    }

    if clash_state == SharedClashFileState::Unrecognized {
        bail!(
            "unrecognized typed config migration state: existing {} is neither \
             a valid typed clash config nor a recognized legacy runtime config; restore or \
             remove it before retrying",
            ctx.clash_config_path().display()
        );
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

fn typed_path_exists(path: &Path) -> anyhow::Result<bool> {
    path.try_exists()
        .with_context(|| format!("failed to inspect typed config file {}", path.display()))
}

fn classify_shared_clash_file(ctx: &Ctx) -> anyhow::Result<SharedClashFileState> {
    let path = ctx.clash_config_path();
    if !typed_path_exists(&path)? {
        return Ok(SharedClashFileState::Missing);
    }

    if read_yaml::<nyanpasu_config::clash::config::ClashConfig>(&path).is_ok() {
        return Ok(SharedClashFileState::Typed);
    }

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value: Value = serde_yaml::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;

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
    .any(|key| map.contains_key(&Value::String((*key).to_string())))
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
) -> anyhow::Result<TypedFileState> {
    bail!(
        "partial typed config migration state: existing [{}], missing [{}]; \
         restore or remove the typed config files before retrying",
        existing.join(", "),
        missing.join(", ")
    );
}

fn validate_existing_typed_files(ctx: &Ctx) -> anyhow::Result<()> {
    read_yaml::<nyanpasu_config::application::NyanpasuAppConfig>(&ctx.application_config_path())
        .context("failed to validate existing application config")?;
    read_yaml::<nyanpasu_config::state::PersistentState>(&ctx.session_state_path())
        .context("failed to validate existing session state")?;
    read_yaml::<nyanpasu_config::clash::config::ClashConfig>(&ctx.clash_config_path())
        .context("failed to validate existing clash config")?;
    Ok(())
}

fn read_legacy_verge(path: &Path) -> anyhow::Result<IVerge> {
    let mut merged = IVerge::template();
    if !path.exists() {
        return Ok(merged);
    }

    let legacy: IVerge = read_yaml(path)
        .with_context(|| format!("failed to read legacy config {}", path.display()))?;
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

fn read_yaml<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn serialize_yaml<T: Serialize>(value: &T) -> anyhow::Result<String> {
    serde_yaml::to_string(value).map_err(Into::into)
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
    use crate::config::{IClashTemp, nyanpasu::WindowState as LegacyWindowState};
    use nyanpasu_config::{
        application::NyanpasuAppConfig,
        clash::config::ClashConfig,
        state::{
            PersistentState,
            window::{WindowLabel, WindowState},
        },
    };

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
    fn typed_clash_config_alone_still_fails_as_partial_typed_state() {
        let (mut ctx, _temp) = test_ctx();
        write_yaml(&ctx.clash_config_path(), &ClashConfig::default());

        let err = SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap_err();

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

    #[test]
    fn all_existing_valid_typed_files_are_not_overwritten() {
        let (mut ctx, _temp) = test_ctx();
        let application = NyanpasuAppConfig {
            enable_system_proxy: true,
            ..NyanpasuAppConfig::default()
        };
        write_yaml(&ctx.application_config_path(), &application);
        write_yaml(&ctx.session_state_path(), &PersistentState::default());
        write_yaml(&ctx.clash_config_path(), &ClashConfig::default());
        write_yaml(&ctx.nyanpasu_config_path(), &IVerge::template());

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let application: NyanpasuAppConfig = read_typed(&ctx.application_config_path());
        assert!(application.enable_system_proxy);
    }

    #[test]
    fn all_existing_invalid_typed_files_fail_validation() {
        let (mut ctx, _temp) = test_ctx();
        std::fs::write(ctx.application_config_path(), "application sentinel").unwrap();
        write_yaml(&ctx.session_state_path(), &PersistentState::default());
        write_yaml(&ctx.clash_config_path(), &ClashConfig::default());

        let err = SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap_err();

        assert!(
            err.to_string()
                .contains("failed to validate existing application config"),
            "{err:#}"
        );
    }

    #[test]
    fn partial_typed_files_fail_with_clear_error() {
        let (mut ctx, _temp) = test_ctx();
        std::fs::write(ctx.application_config_path(), "application sentinel").unwrap();

        let err = SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap_err();

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
