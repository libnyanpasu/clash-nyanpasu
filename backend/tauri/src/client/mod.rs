mod application;
mod clash_config;
mod core_bridge;
mod error;
mod event_sink;
mod ports;
pub mod profiles;
pub mod rebuild;
pub mod runtime;
mod session_state;
mod system_dns;

use self::{
    application::ApplicationClient, clash_config::ClashConfigClient,
    session_state::SessionStateClient,
};
use crate::{
    enhance::{
        EnhanceScriptRunner, FsProfileContentSource, RuntimeBuildInput, RuntimeBuilder,
        runtime_state_from_artifact,
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
        ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
        ProfileDefinition, ProfileId, ProfileMetadata, ProfileMetadataPatch, ProfileSource,
        Profiles, RemoteProfileOptions, RemoteProfileOptionsPatch, SubscriptionInfo,
    },
    runtime::executor::ResolvedPortBindings,
    state::{PersistentState, PersistentStatePatch},
};
use std::{path::PathBuf, sync::Arc};
use struct_patch::Patch as _;

#[cfg(test)]
pub use core_bridge::MockRunningCoreBridge;
pub use core_bridge::{LegacyCoreBridge, RunningCoreBridge};
pub use error::{ClientError, Result};
#[cfg(test)]
pub use event_sink::NoopUiEventSink;
pub use event_sink::{TauriUiEventSink, UiEventSink};
pub use ports::SessionPortResolver;
#[cfg(test)]
pub use system_dns::{MockSystemDnsCache, NoopSystemDnsCache};
pub use system_dns::{OsSystemDnsCache, SystemDnsCache};

