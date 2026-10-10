use std::path::{Path, PathBuf};

use nyanpasu_config::profile::{ExternalProfilePath, ManagedProfilePath};
use serde::{Serialize, Serializer};

/// A filesystem path as it appears in an error. The lossy conversion keeps it
/// serializable when the path is not valid UTF-8.
#[derive(Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct ErrorPath(#[specta(type = String)] PathBuf);

impl Serialize for ErrorPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0.display())
    }
}

impl std::fmt::Display for ErrorPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.display().fmt(f)
    }
}

impl From<&Path> for ErrorPath {
    fn from(path: &Path) -> Self {
        Self(path.to_path_buf())
    }
}

impl From<PathBuf> for ErrorPath {
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

impl From<&PathBuf> for ErrorPath {
    fn from(path: &PathBuf) -> Self {
        Self(path.clone())
    }
}

impl From<&ManagedProfilePath> for ErrorPath {
    fn from(path: &ManagedProfilePath) -> Self {
        Self(path.as_path().to_path_buf())
    }
}

impl From<&ExternalProfilePath> for ErrorPath {
    fn from(path: &ExternalProfilePath) -> Self {
        Self(path.as_path().to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_conversions_keep_display_and_wire_strings() {
        let path = PathBuf::from("profiles/example.yaml");
        let expected = path.display().to_string();
        let managed = ManagedProfilePath::new("profiles/example.yaml").unwrap();
        for error_path in [
            ErrorPath::from(path.as_path()),
            ErrorPath::from(&path),
            ErrorPath::from(path.clone()),
            ErrorPath::from(&managed),
        ] {
            assert_eq!(error_path.to_string(), expected);
            assert_eq!(serde_json::to_value(error_path).unwrap(), expected);
        }
        let external = ExternalProfilePath::new("/external/example.yaml").unwrap();
        let error_path = ErrorPath::from(&external);
        assert_eq!(
            error_path.to_string(),
            external.as_path().display().to_string()
        );
        assert_eq!(serde_json::to_value(error_path).unwrap(), external.as_str());
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_path_serializes_lossily() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt};

        let path = PathBuf::from(OsString::from_vec(b"core-\xff".to_vec()));
        let error_path = ErrorPath::from(path);
        assert_eq!(error_path.to_string(), "core-\u{fffd}");
        assert_eq!(serde_json::to_value(error_path).unwrap(), "core-\u{fffd}");
    }
}
