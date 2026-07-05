//! WhitelistFieldFilter / GuardOverrides / Finalizing (spec 7.4).
//! Constants transplanted verbatim from enhance/field.rs:4-56.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::runtime::value::{ConfigObject, ConfigValue};

use super::{
    GuardInputs, TunFlavor, TunParams,
    error::RuntimePipelineError,
    value_util::{obj_get, obj_insert},
};

pub(super) const HANDLE_FIELDS: [&str; 9] = [
    "mode",
    "port",
    "socks-port",
    "mixed-port",
    "allow-lan",
    "log-level",
    "ipv6",
    "secret",
    "external-controller",
];

pub(super) const DEFAULT_FIELDS: [&str; 5] = [
    "proxies",
    "proxy-groups",
    "proxy-providers",
    "rules",
    "rule-providers",
];

pub(super) const OTHERS_FIELDS: [&str; 31] = [
    "dns",
    "tun",
    "ebpf",
    "hosts",
    "script",
    "profile",
    "payload",
    "tunnels",
    "auto-redir",
    "experimental",
    "interface-name",
    "routing-mark",
    "redir-port",
    "tproxy-port",
    "iptables",
    "external-ui",
    "bind-address",
    "authentication",
    "tls",
    "sniffer",
    "geox-url",
    "listeners",
    "sub-rules",
    "geodata-mode",
    "unified-delay",
    "tcp-concurrent",
    "enable-process",
    "find-process-mode",
    "skip-auth-prefixes",
    "external-controller-tls",
    "global-client-fingerprint",
];

/// 45-key full whitelist, DEFAULT ++ HANDLE ++ OTHERS (field.rs:58-65 order).
pub(super) fn known_fields() -> impl Iterator<Item = &'static str> {
    DEFAULT_FIELDS
        .into_iter()
        .chain(HANDLE_FIELDS)
        .chain(OTHERS_FIELDS)
}

/// Stage-1 list: (valid intersect OTHERS, lowercased) ++ DEFAULT; HANDLE
/// never included. Guarded fields must not come from profile output.
pub(super) fn stage1_fields(valid: &[String]) -> Vec<String> {
    let mut fields: Vec<String> = valid
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .filter(|field| OTHERS_FIELDS.contains(&field.as_str()))
        .collect();
    fields.extend(DEFAULT_FIELDS.iter().map(|field| (*field).to_string()));
    fields
}