pub struct ClientSetupArgs {
    pub paths: PathResolver,
    pub bridges: LegacyBridgeSet,
    pub ui_sink: Arc<dyn UiEventSink>,
    pub core: Arc<dyn RunningCoreBridge>,
    pub system_dns: Arc<dyn SystemDnsCache>,
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

/// Fallback name for an imported subscription with no caller-provided name:
/// the url's last non-empty path segment (sans `.yaml`/`.yml`), else the host,
/// else a constant. Kept separate so `import_profile` reads as orchestration.
fn url_derived_name(url: &url::Url) -> String {
    url.path_segments()
        .and_then(|segments| segments.filter(|segment| !segment.is_empty()).next_back())
        .map(|segment| {
            segment
                .trim_end_matches(".yaml")
                .trim_end_matches(".yml")
                .to_string()
        })
        .filter(|name| !name.is_empty())
        .or_else(|| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "Remote Profile".into())
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
    system_dns: Arc<dyn SystemDnsCache>,
    /// Serializes runtime regeneration (snapshot -> build -> runtime draft ->
    /// core apply). The profiles actor only orders commits; without this gate
    /// a slow rebuild started for an older commit can finish after a newer
    /// one and overwrite the runtime with a stale snapshot.
    rebuild_gate: tokio::sync::Mutex<()>,
    /// PR-4: derived runtime read model (see client/runtime.rs docs).
    runtime: runtime::RuntimeStateStore,
}

impl NyanpasuClient {
    pub fn try_new_with_args(args: ClientSetupArgs) -> anyhow::Result<Self> {
        let ClientSetupArgs {
            paths,
            bridges,
            ui_sink,
            core,
            system_dns,
        } = args;
        let profiles_dir = paths.app_profiles_dir();
        let profiles_path = utf8_path(paths.profiles_path())?;
        let (
            application,
            session_state,
            clash_config,
            profiles,
            runtime_store,
            ports,
            fs,
            rebuild_rx,
        ) = tauri::async_runtime::block_on(async move {
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
            let runtime_store = runtime::new_runtime_state_store().await?;
            anyhow::Ok((
                application,
                session_state,
                clash_config,
                profiles,
                runtime_store,
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
            system_dns,
            runtime_store,
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
            rebuild::install_regen_bridge(move |kind| {
                let client = bridge.clone();
                async move {
                    let result = match kind {
                        rebuild::RegenKind::Regenerate => {
                            client.regenerate_runtime_for_legacy().await
                        }
                        rebuild::RegenKind::RegenerateAndApply => {
                            client.regenerate_and_apply_for_legacy().await
                        }
                        rebuild::RegenKind::RegenerateAndRestart => {
                            client.regenerate_and_restart_for_legacy().await
                        }
                    };
                    result.map_err(anyhow::Error::from)
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
        system_dns: Arc<dyn SystemDnsCache>,
        runtime: runtime::RuntimeStateStore,
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
                system_dns,
                rebuild_gate: tokio::sync::Mutex::new(()),
                runtime,
            }),
        }
    }

    pub async fn get_app_config(&self) -> Result<NyanpasuAppConfig> {
        let client = self.inner.application.clone();
        Ok(client.get().await?.state)
    }

    pub async fn flush_system_dns_cache(&self) -> Result<()> {
        let system_dns = self.inner.system_dns.clone();
        tokio::task::spawn_blocking(move || system_dns.flush())
            .await
            .context("system DNS cache flush task failed")??;
        Ok(())
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

    async fn after_commit(&self, report: &CommitReport) -> runtime::RebuildOutcome {
        // Post-commit side-effect failures are degraded results, not
        // transaction failures (T04 contract): the state is already
        // persisted, so surface them instead of dropping them.
        for warning in &report.warnings {
            tracing::warn!(
                warning = %warning,
                "profile commit completed with a degraded side effect",
            );
        }
        if report.affects_current {
            if let Err(error) = self.rebuild_running_config().await {
                tracing::warn!(%error, "post-commit rebuild failed; state stays committed (degraded)");
                return runtime::RebuildOutcome::Degraded {
                    error: error.to_string(),
                };
            }
        }
        runtime::RebuildOutcome::Ok
    }

    pub async fn add_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> Result<(ProfileId, runtime::RebuildOutcome)> {
        let report = self.inner.profiles.add(request, initial_file).await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| ClientError::Custom("add committed without a created uid".into()))?;
        let rebuild = self.after_commit(&report).await;
        Ok((created, rebuild))
    }

    /// Create a profile from a fully-specified request and apply the design §9
    /// auto-activation rule (activate a new Config profile when nothing is
    /// current). Keeps the auto-activation policy in the facade so the command
    /// stays a thin adapter.
    pub async fn create_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> Result<(ProfileId, runtime::RebuildOutcome)> {
        // Create does not download: a remote source would be added
        // unmaterialized, and the auto-activation below would then rebuild
        // against a missing file. Remote subscriptions must use import_profile.
        if matches!(request.definition.source(), Some(source) if source.is_remote()) {
            return Err(ClientError::Custom(
                "remote profiles must be created via import_profile".into(),
            ));
        }
        let (uid, mut rebuild) = self.add_profile(request, initial_file).await?;
        // design §9: auto-activate a Config definition (File/Composition) when
        // nothing is currently selected. set_current_if_none keeps the
        // check-and-set atomic so a concurrent selection is not overwritten.
        let is_config = matches!(
            self.inner
                .profiles
                .get()
                .await?
                .items
                .get(&uid)
                .map(|item| &item.definition),
            Some(ProfileDefinition::Config { .. })
        );
        if is_config {
            if let Some(report) = self.inner.profiles.set_current_if_none(uid.clone()).await? {
                rebuild = rebuild.merge(self.after_commit(&report).await);
            }
        }
        Ok((uid, rebuild))
    }

    /// Import a remote subscription: add (placeholder name) -> first download
    /// via the refresh transaction -> auto-activate when nothing is current.
    ///
    /// Naming: a non-empty caller-provided `name` (e.g. a deep-link `name=`
    /// parameter) is user intent, so it is pinned (`custom_name = true`) and
    /// never overwritten by later name-sync. Without one, the name is derived
    /// from the url and left unpinned so the first refresh can adopt the
    /// subscription's `profile-title` / `Content-Disposition` name.
    pub async fn import_profile(
        &self,
        url: url::Url,
        name: Option<String>,
        options: Option<RemoteProfileOptionsPatch>,
    ) -> Result<(ProfileId, runtime::RebuildOutcome)> {
        let update_interval_explicit = options
            .as_ref()
            .and_then(|patch| patch.update_interval_minutes)
            .is_some();
        let (name, custom_name) = match name {
            Some(name) if !name.trim().is_empty() => (name, true),
            _ => (url_derived_name(&url), false),
        };
        let mut option = RemoteProfileOptions::default();
        if let Some(patch) = options {
            option.apply(patch);
        }
        let request = NewProfileRequest {
            metadata: ProfileMetadata {
                name,
                desc: None,
                custom_name,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Remote {
                        // Add rewrites the path to `{uid}.{ext}` and resets the
                        // subscription/materialization metadata server-side, so
                        // these placeholders are never persisted.
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new("pending.yaml")
                                .expect("static managed path is valid"),
                            updated_at: None,
                        },
                        url,
                        option,
                        subscription: SubscriptionInfo::default(),
                    },
                    transforms: vec![],
                }),
            },
        };
        let report = self.inner.profiles.add(request, None).await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| ClientError::Custom("import committed without a created uid".into()))?;
        let refreshed = self
            .inner
            .profiles
            .refresh_import(created.clone(), update_interval_explicit)
            .await;
        let mut rebuild = match refreshed {
            // Post-commit rebuild failure now degrades instead of rolling back:
            // the download committed, so the imported profile is kept (BC vs the
            // legacy all-or-nothing behavior — only a failed download deletes it).
            Ok(report) => self.after_commit(&report).await,
            Err(error) => {
                // First download failed = import failed; delete the empty shell to
                // preserve the legacy all-or-nothing observable behavior. Log if the
                // rollback itself fails so the orphaned placeholder is not silent —
                // including file-removal failures, which the actor reports as
                // warnings on an otherwise committed delete.
                match self.inner.profiles.delete(created.clone()).await {
                    Ok(report) => {
                        for warning in &report.warnings {
                            tracing::warn!(
                                uid = %created,
                                warning = %warning,
                                "placeholder cleanup after failed import left degraded state",
                            );
                        }
                    }
                    Err(cleanup_error) => {
                        tracing::warn!(
                            uid = %created,
                            error = %cleanup_error,
                            "failed to delete placeholder profile after failed import download",
                        );
                    }
                }
                return Err(error.into());
            }
        };
        // Atomically activate only when nothing was selected during the download
        // window. The actor decides inside a single serialized message, so a
        // concurrent SetCurrent can never be overwritten by import.
        if let Some(report) = self
            .inner
            .profiles
            .set_current_if_none(created.clone())
            .await?
        {
            rebuild = rebuild.merge(self.after_commit(&report).await);
        }
        Ok((created, rebuild))
    }

