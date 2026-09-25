use std::sync::Arc;

use crate::{
    client::{NyanpasuClient, Result as ClientResult, runtime},
    config::{Config, Draft, IVerge, nyanpasu as legacy_app},
    state::mirror::{PreparedLegacyMirror, VergeLegacyBridge},
};
use nyanpasu_config::application::{
    NetworkStatisticWidgetConfig as AppNetworkStatisticWidgetConfig, NyanpasuAppConfig,
};
use nyanpasu_egui::widget::StatisticWidgetVariant;

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
    /// Serializes legacy saves: composite typed fields (mixed port, break
    /// connection, external controller) are rebuilt from the snapshot base, so
    /// a save must not read its base while another save is still committing.
    save_lock: tokio::sync::Mutex<()>,
}

pub(crate) trait LegacyVergeStore: Send + Sync {
    fn snapshot(&self) -> anyhow::Result<IVerge>;
    fn prepare_application(
        &self,
        snap: &NyanpasuAppConfig,
    ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>>;
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
}

impl LegacyVergeBridge {
    pub(crate) fn with_store(legacy_store: Arc<dyn LegacyVergeStore>) -> Self {
        Self {
            managed: None,
            legacy_store,
        }
    }

    pub(crate) fn new(client: NyanpasuClient, legacy_store: Arc<dyn LegacyVergeStore>) -> Self {
        Self {
            managed: Some(Arc::new(LegacyVergeBridgeInner {
                client,
                save_lock: tokio::sync::Mutex::new(()),
            })),
            legacy_store,
        }
    }

    pub async fn get_verge_config(&self) -> ClientResult<IVerge> {
        self.get_verge_config_unlocked().await
    }

