use super::{runner::ProcessOutput, use_lowercase, Logs, LogsExt};
use mlua::LuaSerdeExt;
use serde_yaml::{Mapping, Value};
use tracing_attributes::instrument;

// Override recursive, and if the value is sequence, it should be append to the end.
fn override_recursive(config: &mut Mapping, key: &Value, data: Value) {
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

/// Key should be a.b.c to access the value
fn find_field<'a>(config: &'a mut Value, key: &'a str) -> Option<&'a mut Value> {
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

fn merge_sequence(target: &mut Value, to_merge: &Value, append: bool) {
    if target.is_sequence() && to_merge.is_sequence() {
        let target = target.as_sequence_mut().unwrap();
        let to_merge = to_merge.as_sequence().unwrap();
        if append {
            target.extend(to_merge.clone());
        } else {
            target.splice(0..0, to_merge.iter().cloned());
        }
    }
}

#[instrument(skip(merge, config))]
pub fn use_merge(merge: Mapping, mut config: Mapping) -> ProcessOutput {
    tracing::trace!("original config: {:#?}", config);
    tracing::trace!("merge: {:#?}", merge);
    let mut logs = Logs::new();
    let mut map = Value::from(config);
    for (key, value) in merge.iter() {
        let key_str = key.as_str().unwrap_or_default().to_lowercase();
        match key_str {
            key_str if key_str.starts_with("prepend__") || key_str.starts_with("prepend-") => {
                if !value.is_sequence() {
                    logs.warn(format!("prepend value is not sequence: {:#?}", key_str));
                    continue;
                }
                let key_str = key_str.replace("prepend__", "").replace("prepend-", "");
                let field = find_field(&mut map, &key_str);
                match field {
                    Some(field) => {
                        if field.is_sequence() {
                            merge_sequence(field, &value, false);
                        } else {
                            logs.warn(format!("field is not sequence: {:#?}", key_str));
                        }
                    }
                    None => {
                        logs.warn(format!("field not found: {:#?}", key_str));
                    }
                }
                continue;
            }
            key_str if key_str.starts_with("append__") || key_str.starts_with("append-") => {
                if !value.is_sequence() {
                    logs.warn(format!("append value is not sequence: {:#?}", key_str));
                    continue;
                }
                let key_str = key_str.replace("append__", "").replace("append-", "");
                let field = find_field(&mut map, &key_str);
                match field {
                    Some(field) => {
                        if field.is_sequence() {
                            merge_sequence(field, &value, true);
                        } else {
                            logs.warn(format!("field is not sequence: {:#?}", key_str));
                        }
                    }
                    None => {
                        logs.warn(format!("field not found: {:#?}", key_str));
                    }
                }
                continue;
            }
            key_str if key_str.starts_with("override__") => {
                let key_str = key_str.replace("override__", "");
                let field = find_field(&mut map, &key_str);
                match field {
                    Some(field) => {
                        *field = value.clone();
                    }
                    None => {
                        logs.warn(format!("field not found: {:#?}", key_str));
                    }
                }
                continue;
            }
            key_str if key_str.starts_with("filter__") => {
                let key_str = key_str.replace("filter__", "");
                if !value.is_string() {
                    logs.warn(format!("filter value is not string: {:#?}", key_str));
                    continue;
                }
                let field = find_field(&mut map, &key_str);
                match field {
                    Some(field) => {
                        if !field.is_sequence() {
                            logs.warn(format!("field is not sequence: {:#?}", key_str));
                            continue;
                        }
                        let filter = value.as_str().unwrap_or_default();
                        let lua = match super::script::create_lua_context() {
                            Ok(lua) => lua,
                            Err(e) => {
                                logs.error(e.to_string());
                                continue;
                            }
                        };

                        let list = field.as_sequence_mut().unwrap();
                        // apply filter to each item
                        list.retain(|item| {
                            let item = lua.to_value(item).unwrap();
                            if let Err(e) = lua.globals().set("item", item) {
                                logs.error(e.to_string());
                                return false;
                            }
                            lua.load(filter).eval::<bool>().unwrap_or(false)
                        });
                    }
                    None => {
                        logs.warn(format!("field not found: {:#?}", key_str));
                    }
                }
                continue;
            }
            _ => {
                override_recursive(map.as_mapping_mut().unwrap(), &key, value.clone());
            }
        }
    }
    config = map.as_mapping().unwrap().clone();
    tracing::trace!("merged config: {:#?}", config);
    (Ok(config), logs)
}

mod tests {
    #[test]
    fn test_find_field() {
        let config = r"
        a:
          b:
            c:
            - 111
            - 222
        ";
        let mut config = serde_yaml::from_str::<super::Value>(config).unwrap();
        eprintln!("{:#?}", config);
        let field = super::find_field(&mut config, "a.b.c");
        assert!(field.is_some(), "a.b.c should be found");
        let field = super::find_field(&mut config, "a.b");
        assert!(field.is_some(), "a.b should be found");
        let field = super::find_field(&mut config, "a.b.c.0");
        assert!(field.is_some(), "a.b.c.0 should be found");
        let field = super::find_field(&mut config, "a.b.c.1");
        assert!(field.is_some(), "a.b.c.1 should be found");
        let field = super::find_field(&mut config, "a.b.c.2");
        assert!(field.is_none(), "a.b.c.2 should not be found");
    }

    #[test]
    fn test_merge_append() {
        let merge = r"
        append-proxies:
          - 666
        append__proxies:
          - 555
        append__a.b.c:
          - 12321
          - 44444
        append__nothing:
          - nothing
        ";
        let config = r"
        proxies:
          - 123
        a:
          b:
            c:
            - 111
            - 222
        ";
        let expected = r"
        proxies:
          - 123
          - 666
          - 555
        a:
          b:
            c:
            - 111
            - 222
            - 12321
            - 44444
        ";
        let merge = serde_yaml::from_str::<super::Mapping>(merge).unwrap();
        let config = serde_yaml::from_str::<super::Mapping>(config).unwrap();
        let (result, logs) = super::use_merge(merge, config);
        eprintln!("{:#?}\n\n{:#?}", logs, result);
        let expected = serde_yaml::from_str::<super::Mapping>(expected).unwrap();
        assert_eq!(logs.len(), 1); // field not found: nothing
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_prepend() {
        let merge = r"
        prepend-proxies:
          - 666
        prepend__proxies:
          - 555
        prepend__a.b.c:
          - 12321
          - 44444
        prepend__nothing:
          - nothing
        ";
        let config = r"
        proxies:
          - 123
        a:
          b:
            c:
            - 111
            - 222
        ";
        let expected = r"
        proxies:
          - 555
          - 666
          - 123
        a:
          b:
            c:
            - 12321
            - 44444
            - 111
            - 222
        ";
        let merge = serde_yaml::from_str::<super::Mapping>(merge).unwrap();
        let config = serde_yaml::from_str::<super::Mapping>(config).unwrap();
        let (result, logs) = super::use_merge(merge, config);
        eprintln!("{:#?}\n\n{:#?}", logs, result);
        let expected = serde_yaml::from_str::<super::Mapping>(expected).unwrap();
        assert_eq!(logs.len(), 1); // field not found: nothing
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_override() {
        let merge = r"
        override__proxies:
          - 555
        override__a.b.c:
          - 12321
          - 44444
        override__nothing:
          - nothing
        override__a.f.0: wow
        ";
        let config = r"
        proxies:
          - 123
        a:
          b:
            c:
            - 111
            - 222
          f:
            - 444
        ";
        let expected = r"
        proxies:
          - 555
        a:
          b:
            c:
            - 12321
            - 44444
          f:
            - wow   
        ";
        let merge = serde_yaml::from_str::<super::Mapping>(merge).unwrap();
        let config = serde_yaml::from_str::<super::Mapping>(config).unwrap();
        let (result, logs) = super::use_merge(merge, config);
        eprintln!("{:#?}\n\n{:#?}", logs, result);
        let expected = serde_yaml::from_str::<super::Mapping>(expected).unwrap();
        assert_eq!(logs.len(), 1); // field not found: nothing
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_filter() {
        let merge = r"
        filter__proxies: |
          item.type == 'ss' or item.type == 'hysteria2'
        filter__wow: |
          item == 'wow'
        ";
        let config = r#"
        wow: 123
        proxies:
          - 123
          - 555
          - name: "hysteria2"
            type: hysteria2
            server: server.com
            port: 443
            ports: 443-8443
            password: yourpassword
            up: "30 Mbps"
            down: "200 Mbps"
            obfs: salamander # 默认为空，如果填写则开启obfs，目前仅支持salamander
            obfs-password: yourpassword

            sni: server.com
            skip-cert-verify: false
            fingerprint: xxxx
            alpn:
              - h3
            ca: "./my.ca"
            ca-str: "xyz"
          - name: "hysteria2"
            type: ss
            server: server.com
            port: 443
            ports: 443-8443
            password: yourpassword
            up: "30 Mbps"
            down: "200 Mbps"
            obfs: salamander # 默认为空，如果填写则开启obfs，目前仅支持salamander
            obfs-password: yourpassword

            sni: server.com
            skip-cert-verify: false
            fingerprint: xxxx
            alpn:
              - h3
            ca: "./my.ca"
            ca-str: "xyz"            
        "#;
        let expected = r#"
        wow: 123
        proxies:
            -   name: hysteria2
                type: hysteria2
                server: server.com
                port: 443
                ports: 443-8443
                password: yourpassword
                up: "30 Mbps"
                down: "200 Mbps"
                obfs: salamander
                obfs-password: yourpassword
                sni: server.com
                skip-cert-verify: false
                fingerprint: xxxx
                alpn:
                - h3
                ca: "./my.ca"
                ca-str: "xyz"
            -   name: hysteria2
                type: ss
                server: server.com
                port: 443
                ports: 443-8443
                password: yourpassword
                up: "30 Mbps"
                down: "200 Mbps"
                obfs: salamander
                obfs-password: yourpassword
                sni: server.com
                skip-cert-verify: false
                fingerprint: xxxx
                alpn:
                - h3
                ca: "./my.ca"
                ca-str: "xyz"
        "#;
        let merge = serde_yaml::from_str::<super::Mapping>(merge).unwrap();
        let config = serde_yaml::from_str::<super::Mapping>(config).unwrap();
        let (result, logs) = super::use_merge(merge, config);
        eprintln!("{:#?}\n\n{:#?}", logs, result);
        assert!(logs.len() == 1, "filter_wow should not work");
        let expected = serde_yaml::from_str::<super::Mapping>(expected).unwrap();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_override_recursive() {
        let merge = r"
        a:
          b:
            c:
              d: 22323
          f:
          - wow
          e: ttt
        ";
        let config = r#"
        a:
          b:
            c:
              d: 123
          f:
          - 123
          - 456 
          t: will preserve
        "#;

        let merge = serde_yaml::from_str::<super::Mapping>(merge).unwrap();
        let config = serde_yaml::from_str::<super::Mapping>(config).unwrap();

        let (result, logs) = super::use_merge(merge, config);
        eprintln!("{:#?}\n\n{:#?}", logs, result);
        assert_eq!(logs.len(), 0);
        let expected = r#"
        a:
          b:
            c:
              d: 22323
          f:
          - wow
          t: will preserve
          e: ttt
        "#;
        let expected = serde_yaml::from_str::<super::Mapping>(expected).unwrap();
        assert_eq!(result.unwrap(), expected);
    }
}
