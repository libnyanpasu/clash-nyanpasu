//! Generated profile-port mocks with explicit content for application tests.

use crate::client::host_adapters::ProfileFileFactory;
#[cfg(test)]
use crate::{
    client::host_adapters::ProfileFilePorts,
    state::profiles::{
        error::ProfileFileError,
        ports::{
            CleanupOutcome, MaterializationResource, MockProfileFsPort,
            MockProfileMaterializationPort, PreparedCleanup, PreparedMaterialization,
            SubscriptionFetcher,
        },
    },
};
use std::sync::Arc;
#[cfg(test)]
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[cfg(test)]
type Files = BTreeMap<String, String>;
#[cfg(test)]
type PreparedFiles = BTreeMap<String, (String, String, Option<String>)>;

pub(crate) fn unused_profile_file_factory() -> Arc<ProfileFileFactory> {
    Arc::new(|_| panic!("the preassembled facade must not construct profile ports"))
}

#[cfg(test)]
pub(crate) fn fixed_profile_file_factory(ports: ProfileFilePorts) -> Arc<ProfileFileFactory> {
    Arc::new(move |_| ProfileFilePorts {
        fs: ports.fs.clone(),
        fetcher: ports.fetcher.clone(),
        materialization: ports.materialization.clone(),
    })
}

#[cfg(test)]
pub(crate) fn scripted_profile_file_ports(
    fetcher: Arc<dyn SubscriptionFetcher>,
) -> ProfileFilePorts {
    let files = Arc::new(Mutex::new(Files::new()));
    let prepared = Arc::new(Mutex::new(PreparedFiles::new()));
    let counter = Arc::new(AtomicU64::new(0));
    let mut fs = MockProfileFsPort::new();
    fs.expect_read().returning({
        let files = files.clone();
        move |path| {
            files
                .lock()
                .unwrap()
                .get(path.as_str())
                .cloned()
                .ok_or_else(|| ProfileFileError::mock("profile content is absent"))
        }
    });
    fs.expect_write_atomic().returning({
        let files = files.clone();
        move |path, content| {
            files
                .lock()
                .unwrap()
                .insert(path.to_string(), content.to_owned());
            Ok(())
        }
    });
    fs.expect_remove().returning({
        let files = files.clone();
        move |path| {
            files.lock().unwrap().remove(path.as_str());
            Ok(())
        }
    });
    fs.expect_ensure_not_symlink().returning(|_| Ok(()));
    let mut materialization = MockProfileMaterializationPort::new();
    materialization
        .expect_reconcile()
        .returning(|_| Ok(Default::default()));
    materialization.expect_prepare_state_first().returning({
        let files = files.clone();
        let prepared = prepared.clone();
        let counter = counter.clone();
        move |path, resource, _| prepare(&files, &prepared, &counter, path.as_str(), resource)
    });
    materialization.expect_prepare_file_first().returning({
        let files = files.clone();
        let prepared = prepared.clone();
        let counter = counter.clone();
        move |path, resource, _| prepare(&files, &prepared, &counter, path.as_str(), resource)
    });
    materialization.expect_promote().returning({
        let files = files.clone();
        let prepared = prepared.clone();
        move |handle| {
            let staged = prepared.lock().unwrap();
            let (path, content, _) = staged.get(handle.operation_id()).expect("prepared content");
            files.lock().unwrap().insert(path.clone(), content.clone());
            Ok(())
        }
    });
    materialization.expect_complete().returning({
        let prepared = prepared.clone();
        move |handle| {
            prepared.lock().unwrap().remove(handle.operation_id());
            Ok(())
        }
    });
    materialization.expect_compensate().returning({
        let files = files.clone();
        let prepared = prepared.clone();
        move |handle| {
            if let Some((path, _, prior)) = prepared.lock().unwrap().remove(handle.operation_id()) {
                let mut files = files.lock().unwrap();
                if let Some(prior) = prior {
                    files.insert(path, prior);
                } else {
                    files.remove(&path);
                }
            }
            Ok(())
        }
    });
    materialization
        .expect_prepare_cleanup()
        .returning(|path, _| Ok(PreparedCleanup::new(path.to_string())));
    materialization
        .expect_activate_cleanup()
        .returning(|_| Ok(()));
    materialization
        .expect_cancel_cleanup()
        .returning(|_| Ok(()));
    materialization
        .expect_retry_cleanup()
        .returning(move |handle, _| {
            let removed = files.lock().unwrap().remove(handle.operation_id());
            Ok(if removed.is_some() {
                CleanupOutcome::Removed
            } else {
                CleanupOutcome::AlreadyAbsent
            })
        });
    ProfileFilePorts {
        fs: Arc::new(fs),
        fetcher,
        materialization: Arc::new(materialization),
    }
}

#[cfg(test)]
fn prepare(
    files: &Mutex<Files>,
    prepared: &Mutex<PreparedFiles>,
    counter: &AtomicU64,
    path: &str,
    resource: MaterializationResource,
) -> Result<PreparedMaterialization, ProfileFileError> {
    let MaterializationResource::File { content } = resource else {
        panic!("this scenario does not use external symlinks")
    };
    let id = counter.fetch_add(1, Ordering::SeqCst).to_string();
    let prior = files.lock().unwrap().get(path).cloned();
    prepared
        .lock()
        .unwrap()
        .insert(id.clone(), (path.to_owned(), content, prior));
    Ok(PreparedMaterialization::new(id))
}
