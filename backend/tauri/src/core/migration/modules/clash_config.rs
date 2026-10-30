use super::super::{
    Ctx, DocumentSpec, MigrationCheckError, MigrationStep, ModuleKind, ModuleMigrator,
    fs::{read_document, write_document},
};
use nyanpasu_config::clash::config::ClashConfig;
use nyanpasu_core::format::{Inspected, StampedDocument, StampedYamlFormat};
use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml::{Mapping, Value};
use snafu::OptionExt as _;
use std::path::Path;

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(context(false), display("failed to inspect migration input"))]
    InspectInput {
        source: super::super::MigrationCheckError,
    },
    #[snafu(context(false), display("failed to write migration output"))]
    WriteOutput { source: super::super::fs::Error },
    #[snafu(display("migration document is missing: {}", path.display()))]
    FindDocument { path: std::path::PathBuf },
    #[snafu(display("expected mapping in {field}"))]
    ReadMapping { field: &'static str },
    #[snafu(display("expected boolean in {field}: {value:?}"))]
    ReadGuard { field: &'static str, value: Value },
}

pub static MIGRATOR: ClashConfigMigrator = ClashConfigMigrator;

static VERSION_2_0_0: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());
static MANAGEABLE_GUARD_FIELDS: MigrateManageableGuardFields = MigrateManageableGuardFields;
static TUN_OVERRIDES: MigrateTunOverrides = MigrateTunOverrides;
static STEPS: [&dyn MigrationStep; 2] = [&MANAGEABLE_GUARD_FIELDS, &TUN_OVERRIDES];

/// Revisions up to here were written without a stamp. Frozen: nothing before
/// this module changed the file's schema, so there is no shape to probe.
const UNSTAMPED_CEILING: u64 = 0;

/// `clash-config.yaml`, stamped with the schema revision of its content.
pub struct ClashConfigDocument;

impl StampedDocument for ClashConfigDocument {
    const DOCUMENT: &'static str = "clash_config";
    const SCHEMA_REVISION: u64 = 2;
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

    fn detect_baseline(&self, _ctx: &Ctx) -> Result<u64, super::super::OperationError> {
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
        manage_guard_fields(&mut payload).map_err(|source| {
            MigrationCheckError::MigrateClashOverrides {
                path: path.to_owned(),
                source: Box::new(source),
            }
        })?;
    }
    if revision < TUN_OVERRIDES.revision() {
        add_tun_overrides(&mut payload);
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

    fn run(&self, ctx: &mut Ctx) -> Result<(), super::super::OperationError> {
        (|| -> Result<(), Error> {
            let path = ctx.clash_config_path();
            let file = read_document(&path, ClashConfigDocument::DOCUMENT)?
                .context(FindDocumentSnafu { path: &path })?;
            let mut payload = file.payload;
            manage_guard_fields(&mut payload)?;
            write_document(
                &path,
                ClashConfigDocument::DOCUMENT,
                self.revision(),
                payload,
                None,
            )
            .map_err(Into::into)
        })()
        .map_err(Into::into)
    }
}

fn manage_guard_fields(payload: &mut Mapping) -> Result<(), Error> {
    let Some(overrides) = payload.get_mut(OVERRIDES_KEY) else {
        return Ok(());
    };
    let Some(overrides) = overrides.as_mapping_mut() else {
        return ReadMappingSnafu {
            field: OVERRIDES_KEY,
        }
        .fail();
    };
    manage_override_fields(overrides)
}

/// The conversion of [`MigrateManageableGuardFields`] on the overrides
/// mapping itself, which the legacy conversion shares. A field already in the
/// managed shape is left alone: `typed_config` may have written the file at
/// head before this step runs on it.
pub(in crate::core::migration) fn manage_override_fields(
    overrides: &mut Mapping,
) -> Result<(), Error> {
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
            other => {
                return ReadGuardSnafu {
                    field: key,
                    value: other.clone(),
                }
                .fail();
            }
        }
    }
    Ok(())
}

const TUN_KEY: &str = "tun";

/// Adds the `tun` overrides with `auto-route` managed as `true`, the value the
/// runtime config filled in before when the profiles left it out. A profile's
/// own value now needs `auto-route` unmanaged.
#[derive(Debug, Clone, Copy)]
pub struct MigrateTunOverrides;

impl MigrationStep for MigrateTunOverrides {
    fn id(&self) -> &'static str {
        "clash_config/tun_overrides"
    }

    fn module(&self) -> &'static str {
        "clash_config"
    }

    fn revision(&self) -> u64 {
        2
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateTunOverrides"
    }

    fn run(&self, ctx: &mut Ctx) -> Result<(), super::super::OperationError> {
        (|| -> Result<(), Error> {
            let path = ctx.clash_config_path();
            let file = read_document(&path, ClashConfigDocument::DOCUMENT)?
                .context(FindDocumentSnafu { path: &path })?;
            let mut payload = file.payload;
            add_tun_overrides(&mut payload);
            write_document(
                &path,
                ClashConfigDocument::DOCUMENT,
                self.revision(),
                payload,
                None,
            )
            .map_err(Into::into)
        })()
        .map_err(Into::into)
    }
}

/// Leaves an existing `tun` alone: `typed_config` may have written the file at
/// head before this step runs on it.
fn add_tun_overrides(payload: &mut Mapping) {
    if payload.contains_key(TUN_KEY) {
        return;
    }
    let mut auto_route = Mapping::new();
    auto_route.insert("kind".into(), "managed".into());
    auto_route.insert("value".into(), true.into());
    let mut tun = Mapping::new();
    tun.insert("auto-route".into(), Value::Mapping(auto_route));
    payload.insert(TUN_KEY.into(), Value::Mapping(tun));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reading_legacy_clash_overrides_keeps_the_conversion_source() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("clash-config.yaml");
        std::fs::write(&path, "overrides: []\n").unwrap();
        let error = read_typed(&path).unwrap_err();
        assert!(
            matches!(error, MigrationCheckError::MigrateClashOverrides { source, .. } if matches!(source.as_ref(), Error::ReadMapping { field: "overrides" }))
        );
    }

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

    #[test]
    fn tun_overrides_manage_auto_route_as_true() {
        let mut config = payload("enable_tun_mode: true\ntun_stack: system\n");

        add_tun_overrides(&mut config);

        assert_eq!(
            config,
            payload(
                "enable_tun_mode: true\ntun_stack: system\n\
                 tun: {auto-route: {kind: managed, value: true}}\n"
            )
        );
    }

    #[test]
    fn tun_overrides_already_present_are_left_alone() {
        let src = "tun: {auto-route: {kind: unmanaged}}\n";
        let mut config = payload(src);

        add_tun_overrides(&mut config);

        assert_eq!(config, payload(src));
    }
}
