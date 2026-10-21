//! The runtime config keys post-processing decides, derived from the
//! settings. Every key Nyanpasu writes, removes or fills in is listed here;
//! the steps only walk these trees.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::runtime::{
    post_process::{OwnedFields, Ownership, OwnershipEntries},
    value::{ConfigObject, ConfigValue},
};

use super::{GuardInputs, TunFlavor, TunParams};

/// Every key post-processing decides: the guard's, then `tun` with its DNS
/// takeover, then the `profile` cache.
pub(super) fn ownership(guard: &GuardInputs<'_>, tun: &TunParams) -> OwnershipEntries {
    let mut entries = guard_ownership(guard);
    entries.extend(tun_ownership(tun));
    entries.push(cache_ownership());
    entries
}

/// The top-level keys the guard owns: the typed overrides, then the resolved
/// ports. Nyanpasu owns `port` and `socks-port` like `mixed-port`: an unset
/// one is removed, so a profile cannot open a listener the user turned off or
/// take the `mixed-port` before the mixed listener binds (#5678).
pub(super) fn guard_ownership(guard: &GuardInputs<'_>) -> OwnershipEntries {
    let mut entries = guard.overrides.ownership_entries();
    entries.push((
        "mixed-port",
        Ownership::Forced(number(guard.ports.mixed_port)),
    ));
    for (key, port) in [
        ("port", guard.ports.port),
        ("socks-port", guard.ports.socks_port),
    ] {
        entries.push((
            key,
            port.map_or(Ownership::Removed, |port| Ownership::Forced(number(port))),
        ));
    }
    if let Some(controller) = &guard.ports.external_controller {
        entries.push(("external-controller", Ownership::Forced(string(controller))));
    }
    entries
}

/// `tun` and the DNS takeover it needs. While TUN is off, a `tun` the
/// profiles wrote is turned off and none is created.
pub(super) fn tun_ownership(tun: &TunParams) -> OwnershipEntries {
    let mut entries = Vec::with_capacity(2);
    if tun.enable {
        entries.push((
            "tun",
            Ownership::Table {
                entries: tun_entries(tun),
                create: true,
            },
        ));
        entries.push((
            "dns",
            Ownership::Table {
                entries: dns_entries(tun.windows_fake_ip_filter),
                create: true,
            },
        ));
    } else {
        entries.push((
            "tun",
            Ownership::Table {
                entries: vec![("enable", Ownership::Forced(ConfigValue::Bool(false)))],
                create: false,
            },
        ));
    }
    entries
}

/// The `profile` cache. An existing `profile` is kept whole; its keys are not
/// filled in.
pub(super) fn cache_ownership() -> (&'static str, Ownership) {
    (
        "profile",
        Ownership::Default(object([
            ("store-selected", ConfigValue::Bool(true)),
            ("store-fake-ip", ConfigValue::Bool(false)),
        ])),
    )
}

fn tun_entries(tun: &TunParams) -> OwnershipEntries {
    let mut entries = vec![("enable", Ownership::Forced(ConfigValue::Bool(true)))];
    entries.extend(tun.overrides.ownership_entries());
    match tun.flavor {
        TunFlavor::ClashRs => {
            entries.push(("device-id", Ownership::Default(string("dev://utun1989"))));
        }
        TunFlavor::Standard { stack } => entries.extend([
            ("stack", Ownership::Default(string(stack.as_ref()))),
            ("dns-hijack", Ownership::Default(strings(&["any:53"]))),
            (
                "auto-detect-interface",
                Ownership::Default(ConfigValue::Bool(true)),
            ),
        ]),
    }
    entries
}

fn dns_entries(windows_fake_ip_filter: bool) -> OwnershipEntries {
    let mut entries = vec![
        ("enable", Ownership::Forced(ConfigValue::Bool(true))),
        ("enhanced-mode", Ownership::Default(string("fake-ip"))),
        ("fake-ip-range", Ownership::Default(string("198.18.0.1/16"))),
        (
            "nameserver",
            Ownership::Default(strings(&["114.114.114.114", "223.5.5.5", "8.8.8.8"])),
        ),
        ("fallback", Ownership::Default(strings(&[]))),
    ];
    if windows_fake_ip_filter {
        entries.push((
            "fake-ip-filter",
            Ownership::Default(strings(&[
                "dns.msftncsi.com",
                "www.msftncsi.com",
                "www.msftconnecttest.com",
            ])),
        ));
    }
    entries
}

fn number(value: u16) -> ConfigValue {
    ConfigValue::Number(serde_json::Number::from(value))
}

fn string(text: &str) -> ConfigValue {
    ConfigValue::String(Arc::from(text))
}

fn strings(items: &[&str]) -> ConfigValue {
    ConfigValue::Array(items.iter().map(|item| string(item)).collect())
}

fn object<const N: usize>(entries: [(&str, ConfigValue); N]) -> ConfigValue {
    let map: ConfigObject = entries
        .into_iter()
        .map(|(key, value)| (Arc::from(key), value))
        .collect::<IndexMap<_, _>>();
    ConfigValue::Object(Arc::new(map))
}
