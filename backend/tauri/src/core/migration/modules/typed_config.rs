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
use serde_yaml::Mapping;
use std::path::{Path, PathBuf};
use struct_patch::Patch;

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
        let legacy_clash = read_legacy_clash(&ctx.clash_guard_overrides_path())?;
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

fn typed_file_state(ctx: &Ctx) -> anyhow::Result<TypedFileState> {
    let paths = typed_paths(ctx);
    let mut existing = Vec::new();
    let mut missing = Vec::new();

    for (name, path) in &paths {
        if path
            .try_exists()
            .with_context(|| format!("failed to inspect typed config file {}", path.display()))?
        {
            existing.push(*name);
        } else {
            missing.push(*name);
        }
    }

    if existing.is_empty() {
        return Ok(TypedFileState::None);
    }

    if missing.is_empty() {
        validate_existing_typed_files(ctx)?;
        return Ok(TypedFileState::All);
    }

    bail!(
        "partial typed config migration state: existing [{}], missing [{}]; \
         restore or remove the typed config files before retrying",
        existing.join(", "),
        missing.join(", ")
    );
}

fn typed_paths(ctx: &Ctx) -> [(&'static str, PathBuf); 3] {
    [
        ("application.yaml", ctx.application_config_path()),
        ("session-state.yaml", ctx.session_state_path()),
        ("clash-config.yaml", ctx.clash_config_path()),
    ]
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

fn read_legacy_clash(path: &Path) -> anyhow::Result<Mapping> {
    let mut merged = IClashTemp::template().0;
    if !path.exists() {
        return Ok(merged);
    }

    let legacy = help::read_merge_mapping(&path.to_path_buf())
        .with_context(|| format!("failed to read legacy clash overrides {}", path.display()))?;
    for (key, value) in legacy {
        merged.insert(key, value);
    }
    Ok(merged)
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
    fn legacy_clash_overrides_seed_modeled_clash_config() {
        let (mut ctx, _temp) = test_ctx();
        write_yaml(&ctx.nyanpasu_config_path(), &IVerge::template());
        write_legacy_clash(&ctx.clash_guard_overrides_path());

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        assert_eq!(clash.mixed_port.start_port, 7890);
        assert_eq!(clash.external_controller.host.to_string(), "127.0.0.1");
        assert_eq!(clash.external_controller.port.start_port, 19090);
    }
}
