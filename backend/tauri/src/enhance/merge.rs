use super::{Logs, LogsExt, runner::ProcessOutput};
use crate::utils::yaml::{apply_overrides, find_field, override_recursive};
use mlua::LuaSerdeExt;
use serde::de::DeserializeOwned;
use serde_yaml::{Mapping, Value};
use tracing_attributes::instrument;

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

fn run_expr<T: DeserializeOwned>(logs: &mut Logs, item: &Value, expr: &str) -> Option<T> {
    let lua_runtime = match super::script::create_lua_context() {
        Ok(lua) => lua,
        Err(e) => {
            logs.error(e.to_string());
            return None;
        }
    };
    let item = match lua_runtime.to_value(item) {
        Ok(v) => v,
        Err(e) => {
            logs.error(format!("failed to convert item to lua value: {e:#?}"));
            return None;
        }
    };

    if let Err(e) = lua_runtime.globals().set("item", item) {
        logs.error(e.to_string());
        return None;
    }
    let res = lua_runtime.load(expr).eval::<mlua::Value>();
    match res {
        Ok(v) => {
            if let Ok(v) = lua_runtime.from_value(v) {
                Some(v)
            } else {
                logs.error("failed to convert lua value to serde value");
                None
            }
        }
        Err(e) => {
            logs.error(format!("failed to run expr: {e:#?}"));
            None
        }
    }
}

