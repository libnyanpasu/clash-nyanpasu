use indexmap::IndexSet;
use serde_yaml::{Mapping, Value};

// Override recursive, and if the value is sequence, it should be append to the end.
pub fn override_recursive(config: &mut Mapping, key: &Value, data: Value) {
    if let Some(value) = config.get_mut(key) {
        if value.is_mapping() {
            let value = value.as_mapping_mut().unwrap();
            let data = data.as_mapping().unwrap();
            for (k, v) in data.iter() {
                override_recursive(value, k, v.clone());
            }
        } else {
            tracing::trace!("override key: {:#?}", key);
            *value = data;
        }
    } else {
        tracing::trace!("insert key: {:#?}", key);
        config.insert(key.clone(), data);
    }
}

/// Apply overrides to the config
pub fn apply_overrides(config: &mut Mapping, overrides: &Mapping) {
    for (k, v) in overrides.iter() {
        override_recursive(config, k, v.clone());
    }
}

/// Recursively compare two Mappings and return the set of differing field paths.
/// Field paths are joined with `.` for nested fields.
/// Returns fields that exist only in `a`, only in `b`, or have different values.
pub fn diff_fields(a: &Mapping, b: &Mapping) -> IndexSet<String> {
    let mut diffs = IndexSet::new();
    diff_fields_inner(a, b, &mut String::new(), &mut diffs);
    diffs
}

fn diff_fields_inner(a: &Mapping, b: &Mapping, prefix: &mut String, diffs: &mut IndexSet<String>) {
    let prefix_len = prefix.len();

    // Check keys in a
    for (key, value_a) in a.iter() {
        let key_str = value_to_key_string(key);

        // Build field path
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(&key_str);

        match b.get(key) {
            Some(value_b) => {
                match (value_a.as_mapping(), value_b.as_mapping()) {
                    (Some(map_a), Some(map_b)) => {
                        // Both are mappings, recurse
                        diff_fields_inner(map_a, map_b, prefix, diffs);
                    }
                    _ if value_a != value_b => {
                        // Values differ
                        diffs.insert(prefix.clone());
                    }
                    _ => {} // Equal
                }
            }
            None => {
                // Key only in a
                diffs.insert(prefix.clone());
            }
        }

        // Restore prefix
        prefix.truncate(prefix_len);
    }

    // Check keys only in b
    for key in b.keys() {
        if !a.contains_key(key) {
            let key_str = value_to_key_string(key);
            let field_path = if prefix.is_empty() {
                key_str
            } else {
                format!("{}.{}", prefix, key_str)
            };
            diffs.insert(field_path);
        }
    }
}

fn value_to_key_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        _ => format!("{:?}", value),
    }
}

