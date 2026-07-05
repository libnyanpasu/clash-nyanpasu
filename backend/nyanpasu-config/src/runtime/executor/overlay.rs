//! Overlay (= legacy Merge chain item) directive semantics.
//! Authority: spec §7.3 table; legacy source enhance/merge.rs. Never fails:
//! every abnormal path logs and skips that key (merge.rs:242-318 parity).

use std::sync::Arc;

use crate::runtime::value::ConfigValue;

use super::{
    artifact::StepLogEntry,
    ports::ScriptRunner,
    value_util::{deep_merge_value, get_at, parse_dotted_path, remove_at, replace_at},
};

pub(super) fn apply_overlay(
    overlay: &ConfigValue,
    mut config: ConfigValue,
    runner: &dyn ScriptRunner,
    logs: &mut Vec<StepLogEntry>,
) -> ConfigValue {
    let Some(entries) = overlay.as_object_arc() else {
        logs.push(StepLogEntry::warn(
            "overlay document is not a mapping, skipped",
        ));
        return config;
    };

    // IndexMap iteration = document order (parity with Mapping iteration).
    for (key, value) in entries.iter() {
        // Legacy quirk kept verbatim (merge.rs:248): directive matching and
        // the remainder path are lowercased; bare keys preserve case.
        let lowered = key.to_ascii_lowercase();
        if let Some(field) = strip_any(&lowered, &["prepend__", "prepend-"]) {
            config = splice_sequence(config, field, value, true, logs);
        } else if let Some(field) = strip_any(&lowered, &["append__", "append-"]) {
            config = splice_sequence(config, field, value, false, logs);
        } else if let Some(field) = lowered.strip_prefix("override__") {
            config = override_path(config, field, value, logs);
        } else if let Some(field) = lowered.strip_prefix("filter__") {
            config = filter_path(config, field, value, runner, logs);
        } else {
            config = bare_key_merge(config, key, value);
        }
    }
    config
}

fn strip_any<'a>(key: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes.iter().find_map(|prefix| key.strip_prefix(prefix))
}

fn splice_sequence(
    config: ConfigValue,
    field: &str,
    value: &ConfigValue,
    prepend: bool,
    logs: &mut Vec<StepLogEntry>,
) -> ConfigValue {
    let Some(to_merge) = value.as_array_arc() else {
        logs.push(StepLogEntry::warn(format!(
            "merge value for `{field}` is not a sequence, skipped"
        )));
        return config;
    };
    let segments = parse_dotted_path(field);
    let Some(target) = get_at(&config, &segments) else {
        logs.push(StepLogEntry::warn(format!(
            "field `{field}` not found, skipped"
        )));
        return config;
    };
    let Some(existing) = target.as_array_arc() else {
        logs.push(StepLogEntry::warn(format!(
            "field `{field}` is not a sequence, skipped"
        )));
        return config;
    };

    let items: Vec<ConfigValue> = if prepend {
        to_merge
            .iter()
            .cloned()
            .chain(existing.iter().cloned())
            .collect()
    } else {
        existing
            .iter()
            .cloned()
            .chain(to_merge.iter().cloned())
            .collect()
    };
    replace_at(&config, &segments, ConfigValue::Array(Arc::from(items))).unwrap_or(config)
}

fn override_path(
    config: ConfigValue,
    field: &str,
    value: &ConfigValue,
    logs: &mut Vec<StepLogEntry>,
) -> ConfigValue {
    let segments = parse_dotted_path(field);
    match replace_at(&config, &segments, value.clone()) {
        Some(next) => next,
        None => {
            // Legacy: override does NOT create missing paths (merge.rs:292-304).
            logs.push(StepLogEntry::warn(format!(
                "field `{field}` not found, skipped"
            )));
            config
        }
    }
}

/// Bare key: deep-merge for maps, wholesale replace otherwise, insert when
/// absent (merge.rs:8-24, 310-312). Original key case preserved.
fn bare_key_merge(config: ConfigValue, key: &Arc<str>, data: &ConfigValue) -> ConfigValue {
    let existing = config
        .as_object_arc()
        .and_then(|map| map.get(key.as_ref()))
        .cloned();
    let merged = deep_merge_value(existing.as_ref(), data);
    super::value_util::obj_insert(&config, key.as_ref(), merged)
}

