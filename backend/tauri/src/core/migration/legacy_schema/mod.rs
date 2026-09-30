//! The configuration as it was stored before the typed documents: the
//! `verge.yaml` shape, the clash guard overrides template, and the
//! conversions the typed config migration uses to split them into the
//! application config, session state and clash config. Nothing outside
//! the migration reads these files any more.

mod application;
mod clash;
mod clash_config;
mod session_state;
mod verge;

#[cfg(test)]
pub use self::verge::WindowState;
pub use self::{clash::IClashTemp, verge::IVerge};

use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, state::PersistentState,
};
use serde::{Serialize, de::DeserializeOwned};

pub fn typed_config_from_legacy_parts(
    legacy: &IVerge,
    legacy_clash: &serde_yaml::Mapping,
) -> anyhow::Result<(NyanpasuAppConfig, PersistentState, ClashConfig)> {
    Ok((
        application::application_from_legacy(legacy)?,
        session_state::persistent_state_from_legacy(legacy)?,
        clash_config::clash_config_from_legacy(legacy, legacy_clash)?,
    ))
}

fn yaml_convert<T, U>(value: T) -> anyhow::Result<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_yaml::to_value(value)?;
    Ok(serde_yaml::from_value(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the typed config migration puts a `verge.yaml` field.
    #[derive(Debug, Clone, Copy)]
    enum Target {
        Application,
        Session,
        Clash,
        /// Deliberately dropped; the string says why.
        #[allow(dead_code)]
        Discarded(&'static str),
    }

    /// Every `IVerge` field with two YAML values that must convert
    /// differently. The destructuring pattern makes a field missing from the
    /// list a compile error.
    macro_rules! legacy_fields {
        ($($field:ident: $target:expr, $a:literal, $b:literal;)*) => {{
            #[allow(deprecated)]
            let IVerge { $($field: _,)* } = IVerge::default();
            vec![$((stringify!($field), $target, $a, $b)),*]
        }};
    }

    fn legacy_fields() -> Vec<(&'static str, Target, &'static str, &'static str)> {
        use Target::*;
        legacy_fields! {
            app_singleton_port: Application, "1", "2";
            app_log_level: Application, "error", "warn";
            language: Application, "en", "ru";
            theme_mode: Application, "light", "dark";
            traffic_graph: Application, "true", "false";
            enable_memory_usage: Application, "true", "false";
            lighten_animation_effects: Application, "true", "false";
            enable_tun_mode: Clash, "true", "false";
            enable_service_mode: Application, "true", "false";
            enable_auto_launch: Application, "true", "false";
            enable_silent_start: Application, "true", "false";
            enable_system_proxy: Application, "true", "false";
            enable_proxy_guard: Application, "true", "false";
            system_proxy_bypass: Application, "a", "b";
            proxy_guard_interval: Application, "10", "20";
            theme_color: Application, "'#111111'", "'#222222'";
            web_ui_list: Clash, "[a]", "[b]";
            clash_core: Application, "mihomo", "clash-rs";
            clash_control_channel: Clash, "prefer_ipc", "http_only";
            clash_ipc_disable_http_controller: Clash, "true", "false";
            hotkeys: Application, "[a]", "[b]";
            auto_close_connection: Clash, "true", "false";
            break_when_proxy_change: Clash, "none", "all";
            break_when_profile_change: Clash, "true", "false";
            break_when_mode_change: Clash, "true", "false";
            default_latency_test: Application, "a", "b";
            enable_clash_fields: Clash, "true", "false";
            enable_builtin_enhanced: Application, "true", "false";
            proxy_layout_column: Application, "2", "3";
            auto_log_clean: Discarded("superseded by max_log_files"), "1", "2";
            max_log_files: Application, "3", "5";
            window_size_position: Session, "[1, 2, 3, 4]", "[5, 6, 7, 8]";
            window_size_state: Session,
                "{width: 1, height: 2, x: 3, y: 4, maximized: false, fullscreen: false}",
                "{width: 5, height: 2, x: 3, y: 4, maximized: false, fullscreen: false}";
            enable_random_port: Clash, "true", "false";
            verge_mixed_port: Clash, "7001", "7002";
            enable_auto_check_update: Application, "true", "false";
            clash_strategy: Clash,
                "{external_controller_port_strategy: fixed}",
                "{external_controller_port_strategy: random}";
            clash_tray_selector: Application, "hidden", "submenu";
            always_on_top: Application, "true", "false";
            tun_stack: Clash, "system", "mixed";
            network_statistic_widget: Application, "large", "small";
            pac_url: Application, "http://a.test/pac", "http://b.test/pac";
            enable_tray_text: Application, "true", "false";
            window_type: Application, "main", "~";
            tray_menu_mode: Application, "native", "webview";
            tray_menu_close_behavior: Application, "hide", "close";
        }
    }

    /// The application, session and clash documents one field converts to.
    fn convert(field: &str, value: &str, clash: &serde_yaml::Mapping) -> [serde_yaml::Value; 3] {
        let legacy: IVerge = serde_yaml::from_str(&format!("{field}: {value}"))
            .unwrap_or_else(|error| panic!("{field}: {value} is not a legacy value: {error}"));
        let (application, session, clash) = typed_config_from_legacy_parts(&legacy, clash)
            .unwrap_or_else(|error| panic!("{field}: {value} does not convert: {error:#}"));
        [
            serde_yaml::to_value(application).unwrap(),
            serde_yaml::to_value(session).unwrap(),
            serde_yaml::to_value(clash).unwrap(),
        ]
    }

    #[test]
    fn every_legacy_field_is_converted_or_explicitly_discarded() {
        // One template, so the generated secret is the same in every run.
        let clash = IClashTemp::template().0;
        for (field, target, a, b) in legacy_fields() {
            let [application_a, session_a, clash_a] = convert(field, a, &clash);
            let [application_b, session_b, clash_b] = convert(field, b, &clash);
            let changed = [
                application_a != application_b,
                session_a != session_b,
                clash_a != clash_b,
            ];
            let expected = match target {
                Target::Application => [true, false, false],
                Target::Session => [false, true, false],
                Target::Clash => [false, false, true],
                Target::Discarded(_) => [false, false, false],
            };
            assert_eq!(
                changed, expected,
                "{field} ({target:?}) changed [application, session, clash] as {changed:?} \
                 between {a} and {b}"
            );
        }
    }
}
