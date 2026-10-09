pub(super) fn get_arch() -> anyhow::Result<&'static str> {
    let env = {
        let arch = std::env::consts::ARCH;
        let os = std::env::consts::OS;
        (arch, os)
    };

    match env {
        ("x86_64", "macos") => Ok("darwin-x64"),
        ("x86_64", "linux") => Ok("linux-amd64"),
        ("x86_64", "windows") => Ok("windows-x86_64"),
        ("aarch64", "macos") => Ok("darwin-arm64"),
        ("aarch64", "linux") => Ok("linux-aarch64"),
        ("aarch64", "windows") => Ok("windows-arm64"),
        _ => anyhow::bail!("unsupported platform"),
    }
}

pub enum CoreTypeMeta {
    ClashPremium(String),
    Mihomo(String),
    MihomoAlpha,
    ClashRs(String),
    ClashRsAlpha,
    Meow(String),
    MeowAlpha(String),
}

pub(super) fn get_download_path(core_type: CoreTypeMeta, artifact: &str) -> String {
    match core_type {
        CoreTypeMeta::Mihomo(tag) => {
            format!("MetaCubeX/mihomo/releases/download/{tag}/{artifact}")
        }
        CoreTypeMeta::MihomoAlpha => {
            format!("MetaCubeX/mihomo/releases/download/Prerelease-Alpha/{artifact}")
        }
        CoreTypeMeta::ClashRs(tag) => {
            format!("ibigbug/clash-rs/releases/download/{tag}/{artifact}")
        }
        CoreTypeMeta::ClashRsAlpha => {
            format!("ibigbug/clash-rs/releases/download/latest/{artifact}")
        }
        CoreTypeMeta::ClashPremium(tag) => {
            format!("zhongfly/Clash-premium-backup/releases/download/{tag}/{artifact}")
        }
        CoreTypeMeta::Meow(tag) => {
            format!("meow-rs/meow-rs/releases/download/{tag}/{artifact}")
        }
        CoreTypeMeta::MeowAlpha(_) => {
            format!("meow-rs/meow-rs/releases/download/Prerelease-Alpha/{artifact}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meow_alpha_uses_the_fixed_prerelease_tag() {
        assert_eq!(
            get_download_path(
                CoreTypeMeta::MeowAlpha("alpha-3c27aca".into()),
                "meow-alpha-3c27aca-aarch64-apple-darwin.tar.gz",
            ),
            "meow-rs/meow-rs/releases/download/Prerelease-Alpha/meow-alpha-3c27aca-aarch64-apple-darwin.tar.gz"
        );
    }
}
