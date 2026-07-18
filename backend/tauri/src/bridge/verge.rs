use std::{future::Future, sync::Arc};

use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    client::{ClientError, NyanpasuClient, PartialCommit, Result as ClientResult},
    config::{Config, Draft, IVerge, nyanpasu as legacy_app},
    state::mirror::{PreparedLegacyMirror, VergeLegacyBridge},
};
use nyanpasu_config::application::{
    NetworkStatisticWidgetConfig as AppNetworkStatisticWidgetConfig, NyanpasuAppConfig,
};
use nyanpasu_egui::widget::StatisticWidgetVariant;
use struct_patch::Patch as _;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct LegacyVergeBridge {
    managed: Option<Arc<LegacyVergeBridgeInner>>,
    legacy_store: Arc<dyn LegacyVergeStore>,
}

impl Default for LegacyVergeBridge {
    fn default() -> Self {
        Self {
            managed: None,
            legacy_store: Arc::new(ConfigLegacyVergeStore::default()),
        }
    }
}

struct LegacyVergeBridgeInner {
    client: NyanpasuClient,
    legacy_verge_path: Utf8PathBuf,
    verge_update_lock: Mutex<()>,
}

pub(crate) trait LegacyVergeStore: Send + Sync {
    fn snapshot(&self) -> anyhow::Result<IVerge>;
    fn prepare_application(
        &self,
        snap: &NyanpasuAppConfig,
    ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>>;
    fn prepare_commit(
        &self,
        path: &Utf8Path,
        state: IVerge,
    ) -> anyhow::Result<Box<dyn PreparedLegacyVergeCommit>>;
    fn prepare_restore(
        &self,
        path: &Utf8Path,
        state: IVerge,
    ) -> anyhow::Result<Box<dyn PreparedLegacyVergeCommit>>;
    fn prepare_projection(&self, state: IVerge) -> anyhow::Result<Box<dyn PreparedLegacyMirror>>;
}

pub(crate) trait PreparedLegacyVergeCommit: Send {
    fn commit(self: Box<Self>) -> anyhow::Result<()>;
}

pub(crate) struct ConfigLegacyVergeStore {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
}

impl Default for ConfigLegacyVergeStore {
    fn default() -> Self {
        Self::new(Arc::new(parking_lot::Mutex::new(())))
    }
}

impl ConfigLegacyVergeStore {
    pub(crate) fn new(legacy_lock: Arc<parking_lot::Mutex<()>>) -> Self {
        Self { legacy_lock }
    }

    fn prepare(
        &self,
        path: &Utf8Path,
        state: IVerge,
        preserve_typed_projection: bool,
    ) -> anyhow::Result<Box<dyn PreparedLegacyVergeCommit>> {
        serde_yaml::to_string(&state)?;
        Ok(Box::new(ConfigPreparedLegacyVergeCommit {
            path: path.to_owned(),
            state,
            preserve_typed_projection,
            legacy_lock: Arc::clone(&self.legacy_lock),
        }))
    }
}

// TODO(actor-migration): compatibility adapter for the legacy Config::verge() store.
// Reason: legacy side-effect writers and readers still use the process-wide Draft<IVerge>.
// Remove when: all IVerge fields and side effects are owned by injected typed services.
impl LegacyVergeStore for ConfigLegacyVergeStore {
    fn snapshot(&self) -> anyhow::Result<IVerge> {
        let _guard = self.legacy_lock.lock();
        Ok(Config::verge().data().clone())
    }

    fn prepare_application(
        &self,
        snap: &NyanpasuAppConfig,
    ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
        let store = Config::verge();
        let mut projected = {
            let _guard = self.legacy_lock.lock();
            store.data().clone()
        };
        apply_app_config_to_legacy_verge(&mut projected, snap)?;
        Ok(Box::new(PreparedVergeMirror {
            legacy_lock: Arc::clone(&self.legacy_lock),
            store,
            projected,
        }))
    }

    fn prepare_commit(
        &self,
        path: &Utf8Path,
        state: IVerge,
    ) -> anyhow::Result<Box<dyn PreparedLegacyVergeCommit>> {
        self.prepare(path, state, true)
    }

    fn prepare_restore(
        &self,
        path: &Utf8Path,
        state: IVerge,
    ) -> anyhow::Result<Box<dyn PreparedLegacyVergeCommit>> {
        self.prepare(path, state, false)
    }

    fn prepare_projection(&self, state: IVerge) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
        Ok(Box::new(PreparedFullVergeProjection {
            legacy_lock: Arc::clone(&self.legacy_lock),
            store: Config::verge(),
            state,
        }))
    }
}

struct PreparedFullVergeProjection {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
    store: Draft<IVerge>,
    state: IVerge,
}

impl PreparedLegacyMirror for PreparedFullVergeProjection {
    fn apply(self: Box<Self>) {
        let _guard = self.legacy_lock.lock();
        self.store
            .apply_update(|target| *target = self.state.clone());
    }
}

struct ConfigPreparedLegacyVergeCommit {
    path: Utf8PathBuf,
    state: IVerge,
    preserve_typed_projection: bool,
    legacy_lock: Arc<parking_lot::Mutex<()>>,
}

