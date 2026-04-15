use crate::{config::Config, feat, log_err};
use anyhow::{Result, bail};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use rust_i18n::t;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{collections::HashMap, fmt, str::FromStr, sync::Arc};
use tauri::AppHandle;

use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Super keys that must be present in a valid hotkey
/// These are case-insensitive checked against the hotkey string
const SUPER_KEYS: &[&str] = &[
    "CommandOrControl",
    "Command",
    "Control",
    "Ctrl",
    "Meta",
    "Super",
    "Win",
    "Shift",
    "Alt",
];

/// Hotkey error types for frontend validation feedback
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "data")]
pub enum HotkeyError {
    InvalidHotkey(String),
    MissingSuperKey(String),
}

impl std::fmt::Display for HotkeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HotkeyError::InvalidHotkey(hotkey) => {
                write!(f, "{}", t!("hotkey_error.invalid_hotkey", hotkey = hotkey))
            }
            HotkeyError::MissingSuperKey(_hotkey) => {
                write!(f, "{}", t!("hotkey_error.missing_super_key"))
            }
        }
    }
}

impl std::error::Error for HotkeyError {}

/// Hotkey function identifier enum
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub enum HotkeyFunc {
    OpenOrCloseDashboard,
    ClashModeRule,
    ClashModeGlobal,
    ClashModeDirect,
    ClashModeScript,
    ToggleSystemProxy,
    EnableSystemProxy,
    DisableSystemProxy,
    ToggleTunMode,
    EnableTunMode,
    DisableTunMode,
}

impl HotkeyFunc {
    /// Returns all supported hotkey functions
    pub fn all() -> &'static [HotkeyFunc] {
        &[
            HotkeyFunc::OpenOrCloseDashboard,
            HotkeyFunc::ClashModeRule,
            HotkeyFunc::ClashModeGlobal,
            HotkeyFunc::ClashModeDirect,
            HotkeyFunc::ClashModeScript,
            HotkeyFunc::ToggleSystemProxy,
            HotkeyFunc::EnableSystemProxy,
            HotkeyFunc::DisableSystemProxy,
            HotkeyFunc::ToggleTunMode,
            HotkeyFunc::EnableTunMode,
            HotkeyFunc::DisableTunMode,
        ]
    }

    /// Returns the string identifier for this function
    pub fn as_str(&self) -> &'static str {
        match self {
            HotkeyFunc::OpenOrCloseDashboard => "open_or_close_dashboard",
            HotkeyFunc::ClashModeRule => "clash_mode_rule",
            HotkeyFunc::ClashModeGlobal => "clash_mode_global",
            HotkeyFunc::ClashModeDirect => "clash_mode_direct",
            HotkeyFunc::ClashModeScript => "clash_mode_script",
            HotkeyFunc::ToggleSystemProxy => "toggle_system_proxy",
            HotkeyFunc::EnableSystemProxy => "enable_system_proxy",
            HotkeyFunc::DisableSystemProxy => "disable_system_proxy",
            HotkeyFunc::ToggleTunMode => "toggle_tun_mode",
            HotkeyFunc::EnableTunMode => "enable_tun_mode",
            HotkeyFunc::DisableTunMode => "disable_tun_mode",
        }
    }

    /// Execute the hotkey action
    fn execute(&self) {
        match self {
            HotkeyFunc::OpenOrCloseDashboard => feat::toggle_dashboard(),
            HotkeyFunc::ClashModeRule => feat::change_clash_mode("rule".into()),
            HotkeyFunc::ClashModeGlobal => feat::change_clash_mode("global".into()),
            HotkeyFunc::ClashModeDirect => feat::change_clash_mode("direct".into()),
            HotkeyFunc::ClashModeScript => feat::change_clash_mode("script".into()),
            HotkeyFunc::ToggleSystemProxy => feat::toggle_system_proxy(),
            HotkeyFunc::EnableSystemProxy => feat::enable_system_proxy(),
            HotkeyFunc::DisableSystemProxy => feat::disable_system_proxy(),
            HotkeyFunc::ToggleTunMode => feat::toggle_tun_mode(),
            HotkeyFunc::EnableTunMode => feat::enable_tun_mode(),
            HotkeyFunc::DisableTunMode => feat::disable_tun_mode(),
        }
    }
}

