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

/// A state of the bundled cat logo, which has a colour per proxy state.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogoPreset {
    #[default]
    Default,
    System,
    Tun,
}

/// The logo a widget shows.
#[derive(Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum Logo {
    /// The bundled cat, drawn from its vector source.
    Builtin(LogoPreset),
    /// A PNG image, rasterized by the widget at the size it is drawn at.
    Image(Vec<u8>),
}

/// An image is shown as a preview: the whole PNG would be every byte of it.
impl std::fmt::Debug for Logo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Builtin(preset) => f.debug_tuple("Builtin").field(preset).finish(),
            Self::Image(bytes) => f
                .debug_tuple("Image")
                .field(&nyanpasu_log_preview::bytes(bytes))
                .finish(),
        }
    }
}

impl Default for Logo {
    fn default() -> Self {
        Self::Builtin(LogoPreset::Default)
    }
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
    UpdateLogo(Logo),
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
            (
                Message::UpdateLogo(Logo::Builtin(LogoPreset::Default)),
                vec![2, 0, 0],
            ),
            (
                Message::UpdateLogo(Logo::Builtin(LogoPreset::System)),
                vec![2, 0, 1],
            ),
            (
                Message::UpdateLogo(Logo::Builtin(LogoPreset::Tun)),
                vec![2, 0, 2],
            ),
            (
                Message::UpdateLogo(Logo::Image(vec![0x89, b'P'])),
                vec![2, 1, 2, 0x89, b'P'],
            ),
        ] {
            assert_eq!(postcard::to_stdvec(&message).unwrap(), bytes);
            let decoded: Message = postcard::from_bytes(&bytes).unwrap();
            assert_eq!(format!("{decoded:?}"), format!("{message:?}"));
        }
    }

    #[test]
    fn image_logos_debug_as_a_preview() {
        let png = vec![0x42; 256 * 1024];
        let debug = format!("{:?}", Message::UpdateLogo(Logo::Image(png.clone())));
        assert_eq!(
            debug,
            format!("UpdateLogo(Image({:?}))", nyanpasu_log_preview::bytes(&png))
        );
        assert!(debug.len() < 100, "{debug}");
        assert_eq!(
            format!("{:?}", Logo::Builtin(LogoPreset::Tun)),
            "Builtin(Tun)"
        );
    }
}
