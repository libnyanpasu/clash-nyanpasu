use super::{MigrationStep, ModuleMigrator, modules};
use once_cell::sync::Lazy;

pub static MODULES: Lazy<Vec<&'static dyn ModuleMigrator>> = Lazy::new(|| {
    vec![
        &modules::profiles::MIGRATOR,
        &modules::app_config::MIGRATOR,
        // Typed config before storage: `storage/hotkeys_to_typed_config` writes
        // into `application.yaml`, which `typed_config/split_legacy_config`
        // creates.
        &modules::typed_config::MIGRATOR,
        &modules::storage::MIGRATOR,
    ]
});

pub fn modules() -> impl Iterator<Item = &'static dyn ModuleMigrator> {
    MODULES.iter().copied()
}

fn get_migrations() -> Vec<&'static dyn MigrationStep> {
    modules()
        .flat_map(|module| module.steps().iter().copied())
        .collect()
}

pub fn find_migration(id: &str) -> Option<&'static dyn MigrationStep> {
    get_migrations().into_iter().find(|step| step.id() == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_steps_are_sorted_by_revision() {
        for module in modules() {
            let mut previous = 0;
            for step in module.steps() {
                assert!(
                    step.revision() > previous,
                    "{} revisions must be strictly ascending",
                    module.module()
                );
                previous = step.revision();
            }
        }
    }

    /// Heuristic modules are frozen: a new step would need another shape
    /// probe. New config migrations belong in a document module.
    #[test]
    fn heuristic_modules_gain_no_steps() {
        use super::super::ModuleKind;

        for module in modules() {
            let head = module.steps().last().map_or(0, |step| step.revision());
            match module.kind() {
                ModuleKind::Heuristic => {
                    let frozen = match module.module() {
                        "app_config" => 4,
                        "typed_config" => 2,
                        "storage" => 2,
                        other => panic!("{other} is a new heuristic module; make it a document"),
                    };
                    assert_eq!(head, frozen, "{} is frozen", module.module());
                }
                ModuleKind::Document(spec) => {
                    assert!(spec.unstamped_ceiling <= head, "{}", module.module());
                }
            }
        }
    }
}
