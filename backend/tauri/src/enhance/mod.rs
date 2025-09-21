mod advice;
mod chain;
mod field;
mod merge;
mod script;
mod tun;
mod utils;

pub use self::chain::ScriptType;
use self::{chain::*, field::*, merge::*, script::*, tun::*};
use crate::config::{Config, ProfileMetaGetter, nyanpasu::ClashCore};
pub use chain::PostProcessingOutput;
use futures::future::join_all;
use indexmap::IndexMap;
use serde_yaml::{Mapping, Value};
use std::collections::HashSet;
pub use utils::{Logs, LogsExt};
use utils::{merge_profiles, process_chain};

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
    config = use_include_all_proxy_groups(config);
    config = use_cache(config);
    config = use_sort(config, enable_filter);

    let (_, logs) = advice::chain_advice(&config);
    postprocessing_output.advice = logs;

    let mut exists_set = HashSet::new();
    exists_set.extend(exists_keys.into_iter().filter(|s| clash_fields.contains(s)));
    exists_keys = exists_set.into_iter().collect();

    (config, exists_keys, postprocessing_output)
}

/// Process proxy groups with include-all field
fn use_include_all_proxy_groups(mut config: Mapping) -> Mapping {
    // Collect all proxy names from proxies and proxy-providers first (before mutable borrow)
    let mut all_proxy_names = Vec::new();

    // Collect from proxies section
    if let Some(proxies_value) = config.get("proxies") {
        if let Some(proxies_seq) = proxies_value.as_sequence() {
            for proxy in proxies_seq {
                if let Some(proxy_map) = proxy.as_mapping() {
                    if let Some(name_value) = proxy_map.get("name") {
                        if let Some(name) = name_value.as_str() {
                            all_proxy_names.push(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // Collect from proxy-providers section
    if let Some(providers_value) = config.get("proxy-providers") {
        if let Some(providers_map) = providers_value.as_mapping() {
            for (provider_name, _) in providers_map {
                if let Some(name) = provider_name.as_str() {
                    all_proxy_names.push(name.to_string());
                }
            }
        }
    }

    // Check if we have proxy-groups field
    if let Some(proxy_groups_value) = config.get_mut("proxy-groups") {
        if let Some(proxy_groups) = proxy_groups_value.as_sequence_mut() {
            // Process each proxy group
            for group in proxy_groups.iter_mut() {
                if let Some(group_map) = group.as_mapping_mut() {
                    // Check if this group has include-all: true
                    if let Some(include_all_value) = group_map.get("include-all") {
                        if include_all_value.as_bool().unwrap_or(false) {
                            // Check if this is the GLOBAL group or any group with include-all
                            if let Some(name_value) = group_map.get("name") {
                                let _name = name_value.as_str().unwrap_or("");
                                // Preserve existing proxies
                                let mut existing_proxies = Vec::new();
                                if let Some(existing) = group_map.get("proxies") {
                                    if let Some(existing_seq) = existing.as_sequence() {
                                        for proxy in existing_seq {
                                            if let Some(proxy_str) = proxy.as_str() {
                                                existing_proxies.push(proxy_str.to_string());
                                            }
                                        }
                                    }
                                }

                                // Create new proxies list with all proxies
                                let mut new_proxies = Vec::new();

                                // Add all collected proxy names
                                for proxy_name in &all_proxy_names {
                                    new_proxies.push(Value::String(proxy_name.clone()));
                                }

                                // Add existing proxies that aren't in the all list
                                for existing_proxy in existing_proxies {
                                    if !all_proxy_names.contains(&existing_proxy) {
                                        new_proxies.push(Value::String(existing_proxy));
                                    }
                                }

                                // Update the proxies field
                                group_map.insert(
                                    Value::String("proxies".to_string()),
                                    Value::Sequence(new_proxies),
                                );

                                // Remove the include-all field since it's been processed
                                group_map.remove("include-all");
                            }
                        }
                    }
                }
            }
        }
    }
    config
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
        assert!(
            config
                .get("profile")
                .unwrap()
                .as_mapping()
                .unwrap()
                .contains_key("do-not-override")
        );
    }

    #[test]
    fn test_use_include_all_proxy_groups() {
        let yaml = r#"
proxies:
  - name: "Proxy1"
    type: ss
    server: server.com
    port: 443
  - name: "Proxy2"
    type: vmess
    server: server2.com
    port: 8080
proxy-providers:
  provider1:
    type: http
    url: "http://example.com/provider1.yaml"
    interval: 3600
  provider2:
    type: file
    path: ./providers/provider2.yaml
proxy-groups:
  - name: GLOBAL
    type: select
    include-all: true
    proxies:
      - DIRECT
  - name: Proxies
    type: select
    proxies:
      - DIRECT
"#;
        let config: Mapping = serde_yaml::from_str(yaml).unwrap();
        let result = use_include_all_proxy_groups(config);

        // Check that GLOBAL group now contains all proxies
        let proxy_groups = result.get("proxy-groups").unwrap().as_sequence().unwrap();
        let global_group = proxy_groups
            .iter()
            .find(|group| {
                if let Some(mapping) = group.as_mapping() {
                    if let Some(name) = mapping.get("name") {
                        return name.as_str().unwrap() == "GLOBAL";
                    }
                }
                false
            })
            .unwrap();

        // Check that include-all field was removed
        assert!(
            global_group
                .as_mapping()
                .unwrap()
                .get("include-all")
                .is_none()
        );

        let global_proxies = global_group
            .as_mapping()
            .unwrap()
            .get("proxies")
            .unwrap()
            .as_sequence()
            .unwrap();
        let proxy_names: Vec<&str> = global_proxies.iter().map(|p| p.as_str().unwrap()).collect();

        // Should contain all proxies from the config
        assert!(proxy_names.contains(&"Proxy1"));
        assert!(proxy_names.contains(&"Proxy2"));
        assert!(proxy_names.contains(&"provider1"));
        assert!(proxy_names.contains(&"provider2"));
        // Should still contain original proxies
        assert!(proxy_names.contains(&"DIRECT"));
    }
}
