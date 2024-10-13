use super::{DynMigration, Migration, Unit};
use once_cell::sync::Lazy;
use semver::Version;
use std::{borrow::Cow, collections::HashMap};

mod unit_160;
mod unit_200;

pub static UNITS: Lazy<HashMap<&'static Version, Unit<'static, DynMigration>>> = Lazy::new(|| {
    let mut units: HashMap<&'static Version, Unit<'static, DynMigration>> = HashMap::new();
    let unit = Unit::Batch(Cow::Borrowed(&unit_160::UNITS));
    units.insert(unit.version(), unit);
    let unit = Unit::Batch(Cow::Borrowed(&unit_200::UNITS));
    units.insert(unit.version(), unit);
    units
});

pub fn find_migration(name: &str) -> Option<Cow<'static, DynMigration<'static>>> {
    for unit in UNITS.values() {
        match unit {
            Unit::Batch(units) => {
                for unit in units.iter() {
                    if unit.name() == name {
                        return Some(Cow::Borrowed(unit));
                    }
                }
            }
            Unit::Single(unit) => {
                if unit.name() == name {
                    return Some(Cow::Borrowed(unit));
                }
            }
        }
    }
    None
}

pub fn get_migrations() -> Vec<Cow<'static, DynMigration<'static>>> {
    let mut migrations = Vec::new();
    for unit in UNITS.values() {
        match unit {
            Unit::Batch(units) => {
                for unit in units.iter() {
                    migrations.push(Cow::Borrowed(unit));
                }
            }
            Unit::Single(unit) => {
                migrations.push(Cow::Borrowed(unit));
            }
        }
    }
    migrations
}
