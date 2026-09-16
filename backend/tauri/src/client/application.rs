use std::sync::Arc;

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};
use nyanpasu_core::state::{PersistentStateManager, PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};

use crate::state::{
    ConditionalReplaceResult,
    application::{
        ApplicationActor, ApplicationActorArgs, ApplicationActorMessage, ApplicationSnapshot,
    },
    mirror::{PreparedTypedReplace, VergeLegacyBridge},
};

#[derive(Clone)]
pub struct ApplicationClient {
    inner: Arc<ApplicationClientInner>,
}

struct ApplicationClientInner {
    actor_ref: ActorRef<ApplicationActorMessage>,
    /// Committed state, read straight from the coordinator's store so a reader
    /// never queues behind a mutation the actor is still holding open.
    snapshot: StateSnapshot<NyanpasuAppConfig>,
}

#[allow(dead_code)]
impl ApplicationClient {
    pub(crate) async fn new(
        build_channel: crate::bundle::Channel,
        config_path: Utf8PathBuf,
        mut seed: NyanpasuAppConfig,
        bridge: Arc<dyn VergeLegacyBridge>,
    ) -> anyhow::Result<Self> {
        seed.release_channel = Some(build_channel.resolve(seed.release_channel));
        let should_load = config_path.exists();
        let setup = PersistentStateManagerSetup::<NyanpasuAppConfig>::builder()
            .config_path(config_path)
            .assemble();
        let mut manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load application persistent state manager")?
        } else {
            setup
                .from_state(seed)
                .await
                .context("failed to initialize application persistent state manager")?
        };

        let mut initial = manager.snapshot_handle().load().state.clone();
        let channel = build_channel.resolve(initial.release_channel);
        if initial.release_channel != Some(channel) {
            initial.release_channel = Some(channel);
            manager
                .upsert(initial)
                .await
                .context("failed to persist release channel")?;
        }

