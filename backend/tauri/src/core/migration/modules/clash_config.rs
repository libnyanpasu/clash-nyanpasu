use super::super::{
    Ctx, DocumentSpec, MigrationCheckError, MigrationStep, ModuleKind, ModuleMigrator,
    fs::{read_document, write_document},
};
use anyhow::{Context as _, bail};
use nyanpasu_config::clash::config::ClashConfig;
use nyanpasu_core::format::{Inspected, StampedDocument, StampedYamlFormat};
use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml::{Mapping, Value};
use std::path::Path;

pub static MIGRATOR: ClashConfigMigrator = ClashConfigMigrator;

static VERSION_2_0_0: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());
static MANAGEABLE_GUARD_FIELDS: MigrateManageableGuardFields = MigrateManageableGuardFields;
static STEPS: [&dyn MigrationStep; 1] = [&MANAGEABLE_GUARD_FIELDS];

/// Revisions up to here were written without a stamp. Frozen: nothing before
/// this module changed the file's schema, so there is no shape to probe.
const UNSTAMPED_CEILING: u64 = 0;

/// `clash-config.yaml`, stamped with the schema revision of its content.
pub struct ClashConfigDocument;

impl StampedDocument for ClashConfigDocument {
    const DOCUMENT: &'static str = "clash_config";
    const SCHEMA_REVISION: u64 = 1;
}

/// How the running app reads and writes `clash-config.yaml`.
pub type ClashConfigFormat = StampedYamlFormat<ClashConfigDocument>;

/// The typed clash config. `typed_config` creates it, from the legacy runtime
/// config that used to live at the same path or from the defaults.
pub struct ClashConfigMigrator;

impl ModuleMigrator for ClashConfigMigrator {
    fn module(&self) -> &'static str {
        "clash_config"
    }

    fn kind(&self) -> ModuleKind {
        ModuleKind::Document(DocumentSpec {
            document: ClashConfigDocument::DOCUMENT,
            path: Ctx::clash_config_path,
            unstamped_ceiling: UNSTAMPED_CEILING,
        })
    }

    fn detect_baseline(&self, _ctx: &Ctx) -> anyhow::Result<u64> {
        Ok(UNSTAMPED_CEILING)
    }

    fn steps(&self) -> &'static [&'static dyn MigrationStep] {
        &STEPS
    }
}

/// Reads a typed clash config as this build's [`ClashConfig`], bringing a
/// payload this module has not migrated yet up to its head first.
/// `typed_config` judges `clash-config.yaml` before this module runs, and
/// repairs it from the older `clash.yaml`, so neither may read the file in the
/// current shape alone.
pub(in crate::core::migration) fn read_typed(
    path: &Path,
) -> Result<ClashConfig, MigrationCheckError> {
    let raw = std::fs::read_to_string(path).map_err(|source| MigrationCheckError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let (revision, mut payload) =
        match nyanpasu_core::format::inspect(&raw).map_err(|source| MigrationCheckError::Stamp {
            path: path.to_path_buf(),
            source,
        })? {
            Inspected::Unstamped(payload) => (UNSTAMPED_CEILING, payload),
            Inspected::Stamped { stamp, payload } => (stamp.schema_revision, payload),
        };
    if revision < MANAGEABLE_GUARD_FIELDS.revision() {
        manage_guard_fields(&mut payload).map_err(|error| {
            MigrationCheckError::Unrecognized(format!("{}: {error:#}", path.display()))
        })?;
    }
    serde_yaml::from_value(Value::Mapping(payload)).map_err(|source| MigrationCheckError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

const OVERRIDES_KEY: &str = "overrides";
const MANAGEABLE_GUARD_KEYS: [&str; 2] = ["unified-delay", "tcp-concurrent"];

/// Turns the boolean `unified-delay` and `tcp-concurrent` overrides into
/// managed fields holding the same value, so the runtime config keeps what
/// it had. Either can then be left to the profiles.
#[derive(Debug, Clone, Copy)]
pub struct MigrateManageableGuardFields;

impl MigrationStep for MigrateManageableGuardFields {
    fn id(&self) -> &'static str {
        "clash_config/manageable_guard_fields"
    }

    fn module(&self) -> &'static str {
        "clash_config"
    }

    fn revision(&self) -> u64 {
        1
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateManageableGuardFields"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let path = ctx.clash_config_path();
        let file = read_document(&path, ClashConfigDocument::DOCUMENT)?
            .context("clash-config.yaml is gone")?;
        let mut payload = file.payload;
        manage_guard_fields(&mut payload)?;
        write_document(
            &path,
            ClashConfigDocument::DOCUMENT,
            self.revision(),
            payload,
            None,
        )
    }
}

fn manage_guard_fields(payload: &mut Mapping) -> anyhow::Result<()> {
    let Some(overrides) = payload.get_mut(OVERRIDES_KEY) else {
        return Ok(());
    };
    let Some(overrides) = overrides.as_mapping_mut() else {
        bail!("`{OVERRIDES_KEY}` is not a mapping");
    };
    manage_override_fields(overrides)
}

/// The conversion of [`MigrateManageableGuardFields`] on the overrides
/// mapping itself, which the legacy conversion shares. A field already in the
/// managed shape is left alone: `typed_config` may have written the file at
/// head before this step runs on it.
pub(in crate::core::migration) fn manage_override_fields(
    overrides: &mut Mapping,
) -> anyhow::Result<()> {
    for key in MANAGEABLE_GUARD_KEYS {
        let Some(value) = overrides.get_mut(key) else {
            continue;
        };
        match value {
            Value::Bool(enabled) => {
                let mut managed = Mapping::new();
                managed.insert("kind".into(), "managed".into());
                managed.insert("value".into(), (*enabled).into());
                *value = Value::Mapping(managed);
            }
            Value::Mapping(_) => {}
            other => bail!("`{key}` is {other:?}, not a boolean"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(src: &str) -> Mapping {
        serde_yaml::from_str(src).unwrap()
    }

    #[test]
    fn booleans_become_managed_with_the_same_value() {
        let mut config =
            payload("overrides:\n  mode: rule\n  unified-delay: true\n  tcp-concurrent: false\n");

        manage_guard_fields(&mut config).unwrap();

        assert_eq!(
            config,
            payload(
                "overrides:\n  mode: rule\n  unified-delay: {kind: managed, value: true}\n  \
                 tcp-concurrent: {kind: managed, value: false}\n"
            )
        );
    }

    #[test]
    fn fields_already_in_the_managed_shape_are_left_alone() {
        let src = "overrides:\n  unified-delay: {kind: unmanaged}\n  \
                   tcp-concurrent: {kind: managed, value: true}\n";
        let mut config = payload(src);

        manage_guard_fields(&mut config).unwrap();

        assert_eq!(config, payload(src));
    }

    #[test]
    fn a_value_of_another_type_is_refused() {
        let mut config = payload("overrides:\n  tcp-concurrent: yes please\n");

        assert!(manage_guard_fields(&mut config).is_err());
    }
}
