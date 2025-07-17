#![allow(dead_code)]
/// A migration mod indicates the migration of the old version to the new version.
/// Because this runner run at the start of the app, it will use eprintln or println to print the migration log.
///
///
use dyn_clone::{DynClone, clone_trait_object};
use semver::Version;
use std::{borrow::Cow, cell::RefCell};

mod db;
pub mod units;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationState {
    /// The migration is pending.
    NotStarted,
    /// The migration is in progress.
    InProgress,
    /// The migration is completed.
    Completed,
    /// The migration is failed.
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationAdvice {
    /// The migration is required to run.
    Pending,
    /// The migration is ignored.
    Ignored,
    /// The migration has been run.
    Done,
}

impl std::fmt::Display for MigrationAdvice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MigrationAdvice::Pending => write!(f, "Pending"),
            MigrationAdvice::Ignored => write!(f, "Ignored"),
            MigrationAdvice::Done => write!(f, "Done"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Unit<'a, T>
where
    T: Clone + Migration<'a> + Send + Sync,
{
    /// A List of migrations, it should be used to wrap a list of migrations in a single version.
    /// Although the fn signature is T generic, it should use a Vec<DynMigration> as the input.
    Batch(Cow<'a, [T]>),
    Single(Cow<'a, T>),
}

impl<'a, T> From<T> for Unit<'a, T>
where
    T: Clone + Migration<'a> + Send + Sync,
{
    fn from(item: T) -> Self {
        Unit::Single(Cow::Owned(item))
    }
}

impl<'a, T> From<&'a T> for Unit<'a, T>
where
    T: Clone + Migration<'a> + Send + Sync,
{
    fn from(item: &'a T) -> Self {
        Unit::Single(Cow::Borrowed(item))
    }
}

impl<'a, T> From<&'a [T]> for Unit<'a, T>
where
    T: Clone + Migration<'a> + Send + Sync,
{
    fn from(list: &'a [T]) -> Self {
        Unit::Batch(Cow::Borrowed(list))
    }
}

impl<'a, T> From<Vec<T>> for Unit<'a, T>
where
    T: Clone + Migration<'a> + Send + Sync,
{
    fn from(list: Vec<T>) -> Self {
        Unit::Batch(Cow::Owned(list))
    }
}

type DynMigration<'a> = Box<dyn Migration<'a> + Send + Sync + 'a>;

pub trait Migration<'a>: DynClone {
    /// A version field to indicate the version of the migration.
    /// It used to compare with the current version to determine whether the migration is needed.
    fn version(&self) -> &'a Version;

    /// A name field to indicate the name of the migration.
    fn name(&self) -> Cow<'a, str>;

    fn migrate(&self) -> std::io::Result<()> {
        unimplemented!()
    }

    fn discard(&self) -> std::io::Result<()> {
        Ok(())
    }
}

clone_trait_object!(Migration<'_>);

pub trait MigrationExt<'a>: Migration<'a>
where
    Self: Sized + 'static + Send + Sync,
{
    fn boxed(self) -> DynMigration<'a> {
        Box::new(self) as DynMigration
    }
}

impl<'a, T> MigrationExt<'a> for T where T: Sized + 'static + Migration<'a> + Send + Sync {}

impl<'a, T> Migration<'a> for Unit<'a, T>
where
    T: Clone + Migration<'a> + Send + Sync,
{
    fn version(&self) -> &'a Version {
        match self {
            Unit::Single(item) => item.version(),
            Unit::Batch(list) => list.first().unwrap().version(),
        }
    }

    fn name(&self) -> Cow<'a, str> {
        match self {
            Unit::Single(item) => item.name(),
            Unit::Batch(list) => Cow::Owned(format!(
                "{} migrations for v{}",
                list.len(),
                list.first().unwrap().version()
            )),
        }
    }

    fn migrate(&self) -> std::io::Result<()> {
        unimplemented!("Batch migrations should be handled by the runner.")
    }
}

impl<'a> Migration<'a> for DynMigration<'a> {
    fn version(&self) -> &'a Version {
        self.as_ref().version()
    }

    fn name(&self) -> Cow<'a, str> {
        self.as_ref().name()
    }

    fn migrate(&self) -> std::io::Result<()> {
        self.as_ref().migrate()
    }
}

#[derive(Debug)]
pub struct Runner<'a> {
    pub current_version: Cow<'a, Version>,
    skip_advice: bool,
    store: RefCell<db::MigrationFile<'a>>,
}

impl Default for Runner<'_> {
    fn default() -> Self {
        let ver = Version::parse(crate::consts::BUILD_INFO.pkg_version).unwrap();
        let file = db::MigrationFileBuilder::default()
            .read_file()
            .build()
            .unwrap();
        Self {
            current_version: Cow::Owned(ver),
            skip_advice: false,
            store: RefCell::new(file),
        }
    }
}

