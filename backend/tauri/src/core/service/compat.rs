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

/// PR-5-pre 起，只有主版本等于此值的 daemon 允许承载核心生命周期。
pub const REQUIRED_SERVICE_MAJOR: u64 = 2;

/// 换线后 `/v2/core/*` 是唯一路径，而这些路由是在 `2.0.0-rc.2` 才齐的。只比
/// 主版本的话，一台残留的 `2.0.0-rc.1` daemon（major == 2）会被判为兼容却没有
/// 该路由——门禁就不再 fail-closed 了。
///
/// 存成字符串而非 `semver::Version` 常量：`Prerelease::new` 不是 `const fn`，
/// 能进 const 上下文的只有不带预发布标识的版本，而那样只能写成 `2.0.0`——按
/// semver 预发布序 `2.0.0-rc.2 < 2.0.0`，反而会把本轮发布的 rc 系列全拒掉。
pub const REQUIRED_SERVICE_MIN: &str = "2.0.0-rc.2";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ServiceCompat {
    /// daemon 未安装 / 未运行 / 未上报 server 信息，没有可判定的版本。
    Unknown,
    /// 主版本匹配，允许进入 Service backend。
    Compatible { server_version: String },
    /// 主版本不匹配（典型：v1.4.5），或主版本对但低于最低版本（典型：
    /// v2.0.0-rc.1）。fail-closed。
    Incompatible {
        server_version: String,
        required_major: u64,
        /// 供 UI 展示：只说"需要 v2.x"无法解释一台 major 正确却被拒的
        /// daemon，它装的**就是** v2.x。
        required_min: String,
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

        // 两条判据都要：最低版本挡住同主版本的旧 rc，主版本挡住比它更高的
        // 大版本（`3.0.0` 满足最低版本，却不该被这一代承载）。
        if version.major != REQUIRED_SERVICE_MAJOR || version < required_service_min() {
            return Self::Incompatible {
                server_version,
                required_major: REQUIRED_SERVICE_MAJOR,
                required_min: REQUIRED_SERVICE_MIN.to_owned(),
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
        STATUS_V2_0_0_RC1_FIXTURE, ServiceCompat, parse_service_version, required_service_min,
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
                required_min: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
        assert!(!compat.allows_service_backend());
    }

    /// 本卡的判据本身：主版本对不再等于放行。换线后 `/v2/core/*` 是唯一路径，
    /// 而 rc.1 没有那些路由，所以它必须和 v1 一样被挡在门外。
    #[test]
    fn an_rc_daemon_with_the_right_major_is_still_incompatible() {
        let info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        let compat = ServiceCompat::classify(&info);

        assert_eq!(
            compat,
            ServiceCompat::Incompatible {
                server_version: "2.0.0-rc.1".to_owned(),
                required_major: REQUIRED_SERVICE_MAJOR,
                required_min: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
        assert!(!compat.allows_service_backend());
    }

    #[test]
    fn a_daemon_at_the_minimum_is_compatible() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        info.server
            .as_mut()
            .expect("running fixture must include server info")
            .version = Cow::Borrowed(REQUIRED_SERVICE_MIN);
        let compat = ServiceCompat::classify(&info);

        assert_eq!(
            compat,
            ServiceCompat::Compatible {
                server_version: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
        assert!(compat.allows_service_backend());
    }

    /// 最低版本不能连"正式版"一起挡掉：按 semver 预发布序 `2.0.0 > 2.0.0-rc.2`。
    #[test]
    fn the_stable_release_outranks_the_minimum() {
        let mut info = parse_fixture(STATUS_V2_0_0_RC1_FIXTURE);
        info.server
            .as_mut()
            .expect("running fixture must include server info")
            .version = Cow::Borrowed("2.0.0");
        let compat = ServiceCompat::classify(&info);

        assert!(compat.allows_service_backend());
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

    /// Reads the `version` key out of the `[package]` table of a Cargo manifest
    /// without pulling in a `toml` dependency, mirroring the line-scan approach
    /// `scripts/check.ts` uses to read the same manifest.
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
            include_str!("../../../../nyanpasu-runtime/nyanpasu_service/Cargo.toml");

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
