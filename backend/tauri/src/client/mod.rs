mod application;
mod clash_config;
mod error;
mod event_sink;
mod session_state;

use self::{
    application::ApplicationClient, clash_config::ClashConfigClient,
    session_state::SessionStateClient,
};
use crate::{
    state::mirror::{
        ClashLegacyBridge as ClashLegacyBridgeTrait, VergeLegacyBridge as VergeLegacyBridgeTrait,
        WindowLegacyBridge as WindowLegacyBridgeTrait,
    },
    utils::path::PathResolver,
};
use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::{
    application::{NyanpasuAppConfig, NyanpasuAppConfigPatch},
    clash::config::{ClashConfig, ClashConfigPatch},
    state::{PersistentState, PersistentStatePatch},
};
use std::{path::PathBuf, sync::Arc};

pub use error::{ClientError, Result};
pub use event_sink::{TauriUiEventSink, UiEventSink};

pub struct ClientSetupArgs {
    pub paths: PathResolver,
    pub bridges: LegacyBridgeSet,
}

#[derive(Clone)]
pub struct LegacyBridgeSet {
    pub verge: Arc<dyn VergeLegacyBridgeTrait>,
    pub window: Arc<dyn WindowLegacyBridgeTrait>,
    pub clash: Arc<dyn ClashLegacyBridgeTrait>,
}

#[derive(Clone)]
pub struct NyanpasuClient {
    inner: Arc<NyanpasuClientInner>,
}

async fn new_typed_config_clients(
    paths: PathResolver,
    bridges: LegacyBridgeSet,
) -> anyhow::Result<(ApplicationClient, SessionStateClient, ClashConfigClient)> {
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

    sync_legacy_mirrors(&application, &session_state, &clash_config, &bridges).await?;
    Ok((application, session_state, clash_config))
}

async fn sync_legacy_mirrors(
    application: &ApplicationClient,
    session_state: &SessionStateClient,
    clash_config: &ClashConfigClient,
    bridges: &LegacyBridgeSet,
) -> anyhow::Result<()> {
    let application = application
        .get()
        .await
        .context("failed to read loaded application config")?
        .state;
    bridges
        .verge
        .mirror(&application)
        .context("failed to mirror loaded application config into legacy state")?;

    let session_state = session_state
        .get()
        .await
        .context("failed to read loaded session state")?
        .state;
    bridges
        .window
        .mirror(&session_state)
        .context("failed to mirror loaded session state into legacy state")?;

    let clash_config = clash_config
        .get()
        .await
        .context("failed to read loaded clash config")?
        .state;
    bridges
        .clash
        .mirror(&clash_config)
        .context("failed to mirror loaded clash config into legacy state")?;

    Ok(())
}

struct NyanpasuClientInner {
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
}

impl NyanpasuClient {
    pub fn try_new_with_args(args: ClientSetupArgs) -> anyhow::Result<Self> {
        let ClientSetupArgs { paths, bridges } = args;
        let (application, session_state, clash_config) =
            tauri::async_runtime::block_on(new_typed_config_clients(paths, bridges))?;
        Ok(Self::with_typed_clients(
            application,
            session_state,
            clash_config,
        ))
    }

    fn with_typed_clients(
        application: ApplicationClient,
        session_state: SessionStateClient,
        clash_config: ClashConfigClient,
    ) -> Self {
        Self {
            inner: Arc::new(NyanpasuClientInner {
                application,
                session_state,
                clash_config,
            }),
        }
    }

    pub async fn get_app_config(&self) -> Result<NyanpasuAppConfig> {
        let client = self.inner.application.clone();
        Ok(client.get().await?.state)
    }

    pub async fn patch_app_config(&self, patch: NyanpasuAppConfigPatch) -> Result<()> {
        let client = self.inner.application.clone();
        client.patch(patch).await?;
        Ok(())
    }

    pub async fn replace_app_config(&self, state: NyanpasuAppConfig) -> Result<()> {
        let client = self.inner.application.clone();
        client.replace(state).await?;
        Ok(())
    }

    pub async fn get_session_state(&self) -> Result<PersistentState> {
        let client = self.inner.session_state.clone();
        Ok(client.get().await?.state)
    }

    pub async fn patch_session_state(&self, patch: PersistentStatePatch) -> Result<()> {
        let client = self.inner.session_state.clone();
        client.patch(patch).await?;
        Ok(())
    }

