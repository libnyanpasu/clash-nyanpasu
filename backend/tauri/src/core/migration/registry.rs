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
}
