use crate::{
    config::{Config, Draft, IClashTemp, IVerge, nyanpasu as legacy_app},
    state::mirror::{ClashLegacyBridge, PreparedLegacyMirror},
};
use nyanpasu_config::clash::config::{
    ClashConfig,
    clash_strategy::{
        BreakConnectionStrategy, PortStrategy, PortStrategyKind, ProxyChangeBreakMode,
    },
};
use serde_yaml::{Mapping, Value};
use std::{net::SocketAddr, sync::Arc};

pub struct LegacyClashBridge {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
}

impl Default for LegacyClashBridge {
    fn default() -> Self {
        Self::new(Arc::new(parking_lot::Mutex::new(())))
    }
}

impl LegacyClashBridge {
    pub(crate) fn new(legacy_lock: Arc<parking_lot::Mutex<()>>) -> Self {
        Self { legacy_lock }
    }
}

struct PreparedClashMirror {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
    clash_store: Draft<IClashTemp>,
    clash_projected: IClashTemp,
    verge_store: Draft<IVerge>,
    verge_projected: IVerge,
}

impl PreparedLegacyMirror for PreparedClashMirror {
    fn apply(self: Box<Self>) {
        let Self {
            legacy_lock,
            clash_store,
            clash_projected,
            verge_store,
            verge_projected,
        } = *self;
        let _guard = legacy_lock.lock();
        clash_store.apply_update(|target| *target = clash_projected.clone());
        verge_store
            .apply_update(|target| apply_prepared_clash_verge_projection(target, &verge_projected));
    }
}

impl LegacyClashBridge {
    fn prepare_mirror(&self, snap: &ClashConfig) -> anyhow::Result<PreparedClashMirror> {
        let clash_store = Config::clash();
        let verge_store = Config::verge();
        let mut clash_projected = {
            let _guard = self.legacy_lock.lock();
            clash_store.data().clone()
        };
        let mut verge_projected = IVerge::default();
        prepare_clash_overrides(&mut clash_projected, snap)?;
        apply_clash_config_to_legacy_verge(&mut verge_projected, snap)?;
        Ok(PreparedClashMirror {
            legacy_lock: Arc::clone(&self.legacy_lock),
            clash_store,
            clash_projected,
            verge_store,
            verge_projected,
        })
    }
}

impl ClashLegacyBridge for LegacyClashBridge {
    fn prepare(&self, snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
        // TODO(actor-migration): compatibility bridge for legacy Config::clash()/Config::verge().
        // Reason: Clash config readers still consume legacy globals while typed actors are wired.
        // Remove when Clash config reads/writes use ClashConfigClient and runtime DTO conversion.
        Ok(Box::new(self.prepare_mirror(snap)?))
    }

    fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
        let _guard = self.legacy_lock.lock();
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

fn prepare_clash_overrides(projected: &mut IClashTemp, snap: &ClashConfig) -> anyhow::Result<()> {
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

    projected.patch_config(mapping);
    Ok(())
}

pub(crate) fn apply_prepared_clash_verge_projection(target: &mut IVerge, projected: &IVerge) {
    target.enable_tun_mode = projected.enable_tun_mode;
    target.web_ui_list = projected.web_ui_list.clone();
    target.enable_clash_fields = projected.enable_clash_fields;
    target.enable_random_port = projected.enable_random_port;
    target.verge_mixed_port = projected.verge_mixed_port;
    target.tun_stack = projected.tun_stack.clone();
    target.clash_strategy = projected.clash_strategy.clone();
    target.break_when_proxy_change = projected.break_when_proxy_change.clone();
    target.break_when_profile_change = projected.break_when_profile_change;
    target.break_when_mode_change = projected.break_when_mode_change;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_apply_preserves_intervening_verge_update_and_shares_lock() {
        let bridge = LegacyClashBridge::default();
        let verge_store = Config::verge();
        let original = verge_store.data().clone();
        let mut next = ClashConfig::default();
        next.enable_tun_mode = true;
        let prepared = bridge
            .prepare_mirror(&next)
            .expect("default Clash projection should prepare");

        assert!(Arc::ptr_eq(&bridge.legacy_lock, &prepared.legacy_lock));
        let guard = bridge.legacy_lock.lock();
        assert!(prepared.legacy_lock.try_lock().is_none());
        drop(guard);

        verge_store.apply_update(|state| state.enable_auto_launch = Some(true));
        Box::new(prepared).apply();

        let applied = verge_store.data().clone();
        assert_eq!(applied.enable_auto_launch, Some(true));
        assert_eq!(applied.enable_tun_mode, Some(true));
        verge_store.apply_update(|state| *state = original.clone());
    }
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