    pub async fn delete_profile(&self, uid: ProfileId) -> Result<runtime::RebuildOutcome> {
        let report = self.inner.profiles.delete(uid).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn reorder_profile(
        &self,
        active: ProfileId,
        over: ProfileId,
    ) -> Result<runtime::RebuildOutcome> {
        let report = self
            .inner
            .profiles
            .reorder(ReorderOp::Move { active, over })
            .await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn reorder_profiles_by_list(
        &self,
        list: Vec<ProfileId>,
    ) -> Result<runtime::RebuildOutcome> {
        let report = self.inner.profiles.reorder(ReorderOp::ByList(list)).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn refresh_profile(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> Result<runtime::RebuildOutcome> {
        let report = self.inner.profiles.refresh(uid, patch).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn patch_profile_metadata(
        &self,
        uid: ProfileId,
        patch: ProfileMetadataPatch,
    ) -> Result<runtime::RebuildOutcome> {
        let report = self.inner.profiles.patch_metadata(uid, patch).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn patch_remote_profile_options(
        &self,
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
    ) -> Result<runtime::RebuildOutcome> {
        let report = self.inner.profiles.patch_remote_options(uid, patch).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn replace_profile_definition(
        &self,
        uid: ProfileId,
        definition: ProfileDefinition,
    ) -> Result<runtime::RebuildOutcome> {
        let report = self
            .inner
            .profiles
            .replace_definition(uid, definition)
            .await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn activate_profile(
        &self,
        uid: Option<ProfileId>,
    ) -> Result<runtime::RebuildOutcome> {
        let report = self.inner.profiles.set_current(uid).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn set_global_transforms(
        &self,
        ids: Vec<ProfileId>,
    ) -> Result<runtime::RebuildOutcome> {
        let report = self.inner.profiles.set_global_transforms(ids).await?;
        Ok(self.after_commit(&report).await)
    }

    pub async fn set_profile_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> Result<runtime::RebuildOutcome> {
        let report = self.inner.profiles.set_valid_fields(fields).await?;
        Ok(self.after_commit(&report).await)
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

    pub async fn runtime_state(&self) -> std::sync::Arc<Option<runtime::RuntimeState>> {
        self.inner.runtime.read().await.snapshot()
    }

    pub async fn rebuild_running_config(&self) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        self.regenerate_runtime_inner().await?;
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
        let _rebuild = self.inner.rebuild_gate.lock().await;
        self.regenerate_runtime_inner().await
    }

    /// Must only run while holding `rebuild_gate`: snapshots are read here, so
    /// the gate guarantees the last write always reflects the newest state.
    async fn regenerate_runtime_inner(&self) -> Result<()> {
        let profiles = self.inner.profiles.get().await?;
        let clash = self.get_clash_config().await?;
        let app = self.get_app_config().await?;
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
        let (state, yaml) = tokio::task::spawn_blocking(
            move || -> anyhow::Result<(crate::client::runtime::RuntimeState, String)> {
                let content = FsProfileContentSource::new(profiles_dir);
                let scripts = EnhanceScriptRunner::new()?;
                let input = RuntimeBuildInput {
                    profiles: profiles.clone(),
                    clash,
                    app,
                    resolved_ports,
                };
                let artifact = RuntimeBuilder::build(&input, &content, &scripts)?;
                let state =
                    runtime_state_from_artifact(&artifact, &profiles, core, builtin_enabled)?;
                let yaml = format!(
                    "# Generated by Clash Nyanpasu\n\n{}",
                    serde_yaml::to_string(&state.config)?
                );
                Ok((state, yaml))
            },
        )
        .await
        .map_err(|error| ClientError::Custom(format!("runtime build task failed: {error}")))?
        .map_err(ClientError::Anyhow)?;
        // Candidate -> check -> promote -> PUBLISH (spec §5.2, P0-1): readers
        // only ever see checked-and-promoted configs; a rejected candidate
        // leaves both the product and the manager untouched. target core =
        // the same input snapshot the builder used (P0-3).
        let candidate = crate::client::runtime::candidate_config_path();
        {
            // Exclusive create (create_new): the unique candidate path must not
            // already exist. A pre-existing file/symlink now fails the pipeline
            // visibly instead of being followed (TOCTOU hardening, PR-4 re-review).
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
                .await
                .map_err(|error| {
                    ClientError::Custom(format!("failed to create candidate: {error}"))
                })?;
            file.write_all(yaml.as_bytes()).await.map_err(|error| {
                ClientError::Custom(format!("failed to write candidate: {error}"))
            })?;
            file.flush().await.map_err(|error| {
                ClientError::Custom(format!("failed to flush candidate: {error}"))
            })?;
        }
        let candidate = utf8_path(candidate).map_err(ClientError::Anyhow)?;
        let checked = self.inner.core.check_and_promote(&candidate, core).await;
        // best-effort candidate cleanup; runs whether the check passed or failed.
        if let Err(error) = tokio::fs::remove_file(candidate.as_std_path()).await {
            tracing::warn!(%error, ?candidate, "failed to remove candidate config");
        }
        checked.map_err(ClientError::Anyhow)?;
        {
            let mut store = self.inner.runtime.write().await;
            store.upsert(Some(state)).await.map_err(|error| {
                ClientError::Custom(format!("failed to store runtime state: {error}"))
            })?;
        }
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
        test_client_with_system_dns(dir, Arc::new(NoopSystemDnsCache)).await
    }

    async fn test_client_with_system_dns(
        dir: &TempDir,
        system_dns: Arc<dyn SystemDnsCache>,
    ) -> NyanpasuClient {
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
            system_dns,
            crate::client::runtime::new_runtime_state_store()
                .await
                .expect("runtime state store"),
        )
    }

    #[tokio::test]
    async fn flush_system_dns_cache_forwards_to_injected_adapter() {
        let dir = tempdir().expect("tempdir should be created");
        let mut system_dns = MockSystemDnsCache::new();
        system_dns.expect_flush().times(1).returning(|| Ok(()));
        let client = test_client_with_system_dns(&dir, Arc::new(system_dns)).await;

        client
            .flush_system_dns_cache()
            .await
            .expect("DNS cache flush should succeed");
    }

    #[tokio::test]
    async fn flush_system_dns_cache_propagates_adapter_failure() {
        let dir = tempdir().expect("tempdir should be created");
        let mut system_dns = MockSystemDnsCache::new();
        system_dns
            .expect_flush()
            .times(1)
            .returning(|| anyhow::bail!("dns flush exploded"));
        let client = test_client_with_system_dns(&dir, Arc::new(system_dns)).await;

        let error = client.flush_system_dns_cache().await.unwrap_err();
        assert!(error.to_string().contains("dns flush exploded"));
    }

    pub(crate) fn test_profiles_client_args(
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
            system_dns: Arc::new(NoopSystemDnsCache),
        }
    }

    fn minimal_file_profile_request() -> NewProfileRequest {
        NewProfileRequest {
            metadata: ProfileMetadata {
                name: "t".into(),
                desc: None,
                custom_name: true,
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
        }
    }

    /// Build a facade whose profiles domain uses a real [`ProfileFileService`]
    /// for the filesystem port and an injected fake fetcher. The refresh
    /// transaction must materialize `{uid}.yaml` on disk so the
    /// activate-triggered rebuild can read it back; only the network fetch is
    /// faked.
    async fn test_client_with_fetcher(
        dir: &TempDir,
        fetcher: Arc<dyn SubscriptionFetcher>,
        core: Arc<dyn RunningCoreBridge>,
    ) -> NyanpasuClient {
        let (application, session_state, clash_config) = test_typed_config_clients(dir).await;
        let paths = PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data"));
        let ports = Arc::new(SessionPortResolver::default());
        ports
            .resolve(&ClashConfig::default())
            .expect("default ports should resolve");
        let file_service = Arc::new(ProfileFileService::new(
            paths.clone(),
            ports.clone() as Arc<dyn SelfProxyPortSource>,
        ));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let profiles = profiles::ProfilesClient::new(
            temp_config_path(dir, "profiles.yaml"),
            file_service.clone() as Arc<dyn ProfileFsPort>,
            fetcher,
            Arc::new(rebuild::ChannelRebuildNotifier::new(tx)),
        )
        .await
        .expect("profiles client should be created");
        let client = NyanpasuClient::with_parts(
            application,
            session_state,
            clash_config,
            profiles,
            file_service.clone() as Arc<dyn ProfileFsPort>,
            ports,
            paths.app_profiles_dir(),
            Arc::new(crate::client::event_sink::NoopUiEventSink),
            core,
            Arc::new(NoopSystemDnsCache),
            crate::client::runtime::new_runtime_state_store()
                .await
                .expect("runtime state store"),
        );
        let listener = client.clone();
        rebuild::spawn_listener_with(rx, move || {
            let client = listener.clone();
            async move {
                client
                    .rebuild_running_config()
                    .await
                    .map_err(anyhow::Error::from)
            }
        });
        client
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
            system_dns: Arc::new(NoopSystemDnsCache),
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
    fn runtime_state_is_none_before_first_rebuild() {
        let dir = tempdir().unwrap();
        let client = tauri::async_runtime::block_on(test_client(&dir));
        let state = tauri::async_runtime::block_on(client.runtime_state());
        assert!(state.as_ref().is_none());
    }

    #[test]
    fn facade_add_activate_rebuilds_via_core_bridge() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().times(1).returning(|| Ok(()));
        core.expect_on_profile_change().times(1).returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();

        tauri::async_runtime::block_on(async {
            let (uid, _) = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add");
            client
                .activate_profile(Some(uid.clone()))
                .await
                .expect("activate");
            let state = client.runtime_state().await;
            let state = state
                .as_ref()
                .as_ref()
                .expect("runtime state stored after rebuild");
            assert!(state.config.get("mixed-port").is_some());
            assert!(
                !state.exists_keys.is_empty(),
                "guard overrides must register applied fields"
            );
            let _ = state.postprocessing_output.clone(); // postprocessing 面可达(无脚本 profile 时为 default)
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

    #[test]
    fn activate_returns_degraded_and_keeps_commit_when_rebuild_fails() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .returning(|_, _| Err(anyhow::anyhow!("check boom")));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        tauri::async_runtime::block_on(async {
            let (uid, _) = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add");
            let outcome = client
                .activate_profile(Some(uid.clone()))
                .await
                .expect("activate must commit");
            assert!(matches!(
                outcome,
                crate::client::runtime::RebuildOutcome::Degraded { .. }
            ));
            let profiles = client.get_profiles().await.unwrap();
            assert_eq!(
                profiles.current.as_ref(),
                Some(&uid),
                "state stays committed"
            );
        });
    }

    #[test]
    fn legacy_regeneration_path_still_errors_on_rebuild_failure() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .returning(|_, _| Err(anyhow::anyhow!("check boom")));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        let result = tauri::async_runtime::block_on(client.regenerate_runtime_for_legacy());
        assert!(
            result.is_err(),
            "legacy callers rely on Err to discard their drafts"
        );
    }

    #[test]
    fn rebuild_checks_and_promotes_before_core_apply() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_apply_config()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        tauri::async_runtime::block_on(async {
            let (uid, _) = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add");
            client.activate_profile(Some(uid)).await.expect("activate");
        });
    }

    /// D5+P0-1 invariant: a failed check must leave the manager unpublished
    /// (product left untouched is proven by LegacyCoreBridge ordering + the
    /// promote atomicity unit test).
    #[test]
    fn failed_check_keeps_runtime_state_unpublished() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .returning(|_, _| Err(anyhow::anyhow!("check boom")));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        tauri::async_runtime::block_on(async {
            let (uid, _) = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add");
            // T8: a failed rebuild degrades (commit stays) instead of erroring;
            // the rejected candidate must still never reach readers.
            let _ = client.activate_profile(Some(uid)).await;
            assert!(
                client.runtime_state().await.as_ref().is_none(),
                "a rejected candidate must never be published to readers"
            );
        });
    }

    #[test]
    fn facade_import_downloads_and_conditionally_activates() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(1).returning(|_, _| {
            Ok(crate::state::profiles::ports::FetchedSubscription {
                content: "proxies: []\n".into(),
                subscription: SubscriptionInfo::default(),
                // No server name: exercises the url last-segment fallback below.
                filename: None,
                suggested_update_interval_minutes: Some(360),
            })
        });
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;
            let url = url::Url::parse("https://example.com/subs/my-sub.yaml").unwrap();
            let mut patch = RemoteProfileOptions::new_empty_patch();
            patch.with_proxy = Some(false);
            let (uid, _) = client
                .import_profile(url, None, Some(patch))
                .await
                .expect("import");
            let snapshot = client.get_profiles().await.unwrap();
            assert_eq!(
                snapshot.current.as_ref(),
                Some(&uid),
                "empty current must auto-activate"
            );
            let item = &snapshot.items[&uid];
            assert_eq!(item.metadata.name, "my-sub"); // url last-segment fallback naming
            let source = item.definition.source().unwrap();
            assert!(source.is_remote());
            let ProfileSource::Remote { option, .. } = source else {
                unreachable!()
            };
            assert_eq!(option.update_interval_minutes, 360);
            assert!(!option.with_proxy);
        });
    }

    #[test]
    fn facade_import_keeps_explicit_interval_over_server_suggestion() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(1).returning(|_, _| {
            Ok(crate::state::profiles::ports::FetchedSubscription {
                content: "proxies: []\n".into(),
                subscription: SubscriptionInfo::default(),
                filename: None,
                suggested_update_interval_minutes: Some(360),
            })
        });
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;
            let mut patch = RemoteProfileOptions::new_empty_patch();
            patch.update_interval_minutes = Some(45);
            let url = url::Url::parse("https://example.com/subs/explicit.yaml").unwrap();
            let (uid, _) = client
                .import_profile(url, None, Some(patch))
                .await
                .expect("import");
            let snapshot = client.get_profiles().await.unwrap();
            let ProfileSource::Remote { option, .. } =
                snapshot.items[&uid].definition.source().unwrap()
            else {
                unreachable!()
            };
            assert_eq!(option.update_interval_minutes, 45);
        });
    }

    #[test]
    fn facade_import_rejects_explicit_zero_interval_before_fetch() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(0);
        let core = MockRunningCoreBridge::new();

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;
            let mut patch = RemoteProfileOptions::new_empty_patch();
            patch.update_interval_minutes = Some(0);
            let url = url::Url::parse("https://example.com/subs/invalid.yaml").unwrap();
            assert!(client.import_profile(url, None, Some(patch)).await.is_err());
            assert!(client.get_profiles().await.unwrap().items.is_empty());
        });
    }

    fn local_config_request(name: &str) -> NewProfileRequest {
        NewProfileRequest {
            metadata: ProfileMetadata {
                name: name.into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new("pending.yaml").unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: vec![],
                }),
            },
        }
    }

    fn remote_config_request() -> NewProfileRequest {
        NewProfileRequest {
            metadata: ProfileMetadata {
                name: "remote".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Remote {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new("pending.yaml").unwrap(),
                            updated_at: None,
                        },
                        url: url::Url::parse("https://example.com/sub").unwrap(),
                        option: RemoteProfileOptions::default(),
                        subscription: SubscriptionInfo::default(),
                    },
                    transforms: vec![],
                }),
            },
        }
    }

    #[test]
    fn facade_import_failure_deletes_placeholder() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher
            .expect_fetch()
            .returning(|_, _| anyhow::bail!("dns exploded"));
        // A failed import never reaches core apply, so the bridge expects nothing.
        let core = MockRunningCoreBridge::new();

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;
            let url = url::Url::parse("https://example.com/subs/x.yaml").unwrap();
            let result = client.import_profile(url, None, None).await;
            assert!(
                result.is_err(),
                "import must fail when the first download fails"
            );
            let snapshot = client.get_profiles().await.unwrap();
            assert!(
                snapshot.items.is_empty(),
                "the placeholder profile must be deleted after a failed import"
            );
        });
    }

    #[test]
    fn facade_create_auto_activates_config_and_rejects_remote() {
        let dir = tempdir().unwrap();
        let fetcher = MockSubscriptionFetcher::new();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;

            // A remote source cannot be created (must go through import_profile).
            let rejected = client.create_profile(remote_config_request(), None).await;
            assert!(
                matches!(rejected, Err(ClientError::Custom(_))),
                "create must reject remote sources"
            );

            // A local Config with no current selection auto-activates (design §9).
            let (uid, _) = client
                .create_profile(local_config_request("local"), Some("proxies: []\n".into()))
                .await
                .expect("create local config");
            let snapshot = client.get_profiles().await.unwrap();
            assert_eq!(
                snapshot.current.as_ref(),
                Some(&uid),
                "an empty current must auto-activate the new Config profile"
            );
        });
    }

    #[test]
    fn facade_import_does_not_steal_existing_current() {
        let dir = tempdir().unwrap();
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().times(1).returning(|_, _| {
            Ok(crate::state::profiles::ports::FetchedSubscription {
                content: "proxies: []\n".into(),
                subscription: SubscriptionInfo::default(),
                filename: None,
                suggested_update_interval_minutes: None,
            })
        });
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client = test_client_with_fetcher(&dir, Arc::new(fetcher), Arc::new(core)).await;

            // Establish a current selection via a local Config.
            let (local_uid, _) = client
                .create_profile(local_config_request("local"), Some("proxies: []\n".into()))
                .await
                .expect("create local config");
            assert_eq!(
                client.get_profiles().await.unwrap().current.as_ref(),
                Some(&local_uid)
            );

            // Import a remote subscription; current is already set, so import
            // must NOT overwrite the selection made before it.
            let url = url::Url::parse("https://example.com/subs/x.yaml").unwrap();
            let (imported, _) = client
                .import_profile(url, None, None)
                .await
                .expect("import");
            let snapshot = client.get_profiles().await.unwrap();
            assert_eq!(
                snapshot.current.as_ref(),
                Some(&local_uid),
                "import must not overwrite an existing current selection"
            );
            assert!(snapshot.items.contains_key(&imported));
            let ProfileSource::Remote { option, .. } =
                snapshot.items[&imported].definition.source().unwrap()
            else {
                unreachable!()
            };
            assert_eq!(option.update_interval_minutes, 120);
        });
    }

    fn ok_fetch_without_name() -> MockSubscriptionFetcher {
        let mut fetcher = MockSubscriptionFetcher::new();
        fetcher.expect_fetch().returning(|_, _| {
            Ok(crate::state::profiles::ports::FetchedSubscription {
                content: "proxies: []\n".into(),
                subscription: SubscriptionInfo::default(),
                filename: None,
                suggested_update_interval_minutes: None,
            })
        });
        fetcher
    }

    #[test]
    fn facade_import_without_name_derives_url_name_and_leaves_it_unpinned() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client =
                test_client_with_fetcher(&dir, Arc::new(ok_fetch_without_name()), Arc::new(core))
                    .await;
            let url = url::Url::parse("https://example.com/subs/my-sub.yaml").unwrap();
            let (uid, _) = client
                .import_profile(url, None, None)
                .await
                .expect("import");
            let item = client.get_profiles().await.unwrap().items[&uid].clone();
            assert_eq!(item.metadata.name, "my-sub");
            assert!(
                !item.metadata.custom_name,
                "no caller name -> unpinned so refresh name-sync can adopt a server name"
            );
        });
    }

    #[test]
    fn facade_import_with_name_uses_it_and_pins_custom_name() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote().returning(|_, _| Ok(()));
        core.expect_apply_config().returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());

        tauri::async_runtime::block_on(async {
            let client =
                test_client_with_fetcher(&dir, Arc::new(ok_fetch_without_name()), Arc::new(core))
                    .await;
            let url = url::Url::parse("https://example.com/subs/my-sub.yaml").unwrap();
            let (uid, _) = client
                .import_profile(url, Some("My VPN".into()), None)
                .await
                .expect("import");
            let item = client.get_profiles().await.unwrap().items[&uid].clone();
            assert_eq!(item.metadata.name, "My VPN");
            assert!(
                item.metadata.custom_name,
                "a caller-provided name is user intent and must be pinned"
            );
        });
    }
}
