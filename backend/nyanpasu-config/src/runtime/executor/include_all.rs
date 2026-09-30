//! IncludeAllExpansion: expands `include-all` / `include-all-proxies` /
//! `include-all-providers` into explicit `proxies` / `use`, mirroring mihomo
//! `ParseProxyGroup` (adapter/outboundgroup/parser.go) and the `AllProxies` /
//! `AllProviders` lists built by `config.parseProxies` (config/config.go).
//! `filter` / `exclude-filter` / `exclude-type` stay on the group: mihomo
//! applies them at runtime, so the expanded group behaves like the native one.

use std::sync::Arc;

use fancy_regex::Regex;

use crate::runtime::value::{ConfigObject, ConfigValue};

use super::{
    StepLogEntry,
    value_util::{obj_get, obj_insert},
};

const INCLUDE_ALL: &str = "include-all";
const INCLUDE_ALL_PROXIES: &str = "include-all-proxies";
const INCLUDE_ALL_PROVIDERS: &str = "include-all-providers";

pub(super) fn expand_include_all(config: &ConfigValue) -> (ConfigValue, Vec<StepLogEntry>) {
    let mut logs = Vec::new();
    let Some(groups) = obj_get(config, "proxy-groups").and_then(ConfigValue::as_array_arc) else {
        return (config.clone(), logs);
    };

    // mihomo sorts both lists before handing them to every group.
    let mut all_proxies: Vec<Arc<str>> = obj_get(config, "proxies")
        .and_then(ConfigValue::as_array_arc)
        .map(|proxies| {
            proxies
                .iter()
                .filter_map(|proxy| match proxy.as_object_arc()?.get("name")? {
                    ConfigValue::String(name) => Some(name.clone()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    all_proxies.sort();
    let mut all_providers: Vec<Arc<str>> = obj_get(config, "proxy-providers")
        .and_then(ConfigValue::as_object_arc)
        .map(|providers| providers.keys().cloned().collect())
        .unwrap_or_default();
    all_providers.sort();

    let rebuilt: Vec<ConfigValue> = groups
        .iter()
        .map(|group| {
            let Some(map) = group.as_object_arc() else {
                return group.clone();
            };
            match expand_group(map, &all_proxies, &all_providers) {
                Ok(Some(next)) => ConfigValue::Object(Arc::new(next)),
                Ok(None) => group.clone(),
                Err(message) => {
                    logs.push(StepLogEntry::error(message));
                    group.clone()
                }
            }
        })
        .collect();
    let next = obj_insert(
        config,
        "proxy-groups",
        ConfigValue::Array(Arc::from(rebuilt)),
    );
    (next, logs)
}

/// `Ok(None)` leaves the group as-is: nothing to expand, or a shape mihomo
/// itself rejects, which is left for the core to report.
fn expand_group(
    map: &ConfigObject,
    all_proxies: &[Arc<str>],
    all_providers: &[Arc<str>],
) -> Result<Option<ConfigObject>, String> {
    let Some(include_all) = weak_bool(map.get(INCLUDE_ALL)) else {
        return Ok(None);
    };
    let Some(include_proxies) = weak_bool(map.get(INCLUDE_ALL_PROXIES)) else {
        return Ok(None);
    };
    let Some(include_providers) = weak_bool(map.get(INCLUDE_ALL_PROVIDERS)) else {
        return Ok(None);
    };
    let include_proxies = include_proxies || include_all;
    let include_providers = include_providers || include_all;
    if !include_proxies && !include_providers {
        return Ok(None);
    }
    let Some(ConfigValue::String(name)) = map.get("name") else {
        return Ok(None);
    };

    let mut proxies: Vec<ConfigValue> = match map.get("proxies") {
        None | Some(ConfigValue::Null) => Vec::new(),
        Some(ConfigValue::Array(items)) => items.to_vec(),
        Some(_) => return Ok(None),
    };
    let mut uses: Vec<ConfigValue> = match map.get("use") {
        None | Some(ConfigValue::Null) => Vec::new(),
        Some(ConfigValue::Array(items)) => items.to_vec(),
        Some(_) => return Ok(None),
    };

    if include_providers {
        uses = all_providers
            .iter()
            .map(|provider| ConfigValue::String(provider.clone()))
            .collect();
    }
    if include_proxies {
        let filter = match map.get("filter") {
            None | Some(ConfigValue::Null) => String::new(),
            Some(value) => weak_string(value).ok_or_else(|| {
                format!(
                    "proxy group `{name}`: `filter` is not a string, include-all left to the core"
                )
            })?,
        };
        if filter.is_empty() {
            proxies.extend(
                all_proxies
                    .iter()
                    .map(|proxy| ConfigValue::String(proxy.clone())),
            );
        } else {
            let regs = filter
                .split('`')
                .map(|pattern| {
                    Regex::new(pattern).map_err(|error| {
                        format!(
                            "proxy group `{name}`: invalid filter `{pattern}` ({error}), include-all left to the core"
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            // Same loop shape as mihomo: a proxy matching several filters is
            // appended once per match, and a match error counts as no match.
            for proxy in all_proxies {
                for reg in &regs {
                    if reg.is_match(proxy.as_ref()).unwrap_or(false) {
                        proxies.push(ConfigValue::String(proxy.clone()));
                    }
                }
            }
        }
        if proxies.is_empty() && uses.is_empty() {
            let fallback = map
                .get("empty-fallback")
                .and_then(weak_string)
                .filter(|fallback| !fallback.is_empty())
                .unwrap_or_else(|| "COMPATIBLE".to_string());
            proxies.push(ConfigValue::String(Arc::from(fallback)));
        }
    }

    let mut next = map.clone();
    for key in [INCLUDE_ALL, INCLUDE_ALL_PROXIES, INCLUDE_ALL_PROVIDERS] {
        next.shift_remove(key);
    }
    if include_proxies {
        next.insert(Arc::from("proxies"), ConfigValue::Array(Arc::from(proxies)));
    }
    if include_providers {
        if uses.is_empty() {
            next.shift_remove("use");
        } else {
            next.insert(Arc::from("use"), ConfigValue::Array(Arc::from(uses)));
        }
    }
    Ok(Some(next))
}

/// mihomo's weakly typed bool decode: a bool, or a non-zero integer. `None`
/// marks a value mihomo fails to decode.
fn weak_bool(value: Option<&ConfigValue>) -> Option<bool> {
    match value {
        None | Some(ConfigValue::Null) => Some(false),
        Some(ConfigValue::Bool(flag)) => Some(*flag),
        Some(ConfigValue::Number(number)) => number
            .as_i64()
            .map(|n| n != 0)
            .or_else(|| number.as_u64().map(|n| n != 0)),
        Some(_) => None,
    }
}

/// mihomo's weakly typed string decode for the integer forms YAML produces.
fn weak_string(value: &ConfigValue) -> Option<String> {
    match value {
        ConfigValue::String(text) => Some(text.to_string()),
        ConfigValue::Number(number) if number.is_i64() || number.is_u64() => {
            Some(number.to_string())
        }
        _ => None,
    }
}
