//! Typed client for the ProfilesActor. Read Some(5s) / write None.

use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::profile::Profiles;
use nyanpasu_core::state::PersistentStateManagerSetup;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};

use crate::state::profiles::{
    ProfilesActor, ProfilesActorArgs, ProfilesActorMessage, ProfilesError,
    ports::{ProfileFsPort, RebuildNotifier, SubscriptionFetcher},
};

pub const PROFILES_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct ProfilesClient {
    inner: Arc<ProfilesClientInner>,
}

struct ProfilesClientInner {
    actor_ref: ActorRef<ProfilesActorMessage>,
}

impl ProfilesClient {
    pub(crate) async fn new(
        profiles_path: Utf8PathBuf,
        fs: Arc<dyn ProfileFsPort>,
        fetcher: Arc<dyn SubscriptionFetcher>,
        notifier: Arc<dyn RebuildNotifier>,
    ) -> anyhow::Result<Self> {
        let should_load = profiles_path.exists();
        let setup = PersistentStateManagerSetup::<Profiles>::builder()
            .config_path(profiles_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load profiles persistent state manager")?
        } else {
            setup
                .from_state(Profiles::default())
                .await
                .context("failed to initialize profiles persistent state manager")?
        };

        manager
            .snapshot_handle()
            .load()
            .state
            .validate()
            .map_err(|errors| anyhow::anyhow!("profiles.yaml failed validation: {errors:?}"))?;

        let actor_ref = Actor::spawn(
            None,
            ProfilesActor,
            ProfilesActorArgs {
                manager,
                fs,
                fetcher,
                notifier,
            },
        )
        .await
        .context("failed to spawn profiles actor")?
        .0;

        Ok(Self {
            inner: Arc::new(ProfilesClientInner { actor_ref }),
        })
    }

    pub async fn get(&self) -> Result<Arc<Profiles>, ProfilesError> {
        self.call(ProfilesActorMessage::Get, Some(PROFILES_READ_TIMEOUT))
            .await
    }

    async fn call<F, T>(&self, make: F, timeout: Option<Duration>) -> Result<T, ProfilesError>
    where
        F: FnOnce(RpcReplyPort<Result<T, ProfilesError>>) -> ProfilesActorMessage,
        T: Send + 'static,
    {
        match self.inner.actor_ref.call(make, timeout).await {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::SenderError) => Err(ProfilesError::Rpc("reply dropped".into())),
            Ok(CallResult::Timeout) => Err(ProfilesError::Rpc("call timed out".into())),
            Err(e) => Err(ProfilesError::Rpc(e.to_string())),
        }
    }
}

impl Drop for ProfilesClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::profiles::ports::{
        MockProfileFsPort, MockRebuildNotifier, MockSubscriptionFetcher,
    };
    use tempfile::{TempDir, tempdir};

    pub(crate) fn temp_profiles_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).expect("utf-8 temp path")
    }

    pub(crate) async fn test_client_with(fs: MockProfileFsPort) -> (ProfilesClient, TempDir) {
        let dir = tempdir().unwrap();
        let client = ProfilesClient::new(
            temp_profiles_path(&dir),
            std::sync::Arc::new(fs),
            std::sync::Arc::new(MockSubscriptionFetcher::new()),
            std::sync::Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .expect("profiles client should spawn");
        (client, dir)
    }

    #[tokio::test]
    async fn fresh_store_starts_with_default_profiles() {
        let (client, _dir) = test_client_with(MockProfileFsPort::new()).await;
        let snapshot = client.get().await.expect("get should succeed");
        assert!(snapshot.current.is_none());
        assert!(snapshot.items.is_empty());
        assert_eq!(snapshot.valid.len(), 3);
    }
}
