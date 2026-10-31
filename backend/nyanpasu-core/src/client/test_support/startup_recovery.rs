//! Startup recovery fixtures shared by application and filesystem integration tests.

use crate::migration::Runner;
use nyanpasu_paths::PathResolver;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn evidence(path: &Path) -> Vec<PathBuf> {
    let prefix = format!("{}.corrupt-", path.file_name().unwrap().to_string_lossy());
    fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&prefix)
        })
        .collect()
}

pub fn migrate(paths: &PathResolver) {
    Runner::with_paths(
        paths.clone(),
        crate::client::bundle_metadata::TEST_BUNDLE_METADATA,
        false,
    )
    .unwrap()
    .run_pending()
    .unwrap();
}
