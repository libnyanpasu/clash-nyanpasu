mod error;
mod event_sink;
mod profiles_state;
mod state;

use self::{
    profiles_state::ProfilesStateClient,
    state::{StateClient, VergePatchRoute, route_verge_patch},
};
use crate::{
    config::{
        Config, IVerge, Profile, ProfileCleanup, ProfileMetaGetter, Profiles, ProfilesBuilder,
        RemoteProfileOptionsBuilder, profile::ProfileBuilder,
    },
    core::{CoreManager, RunType, connection_interruption::ConnectionInterruptionService},
    state::{profiles::ProfilesMirror, verge::VergeMirror},
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
    profiles: ProfilesStateClient,
    /// Guards the "index commit + core reload" window for every profiles mutation. Network
    /// IO stays outside this lock (two-phase). Non-reentrant: each method locks exactly once.
    profiles_update_lock: Mutex<()>,
}

impl NyanpasuClient {
    pub fn try_new(ui: Arc<dyn UiEventSink>) -> anyhow::Result<Self> {
        let verge = StateClient::new(Config::verge().data().clone(), legacy_verge_mirror())?;
        let profiles =
            ProfilesStateClient::new(Config::profiles().data().clone(), legacy_profiles_mirror())?;
        Ok(Self::with_state(ui, verge, profiles))
    }

