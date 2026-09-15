use super::super::{Ctx, MigrationStep, ModuleMigrator};
use crate::core::storage::{Storage, WebStorage};
use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml::Mapping;

pub static MIGRATOR: StorageMigrator = StorageMigrator;

static VERSION_2_0_0: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());
static HOTKEYS_TO_KV: MigrateHotkeysToKv = MigrateHotkeysToKv;
static HOTKEYS_TO_TYPED_CONFIG: MigrateHotkeysToTypedConfig = MigrateHotkeysToTypedConfig;
static STEPS: [&dyn MigrationStep; 2] = [&HOTKEYS_TO_KV, &HOTKEYS_TO_TYPED_CONFIG];

const HOTKEYS_KEY: &str = "hotkeys";

pub struct StorageMigrator;

impl ModuleMigrator for StorageMigrator {
    fn module(&self) -> &'static str {
        "storage"
    }

    fn detect_baseline(&self, ctx: &Ctx) -> anyhow::Result<u64> {
        let config_path = ctx.nyanpasu_config_path();
        if config_path.exists() {
            let raw = std::fs::read_to_string(&config_path)?;
            let config: Mapping = serde_yaml::from_str(&raw)
                .map_err(|e| anyhow::anyhow!("failed to parse config: {e}"))?;
            if config
                .get(HOTKEYS_KEY)
                .is_some_and(|value| value.as_sequence().is_some())
            {
                return Ok(0);
            }
        }

        // Hotkeys still sitting in the key-value store means only the first
        // step ever ran on this installation.
        if read_kv_hotkeys(ctx)?.is_some() {
            return Ok(1);
        }
        Ok(current_revision())
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

/// Hotkeys belong to the typed application config again.
///
/// The key-value store has no namespace, no version and no compare-and-swap,
/// so a hotkey list living there cannot take part in the revision and
/// degradation protocol the other application settings now use. This walks the
/// value back out of storage and into `application.yaml`, which is the single
/// authority from here on.
#[derive(Debug, Clone, Copy)]
pub struct MigrateHotkeysToTypedConfig;

impl MigrationStep for MigrateHotkeysToTypedConfig {
    fn id(&self) -> &'static str {
        "storage/hotkeys_to_typed_config"
    }

    fn module(&self) -> &'static str {
        "storage"
    }

    fn revision(&self) -> u64 {
        2
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateHotkeysToTypedConfig"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let Some(hotkeys) = read_kv_hotkeys(ctx)? else {
            return Ok(());
        };

        let application_path = ctx.application_config_path();
        // `typed_config/split_legacy_config` builds this file and runs before
        // this module. Failing loudly keeps the value in storage so the next
        // launch retries, instead of dropping the user's hotkeys.
        if !application_path.exists() {
            anyhow::bail!("cannot move hotkeys into the typed application config before it exists");
        }

        let raw = std::fs::read_to_string(&application_path)?;
        let mut application: Mapping = serde_yaml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("failed to parse the application config: {e}"))?;
        // While this key exists the key-value store is the authority: it was
        // the only place the hotkey editor wrote to and the only place startup
        // read from, so whatever the typed config holds was never the user's
        // choice. That includes an empty list, which means every binding was
        // cleared on purpose and must not be resurrected.
        application.insert(
            serde_yaml::Value::String(HOTKEYS_KEY.to_string()),
            serde_yaml::Value::Sequence(
                hotkeys
                    .iter()
                    .map(|hotkey| serde_yaml::Value::String(hotkey.clone()))
                    .collect(),
            ),
        );
        let serialized = serde_yaml::to_string(&application)
            .map_err(|e| anyhow::anyhow!("failed to serialize the application config: {e}"))?;
        crate::core::migration::fs::atomic_write(&application_path, serialized.as_bytes())?;
        tracing::info!(
            "moved {} hotkeys from KV storage into the typed application config",
            hotkeys.len()
        );

        let storage = open_storage(ctx)?.expect("storage was readable a moment ago");
        storage
            .remove_item(HOTKEYS_KEY)
            .map_err(|e| anyhow::anyhow!("failed to drop the migrated hotkeys: {e}"))
    }
}

/// `None` when there is no store yet or the key is absent, which is what makes
/// the step idempotent.
fn read_kv_hotkeys(ctx: &Ctx) -> anyhow::Result<Option<Vec<String>>> {
    let Some(storage) = open_storage(ctx)? else {
        return Ok(None);
    };
    storage
        .get_item::<Vec<String>>(HOTKEYS_KEY)
        .map_err(|e| anyhow::anyhow!("failed to read hotkeys from storage: {e}"))
}

