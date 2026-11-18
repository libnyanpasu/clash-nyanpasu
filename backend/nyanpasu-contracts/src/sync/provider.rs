//! Provider settings and their secrets.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};
use specta::Type;
use url::Url;

use crate::auth::SecretString;

/// Where a vault lives. Holds no secret: those travel as [`ProviderSecrets`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderConfig {
    Webdav {
        endpoint: Url,
        /// Directory on the server that contains the vault.
        root: String,
        username: Option<String>,
    },
    S3 {
        /// `None` selects the AWS endpoint of `region`.
        endpoint: Option<Url>,
        region: String,
        bucket: String,
        root: String,
        access_key_id: String,
        virtual_host_style: bool,
    },
    /// A directory on this machine, such as a mounted share.
    Fs { root: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Webdav,
    S3,
    Fs,
}

impl ProviderConfig {
    pub fn kind(&self) -> ProviderKind {
        match self {
            Self::Webdav { .. } => ProviderKind::Webdav,
            Self::S3 { .. } => ProviderKind::S3,
            Self::Fs { .. } => ProviderKind::Fs,
        }
    }
}

/// The secrets of one provider, by the name the provider documents:
/// `password` for WebDAV, `secret_access_key` for S3.
///
/// Not serializable and redacted in `Debug`, so it cannot reach a settings
/// file, a DTO or a log line.
#[derive(Clone, Default)]
pub struct ProviderSecrets(BTreeMap<String, SecretString>);

impl ProviderSecrets {
    #[must_use]
    pub fn with(mut self, name: &str, secret: SecretString) -> Self {
        self.0.insert(name.to_owned(), secret);
        self
    }

    pub fn get(&self, name: &str) -> Option<&SecretString> {
        self.0.get(name)
    }

    /// The names this record holds, without their values.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// Whether no secret is stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Encodes the record for the [`SecretStore`](super::SecretStore).
    ///
    /// This is the only serialized form of a secret, and only the secret store
    /// may call it: the settings document never contains these values.
    ///
    /// # Errors
    ///
    /// Fails only if the record cannot be encoded as JSON.
    pub fn encode(&self) -> Result<Vec<u8>, serde_json::Error> {
        let values: BTreeMap<&str, &str> = self
            .0
            .iter()
            .map(|(name, secret)| (name.as_str(), secret.expose_secret()))
            .collect();
        serde_json::to_vec(&values)
    }

    /// Decodes a record written by [`ProviderSecrets::encode`].
    ///
    /// # Errors
    ///
    /// Fails when the bytes are not a JSON object of string values.
    pub fn decode(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        let values: BTreeMap<String, String> = serde_json::from_slice(bytes)?;
        Ok(Self(
            values
                .into_iter()
                .map(|(name, secret)| (name, SecretString::new(secret)))
                .collect(),
        ))
    }
}

impl fmt::Debug for ProviderSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.0.keys()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_never_print_their_values() {
        let secrets = ProviderSecrets::default().with("password", SecretString::new("hunter2"));
        let printed = format!("{secrets:?}");
        assert!(printed.contains("password"));
        assert!(!printed.contains("hunter2"));
    }

    #[test]
    fn provider_config_round_trips_with_a_kind_tag() {
        let config = ProviderConfig::Webdav {
            endpoint: Url::parse("https://dav.example.com/remote.php/dav").unwrap(),
            root: "/nyanpasu".to_owned(),
            username: Some("keiko".to_owned()),
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["kind"], "webdav");
        assert_eq!(
            serde_json::from_value::<ProviderConfig>(json).unwrap(),
            config
        );
        assert_eq!(config.kind(), ProviderKind::Webdav);
    }

    #[test]
    fn secrets_round_trip_through_their_stored_form() {
        let secrets = ProviderSecrets::default()
            .with("password", SecretString::new("hunter2"))
            .with("secret_access_key", SecretString::new("key"));
        let decoded = ProviderSecrets::decode(&secrets.encode().unwrap()).unwrap();
        assert_eq!(
            decoded.get("password").map(SecretString::expose_secret),
            Some("hunter2")
        );
        assert_eq!(
            decoded
                .get("secret_access_key")
                .map(SecretString::expose_secret),
            Some("key")
        );
        assert_eq!(
            decoded.names().collect::<Vec<_>>(),
            ["password", "secret_access_key"]
        );
    }
}
