use super::super::{
    Ctx, DocumentSpec, MigrationStep, ModuleKind, ModuleMigrator, fs::read_document,
};
use nyanpasu_persistence::{
    format::{StampedDocument, StampedYamlFormat},
    write,
};
use once_cell::sync::Lazy;
use semver::Version;
use snafu::OptionExt as _;
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
    #[snafu(display("invalid tray close behavior: {value:?}"))]
    ReadCloseBehavior { value: Value },
}

pub static MIGRATOR: ApplicationMigrator = ApplicationMigrator;

static VERSION_2_0_0: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());
static WINDOW_CLOSE: MigrateWindowClose = MigrateWindowClose;
static DNSPOD_CONSENT: MigrateDnspodConsent = MigrateDnspodConsent;
static SCRIPT_HOTKEYS: MigrateScriptHotkeys = MigrateScriptHotkeys;
static PREMIUM_CORE: MigratePremiumCore = MigratePremiumCore;
static STEPS: [&dyn MigrationStep; 4] = [
    &WINDOW_CLOSE,
    &DNSPOD_CONSENT,
    &SCRIPT_HOTKEYS,
    &PREMIUM_CORE,
];

/// Revisions up to here were written without a stamp. Frozen: nothing before
/// this module changed the file's schema, so there is no shape to probe.
const UNSTAMPED_CEILING: u64 = 0;

/// `application.yaml`, stamped with the schema revision of its content.
pub struct ApplicationDocument;

impl StampedDocument for ApplicationDocument {
    const DOCUMENT: &'static str = "application";
    const SCHEMA_REVISION: u64 = 4;
}

/// How the running app reads and writes `application.yaml`.
pub type ApplicationFormat = StampedYamlFormat<ApplicationDocument>;

/// The typed application config. The legacy `nyanpasu-config.yaml` that the
/// `app_config` module edits is only an input of `typed_config`, which creates
/// this file.
pub struct ApplicationMigrator;

impl ModuleMigrator for ApplicationMigrator {
    fn module(&self) -> &'static str {
        "application"
    }

    fn kind(&self) -> ModuleKind {
        ModuleKind::Document(DocumentSpec {
            document: ApplicationDocument::DOCUMENT,
            path: Ctx::application_config_path,
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

const TRAY_MENU_CLOSE_BEHAVIOR_KEY: &str = "tray_menu_close_behavior";
const WINDOW_CLOSE_KEY: &str = "window_close";

/// Replaces the tray menu's own close behavior with the per-window close
/// settings. Stamps the file even when the old key is absent, since every step
/// of a document module leaves its document at its revision.
#[derive(Debug, Clone, Copy)]
pub struct MigrateWindowClose;

impl MigrationStep for MigrateWindowClose {
    fn id(&self) -> &'static str {
        "application/window_close"
    }

    fn module(&self) -> &'static str {
        "application"
    }

    fn revision(&self) -> u64 {
        1
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateWindowClose"
    }

    fn run(&self, ctx: &mut Ctx) -> Result<(), super::super::OperationError> {
        (|| -> Result<(), Error> {
            let path = ctx.application_config_path();
            let file = read_document(&path, ApplicationDocument::DOCUMENT)?
                .context(FindDocumentSnafu { path: &path })?;
            let mut payload = file.payload;
            window_close_from_tray_menu(&mut payload)?;
            write::write_document(
                &path,
                ApplicationDocument::DOCUMENT,
                self.revision(),
                payload,
                None,
            )
            .map_err(Into::into)
        })()
        .map_err(Into::into)
    }
}

/// `hide` keeps hiding and `close` becomes `destroy`; every other window kind
/// follows the global default, which is to destroy.
fn window_close_from_tray_menu(payload: &mut Mapping) -> Result<(), Error> {
    let Some(behavior) = payload.remove(TRAY_MENU_CLOSE_BEHAVIOR_KEY) else {
        return Ok(());
    };
    let tray_menu = match behavior.as_str() {
        Some("hide") => "hide",
        Some("close") => "destroy",
        _ => return ReadCloseBehaviorSnafu { value: behavior }.fail(),
    };
    let window_close = payload
        .entry(Value::from(WINDOW_CLOSE_KEY))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    let Some(window_close) = window_close.as_mapping_mut() else {
        return ReadMappingSnafu {
            field: WINDOW_CLOSE_KEY,
        }
        .fail();
    };
    window_close
        .entry(Value::from("tray_menu"))
        .or_insert_with(|| Value::from(tray_menu));
    Ok(())
}

const ENABLE_LOCAL_IP_PROBE_KEY: &str = "enable_local_ip_probe";
const IP_LOOKUP_PROVIDERS_KEY: &str = "ip_lookup_providers";

/// The local IP probe used to ask DNSPod, and its switch told the user so.
/// Users who turned it on accepted that notice, so DNSPod stays an accepted
/// lookup provider for them now that STUN servers are asked first.
#[derive(Debug, Clone, Copy)]
pub struct MigrateDnspodConsent;

impl MigrationStep for MigrateDnspodConsent {
    fn id(&self) -> &'static str {
        "application/dnspod_consent"
    }

    fn module(&self) -> &'static str {
        "application"
    }

    fn revision(&self) -> u64 {
        2
    }

    // The app version is not bumped until release, so a later version here
    // would leave development builds unable to read their stamped file.
    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateDnspodConsent"
    }

    fn run(&self, ctx: &mut Ctx) -> Result<(), super::super::OperationError> {
        (|| -> Result<(), Error> {
            let path = ctx.application_config_path();
            let file = read_document(&path, ApplicationDocument::DOCUMENT)?
                .context(FindDocumentSnafu { path: &path })?;
            let mut payload = file.payload;
            keep_dnspod_consent(&mut payload);
            write::write_document(
                &path,
                ApplicationDocument::DOCUMENT,
                self.revision(),
                payload,
                None,
            )
            .map_err(Into::into)
        })()
        .map_err(Into::into)
    }
}

