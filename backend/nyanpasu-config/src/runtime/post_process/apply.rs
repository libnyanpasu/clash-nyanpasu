use std::sync::Arc;

use json_patch::{
    AddOperation, Patch, PatchOperation, RemoveOperation, ReplaceOperation, jsonptr::PointerBuf,
};

use crate::runtime::value::{ConfigObject, ConfigValue};

use super::{Ownership, OwnershipEntries};

/// Writes the [`Ownership::Forced`] keys over the `config` mapping and
/// removes the [`Ownership::Removed`] ones. Returns the result with the patch
/// that turns `config` into it, for a snapshot that need not diff the trees.
pub fn apply_settings(config: &ConfigValue, entries: &OwnershipEntries) -> (ConfigValue, Patch) {
    apply(config, entries, Pass::Settings)
}

/// Writes the [`Ownership::Default`] keys that the `config` mapping lacks,
/// with the patch as [`apply_settings`] does.
pub fn apply_defaults(config: &ConfigValue, entries: &OwnershipEntries) -> (ConfigValue, Patch) {
    apply(config, entries, Pass::Defaults)
}

/// Copies each [`Ownership::Inherited`] key that `source` has into the
/// `filtered` mapping, creating its missing parent mappings, with the patch as
/// [`apply_settings`] does. The field filter only drops whole top-level keys
/// of `source`, so a key `filtered` still has is the same and stays.
pub fn keep_inherited(
    filtered: &ConfigValue,
    source: &ConfigValue,
    entries: &OwnershipEntries,
) -> (ConfigValue, Patch) {
    let mut ops = Vec::new();
    if let Some(source) = source.as_object_arc()
        && inherits(entries)
    {
        let mut map = object_of(Some(filtered));
        keep(
            &mut map,
            source,
            entries,
            &PointerBuf::new(),
            Some(&mut ops),
        );
        if !ops.is_empty() {
            return (ConfigValue::Object(Arc::new(map)), Patch(ops));
        }
    }
    (filtered.clone(), Patch(ops))
}

fn inherits(entries: &OwnershipEntries) -> bool {
    entries.iter().any(|(_, ownership)| match ownership {
        Ownership::Inherited => true,
        Ownership::Table { entries, .. } => inherits(entries),
        _ => false,
    })
}

