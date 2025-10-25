use crate::utils::{
    dirs,
    help::{self, get_clash_external_port},
};
use anyhow::Result;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
};
use tracing::warn;
use tracing_attributes::instrument;

#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    Deserialize,
    Serialize,
    strum::EnumString,
    strum::Display,
    specta::Type,
)]
#[repr(u8)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum LogLevel {
    Silent,
    Error,
    Warning,
    #[default]
    Info,
    Debug,
}

#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    Deserialize,
    Serialize,
    strum::EnumString,
    strum::Display,
    specta::Type,
)]
#[repr(u8)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Rule,
    Global,
    Direct,
}

#[derive(Debug, Clone, Deserialize, Serialize, specta::Type, Builder)]
#[builder(default, derive(Debug, Serialize, Deserialize, specta::Type))]
#[builder_struct_attr(serde_with::skip_serializing_none)]
#[serde(rename_all = "kebab-case")]
pub struct ClashGuardOverrides {
    log_level: LogLevel,
    allow_lan: bool,
    mode: Mode,
    secret: String,
    #[cfg(feature = "default-meta")]
    unified_delay: bool,
    #[cfg(feature = "default-meta")]
    tcp_concurrent: bool,
    ipv6: bool,
}

impl Default for ClashGuardOverrides {
    fn default() -> Self {
        Self {
            log_level: LogLevel::Info,
            allow_lan: false,
            mode: Mode::Rule,
            secret: uuid::Uuid::new_v4().to_string().to_lowercase(),
            #[cfg(feature = "default-meta")]
            unified_delay: true,
            #[cfg(feature = "default-meta")]
            tcp_concurrent: true,
            ipv6: false,
        }
    }
}

impl ClashGuardOverrides {
    /// Apply overrides to the config
    /// # Arguments
    ///
    /// * `config` - The config to apply overrides to
    ///
    /// # Returns
    ///
    /// The config with overrides applied
    ///
    pub fn apply_overrides(&self, mut config: Mapping) -> Mapping {
        use crate::utils::yaml::apply_overrides;
        let overrides = serde_yaml::to_value(self).expect("failed to convert overrides to value");
        let overrides = overrides
            .as_mapping()
            .expect("failed to convert overrides to mapping");
        apply_overrides(&mut config, overrides);
        config
    }
}

// #[test]
// fn test_clash_info() {
//     fn get_case<T: Into<Value>, D: Into<Value>>(mp: T, ec: D) -> ClashInfo {
//         let mut map = Mapping::new();
//         map.insert("mixed-port".into(), mp.into());
//         map.insert("external-controller".into(), ec.into());

//         ClashGuard(ClashGuard::guard(map)).get_client_info()
//     }

//     fn get_result<S: Into<String>>(port: u16, server: S) -> ClashInfo {
//         ClashInfo {
//             port,
//             server: server.into(),
//             secret: None,
//         }
//     }

//     assert_eq!(
//         ClashGuard(ClashGuard::guard(Mapping::new())).get_client_info(),
//         get_result(7890, "127.0.0.1:9090")
//     );

//     assert_eq!(get_case("", ""), get_result(7890, "127.0.0.1:9090"));

//     assert_eq!(get_case(65537, ""), get_result(1, "127.0.0.1:9090"));

//     assert_eq!(
//         get_case(8888, "127.0.0.1:8888"),
//         get_result(8888, "127.0.0.1:8888")
//     );

//     assert_eq!(
//         get_case(8888, "   :98888 "),
//         get_result(8888, "127.0.0.1:9090")
//     );

//     assert_eq!(
//         get_case(8888, "0.0.0.0:8080  "),
//         get_result(8888, "127.0.0.1:8080")
//     );

//     assert_eq!(
//         get_case(8888, "0.0.0.0:8080"),
//         get_result(8888, "127.0.0.1:8080")
//     );

//     assert_eq!(
//         get_case(8888, "[::]:8080"),
//         get_result(8888, "127.0.0.1:8080")
//     );

//     assert_eq!(
//         get_case(8888, "192.168.1.1:8080"),
//         get_result(8888, "192.168.1.1:8080")
//     );

//     assert_eq!(
//         get_case(8888, "192.168.1.1:80800"),
//         get_result(8888, "127.0.0.1:9090")
//     );
// }

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct IClash {
    pub mixed_port: Option<u16>,
    pub allow_lan: Option<bool>,
    pub log_level: Option<String>,
    pub ipv6: Option<bool>,
    pub mode: Option<String>,
    pub external_controller: Option<String>,
    pub secret: Option<String>,
    pub dns: Option<IClashDNS>,
    pub tun: Option<IClashTUN>,
    pub interface_name: Option<String>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct IClashTUN {
    pub enable: Option<bool>,
    pub stack: Option<String>,
    pub auto_route: Option<bool>,
    pub auto_detect_interface: Option<bool>,
    pub dns_hijack: Option<Vec<String>>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct IClashDNS {
    pub enable: Option<bool>,
    pub listen: Option<String>,
    pub default_nameserver: Option<Vec<String>>,
    pub enhanced_mode: Option<String>,
    pub fake_ip_range: Option<String>,
    pub use_hosts: Option<bool>,
    pub fake_ip_filter: Option<Vec<String>>,
    pub nameserver: Option<Vec<String>>,
    pub fallback: Option<Vec<String>>,
    pub fallback_filter: Option<IClashFallbackFilter>,
    pub nameserver_policy: Option<Vec<String>>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct IClashFallbackFilter {
    pub geoip: Option<bool>,
    pub geoip_code: Option<String>,
    pub ipcidr: Option<Vec<String>>,
    pub domain: Option<Vec<String>>,
}
