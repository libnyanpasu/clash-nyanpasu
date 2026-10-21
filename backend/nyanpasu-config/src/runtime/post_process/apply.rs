use std::sync::Arc;

use crate::runtime::value::{ConfigObject, ConfigValue};

use super::{Ownership, OwnershipEntries};

/// Writes the [`Ownership::Forced`] keys over `config` and removes the
/// [`Ownership::Removed`] ones.
pub fn apply_settings(config: &ConfigValue, entries: &OwnershipEntries) -> ConfigValue {
    apply(config, entries, Pass::Settings)
}

/// Writes the [`Ownership::Default`] keys that `config` lacks.
pub fn apply_defaults(config: &ConfigValue, entries: &OwnershipEntries) -> ConfigValue {
    apply(config, entries, Pass::Defaults)
}

/// The leaves one walk writes; the others are left for another step.
#[derive(Clone, Copy)]
enum Pass {
    Settings,
    Defaults,
}

impl Pass {
    fn handles(self, ownership: &Ownership) -> bool {
        match (self, ownership) {
            (Self::Settings, Ownership::Forced(_) | Ownership::Removed) => true,
            (Self::Defaults, Ownership::Default(_)) => true,
            (_, Ownership::Table { entries, .. }) => self.touches(entries),
            _ => false,
        }
    }

    fn touches(self, entries: &OwnershipEntries) -> bool {
        entries.iter().any(|(_, ownership)| self.handles(ownership))
    }
}

fn apply(config: &ConfigValue, entries: &OwnershipEntries, pass: Pass) -> ConfigValue {
    if !pass.touches(entries) {
        return config.clone();
    }
    let mut map = object_of(Some(config));
    walk(&mut map, entries, pass);
    ConfigValue::Object(Arc::new(map))
}

/// Edits one mapping in place. The caller cloned it once, so each table a
/// walk touches costs one shallow copy; untouched values stay shared.
fn walk(map: &mut ConfigObject, entries: &OwnershipEntries, pass: Pass) {
    for (key, ownership) in entries {
        match (pass, ownership) {
            (Pass::Settings, Ownership::Forced(value)) => set(map, key, value.clone()),
            (Pass::Settings, Ownership::Removed) => {
                map.shift_remove(*key);
            }
            (Pass::Defaults, Ownership::Default(value)) => {
                if !map.contains_key(*key) {
                    map.insert(Arc::from(*key), value.clone());
                }
            }
            (_, Ownership::Table { entries, create }) if pass.touches(entries) => {
                let existing = map.get(*key);
                if existing.is_none() && !create {
                    continue;
                }
                // A value that is not a mapping is replaced by one.
                let mut table = object_of(existing);
                walk(&mut table, entries, pass);
                set(map, key, ConfigValue::Object(Arc::new(table)));
            }
            _ => {}
        }
    }
}

fn object_of(value: Option<&ConfigValue>) -> ConfigObject {
    value
        .and_then(ConfigValue::as_object_arc)
        .map(|map| (**map).clone())
        .unwrap_or_default()
}

/// Replaces the value in place, keeping the key's position and allocation.
fn set(map: &mut ConfigObject, key: &str, value: ConfigValue) {
    match map.get_mut(key) {
        Some(slot) => *slot = value,
        None => {
            map.insert(Arc::from(key), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn value(json: serde_json::Value) -> ConfigValue {
        ConfigValue::try_from(json).unwrap()
    }

    fn table(entries: OwnershipEntries, create: bool) -> Ownership {
        Ownership::Table { entries, create }
    }

    /// Serialized, so key order is compared too.
    fn assert_same(actual: ConfigValue, expected: serde_json::Value) {
        assert_eq!(
            serde_json::to_string(&actual.to_json()).unwrap(),
            serde_json::to_string(&expected).unwrap()
        );
    }

    #[test]
    fn settings_overwrite_in_place_and_remove_without_reordering() {
        let config = value(json!({ "mode": "rule", "port": 1, "rules": [], "ipv6": true }));
        let entries = vec![
            ("mode", Ownership::Forced(value(json!("global")))),
            ("port", Ownership::Removed),
            ("allow-lan", Ownership::Forced(value(json!(false)))),
            ("ipv6", Ownership::Default(value(json!(false)))),
        ];

        assert_same(
            apply_settings(&config, &entries),
            json!({ "mode": "global", "rules": [], "ipv6": true, "allow-lan": false }),
        );
    }

    #[test]
    fn defaults_fill_only_missing_keys() {
        let config = value(json!({ "profile": { "store-selected": false }, "dns": null }));
        let entries = vec![
            (
                "profile",
                Ownership::Default(value(json!({ "store-selected": true }))),
            ),
            ("dns", Ownership::Default(value(json!({ "enable": true })))),
            ("mode", Ownership::Default(value(json!("rule")))),
            ("ipv6", Ownership::Forced(value(json!(true)))),
        ];

        assert_same(
            apply_defaults(&config, &entries),
            json!({ "profile": { "store-selected": false }, "dns": null, "mode": "rule" }),
        );
    }

    #[test]
    fn tables_merge_into_the_mapping_at_their_key() {
        let entries = vec![(
            "tun",
            table(
                vec![
                    ("enable", Ownership::Forced(value(json!(true)))),
                    ("stack", Ownership::Default(value(json!("gvisor")))),
                ],
                true,
            ),
        )];
        let config = value(json!({ "tun": { "stack": "system", "mtu": 1400 }, "mode": "rule" }));

        let config = apply_settings(&config, &entries);
        assert_same(
            config.clone(),
            json!({ "tun": { "stack": "system", "mtu": 1400, "enable": true }, "mode": "rule" }),
        );
        assert_same(
            apply_defaults(&config, &entries),
            json!({ "tun": { "stack": "system", "mtu": 1400, "enable": true }, "mode": "rule" }),
        );
    }

    #[test]
    fn a_missing_table_is_created_only_when_asked() {
        let forced = vec![("enable", Ownership::Forced(value(json!(false))))];

        assert_same(
            apply_settings(
                &value(json!({})),
                &vec![("tun", table(forced.clone(), true))],
            ),
            json!({ "tun": { "enable": false } }),
        );
        assert_same(
            apply_settings(
                &value(json!({})),
                &vec![("tun", table(forced.clone(), false))],
            ),
            json!({}),
        );
        // A value that is not a mapping counts as present.
        assert_same(
            apply_settings(
                &value(json!({ "tun": true })),
                &vec![("tun", table(forced, false))],
            ),
            json!({ "tun": { "enable": false } }),
        );
    }

    /// A table holding no leaf for the walk is skipped, so it never leaves an
    /// empty mapping behind.
    #[test]
    fn a_table_without_leaves_for_the_walk_is_skipped() {
        let entries = vec![(
            "dns",
            table(
                vec![
                    ("enhanced-mode", Ownership::Default(value(json!("fake-ip")))),
                    ("nameserver", Ownership::Inherited),
                ],
                true,
            ),
        )];

        assert_same(apply_settings(&value(json!({})), &entries), json!({}));
        assert_same(
            apply_defaults(&value(json!({})), &entries),
            json!({ "dns": { "enhanced-mode": "fake-ip" } }),
        );
    }
}
