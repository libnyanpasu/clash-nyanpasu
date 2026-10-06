use std::sync::Arc;

use nyanpasu_config::state::{
    PersistentState, PersistentStatePatch,
    window::{WindowLabel, WindowState},
};
use nyanpasu_core::state::{PersistentStateManager, StateSnapshot, VersionedState};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    SessionStateError,
    actor::{SessionStateActor, SessionStateActorArgs, SessionStateActorMessage},
    error::SessionStateStoppedSnafu,
};

#[derive(Debug, Clone)]
pub struct SessionStateSnapshot {
    pub state: PersistentState,
    pub version: u64,
}

impl SessionStateSnapshot {
    pub(super) fn from_versioned(versioned: &VersionedState<PersistentState>) -> Self {
        Self {
            state: versioned.state.clone(),
            version: *versioned.version.as_ref(),
        }
    }
}

#[derive(Clone)]
pub struct SessionStateClient {
    inner: Arc<SessionStateClientInner>,
}

struct SessionStateClientInner {
    actor_ref: ActorRef<SessionStateActorMessage>,
    /// Committed state, read straight from the coordinator's store so a reader
    /// never queues behind a mutation the actor is still holding open.
    snapshot: StateSnapshot<PersistentState>,
    main_window_label: WindowLabel,
    shutdown: CancellationToken,
}