    fn with_state(
        ui: Arc<dyn UiEventSink>,
        state: StateClient,
        profiles: ProfilesStateClient,
    ) -> Self {
        Self {
            inner: Arc::new(NyanpasuClientInner {
                ui,
                state,
                verge_update_lock: Mutex::new(()),
                profiles,
                profiles_update_lock: Mutex::new(()),
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

    /// Read the profiles index through the actor (consistent with `Config::profiles()`).
    /// On an actor RPC error, fall back to the committed `Config` value, which is never
    /// stale (the actor and `Config` stay in lockstep, §3-H).
    pub async fn get_profiles(&self) -> Result<Profiles> {
        match self.inner.profiles.get_profiles().await {
            Ok(profiles) => Ok(profiles),
            Err(err) => {
                log::error!(target: "app", "profiles actor read failed, fallback to Config: {err:#}");
                Ok(Config::profiles().data().clone())
            }
        }
    }

    // —— Commit primitives (all assume the `profiles_update_lock` is held). Each command
    //    picks one to preserve its current error semantics (§9). ——

    /// Persist the index only (no reload). On `upsert` failure `Config` is unchanged and the
    /// command returns `Err` (the mirror never runs).
    async fn persist(&self, next: Profiles) -> Result<()> {
        debug_assert!(
            !Config::profiles().is_dirty(),
            "no pending profiles draft expected before commit"
        );
        self.inner.profiles.commit(next).await?;
        Ok(())
    }

    /// persist-first: commit the index (failure = `Err`), then reload the core; a reload
    /// failure is `Err` when `reload_strict`, otherwise log-only. Matches today's
    /// delete / update / patch_profile (persist, then strict/best-effort reload).
    async fn persist_then_reload(&self, next: Profiles, reload_strict: bool) -> Result<()> {
        self.persist(next).await?;
        match CoreManager::global().update_config().await {
            Ok(_) => {
                self.inner.ui.refresh_clash();
                Ok(())
            }
            Err(err) => {
                if reload_strict {
                    Err(err.into())
                } else {
                    log::error!(target: "app", "{err:?}");
                    Ok(())
                }
            }
        }
    }

    /// reload-first transaction: expose `next` to `enhance` via the draft, reload, and only
    /// commit the index on success; any failure discards the draft (nothing persisted).
    /// Matches today's patch_profiles_config / create-activation (reload failure rolls back).
    async fn reload_then_persist(&self, next: Profiles) -> Result<()> {
        // The draft guard is a temporary dropped at the end of this statement, so it is
        // never held across the await below.
        *Config::profiles().draft() = next.clone();
        match CoreManager::global().update_config().await {
            Ok(_) => {
                self.inner.ui.refresh_clash();
                match self.inner.profiles.commit(next).await {
                    Ok(_) => Ok(()),
                    Err(err) => {
                        Config::profiles().discard();
                        Err(err.into())
                    }
                }
            }
            Err(err) => {
                Config::profiles().discard();
                Err(err.into())
            }
        }
    }

    pub async fn reorder_profile(&self, active_id: String, over_id: String) -> Result<()> {
        let _guard = self.inner.profiles_update_lock.lock().await;
        let mut next = Config::profiles().data().clone();
        crate::config::reorder_items(&mut next.items, &active_id, &over_id);
        self.persist(next).await
    }

    pub async fn reorder_profiles_by_list(&self, order: Vec<String>) -> Result<()> {
        let _guard = self.inner.profiles_update_lock.lock().await;
        let mut next = Config::profiles().data().clone();
        crate::config::reorder_items_by_list(&mut next.items, &order);
        self.persist(next).await
    }

    pub async fn patch_profiles_config(&self, patch: ProfilesBuilder) -> Result<()> {
        let _guard = self.inner.profiles_update_lock.lock().await;
        let mut next = Config::profiles().data().clone();
        next.apply(patch);
        self.reload_then_persist(next).await?;
        drop(_guard);
        // §3-I: run the interruption side-effect outside the commit window.
        let _ = ConnectionInterruptionService::on_profile_change().await;
        Ok(())
    }

    pub async fn patch_profile(
        &self,
        uid: String,
        profile: ProfileBuilder,
    ) -> Result<ProfileMutationReport> {
        let _guard = self.inner.profiles_update_lock.lock().await;
        let mut next = Config::profiles().data().clone();
        next.apply_item_patch(uid.clone(), profile)?;
        if profile_in_active_chain(&next, &uid) {
            // §9: today's patch_profile reloads best-effort (commit failure = Err, reload
            // failure = log-only / command Ok).
            self.persist_then_reload(next, false).await?;
        } else {
            self.persist(next).await?;
        }
        Ok(ProfileMutationReport { refresh_jobs: true })
    }

    pub async fn update_profile(
        &self,
        uid: String,
        opts: Option<RemoteProfileOptionsBuilder>,
    ) -> Result<()> {
        // §3-D: the network download runs before the lock is taken.
        let prepared = crate::feat::prepare_profile_update(&uid, opts).await?;
        let _guard = self.inner.profiles_update_lock.lock().await;
        let mut next = Config::profiles().data().clone();
        // §3-E: TOCTOU fingerprint check + pure in-memory write into `next`.
        let reload = crate::feat::commit_profile_update(&mut next, prepared)?;
        if reload {
            // §9: today's update is persist-first + strict reload + SetConfig notice.
            self.persist(next).await?;
            match CoreManager::global().update_config().await {
                Ok(_) => {
                    self.inner.ui.refresh_clash();
                    self.inner.ui.notice_set_config(Ok(()));
                    Ok(())
                }
                Err(err) => {
                    self.inner.ui.notice_set_config(Err(format!("{err:?}")));
                    Err(err.into())
                }
            }
        } else {
            self.persist(next).await
        }
    }

    pub async fn create_profile(&self, item: Profile, activate_if_first: bool) -> Result<()> {
        // `item` was built (incl. remote download / file IO) outside the lock. The activation
        // decision happens inside the lock so concurrent creates cannot both claim "first".
        let _guard = self.inner.profiles_update_lock.lock().await;
        let mut next = Config::profiles().data().clone();
        let uid = item.uid().to_string();
        let activate =
            activate_if_first && next.current.is_empty() && (item.is_local() || item.is_remote());
        next.push_item(item);
        if activate {
            next.set_current(vec![uid]);
            // Merge append + activation into a single reload-first transaction (reload
            // failure rolls the whole creation back).
            self.reload_then_persist(next).await?;
        } else {
            self.persist(next).await?;
        }
        drop(_guard);
        if activate {
            let _ = ConnectionInterruptionService::on_profile_change().await;
        }
        Ok(())
    }

    pub async fn delete_profile(&self, uid: String) -> Result<()> {
        let _guard = self.inner.profiles_update_lock.lock().await;
        let mut next = Config::profiles().data().clone();
        let (removed, was_current) = next.remove_item(&uid);
        // Persist the index removal first (state failure = Err, nothing changed).
        self.persist(next).await?;
        // §9: today's delete reloads strictly when the active profile changed. Capture the
        // reload result instead of `?`-ing so the content file is still removed below (§5:
        // the index commit stands even on reload failure).
        let reload_result = if was_current {
            match CoreManager::global().update_config().await {
                Ok(_) => {
                    self.inner.ui.refresh_clash();
                    Ok(())
                }
                Err(err) => Err(err.into()),
            }
        } else {
            Ok(())
        };
        drop(_guard);
        // Delete the content file after the index commit regardless of the reload outcome
        // (orphan-safe); failure is log-only and the index removal stands.
        if let Some(mut item) = removed
            && let Err(err) = item.remove_file().await
        {
            log::error!(target: "app", "failed to remove profile file: {err:?}");
        }
        reload_result
    }

    pub async fn enhance_profiles(&self) -> Result<()> {
        // §3-I: reload only, no commit and no version bump.
        let _guard = self.inner.profiles_update_lock.lock().await;
        CoreManager::global().update_config().await?;
        self.inner.ui.refresh_clash();
        Ok(())
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

/// Result of a profile mutation that the IPC layer must act on without coupling the client
/// to Tauri (`patch_profile` asks the command to refresh the scheduled jobs).
pub struct ProfileMutationReport {
    pub refresh_jobs: bool,
}

/// Whether `uid` participates in the active configuration, so a patch to it must reload the
/// core. Mirrors the legacy `ipc::patch_profile` `need_update` check, evaluated against `next`.
fn profile_in_active_chain(profiles: &Profiles, uid: &str) -> bool {
    if profiles.chain.iter().any(|u| u == uid) || profiles.current.iter().any(|u| u == uid) {
        return true;
    }
    profiles
        .current
        .iter()
        .any(|chain_uid| match profiles.get_item(chain_uid) {
            Ok(item) if item.is_local() => item.as_local().unwrap().chain.iter().any(|u| u == uid),
            Ok(item) if item.is_remote() => {
                item.as_remote().unwrap().chain.iter().any(|u| u == uid)
            }
            _ => false,
        })
}

/// Production mirror: only swaps the in-memory `Config::profiles()`. The actor already
/// performs the atomic disk write, so the mirror must not call `save_file` again. Infallible
/// by design (§3-J).
fn legacy_profiles_mirror() -> ProfilesMirror {
    Arc::new(|state| {
        *Config::profiles().draft() = state;
        Config::profiles().apply();
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client::event_sink::NoopUiEventSink, ipc::IpcError};
    use camino::Utf8PathBuf;
    use tempfile::tempdir;

    fn test_state_clients() -> (StateClient, ProfilesStateClient, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let verge_path = Utf8PathBuf::from_path_buf(dir.path().join("nyanpasu-config.yaml"))
            .expect("temp path should be UTF-8");
        let verge_mirror: VergeMirror = Arc::new(|_| Ok(()));
        let state = StateClient::new_with_path(verge_path, IVerge::default(), verge_mirror)
            .expect("state client should be created");

        let profiles_path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml"))
            .expect("temp path should be UTF-8");
        let profiles_mirror: ProfilesMirror = Arc::new(|_| {});
        let profiles =
            ProfilesStateClient::new_with_path(profiles_path, Profiles::default(), profiles_mirror)
                .expect("profiles state client should be created");
        (state, profiles, dir)
    }

    #[test]
    fn client_constructs_without_tauri_runtime() {
        let (state, profiles, _dir) = test_state_clients();
        let client = NyanpasuClient::with_state(Arc::new(NoopUiEventSink), state, profiles);
        let _ = client.clone();
    }

    /// Architecture guard: the actor is the sole persister of the profiles index. Only the
    /// sanctioned legacy bridge in `client/mod.rs` (the reload-first transaction + the
    /// infallible mirror) may drive `Config::profiles()` draft/commit primitives directly.
    /// Any other direct writer would bypass the actor and let `get_profiles` reads go stale.
    #[test]
    fn profiles_writers_are_confined_to_the_bridge() {
        use std::path::Path;

        const FORBIDDEN: [&str; 10] = [
            // Direct draft/commit primitives on the global profiles state.
            "Config::profiles().draft",
            "Config::profiles().apply",
            "Config::profiles().discard",
            "Config::profiles().auto_commit",
            // Legacy `save_file`-backed index mutators; every writer must go through the
            // actor instead, so these must not be invoked anywhere outside their definitions.
            ".append_item(",
            ".patch_item(",
            ".replace_item(",
            ".delete_item(",
            ".reorder(",
            ".reorder_by_list(",
        ];
        // Paths (relative to `src/`) sanctioned to host the bridge primitives. `profiles.rs`
        // *defines* the legacy wrappers (`fn append_item(` — no leading dot) so it does not
        // match the call-site needles above and need not be allow-listed.
        const ALLOWED: [&str; 1] = ["client/mod.rs"];

        fn scan(dir: &Path, src_root: &Path, offenders: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).expect("read_dir should succeed") {
                let path = entry.expect("dir entry should be readable").path();
                if path.is_dir() {
                    scan(&path, src_root, offenders);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let rel = path
                        .strip_prefix(src_root)
                        .expect("path is under src_root")
                        .to_string_lossy()
                        .replace('\\', "/");
                    if ALLOWED.contains(&rel.as_str()) {
                        continue;
                    }
                    let contents = std::fs::read_to_string(&path).expect("source should be UTF-8");
                    for needle in FORBIDDEN {
                        if contents.contains(needle) {
                            offenders.push(format!("{rel}: {needle}"));
                        }
                    }
                }
            }
        }

        let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        scan(&src_root, &src_root, &mut offenders);
        assert!(
            offenders.is_empty(),
            "direct Config::profiles() writers must go through the actor bridge; offenders: {offenders:#?}"
        );
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
