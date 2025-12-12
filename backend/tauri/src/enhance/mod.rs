mod advice;
mod chain;
mod field;
mod merge;
mod script;
mod tun;
mod utils;

pub use self::chain::ScriptType;
use self::{chain::*, field::*, merge::*, script::*, tun::*};
use crate::config::{
    ClashGuardOverrides, ProfileContentGuard,
    nyanpasu::ClashCore,
    snapshot::{
        ChainNodeKind, ConfigSnapshot, ConfigSnapshotState, ConfigSnapshotsBuilder, ProcessKind,
    },
};
pub use chain::PostProcessingOutput;
use futures::future::join_all;
use indexmap::IndexMap;
use serde_yaml::{Mapping, Value};
use std::collections::HashSet;
pub use utils::{Logs, LogsExt};
use utils::{merge_profiles, process_chain};

#[derive(Debug, Clone)]
pub struct EnhanceOptions {
    pub clash_core: ClashCore,
    pub enable_tun: bool,
    pub enable_builtin_enhanced: bool,
    pub enable_clash_fields: bool,
}

pub struct PartialProfileItem<'a, 's: 'a> {
    pub profile_id: String,
    pub profile_config: &'a Mapping,
    pub scoped_chain: &'a [ProfileContentGuard<'s>],
}

pub struct EnhanceResult {
    pub config: Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
    pub snapshots: ConfigSnapshotState,
}

