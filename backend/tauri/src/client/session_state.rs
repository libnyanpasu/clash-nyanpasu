use std::sync::Arc;

use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::state::{
    PersistentState, PersistentStatePatch,
    window::{WindowLabel, WindowState},
};
use nyanpasu_core::state::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::state::session_state::{
    SessionStateActor, SessionStateActorArgs, SessionStateActorMessage, SessionStateSnapshot,
};

#[derive(Clone)]
pub struct SessionStateClient {
    inner: Arc<SessionStateClientInner>,
}

struct SessionStateClientInner {
    actor_ref: ActorRef<SessionStateActorMessage>,
    /// Committed state, read straight from the coordinator's store so a reader
    /// never queues behind a mutation the actor is still holding open.
    snapshot: StateSnapshot<PersistentState>,
}

#[allow(dead_code)]
impl SessionStateClient {
    /// The actor admits every request, the shutdown's final geometry save
    /// included: `shutdown` only drains it, so whatever was queued before the
    /// cancel is written before it stops.
    pub(crate) async fn new(
        config_path: Utf8PathBuf,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
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
                .from_state(PersistentState::default())
                .await
                .context("failed to initialize session persistent state manager")?
        };

        let snapshot = manager.snapshot_handle();
        let actor_ref = Actor::spawn(None, SessionStateActor, SessionStateActorArgs { manager })
            .await
            .context("failed to spawn session state actor")?
            .0;
        crate::client::drain_on_shutdown(tasks, shutdown, actor_ref.get_cell());

        Ok(Self {
            inner: Arc::new(SessionStateClientInner {
                actor_ref,
                snapshot,
            }),
        })
    }

    /// The last committed session state. Reads bypass the mailbox, so an
    /// in-flight transaction parked in `on_prepare` cannot delay them.
    pub fn snapshot(&self) -> SessionStateSnapshot {
        SessionStateSnapshot::from_versioned(&self.inner.snapshot.load())
    }

    /// The geometry the main window reopens with.
    pub fn main_window_geometry(&self) -> Option<WindowState> {
        main_window_geometry(&self.snapshot().state)
    }

    pub async fn save_main_window(
        &self,
        geometry: nyanpasu_config::state::window::WindowState,
    ) -> anyhow::Result<SessionStateSnapshot> {
        self.call(|reply| SessionStateActorMessage::SaveMainWindow {
            geometry,
            reply: Some(reply),
        })
        .await
    }

    /// Queues a save of the main window's geometry without waiting for it:
    /// the actor writes it after what is already queued.
    pub fn queue_main_window_save(
        &self,
        geometry: nyanpasu_config::state::window::WindowState,
    ) -> anyhow::Result<()> {
        self.inner
            .actor_ref
            .cast(SessionStateActorMessage::SaveMainWindow {
                geometry,
                reply: None,
            })
            .context("the session state actor is gone")
    }

    pub async fn patch(&self, patch: PersistentStatePatch) -> anyhow::Result<SessionStateSnapshot> {
        self.call(|reply| SessionStateActorMessage::Patch { patch, reply })
            .await
    }

    pub async fn replace(&self, state: PersistentState) -> anyhow::Result<SessionStateSnapshot> {
        self.call(|reply| SessionStateActorMessage::Replace { state, reply })
            .await
    }

    async fn call<F>(&self, make: F) -> anyhow::Result<SessionStateSnapshot>
    where
        F: FnOnce(RpcReplyPort<anyhow::Result<SessionStateSnapshot>>) -> SessionStateActorMessage,
    {
        match self.inner.actor_ref.call(make, None).await? {
            CallResult::Success(result) => result,
            _ => anyhow::bail!("session state actor reply dropped"),
        }
    }
}

/// The entry `save_main_window` writes: the one under the main window label.
fn main_window_geometry(state: &PersistentState) -> Option<WindowState> {
    state
        .window_state
        .get(&WindowLabel(crate::consts::MAIN_WINDOW_LABEL.into()))
        .cloned()
}

impl Drop for SessionStateClientInner {
    fn drop(&mut self) {
        self.actor_ref.stop(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use struct_patch::Patch;
    use tempfile::{TempDir, tempdir};

    fn temp_config_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("session-state.yaml"))
            .expect("temp path should be UTF-8")
    }

    async fn test_client() -> (SessionStateClient, TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let client = SessionStateClient::new(
            temp_config_path(&dir),
            tokio_util::sync::CancellationToken::new(),
            &tokio_util::task::TaskTracker::new(),
        )
        .await
        .expect("session state client should be created");
        (client, dir)
    }

    #[tokio::test]
    async fn get_patch_and_replace_session_state() {
        let (client, _dir) = test_client().await;

        let initial = client.snapshot();
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

    fn geometry(x: i32) -> WindowState {
        WindowState {
            width: 1024,
            height: 768,
            x,
            y: 20,
            maximized: false,
            fullscreen: false,
        }
    }

    #[test]
    fn main_window_geometry_reads_only_the_main_label() {
        let state = PersistentState {
            window_state: BTreeMap::from([
                (WindowLabel("editor-css".into()), geometry(1)),
                (WindowLabel("main".into()), geometry(2)),
            ]),
        };
        assert_eq!(main_window_geometry(&state), Some(geometry(2)));

        let without_main = PersistentState {
            window_state: BTreeMap::from([(WindowLabel("editor-css".into()), geometry(1))]),
        };
        assert_eq!(main_window_geometry(&without_main), None);
    }

    #[tokio::test]
    async fn a_saved_main_window_geometry_is_what_the_window_restores() {
        let (client, _dir) = test_client().await;
        assert_eq!(client.main_window_geometry(), None);

        client
            .save_main_window(geometry(42))
            .await
            .expect("saving the main window geometry should succeed");

        assert_eq!(client.main_window_geometry(), Some(geometry(42)));
    }

    #[tokio::test]
    async fn a_queued_geometry_save_is_written_before_the_next_request() {
        let (client, _dir) = test_client().await;

        client.queue_main_window_save(geometry(7)).unwrap();
        // The mailbox is FIFO: a request sent afterwards is answered only once
        // the queued save has been written.
        client
            .patch(PersistentState::new_empty_patch())
            .await
            .unwrap();

        assert_eq!(client.main_window_geometry(), Some(geometry(7)));
    }
}