    pub async fn patch_verge_config(
        &self,
        payload: IVerge,
    ) -> ClientResult<runtime::MutationOutcome<()>> {
        Self::validate_patch(&payload)?;
        let managed = self.managed()?;
        let client = &managed.client;
        let _guard = managed.save_lock.lock().await;
        let snapshots = client.typed_config_snapshots();
        let base = super::legacy_iverge_from_typed(
            self.legacy_store.snapshot()?,
            &snapshots.application.state,
            &snapshots.session.state,
            &snapshots.clash.state,
        )?;
        let clash = &snapshots.clash.state;
        let plan = Self::typed_patch_plan(base, &payload, &super::yaml_convert(&clash.overrides)?)?;
        validate_single_domain(&plan)?;
        if let Some(patch) = plan.application {
            client.patch_app_config(patch).await
        } else if let Some(patch) = plan.clash_config {
            client.patch_clash_config(patch).await
        } else if let Some(patch) = plan.session_state {
            let geometry = patch
                .window_state
                .and_then(|mut windows| {
                    windows.remove(&nyanpasu_config::state::window::WindowLabel("main".into()))
                })
                .ok_or_else(|| anyhow::anyhow!("window patch must contain main window geometry"))?;
            client.save_main_window_geometry(geometry).await
        } else {
            Ok(runtime::MutationOutcome::from_parts((), Vec::new()))
        }
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

    pub(crate) fn typed_patch_plan(
        base: IVerge,
        patch: &IVerge,
        legacy_clash: &serde_yaml::Mapping,
    ) -> anyhow::Result<crate::state::TypedConfigPatchPlan> {
        super::typed_patches_from_legacy_patch(base, patch, legacy_clash)
    }

    pub(crate) fn validate_patch(patch: &IVerge) -> anyhow::Result<()> {
        validate_verge_patch(patch)
    }
}

fn validate_single_domain(plan: &crate::state::TypedConfigPatchPlan) -> anyhow::Result<()> {
    let domains = usize::from(plan.application.is_some())
        + usize::from(plan.clash_config.is_some())
        + usize::from(plan.session_state.is_some());
    anyhow::ensure!(
        domains <= 1,
        "configuration requests must target one domain; submit separate actions for Application, ClashConfig and Session"
    );
    Ok(())
}

fn validate_verge_patch(verge: &IVerge) -> anyhow::Result<()> {
    let payload = serde_json::to_value(verge)?;
    for (field, value) in payload.as_object().expect("IVerge is an object") {
        if !value.is_null()
            && !matches!(
                field.as_str(),
                "always_on_top"
                    | "app_log_level"
                    | "app_singleton_port"
                    | "auto_close_connection"
                    | "break_when_mode_change"
                    | "break_when_profile_change"
                    | "break_when_proxy_change"
                    | "clash_control_channel"
                    | "clash_core"
                    | "clash_ipc_disable_http_controller"
                    | "clash_strategy"
                    | "clash_tray_selector"
                    | "default_latency_test"
                    | "enable_auto_check_update"
                    | "enable_auto_launch"
                    | "enable_builtin_enhanced"
                    | "enable_clash_fields"
                    | "enable_memory_usage"
                    | "enable_proxy_guard"
                    | "enable_random_port"
                    | "enable_service_mode"
                    | "enable_silent_start"
                    | "enable_system_proxy"
                    | "enable_tray_text"
                    | "enable_tun_mode"
                    | "hotkeys"
                    | "language"
                    | "lighten_animation_effects"
                    | "max_log_files"
                    | "network_statistic_widget"
                    | "pac_url"
                    | "proxy_guard_interval"
                    | "proxy_layout_column"
                    | "system_proxy_bypass"
                    | "theme_color"
                    | "theme_mode"
                    | "traffic_graph"
                    | "tray_menu_close_behavior"
                    | "tray_menu_mode"
                    | "tun_stack"
                    | "verge_mixed_port"
                    | "web_ui_list"
                    | "window_size_position"
                    | "window_size_state"
                    | "window_type"
            )
        {
            anyhow::bail!("legacy field {field} has no typed configuration owner");
        }
    }
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
    target.clash_core = projected.clash_core;
    target.hotkeys = projected.hotkeys.clone();
    target.default_latency_test = projected.default_latency_test.clone();
    target.enable_builtin_enhanced = projected.enable_builtin_enhanced;
    target.proxy_layout_column = projected.proxy_layout_column;
    target.max_log_files = projected.max_log_files;
    target.enable_auto_check_update = projected.enable_auto_check_update;
    target.clash_tray_selector = projected.clash_tray_selector;
    target.always_on_top = projected.always_on_top;
    target.network_statistic_widget = projected.network_statistic_widget;
    target.pac_url = projected.pac_url.clone();
    target.enable_tray_text = projected.enable_tray_text;
    target.window_type = projected.window_type;
    target.tray_menu_mode = projected.tray_menu_mode;
    target.tray_menu_close_behavior = projected.tray_menu_close_behavior;
}

pub(crate) fn apply_app_config_to_legacy_verge(
    draft: &mut IVerge,
    snap: &NyanpasuAppConfig,
) -> anyhow::Result<()> {
    draft.app_singleton_port = Some(snap.app_singleton_port);
    draft.app_log_level = Some(super::yaml_convert(&snap.app_log_level)?);
    draft.language = Some(super::yaml_convert(snap.language)?);
    draft.theme_mode = Some(super::yaml_convert(snap.theme_mode)?);
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
    draft.clash_core = Some(super::yaml_convert(snap.core)?);
    draft.hotkeys = Some(snap.hotkeys.clone());
    draft.default_latency_test = Some(snap.default_latency_test.clone());
    draft.enable_builtin_enhanced = Some(snap.enable_builtin_enhanced);
    draft.proxy_layout_column = Some(snap.proxy_layout_column);
    draft.max_log_files = Some(snap.max_log_files);
    draft.enable_auto_check_update = Some(snap.enable_auto_check_update);
    draft.clash_tray_selector = Some(super::yaml_convert(snap.tray_selector_mode)?);
    draft.always_on_top = Some(snap.always_on_top);
    draft.network_statistic_widget = Some(network_widget_to_legacy(snap.network_statistic_widget));
    draft.pac_url = snap.pac_url.as_ref().map(ToString::to_string);
    draft.enable_tray_text = Some(snap.enable_tray_text);
    draft.window_type = snap.use_legacy_ui.then_some(legacy_app::WindowType::Main);
    draft.tray_menu_mode = Some(super::yaml_convert(snap.tray_menu_mode)?);
    draft.tray_menu_close_behavior = Some(super::yaml_convert(snap.tray_menu_close_behavior)?);
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
        bridge::LEGACY_CONFIG_TEST_LOCK as INTERLEAVING_TEST_LOCK,
        client::{ClientSetupArgs, LegacyBridgeSet, NoopUiEventSink},
        config::IClashTemp,
        state::mirror::{ClashLegacyBridge, NoopPreparedLegacyMirror, WindowLegacyBridge},
    };
    use camino::Utf8PathBuf;
    use nyanpasu_config::{clash::config::ClashConfig, state::PersistentState};
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

    struct NoopClashBridge;

    impl ClashLegacyBridge for NoopClashBridge {
        fn prepare(&self, _snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
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
        let runtime_paths = crate::client::RuntimePaths::from_resolver(&paths).unwrap();
        let (core_v2, service) = crate::client::tests::test_v2_clients();
        let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
            bundle_metadata: crate::bundle::BundleMetadata {
                is_portable: false,
                is_fixed_webview: false,
                release_channel: crate::bundle::Channel::Stable,
            },
            logging: crate::client::logs::test_setup(paths.app_logs_dir()),
            paths,
            runtime_paths,
            bridges: LegacyBridgeSet {
                verge,
                window,
                clash,
            },
            ui_sink: Arc::new(NoopUiEventSink),
            core_v2,
            service,
            system_dns: Arc::new(crate::client::NoopSystemDnsCache),
            binary_installer: Arc::new(crate::client::core_lifecycle::adapters::FsBinaryInstaller),
            effects: Arc::new(crate::client::effects::ports::NoopApplicationEffects),
            window: Arc::new(crate::client::hotkey::ports::MockWindowControl::new()),
            accelerators: Arc::new(crate::client::hotkey::adapters::PlatformAcceleratorValidator),
        })
        .expect("client should construct with typed config actors");
        let bridge = LegacyVergeBridge::new(client.clone(), legacy_store);
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
    fn legacy_patch_with_invalid_hotkeys_is_rejected_before_commit() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().expect("tempdir should be created");
        let (client, bridge) = test_bridge(&dir);

