use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::clash::config::{ClashConfig, ClashConfigPatch};
use nyanpasu_core::state::PersistentStateManagerSetup;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};

use crate::state::{
    ConditionalReplaceResult,
    clash_config::{
        ClashConfigActor, ClashConfigActorArgs, ClashConfigActorMessage, ClashConfigSnapshot,
    },
    mirror::{ClashLegacyBridge, PreparedTypedReplace},
};

const CLASH_CONFIG_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct ClashConfigClient {
    inner: Arc<ClashConfigClientInner>,
}

struct ClashConfigClientInner {
    actor_ref: ActorRef<ClashConfigActorMessage>,
}

impl ClashConfigClient {
    pub(crate) async fn new(
        config_path: Utf8PathBuf,
        seed: ClashConfig,
        bridge: Arc<dyn ClashLegacyBridge>,
    ) -> anyhow::Result<Self> {
        let should_load = config_path.exists();
        let setup = PersistentStateManagerSetup::<ClashConfig>::builder()
            .config_path(config_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load clash persistent state manager")?
        } else {
            setup
                .from_state(seed)
                .await
                .context("failed to initialize clash persistent state manager")?
        };

        let actor_ref = Actor::spawn(
            None,
            ClashConfigActor,
            ClashConfigActorArgs { manager, bridge },
        )
        .await
        .context("failed to spawn clash config actor")?
        .0;

        Ok(Self {
            inner: Arc::new(ClashConfigClientInner { actor_ref }),
        })
    }

    pub async fn get(&self) -> anyhow::Result<ClashConfigSnapshot> {
        self.call(
            ClashConfigActorMessage::Get,
            Some(CLASH_CONFIG_READ_TIMEOUT),
        )
        .await
    }

    pub async fn patch(&self, patch: ClashConfigPatch) -> anyhow::Result<ClashConfigSnapshot> {
        self.call(
            |reply| ClashConfigActorMessage::Patch { patch, reply },
            None,
        )
        .await
    }

    pub async fn replace(&self, state: ClashConfig) -> anyhow::Result<ClashConfigSnapshot> {
        self.call(
            |reply| ClashConfigActorMessage::Replace { state, reply },
            None,
        )
        .await
    }

    pub(crate) async fn replace_if_version(
        &self,
        expected_version: u64,
        state: ClashConfig,
    ) -> anyhow::Result<ConditionalReplaceResult<ClashConfigSnapshot>> {
        let prepared = self.prepare_replace(state).await?;
        self.replace_prepared_if_version(expected_version, prepared)
            .await
    }

    pub(crate) async fn prepare_replace(
        &self,
        state: ClashConfig,
    ) -> anyhow::Result<PreparedTypedReplace<ClashConfig>> {
        match self
            .inner
            .actor_ref
            .call(
                |reply| ClashConfigActorMessage::PrepareReplace { state, reply },
                None,
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
        }
    }

    pub(crate) async fn replace_prepared_if_version(
        &self,
        expected_version: u64,
        prepared: PreparedTypedReplace<ClashConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ClashConfigSnapshot>> {
        match self
            .inner
            .actor_ref
            .call(
                |reply| ClashConfigActorMessage::ReplacePreparedIfVersion {
                    expected_version,
                    prepared,
                    reply,
                },
                None,
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
        }
    }

    async fn call<F>(
        &self,
        make: F,
        timeout: Option<Duration>,
    ) -> anyhow::Result<ClashConfigSnapshot>
    where
        F: FnOnce(RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>) -> ClashConfigActorMessage,
    {
        match self.inner.actor_ref.call(make, timeout).await? {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
        }
    }
}

impl Drop for ClashConfigClientInner {
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

    struct NoopClashBridge;

    impl ClashLegacyBridge for NoopClashBridge {
        fn prepare(&self, _snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
        }
    }

    fn temp_config_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml"))
            .expect("temp path should be UTF-8")
    }

    async fn test_client() -> (ClashConfigClient, TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let client = ClashConfigClient::new(
            temp_config_path(&dir),
            ClashConfig::default(),
            Arc::new(NoopClashBridge),
        )
        .await
        .expect("clash config client should be created");
        (client, dir)
    }

    #[tokio::test]
    async fn get_patch_and_replace_clash_config() {
        let (client, _dir) = test_client().await;

        let initial = client.get().await.expect("get should succeed");
        assert!(!initial.state.enable_tun_mode);

        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let patched = client.patch(patch).await.expect("patch should succeed");
        assert!(patched.state.enable_tun_mode);

        let replaced = client
            .replace(ClashConfig::default())
            .await
            .expect("replace should succeed");
        assert!(!replaced.state.enable_tun_mode);
    }

    #[tokio::test]
    async fn replace_if_version_commits_matching_snapshot() {
        let (client, _dir) = test_client().await;
        let current = client.get().await.expect("get should succeed");
        let mut next = current.state.clone();
        next.enable_tun_mode = true;

        let result = client
            .replace_if_version(current.version, next)
            .await
            .expect("matching replace should succeed");
        match result {
            ConditionalReplaceResult::Replaced(snapshot) => {
                assert_eq!(snapshot.version, current.version + 1);
                assert!(snapshot.state.enable_tun_mode);
            }
            ConditionalReplaceResult::Conflict { actual_version } => {
                panic!("unexpected conflict at version {actual_version}")
            }
        }
    }
}
