//! Pure ConfigValue helpers. Path semantics mirror legacy `find_field`
//! (enhance/merge.rs:27-48): mapping segment = string key, sequence segment =
//! parsed index; anything else fails the lookup.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::runtime::value::{ConfigObject, ConfigValue};

pub(super) fn empty_object() -> ConfigValue {
    ConfigValue::Object(Arc::new(IndexMap::new()))
}

/// Clean seed for base-less compositions: `{ proxies: [] }` (clean-design §7.4).
pub(super) fn clean_seed() -> ConfigValue {
    let mut map: ConfigObject = IndexMap::new();
    map.insert(
        Arc::from("proxies"),
        ConfigValue::Array(Arc::from(Vec::<ConfigValue>::new())),
    );
    ConfigValue::Object(Arc::new(map))
}

pub(super) fn obj_get<'a>(value: &'a ConfigValue, key: &str) -> Option<&'a ConfigValue> {
    value.as_object_arc().and_then(|map| map.get(key))
}

/// Top-level insert/overwrite. A non-object root is replaced by a fresh
/// object, matching legacy Mapping semantics (the working config is a map).
pub(super) fn obj_insert(value: &ConfigValue, key: &str, entry: ConfigValue) -> ConfigValue {
    let mut map: ConfigObject = value
        .as_object_arc()
        .map(|map| (**map).clone())
        .unwrap_or_default();
    map.insert(Arc::from(key), entry);
    ConfigValue::Object(Arc::new(map))
}

pub(super) fn parse_dotted_path(path: &str) -> Vec<String> {
    path.split('.').map(str::to_string).collect()
}

pub(super) fn get_at<'a>(root: &'a ConfigValue, segments: &[String]) -> Option<&'a ConfigValue> {
    let mut current = root;
    for segment in segments {
        current = match current {
            ConfigValue::Object(map) => map.get(segment.as_str())?,
            ConfigValue::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

/// Copy-on-write replace; `None` when the path does not fully exist.
pub(super) fn replace_at(
    root: &ConfigValue,
    segments: &[String],
    new_value: ConfigValue,
) -> Option<ConfigValue> {
    let Some((head, rest)) = segments.split_first() else {
        return Some(new_value);
    };
    match root {
        ConfigValue::Object(map) => {
            let current = map.get(head.as_str())?;
            let replaced = replace_at(current, rest, new_value)?;
            let mut next = (**map).clone();
            next.insert(Arc::from(head.as_str()), replaced);
            Some(ConfigValue::Object(Arc::new(next)))
        }
        ConfigValue::Array(items) => {
            let index = head.parse::<usize>().ok()?;
            let current = items.get(index)?;
            let replaced = replace_at(current, rest, new_value)?;
            let mut next: Vec<ConfigValue> = items.to_vec();
            next[index] = replaced;
            Some(ConfigValue::Array(Arc::from(next)))
        }
        _ => None,
    }
}

/// Removes the value at the path (map key or sequence index); `None` when the
/// path does not fully exist.
pub(super) fn remove_at(root: &ConfigValue, segments: &[String]) -> Option<ConfigValue> {
    let (head, rest) = segments.split_first()?;
    match root {
        ConfigValue::Object(map) => {
            if rest.is_empty() {
                if !map.contains_key(head.as_str()) {
                    return None;
                }
                let mut next = (**map).clone();
                next.shift_remove(head.as_str());
                return Some(ConfigValue::Object(Arc::new(next)));
            }
            let current = map.get(head.as_str())?;
            let removed = remove_at(current, rest)?;
            let mut next = (**map).clone();
            next.insert(Arc::from(head.as_str()), removed);
            Some(ConfigValue::Object(Arc::new(next)))
        }
        ConfigValue::Array(items) => {
            let index = head.parse::<usize>().ok()?;
            if rest.is_empty() {
                if index >= items.len() {
                    return None;
                }
                let mut next: Vec<ConfigValue> = items.to_vec();
                next.remove(index);
                return Some(ConfigValue::Array(Arc::from(next)));
            }
            let current = items.get(index)?;
            let removed = remove_at(current, rest)?;
            let mut next: Vec<ConfigValue> = items.to_vec();
            next[index] = removed;
            Some(ConfigValue::Array(Arc::from(next)))
        }
        _ => None,
    }
}

/// Legacy `override_recursive` value rule (enhance/merge.rs:8-24): object
/// into object = per-key deep merge preserving unmentioned siblings; anything
/// else = wholesale replace (sequences included).
pub(super) fn deep_merge_value(existing: Option<&ConfigValue>, data: &ConfigValue) -> ConfigValue {
    match (
        existing.and_then(ConfigValue::as_object_arc),
        data.as_object_arc(),
    ) {
        (Some(current), Some(incoming)) => {
            let mut merged = (**current).clone();
            for (key, value) in incoming.iter() {
                let previous = merged.get(key.as_ref()).cloned();
                merged.insert(key.clone(), deep_merge_value(previous.as_ref(), value));
            }
            ConfigValue::Object(Arc::new(merged))
        }
        _ => data.clone(),
    }
}
