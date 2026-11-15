//! `cli.toml`: named contexts, kubectl-style, under the user config directory.
//!
//! A context names one target (a local socket or an HTTP URL) and references where
//! its token lives. The file never holds a secret.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::CliError;

/// Where the CLI keeps its own state.
#[derive(Debug, Clone)]
pub struct CliPaths {
    /// The `cli.toml` file.
    pub config_file: PathBuf,
    /// The directory of `0600` token files used when no OS keyring is available.
    pub token_dir: PathBuf,
}

impl CliPaths {
    /// Resolves the user config directory, or uses an explicit `cli.toml` location.
    ///
    /// # Errors
    /// Fails when the platform has no user config directory and none was given.
    pub fn discover(config_file: Option<PathBuf>) -> Result<Self, CliError> {
        let config_file = match config_file {
            Some(path) => path,
            None => dirs::config_dir()
                .ok_or_else(|| CliError::failed("the user config directory is unknown"))?
                .join("nyanpasu-cli")
                .join("cli.toml"),
        };
        let token_dir = config_file
            .parent()
            .map_or_else(|| PathBuf::from("tokens"), |parent| parent.join("tokens"));
        Ok(Self {
            config_file,
            token_dir,
        })
    }
}

/// Where a context's token is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenBackend {
    /// The OS keychain.
    Keyring,
    /// A `0600` file next to `cli.toml`.
    File,
}

/// The target kind of a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextKind {
    /// The same-user local socket or named pipe.
    LocalSocket,
    /// An HTTP(S) base URL.
    Http,
}

/// One named target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextConfig {
    /// The target kind.
    pub kind: ContextKind,
    /// Overrides the default socket path of a `local_socket` context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<PathBuf>,
    /// The base URL of an `http` context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Where the token of an `http` context was stored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<TokenBackend>,
}

/// The contents of `cli.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliConfig {
    /// The context used when no `--context` or `NYANPASU_URL` selects one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_context: Option<String>,
    /// All contexts by name.
    #[serde(default)]
    pub contexts: BTreeMap<String, ContextConfig>,
}

/// A context name is also a file name, so it stays a short portable identifier.
///
/// # Errors
/// Names outside `[A-Za-z0-9._-]{1,64}`, or `.`/`..`, are a usage error.
pub fn validate_context_name(name: &str) -> Result<(), CliError> {
    let valid = !name.is_empty()
        && name.len() <= 64
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    if valid {
        Ok(())
    } else {
        Err(CliError::usage(format!(
            "invalid context name `{name}`: use 1 to 64 letters, digits, `.`, `_` or `-`"
        )))
    }
}

impl CliConfig {
    /// Reads `cli.toml`; a missing file is an empty configuration.
    ///
    /// # Errors
    /// An unreadable or malformed file is a failure; it is never replaced silently.
    pub fn load(path: &Path) -> Result<Self, CliError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(CliError::failed(format!(
                    "cannot read {}: {error}",
                    path.display()
                )));
            }
        };
        toml::from_str(&text)
            .map_err(|error| CliError::failed(format!("cannot parse {}: {error}", path.display())))
    }

    /// Writes `cli.toml` through a temporary file so a crash never truncates it.
    ///
    /// # Errors
    /// Reports directory creation, serialization or write failures.
    pub fn save(&self, path: &Path) -> Result<(), CliError> {
        let text = toml::to_string_pretty(self)
            .map_err(|error| CliError::failed(format!("cannot encode cli.toml: {error}")))?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                CliError::failed(format!("cannot create {}: {error}", parent.display()))
            })?;
        }
        let temporary = path.with_extension("toml.tmp");
        fs::write(&temporary, text).map_err(|error| {
            CliError::failed(format!("cannot write {}: {error}", temporary.display()))
        })?;
        fs::rename(&temporary, path).map_err(|error| {
            CliError::failed(format!("cannot replace {}: {error}", path.display()))
        })
    }

    /// Adds or replaces a context. The first context becomes current.
    ///
    /// # Errors
    /// An invalid name or an inconsistent context is a usage error.
    pub fn add(&mut self, name: &str, context: ContextConfig) -> Result<(), CliError> {
        validate_context_name(name)?;
        match (context.kind, &context.url, &context.endpoint) {
            (ContextKind::Http, Some(_), None) | (ContextKind::LocalSocket, None, _) => {}
            (ContextKind::Http, None, _) => {
                return Err(CliError::usage("an http context needs a URL"));
            }
            (ContextKind::Http, Some(_), Some(_)) | (ContextKind::LocalSocket, Some(_), _) => {
                return Err(CliError::usage(
                    "a context is either a local socket or an HTTP URL, not both",
                ));
            }
        }
        self.contexts.insert(name.to_owned(), context);
        self.current_context.get_or_insert_with(|| name.to_owned());
        Ok(())
    }

    /// Makes an existing context the default.
    ///
    /// # Errors
    /// An unknown context is a usage error.
    pub fn use_context(&mut self, name: &str) -> Result<(), CliError> {
        if !self.contexts.contains_key(name) {
            return Err(unknown_context(name));
        }
        self.current_context = Some(name.to_owned());
        Ok(())
    }

    /// Removes a context and returns it. Removing the current one clears the default.
    ///
    /// # Errors
    /// An unknown context is a usage error.
    pub fn remove(&mut self, name: &str) -> Result<ContextConfig, CliError> {
        let removed = self
            .contexts
            .remove(name)
            .ok_or_else(|| unknown_context(name))?;
        if self.current_context.as_deref() == Some(name) {
            self.current_context = None;
        }
        Ok(removed)
    }
}