fn filter_path(
    config: ConfigValue,
    field: &str,
    filter: &ConfigValue,
    runner: &dyn ScriptRunner,
    logs: &mut Vec<StepLogEntry>,
) -> ConfigValue {
    let segments = parse_dotted_path(field);
    let Some(target) = get_at(&config, &segments) else {
        logs.push(StepLogEntry::warn(format!(
            "field `{field}` not found, skipped"
        )));
        return config;
    };
    let Some(existing) = target.as_array_arc() else {
        logs.push(StepLogEntry::warn(format!(
            "field `{field}` is not a sequence, skipped"
        )));
        return config;
    };

    let filtered = apply_filter(existing.to_vec(), filter, runner, logs);
    replace_at(&config, &segments, ConfigValue::Array(Arc::from(filtered))).unwrap_or(config)
}

fn apply_filter(
    items: Vec<ConfigValue>,
    filter: &ConfigValue,
    runner: &dyn ScriptRunner,
    logs: &mut Vec<StepLogEntry>,
) -> Vec<ConfigValue> {
    match filter {
        // Sequence of filters: composable multi-pass (merge.rs do_filter).
        ConfigValue::Array(filters) => filters
            .iter()
            .fold(items, |acc, sub| apply_filter(acc, sub, runner, logs)),
        // String: Lua boolean predicate; eval error removes the item (parity).
        ConfigValue::String(expr) => items
            .into_iter()
            .filter(|item| match runner.eval_item_predicate(expr, item) {
                Ok(keep) => keep,
                Err(error) => {
                    logs.push(StepLogEntry::warn(format!(
                        "filter expr failed, item removed: {error}"
                    )));
                    false
                }
            })
            .collect(),
        ConfigValue::Object(actions) => {
            let Some(ConfigValue::String(when)) = actions.get("when") else {
                logs.push(StepLogEntry::warn("invalid filter: missing `when`"));
                return items;
            };
            items
                .into_iter()
                .map(|item| {
                    let hit = match runner.eval_item_predicate(when, &item) {
                        Ok(hit) => hit,
                        Err(error) => {
                            logs.push(StepLogEntry::warn(format!(
                                "filter `when` failed, treated as false: {error}"
                            )));
                            false
                        }
                    };
                    if !hit {
                        return item;
                    }
                    if let Some(ConfigValue::String(expr)) = actions.get("expr") {
                        return match runner.eval_item_expr(expr, &item) {
                            Ok(next) => next,
                            Err(error) => {
                                logs.push(StepLogEntry::warn(format!(
                                    "filter `expr` failed, item kept: {error}"
                                )));
                                item
                            }
                        };
                    }
                    if let Some(replacement) = actions.get("override") {
                        return replacement.clone();
                    }
                    if let Some(merge) = actions.get("merge") {
                        return deep_merge_value(Some(&item), merge);
                    }
                    if let Some(ConfigValue::Array(paths)) = actions.get("remove") {
                        return remove_from_item(item, paths, logs);
                    }
                    logs.push(StepLogEntry::warn("invalid filter: no action"));
                    item
                })
                .collect()
        }
        _ => {
            logs.push(StepLogEntry::warn("invalid filter value, skipped"));
            items
        }
    }
}

fn remove_from_item(
    item: ConfigValue,
    paths: &Arc<[ConfigValue]>,
    logs: &mut Vec<StepLogEntry>,
) -> ConfigValue {
    let mut current = item;
    for path in paths.iter() {
        match path {
            ConfigValue::String(dotted) => {
                let segments = parse_dotted_path(dotted);
                match remove_at(&current, &segments) {
                    Some(next) => current = next,
                    None => logs.push(StepLogEntry::warn(format!(
                        "remove path `{dotted}` not found, skipped"
                    ))),
                }
            }
            ConfigValue::Number(index) => {
                let removed = index
                    .as_u64()
                    .map(|index| index.to_string())
                    .and_then(|segment| remove_at(&current, &[segment]));
                match removed {
                    Some(next) => current = next,
                    None => logs.push(StepLogEntry::warn("remove index invalid, skipped")),
                }
            }
            _ => logs.push(StepLogEntry::warn("invalid remove entry, skipped")),
        }
    }
    current
}