impl SessionStateClient {
    /// Starts the owner for an already-opened persistent-state manager.
    ///
    /// On shutdown the owner drains messages already accepted by the mailbox,
    /// including a final geometry save, and refuses later requests. A queued
    /// save accepted before cancellation is the intentional exception to the
    /// usual rule that shutdown refuses new work: it is the final window state
    /// needed by the next host startup.
    pub async fn new(
        manager: PersistentStateManager<PersistentState>,
        main_window_label: WindowLabel,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> anyhow::Result<Self> {
        let snapshot = manager.snapshot_handle();
        let actor_ref = Actor::spawn(
            None,
            SessionStateActor,
            SessionStateActorArgs {
                manager,
                main_window_label: main_window_label.clone(),
            },
        )
        .await
        .map_err(|error| anyhow::anyhow!("failed to spawn session state actor: {error}"))?
        .0;

        let actor_cell = actor_ref.get_cell();
        let owner_shutdown = shutdown.clone();
        tasks.spawn(async move {
            owner_shutdown.cancelled().await;
            // The actor deliberately keeps accepting work while it drains:
            // the mailbox's FIFO boundary separates pre-cancel accepted work
            // from requests attempted after drain begins.
            let _ = actor_cell.drain_and_wait(None).await;
        });

        Ok(Self {
            inner: Arc::new(SessionStateClientInner {
                actor_ref,
                snapshot,
                main_window_label,
                shutdown,
            }),
        })
    }

    /// The last committed session state. Reads bypass the mailbox, so an
    /// in-flight transaction can never delay them.
    pub fn snapshot(&self) -> SessionStateSnapshot {
        SessionStateSnapshot::from_versioned(&self.inner.snapshot.load())
    }

    /// The geometry the configured main window reopens with.
    pub fn main_window_geometry(&self) -> Option<WindowState> {
        self.snapshot()
            .state
            .window_state
            .get(&self.inner.main_window_label)
            .cloned()
    }

    pub async fn save_main_window(
        &self,
        geometry: WindowState,
    ) -> Result<SessionStateSnapshot, SessionStateError> {
        self.call(|reply| SessionStateActorMessage::SaveMainWindow {
            geometry,
            reply: Some(reply),
        })
        .await
    }

    /// Queues a save without waiting. It is processed after messages already
    /// in the mailbox and before a subsequent request can complete.
    pub fn queue_main_window_save(&self, geometry: WindowState) -> Result<(), SessionStateError> {
        if self.inner.shutdown.is_cancelled() {
            return SessionStateStoppedSnafu.fail();
        }
        match self
            .inner
            .actor_ref
            .cast(SessionStateActorMessage::SaveMainWindow {
                geometry,
                reply: None,
            }) {
            Ok(()) => Ok(()),
            Err(_) => SessionStateStoppedSnafu.fail(),
        }
    }

    pub async fn patch(
        &self,
        patch: PersistentStatePatch,
    ) -> Result<SessionStateSnapshot, SessionStateError> {
        self.call(|reply| SessionStateActorMessage::Patch { patch, reply })
            .await
    }

    pub async fn replace(
        &self,
        state: PersistentState,
    ) -> Result<SessionStateSnapshot, SessionStateError> {
        self.call(|reply| SessionStateActorMessage::Replace { state, reply })
            .await
    }

    async fn call<F>(&self, make: F) -> Result<SessionStateSnapshot, SessionStateError>
    where
        F: FnOnce(
            RpcReplyPort<Result<SessionStateSnapshot, SessionStateError>>,
        ) -> SessionStateActorMessage,
    {
        if self.inner.shutdown.is_cancelled() {
            return SessionStateStoppedSnafu.fail();
        }
        match self.inner.actor_ref.call(make, None).await {
            Ok(CallResult::Success(result)) => result,
            Ok(CallResult::SenderError) | Err(_) => SessionStateStoppedSnafu.fail(),
            Ok(CallResult::Timeout) => {
                unreachable!("session state calls are made without a timeout")
            }
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
    use std::collections::BTreeMap;

    use camino::Utf8PathBuf;
    use nyanpasu_config::state::window::{WindowLabel, WindowState};
    use nyanpasu_core::state::PersistentStateManagerSetup;
    use ractor::concurrency;
    use struct_patch::Patch;
    use tempfile::{TempDir, tempdir};
    use tokio_util::{sync::CancellationToken, task::TaskTracker};

    use super::*;

    fn temp_config_path(dir: &TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join("session-state.yaml"))
            .expect("temp path should be UTF-8")
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

    async fn test_client(
        manager: PersistentStateManager<PersistentState>,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> SessionStateClient {
        SessionStateClient::new(manager, WindowLabel("main".into()), shutdown, tasks)
            .await
            .expect("session state client should be created")
    }

    async fn test_manager(path: Utf8PathBuf) -> PersistentStateManager<PersistentState> {
        let should_load = path.exists();
        let setup = PersistentStateManagerSetup::<PersistentState>::builder()
            .config_path(path)
            .assemble();
        if should_load {
            setup
                .load()
                .await
                .expect("session state manager should load")
        } else {
            setup
                .from_state(PersistentState::default())
                .await
                .expect("session state manager should initialize")
        }
    }

    #[tokio::test]
    async fn patches_persist_and_reload_through_the_existing_state_manager() {
        let dir = tempdir().expect("tempdir should be created");
        let path = temp_config_path(&dir);
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let manager = test_manager(path.clone()).await;
        let client = test_client(manager, shutdown.clone(), &tasks).await;
        let label = WindowLabel("main".into());
        let window = geometry(42);
        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(BTreeMap::from([(label.clone(), window.clone())]));
        client.patch(patch).await.expect("patch should persist");

        shutdown.cancel();
        assert!(client.queue_main_window_save(geometry(4321)).is_err());
        tasks.close();
        tasks.wait().await;
        drop(client);

        let reloaded_tasks = TaskTracker::new();
        let reloaded_shutdown = CancellationToken::new();
        let reloaded_manager = test_manager(path).await;
        let reloaded =
            test_client(reloaded_manager, reloaded_shutdown.clone(), &reloaded_tasks).await;
        assert_eq!(
            reloaded.snapshot().state.window_state.get(&label),
            Some(&window)
        );
        assert_eq!(reloaded.main_window_geometry(), Some(window));
        reloaded_shutdown.cancel();
        reloaded_tasks.close();
        reloaded_tasks.wait().await;
    }

    #[tokio::test]
    async fn replacement_clears_persisted_geometry() {
        let dir = tempdir().unwrap();
        let path = temp_config_path(&dir);
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let client = test_client(test_manager(path.clone()).await, shutdown.clone(), &tasks).await;
        client.save_main_window(geometry(42)).await.unwrap();
        let replaced = client.replace(PersistentState::default()).await.unwrap();
        assert!(replaced.state.window_state.is_empty());
        assert_eq!(client.main_window_geometry(), None);
        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
        let reloaded = test_manager(path).await;
        assert!(
            reloaded
                .snapshot_handle()
                .load()
                .state
                .window_state
                .is_empty()
        );
    }

    #[tokio::test]
    async fn host_label_selects_only_its_own_window_geometry() {
        let dir = tempdir().expect("tempdir should be created");
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let manager = test_manager(temp_config_path(&dir)).await;
        let client = SessionStateClient::new(
            manager,
            WindowLabel("host-window".into()),
            shutdown.clone(),
            &tasks,
        )
        .await
        .unwrap();
        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(BTreeMap::from([
            (WindowLabel("main".into()), geometry(10)),
            (WindowLabel("host-window".into()), geometry(20)),
        ]));
        client.patch(patch).await.unwrap();
        assert_eq!(client.main_window_geometry(), Some(geometry(20)));
        client.save_main_window(geometry(30)).await.unwrap();
        assert_eq!(
            client
                .snapshot()
                .state
                .window_state
                .get(&WindowLabel("main".into())),
            Some(&geometry(10))
        );
        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
    }

    #[tokio::test]
    async fn a_dropped_waiter_does_not_cancel_the_accepted_persistence() {
        let dir = tempdir().expect("tempdir should be created");
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let manager = test_manager(temp_config_path(&dir)).await;
        let client = test_client(manager, shutdown.clone(), &tasks).await;
        let label = WindowLabel("main".into());
        let window = geometry(27);

        let (sender, receiver) = concurrency::oneshot();
        drop(receiver);
        client
            .inner
            .actor_ref
            .cast(SessionStateActorMessage::SaveMainWindow {
                geometry: window.clone(),
                reply: Some(sender.into()),
            })
            .expect("actor should accept the save");

        // This request is ordered behind the save and observes its commit.
        let patched = client
            .patch(PersistentState::new_empty_patch())
            .await
            .expect("later request should complete");
        assert_eq!(patched.state.window_state.get(&label), Some(&window));

        shutdown.cancel();
        tasks.close();
        tasks.wait().await;
    }

    #[tokio::test]
    async fn shutdown_drains_a_queued_geometry_save_and_rejects_later_work() {
        let dir = tempdir().expect("tempdir should be created");
        let shutdown = CancellationToken::new();
        let tasks = TaskTracker::new();
        let manager = test_manager(temp_config_path(&dir)).await;
        let client = test_client(manager, shutdown.clone(), &tasks).await;
        let final_geometry = geometry(1234);

        client
            .queue_main_window_save(final_geometry.clone())
            .expect("save should be accepted before shutdown");
        shutdown.cancel();
        tasks.close();
        tasks.wait().await;

        assert_eq!(client.main_window_geometry(), Some(final_geometry));
        assert!(client.queue_main_window_save(geometry(4321)).is_err());
    }
}
