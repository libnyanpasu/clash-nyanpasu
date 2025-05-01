use crate::{config::Config, feat, log_err};
use anyhow::{Result, bail};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::{collections::HashMap, sync::Arc};
use tauri::AppHandle;

use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

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
    fn check_key(hotkey: &str) -> Result<()> {
        // fix #287
        // tauri的这几个方法全部有Result expect，会panic，先检测一遍避免挂了
        if hotkey.parse::<Shortcut>().is_err() {
            bail!("invalid hotkey `{hotkey}`");
        }
        Ok(())
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

        let f = match func.trim() {
            "open_or_close_dashboard" => feat::toggle_dashboard,
            "clash_mode_rule" => || feat::change_clash_mode("rule".into()),
            "clash_mode_global" => || feat::change_clash_mode("global".into()),
            "clash_mode_direct" => || feat::change_clash_mode("direct".into()),
            "clash_mode_script" => || feat::change_clash_mode("script".into()),
            "toggle_system_proxy" => feat::toggle_system_proxy,
            "enable_system_proxy" => feat::enable_system_proxy,
            "disable_system_proxy" => feat::disable_system_proxy,
            "toggle_tun_mode" => feat::toggle_tun_mode,
            "enable_tun_mode" => feat::enable_tun_mode,
            "disable_tun_mode" => feat::disable_tun_mode,
            _ => bail!("invalid function \"{func}\""),
        };

        manager.on_shortcut(hotkey, move |_app_handle, hotkey, ev| {
            if let ShortcutState::Pressed = ev.state {
                tracing::info!("hotkey pressed: {}", hotkey);
                f();
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