impl Drop for Runner<'_> {
    fn drop(&mut self) {
        let mut store = self.store.take();
        store.version = Cow::Borrowed(&self.current_version);
        store.write_file().unwrap();
    }
}

impl Runner<'_> {
    pub fn new_with_skip_advice() -> Self {
        let ver = Version::parse(crate::consts::BUILD_INFO.pkg_version).unwrap();
        let file = db::MigrationFileBuilder::default()
            .read_file()
            .build()
            .unwrap();
        Self {
            skip_advice: true,
            current_version: Cow::Owned(ver),
            store: RefCell::new(file),
        }
    }

    pub fn advice_migration<'a, T>(&self, migration: &T) -> MigrationAdvice
    where
        T: Clone + Migration<'a> + Send + Sync,
    {
        let migration_ver = migration.version();
        let store = self.store.borrow();
        if migration_ver >= &store.version {
            // Judge the migration is run or not.
            if let Some(state) = store.states.get(&migration.name()) {
                match state {
                    MigrationState::Completed => MigrationAdvice::Done,
                    MigrationState::Failed => MigrationAdvice::Pending,
                    _ => MigrationAdvice::Ignored,
                }
            } else {
                MigrationAdvice::Pending
            }
        } else {
            MigrationAdvice::Ignored
        }
    }

    pub fn advice_unit<'a, T>(&self, unit: &Unit<'a, T>) -> MigrationAdvice
    where
        T: Clone + Migration<'a> + Send + Sync,
    {
        match unit {
            Unit::Single(item) => self.advice_migration(item.as_ref()),
            Unit::Batch(list) => {
                let mut advice = MigrationAdvice::Ignored;
                for item in list.iter() {
                    let item_advice = self.advice_migration(item);
                    if item_advice == MigrationAdvice::Pending {
                        advice = MigrationAdvice::Pending;
                        break;
                    } else if item_advice == MigrationAdvice::Done {
                        advice = MigrationAdvice::Done;
                    }
                }
                advice
            }
        }
    }

    pub fn run_migration<'a, T>(&self, migration: &T) -> std::io::Result<()>
    where
        T: Clone + Migration<'a> + Send + Sync,
    {
        println!("Running migration: {}", migration.name());
        let advice = self.advice_migration(migration);
        println!("Advice: {:?}", advice);
        if matches!(advice, MigrationAdvice::Ignored | MigrationAdvice::Done) {
            return Ok(());
        }
        let name = migration.name();
        let mut store = self.store.borrow_mut();
        match migration.migrate() {
            Ok(_) => {
                println!("Migration {} completed.", name);
                store.set_state(Cow::Owned(name.to_string()), MigrationState::Completed);
                Ok(())
            }
            Err(e) => {
                eprintln!(
                    "Migration {} failed: {}; trying to discard changes",
                    name, e
                );
                match migration.discard() {
                    Ok(_) => {
                        eprintln!("Migration {} discarded.", name);
                    }
                    Err(e) => {
                        eprintln!("Migration {} discard failed: {}", name, e);
                    }
                }
                store.set_state(Cow::Owned(name.to_string()), MigrationState::Failed);
                Err(e)
            }
        }
    }

    pub fn run_unit<'a, T>(&self, unit: &Unit<'a, T>) -> std::io::Result<()>
    where
        T: Clone + Migration<'a> + Send + Sync,
    {
        println!("Running unit: {}", unit.name());
        match unit {
            Unit::Single(item) => self.run_migration(item.as_ref()),
            Unit::Batch(list) => {
                for item in list.iter() {
                    self.run_migration(item)?;
                }
                Ok(())
            }
        }
    }

    pub fn run_units_up_to_version(&self, to_ver: &Version) -> std::io::Result<()> {
        println!("Running units up to version: {}", to_ver);
        let version = {
            let store = self.store.borrow();
            store.version.clone()
        };
        let units = units::UNITS
            .iter()
            .filter(|(ver, _)| **ver >= &version && **ver <= to_ver);
        for (_, unit) in units {
            self.run_unit(unit)?;
        }
        Ok(())
    }
    pub fn run_upcoming_units(&self) -> std::io::Result<()> {
        println!(
            "Running all upcoming units. It is supposed to run in Nightly build. If you see this message in Stable channel, report it in Github Issues Tracker please."
        );
        let version = {
            let store = self.store.borrow();
            store.version.clone()
        };
        let units = units::UNITS.iter().filter(|(ver, _)| **ver >= &version);

        for (_, unit) in units {
            self.run_unit(unit)?;
        }
        Ok(())
    }
}
