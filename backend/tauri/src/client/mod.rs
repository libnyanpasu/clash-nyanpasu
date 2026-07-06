mod application;
mod clash_config;
mod core_bridge;
mod error;
mod event_sink;
mod ports;
pub mod profiles;
pub mod rebuild;
mod session_state;

use self::{
    application::ApplicationClient, clash_config::ClashConfigClient,
    session_state::SessionStateClient,
};
use crate::{
    enhance::{
        EnhanceScriptRunner, FsProfileContentSource, RuntimeBuildInput, RuntimeBuilder,
        runtime_from_artifact,
    },
    service::profile_file::{ProfileFileService, SelfProxyPortSource},
    state::{
        mirror::{
            ClashLegacyBridge as ClashLegacyBridgeTrait,
            VergeLegacyBridge as VergeLegacyBridgeTrait,
            WindowLegacyBridge as WindowLegacyBridgeTrait,
        },
        profiles::{
            CommitReport, NewProfileRequest, ProfilesError, ReorderOp,
            ports::{ProfileFsPort, SubscriptionFetcher},
        },
    },
    utils::path::PathResolver,
};
use anyhow::Context as _;
use camino::Utf8PathBuf;
use nyanpasu_config::{
    application::{NyanpasuAppConfig, NyanpasuAppConfigPatch},
    clash::config::{ClashConfig, ClashConfigPatch},
    profile::{
        LocalBinding, ProfileDefinition, ProfileId, ProfileMetadataPatch, ProfileSource, Profiles,
        RemoteProfileOptionsPatch,
    },
    runtime::executor::ResolvedPortBindings,
    state::{PersistentState, PersistentStatePatch},
};
use std::{path::PathBuf, sync::Arc};

#[cfg(test)]
pub use core_bridge::MockRunningCoreBridge;
pub use core_bridge::{LegacyCoreBridge, RunningCoreBridge};
pub use error::{ClientError, Result};
#[cfg(test)]
pub use event_sink::NoopUiEventSink;
pub use event_sink::{TauriUiEventSink, UiEventSink};
pub use ports::SessionPortResolver;

pub struct ClientSetupArgs {
    pub paths: PathResolver,
    pub bridges: LegacyBridgeSet,
    pub ui_sink: Arc<dyn UiEventSink>,
    pub core: Arc<dyn RunningCoreBridge>,
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
    profiles: profiles::ProfilesClient,
    fs: Arc<dyn ProfileFsPort>,
    ports: Arc<SessionPortResolver>,
    profiles_dir: PathBuf,
    ui_sink: Arc<dyn UiEventSink>,
    core: Arc<dyn RunningCoreBridge>,
}

impl NyanpasuClient {
    pub fn try_new_with_args(args: ClientSetupArgs) -> anyhow::Result<Self> {
        let ClientSetupArgs {
            paths,
            bridges,
            ui_sink,
            core,
        } = args;
        let profiles_dir = paths.app_profiles_dir();
        let profiles_path = utf8_path(paths.profiles_path())?;
        let (application, session_state, clash_config, profiles, ports, fs, rebuild_rx) =
            tauri::async_runtime::block_on(async move {
                let (application, session_state, clash_config) =
                    new_typed_config_clients(paths.clone(), bridges).await?;

                // Eager session port resolution: the core is not running yet,
                // so probing strategies is race-free (design §19.2 caller duty).
                let ports = Arc::new(SessionPortResolver::default());
                let clash_snapshot = clash_config.get().await?.state;
                ports
                    .resolve(&clash_snapshot)
                    .context("failed to resolve session ports")?;

                let file_service = Arc::new(ProfileFileService::new(
                    paths,
                    ports.clone() as Arc<dyn SelfProxyPortSource>,
                ));
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                let profiles = profiles::ProfilesClient::new(
                    profiles_path,
                    file_service.clone() as Arc<dyn ProfileFsPort>,
                    file_service.clone() as Arc<dyn SubscriptionFetcher>,
                    Arc::new(rebuild::ChannelRebuildNotifier::new(tx)),
                )
                .await?;
                anyhow::Ok((
                    application,
                    session_state,
                    clash_config,
                    profiles,
                    ports,
                    file_service as Arc<dyn ProfileFsPort>,
                    rx,
                ))
            })?;
        let client = Self::with_parts(
            application,
            session_state,
            clash_config,
            profiles,
            fs,
            ports,
            profiles_dir,
            ui_sink,
            core,
        );
        {
            let listener = client.clone();
            rebuild::spawn_listener_with(rebuild_rx, move || {
                let client = listener.clone();
                async move {
                    client
                        .rebuild_running_config()
                        .await
                        .map_err(anyhow::Error::from)
                }
            });
        }
        {
            let bridge = client.clone();
            rebuild::install_regen_bridge(move || {
                let client = bridge.clone();
                async move {
                    client
                        .regenerate_runtime_for_legacy()
                        .await
                        .map_err(anyhow::Error::from)
                }
            });
        }
        Ok(client)
    }

