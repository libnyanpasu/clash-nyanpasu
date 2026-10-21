//! WhitelistFieldFilter / GuardOverrides / Finalizing (spec 7.4).
//! Constants transplanted verbatim from enhance/field.rs:4-56.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::runtime::{
    post_process::{apply_defaults, apply_settings},
    value::{ConfigObject, ConfigValue},
};

use super::{
    GuardInputs, TunParams,
    plan::{finalize_ownership, guard_ownership},
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

/// Legacy HANDLE overwrite (enhance/mod.rs:110-116): writes the keys of
/// [`guard_ownership`] at top level, bypassing the stage-1 whitelist.
/// Unmanaged override fields are left to the profiles.
pub(super) fn apply_guard(config: &ConfigValue, guard: &GuardInputs<'_>) -> ConfigValue {
    apply_settings(config, &guard_ownership(guard))
}

/// Finalizing composite node (spec 7.4): stage-2 filter, then the keys of
/// [`finalize_ownership`]: `tun`, its DNS takeover and the `profile` cache.
/// The forced keys go first and the defaults fill in what is still missing,
/// as `revise!` then `append!` did (enhance/tun.rs:26-109). One recorded
/// node; changed_fields shows the net effect.
pub(super) fn finalize(
    config: &ConfigValue,
    tun: &TunParams,
    whitelist_enabled: bool,
) -> ConfigValue {
    let full: Vec<String> = known_fields().map(str::to_string).collect();
    let ownership = finalize_ownership(tun);
    let mut next = whitelist_filter(config, &full, whitelist_enabled);
    next = apply_settings(&next, &ownership);
    next = apply_defaults(&next, &ownership);
    // TODO: 可选排序算法
    // next = apply_sort(&next);
    next
}

/// Mirrors field.rs:115-147: known keys use HANDLE++OTHERS++DEFAULT order;
/// unknown keys keep their original relative order at the end.
#[allow(dead_code)] // Unwired from `finalize` until the ordering TODO there is decided.
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
