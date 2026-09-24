use super::{
    Ctx, DocumentSpec, MigrationAdvice, MigrationCheckError, MigrationState, MigrationStep,
    ModuleKind, ModuleMigrator, StepCheck, current_version, fs, registry,
    store::{MigrationStore, ModuleState},
};
use crate::utils::path::PathResolver;
use anyhow::{Context, bail};
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

    pub fn with_paths(paths: PathResolver, force: bool) -> anyhow::Result<Self> {
        Self::with_context(current_version()?, force, Ctx::from_paths(paths))
    }

    pub fn advice_step(&self, step: &dyn MigrationStep) -> MigrationAdvice {
        if self.force {
            return MigrationAdvice::Pending;
        }

        match self.store.task_state(step.id()) {
            Some(MigrationState::Completed | MigrationState::Skipped) => {
                return MigrationAdvice::Done;
            }
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
        self.ensure_known_revisions()?;
        let advice = self.advice_step(step);
        println!("Advice: {advice:?}");
        if matches!(advice, MigrationAdvice::Ignored | MigrationAdvice::Done) {
            return Ok(());
        }
        self.run_step(step)
    }

    pub fn run_pending(&mut self) -> anyhow::Result<()> {
        println!("Running migrations up to version: {}", self.target);
        self.ensure_known_revisions()?;
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
        self.verify_applied_revisions()?;
        self.stamp_adopted_documents()?;

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
        let mut changed = self.reconcile_documents()?;
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

    /// Settles each document module against the stamp in its file, before
    /// the baselines are written. A stamp is the file's own record of its
    /// revision, so it replaces `detect_baseline` when the state has no entry,
    /// and it refuses a state that the file contradicts. A state already ahead
    /// of this build is left to `ensure_known_revisions`, which runs after the
    /// baselines are written. Returns whether the state changed.
    fn reconcile_documents(&mut self) -> anyhow::Result<bool> {
        let state_path = self.ctx.state_path();
        let mut changed = false;
        for module in registry::modules() {
            let ModuleKind::Document(spec) = module.kind() else {
                continue;
            };
            let head = head_revision(module);
            let entry = self.store.modules.get(module.module()).copied();
            // Left to `ensure_known_revisions`, which explains it better.
            if entry.is_some_and(|state| state.applied_revision > head) {
                continue;
            }
            let path = (spec.path)(&self.ctx);
            let file = fs::read_document(&path, spec.document)
                .with_context(|| format!("failed to inspect the {} files", module.module()))?;
            let Some(file) = file else {
                if entry.is_none() {
                    // Nothing to migrate: the file will be created at head.
                    self.store
                        .modules
                        .insert(module.module().to_string(), at_revision(head, false));
                    changed = true;
                }
                continue;
            };

            match (file.schema_revision, entry) {
                (Some(stamped), _) if stamped > head => bail!(
                    "{} is stamped at revision {stamped}, but this build only knows revisions \
                     up to {head}; the file was written by a newer version. Update to that \
                     version, or restore {} from a backup made by this one.",
                    path.display(),
                    path.display()
                ),
                (Some(stamped), None) => {
                    self.store
                        .modules
                        .insert(module.module().to_string(), at_revision(stamped, true));
                    changed = true;
                }
                (Some(stamped), Some(state)) if stamped < state.applied_revision => bail!(
                    "{} is stamped at revision {stamped}, although {} records {} as migrated \
                     to revision {}. The file may have been restored from a backup or rewritten \
                     by an older version. Restore it, or delete {} so the file is migrated \
                     again from its stamp.",
                    path.display(),
                    state_path.display(),
                    module.module(),
                    state.applied_revision,
                    state_path.display()
                ),
                (Some(_), Some(state)) => {
                    if !state.stamped {
                        self.set_stamped(module.module());
                        changed = true;
                    }
                }
                (None, Some(state))
                    if state.stamped || state.applied_revision > spec.unstamped_ceiling =>
                {
                    bail!(
                        "{} has lost the `{}` stamp it was written with; it may have been \
                         rewritten by an older version or edited by hand. Restore it from a \
                         backup, or delete {} so its revision is detected from its content.",
                        path.display(),
                        nyanpasu_core::format::STAMP_KEY,
                        state_path.display()
                    )
                }
                // Written before stamps: `detect_baseline` and `files_behind`
                // still judge it, and `stamp_adopted_documents` stamps it once
                // it is verified at head.
                (None, _) => {}
            }
        }
        Ok(changed)
    }

    /// Stamps document files that predate stamps once they are verified at
    /// their module's head, and records every stamped file, so that later
    /// starts judge them by their stamp alone.
    fn stamp_adopted_documents(&mut self) -> anyhow::Result<()> {
        for module in registry::modules() {
            let ModuleKind::Document(spec) = module.kind() else {
                continue;
            };
            let path = (spec.path)(&self.ctx);
            let Some(file) = fs::read_document(&path, spec.document)
                .with_context(|| format!("failed to inspect the {} files", module.module()))?
            else {
                continue;
            };
            if file.schema_revision.is_none() {
                let head = head_revision(module);
                if self.store.module_state(module.module()).applied_revision != head {
                    continue;
                }
                fs::write_document(&path, spec.document, head, file.payload, None)
                    .with_context(|| format!("failed to stamp the {} files", module.module()))?;
            }
            self.set_stamped(module.module());
        }
        Ok(())
    }

    fn set_stamped(&mut self, module: &str) {
        self.store
            .modules
            .entry(module.to_string())
            .or_default()
            .stamped = true;
    }

    /// Refuses data migrated by a newer build: this build cannot know which
    /// files those unknown revisions moved or rewrote, so it would start on
    /// stale locations.
    fn ensure_known_revisions(&self) -> anyhow::Result<()> {
        for module in registry::modules() {
            let applied = self.store.module_state(module.module()).applied_revision;
            let head = head_revision(module);
            if applied > head {
                bail!(
                    "migration state records {} at revision {applied}, but this build only \
                     knows revisions up to {head}; the data was migrated by a newer version",
                    module.module()
                );
            }
        }
        Ok(())
    }

    /// Refuses a state file that is ahead of the files on disk, which would
    /// otherwise skip the migrations those files still need.
    fn verify_applied_revisions(&self) -> anyhow::Result<()> {
        let state_path = self.ctx.state_path();
        for module in registry::modules() {
            let applied = self.store.module_state(module.module()).applied_revision;
            let behind = module
                .files_behind(&self.ctx, applied)
                .with_context(|| format!("failed to inspect the {} files", module.module()))?;
            if let Some(reason) = behind {
                bail!(
                    "{reason}, although {} records {} as migrated to revision {applied}. The \
                     files may have been moved, restored from a backup or rewritten by an older \
                     version. Restore them, or delete {} so the files on disk are detected and \
                     migrated again.",
                    state_path.display(),
                    module.module(),
                    state_path.display()
                );
            }
        }
        Ok(())
    }

    fn run_step(&mut self, step: &dyn MigrationStep) -> anyhow::Result<()> {
        match check_step(step, &self.ctx) {
            Ok(Some(StepCheck::Satisfied)) => {
                println!("Migration {} is already satisfied, skipping.", step.id());
                return self.finish_step(step, true);
            }
            Ok(Some(StepCheck::Needed) | None) => {}
            Err(error) => {
                let error =
                    anyhow::Error::new(error).context(format!("failed to check {}", step.id()));
                eprintln!("Migration {} failed: {error:#}", step.id());
                return Err(self.record_failure(step, error));
            }
        }

        self.store.mark_in_progress(step);
        self.store
            .flush_atomic(&self.ctx.state_path())
            .with_context(|| format!("failed to persist {} in-progress state", step.id()))?;

        match step
            .run(&mut self.ctx)
            .and_then(|()| ensure_satisfied(step, &self.ctx))
        {
            Ok(()) => {
                println!("Migration {} completed.", step.id());
                self.finish_step(step, false)
            }
            Err(error) => {
                eprintln!("Migration {} failed: {error:#}", step.id());
                if let Err(rollback_error) = step.rollback(&mut self.ctx) {
                    eprintln!(
                        "Migration {} rollback failed: {rollback_error:#}",
                        step.id()
                    );
                }
                Err(self.record_failure(step, error))
            }
        }
    }

    fn finish_step(&mut self, step: &dyn MigrationStep, skipped: bool) -> anyhow::Result<()> {
        // Recorded in the same flush, so a stamp lost later is recognised as
        // lost even if the rest of the run never finishes.
        if let Some(spec) = document_spec(step.module())
            && fs::read_document(&(spec.path)(&self.ctx), spec.document)?
                .is_some_and(|file| file.schema_revision.is_some())
        {
            self.set_stamped(step.module());
        }
        if skipped {
            self.store.mark_skipped(step);
        } else {
            self.store.mark_completed(step);
        }
        self.store.bump_module(step.module());
        self.store
            .flush_atomic(&self.ctx.state_path())
            .with_context(|| format!("failed to persist {} finished state", step.id()))
    }

    fn record_failure(&mut self, step: &dyn MigrationStep, error: anyhow::Error) -> anyhow::Error {
        self.store.mark_failed(step, &error);
        match self.store.flush_atomic(&self.ctx.state_path()) {
            Ok(()) => error,
            Err(flush_error) => error.context(format!(
                "failed to persist {} failed state: {flush_error:#}",
                step.id()
            )),
        }
    }
}

/// Asks a step's check again right after its run. Still `Needed` means the
/// check or the run is defective, which must not be recorded as completed.
/// A document step must also leave its file in place and stamped, which a
/// fallback check cannot tell.
fn ensure_satisfied(step: &dyn MigrationStep, ctx: &Ctx) -> anyhow::Result<()> {
    if let Some(spec) = document_spec(step.module()) {
        let file = fs::read_document(&(spec.path)(ctx), spec.document)
            .with_context(|| format!("failed to check {} after it ran", step.id()))?;
        if file.is_none_or(|file| file.schema_revision.is_none()) {
            bail!(
                "migration {} ran without leaving its document stamped; this is a bug in the \
                 migration",
                step.id()
            );
        }
    }
    match check_step(step, ctx) {
        Ok(Some(StepCheck::Needed)) => bail!(
            "migration {} ran but its check still reports it as needed; this is a bug in the \
             migration",
            step.id()
        ),
        Ok(Some(StepCheck::Satisfied) | None) => Ok(()),
        Err(error) => Err(anyhow::Error::new(error)
            .context(format!("failed to check {} after it ran", step.id()))),
    }
}

/// Whether `step` is needed. A document module's file answers for every step
/// of the module, so document steps need no check of their own: a missing
/// file has nothing to migrate and is created at head, and a stamped one
/// needs the steps above its stamp. A step that runs without stamping its
/// revision is caught by [`ensure_satisfied`]. Unstamped files fall back to
/// the step's check.
fn check_step(
    step: &dyn MigrationStep,
    ctx: &Ctx,
) -> Result<Option<StepCheck>, MigrationCheckError> {
    if let Some(spec) = document_spec(step.module()) {
        match fs::read_document(&(spec.path)(ctx), spec.document)? {
            None => return Ok(Some(StepCheck::Satisfied)),
            Some(file) => {
                if let Some(stamped) = file.schema_revision {
                    return Ok(Some(StepCheck::from_needed(stamped < step.revision())));
                }
            }
        }
    }
    step.check(ctx)
}

fn document_spec(module: &str) -> Option<DocumentSpec> {
    registry::modules()
        .find(|candidate| candidate.module() == module)
        .and_then(|candidate| match candidate.kind() {
            ModuleKind::Document(spec) => Some(spec),
            ModuleKind::Heuristic => None,
        })
}

fn head_revision(module: &dyn ModuleMigrator) -> u64 {
    module.steps().last().map_or(0, |step| step.revision())
}

fn at_revision(revision: u64, stamped: bool) -> ModuleState {
    ModuleState {
        applied_revision: revision,
        baseline_revision: revision,
        stamped,
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
    use crate::core::migration::{
        MigrationCheckError,
        store::{ModuleState, STORE_FILE_NAME},
    };
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
                stamped: false,
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
                stamped: false,
            },
        );
        let runner = runner_with_store(store.clone(), false);
        assert_eq!(runner.advice_step(&step), MigrationAdvice::Pending);

        store.modules.insert(
            "profiles".to_string(),
            ModuleState {
                applied_revision: 2,
                baseline_revision: 0,
                stamped: false,
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

    fn assert_yaml_shape_eq(path: std::path::PathBuf, expected: &str) {
        let actual_raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let actual: serde_yaml::Value = serde_yaml::from_str(&actual_raw).unwrap();
        let expected: serde_yaml::Value = serde_yaml::from_str(expected).unwrap();
        assert_eq!(
            actual,
            expected,
            "migrated YAML shape mismatch for {}",
            path.display()
        );
    }

    /// Pins the supported upgrade boundary (1.6.1 -> 2.0): a real 1.6.1 config
    /// pair must end up in the exact 2.0 shape every migration step produces.
    /// Compares parsed structure (order-independent) rather than raw bytes so the
    /// test tracks the data contract, not serializer key ordering.
    #[test]
    fn real_1_6_1_fixture_migrates_to_2_0_shape() {
        use crate::core::storage::{Storage, WebStorage};

        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();

        let config_path = config_dir.join("nyanpasu-config.yaml");
        let profiles_path = config_dir.join("profiles.yaml");
        std::fs::write(
            &config_path,
            include_str!("fixtures/v1_6_1/nyanpasu-config.yaml"),
        )
        .unwrap();
        std::fs::write(
            &profiles_path,
            include_str!("fixtures/v1_6_1/profiles.yaml"),
        )
        .unwrap();

        let ctx = Ctx::new(config_dir.clone(), data_dir.clone());
        let mut runner =
            Runner::with_context(Version::parse("2.0.0").unwrap(), false, ctx).unwrap();
        runner.run_pending().unwrap();

        assert_yaml_shape_eq(
            config_path,
            include_str!("fixtures/v2_0_expected/nyanpasu-config.yaml"),
        );

        // The script migration re-prefixes profiles.yaml; the header is part of
        // the on-disk contract, so assert it verbatim before the comment-agnostic
        // shape check.
        let profiles_raw = std::fs::read_to_string(&profiles_path).unwrap();
        assert!(
            profiles_raw.starts_with("# Profiles Config for Clash Nyanpasu\n\n"),
            "profiles.yaml lost its generated header prefix"
        );
        assert_yaml_shape_eq(
            profiles_path,
            include_str!("fixtures/v2_0_expected/profiles.yaml"),
        );

        // Each module advanced to the head of its step list.
        assert_eq!(runner.store.module_state("profiles").applied_revision, 4);
        assert_eq!(runner.store.module_state("app_config").applied_revision, 4);
        assert_eq!(runner.store.module_state("storage").applied_revision, 2);
        assert_eq!(
            runner.store.module_state("typed_config").applied_revision,
            2
        );
        assert_eq!(
            runner.store.app.last_succeeded,
            Some(Version::parse("2.0.0").unwrap())
        );

        // Hotkeys left the legacy config file and ended up in the typed
        // application config, with nothing stranded in KV storage.
        let storage = Storage::try_new(&data_dir.join(crate::utils::dirs::STORAGE_DB)).unwrap();
        let hotkeys: Option<Vec<String>> = storage.get_item("hotkeys").unwrap();
        assert_eq!(hotkeys, None);

        let application: nyanpasu_config::application::NyanpasuAppConfig = serde_yaml::from_str(
            &std::fs::read_to_string(config_dir.join("application.yaml")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            application.hotkeys,
            vec![
                "clash_mode_rule,Control+Q".to_string(),
                "toggle_system_proxy,Control+Shift+P".to_string(),
            ],
            "the typed application config is the single authority for hotkeys"
        );
        let _: nyanpasu_config::state::PersistentState = serde_yaml::from_str(
            &std::fs::read_to_string(config_dir.join("session-state.yaml")).unwrap(),
        )
        .unwrap();
        let _: nyanpasu_config::clash::config::ClashConfig = serde_yaml::from_str(
            &std::fs::read_to_string(config_dir.join("clash-config.yaml")).unwrap(),
        )
        .unwrap();
    }

    /// `typed_config` splits the legacy file while `hotkeys` is still in it, so
    /// both it and `storage` end up with an opinion about the list. The
    /// key-value store is the one the hotkey editor wrote to, so the typed
    /// config must end the upgrade holding exactly what `hotkeys_to_kv` parked
    /// there.
    #[test]
    fn full_chain_keeps_the_key_value_hotkeys() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();

        let legacy_raw = include_str!("fixtures/v1_6_1/nyanpasu-config.yaml");
        std::fs::write(config_dir.join("nyanpasu-config.yaml"), legacy_raw).unwrap();
        std::fs::write(
            config_dir.join("profiles.yaml"),
            include_str!("fixtures/v1_6_1/profiles.yaml"),
        )
        .unwrap();

        // Read back out of the fixture rather than spelled out here, so the
        // assertion tracks whatever the legacy file actually carries.
        let legacy: serde_yaml::Mapping = serde_yaml::from_str(legacy_raw).unwrap();
        let expected: Vec<String> = legacy
            .get(serde_yaml::Value::String("hotkeys".into()))
            .and_then(|value| value.as_sequence())
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(ToString::to_string))
                    .collect()
            })
            .expect("the 1.6.1 fixture carries hotkeys");
        assert!(!expected.is_empty());

        let ctx = Ctx::new(config_dir.clone(), data_dir);
        let mut runner =
            Runner::with_context(Version::parse("2.0.0").unwrap(), false, ctx).unwrap();
        runner.run_pending().unwrap();

        let application: nyanpasu_config::application::NyanpasuAppConfig = serde_yaml::from_str(
            &std::fs::read_to_string(config_dir.join("application.yaml")).unwrap(),
        )
        .unwrap();
        assert_eq!(application.hotkeys, expected);
    }

    #[test]
    fn existing_store_without_typed_config_accepts_legacy_runtime_clash_baseline() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();

        let mut store = MigrationStore::default();
        store.modules.insert(
            "profiles".to_string(),
            ModuleState {
                applied_revision: 2,
                baseline_revision: 2,
                stamped: false,
            },
        );
        store.modules.insert(
            "app_config".to_string(),
            ModuleState {
                applied_revision: 3,
                baseline_revision: 3,
                stamped: false,
            },
        );
        store.modules.insert(
            "storage".to_string(),
            ModuleState {
                applied_revision: 1,
                baseline_revision: 1,
                stamped: false,
            },
        );
        store
            .flush_atomic(&config_dir.join(STORE_FILE_NAME))
            .unwrap();
        std::fs::write(
            config_dir.join("clash-config.yaml"),
            "mixed-port: 7890\nproxies: []\nproxy-groups: []\nrules: []\n",
        )
        .unwrap();

        let ctx = Ctx::new(config_dir.clone(), data_dir);
        let mut runner =
            Runner::with_context(Version::parse("2.0.0").unwrap(), false, ctx).unwrap();

        assert_eq!(
            runner.store.module_state("typed_config").applied_revision,
            0
        );
        assert_eq!(
            runner.store.module_state("typed_config").baseline_revision,
            0
        );

        runner.run_pending().unwrap();

        assert_eq!(
            runner.store.module_state("typed_config").applied_revision,
            2
        );
        let _: nyanpasu_config::application::NyanpasuAppConfig = serde_yaml::from_str(
            &std::fs::read_to_string(config_dir.join("application.yaml")).unwrap(),
        )
        .unwrap();
        let _: nyanpasu_config::state::PersistentState = serde_yaml::from_str(
            &std::fs::read_to_string(config_dir.join("session-state.yaml")).unwrap(),
        )
        .unwrap();
        let _: nyanpasu_config::clash::config::ClashConfig = serde_yaml::from_str(
            &std::fs::read_to_string(config_dir.join("clash-config.yaml")).unwrap(),
        )
        .unwrap();
    }

    /// Every module recorded at the head this build knows, as a finished
    /// migration leaves it.
    fn store_at_head() -> MigrationStore {
        let mut store = MigrationStore::default();
        for module in registry::modules() {
            let head = module.steps().last().unwrap().revision();
            store.modules.insert(
                module.module().to_string(),
                ModuleState {
                    applied_revision: head,
                    baseline_revision: head,
                    stamped: false,
                },
            );
        }
        store
    }

    fn run_pending_with_store(
        store: MigrationStore,
        prepare: impl FnOnce(&std::path::Path),
    ) -> (anyhow::Result<()>, MigrationStore) {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let state_path = config_dir.join(STORE_FILE_NAME);
        store.flush_atomic(&state_path).unwrap();
        prepare(&config_dir);

        let ctx = Ctx::new(config_dir, data_dir);
        let result = Runner::with_context(TEST_VERSION.clone(), false, ctx)
            .and_then(|mut runner| runner.run_pending());
        (result, MigrationStore::load(&state_path).unwrap())
    }

    #[test]
    fn refuses_revisions_newer_than_this_build() {
        let mut store = store_at_head();
        store
            .modules
            .get_mut("typed_config")
            .unwrap()
            .applied_revision += 1;

        let (result, persisted) = run_pending_with_store(store, |_| {});

        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("typed_config"), "{error}");
        assert!(error.contains("newer version"), "{error}");
        assert_eq!(persisted.app.last_succeeded, None);
    }

    #[test]
    fn refuses_typed_config_files_missing_behind_applied_revision() {
        // The split files were moved away while the state still says the split
        // happened; starting would silently fall back to defaults.
        let (result, persisted) = run_pending_with_store(store_at_head(), |_| {});

        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("typed_config"), "{error}");
        assert!(
            error.contains("application.yaml and session-state.yaml are missing"),
            "{error}"
        );
        // The dialog must point at the state file and say how to recover.
        assert!(error.contains(STORE_FILE_NAME), "{error}");
        assert!(error.contains("delete"), "{error}");
        assert_eq!(persisted.app.last_succeeded, None);
    }

    #[test]
    fn refuses_legacy_profiles_behind_applied_revision() {
        let (result, persisted) = run_pending_with_store(store_at_head(), |config_dir| {
            std::fs::write(
                config_dir.join("profiles.yaml"),
                include_str!("fixtures/v1_6_1/profiles.yaml"),
            )
            .unwrap();
        });

        let error = format!("{:#}", result.unwrap_err());
        assert!(error.contains("legacy profile schema"), "{error}");
        assert_eq!(persisted.app.last_succeeded, None);
    }

    /// A step whose check answers with `check` and whose run always fails, so
    /// a test can tell whether the runner reached it.
    struct CheckedStep {
        check: fn() -> Result<Option<StepCheck>, MigrationCheckError>,
    }

    impl MigrationStep for CheckedStep {
        fn id(&self) -> &'static str {
            "example/checked"
        }

        fn module(&self) -> &'static str {
            "example"
        }

        fn revision(&self) -> u64 {
            1
        }

        fn introduced_in(&self) -> &'static Version {
            &TEST_VERSION
        }

        fn name(&self) -> &'static str {
            "CheckedStep"
        }

        fn check(&self, _: &Ctx) -> Result<Option<StepCheck>, MigrationCheckError> {
            (self.check)()
        }

        fn run(&self, _: &mut Ctx) -> anyhow::Result<()> {
            bail!("run must not be reached")
        }
    }

    fn runner_with_example_at_zero() -> Runner {
        let mut store = MigrationStore::default();
        store
            .modules
            .insert("example".to_string(), ModuleState::default());
        runner_with_store(store, false)
    }

    #[test]
    fn satisfied_check_persists_skipped_without_running() {
        let step = CheckedStep {
            check: || Ok(Some(StepCheck::Satisfied)),
        };
        let mut runner = runner_with_example_at_zero();

        runner.run_step(&step).unwrap();

        let loaded = MigrationStore::load(&runner.ctx.state_path()).unwrap();
        assert_eq!(loaded.task_state(step.id()), Some(MigrationState::Skipped));
        assert_eq!(loaded.module_state("example").applied_revision, 1);
        let task = &loaded.tasks[step.id()];
        assert!(task.started_at.is_none());
        assert!(task.finished_at.is_some());
        assert!(task.error.is_none());

        let mut restarted = runner_with_store(loaded, false);
        assert_eq!(restarted.advice_step(&step), MigrationAdvice::Done);
        restarted.force = true;
        assert_eq!(restarted.advice_step(&step), MigrationAdvice::Pending);
        restarted.run_migration(&step).unwrap();
        assert_eq!(
            restarted.store.task_state(step.id()),
            Some(MigrationState::Skipped)
        );
    }

    #[test]
    fn satisfied_retry_replaces_the_previous_failure() {
        let step = CheckedStep {
            check: || Ok(Some(StepCheck::Satisfied)),
        };
        let mut runner = runner_with_example_at_zero();
        runner
            .store
            .mark_failed(&step, &anyhow::anyhow!("previous failure"));

        runner.run_step(&step).unwrap();

        let loaded = MigrationStore::load(&runner.ctx.state_path()).unwrap();
        let task = &loaded.tasks[step.id()];
        assert_eq!(task.state, MigrationState::Skipped);
        assert!(task.started_at.is_none());
        assert!(task.error.is_none());
        assert_eq!(loaded.module_state("example").applied_revision, 1);
    }

    #[test]
    fn check_error_fails_without_running() {
        let step = CheckedStep {
            check: || Err(MigrationCheckError::Unrecognized("odd shape".to_string())),
        };
        let mut runner = runner_with_example_at_zero();

        let error = format!("{:#}", runner.run_step(&step).unwrap_err());

        assert!(error.contains("odd shape"), "{error}");
        assert!(!error.contains("run must not be reached"), "{error}");
        assert_eq!(
            runner.store.task_state(step.id()),
            Some(MigrationState::Failed)
        );
        assert_eq!(runner.store.module_state("example").applied_revision, 0);
    }

    struct NeverSatisfiedStep;

    impl MigrationStep for NeverSatisfiedStep {
        fn id(&self) -> &'static str {
            "example/never_satisfied"
        }

        fn module(&self) -> &'static str {
            "example"
        }

        fn revision(&self) -> u64 {
            1
        }

        fn introduced_in(&self) -> &'static Version {
            &TEST_VERSION
        }

        fn name(&self) -> &'static str {
            "NeverSatisfiedStep"
        }

        fn check(&self, _: &Ctx) -> Result<Option<StepCheck>, MigrationCheckError> {
            Ok(Some(StepCheck::Needed))
        }

        fn run(&self, _: &mut Ctx) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn check_still_needed_after_run_is_reported_as_a_defect() {
        let mut runner = runner_with_example_at_zero();

        let error = format!("{:#}", runner.run_step(&NeverSatisfiedStep).unwrap_err());

        assert!(error.contains("still reports it as needed"), "{error}");
        assert_eq!(
            runner.store.task_state(NeverSatisfiedStep.id()),
            Some(MigrationState::Failed)
        );
        assert_eq!(runner.store.module_state("example").applied_revision, 0);
    }

    #[test]
    fn unreadable_files_are_not_reported_as_behind() {
        let (result, persisted) = run_pending_with_store(store_at_head(), |config_dir| {
            std::fs::write(config_dir.join("profiles.yaml"), "items: [unclosed\n").unwrap();
        });

        let error = format!("{:#}", result.unwrap_err());
        assert!(
            error.contains("failed to inspect the profiles files"),
            "{error}"
        );
        assert!(!error.contains("delete"), "{error}");
        assert_eq!(persisted.app.last_succeeded, None);
    }

    #[test]
    fn single_migration_refuses_revisions_newer_than_this_build() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let mut store = store_at_head();
        store.modules.get_mut("profiles").unwrap().applied_revision += 1;
        store
            .flush_atomic(&config_dir.join(STORE_FILE_NAME))
            .unwrap();

        let ctx = Ctx::new(config_dir, data_dir);
        let mut runner = Runner::with_context(TEST_VERSION.clone(), true, ctx).unwrap();
        let step = registry::find_migration("profiles/null_value").unwrap();

        let error = format!("{:#}", runner.run_migration(step).unwrap_err());
        assert!(error.contains("newer version"), "{error}");
        assert_eq!(runner.store.task_state(step.id()), None);
    }

    #[test]
    fn failed_step_does_not_advance_module_revision() {
        let step = TestStep {
            id: "example/example",
            module: "example",
            revision: 1,
            fail: true,
        };
        let mut store = MigrationStore::default();
        store
            .modules
            .insert("example".to_string(), ModuleState::default());
        let mut runner = runner_with_store(store, false);

        assert!(runner.run_step(&step).is_err());
        assert_eq!(
            runner.store.module_state("example"),
            ModuleState {
                applied_revision: 0,
                baseline_revision: 0,
                stamped: false,
            }
        );
        assert_eq!(
            runner.store.task_state(step.id()),
            Some(MigrationState::Failed)
        );
    }

    const CLEAN_PROFILES: &str = "items:\n- uid: a\n  name: A\n  type: config\n  config:\n    \
                                  type: file\n    source:\n      type: local\n      binding:\n        \
                                  type: managed\n        file: a.yaml\n";

    fn stamped_profiles(revision: u64, body: &str) -> String {
        format!("_nyanpasu:\n  document: profiles\n  schema_revision: {revision}\n{body}")
    }

    fn profiles_at(applied: u64, stamped: bool) -> MigrationStore {
        let mut store = MigrationStore::default();
        store.modules.insert(
            "profiles".to_string(),
            ModuleState {
                applied_revision: applied,
                baseline_revision: applied,
                stamped,
            },
        );
        store
    }

    struct DocumentCase {
        _temp: tempfile::TempDir,
        ctx: Ctx,
    }

    impl DocumentCase {
        fn new(store: Option<MigrationStore>, profiles: &str) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let config_dir = temp.path().join("config");
            let data_dir = temp.path().join("data");
            std::fs::create_dir_all(&config_dir).unwrap();
            std::fs::create_dir_all(&data_dir).unwrap();
            if let Some(store) = store {
                store
                    .flush_atomic(&config_dir.join(STORE_FILE_NAME))
                    .unwrap();
            }
            std::fs::write(config_dir.join("profiles.yaml"), profiles).unwrap();
            Self {
                _temp: temp,
                ctx: Ctx::new(config_dir, data_dir),
            }
        }

        fn runner(&self) -> anyhow::Result<Runner> {
            Runner::with_context(TEST_VERSION.clone(), false, self.ctx.clone())
        }

        fn profiles(&self) -> String {
            std::fs::read_to_string(self.ctx.profiles_path()).unwrap()
        }

        fn state(&self) -> Option<String> {
            std::fs::read_to_string(self.ctx.state_path()).ok()
        }

        fn stamp(&self) -> Option<u64> {
            nyanpasu_core::format::inspect(&self.profiles())
                .unwrap()
                .stamp()
                .map(|stamp| stamp.schema_revision)
        }
    }

    #[test]
    fn stamp_replaces_detection_when_the_state_is_lost() {
        // Shape detection would put this legacy file at revision 0.
        let case = DocumentCase::new(
            None,
            &stamped_profiles(4, include_str!("fixtures/v1_6_1/profiles.yaml")),
        );

        let runner = case.runner().unwrap();

        assert_eq!(
            runner.store.module_state("profiles"),
            ModuleState {
                applied_revision: 4,
                baseline_revision: 4,
                stamped: true,
            }
        );
    }

    #[test]
    fn unstamped_profiles_are_detected_then_stamped_once_migrated() {
        let case = DocumentCase::new(None, include_str!("fixtures/v1_6_1/profiles.yaml"));

        let mut runner = case.runner().unwrap();
        assert_eq!(runner.store.module_state("profiles").baseline_revision, 0);
        runner.run_pending().unwrap();

        assert_eq!(case.stamp(), Some(4));
        let state = MigrationStore::load(&case.ctx.state_path()).unwrap();
        assert_eq!(state.module_state("profiles").applied_revision, 4);
        assert!(state.module_state("profiles").stamped);
    }

    #[test]
    fn refuses_a_lost_stamp() {
        let case = DocumentCase::new(Some(profiles_at(4, true)), CLEAN_PROFILES);
        let state = case.state();

        let error = format!("{:#}", case.runner().unwrap_err());

        assert!(error.contains("lost the `_nyanpasu` stamp"), "{error}");
        assert!(error.contains(STORE_FILE_NAME), "{error}");
        assert_eq!(case.state(), state);
        assert_eq!(case.profiles(), CLEAN_PROFILES);
    }

    #[test]
    fn refuses_a_stamp_behind_the_state() {
        let profiles = stamped_profiles(2, CLEAN_PROFILES);
        let case = DocumentCase::new(Some(profiles_at(4, true)), &profiles);
        let state = case.state();

        let error = format!("{:#}", case.runner().unwrap_err());

        assert!(error.contains("stamped at revision 2"), "{error}");
        assert!(error.contains("revision 4"), "{error}");
        assert_eq!(case.state(), state);
        assert_eq!(case.profiles(), profiles);
    }

    #[test]
    fn refuses_a_stamp_newer_than_this_build() {
        let profiles = stamped_profiles(5, CLEAN_PROFILES);
        let case = DocumentCase::new(None, &profiles);

        let error = format!("{:#}", case.runner().unwrap_err());
        assert!(error.contains("newer version"), "{error}");
        // `--migration <id>` builds the same runner, forced or not.
        let forced = Runner::with_context(TEST_VERSION.clone(), true, case.ctx.clone());
        assert!(forced.is_err());

        assert_eq!(case.state(), None);
        assert_eq!(case.profiles(), profiles);
    }

    #[test]
    fn refuses_malformed_or_foreign_stamps_before_any_write() {
        for profiles in [
            "_nyanpasu: 4\nitems: []\n".to_string(),
            "_nyanpasu:\n  document: application\n  schema_revision: 4\nitems: []\n".to_string(),
        ] {
            let case = DocumentCase::new(None, &profiles);

            let error = format!("{:#}", case.runner().unwrap_err());

            assert!(
                error.contains("failed to inspect the profiles files"),
                "{error}"
            );
            assert_eq!(case.state(), None);
            assert_eq!(case.profiles(), profiles);
        }
    }

    #[test]
    fn stamp_ahead_of_the_state_skips_the_recorded_steps_without_rewriting() {
        // The file reached revision 4 but the state was not updated after.
        let profiles = stamped_profiles(4, CLEAN_PROFILES);
        let case = DocumentCase::new(Some(profiles_at(2, true)), &profiles);

        let mut runner = case.runner().unwrap();
        runner.run_pending().unwrap();

        assert_eq!(runner.store.module_state("profiles").applied_revision, 4);
        assert_eq!(
            runner.store.task_state("profiles/repair_schema"),
            Some(MigrationState::Skipped)
        );
        assert_eq!(case.profiles(), profiles);
    }

    #[test]
    fn document_step_that_does_not_stamp_is_a_defect() {
        let case = DocumentCase::new(Some(profiles_at(0, false)), CLEAN_PROFILES);
        let step = TestStep {
            id: "profiles/example",
            module: "profiles",
            revision: 1,
            fail: false,
        };
        let mut runner = case.runner().unwrap();

        let error = format!("{:#}", runner.run_step(&step).unwrap_err());

        assert!(
            error.contains("without leaving its document stamped"),
            "{error}"
        );
        assert_eq!(
            runner.store.task_state(step.id()),
            Some(MigrationState::Failed)
        );
    }

    struct RemovesProfiles;

    impl MigrationStep for RemovesProfiles {
        fn id(&self) -> &'static str {
            "profiles/removes"
        }

        fn module(&self) -> &'static str {
            "profiles"
        }

        fn revision(&self) -> u64 {
            1
        }

        fn introduced_in(&self) -> &'static Version {
            &TEST_VERSION
        }

        fn name(&self) -> &'static str {
            "RemovesProfiles"
        }

        fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
            std::fs::remove_file(ctx.profiles_path())?;
            Ok(())
        }
    }

    #[test]
    fn document_step_that_removes_its_document_is_a_defect() {
        let case = DocumentCase::new(Some(profiles_at(0, false)), CLEAN_PROFILES);
        let mut runner = case.runner().unwrap();

        let error = format!("{:#}", runner.run_step(&RemovesProfiles).unwrap_err());

        assert!(
            error.contains("without leaving its document stamped"),
            "{error}"
        );
        assert_eq!(runner.store.module_state("profiles").applied_revision, 0);
    }

    #[test]
    fn document_steps_do_not_run_without_their_document() {
        let mut runner = runner_with_store(profiles_at(0, false), false);
        let step = TestStep {
            id: "profiles/example",
            module: "profiles",
            revision: 1,
            fail: true,
        };

        runner.run_step(&step).unwrap();

        assert_eq!(
            runner.store.task_state(step.id()),
            Some(MigrationState::Skipped)
        );
    }

    #[test]
    fn a_completed_step_records_the_stamp_it_wrote() {
        let legacy = "current: [a]\nitems:\n- {uid: a, type: local, name: A, file: a.yaml}\n";
        let case = DocumentCase::new(Some(profiles_at(3, false)), legacy);
        let mut runner = case.runner().unwrap();
        let step = registry::find_migration("profiles/repair_schema").unwrap();

        runner.run_migration(step).unwrap();

        let state = MigrationStore::load(&case.ctx.state_path()).unwrap();
        assert!(state.module_state("profiles").stamped);
        assert_eq!(state.task_state(step.id()), Some(MigrationState::Completed));
        assert!(state.tasks[step.id()].started_at.is_some());
        // So losing the stamp afterwards is refused rather than taken for a
        // file that never had one.
        let payload = nyanpasu_core::format::inspect(&case.profiles())
            .unwrap()
            .into_payload();
        std::fs::write(
            case.ctx.profiles_path(),
            serde_yaml::to_string(&payload).unwrap(),
        )
        .unwrap();
        let error = format!("{:#}", case.runner().unwrap_err());
        assert!(error.contains("lost the `_nyanpasu` stamp"), "{error}");
    }

    #[test]
    fn a_stamp_found_at_startup_is_recorded() {
        let case = DocumentCase::new(
            Some(profiles_at(4, false)),
            &stamped_profiles(4, CLEAN_PROFILES),
        );

        case.runner().unwrap();

        let state = MigrationStore::load(&case.ctx.state_path()).unwrap();
        assert!(state.module_state("profiles").stamped);
    }

    #[test]
    fn profiles_at_head_adopt_a_stamp_without_changing_content() {
        let case = DocumentCase::new(Some(profiles_at(4, false)), CLEAN_PROFILES);

        let mut runner = case.runner().unwrap();
        runner.run_pending().unwrap();

        assert_eq!(case.stamp(), Some(4));
        let before: serde_yaml::Mapping = serde_yaml::from_str(CLEAN_PROFILES).unwrap();
        let after = nyanpasu_core::format::inspect(&case.profiles())
            .unwrap()
            .into_payload();
        assert_eq!(after, before);
        let state = MigrationStore::load(&case.ctx.state_path()).unwrap();
        assert!(state.module_state("profiles").stamped);
    }
}
