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