impl fmt::Display for HotkeyFunc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for HotkeyFunc {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "open_or_close_dashboard" => Ok(HotkeyFunc::OpenOrCloseDashboard),
            "clash_mode_rule" => Ok(HotkeyFunc::ClashModeRule),
            "clash_mode_global" => Ok(HotkeyFunc::ClashModeGlobal),
            "clash_mode_direct" => Ok(HotkeyFunc::ClashModeDirect),
            "clash_mode_script" => Ok(HotkeyFunc::ClashModeScript),
            "toggle_system_proxy" => Ok(HotkeyFunc::ToggleSystemProxy),
            "enable_system_proxy" => Ok(HotkeyFunc::EnableSystemProxy),
            "disable_system_proxy" => Ok(HotkeyFunc::DisableSystemProxy),
            "toggle_tun_mode" => Ok(HotkeyFunc::ToggleTunMode),
            "enable_tun_mode" => Ok(HotkeyFunc::EnableTunMode),
            "disable_tun_mode" => Ok(HotkeyFunc::DisableTunMode),
            _ => bail!("invalid hotkey function: {s}"),
        }
    }
}

pub struct Hotkey {
    current: Arc<Mutex<Vec<String>>>, // 保存当前的热键设置

    app_handle: Arc<Mutex<Option<AppHandle>>>,
}
// (hotkey, func)
type HotKeyOp<'a> = (&'a str, HotKeyOpType<'a>);