/// Enhance mode
/// 返回最终配置、该配置包含的键、和script执行的结果
#[tracing::instrument(skip_all)]
pub async fn process<'i, 'r: 'i, 's: 'i>(
    EnhanceOptions {
        clash_core,
        enable_tun,
        enable_builtin_enhanced: enable_builtin,
        enable_clash_fields: enable_filter,
    }: EnhanceOptions,
    valid_fields: &[String],
    // (profile_id, profile_config)
    selected_profiles: &'i [PartialProfileItem<'r, 's>],
    global_chain: &'i [ProfileContentGuard<'s>],
    guarded_config: &ClashGuardOverrides,
) -> EnhanceResult {
    let mut postprocessing_output = PostProcessingOutput::default();

    let valid_fields = use_valid_fields(&valid_fields);

    let [primary_profile, secondary_profiles @ ..] = selected_profiles else {
        unreachable!("selected profiles always contain at least one profile");
    };
    let mut snapshots_builder = ConfigSnapshotsBuilder::new(
        ConfigSnapshot::new_unchanged(primary_profile.profile_config.clone()),
        primary_profile.profile_id.clone(),
    );

    let tasks = selected_profiles
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            let profile_id = &item.profile_id;
            let profile_config = item.profile_config;
            let scoped_chain = item.scoped_chain;
            let is_primary = idx == 0;

            let scoped_chain = scoped_chain
                .iter()
                .flat_map(Into::<Option<ChainItem>>::into)
                .collect::<Vec<ChainItem>>();

            let mut subtree = if is_primary {
                snapshots_builder.new_subtree(snapshots_builder.root_node_id())
            } else {
                let node_id = snapshots_builder.add_node_to_current(ConfigSnapshotState::new(
                    ConfigSnapshot::new_unchanged(profile_config.clone()),
                    ProcessKind::SecondaryProcessing {
                        profile_id: profile_id.clone(),
                    },
                    None,
                ));
                snapshots_builder.new_subtree(node_id)
            };

            Some(async move {
                let result = process_chain(
                    profile_config.clone(),
                    scoped_chain.as_slice(),
                    &mut subtree,
                    ChainNodeKind::Scoped {
                        parent_profile_id: profile_id.clone(),
                    },
                )
                .await;

                (profile_id.as_str(), result, subtree, is_primary)
            })
        });

    // 执行 scoped chain
    let profiles_outputs = join_all(tasks).await;

    let mut profiles = IndexMap::new();
    for (uid, result, subtree, is_primary) in profiles_outputs {
        postprocessing_output
            .scopes
            .insert(uid.to_string(), result.logs);
        profiles.insert(uid.to_string(), result.config);
        let node_ids =
            snapshots_builder.add_leaf_from_subtree(snapshots_builder.root_node_id(), subtree);
        // TODO: support graph merge strategy
        if let Some(last_node_id) = node_ids.last() {
            snapshots_builder.set_current(*last_node_id);
        }
    }

    // 合并多个配置
    // TODO: 此步骤需要提供针对每个配置的 Meta 信息
    // TODO: 需要支持自定义合并逻辑
    let config = merge_profiles(profiles);
    let merge_snapshot =
        ConfigSnapshot::new_with_diff(primary_profile.profile_config, config.clone());
    snapshots_builder.push_node(ConfigSnapshotState::new(
        merge_snapshot,
        ProcessKind::SelectedProfilesProxiesMerge {
            primary_profile_id: primary_profile.profile_id.clone(),
            other_profiles_ids: secondary_profiles
                .iter()
                .map(|p| p.profile_id.clone())
                .collect(),
        },
        None,
    ));

    let global_chain = global_chain
        .iter()
        .flat_map(Into::<Option<ChainItem>>::into)
        .collect::<Vec<ChainItem>>();

    // 执行全局 chain
    let result = process_chain(
        config,
        &global_chain,
        &mut snapshots_builder,
        ChainNodeKind::Global,
    )
    .await;
    postprocessing_output.global = result.logs;
    let mut config = result.config;

    // 记录当前配置包含的键
    let mut exists_keys = use_keys(&config);
    if enable_filter {
        let new_config = use_whitelist_fields_filter(config.clone(), &valid_fields);
        snapshots_builder.push_node(ConfigSnapshotState::new(
            ConfigSnapshot::new_with_diff(&config, new_config.clone()),
            ProcessKind::WhitelistFieldFilter,
            None,
        ));
        config = new_config;
    }

    // 合并保护配置
    let serde_yaml::Value::Mapping(guarded_config_map) =
        serde_yaml::to_value(guarded_config).expect("failed to convert guarded config to yaml")
    else {
        unreachable!("guarded config should be a mapping");
    };

    let new_config = config.clone();
    crate::utils::yaml::apply_overrides(&mut new_config.clone(), &guarded_config_map);
    snapshots_builder.push_node(ConfigSnapshotState::new(
        ConfigSnapshot::new_with_diff(&config, new_config.clone()),
        ProcessKind::GuardOverrides,
        None,
    ));
    config = new_config;

    let clash_fields = use_clash_fields();

    // 内建脚本最后跑
    if enable_builtin {
        let mut script_runner = RunnerManager::new();
        for item in ChainItem::builtin()
            .into_iter()
            .filter(|(s, _)| s.contains(clash_core))
            .map(|(_, c)| c)
        {
            tracing::debug!(target: "app", "run builtin script {}", item.uid);

            if let ChainTypeWrapper::Script {
                kind: script_type,
                data,
            } = item.data
            {
                let (res, _) = script_runner
                    .process_script(script_type, &data, config.to_owned())
                    .await;
                match res {
                    Ok(res_config) => {
                        snapshots_builder.push_node(ConfigSnapshotState::new(
                            ConfigSnapshot::new_with_diff(&config, res_config.clone()),
                            ProcessKind::BuiltinChain {
                                name: item.uid.clone(),
                            },
                            None,
                        ));
                        config = res_config;
                    }
                    Err(err) => {
                        tracing::error!(target: "app", "builtin script error `{err:?}`");
                    }
                }
            }
        }
    }

    if enable_filter {
        let new_config = use_whitelist_fields_filter(config.clone(), &clash_fields);
        snapshots_builder.push_node(ConfigSnapshotState::new(
            ConfigSnapshot::new_with_diff(&config, new_config.clone()),
            ProcessKind::WhitelistFieldFilter,
            None,
        ));
        config = new_config;
    }

    let previous_config = config.clone();
    config = use_tun(config, enable_tun);
    config = use_include_all_proxy_groups(config);
    config = use_cache(config);
    config = use_sort(config, enable_filter);
    snapshots_builder.push_node(ConfigSnapshotState::new(
        ConfigSnapshot::new_with_diff(&previous_config, config.clone()),
        ProcessKind::Finalizing,
        None,
    ));

    let (_, logs) = advice::chain_advice(&config);
    postprocessing_output.advice = logs;

    let mut exists_set = HashSet::new();
    exists_set.extend(exists_keys.into_iter().filter(|s| clash_fields.contains(s)));
    exists_keys = exists_set.into_iter().collect();

    EnhanceResult {
        config,
        exists_keys,
        postprocessing_output,
        snapshots: snapshots_builder.build(),
    }
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
