use std::{collections::HashMap, io::Cursor, path::Path, sync::OnceLock};

use super::CoreManager;
use crate::config::nyanpasu::ClashCore;
use anyhow::{anyhow, Result};
use gunzip::Decompressor;
use log::{debug, warn};
use runas::Command as RunasCommand;
use serde::{Deserialize, Serialize};
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use tempfile::{tempdir, TempDir};
use tokio::{join, sync::RwLock, task::spawn_blocking};
use zip::ZipArchive;

pub struct Updater {
    manifest_version: ManifestVersion,
    mirror: String,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            manifest_version: ManifestVersion::default(),
            mirror: "https://mirror.ghproxy.com/github.com".to_string(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ManifestVersion {
    manifest_version: u64,
    latest: ManifestVersionLatest,
    arch_template: ArchTemplate,
    updated_at: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ManifestVersionLatest {
    mihomo: String,
    mihomo_alpha: String,
    clash_rs: String,
    clash_premium: String,
}

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
pub struct ArchTemplate {
    mihomo: HashMap<String, String>,
    mihomo_alpha: HashMap<String, String>,
    clash_rs: HashMap<String, String>,
    clash_premium: HashMap<String, String>,
}

impl Default for ManifestVersion {
    fn default() -> Self {
        Self {
            manifest_version: 0,
            latest: ManifestVersionLatest::default(),
            arch_template: ArchTemplate::default(),
            updated_at: "".to_string(),
        }
    }
}

impl Default for ManifestVersionLatest {
    fn default() -> Self {
        Self {
            mihomo: "".to_string(),
            mihomo_alpha: "".to_string(),
            clash_rs: "".to_string(),
            clash_premium: "".to_string(),
        }
    }
}

fn get_arch() -> Result<&'static str> {
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
        // ("aarch64", "windows") => Ok("windows-arm64"),
        _ => anyhow::bail!("unsupported platform"),
    }
}

impl Updater {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn global() -> &'static RwLock<Self> {
        static INSTANCE: OnceLock<RwLock<Updater>> = OnceLock::new();
        INSTANCE.get_or_init(|| RwLock::new(Updater::new()))
    }

    pub fn get_latest_versions(&self) -> ManifestVersionLatest {
        self.manifest_version.latest.clone()
    }

    pub async fn fetch_latest(&mut self) -> Result<()> {
        let latest = get_latest_version_manifest(self.mirror.as_str());
        let mihomo_alpha_version = self.get_mihomo_alpha_version();
        let (latest, mihomo_alpha_version) = join!(latest, mihomo_alpha_version);
        log::debug!("latest version: {:?}", latest);
        self.manifest_version = latest?;
        log::debug!("mihomo alpha version: {:?}", mihomo_alpha_version);
        self.manifest_version.latest.mihomo_alpha = mihomo_alpha_version?;
        Ok(())
    }

    async fn get_mihomo_alpha_version(&self) -> Result<String> {
        let client = crate::utils::candy::get_reqwest_client()?;
        let url = format!(
            "{}/{}",
            self.mirror.as_str(),
            "MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt"
        );
        let res = client.get(url).send().await?;
        let status_code = res.status();
        if !status_code.is_success() {
            anyhow::bail!(
                "failed to get mihomo alpha version: response status is {}, expected 200",
                status_code
            );
        }
        Ok(res.text().await?.trim().to_string())
    }

