//! An in-memory [`RemoteStore`] with switchable faults, and the contract every
//! implementation of the port must pass.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
};

use bytes::Bytes;

use super::{
    PutCondition, RemoteCapabilities, RemoteEntry, RemoteError, RemoteStore, validate_key,
};

/// How the fake treats [`PutCondition::IfAbsent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionalCreate {
    /// Advertised and atomic, as on S3 or a local filesystem.
    Honored,
    /// Not advertised and refused, as on WebDAV.
    Unsupported,
    /// Advertised, but the write overwrites silently: a non-conforming server.
    Ignored,
}

/// One device's connection to a shared in-memory vault.
///
/// [`MemoryRemoteStore::connect`] returns another connection to the same
/// objects with its own fault settings, which is how a test gives two
/// simulated devices one vault.
#[derive(Debug)]
pub struct MemoryRemoteStore {
    objects: Arc<Mutex<BTreeMap<String, Bytes>>>,
    conditional_create: ConditionalCreate,
    /// Operations left before every call fails; negative means unlimited.
    budget: AtomicI64,
}

impl MemoryRemoteStore {
    pub fn new(conditional_create: ConditionalCreate) -> Self {
        Self {
            objects: Arc::default(),
            conditional_create,
            budget: AtomicI64::new(-1),
        }
    }

    /// Another connection to the same objects, without an operation budget.
    #[must_use]
    pub fn connect(&self) -> Self {
        Self {
            objects: Arc::clone(&self.objects),
            conditional_create: self.conditional_create,
            budget: AtomicI64::new(-1),
        }
    }

    /// Lets `operations` more calls succeed, then fails every call with
    /// [`RemoteError::Unavailable`] until [`MemoryRemoteStore::restore`]: a
    /// connection that drops in the middle of a cycle.
    pub fn fail_after(&self, operations: u32) {
        self.budget.store(i64::from(operations), Ordering::SeqCst);
    }

    pub fn restore(&self) {
        self.budget.store(-1, Ordering::SeqCst);
    }

    /// The keys currently stored, sorted.
    pub fn keys(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Bytes>> {
        self.objects
            .lock()
            .expect("no holder panics while the map is locked")
    }

    fn spend(&self) -> Result<(), RemoteError> {
        let spent = self
            .budget
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                (left != 0).then_some(if left > 0 { left - 1 } else { left })
            });
        spent.map(drop).map_err(|_| RemoteError::Unavailable {
            source: "injected connection loss".into(),
        })
    }
}

#[async_trait::async_trait]
impl RemoteStore for MemoryRemoteStore {
    fn capabilities(&self) -> RemoteCapabilities {
        RemoteCapabilities {
            create_if_absent: self.conditional_create != ConditionalCreate::Unsupported,
        }
    }

    async fn check(&self) -> Result<(), RemoteError> {
        self.spend()
    }

    async fn list(&self, prefix: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        self.spend()?;
        Ok(self
            .lock()
            .iter()
            .filter(|(key, _)| key.starts_with(prefix))
            .map(|(key, body)| RemoteEntry {
                key: key.clone(),
                size: body.len() as u64,
            })
            .collect())
    }

    async fn get(&self, key: &str) -> Result<Bytes, RemoteError> {
        validate_key(key)?;
        self.spend()?;
        self.lock()
            .get(key)
            .cloned()
            .ok_or_else(|| RemoteError::NotFound {
                key: key.to_owned(),
            })
    }

    async fn put(
        &self,
        key: &str,
        body: Bytes,
        condition: PutCondition,
    ) -> Result<(), RemoteError> {
        validate_key(key)?;
        self.spend()?;
        let mut objects = self.lock();
        if condition == PutCondition::IfAbsent {
            match self.conditional_create {
                ConditionalCreate::Unsupported => {
                    return Err(RemoteError::Unsupported {
                        operation: "create if absent",
                    });
                }
                ConditionalCreate::Honored if objects.contains_key(key) => {
                    return Err(RemoteError::AlreadyExists {
                        key: key.to_owned(),
                    });
                }
                ConditionalCreate::Honored | ConditionalCreate::Ignored => {}
            }
        }
        objects.insert(key.to_owned(), body);
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), RemoteError> {
        validate_key(key)?;
        self.spend()?;
        self.lock().remove(key);
        Ok(())
    }
}

