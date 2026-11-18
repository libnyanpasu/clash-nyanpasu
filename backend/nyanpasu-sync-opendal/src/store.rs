//! [`RemoteStore`] over one OpenDAL operator.

use bytes::Bytes;
use nyanpasu_contracts::sync::{
    PutCondition, RemoteCapabilities, RemoteEntry, RemoteError, RemoteStore, validate_key,
};
use opendal::{ErrorKind, Operator};

/// The objects under one operator's root.
#[derive(Debug, Clone)]
pub struct OpendalRemoteStore {
    operator: Operator,
}

impl OpendalRemoteStore {
    /// Wraps an operator whose root is the vault root.
    pub fn new(operator: Operator) -> Self {
        Self { operator }
    }
}

#[async_trait::async_trait]
impl RemoteStore for OpendalRemoteStore {
    fn capabilities(&self) -> RemoteCapabilities {
        RemoteCapabilities {
            create_if_absent: self.operator.info().capability().write_with_if_not_exists,
        }
    }

    async fn check(&self) -> Result<(), RemoteError> {
        self.operator
            .check()
            .await
            .map_err(|error| map_error(error, ""))
    }

    async fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        // OpenDAL lists a directory. List the one that contains the prefix and
        // keep the keys that start with it.
        let directory = prefix.rfind('/').map_or("", |end| &prefix[..=end]);
        if !directory.is_empty() {
            validate_key(directory.trim_end_matches('/'))?;
        }
        let entries = match self.operator.list_with(directory).recursive(true).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(map_error(error, directory)),
        };
        Ok(entries
            .into_iter()
            .filter(|entry| entry.metadata().is_file() && entry.path().starts_with(prefix))
            .map(|entry| RemoteEntry {
                key: entry.path().to_owned(),
                size: entry.metadata().content_length(),
            })
            .collect())
    }

    async fn get(&self, key: &str) -> Result<Bytes, RemoteError> {
        validate_key(key)?;
        self.operator
            .read(key)
            .await
            .map(|buffer| buffer.to_bytes())
            .map_err(|error| map_error(error, key))
    }

    async fn put(
        &self,
        key: &str,
        body: Bytes,
        condition: PutCondition,
    ) -> Result<(), RemoteError> {
        validate_key(key)?;
        let if_absent = condition == PutCondition::IfAbsent;
        if if_absent && !self.capabilities().create_if_absent {
            return Err(RemoteError::Unsupported {
                operation: "create if absent",
            });
        }
        self.operator
            .write_with(key, body)
            .if_not_exists(if_absent)
            .await
            .map(drop)
            .map_err(|error| map_error(error, key))
    }

    async fn delete(&self, key: &str) -> Result<(), RemoteError> {
        validate_key(key)?;
        match self.operator.delete(key).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(map_error(error, key)),
        }
    }
}

/// The one place an OpenDAL error becomes a port error.
pub(crate) fn map_error(error: opendal::Error, key: &str) -> RemoteError {
    let key = key.to_owned();
    match error.kind() {
        ErrorKind::NotFound => RemoteError::NotFound { key },
        ErrorKind::ConditionNotMatch | ErrorKind::AlreadyExists => {
            RemoteError::AlreadyExists { key }
        }
        ErrorKind::PermissionDenied => RemoteError::Unauthorized {
            source: error.into(),
        },
        ErrorKind::Unsupported => RemoteError::Unsupported {
            operation: "this storage operation",
        },
        ErrorKind::RateLimited => RemoteError::Unavailable {
            source: error.into(),
        },
        _ if error.is_temporary() => RemoteError::Unavailable {
            source: error.into(),
        },
        _ => RemoteError::Other {
            source: error.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use nyanpasu_contracts::sync::test_support::assert_remote_store_contract;
    use opendal::{Error, services};

    use super::*;

    fn pairs(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[tokio::test]
    async fn the_memory_service_passes_the_port_contract() {
        let operator = Operator::from_iter::<services::Memory>(pairs(&[])).unwrap();
        assert_remote_store_contract(&OpendalRemoteStore::new(operator)).await;
    }

    #[cfg(feature = "fs")]
    #[tokio::test]
    async fn the_filesystem_service_passes_the_port_contract() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_str().unwrap();
        let operator = Operator::from_iter::<services::Fs>(pairs(&[("root", root)])).unwrap();
        let store = OpendalRemoteStore::new(operator);
        assert!(store.capabilities().create_if_absent);
        assert_remote_store_contract(&store).await;
    }

    #[test]
    fn error_kinds_map_to_port_errors() {
        let map = |kind| map_error(Error::new(kind, "x"), "k");
        assert!(matches!(map(ErrorKind::NotFound), RemoteError::NotFound { key } if key == "k"));
        assert!(matches!(
            map(ErrorKind::ConditionNotMatch),
            RemoteError::AlreadyExists { .. }
        ));
        assert!(matches!(
            map(ErrorKind::AlreadyExists),
            RemoteError::AlreadyExists { .. }
        ));
        assert!(matches!(
            map(ErrorKind::PermissionDenied),
            RemoteError::Unauthorized { .. }
        ));
        assert!(matches!(
            map(ErrorKind::RateLimited),
            RemoteError::Unavailable { .. }
        ));
        assert!(matches!(
            map(ErrorKind::Unsupported),
            RemoteError::Unsupported { .. }
        ));
        assert!(matches!(
            map(ErrorKind::Unexpected),
            RemoteError::Other { .. }
        ));
        assert!(matches!(
            map_error(Error::new(ErrorKind::Unexpected, "x").set_temporary(), "k"),
            RemoteError::Unavailable { .. }
        ));
    }
}
