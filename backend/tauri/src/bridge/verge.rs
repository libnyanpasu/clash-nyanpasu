use std::{future::Future, sync::Arc};

use anyhow::Context as _;
use camino::{Utf8Path, Utf8PathBuf};

use crate::{
    client::{NyanpasuClient, Result as ClientResult},
    config::{Config, IVerge, nyanpasu as legacy_app},
    state::mirror::VergeLegacyBridge,
    utils::help,
};
use nyanpasu_config::application::{
    NetworkStatisticWidgetConfig as AppNetworkStatisticWidgetConfig, NyanpasuAppConfig,
};
use nyanpasu_egui::widget::StatisticWidgetVariant;
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub struct LegacyVergeBridge {
    managed: Option<Arc<LegacyVergeBridgeInner>>,
}

struct LegacyVergeBridgeInner {
    client: NyanpasuClient,
    legacy_verge_path: Utf8PathBuf,
    verge_update_lock: Mutex<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegacyVergePatchRoute {
    PureConfig,
    LegacySideEffects,
}

impl LegacyVergeBridge {
    pub(crate) fn new(client: NyanpasuClient, legacy_verge_path: Utf8PathBuf) -> Self {
        Self {
            managed: Some(Arc::new(LegacyVergeBridgeInner {
                client,
                legacy_verge_path,
                verge_update_lock: Mutex::new(()),
            })),
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
                let plan = Self::typed_patch_plan(base, &payload)?;
                self.apply_typed_config_patch_plan(plan).await?;
                let committed = self.get_verge_config_unlocked().await?;
                Self::commit_full_replacement(&managed.legacy_verge_path, committed)?;
            }
            LegacyVergePatchRoute::LegacySideEffects => {
                self.run_legacy_verge_mutation(|| crate::feat::patch_verge(payload))
                    .await?;
            }
        }
        Ok(())
    }

    pub async fn replace_verge_config(&self, state: IVerge) -> ClientResult<()> {
        let managed = self.managed()?;
        let _guard = managed.verge_update_lock.lock().await;
        self.replace_typed_config_from_legacy(state.clone()).await?;
        Self::commit_full_replacement(&managed.legacy_verge_path, state)?;
        Ok(())
    }

    pub async fn run_legacy_verge_mutation<F, Fut>(&self, mutate: F) -> ClientResult<()>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = anyhow::Result<()>>,
    {
        let managed = self.managed()?;
        let _guard = managed.verge_update_lock.lock().await;
        mutate().await?;
        // TODO(actor-migration): compatibility bridge for legacy side-effect writers.
        // Reason: feat::patch_verge still commits side-effect fields through Config::verge().
        // Remove when: side-effect fields are handled by typed actor post-commit effects.
        // Bind the clone to a local so the `Config::verge()` guard is dropped before the
        // await (a held parking_lot guard would make this future !Send).
        let committed = Self::committed_snapshot();
        self.replace_typed_config_from_legacy(committed).await?;
        Self::save_committed_snapshot(&managed.legacy_verge_path)?;
        Ok(())
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

        Ok(Self::compose_from_typed(&app, &session, &clash)?)
    }

    async fn apply_typed_config_patch_plan(
        &self,
        plan: super::TypedConfigPatchPlan,
    ) -> ClientResult<()> {
        let managed = self.managed()?;
        if let Some(patch) = plan.application {
            managed.client.patch_app_config(patch).await?;
        }
        if let Some(patch) = plan.session_state {
            managed.client.patch_session_state(patch).await?;
        }
        if let Some(patch) = plan.clash_config {
            managed.client.patch_clash_config(patch).await?;
        }

        Ok(())
    }

    async fn replace_typed_config_from_legacy(&self, legacy: IVerge) -> ClientResult<()> {
        let managed = self.managed()?;
        let (app, session, clash) = Self::typed_replacement(&legacy)?;

        managed
            .client
            .replace_app_config(app)
            .await
            .context("failed to reseed application config from legacy verge state")?;
        managed
            .client
            .replace_session_state(session)
            .await
            .context("failed to reseed session state from legacy verge state")?;
        managed
            .client
            .replace_clash_config(clash)
            .await
            .context("failed to reseed clash config from legacy verge state")?;
        Ok(())
    }

    pub(crate) fn compose_from_typed(
        app: &NyanpasuAppConfig,
        session: &nyanpasu_config::state::PersistentState,
        clash: &nyanpasu_config::clash::config::ClashConfig,
    ) -> anyhow::Result<IVerge> {
        super::legacy_iverge_from_typed(
            super::legacy_iverge_base_for_typed_read(),
            app,
            session,
            clash,
        )
    }

