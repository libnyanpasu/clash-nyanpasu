use async_trait::async_trait;
use nyanpasu_config::application::ClashCore;
use serde::Serialize;
use snafu::Snafu;

#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub(crate)))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreVersionError {
    #[snafu(visibility(pub))]
    #[snafu(display("could not run the {core} core to read its version: {source}"))]
    RunCoreVersion {
        #[specta(type = String)]
        core: ClashCore,
        #[serde(skip)]
        source: anyhow::Error,
    },
    #[snafu(visibility(pub))]
    #[snafu(display("the {core} core failed when asked for its version"))]
    CoreVersionExit {
        #[specta(type = String)]
        core: ClashCore,
    },
    #[snafu(display("the {core} core did not report a version"))]
    CoreVersionNotReported {
        #[specta(type = String)]
        core: ClashCore,
    },
}

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CoreVersionReader: Send + Sync + 'static {
    async fn read(&self, core: ClashCore) -> Result<String, CoreVersionError>;
}

pub fn parse_version(core: ClashCore, banner: &str) -> Result<String, CoreVersionError> {
    for token in banner.split_whitespace() {
        let version = token.strip_prefix('v').unwrap_or(token);
        let release = semver::Version::parse(version).is_ok();
        let alpha = token.strip_prefix("alpha-").is_some_and(|hash| {
            !hash.is_empty() && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
        let premium = matches!(core, ClashCore::ClashPremium)
            && token.strip_prefix('n').is_some_and(|value| {
                value.starts_with(|character: char| character.is_ascii_digit())
            });
        if release || alpha || premium {
            return Ok(token.to_owned());
        }
    }
    CoreVersionNotReportedSnafu { core }.fail()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_versions_without_mistaking_banner_words_for_versions() {
        for (core, banner, expected) in [
            (ClashCore::Meow, "meow version 0.22.0\n", "0.22.0"),
            (
                ClashCore::Meow,
                "meow 0.22.0-alpha+3c27aca",
                "0.22.0-alpha+3c27aca",
            ),
            (
                ClashCore::ClashRsAlpha,
                "clash-rs 0.10.8-alpha+sha.0cf7ed5",
                "0.10.8-alpha+sha.0cf7ed5",
            ),
            (
                ClashCore::Mihomo,
                "Mihomo Meta v1.19.32 darwin arm64",
                "v1.19.32",
            ),
            (
                ClashCore::MihomoAlpha,
                "Mihomo Meta alpha-9f053c4",
                "alpha-9f053c4",
            ),
            (
                ClashCore::ClashPremium,
                "Clash n2023-09-05-gdcc8d87",
                "n2023-09-05-gdcc8d87",
            ),
        ] {
            assert_eq!(parse_version(core, banner).unwrap(), expected);
        }
        assert!(parse_version(ClashCore::Meow, "version not available").is_err());
        assert!(parse_version(ClashCore::Meow, "").is_err());
    }
}