/// The usage error for a context that does not exist.
pub fn unknown_context(name: &str) -> CliError {
    CliError::usage(format!(
        "unknown context `{name}`; list contexts with `nyanpasu-cli context list`"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(url: &str) -> ContextConfig {
        ContextConfig {
            kind: ContextKind::Http,
            endpoint: None,
            url: Some(url.to_owned()),
            token: Some(TokenBackend::File),
        }
    }

    fn socket() -> ContextConfig {
        ContextConfig {
            kind: ContextKind::LocalSocket,
            endpoint: None,
            url: None,
            token: None,
        }
    }

    #[test]
    fn round_trips_through_toml_without_secrets() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("cli.toml");
        let mut config = CliConfig::default();
        config
            .add("laptop", http("http://127.0.0.1:9090/"))
            .unwrap();
        config.add("local", socket()).unwrap();
        config.save(&path).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("kind = \"http\""), "{text}");
        assert!(!text.to_lowercase().contains("secret"), "{text}");
        assert_eq!(CliConfig::load(&path).unwrap(), config);
        assert_eq!(config.current_context.as_deref(), Some("laptop"));
    }

    #[test]
    fn a_missing_file_is_empty_and_a_malformed_one_is_an_error() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cli.toml");
        assert_eq!(CliConfig::load(&path).unwrap(), CliConfig::default());
        fs::write(&path, "contexts = 3").unwrap();
        assert!(matches!(
            CliConfig::load(&path),
            Err(CliError::Failed { .. })
        ));
    }

    #[test]
    fn use_and_remove_follow_the_current_context() {
        let mut config = CliConfig::default();
        config.add("a", socket()).unwrap();
        config.add("b", http("http://h:1/")).unwrap();
        assert_eq!(config.current_context.as_deref(), Some("a"));

        config.use_context("b").unwrap();
        assert_eq!(config.current_context.as_deref(), Some("b"));
        assert!(matches!(
            config.use_context("missing"),
            Err(CliError::Usage { .. })
        ));

        config.remove("b").unwrap();
        assert_eq!(config.current_context, None);
        assert!(matches!(config.remove("b"), Err(CliError::Usage { .. })));
    }

    #[test]
    fn context_names_cannot_escape_the_token_directory() {
        for name in ["", ".", "..", "a/b", "a\\b", "x y", &"n".repeat(65)] {
            assert!(validate_context_name(name).is_err(), "{name:?}");
        }
        for name in ["work", "my.vps-1", "A_b"] {
            validate_context_name(name).unwrap();
        }
    }

    #[test]
    fn inconsistent_contexts_are_rejected() {
        let mut config = CliConfig::default();
        let mut both = http("http://h/");
        both.endpoint = Some(PathBuf::from("/tmp/x.sock"));
        assert!(config.add("both", both).is_err());
        let mut socket_url = socket();
        socket_url.url = Some("http://h/".to_owned());
        assert!(config.add("socket-url", socket_url).is_err());
        let mut no_url = http("http://h/");
        no_url.url = None;
        assert!(config.add("no-url", no_url).is_err());
    }
}