/// Edits one mapping in place and reports whether it changed, recording as
/// [`walk`] does.
fn keep(
    map: &mut ConfigObject,
    source: &ConfigObject,
    entries: &OwnershipEntries,
    path: &PointerBuf,
    mut ops: Option<&mut Vec<PatchOperation>>,
) -> bool {
    let mut changed = false;
    for (key, ownership) in entries {
        let Some(from) = source.get(*key) else {
            continue;
        };
        match ownership {
            Ownership::Inherited if !map.contains_key(*key) => {
                map.insert(Arc::from(*key), from.clone());
                if let Some(ops) = ops.as_deref_mut() {
                    ops.push(add(path, key, from));
                }
                changed = true;
            }
            Ownership::Table { entries, .. } if inherits(entries) => {
                let Some(from) = from.as_object_arc() else {
                    continue;
                };
                match map.get(*key) {
                    Some(ConfigValue::Object(table)) => {
                        let mut table = (**table).clone();
                        let child = path.with_trailing_token(*key);
                        if keep(&mut table, from, entries, &child, ops.as_deref_mut()) {
                            set(map, key, ConfigValue::Object(Arc::new(table)));
                            changed = true;
                        }
                    }
                    None => {
                        let mut table = ConfigObject::new();
                        if keep(&mut table, from, entries, path, None) {
                            let table = ConfigValue::Object(Arc::new(table));
                            if let Some(ops) = ops.as_deref_mut() {
                                ops.push(add(path, key, &table));
                            }
                            set(map, key, table);
                            changed = true;
                        }
                    }
                    Some(_) => {}
                }
            }
            _ => {}
        }
    }
    changed
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

fn apply(config: &ConfigValue, entries: &OwnershipEntries, pass: Pass) -> (ConfigValue, Patch) {
    let mut ops = Vec::new();
    if pass.touches(entries) {
        let mut map = object_of(Some(config));
        walk(&mut map, entries, pass, &PointerBuf::new(), Some(&mut ops));
        if !ops.is_empty() {
            return (ConfigValue::Object(Arc::new(map)), Patch(ops));
        }
    }
    (config.clone(), Patch(ops))
}

/// Edits one mapping in place and reports whether it changed. The caller
/// cloned the mapping once, so each table a walk changes costs one shallow
/// copy; untouched values stay shared. `ops` records each change at `path`;
/// a table the walk creates is recorded whole by its caller instead.
fn walk(
    map: &mut ConfigObject,
    entries: &OwnershipEntries,
    pass: Pass,
    path: &PointerBuf,
    mut ops: Option<&mut Vec<PatchOperation>>,
) -> bool {
    let mut changed = false;
    for (key, ownership) in entries {
        let record = |ops: &mut Option<&mut Vec<PatchOperation>>, operation| {
            if let Some(ops) = ops {
                ops.push(operation);
            }
        };
        match (pass, ownership) {
            (Pass::Settings, Ownership::Forced(value)) => match map.get_mut(*key) {
                Some(slot) if slot == value => {}
                Some(slot) => {
                    *slot = value.clone();
                    record(&mut ops, replace(path, key, value));
                    changed = true;
                }
                None => {
                    map.insert(Arc::from(*key), value.clone());
                    record(&mut ops, add(path, key, value));
                    changed = true;
                }
            },
            (Pass::Settings, Ownership::Removed) => {
                if map.shift_remove(*key).is_some() {
                    record(
                        &mut ops,
                        PatchOperation::Remove(RemoveOperation {
                            path: path.with_trailing_token(*key),
                        }),
                    );
                    changed = true;
                }
            }
            (Pass::Defaults, Ownership::Default(value)) => {
                if !map.contains_key(*key) {
                    map.insert(Arc::from(*key), value.clone());
                    record(&mut ops, add(path, key, value));
                    changed = true;
                }
            }
            (_, Ownership::Table { entries, create }) if pass.touches(entries) => {
                match map.get(*key) {
                    Some(ConfigValue::Object(table)) => {
                        let mut table = (**table).clone();
                        let child = path.with_trailing_token(*key);
                        if walk(&mut table, entries, pass, &child, ops.as_deref_mut()) {
                            set(map, key, ConfigValue::Object(Arc::new(table)));
                            changed = true;
                        }
                    }
                    // A value that is not a mapping is replaced by one.
                    existing if existing.is_some() || *create => {
                        let replaced = existing.is_some();
                        let mut table = ConfigObject::new();
                        if walk(&mut table, entries, pass, path, None) {
                            let table = ConfigValue::Object(Arc::new(table));
                            record(
                                &mut ops,
                                if replaced {
                                    replace(path, key, &table)
                                } else {
                                    add(path, key, &table)
                                },
                            );
                            set(map, key, table);
                            changed = true;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    changed
}

fn add(path: &PointerBuf, key: &str, value: &ConfigValue) -> PatchOperation {
    PatchOperation::Add(AddOperation {
        path: path.with_trailing_token(key),
        value: value.to_json(),
    })
}

fn replace(path: &PointerBuf, key: &str, value: &ConfigValue) -> PatchOperation {
    PatchOperation::Replace(ReplaceOperation {
        path: path.with_trailing_token(key),
        value: value.to_json(),
    })
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
    use crate::runtime::snapshot::apply_patch;

    fn value(json: serde_json::Value) -> ConfigValue {
        ConfigValue::try_from(json).unwrap()
    }

    fn table(entries: OwnershipEntries, create: bool) -> Ownership {
        Ownership::Table { entries, create }
    }

    fn settings(config: serde_json::Value, entries: &OwnershipEntries) -> serde_json::Value {
        checked(config, |config| apply_settings(config, entries))
    }

    fn defaults(config: serde_json::Value, entries: &OwnershipEntries) -> serde_json::Value {
        checked(config, |config| apply_defaults(config, entries))
    }

    /// Runs a walk and checks that its patch replays the input into its
    /// output, key order included.
    fn checked(
        config: serde_json::Value,
        walk: impl FnOnce(&ConfigValue) -> (ConfigValue, Patch),
    ) -> serde_json::Value {
        let (result, patch) = walk(&value(config.clone()));
        let result = result.to_json();
        let mut replayed = config;
        apply_patch(&mut replayed, &patch).unwrap();
        assert_eq!(
            serde_json::to_string(&replayed).unwrap(),
            serde_json::to_string(&result).unwrap(),
            "the patch must replay into the result"
        );
        result
    }

    /// Serialized, so key order is compared too.
    fn assert_same(actual: serde_json::Value, expected: serde_json::Value) {
        assert_eq!(
            serde_json::to_string(&actual).unwrap(),
            serde_json::to_string(&expected).unwrap()
        );
    }

    #[test]
    fn settings_overwrite_in_place_and_remove_without_reordering() {
        let entries = vec![
            ("mode", Ownership::Forced(value(json!("global")))),
            ("port", Ownership::Removed),
            ("allow-lan", Ownership::Forced(value(json!(false)))),
            ("ipv6", Ownership::Default(value(json!(false)))),
        ];

        assert_same(
            settings(
                json!({ "mode": "rule", "port": 1, "rules": [], "ipv6": true }),
                &entries,
            ),
            json!({ "mode": "global", "rules": [], "ipv6": true, "allow-lan": false }),
        );
    }

    #[test]
    fn defaults_fill_only_missing_keys() {
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
            defaults(
                json!({ "profile": { "store-selected": false }, "dns": null }),
                &entries,
            ),
            json!({ "profile": { "store-selected": false }, "dns": null, "mode": "rule" }),
        );
    }

    fn kept(
        filtered: serde_json::Value,
        source: serde_json::Value,
        entries: &OwnershipEntries,
    ) -> serde_json::Value {
        let source = value(source);
        checked(filtered, |filtered| {
            keep_inherited(filtered, &source, entries)
        })
    }

    /// The field filter dropped `unified-delay` and `tun`; the inherited keys
    /// come back, and only those.
    #[test]
    fn inherited_keys_come_back_from_before_the_filter() {
        let source = json!({
            "unified-delay": false,
            "tun": { "auto-route": "global", "mtu": 1400 },
            "tcp-concurrent": true,
            "proxies": [],
        });
        let entries = vec![
            ("unified-delay", Ownership::Inherited),
            (
                "tun",
                table(
                    vec![
                        ("auto-route", Ownership::Inherited),
                        ("enable", Ownership::Forced(value(json!(true)))),
                    ],
                    true,
                ),
            ),
            ("tcp-concurrent", Ownership::Forced(value(json!(false)))),
            ("log-level", Ownership::Inherited),
        ];

        assert_same(
            kept(json!({ "proxies": [] }), source.clone(), &entries),
            json!({ "proxies": [], "unified-delay": false, "tun": { "auto-route": "global" } }),
        );
        // A key the filter kept is already there and stays as it is.
        assert_same(kept(source.clone(), source.clone(), &entries), source);
    }

    #[test]
    fn a_whole_inherited_table_comes_back() {
        assert_same(
            kept(
                json!({}),
                json!({ "dns": { "enable": false, "nameserver": ["1.1.1.1"] } }),
                &vec![("dns", Ownership::Inherited)],
            ),
            json!({ "dns": { "enable": false, "nameserver": ["1.1.1.1"] } }),
        );
    }

    /// A parent that is not a mapping before the filter holds no inherited
    /// key, so nothing is created for it.
    #[test]
    fn nothing_comes_back_from_under_a_value_that_is_not_a_mapping() {
        let entries = vec![(
            "tun",
            table(vec![("auto-route", Ownership::Inherited)], true),
        )];

        assert_same(kept(json!({}), json!({ "tun": true }), &entries), json!({}));
        assert_same(kept(json!({}), json!({ "tun": {} }), &entries), json!({}));
    }

    #[test]
    fn tables_merge_into_the_mapping_at_their_key() {
        let entries = vec![(
            "tun",
            table(
                vec![
                    ("enable", Ownership::Forced(value(json!(true)))),
                    ("stack", Ownership::Default(value(json!("gvisor")))),
                    ("dns-hijack", Ownership::Default(value(json!(["any:53"])))),
                ],
                true,
            ),
        )];

        let config = settings(
            json!({ "tun": { "stack": "system", "mtu": 1400 }, "mode": "rule" }),
            &entries,
        );
        assert_same(
            config.clone(),
            json!({ "tun": { "stack": "system", "mtu": 1400, "enable": true }, "mode": "rule" }),
        );
        assert_same(
            defaults(config, &entries),
            json!({
                "tun": { "stack": "system", "mtu": 1400, "enable": true, "dns-hijack": ["any:53"] },
                "mode": "rule",
            }),
        );
    }

    #[test]
    fn a_missing_table_is_created_only_when_asked() {
        let forced = vec![("enable", Ownership::Forced(value(json!(false))))];

        assert_same(
            settings(json!({}), &vec![("tun", table(forced.clone(), true))]),
            json!({ "tun": { "enable": false } }),
        );
        assert_same(
            settings(json!({}), &vec![("tun", table(forced.clone(), false))]),
            json!({}),
        );
        // A value that is not a mapping counts as present.
        assert_same(
            settings(json!({ "tun": true }), &vec![("tun", table(forced, false))]),
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

        assert_same(settings(json!({}), &entries), json!({}));
        assert_same(
            defaults(json!({}), &entries),
            json!({ "dns": { "enhanced-mode": "fake-ip" } }),
        );
    }

    /// Writing what is already there changes nothing, so the step records no
    /// field and keeps sharing the input.
    #[test]
    fn writing_the_same_values_records_no_change() {
        let config = value(json!({ "mode": "rule", "tun": { "enable": true } }));
        let entries = vec![
            ("mode", Ownership::Forced(value(json!("rule")))),
            ("port", Ownership::Removed),
            (
                "tun",
                table(
                    vec![("enable", Ownership::Forced(value(json!(true))))],
                    true,
                ),
            ),
        ];

        let (result, patch) = apply_settings(&config, &entries);

        assert!(patch.0.is_empty());
        assert!(Arc::ptr_eq(
            result.as_object_arc().unwrap(),
            config.as_object_arc().unwrap()
        ));
    }
}
