pub mod clash;
pub mod mapping;
pub mod verge;
pub mod window;

use serde::{Serialize, de::DeserializeOwned};

pub(super) fn yaml_convert<T, U>(value: T) -> anyhow::Result<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_yaml::to_value(value)?;
    Ok(serde_yaml::from_value(value)?)
}