    pub(crate) fn typed_replacement(
        legacy: &IVerge,
    ) -> anyhow::Result<(
        NyanpasuAppConfig,
        nyanpasu_config::state::PersistentState,
        nyanpasu_config::clash::config::ClashConfig,
    )> {
        super::typed_config_from_legacy(legacy)
    }

    pub(crate) fn typed_patch_plan(
        base: IVerge,
        patch: &IVerge,
    ) -> anyhow::Result<super::TypedConfigPatchPlan> {
        super::typed_patches_from_legacy_patch(base, patch)
    }

    pub(crate) fn route_patch(patch: &IVerge) -> LegacyVergePatchRoute {
        route_verge_patch(patch)
    }

    pub(crate) fn validate_patch(patch: &IVerge) -> anyhow::Result<()> {
        validate_verge_patch(patch)
    }

    pub(crate) fn commit_full_replacement(path: &Utf8Path, state: IVerge) -> anyhow::Result<()> {
        // TODO(actor-migration): compatibility bridge for full legacy IVerge replacement.
        // Reason: get_verge_config still composes legacy-only fields from Config::verge().
        // Remove when: legacy-only IVerge fields are deleted or moved to typed owners.
        *Config::verge().draft() = state;
        Config::verge().apply();
        Self::save_committed_snapshot(path)
    }

    pub(crate) fn committed_snapshot() -> IVerge {
        Config::verge().data().clone()
    }

    pub(crate) fn save_committed_snapshot(path: &Utf8Path) -> anyhow::Result<()> {
        help::save_yaml(
            path.as_std_path(),
            &Config::verge().data().clone(),
            Some("# Clash Nyanpasu Config"),
        )
    }
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

impl VergeLegacyBridge for LegacyVergeBridge {
    fn mirror(&self, snap: &NyanpasuAppConfig) -> anyhow::Result<()> {
        // TODO(actor-migration): compatibility bridge for legacy Config::verge().
        // Reason: legacy readers still consume Config::verge() while typed actors are introduced.
        // Remove when get_verge_config and direct Config::verge() readers use typed facade data.
        let verge = Config::verge();
        let mut draft = verge.draft();
        apply_app_config_to_legacy_verge(&mut draft, snap)?;
        drop(draft);
        verge.apply();
        Ok(())
    }

    fn snapshot_legacy(&self) -> anyhow::Result<NyanpasuAppConfig> {
        let legacy = Config::verge().data().clone();
        application_from_legacy(&legacy)
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
    draft.system_proxy_bypass = Some(snap.system_proxy_bypass.clone());
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
        client::{ClientSetupArgs, LegacyBridgeSet, NyanpasuClient},
        config::{
            IClashTemp,
            nyanpasu::{
                LoggingLevel, NetworkStatisticWidgetConfig, ProxiesSelectorMode, TrayMenuMode,
            },
        },
        state::mirror::{ClashLegacyBridge, WindowLegacyBridge},
    };
    use nyanpasu_config::{
        clash::config::ClashConfig,
        state::{
            PersistentState,
            window::{WindowLabel, WindowState},
        },
    };
    use std::{collections::BTreeMap, sync::Arc};
    use struct_patch::Patch;
    use tempfile::{TempDir, tempdir};

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

    fn test_bridge(dir: &TempDir) -> (NyanpasuClient, LegacyVergeBridge) {
        // Keep tests hermetic: the legacy global otherwise reads host config/registry state.
        let clash = Config::clash();
        *clash.draft() = IClashTemp::template();
        clash.apply();

        let paths = crate::utils::path::PathResolver::with_base_dirs(
            dir.path().into(),
            dir.path().join("data"),
        );
        let legacy_verge_path = temp_config_path(dir, "nyanpasu-config.yaml");
        let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
            paths,
            bridges: LegacyBridgeSet {
                verge: Arc::new(LegacyVergeBridge::default()),
                window: Arc::new(NoopWindowBridge),
                clash: Arc::new(NoopClashBridge),
            },
        })
        .expect("client should construct with typed config actors");
        let bridge = LegacyVergeBridge::new(client.clone(), legacy_verge_path);
        (client, bridge)
    }

    #[test]
    fn get_verge_config_composes_typed_actor_snapshots() {
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
            assert_eq!(verge.enable_tun_mode, Some(true));
            assert_eq!(
                verge.window_size_state.as_ref().map(|state| state.width),
                Some(window_state.width)
            );
        });
    }

    #[test]
    fn legacy_patch_then_get_verge_config_preserves_contract() {
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
    fn pure_verge_patch_preserves_session_state_fields() {
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
}
