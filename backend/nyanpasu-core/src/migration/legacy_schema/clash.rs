use serde_yaml_ng::{Mapping, Value};
use std::{net::SocketAddr, str::FromStr};

#[derive(Default, Debug, Clone)]
pub struct IClashTemp(pub Mapping);

impl IClashTemp {
    pub fn template() -> Self {
        let mut map = Mapping::new();

        map.insert("mixed-port".into(), 7890.into());
        map.insert("log-level".into(), "info".into());
        map.insert("allow-lan".into(), false.into());
        map.insert("mode".into(), "rule".into());
        #[cfg(debug_assertions)]
        map.insert("external-controller".into(), "127.0.0.1:9872".into());
        #[cfg(not(debug_assertions))]
        map.insert("external-controller".into(), "127.0.0.1:17650".into());
        map.insert(
            "secret".into(),
            uuid::Uuid::new_v4().to_string().to_lowercase().into(), // generate a uuid v4 as default secret to secure the communication between clash and the client
        );
        #[cfg(feature = "default-meta")]
        map.insert("unified-delay".into(), true.into());
        #[cfg(feature = "default-meta")]
        map.insert("tcp-concurrent".into(), true.into());
        map.insert("ipv6".into(), false.into());

        Self(map)
    }

    pub fn guard_mixed_port(config: &Mapping) -> u16 {
        let mut port = config
            .get("mixed-port")
            .and_then(|value| match value {
                Value::String(val_str) => val_str.parse().ok(),
                Value::Number(val_num) => val_num.as_u64().map(|u| u as u16),
                _ => None,
            })
            .unwrap_or(7890);
        if port == 0 {
            port = 7890;
        }
        port
    }

    pub fn guard_server_ctrl(config: &Mapping) -> String {
        config
            .get("external-controller")
            .and_then(|value| match value.as_str() {
                Some(val_str) => {
                    let val_str = val_str.trim();

                    let val = match val_str.starts_with(':') {
                        true => format!("127.0.0.1{val_str}"),
                        false => val_str.to_owned(),
                    };

                    SocketAddr::from_str(val.as_str())
                        .ok()
                        .map(|s| s.to_string())
                }
                None => None,
            })
            .unwrap_or("127.0.0.1:9090".into())
    }
}

#[test]
fn test_clash_info() {
    fn get_endpoint(map: Mapping) -> (u16, String) {
        (
            IClashTemp::guard_mixed_port(&map),
            IClashTemp::guard_server_ctrl(&map),
        )
    }

    fn get_case<T: Into<Value>, D: Into<Value>>(mp: T, ec: D) -> (u16, String) {
        let mut map = Mapping::new();
        map.insert("mixed-port".into(), mp.into());
        map.insert("external-controller".into(), ec.into());

        get_endpoint(map)
    }

    fn get_result<S: Into<String>>(port: u16, server: S) -> (u16, String) {
        (port, server.into())
    }

    assert_eq!(
        get_endpoint(Mapping::new()),
        get_result(7890, "127.0.0.1:9090")
    );

    assert_eq!(get_case("", ""), get_result(7890, "127.0.0.1:9090"));

    assert_eq!(get_case(65537, ""), get_result(1, "127.0.0.1:9090"));

    assert_eq!(
        get_case(8888, "127.0.0.1:8888"),
        get_result(8888, "127.0.0.1:8888")
    );

    assert_eq!(
        get_case(8888, "   :98888 "),
        get_result(8888, "127.0.0.1:9090")
    );

    assert_eq!(
        get_case(8888, "   :8080 "),
        get_result(8888, "127.0.0.1:8080")
    );

    assert_eq!(
        get_case(8888, "0.0.0.0:8080  "),
        get_result(8888, "0.0.0.0:8080")
    );

    assert_eq!(get_case(8888, "[::]:8080"), get_result(8888, "[::]:8080"));

    assert_eq!(
        get_case(8888, "192.168.1.1:8080"),
        get_result(8888, "192.168.1.1:8080")
    );

    assert_eq!(
        get_case(8888, "192.168.1.1:80800"),
        get_result(8888, "127.0.0.1:9090")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_preserves_default_meta_fields() {
        let template = IClashTemp::template();
        for key in ["unified-delay", "tcp-concurrent"] {
            if cfg!(feature = "default-meta") {
                assert_eq!(template.0.get(key), Some(&Value::Bool(true)));
            } else {
                assert!(!template.0.contains_key(key));
            }
        }
    }
}