    pub async fn update_core(&self, core_type: &ClashCore) -> Result<()> {
        let current_core = crate::config::Config::verge()
            .latest()
            .clash_core
            .clone()
            .unwrap_or_default();
        let tmp_dir = tempdir()?;
        // 1. download core
        debug!("downloading core");
        let artifact = self.download_core(core_type, &tmp_dir).await?;
        // 2. decompress core
        debug!("decompressing core");
        let core_type_ref = core_type.clone();
        let tmp_dir_path = tmp_dir.path().to_owned();
        let artifact_ref = artifact.clone();
        spawn_blocking(move || {
            decompress_and_set_permission(&core_type_ref, &tmp_dir_path, &artifact_ref)
        })
        .await??;
        // 3. if core is used, close it
        if current_core == *core_type {
            tokio::task::spawn_blocking(move || CoreManager::global().stop_core()).await??;
        }
        // 4. replace core
        #[cfg(target_os = "windows")]
        let target_core = format!("{}.exe", core_type);
        #[cfg(not(target_os = "windows"))]
        let target_core = core_type.clone().to_string();
        let core_dir = tauri::utils::platform::current_exe()?;
        let core_dir = core_dir.parent().ok_or(anyhow!("failed to get core dir"))?;
        let target_core = core_dir.join(target_core);
        debug!("copying core to {:?}", target_core);
        let tmp_core_path = tmp_dir.path().join(core_type.clone().to_string());
        match std::fs::copy(tmp_core_path.clone(), target_core.clone()) {
            Ok(_) => {}
            Err(err) => {
                warn!(
                    "failed to copy core: {}, trying to use elevated permission to copy and override core",
                    err
                );
                let mut target_core_str = target_core.to_str().unwrap().to_string();
                if target_core_str.starts_with("\\\\?\\") {
                    target_core_str = target_core_str[4..].to_string();
                }
                debug!("tmp core path: {:?}", tmp_core_path);
                debug!("target core path: {:?}", target_core_str);
                // 防止 UAC 弹窗堵塞主线程
                let status_code = tokio::task::spawn_blocking(move || {
                    #[cfg(target_os = "windows")]
                    {
                        RunasCommand::new("cmd")
                            .args(&[
                                "/C",
                                "copy",
                                "/Y",
                                tmp_core_path.to_str().unwrap(),
                                &target_core_str,
                            ])
                            .status()
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        RunasCommand::new("cp")
                            .args(&["-f", tmp_core_path.to_str().unwrap(), &target_core_str])
                            .status()
                    }
                })
                .await??;
                if !status_code.success() {
                    anyhow::bail!("failed to copy core: {}", status_code);
                }
            }
        };

        // 5. if core is used before, restart it
        if current_core == *core_type {
            CoreManager::global().run_core().await?;
        }
        Ok(())
    }

    async fn download_core(&self, core_type: &ClashCore, tmp_dir: &TempDir) -> Result<String> {
        let arch = get_arch()?;
        debug!("download core: {} in arch {}", core_type, arch);
        let version_manifest = &self.manifest_version;
        let (artifact, core_type_meta) = match core_type {
            ClashCore::ClashPremium => (
                version_manifest
                    .arch_template
                    .clash_premium
                    .get(arch)
                    .ok_or(anyhow!("invalid arch"))?
                    .clone()
                    .replace("{}", &version_manifest.latest.clash_premium),
                CoreTypeMeta::ClashPremium(version_manifest.latest.clash_premium.clone()),
            ),
            ClashCore::Mihomo => (
                version_manifest
                    .arch_template
                    .mihomo
                    .get(arch)
                    .ok_or(anyhow!("invalid arch"))?
                    .clone()
                    .replace("{}", &version_manifest.latest.mihomo),
                CoreTypeMeta::Mihomo(version_manifest.latest.mihomo.clone()),
            ),
            ClashCore::MihomoAlpha => (
                version_manifest
                    .arch_template
                    .mihomo_alpha
                    .get(arch)
                    .ok_or(anyhow!("invalid arch"))?
                    .clone()
                    .replace("{}", &version_manifest.latest.mihomo_alpha),
                CoreTypeMeta::MihomoAlpha,
            ),
            ClashCore::ClashRs => (
                version_manifest
                    .arch_template
                    .clash_rs
                    .get(arch)
                    .ok_or(anyhow!("invalid arch"))?
                    .clone()
                    .replace("{}", &version_manifest.latest.clash_rs),
                CoreTypeMeta::ClashRs(version_manifest.latest.clash_rs.clone()),
            ),
        };
        debug!("artifact: {}", artifact);
        let url = format!(
            "{}/{}",
            &self.mirror,
            get_download_path(core_type_meta, artifact.clone())
        );
        debug!("url: {}", url);
        let file_path = tmp_dir.path().join(&artifact);
        debug!("file path: {:?}", file_path);
        let mut dst = std::fs::File::create(&file_path)?;

        let client = crate::utils::candy::get_reqwest_client()?;
        let res = client.get(url).send().await?;
        let status_code = res.status();
        if !status_code.is_success() {
            anyhow::bail!(
                "failed to download core: response status is {}, expected 200",
                status_code
            );
        }
        let mut buff = Cursor::new(res.bytes().await?);
        std::io::copy(&mut buff, &mut dst)?;
        Ok(artifact)
    }
}

