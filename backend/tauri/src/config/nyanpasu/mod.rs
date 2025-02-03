use crate::utils::{dirs, help};
use anyhow::Result;
// use log::LevelFilter;
use enumflags2::bitflags;
use nyanpasu_macro::VergePatch;
use serde::{Deserialize, Serialize};
use specta::Type;

mod clash_strategy;
pub mod logging;
mod widget;

pub use self::clash_strategy::{ClashStrategy, ExternalControllerPortStrategy};
pub use logging::LoggingLevel;
pub use widget::NetworkStatisticWidgetConfig;

// TODO: when support sing-box, remove this struct
#[bitflags]
#[repr(u8)]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Type)]
pub enum ClashCore {
    #[serde(rename = "clash", alias = "clash-premium")]
    ClashPremium = 0b0001,
    #[serde(rename = "clash-rs")]
    ClashRs,
    #[serde(rename = "mihomo", alias = "clash-meta")]
    Mihomo,
    #[serde(rename = "mihomo-alpha")]
    MihomoAlpha,
    #[serde(rename = "clash-rs-alpha")]
    ClashRsAlpha,
}

impl Default for ClashCore {
    fn default() -> Self {
        match cfg!(feature = "default-meta") {
            false => Self::ClashPremium,
            true => Self::Mihomo,
        }
    }
}

impl From<ClashCore> for String {
    fn from(core: ClashCore) -> Self {
        match core {
            ClashCore::ClashPremium => "clash".into(),
            ClashCore::ClashRs => "clash-rs".into(),
            ClashCore::Mihomo => "mihomo".into(),
            ClashCore::MihomoAlpha => "mihomo-alpha".into(),
            ClashCore::ClashRsAlpha => "clash-rs-alpha".into(),
        }
    }
}

impl std::fmt::Display for ClashCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClashCore::ClashPremium => write!(f, "clash"),
            ClashCore::ClashRs => write!(f, "clash-rs"),
            ClashCore::Mihomo => write!(f, "mihomo"),
            ClashCore::MihomoAlpha => write!(f, "mihomo-alpha"),
            ClashCore::ClashRsAlpha => write!(f, "clash-rs-alpha"),
        }
    }
}

impl From<&ClashCore> for nyanpasu_utils::core::CoreType {
    fn from(core: &ClashCore) -> Self {
        match core {
            ClashCore::ClashPremium => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::ClashPremium,
            ),
            ClashCore::ClashRs => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::ClashRust,
            ),
            ClashCore::Mihomo => {
                nyanpasu_utils::core::CoreType::Clash(nyanpasu_utils::core::ClashCoreType::Mihomo)
            }
            ClashCore::MihomoAlpha => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::MihomoAlpha,
            ),
            ClashCore::ClashRsAlpha => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::ClashRustAlpha,
            ),
        }
    }
}

impl TryFrom<&nyanpasu_utils::core::CoreType> for ClashCore {
    type Error = anyhow::Error;