impl PreparedLegacyVergeCommit for ConfigPreparedLegacyVergeCommit {
    fn commit(self: Box<Self>) -> anyhow::Result<()> {
        let _guard = self.legacy_lock.lock();
        let mut state = self.state;
        if self.preserve_typed_projection {
            let current = Config::verge().data().clone();
            apply_prepared_app_projection(&mut state, &current);
            state.window_size_state = current.window_size_state.clone();
            state.window_size_position = current.window_size_position.clone();
            super::clash::apply_prepared_clash_verge_projection(&mut state, &current);
        }
        let yaml = serde_yaml::to_string(&state)?;
        let bytes = format!("# Clash Nyanpasu Config\n\n{yaml}").into_bytes();
        crate::core::migration::fs::atomic_write(self.path.as_std_path(), &bytes)?;
        *Config::verge().draft() = state;
        Config::verge().apply();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyVergePatchRoute {
    PureConfig,
    LegacySideEffects,
}

impl LegacyVergeBridge {
    pub(crate) fn with_store(legacy_store: Arc<dyn LegacyVergeStore>) -> Self {
        Self {
            managed: None,
            legacy_store,
        }
    }

    pub(crate) fn new(
        client: NyanpasuClient,
        legacy_verge_path: Utf8PathBuf,
        legacy_store: Arc<dyn LegacyVergeStore>,
    ) -> Self {
        Self {
            managed: Some(Arc::new(LegacyVergeBridgeInner {
                client,
                legacy_verge_path,
                verge_update_lock: Mutex::new(()),
            })),
            legacy_store,
        }
    }

    pub async fn get_verge_config(&self) -> ClientResult<IVerge> {
        let managed = self.managed()?;
        let _guard = managed.verge_update_lock.lock().await;
        self.get_verge_config_unlocked().await
    }

    pub async fn patch_verge_config(&self, payload: IVerge) -> ClientResult<()> {
        match Self::route_patch(&payload) {
            LegacyVergePatchRoute::PureConfig => {
                let managed = self.managed()?;
                let _guard = managed.verge_update_lock.lock().await;
                Self::validate_patch(&payload)?;
                let base = self.get_verge_config_unlocked().await?;
                let clash = managed.client.get_clash_config().await?;
                let legacy_clash = super::yaml_convert(&clash.overrides)?;
                let plan = Self::typed_patch_plan(base.clone(), &payload, &legacy_clash)?;
                let mut desired = base;
                desired.patch_config(payload.clone());
                let prepared = self
                    .legacy_store
                    .prepare_commit(&managed.legacy_verge_path, desired)?;
                self.apply_typed_config_patch_plan(plan, move || prepared.commit())
                    .await?;
            }
            LegacyVergePatchRoute::LegacySideEffects => {
                let client = self.managed()?.client.clone();
                self.run_legacy_verge_mutation(move || crate::feat::patch_verge(client, payload))
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn replace_verge_config(&self, state: IVerge) -> ClientResult<()> {
        let managed = self.managed()?;
        let _guard = managed.verge_update_lock.lock().await;
        let prepared = self
            .legacy_store
            .prepare_commit(&managed.legacy_verge_path, state.clone())?;
        self.replace_typed_config_from_legacy(state, move || prepared.commit())
            .await?;
        Ok(())
    }

    pub async fn run_legacy_verge_mutation<F, Fut>(&self, mutate: F) -> ClientResult<()>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<()>>,
    {
        let managed = self.managed()?;
        let _guard = managed.verge_update_lock.lock().await;
        let previous = self.legacy_store.snapshot()?;
        if let Err(error) = mutate().await {
            return Err(Self::legacy_mutation_partial(
                anyhow::anyhow!("legacy mutation failed: {error:#}"),
                None,
            ));
        }
        // TODO(actor-migration): compatibility bridge for legacy side-effect writers.
        // Reason: feat::patch_verge still executes OS effects while producing legacy state.
        // Remove when: side effects are prepared and committed by typed domain services.
        let desired = self.legacy_store.snapshot()?;
        let patch = legacy_patch_between(&previous, &desired)?;
        let restore = self
            .legacy_store
            .prepare_restore(&managed.legacy_verge_path, previous)
            .map_err(|error| Self::legacy_mutation_partial(error, None))?;
        if let Err(error) = restore.commit() {
            return Err(Self::legacy_mutation_partial(error, None));
        }

        let base = self.refresh_legacy_projection().await.map_err(|error| {
            Self::legacy_mutation_partial(anyhow::anyhow!(format!("{error:#}")), Some(error))
        })?;
        let clash = managed.client.get_clash_config().await.map_err(|error| {
            Self::legacy_mutation_partial(anyhow::anyhow!(format!("{error:#}")), Some(error))
        })?;
        let legacy_clash = super::yaml_convert(&clash.overrides)
            .map_err(|error| Self::legacy_mutation_partial(error, None))?;
        let plan = Self::typed_patch_plan(base.clone(), &patch, &legacy_clash)
            .map_err(|error| Self::legacy_mutation_partial(error, None))?;
        let mut desired = base;
        desired.patch_config(patch);
        let finalize = self
            .legacy_store
            .prepare_commit(&managed.legacy_verge_path, desired)
            .map_err(|error| Self::legacy_mutation_partial(error, None))?;

        match self
            .apply_typed_config_patch_plan(plan, move || finalize.commit())
            .await
        {
            Ok(()) => Ok(()),
            Err(error) => Err(Self::legacy_mutation_partial(
                anyhow::anyhow!(format!("{error:#}")),
                Some(error),
            )),
        }
    }

    fn legacy_mutation_partial(error: anyhow::Error, source: Option<ClientError>) -> ClientError {
        let message = format!(
            "legacy mutation may have non-reversible side effects and requires reconciliation: {error:#}"
        );
        if let Some(ClientError::PartialCommit(partial)) = source {
            return partial.with_legacy_state_uncertain(message).into();
        }

        let primary = ClientError::Anyhow(error);
        PartialCommit::new(&primary, Vec::new(), Vec::new(), Vec::new())
            .with_legacy_state_uncertain(message)
            .into()
    }

    fn managed(&self) -> ClientResult<&LegacyVergeBridgeInner> {
        self.managed
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("legacy verge bridge is not managed").into())
    }

    async fn get_verge_config_unlocked(&self) -> ClientResult<IVerge> {
        let managed = self.managed()?;
        let app = managed.client.get_app_config().await?;
        let session = managed.client.get_session_state().await?;
        let clash = managed.client.get_clash_config().await?;

        Ok(super::legacy_iverge_from_typed(
            self.legacy_store.snapshot()?,
            &app,
            &session,
            &clash,
        )?)
    }

    async fn refresh_legacy_projection(&self) -> ClientResult<IVerge> {
        let managed = self.managed()?;
        loop {
            let before = managed.client.typed_config_snapshots().await?;
            let projected = super::legacy_iverge_from_typed(
                self.legacy_store.snapshot()?,
                &before.application.state,
                &before.session.state,
                &before.clash.state,
            )?;
            self.legacy_store
                .prepare_projection(projected.clone())?
                .apply();
            let after = managed.client.typed_config_snapshots().await?;
            if before.application.version == after.application.version
                && before.session.version == after.session.version
                && before.clash.version == after.clash.version
            {
                return Ok(projected);
            }
        }
    }

    async fn apply_typed_config_patch_plan<F>(
        &self,
        plan: crate::state::TypedConfigPatchPlan,
        finalize: F,
    ) -> ClientResult<()>
    where
        F: FnOnce() -> anyhow::Result<()>,
    {
        self.managed()?
            .client
            .apply_legacy_verge_patch_saga(plan, finalize)
            .await
    }

    async fn replace_typed_config_from_legacy<F>(
        &self,
        legacy: IVerge,
        finalize: F,
    ) -> ClientResult<()>
    where
        F: FnOnce() -> anyhow::Result<()>,
    {
        let managed = self.managed()?;
        let current_clash = managed.client.get_clash_config().await?;
        let legacy_clash = super::yaml_convert(&current_clash.overrides)?;
        let (app, session, clash) = Self::typed_replacement(&legacy, &legacy_clash)?;
        managed
            .client
            .apply_legacy_verge_replacement_saga(app, session, clash, finalize)
            .await
    }

    pub(crate) fn typed_replacement(
        legacy: &IVerge,
        legacy_clash: &serde_yaml::Mapping,
    ) -> anyhow::Result<(
        NyanpasuAppConfig,
        nyanpasu_config::state::PersistentState,
        nyanpasu_config::clash::config::ClashConfig,
    )> {
        super::typed_config_from_legacy_parts(legacy, legacy_clash)
    }

    pub(crate) fn typed_patch_plan(
        base: IVerge,
        patch: &IVerge,
        legacy_clash: &serde_yaml::Mapping,
    ) -> anyhow::Result<crate::state::TypedConfigPatchPlan> {
        super::typed_patches_from_legacy_patch(base, patch, legacy_clash)
    }

    pub(crate) fn route_patch(patch: &IVerge) -> LegacyVergePatchRoute {
        route_verge_patch(patch)
    }

    pub(crate) fn validate_patch(patch: &IVerge) -> anyhow::Result<()> {
        validate_verge_patch(patch)
    }
}

fn legacy_patch_between(previous: &IVerge, desired: &IVerge) -> anyhow::Result<IVerge> {
    let previous = serde_yaml::to_value(previous)?;
    let desired = serde_yaml::to_value(desired)?;
    let previous = previous
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("legacy verge snapshot must serialize as a mapping"))?;
    let mut patch = desired
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("legacy verge snapshot must serialize as a mapping"))?
        .clone();
    patch.retain(|key, value| previous.get(key) != Some(value));
    Ok(serde_yaml::from_value(serde_yaml::Value::Mapping(patch))?)
}

/// Pure classifier (infallible). Validation is delegated to `validate_verge_patch`
/// or to `feat::patch_verge`. The side-effect field set mirrors `feat::patch_verge`.
#[allow(deprecated)]
fn route_verge_patch(patch: &IVerge) -> LegacyVergePatchRoute {
    let legacy = patch.enable_service_mode.is_some()
        || patch.enable_tun_mode.is_some()
        || patch.enable_auto_launch.is_some()
        || patch.enable_system_proxy.is_some()
        || patch.system_proxy_bypass.is_some()
        || patch.enable_proxy_guard.is_some()
        || patch.hotkeys.is_some()
        || patch.language.is_some()
        || patch.app_log_level.is_some()
        || patch.max_log_files.is_some()
        || patch.auto_log_clean.is_some()
        || patch.clash_tray_selector.is_some()
        || patch.enable_tray_text.is_some()
        || patch.tray_menu_mode.is_some()
        || patch.network_statistic_widget.is_some();

    if legacy {
        LegacyVergePatchRoute::LegacySideEffects
    } else {
        LegacyVergePatchRoute::PureConfig
    }
}

fn validate_verge_patch(verge: &IVerge) -> anyhow::Result<()> {
    if let Some(theme_color) = &verge.theme_color
        && !theme_color.is_empty()
        && !legacy_app::is_hex_color(theme_color)
    {
        anyhow::bail!("Invalid theme color: {}", theme_color);
    }
    Ok(())
}

struct PreparedVergeMirror {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
    store: Draft<IVerge>,
    projected: IVerge,
}

impl PreparedLegacyMirror for PreparedVergeMirror {
    fn apply(self: Box<Self>) {
        let Self {
            legacy_lock,
            store,
            projected,
        } = *self;
        let _guard = legacy_lock.lock();
        store.apply_update(|target| apply_prepared_app_projection(target, &projected));
    }
}

impl VergeLegacyBridge for LegacyVergeBridge {
    fn prepare(&self, snap: &NyanpasuAppConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
        self.legacy_store.prepare_application(snap)
    }

    fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
        application_from_legacy(&self.legacy_store.snapshot()?)
    }
}

pub(crate) fn application_from_legacy(legacy: &IVerge) -> anyhow::Result<NyanpasuAppConfig> {
    let mut next = NyanpasuAppConfig::default();

    if let Some(value) = legacy.app_singleton_port {
        next.app_singleton_port = value;
    }
    if let Some(value) = &legacy.app_log_level {
        next.app_log_level = super::yaml_convert(value)?;
    }
    if let Some(value) = &legacy.language
        && let Ok(value) = super::yaml_convert(value)
    {
        next.language = value;
    }
    if let Some(value) = &legacy.theme_mode
        && let Ok(value) = super::yaml_convert(value)
    {
        next.theme_mode = value;
    }
    if let Some(value) = legacy.traffic_graph {
        next.traffic_graph = value;
    }
    if let Some(value) = legacy.enable_memory_usage {
        next.enable_memory_usage = value;
    }
    if let Some(value) = legacy.lighten_animation_effects {
        next.lighten_animation_effects = value;
    }
    if let Some(value) = legacy.enable_service_mode {
        next.enable_service_mode = value;
    }
    if let Some(value) = legacy.enable_auto_launch {
        next.enable_auto_launch = value;
    }
    if let Some(value) = legacy.enable_silent_start {
        next.enable_silent_start = value;
    }
    if let Some(value) = legacy.enable_system_proxy {
        next.enable_system_proxy = value;
    }
    if let Some(value) = legacy.enable_proxy_guard {
        next.enable_proxy_guard = value;
    }
    if let Some(value) = &legacy.system_proxy_bypass {
        next.system_proxy_bypass = value.clone();
    }
    if let Some(value) = legacy.proxy_guard_interval {
        next.proxy_guard_interval = value;
    }
    if let Some(value) = &legacy.theme_color
        && let Ok(value) = super::yaml_convert(value)
    {
        next.theme_color = value;
    }
    if let Some(value) = &legacy.clash_core
        && let Ok(value) = super::yaml_convert(value)
    {
        next.core = value;
    }
    if let Some(value) = &legacy.hotkeys {
        next.hotkeys = value.clone();
    }
    if let Some(value) = &legacy.default_latency_test {
        next.default_latency_test = value.clone();
    }
    if let Some(value) = legacy.enable_builtin_enhanced {
        next.enable_builtin_enhanced = value;
    }
    if let Some(value) = legacy.proxy_layout_column {
        next.proxy_layout_column = value;
    }
    if let Some(value) = legacy.max_log_files {
        next.max_log_files = value;
    }
    if let Some(value) = legacy.enable_auto_check_update {
        next.enable_auto_check_update = value;
    }
    if let Some(value) = &legacy.clash_tray_selector
        && let Ok(value) = super::yaml_convert(value)
    {
        next.tray_selector_mode = value;
    }
    if let Some(value) = legacy.always_on_top {
        next.always_on_top = value;
    }
    if let Some(value) = legacy.network_statistic_widget {
        next.network_statistic_widget = network_widget_from_legacy(value);
    }
    if let Some(value) = &legacy.pac_url
        && let Ok(value) = super::yaml_convert(value)
    {
        next.pac_url = Some(value);
    }
    if let Some(value) = legacy.enable_tray_text {
        next.enable_tray_text = value;
    }
    if let Some(value) = legacy.window_type {
        next.use_legacy_ui = matches!(value, legacy_app::WindowType::Main);
    }
    if let Some(value) = &legacy.tray_menu_mode
        && let Ok(value) = super::yaml_convert(value)
    {
        next.tray_menu_mode = value;
    }
    if let Some(value) = &legacy.tray_menu_close_behavior
        && let Ok(value) = super::yaml_convert(value)
    {
        next.tray_menu_close_behavior = value;
    }

    Ok(next)
}

fn apply_prepared_app_projection(target: &mut IVerge, projected: &IVerge) {
    target.app_singleton_port = projected.app_singleton_port;
    target.app_log_level = projected.app_log_level.clone();
    target.language = projected.language.clone();
    target.theme_mode = projected.theme_mode.clone();
    target.traffic_graph = projected.traffic_graph;
    target.enable_memory_usage = projected.enable_memory_usage;
    target.lighten_animation_effects = projected.lighten_animation_effects;
    target.enable_service_mode = projected.enable_service_mode;
    target.enable_auto_launch = projected.enable_auto_launch;
    target.enable_silent_start = projected.enable_silent_start;
    target.enable_system_proxy = projected.enable_system_proxy;
    target.enable_proxy_guard = projected.enable_proxy_guard;
    target.system_proxy_bypass = projected.system_proxy_bypass.clone();
    target.proxy_guard_interval = projected.proxy_guard_interval;
    target.theme_color = projected.theme_color.clone();
    target.clash_core = projected.clash_core.clone();
    target.hotkeys = projected.hotkeys.clone();
    target.default_latency_test = projected.default_latency_test.clone();
    target.enable_builtin_enhanced = projected.enable_builtin_enhanced;
    target.proxy_layout_column = projected.proxy_layout_column;
    target.max_log_files = projected.max_log_files;
    target.enable_auto_check_update = projected.enable_auto_check_update;
    target.clash_tray_selector = projected.clash_tray_selector.clone();
    target.always_on_top = projected.always_on_top;
    target.network_statistic_widget = projected.network_statistic_widget;
    target.pac_url = projected.pac_url.clone();
    target.enable_tray_text = projected.enable_tray_text;
    target.window_type = projected.window_type;
    target.tray_menu_mode = projected.tray_menu_mode.clone();
    target.tray_menu_close_behavior = projected.tray_menu_close_behavior.clone();
}

pub(crate) fn apply_app_config_to_legacy_verge(
    draft: &mut IVerge,
    snap: &NyanpasuAppConfig,
) -> anyhow::Result<()> {
    draft.app_singleton_port = Some(snap.app_singleton_port);
    draft.app_log_level = Some(super::yaml_convert(&snap.app_log_level)?);
    draft.language = Some(super::yaml_convert(&snap.language)?);
    draft.theme_mode = Some(super::yaml_convert(&snap.theme_mode)?);
    draft.traffic_graph = Some(snap.traffic_graph);
    draft.enable_memory_usage = Some(snap.enable_memory_usage);
    draft.lighten_animation_effects = Some(snap.lighten_animation_effects);
    draft.enable_service_mode = Some(snap.enable_service_mode);
    draft.enable_auto_launch = Some(snap.enable_auto_launch);
    draft.enable_silent_start = Some(snap.enable_silent_start);
    draft.enable_system_proxy = Some(snap.enable_system_proxy);
    draft.enable_proxy_guard = Some(snap.enable_proxy_guard);
    draft.system_proxy_bypass = if snap.system_proxy_bypass.is_empty() {
        None
    } else {
        Some(snap.system_proxy_bypass.clone())
    };
    draft.proxy_guard_interval = Some(snap.proxy_guard_interval);
    draft.theme_color = Some(super::yaml_convert(&snap.theme_color)?);
    draft.clash_core = Some(super::yaml_convert(&snap.core)?);
    draft.hotkeys = Some(snap.hotkeys.clone());
    draft.default_latency_test = Some(snap.default_latency_test.clone());
    draft.enable_builtin_enhanced = Some(snap.enable_builtin_enhanced);
    draft.proxy_layout_column = Some(snap.proxy_layout_column);
    draft.max_log_files = Some(snap.max_log_files);
    draft.enable_auto_check_update = Some(snap.enable_auto_check_update);
    draft.clash_tray_selector = Some(super::yaml_convert(&snap.tray_selector_mode)?);
    draft.always_on_top = Some(snap.always_on_top);
    draft.network_statistic_widget = Some(network_widget_to_legacy(snap.network_statistic_widget));
    draft.pac_url = snap.pac_url.as_ref().map(ToString::to_string);
    draft.enable_tray_text = Some(snap.enable_tray_text);
    draft.window_type = snap.use_legacy_ui.then_some(legacy_app::WindowType::Main);
    draft.tray_menu_mode = Some(super::yaml_convert(&snap.tray_menu_mode)?);
    draft.tray_menu_close_behavior = Some(super::yaml_convert(&snap.tray_menu_close_behavior)?);
    Ok(())
}

fn network_widget_from_legacy(
    value: legacy_app::NetworkStatisticWidgetConfig,
) -> AppNetworkStatisticWidgetConfig {
    match value {
        legacy_app::NetworkStatisticWidgetConfig::Disabled => {
            AppNetworkStatisticWidgetConfig::Disabled
        }
        legacy_app::NetworkStatisticWidgetConfig::Large => {
            AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Large)
        }
        legacy_app::NetworkStatisticWidgetConfig::Small => {
            AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small)
        }
    }
}

