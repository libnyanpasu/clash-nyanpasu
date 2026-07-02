pub mod clash;
pub mod mapping;
pub mod verge;
pub mod window;

use crate::config::{Config, IVerge};
use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, state::PersistentState,
};
use serde::{Serialize, de::DeserializeOwned};

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

pub(crate) fn typed_config_from_legacy(
    legacy: &IVerge,
) -> anyhow::Result<(NyanpasuAppConfig, PersistentState, ClashConfig)> {
    let legacy_clash = Config::clash().data().clone();
    Ok((
        verge::application_from_legacy(legacy)?,
        window::persistent_state_from_legacy(legacy)?,
        clash::clash_config_from_legacy(legacy, &legacy_clash.0)?,
    ))
}

pub(super) fn yaml_convert<T, U>(value: T) -> anyhow::Result<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_yaml::to_value(value)?;
    Ok(serde_yaml::from_value(value)?)
}