        Self::from_manager(manager, bridge).await
    }

    /// Takes ownership of an already loaded manager. Separate from [`Self::new`]
    /// so a caller can register state subscribers before the actor claims it.
    pub(crate) async fn from_manager(
        manager: PersistentStateManager<NyanpasuAppConfig>,
        bridge: Arc<dyn VergeLegacyBridge>,
    ) -> anyhow::Result<Self> {
        let snapshot = manager.snapshot_handle();
        let actor_ref = Actor::spawn(
            None,
            ApplicationActor,
            ApplicationActorArgs { manager, bridge },
        )
        .await
        .context("failed to spawn application actor")?
        .0;

        Ok(Self {
            inner: Arc::new(ApplicationClientInner {
                actor_ref,
                snapshot,
            }),
        })
    }

    /// The last committed application config. Reads bypass the mailbox, so an
    /// in-flight transaction parked in `on_prepare` cannot delay them.
    pub fn snapshot(&self) -> ApplicationSnapshot {
        ApplicationSnapshot::from_versioned(&self.inner.snapshot.load())
    }

    /// Read-only handle for collaborators that must observe committed state
    /// without holding a client that could write it.
    pub(crate) fn snapshot_handle(&self) -> StateSnapshot<NyanpasuAppConfig> {
        self.inner.snapshot.clone()
    }

    pub async fn patch(
        &self,
        patch: NyanpasuAppConfigPatch,
    ) -> anyhow::Result<ApplicationSnapshot> {
        self.call(
            |reply| ApplicationActorMessage::Patch { patch, reply },
            None,
        )
        .await
    }

    pub async fn replace(&self, state: NyanpasuAppConfig) -> anyhow::Result<ApplicationSnapshot> {
        self.call(
            |reply| ApplicationActorMessage::Replace { state, reply },
            None,
        )
        .await
    }

    pub(crate) async fn replace_if_version(
        &self,
        expected_version: u64,
        state: NyanpasuAppConfig,
    ) -> anyhow::Result<ConditionalReplaceResult<ApplicationSnapshot>> {
        let prepared = self.prepare_replace(state).await?;
        self.replace_prepared_if_version(expected_version, prepared)
            .await
    }

    pub(crate) async fn prepare_replace(
        &self,
        state: NyanpasuAppConfig,
    ) -> anyhow::Result<PreparedTypedReplace<NyanpasuAppConfig>> {
        match self
            .inner
            .actor_ref
            .call(
                |reply| ApplicationActorMessage::PrepareReplace { state, reply },
                None,
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("application actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("application actor call timed out"),
        }
    }

    pub(crate) async fn replace_prepared_if_version(
        &self,
        expected_version: u64,
        prepared: PreparedTypedReplace<NyanpasuAppConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ApplicationSnapshot>> {
        match self
            .inner
            .actor_ref
            .call(
                |reply| ApplicationActorMessage::ReplacePreparedIfVersion {
                    expected_version,
                    prepared,
                    reply,
                },
                None,
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("application actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("application actor call timed out"),
        }
    }

    async fn call<F>(
        &self,
        make: F,
        timeout: Option<std::time::Duration>,
    ) -> anyhow::Result<ApplicationSnapshot>
    where
        F: FnOnce(RpcReplyPort<anyhow::Result<ApplicationSnapshot>>) -> ApplicationActorMessage,
    {
        match self.inner.actor_ref.call(make, timeout).await? {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("application actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("application actor call timed out"),
        }
    }
}

impl Drop for ApplicationClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::{NoopPreparedLegacyMirror, PreparedLegacyMirror};
    use struct_patch::Patch;
    use tempfile::{TempDir, tempdir};

    struct NoopVergeBridge;

    impl VergeLegacyBridge for NoopVergeBridge {
        fn prepare(
            &self,
            _snap: &NyanpasuAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
            Ok(NyanpasuAppConfig::default())
        }
    }

    fn temp_config_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("application.yaml"))
            .expect("temp path should be UTF-8")
    }

    async fn test_client() -> (ApplicationClient, TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let client = ApplicationClient::new(
            crate::bundle::Channel::Stable,
            temp_config_path(&dir),
            NyanpasuAppConfig::default(),
            Arc::new(NoopVergeBridge),
        )
        .await
        .expect("application client should be created");
        (client, dir)
    }

    #[tokio::test]
    async fn get_patch_and_replace_application_config() {
        let (client, _dir) = test_client().await;

        let initial = client.snapshot();
        assert!(!initial.state.enable_system_proxy);

        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.enable_system_proxy = Some(true);
        let patched = client.patch(patch).await.expect("patch should succeed");
        assert!(patched.state.enable_system_proxy);

        let mut replacement = NyanpasuAppConfig::default();
        replacement.enable_silent_start = true;
        let replaced = client
            .replace(replacement)
            .await
            .expect("replace should succeed");
        assert!(replaced.state.enable_silent_start);
    }

    #[tokio::test]
    async fn replace_if_version_rejects_stale_snapshot() {
        let (client, _dir) = test_client().await;
        let current = client.snapshot();
        let mut replacement = current.state.clone();
        replacement.enable_silent_start = true;

        let result = client
            .replace_if_version(current.version + 1, replacement)
            .await
            .expect("stale replace should return a conflict");
        assert!(matches!(
            result,
            ConditionalReplaceResult::Conflict { actual_version: 0 }
        ));
    }

    #[tokio::test]
    async fn release_channel_persists_and_cannot_leave_nightly() {
        use crate::bundle::Channel;
        let (client, dir) = test_client().await;
        for channel in [Channel::Beta, Channel::Stable, Channel::Nightly] {
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.release_channel = Some(Some(channel));
            assert_eq!(
                client.patch(patch).await.unwrap().state.release_channel,
                Some(channel)
            );
        }
        drop(client);
        // Reload without relying on the original actor's memory.
        let reloaded = ApplicationClient::new(
            Channel::Stable,
            temp_config_path(&dir),
            NyanpasuAppConfig::default(),
            Arc::new(NoopVergeBridge),
        )
        .await
        .unwrap();
        for channel in [Channel::Stable, Channel::Beta] {
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.release_channel = Some(Some(channel));
            assert!(reloaded.patch(patch).await.is_err());
            let mut replacement = NyanpasuAppConfig::default();
            replacement.release_channel = Some(channel);
            assert!(reloaded.replace(replacement).await.is_err());
        }
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.release_channel = Some(None);
        assert_eq!(
            reloaded.patch(patch).await.unwrap().state.release_channel,
            Some(Channel::Nightly)
        );
    }

    #[tokio::test]
    async fn release_channel_revalidates_prepared_changes_at_commit() {
        use crate::bundle::Channel;
        let (client, _dir) = test_client().await;
        let mut next = client.snapshot().state;
        next.release_channel = Some(Channel::Beta);
        let prepared = client.prepare_replace(next).await.unwrap();
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.release_channel = Some(Some(Channel::Nightly));
        let current = client.patch(patch).await.unwrap();
        assert!(
            client
                .replace_prepared_if_version(current.version, prepared)
                .await
                .is_err()
        );
        assert_eq!(
            client.snapshot().state.release_channel,
            Some(Channel::Nightly)
        );
    }

    #[tokio::test]
    async fn release_channel_compiled_nightly_overrides_saved_stable() {
        use crate::bundle::Channel;
        let (client, dir) = test_client().await;
        assert_eq!(
            client.snapshot().state.release_channel,
            Some(Channel::Stable)
        );
        let nightly = ApplicationClient::new(
            Channel::Nightly,
            temp_config_path(&dir),
            NyanpasuAppConfig::default(),
            Arc::new(NoopVergeBridge),
        )
        .await
        .unwrap();
        assert_eq!(
            nightly.snapshot().state.release_channel,
            Some(Channel::Nightly)
        );
    }
    #[tokio::test]
    async fn release_channel_migrates_old_beta_config_and_keeps_explicit_stable() {
        use crate::bundle::Channel;
        let dir = tempdir().unwrap();
        let manager = PersistentStateManagerSetup::<NyanpasuAppConfig>::builder()
            .config_path(temp_config_path(&dir))
            .assemble()
            .from_state(NyanpasuAppConfig::default())
            .await
            .unwrap();
        drop(manager);
        let beta = ApplicationClient::new(
            Channel::Beta,
            temp_config_path(&dir),
            NyanpasuAppConfig::default(),
            Arc::new(NoopVergeBridge),
        )
        .await
        .unwrap();
        assert_eq!(beta.snapshot().state.release_channel, Some(Channel::Beta));
        let mut patch = NyanpasuAppConfig::new_empty_patch();
        patch.release_channel = Some(Some(Channel::Stable));
        beta.patch(patch).await.unwrap();
        drop(beta);
        let reloaded = ApplicationClient::new(
            Channel::Beta,
            temp_config_path(&dir),
            NyanpasuAppConfig::default(),
            Arc::new(NoopVergeBridge),
        )
        .await
        .unwrap();
        assert_eq!(
            reloaded.snapshot().state.release_channel,
            Some(Channel::Stable)
        );
    }
}
