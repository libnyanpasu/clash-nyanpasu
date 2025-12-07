use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::config::{
    profile::item_type::{ProfileItemType, ProfileUid},
    snapshot,
};

use super::{ChainItem, ChainTypeWrapper, RunnerManager, use_merge};
use parking_lot::Mutex;
use std::{borrow::Borrow, sync::Arc};

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

pub struct ProcessResult {
    pub config: Mapping,
    pub logs: IndexMap<ProfileUid, Logs>,
}

/// 处理链
pub async fn process_chain(
    mut config: Mapping,
    nodes: &[ChainItem],
    snapshots_builder: &mut snapshot::ConfigSnapshotsBuilder,
    snapshot_chain_type: snapshot::ChainNodeKind,
) -> ProcessResult {
    let mut result_map = IndexMap::new();

    let mut script_runner = RunnerManager::new();

    let mut add_snapshot = |profile_id: &str,
                            profile_item_type: ProfileItemType,
                            previous_config: &Mapping,
                            new_config: &Mapping| {
        snapshots_builder.push_node(snapshot::ConfigSnapshotState::new(
            snapshot::ConfigSnapshot::new_with_diff(previous_config, new_config.clone()),
            snapshot::ProcessKind::ChainNode {
                kind: snapshot_chain_type.clone(),
                profile_id: profile_id.to_string(),
                profile_kind: profile_item_type,
            },
            None,
        ));
    };

    for item in nodes.iter() {
        match &item.data {
            ChainTypeWrapper::Merge(merge) => {
                let mut logs = vec![];
                let (res, process_logs) = use_merge(merge, config.clone());
                let new_config = res.unwrap();
                add_snapshot(&item.uid, ProfileItemType::Merge, &config, &new_config);
                config = new_config;
                logs.extend(process_logs);
                result_map.insert(item.uid.to_string(), logs);
            }
            ChainTypeWrapper::Script {
                kind: script_type,
                data: script,
            } => {
                let mut logs = vec![];
                let (res, process_logs) = script_runner
                    .process_script(*script_type, script, config.clone())
                    .await;
                logs.extend(process_logs);
                // TODO: 修改日记 level 格式？
                match res {
                    Ok(res_config) => {
                        add_snapshot(
                            &item.uid,
                            ProfileItemType::Script(*script_type),
                            &config,
                            &res_config,
                        );
                        config = res_config;
                    }
                    Err(err) => {
                        add_snapshot(
                            &item.uid,
                            ProfileItemType::Script(*script_type),
                            &config,
                            &config,
                        );
                        logs.error(err.to_string())
                    }
                }
                // TODO: 这里添加对 field 的检查，触发 WARN 日记。此外，需要对 Merge 的结果进行检查？
                result_map.insert(item.uid.to_string(), logs);
            }
        }
    }

    ProcessResult {
        config,
        logs: result_map,
    }
}

#[cfg(test)]
mod tests {
    use crate::enhance::chain::ChainTypeWrapper;

    use super::*;
    use serde_yaml::Value;

    #[tokio::test]
    async fn test_process_chain_order() {
        // 准备初始配置
        let mut initial_config = Mapping::new();
        initial_config.insert(
            Value::String("value".to_string()),
            Value::String("initial".to_string()),
        );

        // 创建两个 ChainItem
        let item_a = ChainItem {
            uid: "a".to_string(),
            data: ChainTypeWrapper::new_js(bytes::Bytes::from_static(
                b"function main(cfg) { cfg.value = 'a'; return cfg; }",
            )),
        };

        let item_b = ChainItem {
            uid: "b".to_string(),
            data: ChainTypeWrapper::new_js(bytes::Bytes::from_static(
                b"function main(cfg) { cfg.value = cfg.value + '_b'; return cfg; }",
            )),
        };

        let chain = vec![item_a, item_b];

        let mut snapshots_builder = snapshot::ConfigSnapshotsBuilder::new(
            snapshot::ConfigSnapshot {
                config: initial_config.clone(),
                changed_fields: None,
            },
            "root".to_string(),
        );

        let snapshot_chain_type = snapshot::ChainNodeKind::Global;

        // 执行处理链
        let ProcessResult {
            config: final_config,
            logs,
        } = process_chain(
            initial_config,
            &chain,
            &mut snapshots_builder,
            snapshot_chain_type,
        )
        .await;

        // 验证最终结果
        assert_eq!(
            final_config.get("value").unwrap().as_str().unwrap(),
            "a_b",
            "链式处理应该按顺序执行：A 将值设为 'a'，然后 B 将 'a' 修改为 'a_b'"
        );

        // 验证日志存在
        assert!(logs.contains_key("a"), "应该包含 A 的处理日志");
        assert!(logs.contains_key("b"), "应该包含 B 的处理日志");
    }
}
