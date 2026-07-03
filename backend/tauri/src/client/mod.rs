mod application;
mod clash_config;
mod error;
mod event_sink;
mod session_state;
mod state;

use self::{
    application::ApplicationClient,
    clash_config::ClashConfigClient,
    session_state::SessionStateClient,
    state::{StateClient, VergePatchRoute, route_verge_patch},
};
use crate::{
    bridge::{
        TypedConfigPatchPlan, clash::LegacyClashBridge, legacy_iverge_from_typed,
        typed_config_from_legacy, typed_patches_from_legacy_patch, verge::LegacyVergeBridge,
        window::LegacyWindowBridge,
    },
    config::{Config, IVerge, Profiles, ProfilesBuilder},
    core::{CoreManager, RunType},
    state::{
        mirror::{
            ClashLegacyBridge as ClashLegacyBridgeTrait,
            VergeLegacyBridge as VergeLegacyBridgeTrait,
            WindowLegacyBridge as WindowLegacyBridgeTrait,
        },
        verge::{VergeMirror, validate_verge_patch},
    },
    utils::path::PathResolver,
};
use camino::Utf8PathBuf;
use nyanpasu_config::{
    application::{NyanpasuAppConfig, NyanpasuAppConfigPatch},
    clash::config::{ClashConfig, ClashConfigPatch},
    state::{PersistentState, PersistentStatePatch},
};
use nyanpasu_ipc::api::status::CoreState;
use std::{borrow::Cow, future::Future, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

pub use error::{ClientError, Result};
pub use event_sink::{TauriUiEventSink, UiEventSink};

pub struct ClientSetupArgs {
    pub ui: Arc<dyn UiEventSink>,
    pub paths: PathResolver,
    pub bridges: LegacyBridgeSet,
}

#[derive(Clone)]
pub struct LegacyBridgeSet {
    pub verge: Arc<dyn VergeLegacyBridgeTrait>,
    pub window: Arc<dyn WindowLegacyBridgeTrait>,
    pub clash: Arc<dyn ClashLegacyBridgeTrait>,
}

impl LegacyBridgeSet {
    pub fn tauri() -> Self {
        Self {
            verge: Arc::new(LegacyVergeBridge),
            window: Arc::new(LegacyWindowBridge),
            clash: Arc::new(LegacyClashBridge),
        }
    }
}

#[derive(Clone)]
pub struct NyanpasuClient {
    inner: Arc<NyanpasuClientInner>,
}

struct TypedConfigClients {
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
}

impl TypedConfigClients {
    async fn new(paths: PathResolver, bridges: LegacyBridgeSet) -> anyhow::Result<Self> {
        let application = ApplicationClient::new(
            utf8_path(paths.application_config_path())?,
            bridges.verge.snapshot_legacy()?,
            bridges.verge.clone(),
        )
        .await?;

        let session_state = SessionStateClient::new(
            utf8_path(paths.session_state_path())?,
            bridges.window.snapshot_legacy()?,
            bridges.window.clone(),
        )
        .await?;

        let clash_config = ClashConfigClient::new(
            utf8_path(paths.clash_config_path())?,
            bridges.clash.snapshot_legacy()?,
            bridges.clash.clone(),
        )
        .await?;

        Ok(Self {
            application,
            session_state,
            clash_config,
        })
    }
}

struct NyanpasuClientInner {
    ui: Arc<dyn UiEventSink>,
    state: StateClient,
    typed_config: Option<TypedConfigClients>,
    /// Serializes all verge mutations funneled through this client (IPC patches,
    /// legacy reseeds). The actor serializes its own state, but the legacy path holds
    /// `Config::verge()` draft across awaits, so client-level mutations must not interleave.
    verge_update_lock: Mutex<()>,
}

impl NyanpasuClient {
    pub fn try_new(ui: Arc<dyn UiEventSink>) -> anyhow::Result<Self> {
        Self::try_new_with_args(ClientSetupArgs {
            ui,
            paths: PathResolver::from_env()?,
            bridges: LegacyBridgeSet::tauri(),
        })
    }

    pub fn try_new_with_args(args: ClientSetupArgs) -> anyhow::Result<Self> {
        let ClientSetupArgs { ui, paths, bridges } = args;
        let initial = Config::verge().data().clone();
        let state = StateClient::new_with_path(
            utf8_path(paths.nyanpasu_config_path())?,
            initial,
            legacy_verge_mirror(),
        )?;
        let typed_config = tauri::async_runtime::block_on(TypedConfigClients::new(paths, bridges))?;
        Ok(Self::with_state_and_typed_config(
            ui,
            state,
            Some(typed_config),
        ))
    }

    #[cfg(test)]
    fn with_state(ui: Arc<dyn UiEventSink>, state: StateClient) -> Self {
        Self::with_state_and_typed_config(ui, state, None)
    }

    fn with_state_and_typed_config(
        ui: Arc<dyn UiEventSink>,
        state: StateClient,
        typed_config: Option<TypedConfigClients>,
    ) -> Self {
        Self {
            inner: Arc::new(NyanpasuClientInner {
                ui,
                state,
                typed_config,
                verge_update_lock: Mutex::new(()),
            }),
        }
    }

    fn typed_config(&self) -> Result<&TypedConfigClients> {
        self.inner.typed_config.as_ref().ok_or_else(|| {
            ClientError::Custom(
                "typed config actors are not initialized in the legacy setup path".into(),
            )
        })
    }

    pub async fn get_app_config(&self) -> Result<NyanpasuAppConfig> {
        let client = self.typed_config()?.application.clone();
        Ok(client.get().await?.state)
    }

    pub async fn patch_app_config(&self, patch: NyanpasuAppConfigPatch) -> Result<()> {
        let client = self.typed_config()?.application.clone();
        client.patch(patch).await?;
        Ok(())
    }

    pub async fn replace_app_config(&self, state: NyanpasuAppConfig) -> Result<()> {
        let client = self.typed_config()?.application.clone();
        client.replace(state).await?;
        Ok(())
    }

    pub async fn get_session_state(&self) -> Result<PersistentState> {
        let client = self.typed_config()?.session_state.clone();
        Ok(client.get().await?.state)
    }

    pub async fn patch_session_state(&self, patch: PersistentStatePatch) -> Result<()> {
        let client = self.typed_config()?.session_state.clone();
        client.patch(patch).await?;
        Ok(())
    }

    pub async fn replace_session_state(&self, state: PersistentState) -> Result<()> {
        let client = self.typed_config()?.session_state.clone();
        client.replace(state).await?;
        Ok(())
    }

    pub async fn get_clash_config(&self) -> Result<ClashConfig> {
        let client = self.typed_config()?.clash_config.clone();
        Ok(client.get().await?.state)
    }

    pub async fn patch_clash_config(&self, patch: ClashConfigPatch) -> Result<()> {
        let client = self.typed_config()?.clash_config.clone();
        client.patch(patch).await?;
        Ok(())
    }

    pub async fn replace_clash_config(&self, state: ClashConfig) -> Result<()> {
        let client = self.typed_config()?.clash_config.clone();
        client.replace(state).await?;
        Ok(())
    }

    pub async fn replace_verge_config(&self, state: IVerge) -> Result<()> {
        let _guard = self.inner.verge_update_lock.lock().await;
        self.replace_verge_unlocked(state).await
    }

    async fn replace_verge_unlocked(&self, state: IVerge) -> Result<()> {
        self.inner.state.replace_verge(state).await?;
        self.reseed_typed_config_from_legacy().await
    }

    async fn apply_typed_config_patch_plan(&self, plan: TypedConfigPatchPlan) -> Result<()> {
        let Some(typed) = self.inner.typed_config.as_ref() else {
            return Ok(());
        };

        if let Some(patch) = plan.application {
            typed.application.clone().patch(patch).await?;
        }
        if let Some(patch) = plan.session_state {
            typed.session_state.clone().patch(patch).await?;
        }
        if let Some(patch) = plan.clash_config {
            typed.clash_config.clone().patch(patch).await?;
        }

        Ok(())
    }

    async fn reseed_typed_config_from_legacy(&self) -> Result<()> {
        let Some(typed) = self.inner.typed_config.as_ref() else {
            return Ok(());
        };

        let legacy = self.inner.state.get_verge().await?;
        let (app, session, clash) = typed_config_from_legacy(&legacy)?;

        typed.application.clone().replace(app).await?;
        typed.session_state.clone().replace(session).await?;
        typed.clash_config.clone().replace(clash).await?;
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
        let Some(typed) = self.inner.typed_config.as_ref() else {
            return Ok(self.inner.state.get_verge().await?);
        };

        let base = self.inner.state.get_verge().await?;
        let app = typed.application.clone().get().await?.state;
        let session = typed.session_state.clone().get().await?.state;
        let clash = typed.clash_config.clone().get().await?.state;
        Ok(legacy_iverge_from_typed(base, &app, &session, &clash)?)
    }

    pub async fn patch_verge_config(&self, payload: IVerge) -> Result<()> {
        // Each path locks exactly once: PureConfig locks here; LegacySideEffects locks
        // inside `run_legacy_verge_mutation` (the lock is not reentrant).
        match route_verge_patch(&payload) {
            VergePatchRoute::PureConfig => {
                let _guard = self.inner.verge_update_lock.lock().await;
                if self.inner.typed_config.is_none() {
                    self.inner.state.patch_verge(payload).await?;
                    return Ok(());
                }

                validate_verge_patch(&payload)?;
                let base = self.get_verge_config().await?;
                let plan = typed_patches_from_legacy_patch(base, &payload)?;
                self.apply_typed_config_patch_plan(plan).await?;
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

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(
    manager: &M,
    paths: PathResolver,
) -> anyhow::Result<()> {
    let sink: Arc<dyn UiEventSink> = Arc::new(TauriUiEventSink::new(manager.app_handle().clone()));
    manager.manage(NyanpasuClient::try_new_with_args(ClientSetupArgs {
        ui: sink,
        paths,
        bridges: LegacyBridgeSet::tauri(),
    })?);
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

fn utf8_path(path: PathBuf) -> anyhow::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("config path is not UTF-8: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        client::event_sink::NoopUiEventSink,
        ipc::IpcError,
        state::mirror::{ClashLegacyBridge, VergeLegacyBridge, WindowLegacyBridge},
    };
    use camino::Utf8PathBuf;
    use nyanpasu_config::state::window::{WindowLabel, WindowState};
    use std::collections::BTreeMap;
    use struct_patch::Patch;
    use tempfile::{TempDir, tempdir};

    struct NoopVergeBridge;

    impl VergeLegacyBridge for NoopVergeBridge {
        fn mirror(&self, _snap: &NyanpasuAppConfig) -> anyhow::Result<()> {
            Ok(())
        }

        fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
            Ok(NyanpasuAppConfig::default())
        }
    }

    struct NoopWindowBridge;

    impl WindowLegacyBridge for NoopWindowBridge {
        fn mirror(&self, _snap: &PersistentState) -> anyhow::Result<()> {
            Ok(())
        }

        fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
            Ok(PersistentState::default())
        }
    }

    struct NoopClashBridge;

    impl ClashLegacyBridge for NoopClashBridge {
        fn mirror(&self, _snap: &ClashConfig) -> anyhow::Result<()> {
            Ok(())
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
        }
    }

    fn temp_config_path(dir: &TempDir, file_name: &str) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join(file_name)).expect("temp path should be UTF-8")
    }

    fn test_state_client() -> (StateClient, TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let path = temp_config_path(&dir, "nyanpasu-config.yaml");
        let mirror: VergeMirror = Arc::new(|_| Ok(()));
        let state = StateClient::new_with_path(path, IVerge::default(), mirror)
            .expect("state client should be created");
        (state, dir)
    }

    async fn test_typed_config_clients(dir: &TempDir) -> TypedConfigClients {
        TypedConfigClients {
            application: ApplicationClient::new(
                temp_config_path(dir, "application.yaml"),
                NyanpasuAppConfig::default(),
                Arc::new(NoopVergeBridge),
            )
            .await
            .expect("application client should be created"),
            session_state: SessionStateClient::new(
                temp_config_path(dir, "session-state.yaml"),
                PersistentState::default(),
                Arc::new(NoopWindowBridge),
            )
            .await
            .expect("session state client should be created"),
            clash_config: ClashConfigClient::new(
                temp_config_path(dir, "clash-config.yaml"),
                ClashConfig::default(),
                Arc::new(NoopClashBridge),
            )
            .await
            .expect("clash config client should be created"),
        }
    }

    #[test]
    fn client_constructs_without_tauri_runtime() {
        let (state, _dir) = test_state_client();
        let client = NyanpasuClient::with_state(Arc::new(NoopUiEventSink), state);
        let _ = client.clone();
    }

    #[test]
    fn typed_config_facade_is_not_initialized_by_legacy_setup() {
        let (state, _dir) = test_state_client();
        let client = NyanpasuClient::with_state(Arc::new(NoopUiEventSink), state);

        tauri::async_runtime::block_on(async {
            assert!(matches!(
                client.get_app_config().await,
                Err(ClientError::Custom(message)) if message.contains("typed config actors")
            ));
        });
    }

    #[test]
    fn typed_config_facade_delegates_to_typed_clients() {
        let (state, dir) = test_state_client();

        tauri::async_runtime::block_on(async {
            let typed_config = test_typed_config_clients(&dir).await;
            let client = NyanpasuClient::with_state_and_typed_config(
                Arc::new(NoopUiEventSink),
                state,
                Some(typed_config),
            );

            let mut app_patch = NyanpasuAppConfig::new_empty_patch();
            app_patch.enable_system_proxy = Some(true);
            client
                .patch_app_config(app_patch)
                .await
                .expect("app patch should succeed");
            assert!(client.get_app_config().await.unwrap().enable_system_proxy);

            let mut app_replacement = NyanpasuAppConfig::default();
            app_replacement.enable_silent_start = true;
            client
                .replace_app_config(app_replacement)
                .await
                .expect("app replace should succeed");
            assert!(client.get_app_config().await.unwrap().enable_silent_start);

            let window_label = WindowLabel("main".into());
            let window_state = WindowState {
                width: 1024,
                height: 768,
                x: 10,
                y: 20,
                maximized: false,
                fullscreen: false,
            };
            let mut session_patch = PersistentState::new_empty_patch();
            session_patch.window_state = Some(BTreeMap::from([(
                window_label.clone(),
                window_state.clone(),
            )]));
            client
                .patch_session_state(session_patch)
                .await
                .expect("session patch should succeed");
            assert_eq!(
                client
                    .get_session_state()
                    .await
                    .unwrap()
                    .window_state
                    .get(&window_label),
                Some(&window_state)
            );

            client
                .replace_session_state(PersistentState::default())
                .await
                .expect("session replace should succeed");
            assert!(
                client
                    .get_session_state()
                    .await
                    .unwrap()
                    .window_state
                    .is_empty()
            );

            let mut clash_patch = ClashConfig::new_empty_patch();
            clash_patch.enable_tun_mode = Some(true);
            client
                .patch_clash_config(clash_patch)
                .await
                .expect("clash patch should succeed");
            assert!(client.get_clash_config().await.unwrap().enable_tun_mode);

            client
                .replace_clash_config(ClashConfig::default())
                .await
                .expect("clash replace should succeed");
            assert!(!client.get_clash_config().await.unwrap().enable_tun_mode);
        });
    }

    #[test]
    fn get_verge_config_composes_typed_actor_snapshots() {
        let (state, dir) = test_state_client();

        tauri::async_runtime::block_on(async {
            let typed_config = test_typed_config_clients(&dir).await;
            let client = NyanpasuClient::with_state_and_typed_config(
                Arc::new(NoopUiEventSink),
                state,
                Some(typed_config),
            );

            let mut app_patch = NyanpasuAppConfig::new_empty_patch();
            app_patch.enable_system_proxy = Some(true);
            client
                .patch_app_config(app_patch)
                .await
                .expect("app patch should succeed");

            let window_label = WindowLabel("main".into());
            let window_state = WindowState {
                width: 1024,
                height: 768,
                x: 10,
                y: 20,
                maximized: false,
                fullscreen: false,
            };
            let mut session_patch = PersistentState::new_empty_patch();
            session_patch.window_state =
                Some(BTreeMap::from([(window_label, window_state.clone())]));
            client
                .patch_session_state(session_patch)
                .await
                .expect("session patch should succeed");

            let mut clash_patch = ClashConfig::new_empty_patch();
            clash_patch.enable_tun_mode = Some(true);
            client
                .patch_clash_config(clash_patch)
                .await
                .expect("clash patch should succeed");

            let verge = client
                .get_verge_config()
                .await
                .expect("legacy verge config should compose from typed snapshots");
            assert_eq!(verge.enable_system_proxy, Some(true));
            assert_eq!(verge.enable_tun_mode, Some(true));
            assert_eq!(
                verge.window_size_state.as_ref().map(|state| state.width),
                Some(window_state.width)
            );
        });
    }

    #[test]
    fn legacy_patch_then_get_verge_config_preserves_contract() {
        let (state, dir) = test_state_client();

        tauri::async_runtime::block_on(async {
            let typed_config = test_typed_config_clients(&dir).await;
            let client = NyanpasuClient::with_state_and_typed_config(
                Arc::new(NoopUiEventSink),
                state,
                Some(typed_config),
            );

            client
                .patch_verge_config(IVerge {
                    theme_color: Some("#112233".into()),
                    ..IVerge::default()
                })
                .await
                .expect("legacy patch should succeed");

            let verge = client
                .get_verge_config()
                .await
                .expect("legacy verge config should read patched value");
            assert_eq!(verge.theme_color.as_deref(), Some("#112233"));
            assert_eq!(
                client
                    .get_app_config()
                    .await
                    .unwrap()
                    .theme_color
                    .to_string(),
                "#112233"
            );
        });
    }

    #[test]
    fn patch_verge_config_window_state_updates_session_actor() {
        let (state, dir) = test_state_client();

        tauri::async_runtime::block_on(async {
            let typed_config = test_typed_config_clients(&dir).await;
            let client = NyanpasuClient::with_state_and_typed_config(
                Arc::new(NoopUiEventSink),
                state,
                Some(typed_config),
            );

            let window_state = crate::config::nyanpasu::WindowState {
                width: 1280,
                height: 720,
                x: 30,
                y: 40,
                maximized: false,
                fullscreen: false,
            };
            client
                .patch_verge_config(IVerge {
                    window_size_state: Some(window_state.clone()),
                    ..IVerge::default()
                })
                .await
                .expect("window state patch should succeed");

            let session = client.get_session_state().await.unwrap();
            assert_eq!(
                session
                    .window_state
                    .get(&WindowLabel("main".into()))
                    .map(|state| (state.width, state.height, state.x, state.y)),
                Some((
                    window_state.width,
                    window_state.height,
                    window_state.x,
                    window_state.y
                ))
            );
            assert_eq!(
                client
                    .get_verge_config()
                    .await
                    .unwrap()
                    .window_size_state
                    .as_ref()
                    .map(|state| state.width),
                Some(window_state.width)
            );
        });
    }

    #[test]
    fn patch_verge_config_clash_field_updates_persistent_clash_actor() {
        let (state, dir) = test_state_client();

        tauri::async_runtime::block_on(async {
            let typed_config = test_typed_config_clients(&dir).await;
            let client = NyanpasuClient::with_state_and_typed_config(
                Arc::new(NoopUiEventSink),
                state,
                Some(typed_config),
            );

            client
                .patch_verge_config(IVerge {
                    web_ui_list: Some(vec!["dashboard".into(), "yacd".into()]),
                    enable_clash_fields: Some(false),
                    ..IVerge::default()
                })
                .await
                .expect("clash-owned pure patch should succeed");

            let clash = client.get_clash_config().await.unwrap();
            assert_eq!(clash.web_ui_list, vec!["dashboard", "yacd"]);
            assert!(!clash.enable_clash_fields);

            let verge = client.get_verge_config().await.unwrap();
            assert_eq!(
                verge.web_ui_list,
                Some(vec!["dashboard".into(), "yacd".into()])
            );
            assert_eq!(verge.enable_clash_fields, Some(false));
        });
    }

    #[test]
    #[allow(deprecated)]
    fn patch_verge_config_deprecated_auto_log_clean_stays_on_legacy_path() {
        let (state, dir) = test_state_client();

        tauri::async_runtime::block_on(async {
            let typed_config = test_typed_config_clients(&dir).await;
            let client = NyanpasuClient::with_state_and_typed_config(
                Arc::new(NoopUiEventSink),
                state,
                Some(typed_config),
            );

            client
                .patch_verge_config(IVerge {
                    auto_log_clean: Some(30),
                    ..IVerge::default()
                })
                .await
                .expect("deprecated legacy field should still be preserved");

            assert_eq!(
                client.get_verge_config().await.unwrap().auto_log_clean,
                Some(30)
            );
        });
    }

    #[test]
    fn legacy_mutation_reseeds_typed_actors_without_os_side_effects() {
        let (state, dir) = test_state_client();

        tauri::async_runtime::block_on(async {
            let typed_config = test_typed_config_clients(&dir).await;
            let client = NyanpasuClient::with_state_and_typed_config(
                Arc::new(NoopUiEventSink),
                state,
                Some(typed_config),
            );

            client
                .run_legacy_verge_mutation(|| async {
                    Config::verge().draft().patch_config(IVerge {
                        theme_color: Some("#445566".into()),
                        ..IVerge::default()
                    });
                    Config::verge().apply();
                    Ok(())
                })
                .await
                .expect("legacy mutation should reseed typed actors");

            assert_eq!(
                client
                    .get_app_config()
                    .await
                    .unwrap()
                    .theme_color
                    .to_string(),
                "#445566"
            );
            assert_eq!(
                client
                    .get_verge_config()
                    .await
                    .unwrap()
                    .theme_color
                    .as_deref(),
                Some("#445566")
            );
        });
    }

    #[test]
    fn try_new_with_args_constructs_typed_config_facade() {
        let dir = tempdir().expect("tempdir should be created");
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
            ui: Arc::new(NoopUiEventSink),
            paths,
            bridges: LegacyBridgeSet {
                verge: Arc::new(NoopVergeBridge),
                window: Arc::new(NoopWindowBridge),
                clash: Arc::new(NoopClashBridge),
            },
        })
        .expect("client should construct with typed config actors");

        tauri::async_runtime::block_on(async {
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.enable_system_proxy = Some(true);
            client
                .patch_app_config(patch)
                .await
                .expect("typed app patch should succeed");
            assert!(client.get_app_config().await.unwrap().enable_system_proxy);
        });
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
