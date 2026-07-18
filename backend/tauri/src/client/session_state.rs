use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::state::{PersistentState, PersistentStatePatch};
use nyanpasu_core::state::PersistentStateManagerSetup;
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};

use crate::state::{
    ConditionalReplaceResult,
    mirror::{PreparedTypedReplace, WindowLegacyBridge},
    session_state::{
        SessionStateActor, SessionStateActorArgs, SessionStateActorMessage, SessionStateSnapshot,
    },
};

const SESSION_STATE_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct SessionStateClient {
    inner: Arc<SessionStateClientInner>,
}

struct SessionStateClientInner {
    actor_ref: ActorRef<SessionStateActorMessage>,
}

impl SessionStateClient {
    pub(crate) async fn new(
        config_path: Utf8PathBuf,
        seed: PersistentState,
        bridge: Arc<dyn WindowLegacyBridge>,
    ) -> anyhow::Result<Self> {
        let should_load = config_path.exists();
        let setup = PersistentStateManagerSetup::<PersistentState>::builder()
            .config_path(config_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load session persistent state manager")?
        } else {
            setup
                .from_state(seed)
                .await
                .context("failed to initialize session persistent state manager")?
        };

        let actor_ref = Actor::spawn(
            None,
            SessionStateActor,
            SessionStateActorArgs { manager, bridge },
        )
        .await
        .context("failed to spawn session state actor")?
        .0;

        Ok(Self {
            inner: Arc::new(SessionStateClientInner { actor_ref }),
        })
    }

    pub async fn get(&self) -> anyhow::Result<SessionStateSnapshot> {
        self.call(
            SessionStateActorMessage::Get,
            Some(SESSION_STATE_READ_TIMEOUT),
        )
        .await
    }

    pub async fn patch(&self, patch: PersistentStatePatch) -> anyhow::Result<SessionStateSnapshot> {
        self.call(
            |reply| SessionStateActorMessage::Patch { patch, reply },
            None,
        )
        .await
    }

    pub async fn replace(&self, state: PersistentState) -> anyhow::Result<SessionStateSnapshot> {
        self.call(
            |reply| SessionStateActorMessage::Replace { state, reply },
            None,
        )
        .await
    }

    pub(crate) async fn replace_if_version(
        &self,
        expected_version: u64,
        state: PersistentState,
    ) -> anyhow::Result<ConditionalReplaceResult<SessionStateSnapshot>> {
        let prepared = self.prepare_replace(state).await?;
        self.replace_prepared_if_version(expected_version, prepared)
            .await
    }

    pub(crate) async fn prepare_replace(
        &self,
        state: PersistentState,
    ) -> anyhow::Result<PreparedTypedReplace<PersistentState>> {
        match self
            .inner
            .actor_ref
            .call(
                |reply| SessionStateActorMessage::PrepareReplace { state, reply },
                None,
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("session state actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("session state actor call timed out"),
        }
    }

    pub(crate) async fn replace_prepared_if_version(
        &self,
        expected_version: u64,
        prepared: PreparedTypedReplace<PersistentState>,
    ) -> anyhow::Result<ConditionalReplaceResult<SessionStateSnapshot>> {
        match self
            .inner
            .actor_ref
            .call(
                |reply| SessionStateActorMessage::ReplacePreparedIfVersion {
                    expected_version,
                    prepared,
                    reply,
                },
                None,
            )
            .await?
        {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("session state actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("session state actor call timed out"),
        }
    }

    async fn call<F>(
        &self,
        make: F,
        timeout: Option<Duration>,
    ) -> anyhow::Result<SessionStateSnapshot>
    where
        F: FnOnce(RpcReplyPort<anyhow::Result<SessionStateSnapshot>>) -> SessionStateActorMessage,
    {
        match self.inner.actor_ref.call(make, timeout).await? {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("session state actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("session state actor call timed out"),
        }
    }
}

impl Drop for SessionStateClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::{NoopPreparedLegacyMirror, PreparedLegacyMirror};
    use nyanpasu_config::state::window::{WindowLabel, WindowState};
    use std::collections::BTreeMap;
    use struct_patch::Patch;
    use tempfile::{TempDir, tempdir};

    struct NoopWindowBridge;

    impl WindowLegacyBridge for NoopWindowBridge {
        fn prepare(
            &self,
            _snap: &PersistentState,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
            Ok(PersistentState::default())
        }
    }

    fn temp_config_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("session-state.yaml"))
            .expect("temp path should be UTF-8")
    }

    async fn test_client() -> (SessionStateClient, TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let client = SessionStateClient::new(
            temp_config_path(&dir),
            PersistentState::default(),
            Arc::new(NoopWindowBridge),
        )
        .await
        .expect("session state client should be created");
        (client, dir)
    }

    #[tokio::test]
    async fn get_patch_and_replace_session_state() {
        let (client, _dir) = test_client().await;

        let initial = client.get().await.expect("get should succeed");
        assert!(initial.state.window_state.is_empty());

        let label = WindowLabel("main".into());
        let window = WindowState {
            width: 800,
            height: 600,
            x: 10,
            y: 20,
            maximized: false,
            fullscreen: false,
        };

        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(BTreeMap::from([(label.clone(), window.clone())]));
        let patched = client.patch(patch).await.expect("patch should succeed");
        assert_eq!(patched.state.window_state.get(&label), Some(&window));

        let replaced = client
            .replace(PersistentState::default())
            .await
            .expect("replace should succeed");
        assert!(replaced.state.window_state.is_empty());
    }

    #[tokio::test]
    async fn replace_if_version_commits_matching_snapshot() {
        let (client, _dir) = test_client().await;
        let current = client.get().await.expect("get should succeed");
        let label = WindowLabel("main".into());
        let next = PersistentState {
            window_state: BTreeMap::from([(
                label.clone(),
                WindowState {
                    width: 800,
                    height: 600,
                    x: 10,
                    y: 20,
                    maximized: false,
                    fullscreen: false,
                },
            )]),
        };

        let result = client
            .replace_if_version(current.version, next)
            .await
            .expect("matching replace should succeed");
        match result {
            ConditionalReplaceResult::Replaced(snapshot) => {
                assert_eq!(snapshot.version, current.version + 1);
                assert!(snapshot.state.window_state.contains_key(&label));
            }
            ConditionalReplaceResult::Conflict { actual_version } => {
                panic!("unexpected conflict at version {actual_version}")
            }
        }
    }
}
