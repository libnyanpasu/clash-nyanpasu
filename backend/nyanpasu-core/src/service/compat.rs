//! PR-5-pre: daemon 版本兼容门。
//!
//! v2 的 `StatusResBody` 把所有新增字段都标成了
//! `#[serde(default, skip_serializing_if = "Option::is_none")]`，所以 v1.4.5 daemon 的
//! `/status` 负载是 v2 结构的严格子集，**一定**能静默解码成功。因此本门禁必须做
//! 显式 semver 比较，绝不能依赖解码失败。
//!
//! 判据是主版本加最低版本两条：主版本单独不够，同一主版本内的旧 rc 同样缺
//! 换线后唯一在用的 `/v2/core/*` 路由。

use nyanpasu_ipc::types::{ServiceStatus, StatusInfo};
use nyanpasu_runtime_values::core_status::ServiceCompat;

/// PR-5-pre 起，只有主版本等于此值的 daemon 允许承载核心生命周期。
pub const REQUIRED_SERVICE_MAJOR: u64 = 2;

/// Linux transparent proxy and desktop-owned IPC authorization ship in `2.0.3`.
/// Earlier services cannot provide those routes or authorize the current desktop session.
pub const REQUIRED_SERVICE_MIN: &str = "2.0.3";

/// 纯函数：无 I/O、无全局状态、无 `Config::*()`。
pub fn classify(info: &StatusInfo<'_>) -> ServiceCompat {
    if info.status != ServiceStatus::Running {
        return ServiceCompat::Unknown;
    }

    let Some(server) = info.server.as_ref() else {
        return ServiceCompat::Unknown;
    };
    let server_version = server.version.to_string();
    let Some(version) = parse_service_version(&server_version) else {
        return ServiceCompat::Unparsable { server_version };
    };

    // 两条判据都要：最低版本挡住同主版本的旧 rc，主版本挡住比它更高的
    // 大版本（`3.0.0` 满足最低版本，却不该被这一代承载）。
    if version.major != REQUIRED_SERVICE_MAJOR || version < required_service_min() {
        return ServiceCompat::Incompatible {
            server_version,
            required_major: REQUIRED_SERVICE_MAJOR,
            required_min: REQUIRED_SERVICE_MIN.to_owned(),
        };
    }

    ServiceCompat::Compatible { server_version }
}

/// 唯一的放行判据。
pub fn allows_service_backend(compat: &ServiceCompat) -> bool {
    matches!(compat, ServiceCompat::Compatible { .. })
}

/// 供 `ServiceCompat` 与启动时自动升级检查复用的唯一版本解析入口。
pub fn parse_service_version(raw: &str) -> Option<semver::Version> {
    semver::Version::parse(raw).ok()
}

