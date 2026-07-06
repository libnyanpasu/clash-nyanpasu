use crate::{
    config::{Config, IClashTemp, IVerge, nyanpasu as legacy_app},
    state::mirror::ClashLegacyBridge,
};
use nyanpasu_config::clash::config::{
    ClashConfig,
    clash_strategy::{
        BreakConnectionStrategy, PortStrategy, PortStrategyKind, ProxyChangeBreakMode,
    },
};
use serde_yaml::{Mapping, Value};
use std::net::SocketAddr;

pub struct LegacyClashBridge;

impl ClashLegacyBridge for LegacyClashBridge {
    fn mirror(&self, snap: &ClashConfig) -> anyhow::Result<()> {
        // TODO(actor-migration): compatibility bridge for legacy Config::clash()/Config::verge().
        // Reason: Clash config readers still consume legacy globals while typed actors are wired.
        // Remove when Clash config reads/writes use ClashConfigClient and runtime DTO conversion.
        mirror_clash_overrides(snap)?;
        mirror_clash_verge_fields(snap)?;
        Ok(())
    }

    fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
        let legacy_verge = Config::verge().data().clone();
        let legacy_clash = Config::clash().data().clone();
        clash_config_from_legacy(&legacy_verge, &legacy_clash.0)
    }
}

pub(crate) fn clash_config_from_legacy(
    legacy_verge: &IVerge,
    legacy_clash: &Mapping,
) -> anyhow::Result<ClashConfig> {
    let legacy_clash = normalize_legacy_clash_overrides(legacy_clash);
    let mut next = ClashConfig {
        overrides: super::yaml_convert(&legacy_clash)?,
        ..ClashConfig::default()
    };

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
    next.mixed_port = if legacy_verge.enable_random_port.unwrap_or(false) {
        PortStrategy {
            kind: PortStrategyKind::Random,
            start_port: mixed_port,
        }
    } else {
        PortStrategy::new_allow_fallback(mixed_port)
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

fn mirror_clash_overrides(snap: &ClashConfig) -> anyhow::Result<()> {
    let clash = Config::clash();
    let mut draft = clash.draft();
    let mut mapping: Mapping = super::yaml_convert(&snap.overrides)?;

    mapping.insert("mixed-port".into(), snap.mixed_port.start_port.into());
    mapping.insert(
        "external-controller".into(),
        format!(
            "{}:{}",
            snap.external_controller.host, snap.external_controller.port.start_port
        )
        .into(),
    );

    draft.patch_config(mapping);
    drop(draft);
    clash.apply();
    Ok(())
}

fn mirror_clash_verge_fields(snap: &ClashConfig) -> anyhow::Result<()> {
    let verge = Config::verge();
    let mut draft = verge.draft();
    apply_clash_config_to_legacy_verge(&mut draft, snap)?;
    drop(draft);
    verge.apply();
    Ok(())
}

pub(crate) fn apply_clash_config_to_legacy_verge(
    draft: &mut IVerge,
    snap: &ClashConfig,
) -> anyhow::Result<()> {
    draft.enable_tun_mode = Some(snap.enable_tun_mode);
    draft.web_ui_list = Some(snap.web_ui_list.clone());
    draft.enable_clash_fields = Some(snap.enable_clash_fields);
    draft.enable_random_port = Some(matches!(snap.mixed_port.kind, PortStrategyKind::Random));
    draft.verge_mixed_port = Some(snap.mixed_port.start_port);
    draft.tun_stack = Some(super::yaml_convert(&snap.tun_stack)?);
    draft.clash_strategy = Some(legacy_app::ClashStrategy {
        external_controller_port_strategy: super::yaml_convert(
            &snap.external_controller.port.kind,
        )?,
    });

    let (proxy_change, profile_change, mode_change) =
        break_connection_to_legacy(&snap.break_connection);
    draft.break_when_proxy_change = Some(proxy_change);
    draft.break_when_profile_change = Some(profile_change);
    draft.break_when_mode_change = Some(mode_change);

    Ok(())
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

fn break_connection_to_legacy(
    value: &BreakConnectionStrategy,
) -> (legacy_app::BreakWhenProxyChange, bool, bool) {
    let proxy_change = match value.on_proxy_change {
        ProxyChangeBreakMode::Off => legacy_app::BreakWhenProxyChange::None,
        ProxyChangeBreakMode::ProxyGroup => legacy_app::BreakWhenProxyChange::Chain,
        ProxyChangeBreakMode::All => legacy_app::BreakWhenProxyChange::All,
    };
    (proxy_change, value.on_profile_change, value.on_mode_change)
}
