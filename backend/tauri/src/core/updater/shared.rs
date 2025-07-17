pub(super) fn get_arch() -> anyhow::Result<&'static str> {
    let env = {
        let arch = std::env::consts::ARCH;
        let os = std::env::consts::OS;
        #[cfg(all(target_arch = "arm", target_abi = "eabihf"))]
        let arch = "armhf";
        #[cfg(all(target_arch = "arm", target_abi = ""))]
        let arch = "armel";
        (arch, os)
    };

    match env {
        ("x86_64", "macos") => Ok("darwin-x64"),
        ("x86_64", "linux") => Ok("linux-amd64"),
        ("x86_64", "windows") => Ok("windows-x86_64"),
        ("i686", "windows") => Ok("windows-i386"),
        ("i686", "linux") => Ok("linux-i386"),
        ("armhf", "linux") => Ok("linux-armv7hf"),
        ("armel", "linux") => Ok("linux-armv7"),
        ("aarch64", "macos") => Ok("darwin-arm64"),
        ("aarch64", "linux") => Ok("linux-aarch64"),
        ("aarch64", "windows") => Ok("windows-arm64"),
        _ => anyhow::bail!("unsupported platform"),
    }
}

pub(super) enum CoreTypeMeta {
    ClashPremium(String),
    Mihomo(String),
    MihomoAlpha,
    ClashRs(String),
    ClashRsAlpha,
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
            format!("Watfaq/clash-rs/releases/download/{tag}/{artifact}")
        }
        CoreTypeMeta::ClashRsAlpha => {
            format!("Watfaq/clash-rs/releases/download/latest/{artifact}")
        }
        CoreTypeMeta::ClashPremium(tag) => {
            format!("zhongfly/Clash-premium-backup/releases/download/{tag}/{artifact}")
        }
    }
}
