use super::super::{
    Ctx, DocumentSpec, MigrationCheckError, MigrationStep, ModuleKind, ModuleMigrator,
    fs::read_document,
};
use nyanpasu_config::clash::config::ClashConfig;
use nyanpasu_persistence::{
    format::{Inspected, StampedDocument, StampedYamlFormat},
    write,
};
use once_cell::sync::Lazy;
use semver::Version;
use snafu::OptionExt as _;
use std::path::Path;
use yaml_serde::{Mapping, Value};

#[derive(Debug, snafu::Snafu)]
pub enum Error {
    #[snafu(context(false), display("failed to inspect migration input"))]
    InspectInput {
        source: super::super::MigrationCheckError,
    },
    #[snafu(context(false), display("failed to write migration output"))]
    WriteOutput { source: write::Error },
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
static TUN_MIPS: MigrateTunMips = MigrateTunMips;
static SCRIPT_MODE: MigrateScriptMode = MigrateScriptMode;
static DNS_CONTROL: MigrateDnsControl = MigrateDnsControl;
static MANAGEABLE_ACCESS_FIELDS: MigrateManageableAccessFields = MigrateManageableAccessFields;
static STEPS: [&dyn MigrationStep; 6] = [
    &MANAGEABLE_GUARD_FIELDS,
    &TUN_OVERRIDES,
    &TUN_MIPS,
    &SCRIPT_MODE,
    &DNS_CONTROL,
    &MANAGEABLE_ACCESS_FIELDS,
];

/// Revisions up to here were written without a stamp. Frozen: nothing before
/// this module changed the file's schema, so there is no shape to probe.
const UNSTAMPED_CEILING: u64 = 0;

/// `clash-config.yaml`, stamped with the schema revision of its content.
pub struct ClashConfigDocument;

impl StampedDocument for ClashConfigDocument {
    const DOCUMENT: &'static str = "clash_config";
    const SCHEMA_REVISION: u64 = 6;
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
pub(crate) fn read_typed(path: &Path) -> Result<ClashConfig, MigrationCheckError> {
    let raw = std::fs::read_to_string(path).map_err(|source| MigrationCheckError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let (revision, mut payload) =
        match nyanpasu_persistence::format::inspect(&raw).map_err(|source| {
            MigrationCheckError::Stamp {
                path: path.to_path_buf(),
                source,
            }
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
    if revision < TUN_MIPS.revision() {
        add_tun_mips(&mut payload);
    }
    if revision < SCRIPT_MODE.revision() {
        normalize_script_mode(&mut payload);
    }
    if revision < DNS_CONTROL.revision() {
        add_dns_control(&mut payload);
    }
    if revision < MANAGEABLE_ACCESS_FIELDS.revision() {
        manage_access_fields(&mut payload).map_err(|source| {
            MigrationCheckError::MigrateClashOverrides {
                path: path.to_owned(),
                source: Box::new(source),
            }
        })?;
    }
    yaml_serde::from_value(Value::Mapping(payload)).map_err(|source| MigrationCheckError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

const OVERRIDES_KEY: &str = "overrides";
pub(crate) const MANAGEABLE_GUARD_KEYS: [&str; 2] = ["unified-delay", "tcp-concurrent"];

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
            write::write_document(
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
    manage_override_fields(overrides, &MANAGEABLE_GUARD_KEYS)
}

/// The conversion of [`MigrateManageableGuardFields`] and
/// [`MigrateManageableAccessFields`] on the overrides mapping itself, which the
/// legacy conversion shares. A field already in the managed shape is left
/// alone: `typed_config` may have written the file at head before a step runs
/// on it.
pub(crate) fn manage_override_fields(
    overrides: &mut Mapping,
    keys: &[&'static str],
) -> Result<(), Error> {
    for &key in keys {
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
            write::write_document(
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

const TUN_MIPS_KEY: &str = "tun_mips";

/// Adds the mips stack options, empty, so that the core keeps its own
/// defaults. The data needs no conversion; the revision exists so that a build
/// from before the mips stack refuses the file instead of failing on
/// `tun_stack: mips` or dropping `tun_mips` when it writes the file back.
#[derive(Debug, Clone, Copy)]
pub struct MigrateTunMips;

impl MigrationStep for MigrateTunMips {
    fn id(&self) -> &'static str {
        "clash_config/tun_mips"
    }

    fn module(&self) -> &'static str {
        "clash_config"
    }

    fn revision(&self) -> u64 {
        3
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateTunMips"
    }

    fn run(&self, ctx: &mut Ctx) -> Result<(), super::super::OperationError> {
        (|| -> Result<(), Error> {
            let path = ctx.clash_config_path();
            let file = read_document(&path, ClashConfigDocument::DOCUMENT)?
                .context(FindDocumentSnafu { path: &path })?;
            let mut payload = file.payload;
            add_tun_mips(&mut payload);
            write::write_document(
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

/// Leaves existing options alone: `typed_config` may have written the file at
/// head before this step runs on it.
fn add_tun_mips(payload: &mut Mapping) {
    if !payload.contains_key(TUN_MIPS_KEY) {
        payload.insert(TUN_MIPS_KEY.into(), Value::Mapping(Mapping::new()));
    }
}

/// Replaces the removed Script proxy mode with rule mode.
#[derive(Debug, Clone, Copy)]
pub struct MigrateScriptMode;

impl MigrationStep for MigrateScriptMode {
    fn id(&self) -> &'static str {
        "clash_config/script_mode"
    }

    fn module(&self) -> &'static str {
        "clash_config"
    }

    fn revision(&self) -> u64 {
        4
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateScriptMode"
    }

    fn run(&self, ctx: &mut Ctx) -> Result<(), super::super::OperationError> {
        (|| -> Result<(), Error> {
            let path = ctx.clash_config_path();
            let file = read_document(&path, ClashConfigDocument::DOCUMENT)?
                .context(FindDocumentSnafu { path: &path })?;
            let mut payload = file.payload;
            normalize_script_mode(&mut payload);
            write::write_document(
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

fn normalize_script_mode(payload: &mut Mapping) {
    if let Some(overrides) = payload
        .get_mut(OVERRIDES_KEY)
        .and_then(Value::as_mapping_mut)
    {
        normalize_override_mode(overrides);
    }
}

/// Normalizes the removed mode before typed or legacy overrides are decoded.
pub(crate) fn normalize_override_mode(overrides: &mut Mapping) {
    if let Some(mode) = overrides.get_mut("mode")
        && mode.as_str() == Some("script")
    {
        *mode = "rule".into();
    }
}

const DNS_CONTROL_KEYS: [&str; 2] = ["dns_control", "hosts_control"];

/// Records that the profiles keep controlling `dns` and `hosts`, as they did
/// before Nyanpasu had DNS settings; the settings themselves load at their
/// defaults. The revision exists so that a build from before the settings
/// refuses the file instead of dropping them when it writes the file back.
#[derive(Debug, Clone, Copy)]
pub struct MigrateDnsControl;

impl MigrationStep for MigrateDnsControl {
    fn id(&self) -> &'static str {
        "clash_config/dns_control"
    }

    fn module(&self) -> &'static str {
        "clash_config"
    }

    fn revision(&self) -> u64 {
        5
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateDnsControl"
    }

    fn run(&self, ctx: &mut Ctx) -> Result<(), super::super::OperationError> {
        (|| -> Result<(), Error> {
            let path = ctx.clash_config_path();
            let file = read_document(&path, ClashConfigDocument::DOCUMENT)?
                .context(FindDocumentSnafu { path: &path })?;
            let mut payload = file.payload;
            add_dns_control(&mut payload);
            write::write_document(
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

/// Leaves an existing choice alone: `typed_config` may have written the file
/// at head before this step runs on it.
fn add_dns_control(payload: &mut Mapping) {
    for key in DNS_CONTROL_KEYS {
        if !payload.contains_key(key) {
            payload.insert(key.into(), "profile".into());
        }
    }
}

pub(crate) const MANAGEABLE_ACCESS_KEYS: [&str; 2] = ["allow-lan", "ipv6"];

/// Turns the boolean `allow-lan` and `ipv6` overrides into managed fields
/// holding the same value, so the runtime config keeps what it had. Either can
/// then be left to the profiles.
#[derive(Debug, Clone, Copy)]
pub struct MigrateManageableAccessFields;

impl MigrationStep for MigrateManageableAccessFields {
    fn id(&self) -> &'static str {
        "clash_config/manageable_access_fields"
    }

    fn module(&self) -> &'static str {
        "clash_config"
    }

    fn revision(&self) -> u64 {
        6
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateManageableAccessFields"
    }

    fn run(&self, ctx: &mut Ctx) -> Result<(), super::super::OperationError> {
        (|| -> Result<(), Error> {
            let path = ctx.clash_config_path();
            let file = read_document(&path, ClashConfigDocument::DOCUMENT)?
                .context(FindDocumentSnafu { path: &path })?;
            let mut payload = file.payload;
            manage_access_fields(&mut payload)?;
            write::write_document(
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

fn manage_access_fields(payload: &mut Mapping) -> Result<(), Error> {
    let Some(overrides) = payload.get_mut(OVERRIDES_KEY) else {
        return Ok(());
    };
    let Some(overrides) = overrides.as_mapping_mut() else {
        return ReadMappingSnafu {
            field: OVERRIDES_KEY,
        }
        .fail();
    };
    manage_override_fields(overrides, &MANAGEABLE_ACCESS_KEYS)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::fake_hotkey_store::FakeHotkeyStoreOpener;

    use super::*;

    #[test]
    fn script_mode_step_converts_and_stamps_even_without_the_old_mode() {
        for (source, expected) in [
            (
                "overrides: {mode: script, ipv6: true}\n",
                "overrides: {mode: rule, ipv6: true}\n",
            ),
            ("overrides: {mode: global}\n", "overrides: {mode: global}\n"),
            ("overrides: {}\n", "overrides: {}\n"),
            ("tun_mips: {}\n", "tun_mips: {}\n"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut ctx = Ctx::new(
                root.path().into(),
                root.path().into(),
                Arc::new(FakeHotkeyStoreOpener::default()),
            );
            write::write_document(
                &ctx.clash_config_path(),
                ClashConfigDocument::DOCUMENT,
                3,
                payload(source),
                None,
            )
            .unwrap();

            SCRIPT_MODE.run(&mut ctx).unwrap();

            let file = read_document(&ctx.clash_config_path(), ClashConfigDocument::DOCUMENT)
                .unwrap()
                .unwrap();
            assert_eq!(file.schema_revision, Some(4));
            assert_eq!(file.payload, payload(expected));
            SCRIPT_MODE.run(&mut ctx).unwrap();
            assert_eq!(
                read_document(&ctx.clash_config_path(), ClashConfigDocument::DOCUMENT)
                    .unwrap()
                    .unwrap()
                    .payload,
                payload(expected)
            );
        }
    }

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
        yaml_serde::from_str(src).unwrap()
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
    fn access_booleans_become_managed_with_the_same_value() {
        let mut config = payload(
            "overrides:\n  allow-lan: false\n  ipv6: true\n  \
             unified-delay: {kind: unmanaged}\n",
        );

        manage_access_fields(&mut config).unwrap();

        assert_eq!(
            config,
            payload(
                "overrides:\n  allow-lan: {kind: managed, value: false}\n  \
                 ipv6: {kind: managed, value: true}\n  unified-delay: {kind: unmanaged}\n"
            )
        );
    }

    #[test]
    fn access_fields_already_in_the_managed_shape_are_left_alone() {
        let src = "overrides:\n  allow-lan: {kind: unmanaged}\n  \
                   ipv6: {kind: managed, value: false}\n";
        let mut config = payload(src);

        manage_access_fields(&mut config).unwrap();

        assert_eq!(config, payload(src));
    }

    #[test]
    fn reading_a_revision_5_file_manages_the_access_fields() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("clash-config.yaml");
        let Value::Mapping(mut revision_5) = yaml_serde::to_value(ClashConfig::default()).unwrap()
        else {
            unreachable!("the clash config serializes as a mapping");
        };
        let overrides = revision_5
            .get_mut(OVERRIDES_KEY)
            .and_then(Value::as_mapping_mut)
            .unwrap();
        overrides.insert("allow-lan".into(), true.into());
        overrides.insert("ipv6".into(), false.into());
        write::write_document(&path, ClashConfigDocument::DOCUMENT, 5, revision_5, None).unwrap();

        let config = read_typed(&path).unwrap();
        let overrides = serde_json::to_value(&config.overrides).unwrap();

        assert_eq!(
            overrides["allow-lan"],
            serde_json::json!({"kind": "managed", "value": true})
        );
        assert_eq!(
            overrides["ipv6"],
            serde_json::json!({"kind": "managed", "value": false})
        );
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
    fn tun_mips_options_start_empty_and_existing_ones_are_left_alone() {
        let mut config = payload("tun_stack: gvisor\n");
        add_tun_mips(&mut config);
        assert_eq!(config, payload("tun_stack: gvisor\ntun_mips: {}\n"));
        let options: nyanpasu_config::clash::config::tun_stack::MipsConfig =
            yaml_serde::from_value(config["tun_mips"].clone()).unwrap();
        assert_eq!(options.congestion_controller, None);

        let src = "tun_mips: {congestion_controller: bbr}\n";
        let mut config = payload(src);
        add_tun_mips(&mut config);
        assert_eq!(config, payload(src));
    }

    #[test]
    fn tun_overrides_already_present_are_left_alone() {
        let src = "tun: {auto-route: {kind: unmanaged}}\n";
        let mut config = payload(src);

        add_tun_overrides(&mut config);

        assert_eq!(config, payload(src));
    }
}
