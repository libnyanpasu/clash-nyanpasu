use super::{
    Ctx, MigrationAdvice, MigrationState, MigrationStep, current_version, registry,
    store::MigrationStore,
};
use anyhow::Context;
use semver::Version;

#[derive(Debug)]
pub struct Runner {
    target: Version,
    force: bool,
    ctx: Ctx,
    store: MigrationStore,
}

impl Default for Runner {
    fn default() -> Self {
        Self::new(false).expect("failed to create migration runner")
    }
}

impl Runner {
    pub fn new(force: bool) -> anyhow::Result<Self> {
        Self::with_target(current_version()?, force)
    }

    pub fn with_target(target: Version, force: bool) -> anyhow::Result<Self> {
        Self::with_context(target, force, Ctx::from_app_dirs()?)
    }

    pub fn advice_step(&self, step: &dyn MigrationStep) -> MigrationAdvice {
        if self.force {
            return MigrationAdvice::Pending;
        }

        match self.store.task_state(step.id()) {
            Some(MigrationState::Completed) => return MigrationAdvice::Done,
            Some(
                MigrationState::Failed | MigrationState::InProgress | MigrationState::NotStarted,
            ) => {
                return MigrationAdvice::Pending;
            }
            None => {}
        }

        let module_state = self.store.module_state(step.module());
        if step.revision() > module_state.applied_revision
            && introduced_in_reached(step.introduced_in(), &self.target)
        {
            MigrationAdvice::Pending
        } else {
            MigrationAdvice::Ignored
        }
    }

    pub fn run_migration(&mut self, step: &dyn MigrationStep) -> anyhow::Result<()> {
        println!("Running migration: {} ({})", step.id(), step.name());
        let advice = self.advice_step(step);
        println!("Advice: {advice:?}");
        if matches!(advice, MigrationAdvice::Ignored | MigrationAdvice::Done) {
            return Ok(());
        }
        self.run_step(step)
    }

    pub fn run_pending(&mut self) -> anyhow::Result<()> {
        println!("Running migrations up to version: {}", self.target);
        let mut first_error = None;

        for module in registry::modules() {
            for step in module.steps() {
                let advice = self.advice_step(*step);
                println!(
                    "[{advice}] {} rev{} {}",
                    step.module(),
                    step.revision(),
                    step.id()
                );
                if advice != MigrationAdvice::Pending {
                    continue;
                }

                if let Err(error) = self.run_step(*step) {
                    eprintln!(
                        "Migration {} failed; stopping module {} and continuing",
                        step.id(),
                        step.module()
                    );
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    break;
                }
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }

        self.store.set_last_succeeded(self.target.clone());
        self.store
            .flush_atomic(&self.ctx.state_path())
            .context("failed to persist successful migration state")?;
        Ok(())
    }

    fn with_context(target: Version, force: bool, ctx: Ctx) -> anyhow::Result<Self> {
        let state_path = ctx.state_path();
        let store = MigrationStore::load(&state_path)?;
        let mut runner = Self {
            target,
            force,
            ctx,
            store,
        };
        runner.ensure_baselines()?;
        Ok(runner)
    }

    fn ensure_baselines(&mut self) -> anyhow::Result<()> {
        let mut changed = false;
        for module in registry::modules() {
            changed |= self.store.ensure_module(module, &self.ctx)?;
        }

        if changed {
            self.store
                .flush_atomic(&self.ctx.state_path())
                .context("failed to persist migration baselines")?;
        }
        Ok(())
    }

    fn run_step(&mut self, step: &dyn MigrationStep) -> anyhow::Result<()> {
        self.store.mark_in_progress(step);
        self.store
            .flush_atomic(&self.ctx.state_path())
            .with_context(|| format!("failed to persist {} in-progress state", step.id()))?;

        match step.run(&mut self.ctx) {
            Ok(()) => {
                println!("Migration {} completed.", step.id());
                self.store.mark_completed(step);
                self.store.bump_module(step.module());
                self.store
                    .flush_atomic(&self.ctx.state_path())
                    .with_context(|| format!("failed to persist {} completed state", step.id()))?;
                Ok(())
            }
            Err(error) => {
                eprintln!("Migration {} failed: {error:#}", step.id());
                if let Err(rollback_error) = step.rollback(&mut self.ctx) {
                    eprintln!(
                        "Migration {} rollback failed: {rollback_error:#}",
                        step.id()
                    );
                }
                self.store.mark_failed(step, &error);
                if let Err(flush_error) = self.store.flush_atomic(&self.ctx.state_path()) {
                    return Err(error.context(format!(
                        "failed to persist {} failed state: {flush_error:#}",
                        step.id()
                    )));
                }
                Err(error)
            }
        }
    }
}

/// Whether a migration introduced in `introduced_in` should run when upgrading
/// to `target`. Only the `(major, minor, patch)` triple is compared, so a
/// prerelease/nightly build (e.g. `2.0.0-rc.1`) still runs migrations introduced
/// in the matching release (`2.0.0`) instead of skipping them due to semver
/// prerelease ordering (`2.0.0-rc.1 < 2.0.0`).
fn introduced_in_reached(introduced_in: &Version, target: &Version) -> bool {
    (
        introduced_in.major,
        introduced_in.minor,
        introduced_in.patch,
    ) <= (target.major, target.minor, target.patch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::migration::store::ModuleState;
    use anyhow::bail;
    use once_cell::sync::Lazy;

    static TEST_VERSION: Lazy<Version> = Lazy::new(|| Version::parse("2.0.0").unwrap());

    struct TestStep {
        id: &'static str,
        module: &'static str,
        revision: u64,
        fail: bool,
    }

    impl MigrationStep for TestStep {
        fn id(&self) -> &'static str {
            self.id
        }

        fn module(&self) -> &'static str {
            self.module
        }

        fn revision(&self) -> u64 {
            self.revision
        }

        fn introduced_in(&self) -> &'static Version {
            &TEST_VERSION
        }

