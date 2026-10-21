//! The field filter of post-processing (spec 7.4). Constants transplanted
//! verbatim from enhance/field.rs:4-56.

use std::sync::Arc;

use indexmap::IndexMap;

use json_patch::{Patch, PatchOperation, RemoveOperation, jsonptr::PointerBuf};

use crate::runtime::value::{ConfigObject, ConfigValue};

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
pub fn known_fields() -> impl Iterator<Item = &'static str> {
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

/// Keeps the top-level keys in `allow`, with the patch that removes the rest.
pub(super) fn filter_fields(config: &ConfigValue, allow: &[String]) -> (ConfigValue, Patch) {
    let Some(map) = config.as_object_arc() else {
        return (config.clone(), Patch(Vec::new()));
    };
    let mut removed = Vec::new();
    let mut filtered = ConfigObject::with_capacity(map.len());
    for (key, value) in map.iter() {
        if allow.iter().any(|allowed| allowed.as_str() == key.as_ref()) {
            filtered.insert(key.clone(), value.clone());
        } else {
            removed.push(PatchOperation::Remove(RemoveOperation {
                path: PointerBuf::from_tokens([key.as_ref()]),
            }));
        }
    }
    if removed.is_empty() {
        return (config.clone(), Patch(removed));
    }
    (ConfigValue::Object(Arc::new(filtered)), Patch(removed))
}

/// Mirrors field.rs:115-147: known keys use HANDLE++OTHERS++DEFAULT order;
/// unknown keys keep their original relative order at the end.
#[allow(dead_code)] // Unwired until the ordering TODO in `execute` is decided.
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