fn do_filter(logs: &mut Logs, config: &mut Value, field_str: &str, filter: &Value) {
    let field = match find_field(config, field_str) {
        Some(field) if !field.is_sequence() => {
            logs.warn(format!("field is not sequence: {field_str:#?}"));
            return;
        }
        Some(field) => field,
        None => {
            logs.warn(format!("field not found: {field_str:#?}"));
            return;
        }
    };
    match filter {
        Value::Sequence(filters) => {
            for filter in filters {
                do_filter(logs, config, field_str, filter);
            }
        }
        Value::String(filter) => {
            let list = field.as_sequence_mut().unwrap();
            list.retain(|item| run_expr(logs, item, filter).unwrap_or(false));
        }
        Value::Mapping(filter)
            if filter.get("when").is_some_and(|v| v.is_string())
                && filter.get("expr").is_some_and(|v| v.is_string()) =>
        {
            let when = filter.get("when").unwrap().as_str().unwrap();
            let expr = filter.get("expr").unwrap().as_str().unwrap();
            let list = field.as_sequence_mut().unwrap();
            list.iter_mut().for_each(|item| {
                let r#match = run_expr(logs, item, when);
                if r#match.unwrap_or(false) {
                    let res: Option<Value> = run_expr(logs, item, expr);
                    if let Some(res) = res {
                        *item = res;
                    }
                }
            });
        }
        Value::Mapping(filter)
            if filter.get("when").is_some_and(|v| v.is_string())
                && filter.contains_key("override") =>
        {
            let when = filter.get("when").unwrap().as_str().unwrap();
            let r#override = filter.get("override").unwrap();
            let list = field.as_sequence_mut().unwrap();
            list.iter_mut().for_each(|item| {
                let r#match = run_expr(logs, item, when);
                if r#match.unwrap_or(false) {
                    *item = r#override.clone();
                }
            });
        }
        Value::Mapping(filter)
            if filter.get("when").is_some_and(|v| v.is_string())
                && filter.get("merge").is_some_and(|v| v.is_mapping()) =>
        {
            let when = filter.get("when").unwrap().as_str().unwrap();
            let merge = filter.get("merge").unwrap().as_mapping().unwrap();
            let list = field.as_sequence_mut().unwrap();
            list.iter_mut().for_each(|item| {
                let r#match = run_expr(logs, item, when);
                if r#match.unwrap_or(false) {
                    for (key, value) in merge.iter() {
                        let item = item.as_mapping_mut().unwrap();
                        if item.contains_key(key) {
                            override_recursive(item, key, value.clone());
                        } else {
                            item.insert(key.clone(), value.clone());
                        }
                    }
                }
            });
        }

        Value::Mapping(filter)
            if filter.get("when").is_some_and(|v| v.is_string())
                && filter.get("remove").is_some_and(|v| v.is_sequence()) =>
        {
            let when = filter.get("when").unwrap().as_str().unwrap();
            let remove = filter.get("remove").unwrap().as_sequence().unwrap();
            let list = field.as_sequence_mut().unwrap();
            list.iter_mut().for_each(|item| {
                let r#match = run_expr(logs, item, when);
                if r#match.unwrap_or(false) {
                    remove.iter().for_each(|key| {
                        if key.is_string() && item.is_mapping() {
                            let key_str = key.as_str().unwrap();
                            // 对 key_str 做一下处理，跳过最后一个元素
                            let mut keys = key_str.split('.').collect::<Vec<_>>();
                            let last_key = if keys.len() > 1 { keys.pop() } else { None };
                            let key_str = keys.join(".");
                            match last_key {
                                None => {
                                    item.as_mapping_mut().unwrap().remove(key_str);
                                }
                                Some(last_key) => {
                                    let field = find_field(item, &key_str);
                                    if let Some(field) = field {
                                        match field {
                                            Value::Mapping(map) => {
                                                map.remove(last_key);
                                            }
                                            Value::Sequence(list)
                                                if last_key.parse::<usize>().is_ok() =>
                                            {
                                                let index = last_key.parse::<usize>().unwrap();
                                                if index < list.len() {
                                                    list.remove(index);
                                                }
                                            }
                                            _ => {
                                                logs.info(format!("invalid key: {last_key:#?}"));
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            match item {
                                Value::Sequence(list) if key.is_i64() => {
                                    let index = key.as_i64().unwrap();
                                    if index >= 0 && (index as usize) < list.len() {
                                        list.remove(index as usize);
                                    }
                                }
                                _ => {
                                    logs.info(format!("invalid key: {key:#?}"));
                                }
                            }
                        }
                    });
                }
            });
        }

        _ => {
            logs.warn(format!("invalid filter: {filter:#?}"));
        }
    }
}

#[instrument(skip(merge, config))]
pub fn use_merge(merge: &Mapping, mut config: Mapping) -> ProcessOutput {
    tracing::trace!("original config: {:#?}", config);
    tracing::trace!("merge: {:#?}", merge);
    let mut logs = Logs::new();
    let mut map = Value::from(config);
    for (key, value) in merge.iter() {
        let key_str = key.as_str().unwrap_or_default().to_lowercase();
        match key_str {
            key_str if key_str.starts_with("prepend__") || key_str.starts_with("prepend-") => {
                if !value.is_sequence() {
                    logs.warn(format!("prepend value is not sequence: {key_str:#?}"));
                    continue;
                }
                let key_str = key_str.replace("prepend__", "").replace("prepend-", "");
                let field = find_field(&mut map, &key_str);
                match field {
                    Some(field) => {
                        if field.is_sequence() {
                            merge_sequence(field, value, false);
                        } else {
                            logs.warn(format!("field is not sequence: {key_str:#?}"));
                        }
                    }
                    None => {
                        logs.warn(format!("field not found: {key_str:#?}"));
                    }
                }
                continue;
            }
            key_str if key_str.starts_with("append__") || key_str.starts_with("append-") => {
                if !value.is_sequence() {
                    logs.warn(format!("append value is not sequence: {key_str:#?}"));
                    continue;
                }
                let key_str = key_str.replace("append__", "").replace("append-", "");
                let field = find_field(&mut map, &key_str);
                match field {
                    Some(field) => {
                        if field.is_sequence() {
                            merge_sequence(field, value, true);
                        } else {
                            logs.warn(format!("field is not sequence: {key_str:#?}"));
                        }
                    }
                    None => {
                        logs.warn(format!("field not found: {key_str:#?}"));
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
                        logs.warn(format!("field not found: {key_str:#?}"));
                    }
                }
                continue;
            }
            key_str if key_str.starts_with("filter__") => {
                let key_str = key_str.replace("filter__", "");
                do_filter(&mut logs, &mut map, &key_str, value);
                continue;
            }
            _ => {
                override_recursive(map.as_mapping_mut().unwrap(), key, value.clone());
            }
        }
    }
    config = map.as_mapping().unwrap().clone();
    tracing::trace!("merged config: {:#?}", config);
    (Ok(config), logs)
}

mod tests {
    #[allow(unused_imports)]
    use pretty_assertions::{assert_eq, assert_ne};

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
        eprintln!("{config:#?}");
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
        let (result, logs) = super::use_merge(&merge, config);
        eprintln!("{logs:#?}\n\n{result:#?}");
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
        let (result, logs) = super::use_merge(&merge, config);
        eprintln!("{logs:#?}\n\n{result:#?}");
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
        let (result, logs) = super::use_merge(&merge, config);
        eprintln!("{logs:#?}\n\n{result:#?}");
        let expected = serde_yaml::from_str::<super::Mapping>(expected).unwrap();
        assert_eq!(logs.len(), 1); // field not found: nothing
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_filter_string() {
        let merge = r"
        filter__proxies: |
          type(item) == 'table' and (item.type == 'ss' or item.type == 'hysteria2')
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
        let (result, logs) = super::use_merge(&merge, config);
        eprintln!("{logs:#?}\n\n{result:#?}");
        assert!(logs.len() == 1, "filter_wow should not work");
        let expected = serde_yaml::from_str::<super::Mapping>(expected).unwrap();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_filter_when_and_expr() {
        let merge = r"
        filter__proxies:
          - when: |
              type(item) == 'table' and (item.type == 'ss' or item.type == 'hysteria2')
            expr: |
              item
        filter__proxy-groups:
          - when: |
              item.name == 'Spotify'
            expr: |
              item.icon = 'https://raw.githubusercontent.com/Koolson/Qure/master/IconSet/Color/Spotify.png'
              return item
        ";
        let config = r#"proxy-groups:
- name: Spotify
  type: select
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Steam
  type: select
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Telegram
  type: select
  proxies:
  - Proxies
  - HK
  - JP
  - SG
  - TW
  - US"#;
        let expected = r#"proxy-groups:
- name: Spotify
  icon: https://raw.githubusercontent.com/Koolson/Qure/master/IconSet/Color/Spotify.png
  type: select
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Steam
  type: select
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Telegram
  type: select
  proxies:
  - Proxies
  - HK
  - JP
  - SG
  - TW
  - US"#;
        let merge = serde_yaml::from_str::<super::Mapping>(merge).unwrap();
        let config = serde_yaml::from_str::<super::Mapping>(config).unwrap();
        let (result, logs) = super::use_merge(&merge, config);
        eprintln!("{logs:#?}\n\n{result:#?}");
        assert_eq!(logs.len(), 1);
        let expected = serde_yaml::from_str::<super::Mapping>(expected).unwrap();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_filter_when_and_override() {
        let merge = r"
        filter__proxies:
          - when: |
              type(item) == 'table' and (item.type == 'ss' or item.type == 'hysteria2')
            override: OVERRIDDEN
        ";
        let config = r#"
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
        proxies:
          - 123
          - 555
          - OVERRIDDEN
          - OVERRIDDEN
        "#;
        let merge = serde_yaml::from_str::<super::Mapping>(merge).unwrap();
        let config = serde_yaml::from_str::<super::Mapping>(config).unwrap();
        let (result, logs) = super::use_merge(&merge, config);
        eprintln!("{logs:#?}\n\n{result:#?}");
        assert_eq!(logs.len(), 0);
        let expected = serde_yaml::from_str::<super::Mapping>(expected).unwrap();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_filter_when_and_merge() {
        let merge = r"
        filter__proxy-groups:
          when: |
            item.name == 'Spotify'
          merge:
            icon: 'https://raw.githubusercontent.com/Koolson/Qure/master/IconSet/Color/Spotify.png'
        filter__wow:
          when: |
            item == 'wow'
          merge:
            item: 'wow'";
        let config = r#"proxy-groups:
- name: Spotify
  type: select
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Steam
  type: select
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Telegram
  type: select
  proxies:
  - Proxies
  - HK
  - JP
  - SG
  - TW
  - US"#;
        let expected = r#"proxy-groups:
- name: Spotify
  type: select
  icon: https://raw.githubusercontent.com/Koolson/Qure/master/IconSet/Color/Spotify.png
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Steam
  type: select
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Telegram
  type: select
  proxies:
  - Proxies
  - HK
  - JP
  - SG
  - TW
  - US"#;
        let merge = serde_yaml::from_str::<super::Mapping>(merge).unwrap();
        let config = serde_yaml::from_str::<super::Mapping>(config).unwrap();
        let (result, logs) = super::use_merge(&merge, config);
        eprintln!("{logs:#?}\n\n{result:#?}");
        assert_eq!(logs.len(), 1);
        let expected = serde_yaml::from_str::<super::Mapping>(expected).unwrap();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_filter_when_and_remove() {
        let merge = r"
        filter__proxies:
          when: |
            type(item) == 'table' and (item.type == 'ss' or item.type == 'hysteria2')
          remove:
            - name
            - type
        filter__list: # note that Lua table index starts from 1
          when: |
            item[1] == 123
          remove:
            - 0
        filter__wow:
          when: |
            item.flag == true
          remove:
            - test.1
            - good.should_remove
        ";
        let config = r#"
        wow:
          - test:
               - 123
               - 456
            flag: true
          - good:
              should_remove: true
              should_not_remove: true
            flag: true
        list:
          - - 123
            - 456
            - 222
          - - 123
            - 456
            - 222
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
        wow:
          - test:
               - 123
            flag: true
          - good:
              should_not_remove: true
            flag: true
        list:
          - - 456
            - 222
          - - 456
            - 222
        proxies:
          - 123
          - 555
          - server: server.com
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
          - server: server.com
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
        let (result, logs) = super::use_merge(&merge, config);
        eprintln!("{logs:#?}\n\n{result:#?}");
        assert_eq!(logs.len(), 0);
        let expected = serde_yaml::from_str::<super::Mapping>(expected).unwrap();
        assert_eq!(result.unwrap(), expected);
    }

    #[test]
    fn test_filter_sequence() {
        let merge = r"
        filter__proxy-groups:
          - when: |
              item.name == 'Spotify'
            merge:
              icon: 'https://raw.githubusercontent.com/Koolson/Qure/master/IconSet/Color/Spotify.png'
          - when: |
              item.name == 'Steam'
            merge:
              icon: 'https://raw.githubusercontent.com/Koolson/Qure/master/IconSet/Color/Steam.png'
          - when: |
              item.name == 'Telegram'
            merge:
              icon: 'https://raw.githubusercontent.com/Koolson/Qure/master/IconSet/Color/Telegram.png'  
              ";
        let config = r#"proxy-groups:
- name: Spotify
  type: select
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Steam
  type: select
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Telegram
  type: select
  proxies:
  - Proxies
  - HK
  - JP
  - SG
  - TW
  - US"#;
        let expected = r#"proxy-groups:
- name: Spotify
  type: select
  icon: https://raw.githubusercontent.com/Koolson/Qure/master/IconSet/Color/Spotify.png
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Steam
  type: select
  icon: https://raw.githubusercontent.com/Koolson/Qure/master/IconSet/Color/Steam.png
  proxies:
  - Proxies
  - DIRECT
  - HK
  - JP
  - SG
  - TW
  - US
- name: Telegram
  type: select
  icon: https://raw.githubusercontent.com/Koolson/Qure/master/IconSet/Color/Telegram.png
  proxies:
  - Proxies
  - HK
  - JP
  - SG
  - TW
  - US"#;
        let merge = serde_yaml::from_str::<super::Mapping>(merge).unwrap();
        let config = serde_yaml::from_str::<super::Mapping>(config).unwrap();
        let (result, logs) = super::use_merge(&merge, config);
        eprintln!("{logs:#?}\n\n{result:#?}");
        assert_eq!(logs.len(), 0);
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

        let (result, logs) = super::use_merge(&merge, config);
        eprintln!("{logs:#?}\n\n{result:#?}");
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