pub(super) fn whitelist_filter(
    config: &ConfigValue,
    allow: &[String],
    enabled: bool,
) -> ConfigValue {
    if !enabled {
        return config.clone();
    }
    let Some(map) = config.as_object_arc() else {
        return config.clone();
    };
    let filtered: ConfigObject = map
        .iter()
        .filter(|(key, _)| allow.iter().any(|allowed| allowed.as_str() == key.as_ref()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    ConfigValue::Object(Arc::new(filtered))
}

/// Legacy HANDLE overwrite (enhance/mod.rs:110-116): typed overrides + caller
/// resolved ports force-inserted at top level, bypassing stage-1 whitelist.
pub(super) fn apply_guard(
    config: &ConfigValue,
    guard: &GuardInputs<'_>,
) -> Result<ConfigValue, RuntimePipelineError> {
    let raw = serde_json::to_value(guard.overrides).map_err(|error| {
        RuntimePipelineError::Internal(format!("encode guard overrides: {error}"))
    })?;
    let entries = ConfigValue::try_from(raw).map_err(|error| {
        RuntimePipelineError::Internal(format!("convert guard overrides: {error:?}"))
    })?;

    let mut next = config.clone();
    if let Some(map) = entries.as_object_arc() {
        for (key, value) in map.iter() {
            next = obj_insert(&next, key.as_ref(), value.clone());
        }
    }

    next = obj_insert(
        &next,
        "mixed-port",
        ConfigValue::Number(serde_json::Number::from(guard.ports.mixed_port)),
    );
    if let Some(port) = guard.ports.port {
        next = obj_insert(
            &next,
            "port",
            ConfigValue::Number(serde_json::Number::from(port)),
        );
    }
    if let Some(port) = guard.ports.socks_port {
        next = obj_insert(
            &next,
            "socks-port",
            ConfigValue::Number(serde_json::Number::from(port)),
        );
    }
    if let Some(controller) = &guard.ports.external_controller {
        next = obj_insert(
            &next,
            "external-controller",
            ConfigValue::String(Arc::from(controller.as_str())),
        );
    }
    Ok(next)
}

/// Finalizing composite node (spec 7.4): stage-2 filter, tun, include-all,
/// cache, then sort. One recorded node; changed_fields shows the net effect.
pub(super) fn finalize(
    config: &ConfigValue,
    tun: &TunParams,
    whitelist_enabled: bool,
) -> ConfigValue {
    let full: Vec<String> = known_fields().map(str::to_string).collect();
    let mut next = whitelist_filter(config, &full, whitelist_enabled);
    next = apply_tun(&next, tun);
    next = apply_include_all(&next);
    next = apply_cache(&next);
    apply_sort(&next)
}

fn object_of(value: Option<&ConfigValue>) -> ConfigObject {
    value
        .and_then(ConfigValue::as_object_arc)
        .map(|map| (**map).clone())
        .unwrap_or_default()
}

fn append_default(map: &mut ConfigObject, key: &str, value: ConfigValue) {
    if !map.contains_key(key) {
        map.insert(Arc::from(key), value);
    }
}

fn string_value(text: &str) -> ConfigValue {
    ConfigValue::String(Arc::from(text))
}

fn string_list(items: &[&str]) -> ConfigValue {
    ConfigValue::Array(Arc::from(
        items
            .iter()
            .map(|item| string_value(item))
            .collect::<Vec<_>>(),
    ))
}

/// Mirrors enhance/tun.rs:26-109; `revise!` overwrites and `append!` fills
/// missing keys only.
fn apply_tun(config: &ConfigValue, params: &TunParams) -> ConfigValue {
    let existing = obj_get(config, "tun");
    if !params.enable && existing.is_none() {
        return config.clone();
    }

    let mut tun = object_of(existing);
    tun.insert(Arc::from("enable"), ConfigValue::Bool(params.enable));
    if params.enable {
        match params.flavor {
            TunFlavor::ClashRs => {
                append_default(&mut tun, "device-id", string_value("dev://utun1989"));
                append_default(&mut tun, "auto-route", ConfigValue::Bool(true));
            }
            TunFlavor::Standard { stack } => {
                append_default(&mut tun, "stack", string_value(stack.as_ref()));
                append_default(&mut tun, "dns-hijack", string_list(&["any:53"]));
                append_default(&mut tun, "auto-route", ConfigValue::Bool(true));
                append_default(&mut tun, "auto-detect-interface", ConfigValue::Bool(true));
            }
        }
    }

    let mut next = obj_insert(config, "tun", ConfigValue::Object(Arc::new(tun)));
    if params.enable {
        next = apply_tun_dns(&next, params.windows_fake_ip_filter);
    }
    next
}

fn apply_tun_dns(config: &ConfigValue, windows_fake_ip_filter: bool) -> ConfigValue {
    let mut dns = object_of(obj_get(config, "dns"));
    dns.insert(Arc::from("enable"), ConfigValue::Bool(true));
    append_default(&mut dns, "enhanced-mode", string_value("fake-ip"));
    append_default(&mut dns, "fake-ip-range", string_value("198.18.0.1/16"));
    append_default(
        &mut dns,
        "nameserver",
        string_list(&["114.114.114.114", "223.5.5.5", "8.8.8.8"]),
    );
    append_default(
        &mut dns,
        "fallback",
        ConfigValue::Array(Arc::from(Vec::<ConfigValue>::new())),
    );
    if windows_fake_ip_filter {
        append_default(
            &mut dns,
            "fake-ip-filter",
            string_list(&[
                "dns.msftncsi.com",
                "www.msftncsi.com",
                "www.msftconnecttest.com",
            ]),
        );
    }
    obj_insert(config, "dns", ConfigValue::Object(Arc::new(dns)))
}

/// Mirrors enhance/mod.rs:163-248.
fn apply_include_all(config: &ConfigValue) -> ConfigValue {
    let mut names: Vec<String> = Vec::new();
    if let Some(proxies) = obj_get(config, "proxies").and_then(ConfigValue::as_array_arc) {
        for proxy in proxies.iter() {
            if let Some(ConfigValue::String(name)) =
                proxy.as_object_arc().and_then(|map| map.get("name"))
            {
                names.push(name.to_string());
            }
        }
    }
    if let Some(providers) = obj_get(config, "proxy-providers").and_then(ConfigValue::as_object_arc)
    {
        names.extend(providers.keys().map(|key| key.to_string()));
    }

    let Some(groups) = obj_get(config, "proxy-groups").and_then(ConfigValue::as_array_arc) else {
        return config.clone();
    };
    let rebuilt: Vec<ConfigValue> = groups
        .iter()
        .map(|group| {
            let Some(map) = group.as_object_arc() else {
                return group.clone();
            };
            let include_all = matches!(map.get("include-all"), Some(ConfigValue::Bool(true)));
            if !include_all || !map.contains_key("name") {
                return group.clone();
            }
            let existing: Vec<String> = map
                .get("proxies")
                .and_then(ConfigValue::as_array_arc)
                .map(|seq| {
                    seq.iter()
                        .filter_map(|value| match value {
                            ConfigValue::String(name) => Some(name.to_string()),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();

            let mut proxies: Vec<ConfigValue> =
                names.iter().map(|name| string_value(name)).collect();
            proxies.extend(
                existing
                    .iter()
                    .filter(|name| !names.contains(name))
                    .map(|name| string_value(name)),
            );

            let mut next = (**map).clone();
            next.insert(Arc::from("proxies"), ConfigValue::Array(Arc::from(proxies)));
            next.shift_remove("include-all");
            ConfigValue::Object(Arc::new(next))
        })
        .collect();
    obj_insert(
        config,
        "proxy-groups",
        ConfigValue::Array(Arc::from(rebuilt)),
    )
}

/// Mirrors enhance/mod.rs:250-261: existing `profile` keys are not merged.
fn apply_cache(config: &ConfigValue) -> ConfigValue {
    if obj_get(config, "profile").is_some() {
        return config.clone();
    }
    let mut profile: ConfigObject = IndexMap::new();
    profile.insert(Arc::from("store-selected"), ConfigValue::Bool(true));
    profile.insert(Arc::from("store-fake-ip"), ConfigValue::Bool(false));
    obj_insert(config, "profile", ConfigValue::Object(Arc::new(profile)))
}

/// Mirrors field.rs:115-147: known keys use HANDLE++OTHERS++DEFAULT order;
/// unknown keys keep their original relative order at the end.
fn apply_sort(config: &ConfigValue) -> ConfigValue {
    let Some(map) = config.as_object_arc() else {
        return config.clone();
    };
    let mut next: ConfigObject = IndexMap::new();
    for key in HANDLE_FIELDS
        .into_iter()
        .chain(OTHERS_FIELDS)
        .chain(DEFAULT_FIELDS)
    {
        if let Some(value) = map.get(key) {
            next.insert(Arc::from(key), value.clone());
        }
    }
    for (key, value) in map.iter() {
        if !next.contains_key(key.as_ref()) {
            next.insert(key.clone(), value.clone());
        }
    }
    ConfigValue::Object(Arc::new(next))
}
