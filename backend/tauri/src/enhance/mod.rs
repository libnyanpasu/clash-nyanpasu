mod advice;
mod chain;
mod field;
mod merge;
mod script;
mod tun;
mod utils;

pub use self::chain::ScriptType;
use self::{chain::*, field::*, merge::*, script::*, tun::*};
use crate::config::{nyanpasu::ClashCore, Config, ProfileSharedGetter};
pub use chain::PostProcessingOutput;
use futures::future::join_all;
use indexmap::IndexMap;
use serde_yaml::Mapping;
use std::collections::HashSet;
use utils::{merge_profiles, process_chain};
pub use utils::{Logs, LogsExt};

/// Enhance mode
/// 返回最终配置、该配置包含的键、和script执行的结果
pub async fn enhance() -> (Mapping, Vec<String>, PostProcessingOutput) {
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
    let (profiles, profile_chain, global_chain, valid) = {
        let profiles = Config::profiles();
        let profiles = profiles.latest();

        let profile_chain_mapping = profiles
            .get_current()
            .iter()
            .filter_map(|uid| profiles.get_item(uid).ok())
            .map(|item| {
                (
                    item.uid().to_string(),
                    match item {
                        profile if profile.is_local() => {
                            let profile = profile.as_local().unwrap();
                            utils::convert_uids_to_scripts(&profiles, &profile.chain)
                        }
                        profile if profile.is_remote() => {
                            let profile = profile.as_remote().unwrap();
                            utils::convert_uids_to_scripts(&profiles, &profile.chain)
                        }
                        _ => vec![],
                    },
                )
            })
            .collect::<IndexMap<_, _>>();

        let current_mappings = profiles
            .current_mappings()
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<IndexMap<_, _>>();

        let global_chain = utils::convert_uids_to_scripts(&profiles, &profiles.chain);

        let valid = profiles.valid.clone();

        (current_mappings, profile_chain_mapping, global_chain, valid)
    };

    let mut postprocessing_output = PostProcessingOutput::default();

    let valid = use_valid_fields(&valid);

    // 执行 scoped chain
    let profiles_outputs = join_all(profiles.into_iter().map(|(uid, mapping)| async {
        let chain = profile_chain.get(&uid).map_or(&[] as &[_], |v| v);
        let output = process_chain(mapping, chain).await;
        (uid, output)
    }))
    .await;

    let mut profiles = IndexMap::new();
    for (uid, (config, output)) in profiles_outputs {
        postprocessing_output.scopes.insert(uid.to_string(), output);
        profiles.insert(uid.to_string(), config);
    }

    // 合并多个配置
    // TODO: 此步骤需要提供针对每个配置的 Meta 信息
    // TODO: 需要支持自定义合并逻辑
    let config = merge_profiles(profiles);

    // 执行全局 chain
    let (mut config, global_chain_output) = process_chain(config, &global_chain).await;
    postprocessing_output.global = global_chain_output;

    // 记录当前配置包含的键
    let mut exists_keys = use_keys(&config);
    config = use_whitelist_fields_filter(config, &valid, enable_filter);

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
        let mut script_runner = RunnerManager::new();
        for item in ChainItem::builtin()
            .into_iter()
            .filter(|(s, _)| s.contains(*clash_core.as_ref().unwrap_or(&ClashCore::default())))
            .map(|(_, c)| c)
        {
            log::debug!(target: "app", "run builtin script {}", item.uid);

            if let ChainTypeWrapper::Script(script) = item.data {
                let (res, _) = script_runner
                    .process_script(&script, config.to_owned())
                    .await;
                match res {
                    Ok(res_config) => {
                        config = res_config;
                    }
                    Err(err) => {
                        log::error!(target: "app", "builtin script error `{err:?}`");
                    }
                }
            }
        }
    }

    config = use_whitelist_fields_filter(config, &clash_fields, enable_filter);
    config = use_tun(config, enable_tun);
    config = use_cache(config);
    config = use_sort(config, enable_filter);

    let (_, logs) = advice::chain_advice(&config);
    postprocessing_output.advice = logs;

    let mut exists_set = HashSet::new();
    exists_set.extend(exists_keys.into_iter().filter(|s| clash_fields.contains(s)));
    exists_keys = exists_set.into_iter().collect();

    (config, exists_keys, postprocessing_output)
}

fn use_cache(mut config: Mapping) -> Mapping {
    if !config.contains_key("profile") {
        tracing::debug!("Don't detect profile, set default profile for memorized profile");
        let mut profile = Mapping::new();
        profile.insert("store-selected".into(), true.into());
        // Disable fake-ip store, due to the slow speed.
        // each dns query should indirect to the file io, which is very very slow.
        profile.insert("store-fake-ip".into(), false.into());
        config.insert("profile".into(), profile.into());
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_use_cache() {
        let config = Mapping::new();
        dbg!(&config);
        let config = use_cache(config);
        dbg!(&config);
        assert!(config.contains_key("profile"));

        let mut config = Mapping::new();
        let mut profile = Mapping::new();
        profile.insert("do-not-override".into(), true.into());
        config.insert("profile".into(), profile.into());
        dbg!(&config);
        let config = use_cache(config);
        dbg!(&config);
        assert!(config.contains_key("profile"));
        assert!(config
            .get("profile")
            .unwrap()
            .as_mapping()
            .unwrap()
            .contains_key("do-not-override"));
    }
}
