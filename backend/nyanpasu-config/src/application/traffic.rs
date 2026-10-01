use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;

/// How long recorded traffic is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
pub enum TrafficRetention {
    #[serde(rename = "1d")]
    OneDay,
    #[serde(rename = "7d")]
    #[default]
    SevenDays,
    #[serde(rename = "30d")]
    ThirtyDays,
    #[serde(rename = "90d")]
    NinetyDays,
    #[serde(rename = "forever")]
    Forever,
}

impl TrafficRetention {
    /// `None` keeps everything.
    pub fn duration(self) -> Option<Duration> {
        let days = match self {
            Self::OneDay => 1,
            Self::SevenDays => 7,
            Self::ThirtyDays => 30,
            Self::NinetyDays => 90,
            Self::Forever => return None,
        };
        Some(Duration::from_secs(days * 24 * 60 * 60))
    }
}