        fn name(&self) -> &'static str {
            self.id
        }

        fn run(&self, _: &mut Ctx) -> anyhow::Result<()> {
            if self.fail {
                bail!("boom")
            }
            Ok(())
        }
    }

    fn runner_with_store(store: MigrationStore, force: bool) -> Runner {
        let temp = tempfile::tempdir().unwrap().keep();
        let config_dir = temp.join("config");
        let data_dir = temp.join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        Runner {
            target: TEST_VERSION.clone(),
            force,
            ctx: Ctx::new(config_dir, data_dir),
            store,
        }
    }

    #[test]
    fn advice_uses_state_before_revision() {
        let step = TestStep {
            id: "profiles/example",
            module: "profiles",
            revision: 1,
            fail: false,
        };
        let mut store = MigrationStore::default();
        store.modules.insert(
            "profiles".to_string(),
            ModuleState {
                applied_revision: 1,
                baseline_revision: 0,
            },
        );

        store.mark_completed(&step);
        let runner = runner_with_store(store.clone(), false);
        assert_eq!(runner.advice_step(&step), MigrationAdvice::Done);

        store.mark_failed(&step, &anyhow::anyhow!("failed"));
        let runner = runner_with_store(store.clone(), false);
        assert_eq!(runner.advice_step(&step), MigrationAdvice::Pending);

        store.mark_in_progress(&step);
        let runner = runner_with_store(store.clone(), false);
        assert_eq!(runner.advice_step(&step), MigrationAdvice::Pending);

        let mut task = store.tasks.get_mut(step.id()).unwrap().clone();
        task.state = MigrationState::NotStarted;
        store.tasks.insert(step.id().to_string(), task);
        let runner = runner_with_store(store.clone(), false);
        assert_eq!(runner.advice_step(&step), MigrationAdvice::Pending);
    }

    #[test]
    fn advice_uses_revision_when_state_is_missing() {
        let step = TestStep {
            id: "profiles/example",
            module: "profiles",
            revision: 2,
            fail: false,
        };
        let mut store = MigrationStore::default();
        store.modules.insert(
            "profiles".to_string(),
            ModuleState {
                applied_revision: 1,
                baseline_revision: 0,
            },
        );
        let runner = runner_with_store(store.clone(), false);
        assert_eq!(runner.advice_step(&step), MigrationAdvice::Pending);

        store.modules.insert(
            "profiles".to_string(),
            ModuleState {
                applied_revision: 2,
                baseline_revision: 0,
            },
        );
        let runner = runner_with_store(store.clone(), false);
        assert_eq!(runner.advice_step(&step), MigrationAdvice::Ignored);

        let runner = runner_with_store(store, true);
        assert_eq!(runner.advice_step(&step), MigrationAdvice::Pending);
    }

    #[test]
    fn advice_runs_on_prerelease_target() {
        // A migration introduced in 2.0.0 must still run on a 2.0.0-rc.1 build,
        // even though semver orders the prerelease below the release.
        let step = TestStep {
            id: "profiles/example",
            module: "profiles",
            revision: 1,
            fail: false,
        };
        let mut store = MigrationStore::default();
        store
            .modules
            .insert("profiles".to_string(), ModuleState::default());
        let mut runner = runner_with_store(store, false);
        runner.target = Version::parse("2.0.0-rc.1").unwrap();
        assert_eq!(runner.advice_step(&step), MigrationAdvice::Pending);
    }

    #[test]
    fn in_progress_is_pending_on_reentry() {
        let step = TestStep {
            id: "profiles/example",
            module: "profiles",
            revision: 1,
            fail: false,
        };
        let mut store = MigrationStore::default();
        store
            .modules
            .insert("profiles".to_string(), ModuleState::default());
        store.mark_in_progress(&step);
        let runner = runner_with_store(store, false);
        assert_eq!(runner.advice_step(&step), MigrationAdvice::Pending);
    }

    #[test]
    fn failed_step_does_not_advance_module_revision() {
        let step = TestStep {
            id: "profiles/example",
            module: "profiles",
            revision: 1,
            fail: true,
        };
        let mut store = MigrationStore::default();
        store
            .modules
            .insert("profiles".to_string(), ModuleState::default());
        let mut runner = runner_with_store(store, false);

        assert!(runner.run_step(&step).is_err());
        assert_eq!(
            runner.store.module_state("profiles"),
            ModuleState {
                applied_revision: 0,
                baseline_revision: 0,
            }
        );
        assert_eq!(
            runner.store.task_state(step.id()),
            Some(MigrationState::Failed)
        );
    }
}
