mod error;
mod event_sink;
mod state;

use self::state::{StateClient, VergePatchRoute, route_verge_patch};
use crate::{
    config::{Config, IVerge, Profiles, ProfilesBuilder},
    core::{CoreManager, RunType},
    state::verge::VergeMirror,
};
use nyanpasu_ipc::api::status::CoreState;
use std::{borrow::Cow, future::Future, sync::Arc};
use tokio::sync::Mutex;

pub use error::{ClientError, Result};
pub use event_sink::{TauriUiEventSink, UiEventSink};

#[derive(Clone)]
pub struct NyanpasuClient {
    inner: Arc<NyanpasuClientInner>,
}

struct NyanpasuClientInner {
    ui: Arc<dyn UiEventSink>,
    state: StateClient,
    /// Serializes all verge mutations funneled through this client (IPC patches,
    /// legacy reseeds). The actor serializes its own state, but the legacy path holds
    /// `Config::verge()` draft across awaits, so client-level mutations must not interleave.
    verge_update_lock: Mutex<()>,
}

impl NyanpasuClient {
    pub fn try_new(ui: Arc<dyn UiEventSink>) -> anyhow::Result<Self> {
        let initial = Config::verge().data().clone();
        let state = StateClient::new(initial, legacy_verge_mirror())?;
        Ok(Self::with_state(ui, state))
    }

    fn with_state(ui: Arc<dyn UiEventSink>, state: StateClient) -> Self {
        Self {
            inner: Arc::new(NyanpasuClientInner {
                ui,
                state,
                verge_update_lock: Mutex::new(()),
            }),
        }
    }

    pub async fn replace_verge_config(&self, state: IVerge) -> Result<()> {
        let _guard = self.inner.verge_update_lock.lock().await;
        self.replace_verge_unlocked(state).await
    }

    async fn replace_verge_unlocked(&self, state: IVerge) -> Result<()> {
        self.inner.state.replace_verge(state).await?;
        Ok(())
    }

    /// Run a legacy mutation that writes `Config::verge()` directly (e.g. core change,
    /// window-state save), then reseed the actor from the post-mutation legacy state.
    /// Every legacy verge writer that bypasses the actor must go through this, otherwise
    /// a later actor commit would persist a stale snapshot and clobber the legacy change.
    pub async fn run_legacy_verge_mutation<F, Fut>(&self, mutate: F) -> Result<()>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<()>>,
    {
        let _guard = self.inner.verge_update_lock.lock().await;
        mutate().await?;
        // Bind the clone to a local so the `Config::verge()` guard is dropped before the
        // await (a held parking_lot guard would make this future !Send).
        let committed = Config::verge().data().clone();
        self.replace_verge_unlocked(committed).await
    }

    pub fn get_profiles(&self) -> Profiles {
        Config::profiles().data().clone()
    }

    pub async fn patch_profiles_config(&self, profiles: ProfilesBuilder) -> Result<()> {
        Config::profiles().draft().apply(profiles);

        match CoreManager::global().update_config().await {
            Ok(_) => {
                self.inner.ui.refresh_clash();
                Config::profiles().apply();
                Config::profiles().data().save_file()?;

                let _ = crate::core::connection_interruption::ConnectionInterruptionService::on_profile_change().await;

                Ok(())
            }
            Err(err) => {
                Config::profiles().discard();
                log::error!(target: "app", "{err:?}");
                Err(err.into())
            }
        }
    }

    pub async fn get_verge_config(&self) -> Result<IVerge> {
        Ok(self.inner.state.get_verge().await?)
    }

    pub async fn patch_verge_config(&self, payload: IVerge) -> Result<()> {
        // Each path locks exactly once: PureConfig locks here; LegacySideEffects locks
        // inside `run_legacy_verge_mutation` (the lock is not reentrant).
        match route_verge_patch(&payload) {
            VergePatchRoute::PureConfig => {
                let _guard = self.inner.verge_update_lock.lock().await;
                self.inner.state.patch_verge(payload).await?;
            }
            VergePatchRoute::LegacySideEffects => {
                self.run_legacy_verge_mutation(|| crate::feat::patch_verge(payload))
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn get_core_status(&self) -> (Cow<'static, CoreState>, i64, RunType) {
        CoreManager::global().status().await
    }
}

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(manager: &M) -> anyhow::Result<()> {
    let sink: Arc<dyn UiEventSink> = Arc::new(TauriUiEventSink::new(manager.app_handle().clone()));
    manager.manage(NyanpasuClient::try_new(sink)?);
    Ok(())
}

/// Production mirror: only updates the in-memory `Config::verge()`. The actor already
/// performs the atomic disk write, so the mirror must not call `save_file` again.
fn legacy_verge_mirror() -> VergeMirror {
    Arc::new(|state| {
        *Config::verge().draft() = state;
        Config::verge().apply();
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client::event_sink::NoopUiEventSink, ipc::IpcError};
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    fn test_state_client() -> (StateClient, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let path = Utf8PathBuf::from_path_buf(dir.path().join("nyanpasu-config.yaml"))
            .expect("temp path should be UTF-8");
        let mirror: VergeMirror = Arc::new(|_| Ok(()));
        let state = StateClient::new_with_path(path, IVerge::default(), mirror)
            .expect("state client should be created");
        (state, dir)
    }

    #[test]
    fn client_constructs_without_tauri_runtime() {
        let (state, _dir) = test_state_client();
        let client = NyanpasuClient::with_state(Arc::new(NoopUiEventSink), state);
        let _ = client.clone();
    }

    #[test]
    fn client_error_bridges_to_ipc_error() {
        assert!(matches!(
            IpcError::from(ClientError::Custom("boom".into())),
            IpcError::Custom(msg) if msg == "boom"
        ));
        assert!(matches!(
            IpcError::from(ClientError::Io(std::io::Error::other("io"))),
            IpcError::Io(_)
        ));
        assert!(matches!(
            IpcError::from(ClientError::Anyhow(anyhow::anyhow!("oops"))),
            IpcError::Anyhow(_)
        ));
    }
}