    fn try_from(core: &nyanpasu_utils::core::CoreType) -> Result<Self> {
        match core {
            nyanpasu_utils::core::CoreType::Clash(clash) => match clash {
                nyanpasu_utils::core::ClashCoreType::ClashPremium => Ok(ClashCore::ClashPremium),
                nyanpasu_utils::core::ClashCoreType::ClashRust => Ok(ClashCore::ClashRs),
                nyanpasu_utils::core::ClashCoreType::ClashRustAlpha => Ok(ClashCore::ClashRsAlpha),
                nyanpasu_utils::core::ClashCoreType::Mihomo => Ok(ClashCore::Mihomo),
                nyanpasu_utils::core::ClashCoreType::MihomoAlpha => Ok(ClashCore::MihomoAlpha),
            },
            _ => Err(anyhow::anyhow!("unsupported core type")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProxiesSelectorMode {
    Hidden,
    #[default]
    Normal,
    Submenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum TunStack {
    System,
    #[default]
    Gvisor,
    Mixed,
}

impl AsRef<str> for TunStack {
    fn as_ref(&self) -> &str {
        match self {
            TunStack::System => "system",
            TunStack::Gvisor => "gvisor",
            TunStack::Mixed => "mixed",
        }
    }
}

/// ### `verge.yaml` schema
#[derive(Default, Debug, Clone, Deserialize, Serialize, VergePatch, specta::Type)]
#[verge(patch_fn = "patch_config")]
// TODO: use new managedState and builder pattern instead
pub struct IVerge {
    /// app listening port for app singleton
    pub app_singleton_port: Option<u16>,

    /// app log level
    /// silent | error | warn | info | debug | trace
    pub app_log_level: Option<logging::LoggingLevel>,

    // i18n
    pub language: Option<String>,

    /// `light` or `dark` or `system`
    pub theme_mode: Option<String>,

    /// enable traffic graph default is true
    pub traffic_graph: Option<bool>,

    /// show memory info (only for Clash Meta)
    pub enable_memory_usage: Option<bool>,

    /// global ui framer motion effects
    pub lighten_animation_effects: Option<bool>,

    /// clash tun mode
    pub enable_tun_mode: Option<bool>,

    /// windows service mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_service_mode: Option<bool>,

    /// can the app auto startup
    pub enable_auto_launch: Option<bool>,

    /// not show the window on launch
    pub enable_silent_start: Option<bool>,

    /// set system proxy
    pub enable_system_proxy: Option<bool>,

    /// enable proxy guard
    pub enable_proxy_guard: Option<bool>,

    /// set system proxy bypass
    pub system_proxy_bypass: Option<String>,

    /// proxy guard interval
    #[serde(alias = "proxy_guard_duration")]
    pub proxy_guard_interval: Option<u64>,

    /// theme setting
    pub theme_color: Option<String>,

    /// web ui list
    pub web_ui_list: Option<Vec<String>>,

    /// clash core path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clash_core: Option<ClashCore>,

    /// hotkey map
    /// format: {func},{key}
    pub hotkeys: Option<Vec<String>>,

    /// 切换代理时自动关闭连接
    pub auto_close_connection: Option<bool>,

    /// 默认的延迟测试连接
    pub default_latency_test: Option<String>,

    /// 支持关闭字段过滤，避免meta的新字段都被过滤掉，默认为真
    pub enable_clash_fields: Option<bool>,

    /// 是否使用内部的脚本支持，默认为真
    pub enable_builtin_enhanced: Option<bool>,

    /// proxy 页面布局 列数
    pub proxy_layout_column: Option<i32>,

    /// 日志清理
    /// 分钟数； 0 为不清理
    #[deprecated(note = "use `max_log_files` instead")]
    pub auto_log_clean: Option<i64>,
    /// 日记轮转时间，单位：天
    pub max_log_files: Option<usize>,
    /// window size and position
    #[deprecated(note = "use `window_size_state` instead")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_size_position: Option<Vec<f64>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_size_state: Option<WindowState>,

    /// 是否启用随机端口
    pub enable_random_port: Option<bool>,

    /// verge mixed port 用于覆盖 clash 的 mixed port
    pub verge_mixed_port: Option<u16>,

    /// Check update when app launch
    pub enable_auto_check_update: Option<bool>,

    /// Clash 相关策略
    pub clash_strategy: Option<ClashStrategy>,

    /// 是否启用代理托盘选择
    pub clash_tray_selector: Option<ProxiesSelectorMode>,

    pub always_on_top: Option<bool>,

    /// Tun 堆栈选择
    /// TODO: 弃用此字段，转移到 clash config 里
    pub tun_stack: Option<TunStack>,

    /// 是否启用网络统计信息浮窗
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_statistic_widget: Option<NetworkStatisticWidgetConfig>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type)]
pub struct WindowState {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl IVerge {
    pub fn new() -> Self {
        match dirs::nyanpasu_config_path().and_then(|path| help::read_yaml::<IVerge>(&path)) {
            Ok(config) => Self::merge_with_template(config),
            Err(err) => {
                log::error!(target: "app", "{err:?}");
                Self::template()
            }
        }
    }

    fn merge_with_template(mut config: IVerge) -> Self {
        let template = Self::template();

        if config.enable_auto_check_update.is_none() {
            config.enable_auto_check_update = template.enable_auto_check_update;
        }

        if config.clash_tray_selector.is_none() {
            config.clash_tray_selector = template.clash_tray_selector;
        }

        if config.max_log_files.is_none() {
            config.max_log_files = template.max_log_files;
        }

        if config.lighten_animation_effects.is_none() {
            config.lighten_animation_effects = template.lighten_animation_effects;
        }

        if config.enable_service_mode.is_none() {
            config.enable_service_mode = template.enable_service_mode;
        }

        config
    }

    pub fn template() -> Self {
        Self {
            clash_core: Some(ClashCore::default()),
            language: {
                let locale = crate::utils::help::get_system_locale();
                Some(crate::utils::help::mapping_to_i18n_key(&locale).into())
            },
            app_log_level: Some(logging::LoggingLevel::default()),
            theme_mode: Some("system".into()),
            traffic_graph: Some(true),
            enable_memory_usage: Some(true),
            enable_auto_launch: Some(false),
            enable_silent_start: Some(false),
            enable_system_proxy: Some(false),
            enable_random_port: Some(false),
            verge_mixed_port: Some(7890),
            enable_proxy_guard: Some(false),
            proxy_guard_interval: Some(30),
            auto_close_connection: Some(true),
            enable_builtin_enhanced: Some(true),
            enable_clash_fields: Some(true),
            lighten_animation_effects: Some(false),
            // auto_log_clean: Some(60 * 24 * 7), // 7 days 自动清理日记
            max_log_files: Some(7), // 7 days
            enable_auto_check_update: Some(true),
            clash_tray_selector: Some(ProxiesSelectorMode::default()),
            enable_service_mode: Some(false),
            always_on_top: Some(false),
            ..Self::default()
        }
    }

    /// Save IVerge App Config
    pub fn save_file(&self) -> Result<()> {
        help::save_yaml(
            &dirs::nyanpasu_config_path()?,
            &self,
            Some("# Clash Nyanpasu Config"),
        )
    }
}