        tauri::async_runtime::block_on(async {
            let error = bridge
                .patch_verge_config(IVerge {
                    theme_color: Some("#334455".into()),
                    hotkeys: Some(vec!["toggle_tun_mode,Control+DefinitelyNotAKey".to_owned()]),
                    ..IVerge::default()
                })
                .await
                .expect_err("an accelerator the platform cannot parse must not be persisted");
            assert!(
                error.to_string().contains("DefinitelyNotAKey"),
                "unexpected error: {error}"
            );

            let app = client
                .get_app_config()
                .await
                .expect("typed config should read back");
            assert!(
                app.hotkeys.is_empty(),
                "nothing may be written when validation fails: {:?}",
                app.hotkeys
            );
            assert_eq!(
                app.theme_color.to_string(),
                NyanpasuAppConfig::default().theme_color.to_string(),
                "the rest of the patch must not be committed either"
            );
            let projected = bridge
                .get_verge_config()
                .await
                .expect("legacy projection should read back")
                .hotkeys;
            assert!(
                projected.is_none_or(|hotkeys| hotkeys.is_empty()),
                "the legacy projection must not hold the rejected bindings"
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
    fn mixed_domains_are_rejected_before_any_commit() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().unwrap();
        let (client, bridge) = test_bridge(&dir);
        tauri::async_runtime::block_on(async {
            let before = client.typed_config_snapshots();
            for patch in [
                IVerge {
                    theme_color: Some("#112233".into()),
                    enable_tun_mode: Some(true),
                    ..IVerge::default()
                },
                IVerge {
                    theme_color: Some("#112233".into()),
                    window_size_state: Some(Default::default()),
                    ..IVerge::default()
                },
            ] {
                assert!(
                    bridge
                        .patch_verge_config(patch)
                        .await
                        .unwrap_err()
                        .to_string()
                        .contains("one domain")
                );
                let after = client.typed_config_snapshots();
                assert_eq!(before.application.version, after.application.version);
                assert_eq!(before.session.version, after.session.version);
                assert_eq!(before.clash.version, after.clash.version);
            }
            assert!(!dir.path().join("nyanpasu-config.yaml").exists());
        });
    }
    #[test]
    fn overlapping_saves_of_different_fields_both_persist() {
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().unwrap();
        let (client, bridge) = test_bridge(&dir);
        tauri::async_runtime::block_on(async {
            let (theme, layout) = tokio::join!(
                bridge.patch_verge_config(IVerge {
                    theme_color: Some("#112233".into()),
                    ..Default::default()
                }),
                bridge.patch_verge_config(IVerge {
                    proxy_layout_column: Some(3),
                    ..Default::default()
                }),
            );
            theme.unwrap();
            layout.unwrap();
            let app = client.get_app_config().await.unwrap();
            assert_eq!(app.theme_color.to_string(), "#112233");
            assert_eq!(app.proxy_layout_column, 3);

            let (proxy, profile) = tokio::join!(
                bridge.patch_verge_config(IVerge {
                    break_when_proxy_change: Some(legacy_app::BreakWhenProxyChange::None),
                    ..Default::default()
                }),
                bridge.patch_verge_config(IVerge {
                    break_when_profile_change: Some(false),
                    ..Default::default()
                }),
            );
            proxy.unwrap();
            profile.unwrap();
            let clash = client.get_clash_config().await.unwrap();
            assert_eq!(
                clash.break_connection.on_proxy_change,
                nyanpasu_config::clash::config::clash_strategy::ProxyChangeBreakMode::Off
            );
            assert!(!clash.break_connection.on_profile_change);
        });
    }

    #[test]
    fn legacy_window_save_preserves_other_windows_and_reports_session_version() {
        use nyanpasu_config::state::window::{WindowLabel, WindowState};
        let _serial = INTERLEAVING_TEST_LOCK.lock();
        let dir = tempdir().unwrap();
        let (client, bridge) = test_bridge(&dir);
        tauri::async_runtime::block_on(async {
            let other = WindowState {
                width: 471,
                ..Default::default()
            };
            let mut state = PersistentState::default();
            state
                .window_state
                .insert(WindowLabel("other".into()), other.clone());
            client.replace_session_state(state).await.unwrap();
            let before = client.typed_config_snapshots().session.version;
            let outcome = bridge
                .patch_verge_config(IVerge {
                    window_size_state: Some(legacy_app::WindowState {
                        width: 913,
                        ..Default::default()
                    }),
                    ..Default::default()
                })
                .await
                .unwrap();
            let after = client.get_session_state().await.unwrap();
            assert_eq!(
                after.window_state[&WindowLabel("other".into())].width,
                other.width
            );
            assert_eq!(after.window_state[&WindowLabel("main".into())].width, 913);
            let wire = serde_json::to_value(outcome).unwrap();
            assert_eq!(wire["commits"][0]["domain"], "session");
            assert_eq!(wire["commits"][0]["source_version"], before + 1);
        });
    }
}
