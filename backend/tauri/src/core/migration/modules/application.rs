use super::super::{Ctx, DocumentSpec, MigrationStep, ModuleKind, ModuleMigrator};
use nyanpasu_core::format::{StampedDocument, StampedYamlFormat};

pub static MIGRATOR: ApplicationMigrator = ApplicationMigrator;

static STEPS: [&dyn MigrationStep; 0] = [];

/// Revisions up to here were written without a stamp. Frozen: nothing before
/// this module changed the file's schema, so there is no shape to probe.
const UNSTAMPED_CEILING: u64 = 0;

/// `application.yaml`, stamped with the schema revision of its content.
pub struct ApplicationDocument;

impl StampedDocument for ApplicationDocument {
    const DOCUMENT: &'static str = "application";
    const SCHEMA_REVISION: u64 = 0;
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
