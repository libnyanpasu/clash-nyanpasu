use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use specta::Type;

/// Stable profile identifier. It is also the key used by [`Profiles::items`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
#[repr(transparent)]
pub struct ProfileId(pub String);

impl fmt::Display for ProfileId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ProfileId {
    type Err = std::convert::Infallible;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self(value.to_owned()))
    }
}
