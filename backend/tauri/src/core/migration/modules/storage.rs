use super::super::{Ctx, MigrationStep, ModuleMigrator};
use crate::core::storage::{Storage, WebStorage};
use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml::Mapping;

pub static MIGRATOR: StorageMigrator = StorageMigrator;

static VERSION_2_0_0: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());
static HOTKEYS_TO_KV: MigrateHotkeysToKv = MigrateHotkeysToKv;
static STEPS: [&dyn MigrationStep; 1] = [&HOTKEYS_TO_KV];

const HOTKEYS_KEY: &str = "hotkeys";

pub struct StorageMigrator;

impl ModuleMigrator for StorageMigrator {
    fn module(&self) -> &'static str {
        "storage"
    }

    fn detect_baseline(&self, ctx: &Ctx) -> anyhow::Result<u64> {
        let config_path = ctx.nyanpasu_config_path();
        if !config_path.exists() {
            return Ok(current_revision());
        }

        let raw = std::fs::read_to_string(&config_path)?;
        let config: Mapping = serde_yaml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("failed to parse config: {e}"))?;
        if config
            .get(HOTKEYS_KEY)
            .is_some_and(|value| value.as_sequence().is_some())
        {
            Ok(0)
        } else {
            Ok(current_revision())
        }
    }

    fn steps(&self) -> &'static [&'static dyn MigrationStep] {
        &STEPS
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateHotkeysToKv;

impl MigrationStep for MigrateHotkeysToKv {
    fn id(&self) -> &'static str {
        "storage/hotkeys_to_kv"
    }

    fn module(&self) -> &'static str {
        "storage"
    }

    fn revision(&self) -> u64 {
        1
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateHotkeysToKv"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let config_path = ctx.nyanpasu_config_path();

        if !config_path.exists() {
            return Ok(());
        }

        let raw = std::fs::read_to_string(&config_path)?;
        let mut config: Mapping = serde_yaml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("failed to parse config: {e}"))?;

        let hotkeys_key = serde_yaml::Value::String(HOTKEYS_KEY.to_string());
        let Some(hotkeys_value) = config.get(&hotkeys_key).cloned() else {
            return Ok(());
        };
        let Some(hotkeys) = hotkeys_value.as_sequence() else {
            return Ok(());
        };

        let hotkey_strings: Vec<String> = hotkeys
            .iter()
            .filter_map(|value| value.as_str().map(ToString::to_string))
            .collect();

        if !hotkey_strings.is_empty() {
            let storage_path = ctx.storage_path();
            if let Some(parent) = storage_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let storage = Storage::try_new(&storage_path)
                .map_err(|e| anyhow::anyhow!("failed to open storage: {e}"))?;

            storage
                .set_item("hotkeys", &hotkey_strings)
                .map_err(|e| anyhow::anyhow!("failed to save hotkeys: {e}"))?;

            // Note: registration is intentionally NOT done here. This migration
            // runs in a separate `migrate` subprocess with no Tauri app handle, so
            // `Hotkey::update` would fail; `Hotkey::init` reads the migrated value
            // from KV storage at app startup instead.
            tracing::info!("migrated {} hotkeys to KV storage", hotkey_strings.len());
        }

        config.remove(&hotkeys_key);
        let new_config = serde_yaml::to_string(&config)
            .map_err(|e| anyhow::anyhow!("failed to serialize config: {e}"))?;
        crate::core::migration::fs::atomic_write(&config_path, new_config.as_bytes())?;

        Ok(())
    }
}

fn current_revision() -> u64 {
    STEPS.last().map(|step| step.revision()).unwrap_or_default()
}