fn open_storage(ctx: &Ctx) -> anyhow::Result<Option<Storage>> {
    let storage_path = ctx.storage_path();
    if !storage_path.exists() {
        return Ok(None);
    }
    Storage::try_new(&storage_path)
        .map(Some)
        .map_err(|e| anyhow::anyhow!("failed to open storage: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _temp: tempfile::TempDir,
        ctx: Ctx,
    }

    impl Fixture {
        /// Everything lives under a `TempDir`, so no test ever reaches a real
        /// application directory.
        fn new(application_yaml: Option<&str>, kv_hotkeys: Option<&[&str]>) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let config_dir = temp.path().join("config");
            let data_dir = temp.path().join("data");
            std::fs::create_dir_all(&config_dir).unwrap();
            std::fs::create_dir_all(&data_dir).unwrap();
            let ctx = Ctx::new(config_dir, data_dir);

            if let Some(yaml) = application_yaml {
                std::fs::write(ctx.application_config_path(), yaml).unwrap();
            }
            if let Some(hotkeys) = kv_hotkeys {
                let storage = Storage::try_new(&ctx.storage_path()).unwrap();
                let hotkeys: Vec<String> = hotkeys.iter().map(ToString::to_string).collect();
                storage.set_item(HOTKEYS_KEY, &hotkeys).unwrap();
            }

            Self { _temp: temp, ctx }
        }

        fn run(&mut self) -> anyhow::Result<()> {
            MigrateHotkeysToTypedConfig.run(&mut self.ctx)
        }

        fn typed_hotkeys(&self) -> Vec<String> {
            let raw = std::fs::read_to_string(self.ctx.application_config_path()).unwrap();
            let config: Mapping = serde_yaml::from_str(&raw).unwrap();
            config
                .get(serde_yaml::Value::String(HOTKEYS_KEY.to_string()))
                .and_then(|value| value.as_sequence())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(ToString::to_string))
                        .collect()
                })
                .unwrap_or_default()
        }

        fn kv_hotkeys(&self) -> Option<Vec<String>> {
            read_kv_hotkeys(&self.ctx).unwrap()
        }
    }

    #[test]
    fn migrates_kv_hotkeys_into_typed_config() {
        let mut fixture = Fixture::new(
            Some("language: en\n"),
            Some(&["clash_mode_rule,Control+Q", "toggle_tun_mode,Control+T"]),
        );

        fixture.run().unwrap();

        assert_eq!(
            fixture.typed_hotkeys(),
            vec![
                "clash_mode_rule,Control+Q".to_string(),
                "toggle_tun_mode,Control+T".to_string()
            ]
        );
        assert_eq!(fixture.kv_hotkeys(), None);
        let raw = std::fs::read_to_string(fixture.ctx.application_config_path()).unwrap();
        let config: Mapping = serde_yaml::from_str(&raw).unwrap();
        assert_eq!(
            config
                .get(serde_yaml::Value::String("language".into()))
                .and_then(|value| value.as_str()),
            Some("en"),
            "the rest of the application config must survive untouched"
        );
    }

    #[test]
    fn is_idempotent_when_kv_key_absent() {
        let mut fixture = Fixture::new(Some("language: en\n"), None);

        fixture.run().unwrap();
        assert!(fixture.typed_hotkeys().is_empty());

        // A store that exists but holds no hotkeys is the state a second run
        // leaves behind, and must also be a no-op.
        let storage = Storage::try_new(&fixture.ctx.storage_path()).unwrap();
        storage.set_item("unrelated", &"value".to_string()).unwrap();
        drop(storage);

        fixture.run().unwrap();
        assert!(fixture.typed_hotkeys().is_empty());
        assert_eq!(fixture.kv_hotkeys(), None);
    }

    #[test]
    fn kv_value_replaces_typed_hotkeys() {
        let mut fixture = Fixture::new(
            Some("hotkeys:\n  - clash_mode_rule,Control+Q\n"),
            Some(&["toggle_tun_mode,Control+T"]),
        );

        fixture.run().unwrap();

        assert_eq!(
            fixture.typed_hotkeys(),
            vec!["toggle_tun_mode,Control+T".to_string()],
            "the key-value store held the only list the user could still edit"
        );
        assert_eq!(fixture.kv_hotkeys(), None);
    }

    #[test]
    fn explicitly_empty_kv_list_clears_typed_hotkeys() {
        let mut fixture =
            Fixture::new(Some("hotkeys:\n  - clash_mode_rule,Control+Q\n"), Some(&[]));

        fixture.run().unwrap();

        assert!(
            fixture.typed_hotkeys().is_empty(),
            "an empty stored list is a user who cleared every binding, not a missing value"
        );
        assert_eq!(fixture.kv_hotkeys(), None);
    }

    #[test]
    fn refuses_to_drop_hotkeys_before_the_typed_config_exists() {
        let mut fixture = Fixture::new(None, Some(&["clash_mode_rule,Control+Q"]));

        fixture
            .run()
            .expect_err("without a typed config there is nowhere to put the hotkeys");

        assert_eq!(
            fixture.kv_hotkeys(),
            Some(vec!["clash_mode_rule,Control+Q".to_string()]),
            "a failed step must leave the value in place so the next launch retries"
        );
    }
}
