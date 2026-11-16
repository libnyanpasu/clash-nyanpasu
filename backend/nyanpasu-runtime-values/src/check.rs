//! Failures that prevent candidate validation from returning a verdict.

/// Why candidate validation could not return a verdict.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "cause", rename_all = "snake_case")]
pub enum RuntimeCheckUnavailable {
    /// The host has the capability but could not serve it.
    Backend {
        kind: Option<nyanpasu_core_metadata::CoreErrorKind>,
        message: String,
        retryable: bool,
    },
}