fn decompress_and_set_permission(
    core_type: &ClashCore,
    tmp_path: &Path,
    fname: &str,
) -> Result<()> {
    let mut buff = Vec::<u8>::new();
    let path = tmp_path.join(fname);
    debug!("decompressing file: {:?}", path);
    let mut tmp_file = std::fs::File::open(path)?;
    debug!("file size: {}", tmp_file.metadata()?.len());
    match fname {
        fname if fname.ends_with(".gz") => {
            debug!("decompressing gz file");
            let mut decompressor = Decompressor::new(tmp_file, true);
            std::io::copy(&mut decompressor, &mut buff)?;
        }
        fname if fname.ends_with(".zip") => {
            debug!("decompressing zip file");
            let mut archive = ZipArchive::new(tmp_file)?;
            let len = archive.len();
            for i in 0..len {
                let mut file = archive.by_index(i)?;
                let file_name = file.name();
                debug!("Filename: {}", file.name());
                // TODO: 在 enum 做点魔法
                if file_name.contains("mihomo") || file_name.contains("clash") {
                    debug!("extract file: {}", file_name);
                    debug!("extract file size: {}", file.size());
                    std::io::copy(&mut file, &mut buff)?;
                    break;
                }
                if i == len - 1 {
                    anyhow::bail!("failed to find core file in a zip archive");
                }
            }
        }
        _ => {
            debug!("directly copying file");
            std::io::copy(&mut tmp_file, &mut buff)?;
        }
    };
    let tmp_core = tmp_path.join(core_type.clone().to_string());
    debug!("writing core to {:?} ({} bytes)", tmp_core, buff.len());
    let mut core_file = std::fs::File::create(&tmp_core)?;
    std::io::copy(&mut buff.as_slice(), &mut core_file)?;
    #[cfg(target_family = "unix")]
    {
        std::fs::set_permissions(&tmp_core, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

pub async fn get_latest_version_manifest(mirror: &str) -> Result<ManifestVersion> {
    let url = format!(
        "{}/LibNyanpasu/clash-nyanpasu/raw/dev/manifest/version.json",
        mirror
    );
    log::debug!("{}", url);
    let client = crate::utils::candy::get_reqwest_client()?;
    let res = client.get(url).send().await?;
    let status_code = res.status();
    if !status_code.is_success() {
        anyhow::bail!(
            "failed to get latest version manifest: response status is {}, expected 200",
            status_code
        );
    }
    Ok(res.json::<ManifestVersion>().await?)
}

enum CoreTypeMeta {
    ClashPremium(String),
    Mihomo(String),
    MihomoAlpha,
    ClashRs(String),
}

fn get_download_path(core_type: CoreTypeMeta, artifact: String) -> String {
    match core_type {
        CoreTypeMeta::Mihomo(tag) => {
            format!("MetaCubeX/mihomo/releases/download/{}/{}", tag, artifact)
        }
        CoreTypeMeta::MihomoAlpha => format!(
            "MetaCubeX/mihomo/releases/download/Prerelease-Alpha/{}",
            artifact
        ),
        CoreTypeMeta::ClashRs(tag) => {
            format!("Watfaq/clash-rs/releases/download/{}/{}", tag, artifact)
        }
        CoreTypeMeta::ClashPremium(tag) => format!(
            "zhongfly/Clash-premium-backup/releases/download/{}/{}",
            tag, artifact
        ),
    }
}