/// The behavior every [`RemoteStore`] must have. `store` must be empty.
///
/// # Panics
///
/// Panics on the first violated expectation.
pub async fn assert_remote_store_contract(store: &dyn RemoteStore) {
    store.check().await.expect("an open store is reachable");
    assert_eq!(store.list("").await.expect("list an empty store"), []);
    assert!(matches!(
        store.get("objects/aa/missing").await,
        Err(RemoteError::NotFound { key }) if key == "objects/aa/missing"
    ));

    let small = Bytes::from_static(b"{}");
    let large = Bytes::from(vec![0x5a; 256 * 1024]);
    store
        .put("objects/aa/small", small.clone(), PutCondition::Any)
        .await
        .expect("put a small object");
    store
        .put("objects/bb/large", large.clone(), PutCondition::Any)
        .await
        .expect("put a large object");
    store
        .put(
            "commits/000000000001.json",
            small.clone(),
            PutCondition::Any,
        )
        .await
        .expect("put under another prefix");
    assert_eq!(store.get("objects/aa/small").await.expect("get"), small);
    assert_eq!(store.get("objects/bb/large").await.expect("get"), large);

    let mut listed = store.list("objects/").await.expect("recursive list");
    listed.sort_by(|left, right| left.key.cmp(&right.key));
    assert_eq!(
        listed,
        [
            RemoteEntry {
                key: "objects/aa/small".to_owned(),
                size: 2,
            },
            RemoteEntry {
                key: "objects/bb/large".to_owned(),
                size: 256 * 1024,
            },
        ]
    );
    assert_eq!(store.list("").await.expect("list everything").len(), 3);
    assert_eq!(store.list("probe/").await.expect("list nothing"), []);

    store
        .put("objects/aa/small", large.clone(), PutCondition::Any)
        .await
        .expect("unconditional put overwrites");
    assert_eq!(store.get("objects/aa/small").await.expect("get"), large);

    if store.capabilities().create_if_absent {
        store
            .put("probe/once", small.clone(), PutCondition::IfAbsent)
            .await
            .expect("conditional create of a new object");
        assert!(matches!(
            store.put("probe/once", large.clone(), PutCondition::IfAbsent).await,
            Err(RemoteError::AlreadyExists { key }) if key == "probe/once"
        ));
        assert_eq!(
            store.get("probe/once").await.expect("get"),
            small,
            "a refused create leaves the object untouched"
        );
    } else {
        assert!(matches!(
            store.put("probe/once", small, PutCondition::IfAbsent).await,
            Err(RemoteError::Unsupported { .. })
        ));
    }

    store.delete("objects/bb/large").await.expect("delete");
    store
        .delete("objects/bb/large")
        .await
        .expect("deleting a missing object succeeds");
    assert!(matches!(
        store.get("objects/bb/large").await,
        Err(RemoteError::NotFound { .. })
    ));

    for key in ["", "/absolute", "a//b", "a/../b", "a/./b", "a\\b", "a/"] {
        assert!(
            matches!(store.get(key).await, Err(RemoteError::InvalidKey { .. })),
            "{key:?} must be rejected"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_fake_passes_the_contract_in_both_capability_modes() {
        for mode in [ConditionalCreate::Honored, ConditionalCreate::Unsupported] {
            assert_remote_store_contract(&MemoryRemoteStore::new(mode)).await;
        }
    }

    #[tokio::test]
    async fn connections_share_objects_but_not_faults() {
        let first = MemoryRemoteStore::new(ConditionalCreate::Honored);
        let second = first.connect();
        first.fail_after(1);
        first
            .put("a", Bytes::from_static(b"1"), PutCondition::Any)
            .await
            .unwrap();
        assert!(matches!(
            first.get("a").await,
            Err(RemoteError::Unavailable { .. })
        ));
        assert_eq!(second.get("a").await.unwrap(), Bytes::from_static(b"1"));
        first.restore();
        assert_eq!(first.keys(), ["a"]);
    }

    #[tokio::test]
    async fn an_ignored_condition_overwrites_silently() {
        let store = MemoryRemoteStore::new(ConditionalCreate::Ignored);
        assert!(store.capabilities().create_if_absent);
        for body in [&b"1"[..], &b"2"[..]] {
            store
                .put(
                    "probe/x",
                    Bytes::copy_from_slice(body),
                    PutCondition::IfAbsent,
                )
                .await
                .unwrap();
        }
        assert_eq!(
            store.get("probe/x").await.unwrap(),
            Bytes::from_static(b"2")
        );
    }
}