fn keep_dnspod_consent(payload: &mut Mapping) {
    let opted_in = payload
        .get(ENABLE_LOCAL_IP_PROBE_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if opted_in && !payload.contains_key(IP_LOOKUP_PROVIDERS_KEY) {
        payload.insert(
            Value::from(IP_LOOKUP_PROVIDERS_KEY),
            Value::Sequence(vec![Value::from("dnspod")]),
        );
    }
}

/// Drops hotkeys for the removed Script proxy mode.
#[derive(Debug, Clone, Copy)]
pub struct MigrateScriptHotkeys;

impl MigrationStep for MigrateScriptHotkeys {
    fn id(&self) -> &'static str {
        "application/script_hotkeys"
    }

    fn module(&self) -> &'static str {
        "application"
    }

    fn revision(&self) -> u64 {
        3
    }

    // The app version is not bumped until release, so a later version here
    // would leave development builds unable to read their stamped file.
    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateScriptHotkeys"
    }

    fn run(&self, ctx: &mut Ctx) -> Result<(), super::super::OperationError> {
        (|| -> Result<(), Error> {
            let path = ctx.application_config_path();
            let file = read_document(&path, ApplicationDocument::DOCUMENT)?
                .context(FindDocumentSnafu { path: &path })?;
            let mut payload = file.payload;
            drop_script_hotkeys(&mut payload);
            write::write_document(
                &path,
                ApplicationDocument::DOCUMENT,
                self.revision(),
                payload,
                None,
            )
            .map_err(Into::into)
        })()
        .map_err(Into::into)
    }
}

fn drop_script_hotkeys(payload: &mut Mapping) {
    if let Some(hotkeys) = payload.get_mut("hotkeys").and_then(Value::as_sequence_mut) {
        hotkeys.retain(|value| !value.as_str().is_some_and(is_script_hotkey));
    }
}

/// Matches the persisted function name exactly, independently of its accelerator.
pub(crate) fn is_script_hotkey(hotkey: &str) -> bool {
    hotkey.split(',').next() == Some("clash_mode_script")
}

/// Replaces stored Clash Premium selections with Mihomo.
#[derive(Debug, Clone, Copy)]
pub struct MigratePremiumCore;

impl MigrationStep for MigratePremiumCore {
    fn id(&self) -> &'static str {
        "application/premium_core"
    }

    fn module(&self) -> &'static str {
        "application"
    }

    fn revision(&self) -> u64 {
        4
    }

    // The app version is not bumped until release, so a later version here
    // would leave development builds unable to read their stamped file.
    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigratePremiumCore"
    }

    fn run(&self, ctx: &mut Ctx) -> Result<(), super::super::OperationError> {
        (|| -> Result<(), Error> {
            let path = ctx.application_config_path();
            let file = read_document(&path, ApplicationDocument::DOCUMENT)?
                .context(FindDocumentSnafu { path: &path })?;
            let mut payload = file.payload;
            normalize_premium_core(&mut payload);
            write::write_document(
                &path,
                ApplicationDocument::DOCUMENT,
                self.revision(),
                payload,
                None,
            )
            .map_err(Into::into)
        })()
        .map_err(Into::into)
    }
}

