use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::config::profile::{item_type::ProfileUid, profiles::Profiles};

use super::{use_merge, ChainItem, ChainTypeWrapper, RunnerManager};
use parking_lot::Mutex;
use std::{borrow::Borrow, sync::Arc};

pub fn convert_uids_to_scripts(profiles: &Profiles, uids: &[ProfileUid]) -> Vec<ChainItem> {
    uids.iter()
        .filter_map(|uid| profiles.get_item(uid).ok())
        .filter_map(<Option<ChainItem>>::from)
        .collect::<Vec<ChainItem>>()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum LogSpan {
    Log,
    Info,
    Warn,
    Error,
}

impl AsRef<str> for LogSpan {
    fn as_ref(&self) -> &str {
        match self {
            LogSpan::Log => "log",
            LogSpan::Info => "info",
            LogSpan::Warn => "warn",
            LogSpan::Error => "error",
        }
    }
}

pub type Logs = Vec<(LogSpan, String)>;
pub trait LogsExt {
    fn span<T: AsRef<str>>(&mut self, span: LogSpan, msg: T);
    fn log<T: AsRef<str>>(&mut self, msg: T);
    fn info<T: AsRef<str>>(&mut self, msg: T);
    fn warn<T: AsRef<str>>(&mut self, msg: T);
    fn error<T: AsRef<str>>(&mut self, msg: T);
}
impl LogsExt for Logs {
    fn span<T: AsRef<str>>(&mut self, span: LogSpan, msg: T) {
        self.push((span, msg.as_ref().to_string()));
    }
    fn log<T: AsRef<str>>(&mut self, msg: T) {
        self.span(LogSpan::Log, msg);
    }
    fn info<T: AsRef<str>>(&mut self, msg: T) {
        self.span(LogSpan::Info, msg);
    }
    fn warn<T: AsRef<str>>(&mut self, msg: T) {
        self.span(LogSpan::Warn, msg);
    }
    fn error<T: AsRef<str>>(&mut self, msg: T) {
        self.span(LogSpan::Error, msg);
    }
}

pub fn take_logs(logs: Arc<Mutex<Option<Logs>>>) -> Logs {
    logs.lock().take().unwrap()
}

/// 合并多个配置
// TODO: 可能移动到其他地方
// TODO: 增加自定义合并逻辑
// TODO: 添加元信息
pub fn merge_profiles<T: Borrow<String>>(mappings: IndexMap<T, Mapping>) -> Mapping {
    mappings
        .into_iter()
        .enumerate()
        .fold(Mapping::new(), |mut acc, (idx, (_key, value))| {
            // full extend the first one, others just extend proxies
            // TODO: custom merge logic
            // TODO: add meta info
            if idx == 0 {
                acc.extend(value);
            } else {
                let proxies = value.get("proxies").unwrap().as_sequence().unwrap().clone();
                let acc_proxies = acc.get_mut("proxies").unwrap().as_sequence_mut().unwrap();
                acc_proxies.extend(proxies);
            }
            acc
        })
}

/// 处理链
pub async fn process_chain(
    mut config: Mapping,
    nodes: &[ChainItem],
) -> (Mapping, IndexMap<ProfileUid, Logs>) {
    let mut result_map = IndexMap::new();

    let mut script_runner = RunnerManager::new();
    for item in nodes.iter() {
        match &item.data {
            ChainTypeWrapper::Merge(merge) => {
                let mut logs = vec![];
                let (res, process_logs) = use_merge(merge, config.to_owned());
                config = res.unwrap();
                logs.extend(process_logs);
                result_map.insert(item.uid.to_string(), logs);
            }
            ChainTypeWrapper::Script(script) => {
                let mut logs = vec![];
                let (res, process_logs) = script_runner
                    .process_script(script, config.to_owned())
                    .await;
                logs.extend(process_logs);
                // TODO: 修改日记 level 格式？
                match res {
                    Ok(res_config) => {
                        config = res_config;
                    }
                    Err(err) => logs.error(err.to_string()),
                }
                // TODO: 这里添加对 field 的检查，触发 WARN 日记。此外，需要对 Merge 的结果进行检查？
                result_map.insert(item.uid.to_string(), logs);
            }
        }
    }

    (config, result_map)
}
