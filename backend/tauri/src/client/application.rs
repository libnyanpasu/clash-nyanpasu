use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::application::{NyanpasuAppConfig, NyanpasuAppConfigPatch};
use nyanpasu_core::state::PersistentStateManagerSetup;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};

use crate::state::{
    ConditionalReplaceResult,
    application::{
        ApplicationActor, ApplicationActorArgs, ApplicationActorMessage, ApplicationSnapshot,
    },
    mirror::{PreparedTypedReplace, VergeLegacyBridge},
};

const APPLICATION_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct ApplicationClient {
    inner: Arc<ApplicationClientInner>,
}

struct ApplicationClientInner {
    actor_ref: ActorRef<ApplicationActorMessage>,
}

impl ApplicationClient {
    pub(crate) async fn new(
        config_path: Utf8PathBuf,
        seed: NyanpasuAppConfig,
        bridge: Arc<dyn VergeLegacyBridge>,
    ) -> anyhow::Result<Self> {
        let should_load = config_path.exists();
        let setup = PersistentStateManagerSetup::<NyanpasuAppConfig>::builder()
            .config_path(config_path)
            .assemble();
        let manager = if should_load {
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

        let actor_ref = Actor::spawn(
            None,
            ApplicationActor,
            ApplicationActorArgs { manager, bridge },
        )
        .await
        .context("failed to spawn application actor")?
        .0;

        Ok(Self {
            inner: Arc::new(ApplicationClientInner { actor_ref }),
        })
    }

    pub async fn get(&self) -> anyhow::Result<ApplicationSnapshot> {
        self.call(ApplicationActorMessage::Get, Some(APPLICATION_READ_TIMEOUT))
            .await
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
        timeout: Option<Duration>,
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

        let initial = client.get().await.expect("get should succeed");
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
        let current = client.get().await.expect("get should succeed");
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
}