    pub async fn replace_session_state(&self, state: PersistentState) -> Result<()> {
        let client = self.inner.session_state.clone();
        client.replace(state).await?;
        Ok(())
    }

    pub async fn get_clash_config(&self) -> Result<ClashConfig> {
        let client = self.inner.clash_config.clone();
        Ok(client.get().await?.state)
    }

    pub async fn patch_clash_config(&self, patch: ClashConfigPatch) -> Result<()> {
        let client = self.inner.clash_config.clone();
        client.patch(patch).await?;
        Ok(())
    }

    pub async fn replace_clash_config(&self, state: ClashConfig) -> Result<()> {
        let client = self.inner.clash_config.clone();
        client.replace(state).await?;
        Ok(())
    }
}

fn utf8_path(path: PathBuf) -> anyhow::Result<Utf8PathBuf> {
    Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("config path is not UTF-8: {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::{ClashLegacyBridge, VergeLegacyBridge, WindowLegacyBridge};
    use camino::Utf8PathBuf;
    use nyanpasu_config::state::window::{WindowLabel, WindowState};
    use std::{collections::BTreeMap, sync::Mutex as StdMutex};
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

    struct RecordingVergeBridge {
        mirrored_theme_color: Arc<StdMutex<Option<String>>>,
    }

    impl VergeLegacyBridge for RecordingVergeBridge {
        fn mirror(&self, snap: &NyanpasuAppConfig) -> anyhow::Result<()> {
            *self
                .mirrored_theme_color
                .lock()
                .expect("mirror capture should not poison") = Some(snap.theme_color.to_string());
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

    async fn test_typed_config_clients(
        dir: &TempDir,
    ) -> (ApplicationClient, SessionStateClient, ClashConfigClient) {
        let application = ApplicationClient::new(
            temp_config_path(dir, "application.yaml"),
            NyanpasuAppConfig::default(),
            Arc::new(NoopVergeBridge),
        )
        .await
        .expect("application client should be created");
        let session_state = SessionStateClient::new(
            temp_config_path(dir, "session-state.yaml"),
            PersistentState::default(),
            Arc::new(NoopWindowBridge),
        )
        .await
        .expect("session state client should be created");
        let clash_config = ClashConfigClient::new(
            temp_config_path(dir, "clash-config.yaml"),
            ClashConfig::default(),
            Arc::new(NoopClashBridge),
        )
        .await
        .expect("clash config client should be created");

        (application, session_state, clash_config)
    }

    async fn test_client(dir: &TempDir) -> NyanpasuClient {
        let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
        NyanpasuClient::with_typed_clients(application, session_state, clash_config)
    }

    #[test]
    fn client_constructs_with_mandatory_typed_config_clients() {
        let dir = tempdir().expect("tempdir should be created");

        tauri::async_runtime::block_on(async {
            let client = test_client(&dir).await;
            let _ = client.clone();
        });
    }

    #[test]
    fn typed_config_facade_delegates_to_typed_clients() {
        let dir = tempdir().expect("tempdir should be created");

        tauri::async_runtime::block_on(async {
            let client = test_client(&dir).await;

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
    fn typed_setup_mirrors_loaded_state_to_legacy_bridges() {
        let dir = tempdir().expect("tempdir should be created");

        tauri::async_runtime::block_on(async {
            let (application, session_state, clash_config) = test_typed_config_clients(&dir).await;
            let mut patch = NyanpasuAppConfig::new_empty_patch();
            patch.theme_color = Some(serde_yaml::from_str("\"#123456\"").unwrap());
            application
                .patch(patch)
                .await
                .expect("typed application patch should persist");
            drop(application);
            drop(session_state);
            drop(clash_config);

            let mirrored_theme_color = Arc::new(StdMutex::new(None));
            let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
            let bridges = LegacyBridgeSet {
                verge: Arc::new(RecordingVergeBridge {
                    mirrored_theme_color: mirrored_theme_color.clone(),
                }),
                window: Arc::new(NoopWindowBridge),
                clash: Arc::new(NoopClashBridge),
            };

            let _loaded = new_typed_config_clients(paths, bridges)
                .await
                .expect("typed clients should load and mirror persisted state");

            assert_eq!(
                mirrored_theme_color
                    .lock()
                    .expect("mirror capture should not poison")
                    .as_deref(),
                Some("#123456")
            );
        });
    }

    #[test]
    fn try_new_with_args_constructs_typed_config_facade() {
        let dir = tempdir().expect("tempdir should be created");
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
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
}