fn network_widget_to_legacy(
    value: AppNetworkStatisticWidgetConfig,
) -> legacy_app::NetworkStatisticWidgetConfig {
    match value {
        AppNetworkStatisticWidgetConfig::Disabled => {
            legacy_app::NetworkStatisticWidgetConfig::Disabled
        }
        AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Large) => {
            legacy_app::NetworkStatisticWidgetConfig::Large
        }
        AppNetworkStatisticWidgetConfig::Enabled(StatisticWidgetVariant::Small) => {
            legacy_app::NetworkStatisticWidgetConfig::Small
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        client::{
            ClientError, ClientSetupArgs, CompensationFailure, LegacyBridgeSet,
            LegacyRunningConfigPatchBridge, LegacyVergeDomain, MockRunningCoreBridge,
            NoopUiEventSink, NyanpasuClient,
        },
        config::{
            IClashTemp,
            nyanpasu::{
                LoggingLevel, NetworkStatisticWidgetConfig, ProxiesSelectorMode, TrayMenuMode,
            },
        },
        state::mirror::{
            ClashLegacyBridge, NoopPreparedLegacyMirror, PreparedLegacyMirror, WindowLegacyBridge,
        },
    };
    use nyanpasu_config::{
        application::I18nLanguage,
        clash::config::ClashConfig,
        state::{
            PersistentState,
            window::{WindowLabel, WindowState},
        },
    };
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex as StdMutex, mpsc},
    };
    use struct_patch::Patch;
    use tempfile::{TempDir, tempdir};
    use tokio::sync::oneshot;

    static INTERLEAVING_TEST_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

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

    /// Test-only double that accepts the initial empty snapshot and fails once
    /// the session patch contains window state.
    struct FailingWindowMirror;

    impl WindowLegacyBridge for FailingWindowMirror {
        fn prepare(&self, snap: &PersistentState) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            if snap.window_state.is_empty() {
                return Ok(Box::new(NoopPreparedLegacyMirror));
            }
            anyhow::bail!("injected session mirror prepare failure");
        }

        fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
            Ok(PersistentState::default())
        }
    }

    struct BlockingPreparedLegacyCommit {
        inner: Box<dyn PreparedLegacyVergeCommit>,
        entered: oneshot::Sender<()>,
        release: Arc<StdMutex<mpsc::Receiver<()>>>,
    }

    impl PreparedLegacyVergeCommit for BlockingPreparedLegacyCommit {
        fn commit(self: Box<Self>) -> anyhow::Result<()> {
            let _ = self.entered.send(());
            self.release.lock().unwrap().recv().unwrap();
            self.inner.commit()
        }
    }

    struct BlockingLegacyCommitStore {
        inner: ConfigLegacyVergeStore,
        block_restore: bool,
        barrier: StdMutex<Option<(oneshot::Sender<()>, Arc<StdMutex<mpsc::Receiver<()>>>)>>,
    }

    impl BlockingLegacyCommitStore {
        fn block(
            &self,
            inner: Box<dyn PreparedLegacyVergeCommit>,
        ) -> anyhow::Result<Box<dyn PreparedLegacyVergeCommit>> {
            let Some((entered, release)) = self.barrier.lock().unwrap().take() else {
                return Ok(inner);
            };
            Ok(Box::new(BlockingPreparedLegacyCommit {
                inner,
                entered,
                release,
            }))
        }
    }

    impl LegacyVergeStore for BlockingLegacyCommitStore {
        fn snapshot(&self) -> anyhow::Result<IVerge> {
            self.inner.snapshot()
        }

        fn prepare_application(
            &self,
            snap: &NyanpasuAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            self.inner.prepare_application(snap)
        }

        fn prepare_commit(
            &self,
            path: &Utf8Path,
            state: IVerge,
        ) -> anyhow::Result<Box<dyn PreparedLegacyVergeCommit>> {
            let inner = self.inner.prepare_commit(path, state)?;
            if self.block_restore {
                return Ok(inner);
            }
            self.block(inner)
        }

        fn prepare_restore(
            &self,
            path: &Utf8Path,
            state: IVerge,
        ) -> anyhow::Result<Box<dyn PreparedLegacyVergeCommit>> {
            let inner = self.inner.prepare_restore(path, state)?;
            if !self.block_restore {
                return Ok(inner);
            }
            self.block(inner)
        }

        fn prepare_projection(
            &self,
            state: IVerge,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            self.inner.prepare_projection(state)
        }
    }

    struct FailingLegacyCommit;

    impl PreparedLegacyVergeCommit for FailingLegacyCommit {
        fn commit(self: Box<Self>) -> anyhow::Result<()> {
            anyhow::bail!("injected legacy persistence failure")
        }
    }

    impl LegacyVergeStore for FailingLegacyCommit {
        fn snapshot(&self) -> anyhow::Result<IVerge> {
            ConfigLegacyVergeStore::default().snapshot()
        }

        fn prepare_application(
            &self,
            snap: &NyanpasuAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            ConfigLegacyVergeStore::default().prepare_application(snap)
        }

        fn prepare_commit(
            &self,
            _path: &Utf8Path,
            _state: IVerge,
        ) -> anyhow::Result<Box<dyn PreparedLegacyVergeCommit>> {
            Ok(Box::new(Self))
        }

        fn prepare_restore(
            &self,
            path: &Utf8Path,
            state: IVerge,
        ) -> anyhow::Result<Box<dyn PreparedLegacyVergeCommit>> {
            ConfigLegacyVergeStore::default().prepare_restore(path, state)
        }

        fn prepare_projection(
            &self,
            state: IVerge,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            ConfigLegacyVergeStore::default().prepare_projection(state)
        }
    }

    struct NoopClashBridge;

    impl ClashLegacyBridge for NoopClashBridge {
        fn prepare(&self, _snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
        }
    }

    struct RecordingPreparedMirror {
        event: &'static str,
        events: Arc<StdMutex<Vec<&'static str>>>,
        entered: Option<oneshot::Sender<()>>,
        release: Option<Arc<StdMutex<mpsc::Receiver<()>>>>,
    }

    impl PreparedLegacyMirror for RecordingPreparedMirror {
        fn apply(self: Box<Self>) {
            self.events.lock().unwrap().push(self.event);
            if let Some(entered) = self.entered {
                let _ = entered.send(());
            }
            if let Some(release) = self.release {
                release.lock().unwrap().recv().unwrap();
            }
        }
    }

    struct RecordingVergeMirror {
        events: Arc<StdMutex<Vec<&'static str>>>,
        barrier: StdMutex<Option<(oneshot::Sender<()>, Arc<StdMutex<mpsc::Receiver<()>>>)>>,
    }

    impl VergeLegacyBridge for RecordingVergeMirror {
        fn prepare(
            &self,
            snap: &NyanpasuAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            let is_new = snap.theme_color.to_string() == "#abcdef";
            let (entered, release) = if is_new {
                match self.barrier.lock().unwrap().take() {
                    Some((entered, release)) => (Some(entered), Some(release)),
                    None => (None, None),
                }
            } else {
                (None, None)
            };
            Ok(Box::new(RecordingPreparedMirror {
                event: if is_new {
                    "application:new"
                } else {
                    "application:old"
                },
                events: Arc::clone(&self.events),
                entered,
                release,
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
            Ok(NyanpasuAppConfig::default())
        }
    }

    struct RecordingWindowMirror {
        events: Arc<StdMutex<Vec<&'static str>>>,
        barrier: StdMutex<Option<(oneshot::Sender<()>, Arc<StdMutex<mpsc::Receiver<()>>>)>>,
    }

    impl WindowLegacyBridge for RecordingWindowMirror {
        fn prepare(&self, snap: &PersistentState) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            let is_new = !snap.window_state.is_empty();
            let (entered, release) = if is_new {
                match self.barrier.lock().unwrap().take() {
                    Some((entered, release)) => (Some(entered), Some(release)),
                    None => (None, None),
                }
            } else {
                (None, None)
            };
            Ok(Box::new(RecordingPreparedMirror {
                event: if is_new { "session:new" } else { "session:old" },
                events: Arc::clone(&self.events),
                entered,
                release,
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
            Ok(PersistentState::default())
        }
    }

    fn temp_config_path(dir: &TempDir, file_name: &str) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(dir.path().join(file_name)).expect("temp path should be UTF-8")
    }

    fn test_bridge(dir: &TempDir) -> (NyanpasuClient, LegacyVergeBridge) {
        test_bridge_with_window(dir, Arc::new(NoopWindowBridge))
    }

    fn test_bridge_with_window(
        dir: &TempDir,
        window: Arc<dyn WindowLegacyBridge>,
    ) -> (NyanpasuClient, LegacyVergeBridge) {
        test_bridge_with_bridges(
            dir,
            Arc::new(LegacyVergeBridge::default()),
            window,
            Arc::new(NoopClashBridge),
        )
    }

    fn test_bridge_with_bridges(
        dir: &TempDir,
        verge: Arc<dyn VergeLegacyBridge>,
        window: Arc<dyn WindowLegacyBridge>,
        clash: Arc<dyn ClashLegacyBridge>,
    ) -> (NyanpasuClient, LegacyVergeBridge) {
        test_bridge_with_bridges_and_store(
            dir,
            verge,
            window,
            clash,
            Arc::new(ConfigLegacyVergeStore::default()),
        )
    }

    fn test_bridge_with_bridges_and_store(
        dir: &TempDir,
        verge: Arc<dyn VergeLegacyBridge>,
        window: Arc<dyn WindowLegacyBridge>,
        clash: Arc<dyn ClashLegacyBridge>,
        legacy_store: Arc<dyn LegacyVergeStore>,
    ) -> (NyanpasuClient, LegacyVergeBridge) {
        let clash_store = Config::clash();
        *clash_store.draft() = IClashTemp::template();
        clash_store.apply();
        let verge_store = Config::verge();
        *verge_store.draft() = IVerge::default();
        verge_store.apply();

        let paths = crate::utils::path::PathResolver::with_base_dirs(
            dir.path().into(),
            dir.path().join("data"),
        );
        let legacy_verge_path = temp_config_path(dir, "nyanpasu-config.yaml");
        let runtime_paths = crate::client::RuntimePaths::from_resolver(&paths).unwrap();
        let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
            paths,
            runtime_paths,
            bridges: LegacyBridgeSet {
                verge,
                window,
                clash,
            },
            ui_sink: Arc::new(NoopUiEventSink),
            core: Arc::new(MockRunningCoreBridge::new()),
            clash_patch: Some(Arc::new(LegacyRunningConfigPatchBridge)),
            system_dns: Arc::new(crate::client::NoopSystemDnsCache),
        })
        .expect("client should construct with typed config actors");
        let bridge = LegacyVergeBridge::new(client.clone(), legacy_verge_path, legacy_store);
        (client, bridge)
    }

    #[test]
    fn apply_app_config_to_legacy_verge_maps_empty_bypass_to_none() {
        let snap = NyanpasuAppConfig::default();
        let mut draft = IVerge::default();

        apply_app_config_to_legacy_verge(&mut draft, &snap)
            .expect("app config should map to legacy verge");

        assert_eq!(draft.system_proxy_bypass, None);
    }

    #[test]
    fn apply_app_config_to_legacy_verge_preserves_custom_bypass() {
        let mut snap = NyanpasuAppConfig::default();
        snap.system_proxy_bypass = "localhost;127.*;<local>".to_string();
        let mut draft = IVerge::default();

        apply_app_config_to_legacy_verge(&mut draft, &snap)
            .expect("app config should map to legacy verge");

        assert_eq!(
            draft.system_proxy_bypass.as_deref(),
            Some("localhost;127.*;<local>")
        );
    }

    #[test]
    fn apply_app_config_to_legacy_verge_preserves_whitespace_only_bypass() {
        let mut snap = NyanpasuAppConfig::default();
        snap.system_proxy_bypass = " \t\r\n".to_string();
        let mut draft = IVerge::default();

        apply_app_config_to_legacy_verge(&mut draft, &snap)
            .expect("app config should map to legacy verge");

        assert_eq!(draft.system_proxy_bypass.as_deref(), Some(" \t\r\n"));
    }

    #[test]
    fn get_verge_config_composes_typed_actor_snapshots() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (client, bridge) = test_bridge(&dir);

        tauri::async_runtime::block_on(async {
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

            let verge = bridge
                .get_verge_config()
                .await
                .expect("legacy verge config should compose from typed snapshots");
            assert_eq!(verge.enable_system_proxy, Some(true));
            assert_eq!(verge.system_proxy_bypass, None);
            assert_eq!(verge.enable_tun_mode, Some(true));
            assert_eq!(
                verge.window_size_state.as_ref().map(|state| state.width),
                Some(window_state.width)
            );
        });
    }

    #[test]
    fn legacy_patch_then_get_verge_config_preserves_contract() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (client, bridge) = test_bridge(&dir);

        tauri::async_runtime::block_on(async {
            bridge
                .patch_verge_config(IVerge {
                    theme_color: Some("#112233".into()),
                    ..IVerge::default()
                })
                .await
                .expect("legacy patch should succeed");

            let verge = bridge
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
    fn pure_verge_patch_persists_legacy_snapshot_to_injected_path() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (_client, bridge) = test_bridge(&dir);

        tauri::async_runtime::block_on(async {
            bridge
                .patch_verge_config(IVerge {
                    theme_color: Some("#223344".into()),
                    ..IVerge::default()
                })
                .await
                .expect("pure legacy patch should persist");

            let saved: IVerge = crate::utils::help::read_yaml(
                temp_config_path(&dir, "nyanpasu-config.yaml").as_std_path(),
            )
            .expect("legacy verge snapshot should be saved to injected path");
            assert_eq!(saved.theme_color.as_deref(), Some("#223344"));
        });
    }

    #[test]
    fn legacy_commit_failure_compensates_typed_application() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let events = Arc::new(StdMutex::new(Vec::new()));
        let (client, bridge) = test_bridge_with_bridges_and_store(
            &dir,
            Arc::new(RecordingVergeMirror {
                events: Arc::clone(&events),
                barrier: StdMutex::new(None),
            }),
            Arc::new(NoopWindowBridge),
            Arc::new(NoopClashBridge),
            Arc::new(FailingLegacyCommit),
        );

        tauri::async_runtime::block_on(async {
            let before = client
                .typed_config_snapshots()
                .await
                .expect("snapshots should load");
            let error = bridge
                .patch_verge_config(IVerge {
                    theme_color: Some("#abcdef".into()),
                    ..IVerge::default()
                })
                .await
                .expect_err("legacy persistence failure should fail the patch");
            let ClientError::PartialCommit(partial) = error else {
                panic!("expected legacy uncertainty partial commit, got {error:#}");
            };
            assert!(
                partial
                    .primary_error
                    .contains("failed to finalize legacy verge persistence")
            );
            assert!(matches!(
                partial.failed_compensations.as_slice(),
                [CompensationFailure::LegacyStateUncertain { message }]
                    if message.contains("injected legacy persistence failure")
            ));
            assert_eq!(
                partial.compensated_domains,
                vec![LegacyVergeDomain::Application]
            );
            let after = client
                .typed_config_snapshots()
                .await
                .expect("snapshots should reload");
            assert_eq!(after.application.version, before.application.version + 2);
            assert_eq!(
                after.application.state.theme_color,
                before.application.state.theme_color
            );
            assert_eq!(after.session.version, before.session.version);
            assert_eq!(after.clash.version, before.clash.version);
        });
    }

    #[test]
    fn legacy_finalizer_preserves_concurrent_typed_projection() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let legacy_lock = Arc::new(parking_lot::Mutex::new(()));
        let legacy_store = Arc::new(BlockingLegacyCommitStore {
            inner: ConfigLegacyVergeStore::new(legacy_lock),
            block_restore: false,
            barrier: StdMutex::new(Some((entered_tx, Arc::new(StdMutex::new(release_rx))))),
        });
        let (client, bridge) = test_bridge_with_bridges_and_store(
            &dir,
            Arc::new(LegacyVergeBridge::with_store(legacy_store.clone())),
            Arc::new(NoopWindowBridge),
            Arc::new(NoopClashBridge),
            legacy_store,
        );

        tauri::async_runtime::block_on(async {
            let saga = bridge.clone();
            let task = tauri::async_runtime::spawn(async move {
                saga.patch_verge_config(IVerge {
                    theme_color: Some("#abcdef".into()),
                    ..IVerge::default()
                })
                .await
            });

            entered_rx.await.expect("legacy finalizer should start");
            let mut app_patch = NyanpasuAppConfig::new_empty_patch();
            app_patch.language = Some(I18nLanguage::Korean);
            client
                .patch_app_config(app_patch)
                .await
                .expect("concurrent typed update should succeed");
            release_tx
                .send(())
                .expect("legacy finalizer should release");
            task.await
                .expect("saga task should join")
                .expect("saga should succeed");

            let typed = client
                .get_app_config()
                .await
                .expect("application state should load");
            assert_eq!(typed.theme_color.to_string(), "#abcdef");
            assert_eq!(typed.language, I18nLanguage::Korean);
            assert_eq!(Config::verge().data().language.as_deref(), Some("ko"));
            let saved: IVerge = crate::utils::help::read_yaml(
                temp_config_path(&dir, "nyanpasu-config.yaml").as_std_path(),
            )
            .expect("legacy snapshot should load");
            assert_eq!(saved.language.as_deref(), Some("ko"));
        });
    }

    #[test]
    fn legacy_mutation_preserves_concurrent_typed_update_before_restore() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let legacy_lock = Arc::new(parking_lot::Mutex::new(()));
        let legacy_store = Arc::new(BlockingLegacyCommitStore {
            inner: ConfigLegacyVergeStore::new(legacy_lock.clone()),
            block_restore: true,
            barrier: StdMutex::new(Some((entered_tx, Arc::new(StdMutex::new(release_rx))))),
        });
        let (client, bridge) = test_bridge_with_bridges_and_store(
            &dir,
            Arc::new(LegacyVergeBridge::with_store(legacy_store.clone())),
            Arc::new(crate::bridge::window::LegacyWindowBridge::new(legacy_lock)),
            Arc::new(NoopClashBridge),
            legacy_store,
        );

        tauri::async_runtime::block_on(async {
            let mutation = bridge.clone();
            let task = tauri::async_runtime::spawn(async move {
                mutation
                    .run_legacy_verge_mutation(|| async {
                        Config::verge().draft().theme_color = Some("#abcdef".into());
                        Config::verge().apply();
                        Ok(())
                    })
                    .await
            });

            entered_rx.await.expect("legacy restore should start");
            let concurrent_window = WindowState {
                width: 1000,
                height: 700,
                x: 10,
                y: 20,
                maximized: false,
                fullscreen: false,
            };
            let mut session_patch = PersistentState::new_empty_patch();
            session_patch.window_state = Some(BTreeMap::from([(
                WindowLabel("main".into()),
                concurrent_window.clone(),
            )]));
            client
                .patch_session_state(session_patch)
                .await
                .expect("concurrent typed session update should succeed");
            release_tx.send(()).expect("legacy restore should release");
            task.await
                .expect("mutation task should join")
                .expect("mutation saga should succeed");

            let application = client
                .get_app_config()
                .await
                .expect("application state should load");
            let session = client
                .get_session_state()
                .await
                .expect("session state should load");
            assert_eq!(application.theme_color.to_string(), "#abcdef");
            assert_eq!(
                session.window_state.get(&WindowLabel("main".into())),
                Some(&concurrent_window)
            );
            assert_eq!(
                Config::verge()
                    .data()
                    .window_size_state
                    .as_ref()
                    .map(|state| state.width),
                Some(1000)
            );
        });
    }

    #[test]
    fn legacy_mutation_failure_restores_legacy_state_and_reports_reconciliation() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (client, bridge) = test_bridge_with_window(&dir, Arc::new(FailingWindowMirror));

        tauri::async_runtime::block_on(async {
            let error = bridge
                .run_legacy_verge_mutation(|| async {
                    Config::verge().draft().patch_config(IVerge {
                        window_size_state: Some(crate::config::nyanpasu::WindowState {
                            width: 1440,
                            height: 900,
                            x: 1,
                            y: 2,
                            maximized: false,
                            fullscreen: false,
                        }),
                        ..IVerge::default()
                    });
                    Config::verge().apply();
                    Ok(())
                })
                .await
                .expect_err("typed prepare failure must report reconciliation");
            let ClientError::PartialCommit(partial) = error else {
                panic!("expected partial commit, got {error:#}");
            };
            assert!(partial.failed_compensations.iter().any(|failure| matches!(
                failure,
                CompensationFailure::LegacyStateUncertain { .. }
            )));
            assert!(Config::verge().data().window_size_state.is_none());
            assert!(
                client
                    .get_session_state()
                    .await
                    .expect("session state should load")
                    .window_state
                    .is_empty()
            );
        });
    }

    #[test]
    fn failed_legacy_mutation_reports_uncertainty_without_guessing_restore() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (_client, bridge) = test_bridge(&dir);

        tauri::async_runtime::block_on(async {
            let error = bridge
                .run_legacy_verge_mutation(|| async {
                    Config::verge().draft().theme_color = Some("#abcdef".into());
                    Config::verge().apply();
                    anyhow::bail!("injected legacy mutation failure")
                })
                .await
                .expect_err("failed legacy mutation must report uncertainty");
            let ClientError::PartialCommit(partial) = error else {
                panic!("expected partial commit, got {error:#}");
            };
            assert!(
                partial
                    .primary_error
                    .contains("injected legacy mutation failure")
            );
            assert!(partial.failed_compensations.iter().any(|failure| matches!(
                failure,
                CompensationFailure::LegacyStateUncertain { message }
                    if message.contains("injected legacy mutation failure")
            )));
        });
    }

    #[test]
    fn failed_legacy_mutation_preserves_concurrent_typed_projection() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let legacy_lock = Arc::new(parking_lot::Mutex::new(()));
        let legacy_store = Arc::new(ConfigLegacyVergeStore::new(legacy_lock));
        let (client, bridge) = test_bridge_with_bridges_and_store(
            &dir,
            Arc::new(LegacyVergeBridge::with_store(legacy_store.clone())),
            Arc::new(NoopWindowBridge),
            Arc::new(NoopClashBridge),
            legacy_store,
        );
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();

        tauri::async_runtime::block_on(async {
            let mutation = bridge.clone();
            let task = tauri::async_runtime::spawn(async move {
                mutation
                    .run_legacy_verge_mutation(|| async move {
                        Config::verge().draft().theme_color = Some("#abcdef".into());
                        Config::verge().apply();
                        let _ = entered_tx.send(());
                        let _ = release_rx.await;
                        anyhow::bail!("injected legacy mutation failure")
                    })
                    .await
            });

            entered_rx.await.expect("legacy mutation should apply");
            let mut app_patch = NyanpasuAppConfig::new_empty_patch();
            app_patch.language = Some(I18nLanguage::Korean);
            client
                .patch_app_config(app_patch)
                .await
                .expect("concurrent typed update should succeed");
            release_tx.send(()).expect("legacy mutation should release");

            let error = task
                .await
                .expect("mutation task should join")
                .expect_err("mutation failure should surface");
            assert!(matches!(error, ClientError::PartialCommit(_)));
            let typed = client
                .get_app_config()
                .await
                .expect("application state should load");
            assert_eq!(typed.language, I18nLanguage::Korean);
            assert_eq!(Config::verge().data().language.as_deref(), Some("ko"));
        });
    }

    #[test]
    fn pure_verge_patch_preserves_session_state_fields() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (_client, bridge) = test_bridge(&dir);

        tauri::async_runtime::block_on(async {
            bridge
                .patch_verge_config(IVerge {
                    window_size_state: Some(crate::config::nyanpasu::WindowState {
                        width: 1200,
                        height: 900,
                        x: 30,
                        y: 40,
                        maximized: true,
                        fullscreen: false,
                    }),
                    ..IVerge::default()
                })
                .await
                .expect("window state patch should persist");

            let verge = bridge
                .get_verge_config()
                .await
                .expect("legacy verge config should compose patched window state");
            assert_eq!(
                verge.window_size_state.as_ref().map(|state| state.width),
                Some(1200)
            );
            assert_eq!(
                verge
                    .window_size_state
                    .as_ref()
                    .map(|state| state.maximized),
                Some(true)
            );
        });
    }

    #[test]
    fn pure_verge_patch_preserves_clash_config_fields() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (_client, bridge) = test_bridge(&dir);

        tauri::async_runtime::block_on(async {
            bridge
                .patch_verge_config(IVerge {
                    web_ui_list: Some(vec!["https://example.invalid/ui".to_string()]),
                    ..IVerge::default()
                })
                .await
                .expect("clash config patch should persist");

            let verge = bridge
                .get_verge_config()
                .await
                .expect("legacy verge config should compose patched clash config");
            assert_eq!(
                verge.web_ui_list.as_deref(),
                Some(["https://example.invalid/ui".to_string()].as_slice())
            );
        });
    }

    #[test]
    #[allow(deprecated)]
    fn replace_verge_config_persists_legacy_only_fields_to_injected_path() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (_client, bridge) = test_bridge(&dir);

        tauri::async_runtime::block_on(async {
            bridge
                .replace_verge_config(IVerge {
                    auto_log_clean: Some(14),
                    theme_color: Some("#334455".into()),
                    ..IVerge::default()
                })
                .await
                .expect("legacy replacement should persist");

            let saved: IVerge = crate::utils::help::read_yaml(
                temp_config_path(&dir, "nyanpasu-config.yaml").as_std_path(),
            )
            .expect("legacy replacement should be saved to injected path");
            assert_eq!(saved.auto_log_clean, Some(14));
            assert_eq!(saved.theme_color.as_deref(), Some("#334455"));
        });
    }

    #[test]
    fn legacy_mutation_reseeds_typed_actors_without_os_side_effects() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (client, bridge) = test_bridge(&dir);

        tauri::async_runtime::block_on(async {
            bridge
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
                bridge
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
    fn validate_verge_patch_accepts_valid_theme_colors() {
        assert!(
            LegacyVergeBridge::validate_patch(&IVerge {
                theme_color: Some(String::new()),
                ..IVerge::default()
            })
            .is_ok()
        );
        assert!(
            LegacyVergeBridge::validate_patch(&IVerge {
                theme_color: Some("#0a1B2c".into()),
                ..IVerge::default()
            })
            .is_ok()
        );
    }

    #[test]
    fn validate_verge_patch_rejects_invalid_theme_colors() {
        let short = LegacyVergeBridge::validate_patch(&IVerge {
            theme_color: Some("#abc".into()),
            ..IVerge::default()
        })
        .expect_err("short hex should fail");
        assert!(short.to_string().contains("Invalid theme color"));

        let non_hex = LegacyVergeBridge::validate_patch(&IVerge {
            theme_color: Some("#GGGGGG".into()),
            ..IVerge::default()
        })
        .expect_err("non-hex color should fail");
        assert!(non_hex.to_string().contains("Invalid theme color"));
    }

    #[test]
    fn route_verge_patch_classifies_pure_fields() {
        macro_rules! assert_pure {
            ($field:ident: $value:expr) => {{
                let mut patch = IVerge::default();
                patch.$field = Some($value);
                assert_eq!(
                    LegacyVergeBridge::route_patch(&patch),
                    LegacyVergePatchRoute::PureConfig,
                    stringify!($field)
                );
            }};
        }

        assert_pure!(theme_color: "#112233".to_string());
        assert_pure!(traffic_graph: true);
        assert_pure!(theme_mode: "dark".to_string());
    }

    #[test]
    fn route_verge_patch_classifies_side_effect_fields() {
        macro_rules! assert_legacy {
            ($field:ident: $value:expr) => {{
                let mut patch = IVerge::default();
                patch.$field = Some($value);
                assert_eq!(
                    LegacyVergeBridge::route_patch(&patch),
                    LegacyVergePatchRoute::LegacySideEffects,
                    stringify!($field)
                );
            }};
        }

        assert_legacy!(enable_service_mode: true);
        assert_legacy!(enable_tun_mode: true);
        assert_legacy!(enable_auto_launch: true);
        assert_legacy!(enable_system_proxy: true);
        assert_legacy!(system_proxy_bypass: "localhost".to_string());
        assert_legacy!(enable_proxy_guard: true);
        assert_legacy!(hotkeys: Vec::<String>::new());
        assert_legacy!(language: "en".to_string());
        assert_legacy!(app_log_level: LoggingLevel::default());
        assert_legacy!(max_log_files: 7usize);
        #[allow(deprecated)]
        {
            assert_legacy!(auto_log_clean: 7i64);
        }
        assert_legacy!(clash_tray_selector: ProxiesSelectorMode::default());
        assert_legacy!(enable_tray_text: true);
        assert_legacy!(tray_menu_mode: TrayMenuMode::default());
        assert_legacy!(network_statistic_widget: NetworkStatisticWidgetConfig::default());
    }

    #[test]
    fn three_domain_second_domain_prepare_failure_leaves_failing_domain_old() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (client, bridge) = test_bridge_with_window(&dir, Arc::new(FailingWindowMirror));

        tauri::async_runtime::block_on(async {
            let versions_before = client
                .typed_config_snapshots()
                .await
                .expect("typed snapshots should load");
            let app_before = client
                .get_app_config()
                .await
                .expect("app get should succeed");
            let session_before = client
                .get_session_state()
                .await
                .expect("session get should succeed");
            assert_ne!(app_before.theme_color.to_string(), "#abcdef");
            assert!(session_before.window_state.is_empty());

            let err = bridge
                .patch_verge_config(IVerge {
                    // Application domain (first)
                    theme_color: Some("#abcdef".into()),
                    // Session domain (second) — mirror preparation fails before upsert
                    window_size_state: Some(crate::config::nyanpasu::WindowState {
                        width: 1440,
                        height: 900,
                        x: 1,
                        y: 2,
                        maximized: false,
                        fullscreen: false,
                    }),
                    // Clash domain (third) — should not be reached under sequential plan
                    web_ui_list: Some(vec!["https://example.invalid/ui".to_string()]),
                    ..IVerge::default()
                })
                .await
                .expect_err("second-domain prepare failure must surface as Err");
            let err_text = format!("{err:#}");
            assert!(
                err_text.contains("legacy session mirror")
                    || err_text.contains("injected session mirror failure"),
                "unexpected error: {err_text}"
            );

            let app_after = client
                .get_app_config()
                .await
                .expect("app get after failure should succeed");
            let session_after = client
                .get_session_state()
                .await
                .expect("session get after failure should succeed");
            let clash_after = client
                .get_clash_config()
                .await
                .expect("clash get after failure should succeed");
            let versions_after = client
                .typed_config_snapshots()
                .await
                .expect("typed snapshots should reload");

            assert_eq!(
                versions_after.application.version,
                versions_before.application.version
            );
            assert_eq!(
                versions_after.session.version,
                versions_before.session.version
            );
            assert_eq!(versions_after.clash.version, versions_before.clash.version);
            assert_eq!(
                app_after.theme_color.to_string(),
                app_before.theme_color.to_string(),
                "application domain must remain old when any prepare fails"
            );
            // Prepared mirror failure prevents the failing domain from committing.
            assert!(
                session_after.window_state.is_empty(),
                "session domain must remain old when mirror preparation fails"
            );
            // Third domain must not have been patched (sequential stop).
            assert!(
                clash_after.web_ui_list.is_empty(),
                "clash domain must remain untouched when second domain fails"
            );
        });
    }

    #[test]
    fn three_domain_second_domain_commit_failure_compensates_application() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let events = Arc::new(StdMutex::new(Vec::new()));
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let verge = Arc::new(RecordingVergeMirror {
            events: Arc::clone(&events),
            barrier: StdMutex::new(Some((entered_tx, Arc::new(StdMutex::new(release_rx))))),
        });
        let (client, bridge) = test_bridge_with_bridges(
            &dir,
            verge,
            Arc::new(NoopWindowBridge),
            Arc::new(NoopClashBridge),
        );

        tauri::async_runtime::block_on(async {
            events.lock().unwrap().clear();
            let before = client
                .typed_config_snapshots()
                .await
                .expect("snapshots should load");
            let saga = bridge.clone();
            let task = tauri::async_runtime::spawn(async move {
                saga.patch_verge_config(IVerge {
                    theme_color: Some("#abcdef".into()),
                    window_size_state: Some(crate::config::nyanpasu::WindowState {
                        width: 1440,
                        height: 900,
                        x: 1,
                        y: 2,
                        maximized: false,
                        fullscreen: false,
                    }),
                    ..IVerge::default()
                })
                .await
            });

            entered_rx.await.expect("application apply should start");
            let mut session_patch = PersistentState::new_empty_patch();
            session_patch.window_state = Some(BTreeMap::from([(
                WindowLabel("concurrent".into()),
                WindowState {
                    width: 800,
                    height: 600,
                    x: 0,
                    y: 0,
                    maximized: false,
                    fullscreen: false,
                },
            )]));
            client
                .patch_session_state(session_patch)
                .await
                .expect("concurrent session update should succeed");
            release_tx
                .send(())
                .expect("application apply should release");

            let error = task
                .await
                .expect("saga task should join")
                .expect_err("session CAS conflict should fail the saga");
            assert!(
                error
                    .to_string()
                    .contains("session config version conflict")
            );

            let after = client
                .typed_config_snapshots()
                .await
                .expect("snapshots should reload");
            assert_eq!(after.application.version, before.application.version + 2);
            assert_eq!(after.session.version, before.session.version + 1);
            assert_eq!(after.clash.version, before.clash.version);
            assert_eq!(
                after.application.state.theme_color,
                before.application.state.theme_color
            );
            assert!(!after.session.state.window_state.is_empty());
            assert_eq!(
                *events.lock().unwrap(),
                vec!["application:new", "application:old"]
            );
        });
    }

    #[test]
    fn full_replacement_second_domain_failure_compensates_application() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let events = Arc::new(StdMutex::new(Vec::new()));
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (client, bridge) = test_bridge_with_bridges(
            &dir,
            Arc::new(RecordingVergeMirror {
                events: Arc::clone(&events),
                barrier: StdMutex::new(Some((entered_tx, Arc::new(StdMutex::new(release_rx))))),
            }),
            Arc::new(NoopWindowBridge),
            Arc::new(NoopClashBridge),
        );

        tauri::async_runtime::block_on(async {
            events.lock().unwrap().clear();
            let before = client
                .typed_config_snapshots()
                .await
                .expect("snapshots should load");
            let saga = bridge.clone();
            let task = tauri::async_runtime::spawn(async move {
                saga.replace_verge_config(IVerge {
                    theme_color: Some("#abcdef".into()),
                    window_size_state: Some(crate::config::nyanpasu::WindowState {
                        width: 1440,
                        height: 900,
                        x: 1,
                        y: 2,
                        maximized: false,
                        fullscreen: false,
                    }),
                    ..IVerge::default()
                })
                .await
            });

            entered_rx.await.expect("application apply should start");
            let mut session_patch = PersistentState::new_empty_patch();
            session_patch.window_state = Some(BTreeMap::from([(
                WindowLabel("concurrent".into()),
                WindowState {
                    width: 800,
                    height: 600,
                    x: 0,
                    y: 0,
                    maximized: false,
                    fullscreen: false,
                },
            )]));
            client
                .patch_session_state(session_patch)
                .await
                .expect("concurrent session update should succeed");
            release_tx
                .send(())
                .expect("application apply should release");

            let error = task
                .await
                .expect("replacement task should join")
                .expect_err("session CAS conflict should fail full replacement");
            assert!(
                format!("{error:#}").contains("session config version conflict"),
                "unexpected replacement error: {error:#}"
            );
            let after = client
                .typed_config_snapshots()
                .await
                .expect("snapshots should reload");
            assert_eq!(after.application.version, before.application.version + 2);
            assert_eq!(
                after.application.state.theme_color,
                before.application.state.theme_color
            );
            assert_eq!(after.session.version, before.session.version + 1);
            assert_eq!(after.clash.version, before.clash.version);
            assert_eq!(
                *events.lock().unwrap(),
                vec!["application:new", "application:old"]
            );
        });
    }

    #[test]
    fn three_domain_third_domain_failure_compensates_session_then_application() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let events = Arc::new(StdMutex::new(Vec::new()));
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let window = Arc::new(RecordingWindowMirror {
            events: Arc::clone(&events),
            barrier: StdMutex::new(Some((entered_tx, Arc::new(StdMutex::new(release_rx))))),
        });
        let (client, bridge) = test_bridge_with_bridges(
            &dir,
            Arc::new(RecordingVergeMirror {
                events: Arc::clone(&events),
                barrier: StdMutex::new(None),
            }),
            window,
            Arc::new(NoopClashBridge),
        );

        tauri::async_runtime::block_on(async {
            events.lock().unwrap().clear();
            let before = client
                .typed_config_snapshots()
                .await
                .expect("snapshots should load");
            let saga = bridge.clone();
            let task = tauri::async_runtime::spawn(async move {
                saga.patch_verge_config(IVerge {
                    theme_color: Some("#abcdef".into()),
                    window_size_state: Some(crate::config::nyanpasu::WindowState {
                        width: 1440,
                        height: 900,
                        x: 1,
                        y: 2,
                        maximized: false,
                        fullscreen: false,
                    }),
                    web_ui_list: Some(vec!["https://example.invalid/ui".into()]),
                    ..IVerge::default()
                })
                .await
            });

            entered_rx.await.expect("session apply should start");
            let mut clash_patch = ClashConfig::new_empty_patch();
            clash_patch.web_ui_list = Some(vec!["https://concurrent.invalid/ui".into()]);
            client
                .patch_clash_config(clash_patch)
                .await
                .expect("concurrent clash update should succeed");
            release_tx.send(()).expect("session apply should release");

            let error = task
                .await
                .expect("saga task should join")
                .expect_err("clash CAS conflict should fail the saga");
            assert!(error.to_string().contains("clash config version conflict"));
            let after = client
                .typed_config_snapshots()
                .await
                .expect("snapshots should reload");
            assert_eq!(after.application.version, before.application.version + 2);
            assert_eq!(after.session.version, before.session.version + 2);
            assert_eq!(after.clash.version, before.clash.version + 1);
            assert_eq!(
                after.application.state.theme_color,
                before.application.state.theme_color
            );
            assert!(after.session.state.window_state.is_empty());
            assert_eq!(
                *events.lock().unwrap(),
                vec![
                    "application:new",
                    "session:new",
                    "session:old",
                    "application:old"
                ]
            );
        });
    }

    #[test]
    fn compensation_conflict_returns_partial_commit_and_preserves_concurrent_update() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let events = Arc::new(StdMutex::new(Vec::new()));
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let window = Arc::new(RecordingWindowMirror {
            events: Arc::clone(&events),
            barrier: StdMutex::new(Some((entered_tx, Arc::new(StdMutex::new(release_rx))))),
        });
        let (client, bridge) = test_bridge_with_bridges(
            &dir,
            Arc::new(RecordingVergeMirror {
                events: Arc::clone(&events),
                barrier: StdMutex::new(None),
            }),
            window,
            Arc::new(NoopClashBridge),
        );

        tauri::async_runtime::block_on(async {
            events.lock().unwrap().clear();
            let before = client
                .typed_config_snapshots()
                .await
                .expect("snapshots should load");
            let saga = bridge.clone();
            let task = tauri::async_runtime::spawn(async move {
                saga.patch_verge_config(IVerge {
                    theme_color: Some("#abcdef".into()),
                    window_size_state: Some(crate::config::nyanpasu::WindowState {
                        width: 1440,
                        height: 900,
                        x: 1,
                        y: 2,
                        maximized: false,
                        fullscreen: false,
                    }),
                    web_ui_list: Some(vec!["https://example.invalid/ui".into()]),
                    ..IVerge::default()
                })
                .await
            });

            entered_rx.await.expect("session apply should start");
            let mut clash_patch = ClashConfig::new_empty_patch();
            clash_patch.web_ui_list = Some(vec!["https://concurrent.invalid/ui".into()]);
            client
                .patch_clash_config(clash_patch)
                .await
                .expect("concurrent clash update should succeed");
            let mut app_patch = NyanpasuAppConfig::new_empty_patch();
            app_patch.language = Some(I18nLanguage::Korean);
            client
                .patch_app_config(app_patch)
                .await
                .expect("concurrent application update should succeed");
            release_tx.send(()).expect("session apply should release");

            let error = task
                .await
                .expect("saga task should join")
                .expect_err("compensation conflict should return an error");
            let ClientError::PartialCommit(partial) = error else {
                panic!("expected partial commit, got {error:#}");
            };
            assert_eq!(
                partial.committed_domains,
                vec![
                    crate::client::LegacyVergeDomain::Application,
                    crate::client::LegacyVergeDomain::Session,
                ]
            );
            assert_eq!(
                partial.compensated_domains,
                vec![crate::client::LegacyVergeDomain::Session,]
            );
            assert!(matches!(
                partial.failed_compensations.as_slice(),
                [crate::client::CompensationFailure::Conflict {
                    domain: crate::client::LegacyVergeDomain::Application,
                    ..
                }]
            ));

            let after = client
                .typed_config_snapshots()
                .await
                .expect("snapshots should reload");
            assert_eq!(after.application.version, before.application.version + 2);
            assert_eq!(after.application.state.language, I18nLanguage::Korean);
            assert_eq!(after.session.version, before.session.version + 2);
            assert_eq!(
                after.session.state.window_state,
                before.session.state.window_state
            );
            assert_eq!(after.clash.version, before.clash.version + 1);
        });
    }
}