/// Key should be a.b.c to access the value
pub fn find_field<'a>(config: &'a mut Value, key: &'a str) -> Option<&'a mut Value> {
    let mut keys = key.split('.').peekable();
    let mut value = config;
    while let Some(k) = keys.next() {
        if let Some(v) = match k.parse::<usize>() {
            Ok(i) => value.get_mut(i),
            Err(_) => value.get_mut(k),
        } {
            if keys.peek().is_none() {
                return Some(v);
            }
            if v.is_mapping() || v.is_sequence() {
                value = v
            } else {
                return None;
            }
        } else {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml::from_str;

    // ==================== override_recursive tests ====================

    #[test]
    fn test_override_simple_value() {
        let mut config: Mapping = from_str("key1: value1\nkey2: value2").unwrap();
        override_recursive(
            &mut config,
            &Value::String("key1".into()),
            Value::String("new_value".into()),
        );
        assert_eq!(config.get("key1"), Some(&Value::String("new_value".into())));
        assert_eq!(config.get("key2"), Some(&Value::String("value2".into())));
    }

    #[test]
    fn test_override_insert_new_key() {
        let mut config: Mapping = from_str("key1: value1").unwrap();
        override_recursive(
            &mut config,
            &Value::String("key2".into()),
            Value::String("value2".into()),
        );
        assert_eq!(config.get("key2"), Some(&Value::String("value2".into())));
    }

    #[test]
    fn test_override_nested_mapping() {
        let mut config: Mapping = from_str(
            r#"
nested:
  a: 1
  b: 2
"#,
        )
        .unwrap();

        let override_data: Value = from_str(
            r#"
a: 10
c: 3
"#,
        )
        .unwrap();

        override_recursive(&mut config, &Value::String("nested".into()), override_data);

        let nested = config.get("nested").unwrap().as_mapping().unwrap();
        assert_eq!(nested.get("a"), Some(&Value::Number(10.into())));
        assert_eq!(nested.get("b"), Some(&Value::Number(2.into())));
        assert_eq!(nested.get("c"), Some(&Value::Number(3.into())));
    }

    #[test]
    fn test_override_deeply_nested() {
        let mut config: Mapping = from_str(
            r#"
level1:
  level2:
    key: original
"#,
        )
        .unwrap();

        let override_data: Value = from_str(
            r#"
level2:
  key: modified
  new_key: added
"#,
        )
        .unwrap();

        override_recursive(&mut config, &Value::String("level1".into()), override_data);

        let level1 = config.get("level1").unwrap().as_mapping().unwrap();
        let level2 = level1.get("level2").unwrap().as_mapping().unwrap();
        assert_eq!(level2.get("key"), Some(&Value::String("modified".into())));
        assert_eq!(level2.get("new_key"), Some(&Value::String("added".into())));
    }

    // ==================== apply_overrides tests ====================

    #[test]
    fn test_apply_overrides_multiple() {
        let mut config: Mapping = from_str(
            r#"
key1: value1
nested:
  a: 1
"#,
        )
        .unwrap();

        let overrides: Mapping = from_str(
            r#"
key1: override1
key2: new_key
nested:
  b: 2
"#,
        )
        .unwrap();

        apply_overrides(&mut config, &overrides);

        assert_eq!(config.get("key1"), Some(&Value::String("override1".into())));
        assert_eq!(config.get("key2"), Some(&Value::String("new_key".into())));

        let nested = config.get("nested").unwrap().as_mapping().unwrap();
        assert_eq!(nested.get("a"), Some(&Value::Number(1.into())));
        assert_eq!(nested.get("b"), Some(&Value::Number(2.into())));
    }

    #[test]
    fn test_apply_overrides_empty() {
        let mut config: Mapping = from_str("key1: value1").unwrap();
        let overrides: Mapping = Mapping::new();

        apply_overrides(&mut config, &overrides);

        assert_eq!(config.get("key1"), Some(&Value::String("value1".into())));
    }

    // ==================== find_field tests ====================

    #[test]
    fn test_find_field_simple() {
        let mut config: Value = from_str("key1: value1\nkey2: value2").unwrap();
        let result = find_field(&mut config, "key1");
        assert_eq!(result, Some(&mut Value::String("value1".into())));
    }

    #[test]
    fn test_find_field_nested() {
        let mut config: Value = from_str(
            r#"
nested:
  deep:
    key: value
"#,
        )
        .unwrap();

        let result = find_field(&mut config, "nested.deep.key");
        assert_eq!(result, Some(&mut Value::String("value".into())));
    }

    #[test]
    fn test_find_field_array_index() {
        let mut config: Value = from_str(
            r#"
arr:
  - item0
  - item1
  - item2
"#,
        )
        .unwrap();

        assert_eq!(
            find_field(&mut config, "arr.0"),
            Some(&mut Value::String("item0".into()))
        );
        assert_eq!(
            find_field(&mut config, "arr.1"),
            Some(&mut Value::String("item1".into()))
        );
        assert_eq!(
            find_field(&mut config, "arr.2"),
            Some(&mut Value::String("item2".into()))
        );
    }

    #[test]
    fn test_find_field_nested_in_array() {
        let mut config: Value = from_str(
            r#"
arr:
  - name: first
    value: 1
  - name: second
    value: 2
"#,
        )
        .unwrap();

        assert_eq!(
            find_field(&mut config, "arr.0.name"),
            Some(&mut Value::String("first".into()))
        );
        assert_eq!(
            find_field(&mut config, "arr.1.value"),
            Some(&mut Value::Number(2.into()))
        );
    }

    #[test]
    fn test_find_field_not_found() {
        let mut config: Value = from_str("key1: value1").unwrap();
        assert_eq!(find_field(&mut config, "nonexistent"), None);
        assert_eq!(find_field(&mut config, "key1.nested"), None);
    }

    #[test]
    fn test_find_field_partial_path_not_mapping() {
        let mut config: Value = from_str("key1: value1").unwrap();
        // key1 is a string, not a mapping, so can't traverse further
        assert_eq!(find_field(&mut config, "key1.sub"), None);
    }

    // ==================== diff_fields tests ====================

    #[test]
    fn test_diff_fields_identical() {
        let a: Mapping = from_str("key1: value1\nkey2: value2").unwrap();
        let b: Mapping = from_str("key1: value1\nkey2: value2").unwrap();

        let diffs = diff_fields(&a, &b);
        assert!(diffs.is_empty());
    }

    #[test]
    fn test_diff_fields_different_value() {
        let a: Mapping = from_str("key1: value1\nkey2: value2").unwrap();
        let b: Mapping = from_str("key1: changed\nkey2: value2").unwrap();

        let diffs = diff_fields(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert!(diffs.contains("key1"));
    }

    #[test]
    fn test_diff_fields_key_only_in_a() {
        let a: Mapping = from_str("key1: value1\nkey2: value2").unwrap();
        let b: Mapping = from_str("key1: value1").unwrap();

        let diffs = diff_fields(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert!(diffs.contains("key2"));
    }

    #[test]
    fn test_diff_fields_key_only_in_b() {
        let a: Mapping = from_str("key1: value1").unwrap();
        let b: Mapping = from_str("key1: value1\nkey2: value2").unwrap();

        let diffs = diff_fields(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert!(diffs.contains("key2"));
    }

    #[test]
    fn test_diff_fields_nested_difference() {
        let a: Mapping = from_str(
            r#"
nested:
  key1: value1
  key2: value2
"#,
        )
        .unwrap();

        let b: Mapping = from_str(
            r#"
nested:
  key1: changed
  key2: value2
"#,
        )
        .unwrap();

        let diffs = diff_fields(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert!(diffs.contains("nested.key1"));
    }

    #[test]
    fn test_diff_fields_deeply_nested() {
        let a: Mapping = from_str(
            r#"
level1:
  level2:
    level3:
      key: original
"#,
        )
        .unwrap();

        let b: Mapping = from_str(
            r#"
level1:
  level2:
    level3:
      key: changed
"#,
        )
        .unwrap();

        let diffs = diff_fields(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert!(diffs.contains("level1.level2.level3.key"));
    }

    #[test]
    fn test_diff_fields_multiple_differences() {
        let a: Mapping = from_str(
            r#"
key1: value1
nested:
  a: 1
  b: 2
extra: only_in_a
"#,
        )
        .unwrap();

        let b: Mapping = from_str(
            r#"
key1: changed
nested:
  a: 1
  c: 3
another: only_in_b
"#,
        )
        .unwrap();

        let diffs = diff_fields(&a, &b);
        assert!(diffs.contains("key1")); // different value
        assert!(diffs.contains("nested.b")); // only in a
        assert!(diffs.contains("nested.c")); // only in b
        assert!(diffs.contains("extra")); // only in a
        assert!(diffs.contains("another")); // only in b
        assert_eq!(diffs.len(), 5);
    }

    #[test]
    fn test_diff_fields_type_change() {
        let a: Mapping = from_str(
            r#"
field:
  nested: value
"#,
        )
        .unwrap();

        let b: Mapping = from_str(
            r#"
field: simple_value
"#,
        )
        .unwrap();

        let diffs = diff_fields(&a, &b);
        assert!(diffs.contains("field"));
    }

    #[test]
    fn test_diff_fields_empty_mappings() {
        let a: Mapping = Mapping::new();
        let b: Mapping = Mapping::new();

        let diffs = diff_fields(&a, &b);
        assert!(diffs.is_empty());
    }

    #[test]
    fn test_diff_fields_one_empty() {
        let a: Mapping = from_str("key1: value1").unwrap();
        let b: Mapping = Mapping::new();

        let diffs = diff_fields(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert!(diffs.contains("key1"));

        let diffs_reverse = diff_fields(&b, &a);
        assert_eq!(diffs_reverse.len(), 1);
        assert!(diffs_reverse.contains("key1"));
    }

    #[test]
    fn test_diff_fields_number_keys() {
        let a: Mapping = from_str("1: value1\n2: value2").unwrap();
        let b: Mapping = from_str("1: value1\n2: changed").unwrap();

        let diffs = diff_fields(&a, &b);
        assert_eq!(diffs.len(), 1);
        assert!(diffs.contains("2"));
    }
}
