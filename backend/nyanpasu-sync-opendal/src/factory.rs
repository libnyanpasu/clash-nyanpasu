//! Builds a store for each provider kind compiled into this build.

use std::sync::Arc;

use nyanpasu_contracts::sync::{
    ProviderConfig, ProviderKind, ProviderSecrets, RemoteError, RemoteStore, RemoteStoreFactory,
};
use opendal::{
    HttpTransporter, OperationContext, Operator,
    layers::{RetryLayer, TimeoutLayer},
    services,
};
use opendal_http_transport_reqwest::ReqwestTransport;

use crate::store::{OpendalRemoteStore, map_error};

/// The directory a vault occupies below the root the user chose, so that
/// pointing sync at an existing folder never lists or removes foreign objects.
const VAULT_DIRECTORY: &str = "nyanpasu-sync";

/// Opens OpenDAL-backed stores that send their requests through one HTTP client.
#[derive(Debug, Clone)]
pub struct OpendalStoreFactory {
    http: reqwest::Client,
}

impl OpendalStoreFactory {
    /// `http` carries the shell's proxy and TLS policy; every store this
    /// factory opens uses it and no process-wide transport.
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }
}

impl RemoteStoreFactory for OpendalStoreFactory {
    fn kinds(&self) -> Vec<ProviderKind> {
        [
            (cfg!(feature = "webdav"), ProviderKind::Webdav),
            (cfg!(feature = "s3"), ProviderKind::S3),
            (cfg!(feature = "fs"), ProviderKind::Fs),
        ]
        .into_iter()
        .filter_map(|(compiled, kind)| compiled.then_some(kind))
        .collect()
    }

    fn open(
        &self,
        config: &ProviderConfig,
        secrets: &ProviderSecrets,
    ) -> Result<Arc<dyn RemoteStore>, RemoteError> {
        let operator = build(config, secrets)?
            .with_context(
                OperationContext::new().with_http_transport(HttpTransporter::new(
                    ReqwestTransport::new(self.http.clone()),
                )),
            )
            .layer(TimeoutLayer::new())
            .layer(RetryLayer::new().with_jitter());
        Ok(Arc::new(OpendalRemoteStore::new(operator)))
    }
}

fn vault_root(root: &str) -> String {
    format!("{}/{VAULT_DIRECTORY}/", root.trim_end_matches('/'))
}

fn build(config: &ProviderConfig, secrets: &ProviderSecrets) -> Result<Operator, RemoteError> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut set = |key: &str, value: &str| pairs.push((key.to_owned(), value.to_owned()));
    let secret = |name: &str| secrets.get(name).map(|secret| secret.expose_secret());
    let operator = match config {
        #[cfg(feature = "webdav")]
        ProviderConfig::Webdav {
            endpoint,
            root,
            username,
        } => {
            set("endpoint", endpoint.as_str());
            set("root", &vault_root(root));
            if let Some(username) = username {
                set("username", username);
            }
            if let Some(password) = secret("password") {
                set("password", password);
            }
            Operator::from_iter::<services::Webdav>(pairs)
        }
        #[cfg(feature = "s3")]
        ProviderConfig::S3 {
            endpoint,
            region,
            bucket,
            root,
            access_key_id,
            virtual_host_style,
        } => {
            let secret_access_key =
                secret("secret_access_key").ok_or(RemoteError::MissingSetting {
                    field: "secret_access_key",
                })?;
            if let Some(endpoint) = endpoint {
                set("endpoint", endpoint.as_str());
            }
            set("region", region);
            set("bucket", bucket);
            set("root", &vault_root(root));
            set("access_key_id", access_key_id);
            set("secret_access_key", secret_access_key);
            set("enable_virtual_host_style", &virtual_host_style.to_string());
            // Credentials come from the settings only, never from the
            // environment or an AWS profile of whoever runs the application.
            set("disable_config_load", "true");
            Operator::from_iter::<services::S3>(pairs)
        }
        #[cfg(feature = "fs")]
        ProviderConfig::Fs { root } => {
            set("root", &vault_root(root));
            Operator::from_iter::<services::Fs>(pairs)
        }
        #[cfg(not(all(feature = "webdav", feature = "s3", feature = "fs")))]
        _ => {
            return Err(RemoteError::Unsupported {
                operation: "this provider kind in this build",
            });
        }
    };
    operator.map_err(|error| map_error(error, ""))
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use nyanpasu_contracts::{auth::SecretString, sync::PutCondition};
    use url::Url;

    use super::*;

    fn factory() -> OpendalStoreFactory {
        OpendalStoreFactory::new(reqwest::Client::new())
    }

    #[test]
    fn reports_the_compiled_provider_kinds() {
        assert_eq!(
            factory().kinds(),
            [ProviderKind::Webdav, ProviderKind::S3, ProviderKind::Fs]
        );
    }

    #[tokio::test]
    async fn a_filesystem_vault_lives_in_its_own_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("foreign.txt"), b"not ours").unwrap();
        let config = ProviderConfig::Fs {
            root: dir.path().to_str().unwrap().to_owned(),
        };
        let store = factory()
            .open(&config, &ProviderSecrets::default())
            .unwrap();
        store
            .put("format.json", Bytes::from_static(b"{}"), PutCondition::Any)
            .await
            .unwrap();
        assert!(dir.path().join("nyanpasu-sync/format.json").is_file());
        let listed = store.list("").await.unwrap();
        assert_eq!(
            listed.len(),
            1,
            "foreign files are outside the vault: {listed:?}"
        );
    }

    #[test]
    fn network_providers_open_without_contacting_them() {
        let webdav = ProviderConfig::Webdav {
            endpoint: Url::parse("https://dav.invalid/remote.php/dav").unwrap(),
            root: "/sync".to_owned(),
            username: Some("user".to_owned()),
        };
        let password = ProviderSecrets::default().with("password", SecretString::new("pw"));
        let store = factory().open(&webdav, &password).unwrap();
        assert!(
            !store.capabilities().create_if_absent,
            "WebDAV has no atomic create"
        );

        let s3 = ProviderConfig::S3 {
            endpoint: Some(Url::parse("https://s3.invalid").unwrap()),
            region: "auto".to_owned(),
            bucket: "bucket".to_owned(),
            root: "/".to_owned(),
            access_key_id: "id".to_owned(),
            virtual_host_style: false,
        };
        assert!(matches!(
            factory().open(&s3, &ProviderSecrets::default()),
            Err(RemoteError::MissingSetting {
                field: "secret_access_key"
            })
        ));
        let key = ProviderSecrets::default().with("secret_access_key", SecretString::new("k"));
        let store = factory().open(&s3, &key).unwrap();
        assert!(store.capabilities().create_if_absent);
    }
}
