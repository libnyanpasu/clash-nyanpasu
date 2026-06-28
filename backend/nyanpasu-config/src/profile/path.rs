use std::{
    fmt,
    path::{Component, Path},
};

use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use specta::Type;
use thiserror::Error;

/// A path relative to the application-managed profile directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Type)]
#[serde(transparent)]
#[repr(transparent)]
pub struct ManagedProfilePath(String);

impl ManagedProfilePath {
    pub fn new(value: impl Into<String>) -> Result<Self, ProfilePathError> {
        let value = value.into();
        let path = Path::new(&value);

        if value.is_empty() {
            return Err(ProfilePathError::Empty);
        }
        if looks_like_url(&value) {
            return Err(ProfilePathError::ManagedMustNotBeUrl(value));
        }
        if path.is_absolute() {
            return Err(ProfilePathError::ManagedMustBeRelative(value));
        }
        if path.components().any(|component| {
            matches!(
                component,
                Component::Prefix(_)
                    | Component::RootDir
                    | Component::ParentDir
                    | Component::CurDir
            )
        }) {
            return Err(ProfilePathError::ManagedContainsTraversal(value));
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl fmt::Display for ManagedProfilePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ManagedProfilePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

/// An absolute path outside the managed profile directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Type)]
#[serde(transparent)]
#[repr(transparent)]
pub struct ExternalProfilePath(String);

impl ExternalProfilePath {
    pub fn new(value: impl Into<String>) -> Result<Self, ProfilePathError> {
        let value = value.into();
        let path = Path::new(&value);

        if value.is_empty() {
            return Err(ProfilePathError::Empty);
        }
        // Accept both native absolute paths and Unix-style absolute paths so
        // that profiles written on Linux/macOS are portable to Windows.
        if !path.is_absolute() && !value.starts_with('/') {
            return Err(ProfilePathError::ExternalMustBeAbsolute(value));
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl fmt::Display for ExternalProfilePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ExternalProfilePath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProfilePathError {
    #[error("profile path cannot be empty")]
    Empty,

    #[error("managed profile path must be relative: {0}")]
    ManagedMustBeRelative(String),

    #[error("managed profile path cannot be a URL: {0}")]
    ManagedMustNotBeUrl(String),

    #[error("managed profile path cannot contain root, current or parent components: {0}")]
    ManagedContainsTraversal(String),

    #[error("external profile path must be absolute: {0}")]
    ExternalMustBeAbsolute(String),
}

fn looks_like_url(value: &str) -> bool {
    value.contains("://")
}
