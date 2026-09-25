pub mod clash;
pub mod mapping;
pub mod verge;
pub mod window;

use crate::config::IVerge;
use nyanpasu_config::{
    application::NyanpasuAppConfig, clash::config::ClashConfig, state::PersistentState,
};
use serde::{Serialize, de::DeserializeOwned};

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

pub(super) fn yaml_convert<T, U>(value: T) -> anyhow::Result<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_yaml::to_value(value)?;
    Ok(serde_yaml::from_value(value)?)
}