/// [`REQUIRED_SERVICE_MIN`] 的解析结果。常量是编译期写死的字面量，解析不到
/// 说明常量本身写错了，属于构建时就该发现的错误而非运行时输入问题。
fn required_service_min() -> semver::Version {
    semver::Version::parse(REQUIRED_SERVICE_MIN)
        .expect("REQUIRED_SERVICE_MIN must be a valid semver version")
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
        REQUIRED_SERVICE_MAJOR, REQUIRED_SERVICE_MIN, STATUS_V1_4_5_FIXTURE,
        STATUS_V2_0_0_RC1_FIXTURE, allows_service_backend, classify, parse_service_version,
        required_service_min,
    };
    use nyanpasu_runtime_values::core_status::ServiceCompat;

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
        let compat = classify(&info);

        assert_eq!(
            compat,
            ServiceCompat::Incompatible {
                server_version: "1.4.5".to_owned(),
                required_major: REQUIRED_SERVICE_MAJOR,
                required_min: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
        assert!(!allows_service_backend(&compat));
    }

    /// 本卡的判据本身：主版本对不再等于放行。换线后 `/v2/core/*` 是唯一路径，
    /// 而 rc.1 没有那些路由，所以它必须和 v1 一样被挡在门外。
    #[test]
    fn an_rc_daemon_with_the_right_major_is_still_incompatible() {
        let info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        let compat = classify(&info);

        assert_eq!(
            compat,
            ServiceCompat::Incompatible {
                server_version: "2.0.0-rc.1".to_owned(),
                required_major: REQUIRED_SERVICE_MAJOR,
                required_min: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
        assert!(!allows_service_backend(&compat));
    }

    #[test]
    fn rc2_without_api_connection_is_incompatible() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        info.server.as_mut().unwrap().version = Cow::Borrowed("2.0.0-rc.2");

        assert_eq!(
            classify(&info),
            ServiceCompat::Incompatible {
                server_version: "2.0.0-rc.2".to_owned(),
                required_major: REQUIRED_SERVICE_MAJOR,
                required_min: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
    }

    #[test]
    fn rc3_without_control_channel_settings_is_incompatible() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        info.server.as_mut().unwrap().version = Cow::Borrowed("2.0.0-rc.3");
        assert!(!allows_service_backend(&classify(&info)));
    }

    #[test]
    fn rc4_before_control_channel_release_is_incompatible() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        info.server.as_mut().unwrap().version = Cow::Borrowed("2.0.0-rc.4");
        assert!(!allows_service_backend(&classify(&info)));
    }

    #[test]
    fn daemons_without_windows_pipe_authorization_are_incompatible() {
        for version in ["2.0.0-rc.5", "2.0.0-rc.6", "2.0.0-rc.7"] {
            let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
            info.server.as_mut().unwrap().version = Cow::Borrowed(version);
            assert!(
                !allows_service_backend(&classify(&info)),
                "{version} must require an upgrade"
            );
        }
    }

    #[test]
    fn daemons_without_managed_native_storage_are_incompatible() {
        for version in ["2.0.0-rc.8", "2.0.0-rc.9"] {
            let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
            info.server.as_mut().unwrap().version = Cow::Borrowed(version);
            assert!(
                !allows_service_backend(&classify(&info)),
                "{version} must require an upgrade"
            );
        }
    }

    #[test]
    fn a_daemon_at_the_minimum_is_compatible() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        info.server
            .as_mut()
            .expect("running fixture must include server info")
            .version = Cow::Borrowed(REQUIRED_SERVICE_MIN);
        let compat = classify(&info);

        assert_eq!(
            compat,
            ServiceCompat::Compatible {
                server_version: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
        assert!(allows_service_backend(&compat));
    }

    #[test]
    fn a_daemon_without_the_reused_pid_sweep_is_incompatible() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        info.server
            .as_mut()
            .expect("running fixture must include server info")
            .version = Cow::Borrowed("2.0.1");
        let compat = classify(&info);

        assert_eq!(
            compat,
            ServiceCompat::Incompatible {
                server_version: "2.0.1".to_owned(),
                required_major: REQUIRED_SERVICE_MAJOR,
                required_min: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
        assert!(!allows_service_backend(&compat));
    }

    #[test]
    fn previous_release_without_transparent_proxy_is_incompatible() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        info.server
            .as_mut()
            .expect("running fixture must include server info")
            .version = Cow::Borrowed("2.0.2");
        let compat = classify(&info);

        assert_eq!(
            compat,
            ServiceCompat::Incompatible {
                server_version: "2.0.2".to_owned(),
                required_major: REQUIRED_SERVICE_MAJOR,
                required_min: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
        assert!(!allows_service_backend(&compat));
    }

    /// 最低版本是编译期字面量，写错了要在测试里就炸，而不是等运行时 `expect`。
    #[test]
    fn the_minimum_is_valid_semver_inside_the_required_major() {
        let min = parse_service_version(REQUIRED_SERVICE_MIN)
            .expect("REQUIRED_SERVICE_MIN must parse as semver");

        assert_eq!(min.major, REQUIRED_SERVICE_MAJOR);
    }

    #[test]
    fn unparsable_version_is_fail_closed() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        info.server
            .as_mut()
            .expect("running fixture must include server info")
            .version = Cow::Borrowed("nightly");
        let compat = classify(&info);

        assert_eq!(
            compat,
            ServiceCompat::Unparsable {
                server_version: "nightly".to_owned(),
            }
        );
        assert!(!allows_service_backend(&compat));
    }

    #[test]
    fn stopped_or_not_installed_is_unknown() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);

        info.status = ServiceStatus::Stopped;
        let stopped = classify(&info);
        assert_eq!(stopped, ServiceCompat::Unknown);
        assert!(!allows_service_backend(&stopped));

        info.status = ServiceStatus::NotInstalled;
        let not_installed = classify(&info);
        assert_eq!(not_installed, ServiceCompat::Unknown);
        assert!(!allows_service_backend(&not_installed));
    }

    #[test]
    fn rc_prerelease_still_outranks_v1() {
        let rc = parse_service_version("2.0.0-rc.1").expect("rc version must parse");
        let v1 = parse_service_version("1.4.5").expect("v1 version must parse");

        assert!(rc > v1);
    }

    /// Reads the `version` key out of the `[package]` table of a Cargo manifest
    /// without pulling in a `toml` dependency, mirroring the line-scan approach
    /// `scripts/src/prepare/check.ts` uses to read the same manifest.
    fn parse_package_version(manifest: &str) -> semver::Version {
        let raw = manifest
            .lines()
            .skip_while(|line| line.trim() != "[package]")
            .skip(1)
            .take_while(|line| !line.trim_start().starts_with('['))
            .find_map(|line| {
                let rest = line.trim().strip_prefix("version")?;
                let rest = rest.trim_start().strip_prefix('=')?.trim();
                let rest = rest.strip_prefix('"')?;
                let end = rest.find('"')?;
                Some(rest[..end].to_owned())
            })
            .expect("[package] table must declare a version key");

        semver::Version::parse(&raw).expect("bundled nyanpasu-service version must be valid semver")
    }

    /// This is the tie the review finding asked for: `REQUIRED_SERVICE_MIN` is a
    /// hand-maintained constant, and the bundled daemon binary comes from
    /// whatever version `backend/nyanpasu-runtime/nyanpasu_service/Cargo.toml`
    /// currently declares. If the constant is ever bumped ahead of that
    /// manifest, every checkout would download and ship a daemon that this
    /// very gate then rejects at runtime. Deleting this assertion (or reading
    /// a different file) is the only way to make this test blind to that
    /// regression.
    #[test]
    fn required_min_never_exceeds_the_bundled_daemon_version() {
        const MANIFEST: &str =
            include_str!("../../../nyanpasu-runtime/nyanpasu_service/Cargo.toml");

        let bundled = parse_package_version(MANIFEST);

        assert!(
            required_service_min() <= bundled,
            "REQUIRED_SERVICE_MIN ({REQUIRED_SERVICE_MIN}) exceeds the bundled \
             nyanpasu-service crate version ({bundled}); the app would reject its \
             own daemon"
        );

        // The gate rejects a mismatched major in either direction, so a bundled
        // daemon with a higher major would also be refused.
        assert_eq!(bundled.major, REQUIRED_SERVICE_MAJOR);
    }
}
