use super::super::{
    Ctx, DocumentSpec, MigrationStep, ModuleKind, ModuleMigrator,
    fs::{read_document, write_document},
};
use crate::format::{StampedDocument, StampedYamlFormat};
use anyhow::{Context as _, bail};
use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml_ng::{Mapping, Value};

pub static MIGRATOR: ApplicationMigrator = ApplicationMigrator;

static VERSION_2_0_0: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());
static WINDOW_CLOSE: MigrateWindowClose = MigrateWindowClose;
static STEPS: [&dyn MigrationStep; 1] = [&WINDOW_CLOSE];

/// Revisions up to here were written without a stamp. Frozen: nothing before
/// this module changed the file's schema, so there is no shape to probe.
const UNSTAMPED_CEILING: u64 = 0;

/// `application.yaml`, stamped with the schema revision of its content.
pub struct ApplicationDocument;

impl StampedDocument for ApplicationDocument {
    const DOCUMENT: &'static str = "application";
    const SCHEMA_REVISION: u64 = 1;
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

    fn detect_baseline(&self, _ctx: &Ctx) -> anyhow::Result<u64> {
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

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        let path = ctx.application_config_path();
        let file = read_document(&path, ApplicationDocument::DOCUMENT)?
            .context("application.yaml is gone")?;
        let mut payload = file.payload;
        window_close_from_tray_menu(&mut payload)?;
        write_document(
            &path,
            ApplicationDocument::DOCUMENT,
            self.revision(),
            payload,
            None,
        )
    }
}

/// `hide` keeps hiding and `close` becomes `destroy`; every other window kind
/// follows the global default, which is to destroy.
fn window_close_from_tray_menu(payload: &mut Mapping) -> anyhow::Result<()> {
    let Some(behavior) = payload.remove(TRAY_MENU_CLOSE_BEHAVIOR_KEY) else {
        return Ok(());
    };
    let tray_menu = match behavior.as_str() {
        Some("hide") => "hide",
        Some("close") => "destroy",
        _ => bail!("`{TRAY_MENU_CLOSE_BEHAVIOR_KEY}` is {behavior:?}, not `hide` or `close`"),
    };
    let window_close = payload
        .entry(Value::from(WINDOW_CLOSE_KEY))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    let Some(window_close) = window_close.as_mapping_mut() else {
        bail!("`{WINDOW_CLOSE_KEY}` is not a mapping");
    };
    window_close
        .entry(Value::from("tray_menu"))
        .or_insert_with(|| Value::from(tray_menu));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(src: &str) -> Mapping {
        serde_yaml_ng::from_str(src).unwrap()
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
    fn an_unknown_behavior_is_refused() {
        let mut config = payload("tray_menu_close_behavior: minimize\n");

        assert!(window_close_from_tray_menu(&mut config).is_err());
    }
}
