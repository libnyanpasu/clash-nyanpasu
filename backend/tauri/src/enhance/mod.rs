mod chain;
mod field;
mod merge;
mod script;
mod tun;
mod utils;

pub use self::chain::ScriptType;
use self::{chain::*, field::*, merge::*, script::*, tun::*};
use crate::config::{nyanpasu::ClashCore, Config};
use indexmap::IndexMap;
use serde_yaml::Mapping;
use std::collections::HashSet;
pub use utils::{Logs, LogsExt};

/// Enhance mode
/// 返回最终配置、该配置包含的键、和script执行的结果
pub async fn enhance() -> (Mapping, Vec<String>, IndexMap<String, Logs>) {
    // config.yaml 的配置
    let clash_config = { Config::clash().latest().0.clone() };

    let (clash_core, enable_tun, enable_builtin, enable_filter) = {
        let verge = Config::verge();
        let verge = verge.latest();
        (
            verge.clash_core,
            verge.enable_tun_mode.unwrap_or(false),
            verge.enable_builtin_enhanced.unwrap_or(true),
            verge.enable_clash_fields.unwrap_or(true),
        )
    };

    // 从profiles里拿东西
    let (mut config, chains, valid) = {
        let profiles = Config::profiles();
        let profiles = profiles.latest();

        let mut profile_spec_chains = {
            let profile = profiles
                .get_current()
                .and_then(|uid| profiles.get_item(&uid).ok());
            match profile {
                Some(profile) => match &profile.chains {
                    Some(chains) => utils::convert_uids_to_scripts(&profiles, chains),
                    None => vec![],
                },
                None => vec![],
            }
        };

        let current_mapping = profiles.current_mapping().unwrap_or_default();

        let chain = match profiles.chain.as_ref() {
            Some(chain) => utils::convert_uids_to_scripts(&profiles, chain),
            None => vec![],
        };

        let valid = profiles.valid.clone().unwrap_or_default();

        profile_spec_chains.extend(chain); // Profile 里的 Chain -> 全局 Chain

        (current_mapping, profile_spec_chains, valid)
    };

    let mut result_map = IndexMap::new(); // 保存脚本日志
    let mut exists_keys = use_keys(&config); // 保存出现过的keys

    let valid = use_valid_fields(valid);
    config = use_filter(config, &valid, enable_filter);

    // 处理用户的profile
    let mut script_runner = RunnerManager::new();
    for item in chains.into_iter() {
        // TODO: 想一个更好的办法，避免内存拷贝
        match item.data {
            ChainTypeWrapper::Merge(merge) => {
                let mut logs = vec![];
                exists_keys.extend(use_keys(&merge));
                let (res, process_logs) = use_merge(merge, config.to_owned());
                config = res.unwrap();
                config = use_filter(config.to_owned(), &valid, enable_filter);
                logs.extend(process_logs);
                result_map.insert(item.uid, logs);
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
                        exists_keys.extend(use_keys(&res_config));
                        config = use_filter(res_config, &valid, enable_filter);
                    }
                    Err(err) => logs.error(err.to_string()),
                }
                // TODO: 这里添加对 field 的检查，触发 WARN 日记。此外，需要对 Merge 的结果进行检查？
                result_map.insert(item.uid, logs);
            }
        }
    }

    // 合并默认的config
    clash_config
        .iter()
        // only guarded fields should be overwritten
        .filter(|(k, _)| HANDLE_FIELDS.contains(&k.as_str().unwrap_or_default()))
        .for_each(|(key, value)| {
            config.insert(key.to_owned(), value.clone());
        });

    let clash_fields = use_clash_fields();

    // 内建脚本最后跑
    if enable_builtin {
        for item in ChainItem::builtin()
            .into_iter()
            .filter(|(s, _)| s.contains(*clash_core.as_ref().unwrap_or(&ClashCore::default())))
            .map(|(_, c)| c)
        {
            log::debug!(target: "app", "run builtin script {}", item.uid);

            if let ChainTypeWrapper::Script(script) = item.data {
                let (res, _) = script_runner
                    .process_script(script, config.to_owned())
                    .await;
                match res {
                    Ok(res_config) => {
                        config = use_filter(res_config, &clash_fields, enable_filter);
                    }
                    Err(err) => {
                        log::error!(target: "app", "builtin script error `{err}`");
                    }
                }
            }
        }
    }

    config = use_filter(config, &clash_fields, enable_filter);
    config = use_tun(config, enable_tun);
    config = use_sort(config, enable_filter);

    let mut exists_set = HashSet::new();
    exists_set.extend(exists_keys.into_iter().filter(|s| clash_fields.contains(s)));
    exists_keys = exists_set.into_iter().collect();

    (config, exists_keys, result_map)
}