/// Normalizes before typed-file validation, which runs before document steps.
pub(crate) fn normalize_premium_core(payload: &mut Mapping) {
    if let Some(core) = payload.get_mut("core")
        && matches!(core.as_str(), Some("clash" | "clash-premium"))
    {
        *core = Value::from("mihomo");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::fake_hotkey_store::FakeHotkeyStoreOpener;

    use super::*;

    #[test]
    fn premium_core_step_converts_only_retired_selections_and_always_stamps() {
        for (source, expected) in [
            (
                "core: clash\nlanguage: en\n",
                "core: mihomo\nlanguage: en\n",
            ),
            ("core: clash-premium\n", "core: mihomo\n"),
            ("core: clash-rs\n", "core: clash-rs\n"),
            ("core: unknown\n", "core: unknown\n"),
            ("language: en\n", "language: en\n"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut ctx = Ctx::new(
                root.path().into(),
                root.path().into(),
                Arc::new(FakeHotkeyStoreOpener::default()),
            );
            write::write_document(
                &ctx.application_config_path(),
                ApplicationDocument::DOCUMENT,
                3,
                payload(source),
                None,
            )
            .unwrap();

            for _ in 0..2 {
                PREMIUM_CORE.run(&mut ctx).unwrap();

                let file = read_document(
                    &ctx.application_config_path(),
                    ApplicationDocument::DOCUMENT,
                )
                .unwrap()
                .unwrap();
                assert_eq!(file.schema_revision, Some(4));
                assert_eq!(file.payload, payload(expected));
            }
        }
    }

    #[test]
    fn script_hotkeys_step_drops_only_the_exact_function_and_always_stamps() {
        for (source, expected) in [
            (
                "hotkeys: [clash_mode_script, 'clash_mode_script,Ctrl+S', 'clash_mode_rule,Ctrl+R', 'clash_mode_script_extra,Ctrl+E', ' clash_mode_script,Ctrl+X']\nlanguage: en\n",
                "hotkeys: ['clash_mode_rule,Ctrl+R', 'clash_mode_script_extra,Ctrl+E', ' clash_mode_script,Ctrl+X']\nlanguage: en\n",
            ),
            ("hotkeys: []\n", "hotkeys: []\n"),
            ("language: en\n", "language: en\n"),
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut ctx = Ctx::new(
                root.path().into(),
                root.path().into(),
                Arc::new(FakeHotkeyStoreOpener::default()),
            );
            write::write_document(
                &ctx.application_config_path(),
                ApplicationDocument::DOCUMENT,
                2,
                payload(source),
                None,
            )
            .unwrap();

            SCRIPT_HOTKEYS.run(&mut ctx).unwrap();

            let file = read_document(
                &ctx.application_config_path(),
                ApplicationDocument::DOCUMENT,
            )
            .unwrap()
            .unwrap();
            assert_eq!(file.schema_revision, Some(3));
            assert_eq!(file.payload, payload(expected));
            SCRIPT_HOTKEYS.run(&mut ctx).unwrap();
            assert_eq!(
                read_document(
                    &ctx.application_config_path(),
                    ApplicationDocument::DOCUMENT
                )
                .unwrap()
                .unwrap()
                .payload,
                payload(expected)
            );
        }
    }

    fn payload(src: &str) -> Mapping {
        yaml_serde::from_str(src).unwrap()
    }

    #[test]
    fn hide_stays_hide_and_close_becomes_destroy() {
        for (old, new) in [("hide", "hide"), ("close", "destroy")] {
            let mut config = payload(&format!("language: en\ntray_menu_close_behavior: {old}\n"));

            window_close_from_tray_menu(&mut config).unwrap();

            assert_eq!(
                config,
                payload(&format!(
                    "language: en\nwindow_close:\n  tray_menu: {new}\n"
                ))
            );
        }
    }

    #[test]
    fn a_config_without_the_old_key_is_left_alone() {
        let mut config = payload("language: en\n");

        window_close_from_tray_menu(&mut config).unwrap();

        assert_eq!(config, payload("language: en\n"));
    }

    #[test]
    fn an_opted_in_probe_keeps_dnspod_accepted() {
        let mut config = payload("enable_local_ip_probe: true\n");

        keep_dnspod_consent(&mut config);

        assert_eq!(
            config,
            payload("enable_local_ip_probe: true\nip_lookup_providers:\n- dnspod\n")
        );
    }

    #[test]
    fn a_probe_that_was_off_accepts_no_provider() {
        for src in ["enable_local_ip_probe: false\n", "language: en\n"] {
            let mut config = payload(src);

            keep_dnspod_consent(&mut config);

            assert_eq!(config, payload(src));
        }
    }

    #[test]
    fn an_existing_provider_list_is_left_alone() {
        let src = "enable_local_ip_probe: true\nip_lookup_providers: []\n";
        let mut config = payload(src);

        keep_dnspod_consent(&mut config);

        assert_eq!(config, payload(src));
    }

    #[test]
    fn an_unknown_behavior_is_refused() {
        let mut config = payload("tray_menu_close_behavior: minimize\n");

        assert!(window_close_from_tray_menu(&mut config).is_err());
    }
}