#[derive(Debug)]
enum HotKeyOpType<'a> {
    #[allow(unused)]
    Unbind(&'a str),
    #[allow(unused)]
    Change(&'a str, &'a str),
    Bind(&'a str),
}

impl Hotkey {
    /// Returns the list of supported hotkey function identifiers
    pub fn get_supported_hotkey_functions() -> Vec<&'static str> {
        HotkeyFunc::all().iter().map(|f| f.as_str()).collect()
    }

    pub fn global() -> &'static Hotkey {
        static HOTKEY: OnceCell<Hotkey> = OnceCell::new();

        HOTKEY.get_or_init(|| Hotkey {
            current: Arc::new(Mutex::new(Vec::new())),
            app_handle: Arc::new(Mutex::new(None)),
        })
    }

    pub fn init(&self, app_handle: AppHandle) -> Result<()> {
        *self.app_handle.lock() = Some(app_handle);

        let verge = Config::verge();

        if let Some(hotkeys) = verge.latest().hotkeys.as_ref() {
            for hotkey in hotkeys.iter() {
                let mut iter = hotkey.split(',');
                let func = iter.next();
                let key = iter.next();

                match (key, func) {
                    (Some(key), Some(func)) => {
                        log_err!(Self::check_key(key).and_then(|_| self.register(key, func)));
                    }
                    _ => {
                        let key = key.unwrap_or("None");
                        let func = func.unwrap_or("None");
                        log::error!(target: "app", "invalid hotkey `{key}`:`{func}`");
                    }
                }
            }
            self.current.lock().clone_from(hotkeys);
        }

        Ok(())
    }

    /// 检查一个键是否合法
    fn check_key(hotkey: &str) -> anyhow::Result<()> {
        // fix #287
        // tauri的这几个方法全部有Result expect，会panic，先检测一遍避免挂了
        if hotkey.parse::<Shortcut>().is_err() {
            bail!("{}", t!("hotkey_error.invalid_hotkey", hotkey = hotkey));
        }
        // Validate super key requirement
        if !Self::validate_super_key(hotkey) {
            bail!("{}", t!("hotkey_error.missing_super_key"));
        }
        Ok(())
    }

    /// Check if the hotkey contains a super key modifier (case-insensitive)
    pub fn validate_super_key(hotkey: &str) -> bool {
        let hotkey_lower = hotkey.to_lowercase();
        SUPER_KEYS
            .iter()
            .any(|key| hotkey_lower.contains(&key.to_lowercase()))
    }

    fn register(&self, hotkey: &str, func: &str) -> Result<()> {
        let app_handle = self.app_handle.lock();
        if app_handle.is_none() {
            bail!("app handle is none");
        }
        let manager = app_handle.as_ref().unwrap().global_shortcut();

        if manager.is_registered(hotkey) {
            manager.unregister(hotkey)?;
        }

        let hotkey_func: HotkeyFunc = func.trim().parse()?;

        manager.on_shortcut(hotkey, move |_app_handle, hotkey, ev| {
            if let ShortcutState::Pressed = ev.state {
                tracing::info!("hotkey pressed: {}", hotkey);
                hotkey_func.execute();
            }
        })?;

        log::info!(target: "app", "register hotkey {hotkey} {func}");
        Ok(())
    }

    fn unregister(&self, hotkey: &str) -> Result<()> {
        let app_handle = self.app_handle.lock();
        if app_handle.is_none() {
            bail!("app handle is none");
        }
        let manager = app_handle.as_ref().unwrap().global_shortcut();

        manager.unregister(hotkey)?;
        log::info!(target: "app", "unregister hotkey {hotkey}");
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn update(&self, new_hotkeys: Vec<String>) -> Result<()> {
        let mut current = self.current.lock();
        let old_map = Self::get_map_from_vec(&current);
        let new_map = Self::get_map_from_vec(&new_hotkeys);

        let ops = Self::get_ops(old_map, new_map);

        // 先检查一遍所有新的热键是不是可以用的
        for (hotkey, op) in ops.iter() {
            if matches!(op, HotKeyOpType::Bind(_) | HotKeyOpType::Change(_, _)) {
                Self::check_key(hotkey)?
            }
        }

        tracing::info!("hotkey update: {:?}", ops);

        for (hotkey, op) in ops.iter() {
            match op {
                HotKeyOpType::Unbind(_) => self.unregister(hotkey)?,
                HotKeyOpType::Change(_, new_func) => {
                    self.unregister(hotkey)?;
                    self.register(hotkey, new_func)?;
                }
                HotKeyOpType::Bind(func) => self.register(hotkey, func)?,
            }
        }

        *current = new_hotkeys;
        Ok(())
    }

    fn get_map_from_vec(hotkeys: &[String]) -> HashMap<&str, &str> {
        let mut map = HashMap::new();

        hotkeys.iter().for_each(|hotkey| {
            let mut iter = hotkey.split(',');
            let func = iter.next();
            let key = iter.next();

            if func.is_some() && key.is_some() {
                let func = func.unwrap().trim();
                let key = key.unwrap().trim();
                map.insert(key, func);
            }
        });
        map
    }

    fn get_ops<'a>(
        old_map: HashMap<&'a str, &'a str>,
        new_map: HashMap<&'a str, &'a str>,
    ) -> Vec<HotKeyOp<'a>> {
        let mut list = Vec::<HotKeyOp<'a>>::new();
        old_map.iter().for_each(|(key, func)| {
            match new_map.get(key) {
                Some(new_func) => {
                    if new_func != func {
                        list.push((*key, HotKeyOpType::Change(func, new_func)))
                    }

                    // 无变化，无需操作
                }
                None => {
                    list.push((*key, HotKeyOpType::Unbind(func)));
                }
            }
        });

        new_map.iter().for_each(|(key, func)| {
            if !old_map.contains_key(key) {
                list.push((*key, HotKeyOpType::Bind(func)));
            }
        });
        list
    }
}

impl Drop for Hotkey {
    fn drop(&mut self) {
        let app_handle = self.app_handle.lock();
        if let Some(app_handle) = app_handle.as_ref() {
            let manager = app_handle.global_shortcut();
            if let Ok(()) = manager.unregister_all() {
                log::info!(target: "app", "unregister all hotkeys");
            }
        }
    }
}
