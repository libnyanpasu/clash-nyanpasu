//! PR-5-pre: daemon 主版本兼容门。
//!
//! v2 的 `StatusResBody` 把所有新增字段都标成了
//! `#[serde(default, skip_serializing_if = "Option::is_none")]`，所以 v1.4.5 daemon 的
//! `/status` 负载是 v2 结构的严格子集，**一定**能静默解码成功。因此本门禁必须做
//! 显式 semver 主版本比较，绝不能依赖解码失败。

use nyanpasu_ipc::types::{ServiceStatus, StatusInfo};

/// PR-5-pre 起，只有主版本等于此值的 daemon 允许承载核心生命周期。
pub const REQUIRED_SERVICE_MAJOR: u64 = 2;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ServiceCompat {
    /// daemon 未安装 / 未运行 / 未上报 server 信息，没有可判定的版本。
    Unknown,
    /// 主版本匹配，允许进入 Service backend。
    Compatible { server_version: String },
    /// 主版本不匹配（典型：v1.4.5）。fail-closed。
    Incompatible {
        server_version: String,
        required_major: u64,
    },
    /// server 上报的版本不是合法 semver。fail-closed。
    Unparsable { server_version: String },
}

impl ServiceCompat {
    /// 纯函数：无 I/O、无全局状态、无 `Config::*()`。
    pub fn classify(info: &StatusInfo<'_>) -> Self {
        if info.status != ServiceStatus::Running {
            return Self::Unknown;
        }

        let Some(server) = info.server.as_ref() else {
            return Self::Unknown;
        };
        let server_version = server.version.to_string();
        let Some(version) = parse_service_version(&server_version) else {
            return Self::Unparsable { server_version };
        };

        if version.major != REQUIRED_SERVICE_MAJOR {
            return Self::Incompatible {
                server_version,
                required_major: REQUIRED_SERVICE_MAJOR,
            };
        }

        Self::Compatible { server_version }
    }

    /// 唯一的放行判据。
    pub fn allows_service_backend(&self) -> bool {
        matches!(self, Self::Compatible { .. })
    }
}

/// 供 `ServiceCompat` 与启动时自动升级检查复用的唯一版本解析入口。
pub fn parse_service_version(raw: &str) -> Option<semver::Version> {
    semver::Version::parse(raw).ok()
}

/// Source: nyanpasu-runtime @ tag v1.4.5 —
/// `nyanpasu_ipc/src/{types,api}/status.rs`.
/// 用途：证明 v1 daemon 的 `/status` 会静默解码成 v2 结构，因此兼容门必须做显式
/// semver 比较。
#[cfg(test)]
pub(super) const STATUS_V1_4_5_FIXTURE: &str = include_str!("fixtures/status_v1_4_5.json");

#[cfg(test)]
pub(super) const STATUS_V2_0_0_RC1_FIXTURE: &str = include_str!("fixtures/status_v2_0_0_rc1.json");

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use nyanpasu_ipc::types::{ServiceStatus, StatusInfo};

    use super::{
        REQUIRED_SERVICE_MAJOR, STATUS_V1_4_5_FIXTURE, STATUS_V2_0_0_RC1_FIXTURE, ServiceCompat,
        parse_service_version,
    };

    fn parse_fixture(fixture: &'static str) -> StatusInfo<'static> {
        serde_json::from_str(fixture).expect("status fixture must decode")
    }

    #[test]
    fn v1_fixture_is_really_v1_shaped() {
        for key in ["controller", "health", "revision", "detail", "logs"] {
            assert!(
                !STATUS_V1_4_5_FIXTURE.contains(&format!("\"{key}\"")),
                "v1 fixture must not contain v2-only key {key}"
            );
        }
    }

    #[test]
    fn v1_payload_still_decodes_into_v2_struct() {
        let info = parse_fixture(STATUS_V1_4_5_FIXTURE);

        assert_eq!(
            info.server.as_ref().map(|server| server.version.as_ref()),
            Some("1.4.5")
        );
    }

    #[test]
    fn v1_daemon_is_incompatible() {
        let info = parse_fixture(STATUS_V1_4_5_FIXTURE);
        let compat = ServiceCompat::classify(&info);

        assert_eq!(
            compat,
            ServiceCompat::Incompatible {
                server_version: "1.4.5".to_owned(),
                required_major: REQUIRED_SERVICE_MAJOR,
            }
        );
        assert!(!compat.allows_service_backend());
    }

    #[test]
    fn v2_daemon_is_compatible() {
        let info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        let compat = ServiceCompat::classify(&info);

        assert_eq!(
            compat,
            ServiceCompat::Compatible {
                server_version: "2.0.0-rc.1".to_owned(),
            }
        );
        assert!(compat.allows_service_backend());
    }

    #[test]
    fn unparsable_version_is_fail_closed() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        info.server
            .as_mut()
            .expect("running fixture must include server info")
            .version = Cow::Borrowed("nightly");
        let compat = ServiceCompat::classify(&info);

        assert_eq!(
            compat,
            ServiceCompat::Unparsable {
                server_version: "nightly".to_owned(),
            }
        );
        assert!(!compat.allows_service_backend());
    }

    #[test]
    fn stopped_or_not_installed_is_unknown() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);

        info.status = ServiceStatus::Stopped;
        let stopped = ServiceCompat::classify(&info);
        assert_eq!(stopped, ServiceCompat::Unknown);
        assert!(!stopped.allows_service_backend());

        info.status = ServiceStatus::NotInstalled;
        let not_installed = ServiceCompat::classify(&info);
        assert_eq!(not_installed, ServiceCompat::Unknown);
        assert!(!not_installed.allows_service_backend());
    }

    #[test]
    fn rc_prerelease_still_outranks_v1() {
        let rc = parse_service_version("2.0.0-rc.1").expect("rc version must parse");
        let v1 = parse_service_version("1.4.5").expect("v1 version must parse");

        assert!(rc > v1);
    }
}
