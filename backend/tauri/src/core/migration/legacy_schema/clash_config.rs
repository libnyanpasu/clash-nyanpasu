use super::{
    clash::IClashTemp,
    verge::{self as legacy_app, IVerge},
};
use nyanpasu_config::clash::config::{
    ClashConfig,
    clash_strategy::{
        BreakConnectionStrategy, PortStrategy, PortStrategyKind, ProxyChangeBreakMode,
    },
};
use serde_yaml::{Mapping, Value};
use std::net::SocketAddr;

pub(super) fn clash_config_from_legacy(
    legacy_verge: &IVerge,
    legacy_clash: &Mapping,
) -> anyhow::Result<ClashConfig> {
    let mut legacy_clash = normalize_legacy_clash_overrides(legacy_clash);
    crate::core::migration::modules::clash_config::manage_override_fields(&mut legacy_clash)?;
    let mut next = ClashConfig {
        overrides: super::yaml_convert(&legacy_clash)?,
        ..ClashConfig::default()
    };

    if let Some(value) = legacy_verge.clash_control_channel {
        next.clash_control_channel = value;
    }
    if let Some(value) = legacy_verge.clash_ipc_disable_http_controller {
        next.clash_ipc_disable_http_controller = value;
    }
    if let Some(value) = legacy_verge.enable_tun_mode {
        next.enable_tun_mode = value;
    }
    if let Some(value) = &legacy_verge.web_ui_list {
        next.web_ui_list = value.clone();
    }
    if let Some(value) = legacy_verge.enable_clash_fields {
        next.enable_clash_fields = value;
    }
    if let Some(value) = &legacy_verge.tun_stack {
        next.tun_stack = super::yaml_convert(value)?;
    }

    let mixed_port = legacy_verge
        .verge_mixed_port
        .unwrap_or_else(|| IClashTemp::guard_mixed_port(&legacy_clash));
    // The legacy app bound a non-random mixed port as is, with no fallback.
    next.mixed_port = PortStrategy {
        kind: if legacy_verge.enable_random_port.unwrap_or(false) {
            PortStrategyKind::Random
        } else {
            PortStrategyKind::Fixed
        },
        start_port: mixed_port,
    };

    if let Some(controller) = external_controller_from_legacy_clash(&legacy_clash) {
        next.external_controller.host = controller.ip();
        next.external_controller.port.start_port = controller.port();
    }

    if let Some(strategy) = &legacy_verge.clash_strategy {
        next.external_controller.port.kind =
            super::yaml_convert(&strategy.external_controller_port_strategy)?;
    }

    next.break_connection = break_connection_from_legacy(legacy_verge);

    Ok(next)
}

fn normalize_legacy_clash_overrides(legacy_clash: &Mapping) -> Mapping {
    let mut merged = IClashTemp::template().0;
    for (key, value) in legacy_clash {
        if !matches!(value, Value::Null) {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

fn external_controller_from_legacy_clash(legacy_clash: &Mapping) -> Option<SocketAddr> {
    IClashTemp::guard_server_ctrl(legacy_clash).parse().ok()
}

fn break_connection_from_legacy(legacy: &IVerge) -> BreakConnectionStrategy {
    let on_proxy_change = legacy
        .break_when_proxy_change
        .as_ref()
        .map(proxy_change_from_legacy)
        .or_else(|| {
            #[allow(deprecated)]
            legacy.auto_close_connection.map(|enabled| {
                if enabled {
                    ProxyChangeBreakMode::All
                } else {
                    ProxyChangeBreakMode::Off
                }
            })
        })
        .unwrap_or_default();

    BreakConnectionStrategy {
        on_proxy_change,
        on_profile_change: legacy.break_when_profile_change.unwrap_or(true),
        on_mode_change: legacy.break_when_mode_change.unwrap_or(true),
    }
}

fn proxy_change_from_legacy(value: &legacy_app::BreakWhenProxyChange) -> ProxyChangeBreakMode {
    match value {
        legacy_app::BreakWhenProxyChange::None => ProxyChangeBreakMode::Off,
        legacy_app::BreakWhenProxyChange::Chain => ProxyChangeBreakMode::ProxyGroup,
        legacy_app::BreakWhenProxyChange::All => ProxyChangeBreakMode::All,
    }
}
