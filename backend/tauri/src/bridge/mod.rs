pub mod clash;
pub mod mapping;
pub mod verge;
pub mod window;

use crate::{config::IVerge, state::TypedConfigPatchPlan};
use nyanpasu_config::{
    application::{NyanpasuAppConfig, NyanpasuAppConfigPatch},
    clash::config::{ClashConfig, ClashConfigPatch},
    state::{PersistentState, PersistentStatePatch},
};
use serde::{Serialize, de::DeserializeOwned};
use struct_patch::Patch;

pub(crate) fn legacy_iverge_from_typed(
    mut base: IVerge,
    app: &NyanpasuAppConfig,
    session: &PersistentState,
    clash: &ClashConfig,
) -> anyhow::Result<IVerge> {
    verge::apply_app_config_to_legacy_verge(&mut base, app)?;
    window::apply_session_state_to_legacy_verge(&mut base, session)?;
    clash::apply_clash_config_to_legacy_verge(&mut base, clash)?;
    Ok(base)
}

pub(crate) fn typed_config_from_legacy_parts(
    legacy: &IVerge,
    legacy_clash: &serde_yaml::Mapping,
) -> anyhow::Result<(NyanpasuAppConfig, PersistentState, ClashConfig)> {
    Ok((
        verge::application_from_legacy(legacy)?,
        window::persistent_state_from_legacy(legacy)?,
        clash::clash_config_from_legacy(legacy, legacy_clash)?,
    ))
}

pub(crate) fn typed_patches_from_legacy_patch(
    mut base: IVerge,
    patch: &IVerge,
    legacy_clash: &serde_yaml::Mapping,
) -> anyhow::Result<TypedConfigPatchPlan> {
    base.patch_config(patch.clone());
    let (app, session, clash) = typed_config_from_legacy_parts(&base, legacy_clash)?;

    Ok(TypedConfigPatchPlan {
        application: application_patch_from_legacy_patch(patch, app),
        session_state: session_patch_from_legacy_patch(patch, session),
        clash_config: clash_patch_from_legacy_patch(patch, clash),
    })
}

fn application_patch_from_legacy_patch(
    patch: &IVerge,
    next: NyanpasuAppConfig,
) -> Option<NyanpasuAppConfigPatch> {
    let mut app = NyanpasuAppConfig::new_empty_patch();
    let mut touched = false;

    macro_rules! set_if_some {
        ($legacy:ident, $target:ident) => {
            if patch.$legacy.is_some() {
                app.$target = Some(next.$target);
                touched = true;
            }
        };
    }

    set_if_some!(app_singleton_port, app_singleton_port);
    set_if_some!(app_log_level, app_log_level);
    set_if_some!(language, language);
    set_if_some!(theme_mode, theme_mode);
    set_if_some!(traffic_graph, traffic_graph);
    set_if_some!(enable_memory_usage, enable_memory_usage);
    set_if_some!(lighten_animation_effects, lighten_animation_effects);
    set_if_some!(enable_service_mode, enable_service_mode);
    set_if_some!(enable_auto_launch, enable_auto_launch);
    set_if_some!(enable_silent_start, enable_silent_start);
    set_if_some!(enable_system_proxy, enable_system_proxy);
    set_if_some!(enable_proxy_guard, enable_proxy_guard);
    set_if_some!(system_proxy_bypass, system_proxy_bypass);
    set_if_some!(proxy_guard_interval, proxy_guard_interval);
    set_if_some!(theme_color, theme_color);
    set_if_some!(hotkeys, hotkeys);
    set_if_some!(default_latency_test, default_latency_test);
    set_if_some!(enable_builtin_enhanced, enable_builtin_enhanced);
    set_if_some!(proxy_layout_column, proxy_layout_column);
    set_if_some!(max_log_files, max_log_files);
    set_if_some!(enable_auto_check_update, enable_auto_check_update);
    set_if_some!(always_on_top, always_on_top);
    set_if_some!(network_statistic_widget, network_statistic_widget);
    set_if_some!(enable_tray_text, enable_tray_text);
    set_if_some!(tray_menu_mode, tray_menu_mode);
    set_if_some!(tray_menu_close_behavior, tray_menu_close_behavior);

    if patch.clash_core.is_some() {
        app.core = Some(next.core);
        touched = true;
    }
    if patch.clash_tray_selector.is_some() {
        app.tray_selector_mode = Some(next.tray_selector_mode);
        touched = true;
    }
    if patch.pac_url.is_some() {
        app.pac_url = Some(next.pac_url);
        touched = true;
    }
    if patch.window_type.is_some() {
        app.use_legacy_ui = Some(next.use_legacy_ui);
        touched = true;
    }

    touched.then_some(app)
}

fn session_patch_from_legacy_patch(
    patch: &IVerge,
    next: PersistentState,
) -> Option<PersistentStatePatch> {
    #[allow(deprecated)]
    let touched = patch.window_size_state.is_some() || patch.window_size_position.is_some();

    if !touched {
        return None;
    }

    let mut session = PersistentState::new_empty_patch();
    session.window_state = Some(next.window_state);
    Some(session)
}

fn clash_patch_from_legacy_patch(patch: &IVerge, next: ClashConfig) -> Option<ClashConfigPatch> {
    let mut clash = ClashConfig::new_empty_patch();
    let mut touched = false;

    if patch.enable_tun_mode.is_some() {
        clash.enable_tun_mode = Some(next.enable_tun_mode);
        touched = true;
    }
    if patch.web_ui_list.is_some() {
        clash.web_ui_list = Some(next.web_ui_list);
        touched = true;
    }
    if patch.enable_clash_fields.is_some() {
        clash.enable_clash_fields = Some(next.enable_clash_fields);
        touched = true;
    }
    if patch.tun_stack.is_some() {
        clash.tun_stack = Some(next.tun_stack);
        touched = true;
    }
    if patch.enable_random_port.is_some() || patch.verge_mixed_port.is_some() {
        clash.mixed_port = Some(next.mixed_port);
        touched = true;
    }
    if patch.clash_strategy.is_some() {
        clash.external_controller = Some(next.external_controller);
        touched = true;
    }

    #[allow(deprecated)]
    let break_connection_touched = patch.auto_close_connection.is_some()
        || patch.break_when_proxy_change.is_some()
        || patch.break_when_profile_change.is_some()
        || patch.break_when_mode_change.is_some();

    if break_connection_touched {
        clash.break_connection = Some(next.break_connection);
        touched = true;
    }

    touched.then_some(clash)
}

pub(super) fn yaml_convert<T, U>(value: T) -> anyhow::Result<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_yaml::to_value(value)?;
    Ok(serde_yaml::from_value(value)?)
}
