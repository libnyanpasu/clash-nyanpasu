//! What the application and a statistic widget process share: the widget
//! variants, the messages the application sends, and the IPC channel that
//! carries them. Nothing here renders, so every widget frontend can use it.

#[cfg(feature = "ipc")]
pub mod ipc;

#[derive(
    Debug,
    serde::Serialize,
    serde::Deserialize,
    specta::Type,
    Copy,
    Clone,
    PartialEq,
    Eq,
    clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum StatisticWidgetVariant {
    Large,
    Small,
}

impl std::fmt::Display for StatisticWidgetVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Large => write!(f, "large"),
            Self::Small => write!(f, "small"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogoPreset {
    #[default]
    Default,
    System,
    Tun,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct StatisticMessage {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum Message {
    Stop,
    UpdateStatistic(StatisticMessage),
    UpdateLogo(LogoPreset),
}

#[cfg(test)]
mod tests {
    use clap::ValueEnum as _;

    use super::*;

    #[test]
    fn variants_keep_their_config_and_cli_names() {
        for (variant, name) in [
            (StatisticWidgetVariant::Large, "large"),
            (StatisticWidgetVariant::Small, "small"),
        ] {
            let json = format!("\"{name}\"");
            assert_eq!(serde_json::to_string(&variant).unwrap(), json);
            assert_eq!(
                serde_json::from_str::<StatisticWidgetVariant>(&json).unwrap(),
                variant
            );
            // The application passes the variant to the widget process by its
            // `Display` name, which the subcommand parses back.
            assert_eq!(variant.to_string(), name);
            assert_eq!(
                StatisticWidgetVariant::from_str(name, false).unwrap(),
                variant
            );
        }
    }

    /// The payload nyanpasu-local-channel frames: the variant index, then the fields in
    /// declaration order.
    #[test]
    fn messages_keep_their_wire_layout() {
        let statistic = Message::UpdateStatistic(StatisticMessage {
            download_total: 1,
            upload_total: 2,
            download_speed: 3,
            upload_speed: 4,
        });
        for (message, bytes) in [
            (Message::Stop, vec![0]),
            (statistic, vec![1, 1, 2, 3, 4]),
            (Message::UpdateLogo(LogoPreset::Default), vec![2, 0]),
            (Message::UpdateLogo(LogoPreset::System), vec![2, 1]),
            (Message::UpdateLogo(LogoPreset::Tun), vec![2, 2]),
        ] {
            assert_eq!(postcard::to_stdvec(&message).unwrap(), bytes);
            let decoded: Message = postcard::from_bytes(&bytes).unwrap();
            assert_eq!(format!("{decoded:?}"), format!("{message:?}"));
        }
    }
}
