use super::super::{Ctx, DocumentSpec, MigrationStep, ModuleKind, ModuleMigrator};
use nyanpasu_core::format::{StampedDocument, StampedYamlFormat};

pub static MIGRATOR: ClashConfigMigrator = ClashConfigMigrator;

static STEPS: [&dyn MigrationStep; 0] = [];

/// Revisions up to here were written without a stamp. Frozen: nothing before
/// this module changed the file's schema, so there is no shape to probe.
const UNSTAMPED_CEILING: u64 = 0;

/// `clash-config.yaml`, stamped with the schema revision of its content.
pub struct ClashConfigDocument;

impl StampedDocument for ClashConfigDocument {
    const DOCUMENT: &'static str = "clash_config";
    const SCHEMA_REVISION: u64 = 0;
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