    fn with_parts(
        application: ApplicationClient,
        session_state: SessionStateClient,
        clash_config: ClashConfigClient,
        profiles: profiles::ProfilesClient,
        fs: Arc<dyn ProfileFsPort>,
        ports: Arc<SessionPortResolver>,
        profiles_dir: PathBuf,
        ui_sink: Arc<dyn UiEventSink>,
        core: Arc<dyn RunningCoreBridge>,
    ) -> Self {
        Self {
            inner: Arc::new(NyanpasuClientInner {
                application,
                session_state,
                clash_config,
                profiles,
                fs,
                ports,
                profiles_dir,
                ui_sink,
                core,
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

    // ---- profiles domain (PR-3 T07) ----

    pub async fn get_profiles(&self) -> Result<Arc<Profiles>> {
        Ok(self.inner.profiles.get().await?)
    }

    async fn after_commit(&self, report: &CommitReport) -> Result<()> {
        if report.affects_current {
            self.rebuild_running_config().await?;
        }
        Ok(())
    }

    pub async fn add_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> Result<ProfileId> {
        let report = self.inner.profiles.add(request, initial_file).await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| ClientError::Custom("add committed without a created uid".into()))?;
        self.after_commit(&report).await?;
        Ok(created)
    }

    pub async fn delete_profile(&self, uid: ProfileId) -> Result<()> {
        let report = self.inner.profiles.delete(uid).await?;
        self.after_commit(&report).await
    }

    pub async fn reorder_profile(&self, active: ProfileId, over: ProfileId) -> Result<()> {
        let report = self
            .inner
            .profiles
            .reorder(ReorderOp::Move { active, over })
            .await?;
        self.after_commit(&report).await
    }

    pub async fn reorder_profiles_by_list(&self, list: Vec<ProfileId>) -> Result<()> {
        let report = self.inner.profiles.reorder(ReorderOp::ByList(list)).await?;
        self.after_commit(&report).await
    }

    pub async fn refresh_profile(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> Result<()> {
        let report = self.inner.profiles.refresh(uid, patch).await?;
        self.after_commit(&report).await
    }

    pub async fn patch_profile_metadata(
        &self,
        uid: ProfileId,
        patch: ProfileMetadataPatch,
    ) -> Result<()> {
        let report = self.inner.profiles.patch_metadata(uid, patch).await?;
        self.after_commit(&report).await
    }

    pub async fn patch_remote_profile_options(
        &self,
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
    ) -> Result<()> {
        let report = self.inner.profiles.patch_remote_options(uid, patch).await?;
        self.after_commit(&report).await
    }

    pub async fn replace_profile_definition(
        &self,
        uid: ProfileId,
        definition: ProfileDefinition,
    ) -> Result<()> {
        let report = self
            .inner
            .profiles
            .replace_definition(uid, definition)
            .await?;
        self.after_commit(&report).await
    }

    pub async fn activate_profile(&self, uid: Option<ProfileId>) -> Result<()> {
        let report = self.inner.profiles.set_current(uid).await?;
        self.after_commit(&report).await
    }

    pub async fn set_global_transforms(&self, ids: Vec<ProfileId>) -> Result<()> {
        let report = self.inner.profiles.set_global_transforms(ids).await?;
        self.after_commit(&report).await
    }

    pub async fn get_profile_materialized_path(&self, uid: ProfileId) -> Result<PathBuf> {
        let snapshot = self.inner.profiles.get().await?;
        let item = snapshot
            .items
            .get(&uid)
            .ok_or(ProfilesError::ProfileNotFound(uid))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        Ok(self
            .inner
            .profiles_dir
            .join(source.materialized().file.as_path()))
    }

    pub async fn read_profile_file(&self, uid: ProfileId) -> Result<String> {
        let snapshot = self.inner.profiles.get().await?;
        let item = snapshot
            .items
            .get(&uid)
            .ok_or_else(|| ProfilesError::ProfileNotFound(uid.clone()))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        let raw = self
            .inner
            .fs
            .read(&source.materialized().file)
            .map_err(ClientError::Anyhow)?;
        match &item.definition {
            ProfileDefinition::Config { .. } => {
                crate::service::profile_file::normalize_yaml_document(&raw)
                    .map_err(ClientError::Anyhow)
            }
            ProfileDefinition::Transform { .. } => Ok(raw),
        }
    }

    pub async fn save_profile_file(&self, uid: ProfileId, data: String) -> Result<()> {
        let snapshot = self.inner.profiles.get().await?;
        let item = snapshot
            .items
            .get(&uid)
            .ok_or_else(|| ProfilesError::ProfileNotFound(uid.clone()))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        match source {
            ProfileSource::Local {
                binding:
                    LocalBinding::Managed {
                        materialized: materialized_file,
                    },
            } => {
                self.inner
                    .fs
                    .write_atomic(&materialized_file.file, &data)
                    .map_err(ClientError::Anyhow)?;
                Ok(())
            }
            ProfileSource::Remote { .. } => Err(ProfilesError::FileNotWritable {
                reason: "remote profiles are updater-owned".into(),
            }
            .into()),
            ProfileSource::Local {
                binding: LocalBinding::External { .. },
            } => Err(ProfilesError::FileNotWritable {
                reason: "external profiles are edited at their source".into(),
            }
            .into()),
        }
    }

    pub fn session_ports(&self) -> Option<ResolvedPortBindings> {
        self.inner.ports.cached_ports()
    }

    pub async fn rebuild_running_config(&self) -> Result<()> {
        self.regenerate_runtime().await?;
        self.inner
            .core
            .apply_config()
            .await
            .map_err(ClientError::Anyhow)?;
        self.inner.ui_sink.refresh_clash();
        // 用户决策 2026-07-06:所有 rebuild 统一触发(选项默认 false 门控)。
        self.inner.core.on_profile_change().await;
        Ok(())
    }

    pub(crate) async fn regenerate_runtime(&self) -> Result<()> {
        let profiles = self.inner.profiles.get().await?;
        let clash = self.get_clash_config().await?;
        let app = self.get_app_config().await?;
        self.regenerate_runtime_with(profiles, clash, app).await
    }

    /// Legacy-draft snapshot -> typed build inputs for the regeneration bridge.
    // FIXME(actor-migration): legacy-draft-aware input assembly for BC callers.
    // Legacy Config::generate() read Config::{verge,clash}().latest() — including
    // uncommitted drafts. Legacy side-effect writers (feat::patch_clash /
    // patch_verge tun+service paths, CoreManager::change_core) draft first and
    // only reseed typed actors after the mutation commits, so regenerating from
    // typed snapshots would run one step behind (stale ports/secret/core).
    // Convert legacy latest() via the reseed converters instead — without
    // mutating the typed actors, so a later discard() stays a discard.
    // New code must use rebuild_running_config()/regenerate_runtime().
    // Remove when: PR-4/5/6 migrate the legacy writers onto typed clients.
    fn legacy_regen_inputs() -> Result<(NyanpasuAppConfig, ClashConfig)> {
        let legacy_verge = crate::config::Config::verge().latest().clone();
        let legacy_clash = crate::config::Config::clash().latest().0.clone();
        let (app, _session, clash) =
            crate::bridge::typed_config_from_legacy_parts(&legacy_verge, &legacy_clash)
                .map_err(ClientError::Anyhow)?;
        Ok((app, clash))
    }

    /// Regeneration entry for legacy bridge callers (`CoreManager::update_config`,
    /// `feat::patch_clash`/`patch_verge` side-effect paths, `change_core`).
    /// Profiles still come from the typed actor: their legacy writers are
    /// rewritten against the facade in T08.
    pub(crate) async fn regenerate_runtime_for_legacy(&self) -> Result<()> {
        let (app, clash) = Self::legacy_regen_inputs()?;
        let profiles = self.inner.profiles.get().await?;
        self.regenerate_runtime_with(profiles, clash, app).await
    }

    async fn regenerate_runtime_with(
        &self,
        profiles: Arc<Profiles>,
        clash: ClashConfig,
        app: NyanpasuAppConfig,
    ) -> Result<()> {
        let resolved_ports = self
            .inner
            .ports
            .resolve(&clash)
            .map_err(ClientError::Anyhow)?;
        let profiles_dir = self.inner.profiles_dir.clone();
        let core = app.core;
        let builtin_enabled = app.enable_builtin_enhanced;
        let runtime =
            tokio::task::spawn_blocking(move || -> anyhow::Result<crate::config::IRuntime> {
                let content = FsProfileContentSource::new(profiles_dir);
                let scripts = EnhanceScriptRunner::new()?;
                let input = RuntimeBuildInput {
                    profiles: profiles.clone(),
                    clash,
                    app,
                    resolved_ports,
                };
                let artifact = RuntimeBuilder::build(&input, &content, &scripts)?;
                runtime_from_artifact(&artifact, &profiles, core, builtin_enabled)
            })
            .await
            .map_err(|error| ClientError::Custom(format!("runtime build task failed: {error}")))?
            .map_err(ClientError::Anyhow)?;
        // TODO(actor-migration): temporary bridge to Config::runtime() draft (B8).
        // Reason: runtime derivation cleanup is PR-4.
        // Remove when: PR-4 lands RuntimeArtifact in SimpleStateManager.
        *crate::config::Config::runtime().draft() = runtime;
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
    use crate::{
        client::core_bridge::MockRunningCoreBridge,
        state::{
            mirror::{ClashLegacyBridge, VergeLegacyBridge, WindowLegacyBridge},
            profiles::ports::{MockProfileFsPort, MockRebuildNotifier, MockSubscriptionFetcher},
        },
    };
    use camino::Utf8PathBuf;
    use nyanpasu_config::{
        profile::{
            ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
            ProfileDefinition, ProfileMetadata, ProfileSource,
        },
        state::window::{WindowLabel, WindowState},
    };
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
        let profiles = profiles::ProfilesClient::new(
            temp_config_path(dir, "profiles.yaml"),
            Arc::new(MockProfileFsPort::new()),
            Arc::new(MockSubscriptionFetcher::new()),
            Arc::new(MockRebuildNotifier::new()),
        )
        .await
        .expect("profiles client should be created");
        let ports = Arc::new(SessionPortResolver::default());
        ports
            .resolve(&ClashConfig::default())
            .expect("default ports should resolve");
        NyanpasuClient::with_parts(
            application,
            session_state,
            clash_config,
            profiles,
            Arc::new(MockProfileFsPort::new()),
            ports,
            dir.path().join("profiles"),
            Arc::new(crate::client::event_sink::NoopUiEventSink),
            Arc::new(MockRunningCoreBridge::new()),
        )
    }

    fn test_profiles_client_args(
        dir: &TempDir,
        core: Arc<dyn RunningCoreBridge>,
    ) -> ClientSetupArgs {
        ClientSetupArgs {
            paths: PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data")),
            bridges: LegacyBridgeSet {
                verge: Arc::new(NoopVergeBridge),
                window: Arc::new(NoopWindowBridge),
                clash: Arc::new(NoopClashBridge),
            },
            ui_sink: Arc::new(crate::client::event_sink::NoopUiEventSink),
            core,
        }
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
            ui_sink: Arc::new(crate::client::event_sink::NoopUiEventSink),
            core: Arc::new(MockRunningCoreBridge::new()),
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

    /// T07 review fix regression pin: the regeneration bridge must see legacy
    /// DRAFT state (feat::patch_clash / change_core draft first, reseed typed
    /// actors only after commit). Locks legacy_regen_inputs on latest(), not
    /// on typed snapshots or committed data().
    #[test]
    fn legacy_regen_inputs_see_uncommitted_legacy_drafts() {
        use crate::config::Config;
        {
            let mut mapping = serde_yaml::Mapping::new();
            mapping.insert("mixed-port".into(), 49301.into());
            Config::clash().draft().patch_config(mapping);
        }
        Config::verge().draft().clash_core = Some(crate::config::nyanpasu::ClashCore::ClashRs);

        let result = NyanpasuClient::legacy_regen_inputs();

        Config::clash().discard();
        Config::verge().discard();

        let (app, clash) = result.expect("legacy regen inputs should assemble");
        assert_eq!(
            app.core,
            nyanpasu_config::application::ClashCore::ClashRs,
            "drafted clash_core must reach the app input before commit"
        );
        assert_eq!(
            clash.mixed_port.start_port, 49301,
            "drafted mixed-port must reach the clash input before commit"
        );
    }

    #[test]
    fn facade_add_activate_rebuilds_via_core_bridge() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_apply_config().times(1).returning(|| Ok(()));
        core.expect_on_profile_change().times(1).returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();

        tauri::async_runtime::block_on(async {
            let uid = client
                .add_profile(
                    NewProfileRequest {
                        metadata: ProfileMetadata {
                            name: "t".into(),
                            desc: None,
                        },
                        definition: ProfileDefinition::Config {
                            config: ConfigDefinition::File(FileConfig {
                                source: ProfileSource::Local {
                                    binding: LocalBinding::Managed {
                                        materialized: MaterializedFile {
                                            file: ManagedProfilePath::new("t.yaml").unwrap(),
                                            updated_at: None,
                                        },
                                    },
                                },
                                transforms: vec![],
                            }),
                        },
                    },
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add");
            client
                .activate_profile(Some(uid.clone()))
                .await
                .expect("activate");
            let runtime = crate::config::Config::runtime();
            let runtime = runtime.latest();
            let config = runtime.config.as_ref().expect("runtime draft written");
            assert!(config.get("mixed-port").is_some());
            let path = client
                .get_profile_materialized_path(uid.clone())
                .await
                .unwrap();
            let expected_file = format!("{}.yaml", uid.0);
            assert_eq!(
                path.file_name().and_then(|name| name.to_str()),
                Some(expected_file.as_str())
            );
            let content = client.read_profile_file(uid.clone()).await.unwrap();
            assert!(content.contains("proxies"));
            client
                .save_profile_file(uid.clone(), "proxies: []\nmode: direct\n".into())
                .await
                .unwrap();
        });
    }
}
