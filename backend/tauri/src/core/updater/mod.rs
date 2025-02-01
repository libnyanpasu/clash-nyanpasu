use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

use crate::{
    config::nyanpasu::ClashCore,
    utils::candy::{parse_gh_url, ReqwestSpeedTestExt},
};
use anyhow::{anyhow, Result};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use shared::{get_arch, CoreTypeMeta};
use specta::Type;
use tokio::{join, sync::RwLock};

mod instance;
mod shared;

pub use instance::UpdaterSummary;

pub struct UpdaterManager {
    manifest_version: ManifestVersion,
    client: reqwest::Client,
    mirror: Arc<parking_lot::RwLock<Option<(String, u64)>>>,
    instances: Arc<DashMap<usize, Arc<instance::Updater>>>,
}

impl Default for UpdaterManager {
    fn default() -> Self {
        Self {
            manifest_version: ManifestVersion::default(),
            client: crate::utils::candy::get_reqwest_client().unwrap(),
            mirror: Arc::new(parking_lot::RwLock::new(None)),
            instances: Arc::new(DashMap::new()),
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

#[derive(Deserialize, Serialize, Clone, Debug, Type)]
pub struct ManifestVersionLatest {
    mihomo: String,
    mihomo_alpha: String,
    clash_rs: String,
    clash_rs_alpha: String,
    clash_premium: String,
}

#[derive(Deserialize, Serialize, Default, Clone, Debug)]
pub struct ArchTemplate {
    mihomo: HashMap<String, String>,
    mihomo_alpha: HashMap<String, String>,
    clash_rs: HashMap<String, String>,
    clash_rs_alpha: HashMap<String, String>,
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
            clash_rs_alpha: "".to_string(),
            clash_premium: "".to_string(),
        }
    }
}

impl ManifestVersion {
    pub(self) fn get_matches(&self, core_type: &ClashCore) -> Option<(String, CoreTypeMeta)> {
        let arch = get_arch().ok()?;
        match core_type {
            ClashCore::ClashPremium => Some((
                self.arch_template
                    .clash_premium
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.clash_premium),
                CoreTypeMeta::ClashPremium(self.latest.clash_premium.clone()),
            )),
            ClashCore::Mihomo => Some((
                self.arch_template
                    .mihomo
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.mihomo),
                CoreTypeMeta::Mihomo(self.latest.mihomo.clone()),
            )),
            ClashCore::MihomoAlpha => Some((
                self.arch_template
                    .mihomo_alpha
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.mihomo_alpha),
                CoreTypeMeta::MihomoAlpha,
            )),
            ClashCore::ClashRs => Some((
                self.arch_template
                    .clash_rs
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.clash_rs),
                CoreTypeMeta::ClashRs(self.latest.clash_rs.clone()),
            )),
            ClashCore::ClashRsAlpha => Some((
                self.arch_template
                    .clash_rs_alpha
                    .get(arch)?
                    .clone()
                    .replace("{}", &self.latest.clash_rs_alpha),
                CoreTypeMeta::ClashRsAlpha,
            )),
        }
    }
}

impl UpdaterManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn global() -> &'static RwLock<Self> {
        static INSTANCE: OnceLock<RwLock<UpdaterManager>> = OnceLock::new();
        INSTANCE.get_or_init(|| RwLock::new(UpdaterManager::new()))
    }

    pub fn get_latest_versions(&self) -> ManifestVersionLatest {
        self.manifest_version.latest.clone()
    }

    pub fn get_mirror(&self) -> Option<String> {
        self.mirror.read().clone().map(|(mirror, _)| mirror)
    }

    async fn get_latest_version_manifest(&self, mirror: &str) -> Result<ManifestVersion> {
        let url = parse_gh_url(
            mirror,
            "/libnyanpasu/clash-nyanpasu/raw/main/manifest/version.json",
        )?;
        log::debug!("{}", url);
        let res = self.client.get(url).send().await?;
        let status_code = res.status();
        if !status_code.is_success() {
            anyhow::bail!(
                "failed to get latest version manifest: response status is {}, expected 200",
                status_code
            );
        }
        Ok(res.json::<ManifestVersion>().await?)
    }

    pub async fn fetch_latest(&mut self) -> Result<()> {
        self.mirror_speed_test().await?;
        let mirror = self.get_mirror().unwrap();
        let latest = self.get_latest_version_manifest(&mirror);
        let mihomo_alpha_version = self.get_mihomo_alpha_version();
        let clash_rs_alpha_version = self.get_clash_rs_alpha_version();
        let (latest, mihomo_alpha_version, clash_rs_alpha_version) =
            join!(latest, mihomo_alpha_version, clash_rs_alpha_version);
        log::debug!("latest version: {:?}", latest);
        self.manifest_version = latest?;
        log::debug!("mihomo alpha version: {:?}", mihomo_alpha_version);
        self.manifest_version.latest.mihomo_alpha = mihomo_alpha_version?;
        log::debug!("clash rs alpha version: {:?}", clash_rs_alpha_version);
        self.manifest_version.latest.clash_rs_alpha = clash_rs_alpha_version?;
        Ok(())
    }

    // TODO: add user-spec mirror support
    pub async fn mirror_speed_test(&self) -> Result<()> {
        {
            let mirror = self.mirror.read();
            if let Some((_, timestamp)) = mirror.as_ref() {
                if chrono::Utc::now().timestamp() - (*timestamp as i64) < 3600 {
                    return Ok(());
                }
            }
        }
        let mirrors = crate::utils::candy::INTERNAL_MIRRORS;
        let path = "https://github.com/libnyanpasu/clash-nyanpasu/raw/main/manifest/version.json";
        let client = crate::utils::candy::get_reqwest_client()?;
        let results = client.mirror_speed_test(mirrors, path).await?;
        let (fastest_mirror, speed) = results.first().ok_or(anyhow!("no mirrors found"))?;
        if speed - 1.0 < 0.0001 {
            anyhow::bail!("all mirrors are too slow");
        }
        tracing::debug!("fastest mirror: {}, speed: {}", fastest_mirror, speed);
        {
            let mut mirror = self.mirror.write();
            *mirror = Some((
                fastest_mirror.to_string(),
                chrono::Utc::now().timestamp() as u64,
            ));
        }
        Ok(())
    }

    async fn get_mihomo_alpha_version(&self) -> Result<String> {
        self.mirror_speed_test().await?;
        let mirror = self.get_mirror().unwrap();
        let url = crate::utils::candy::parse_gh_url(
            &mirror,
            "/MetaCubeX/mihomo/releases/download/Prerelease-Alpha/version.txt",
        )?;
        let res = self.client.get(url).send().await?;
        let status_code = res.status();
        if !status_code.is_success() {
            anyhow::bail!(
                "failed to get mihomo alpha version: response status is {}, expected 200",
                status_code
            );
        }
        Ok(res.text().await?.trim().to_string())
    }

    async fn get_clash_rs_alpha_version(&self) -> Result<String> {
        self.mirror_speed_test().await?;
        let mirror = self.get_mirror().unwrap();
        let url = crate::utils::candy::parse_gh_url(
            &mirror,
            "/Watfaq/clash-rs/releases/download/latest/version.txt",
        )?;
        let res = self.client.get(url).send().await?;
        let status_code = res.status();
        if !status_code.is_success() {
            anyhow::bail!(
                "failed to get clash rs alpha version: response status is {}, expected 200",
                status_code
            );
        }
        let res = res.text().await?;
        let version = res
            .trim()
            .split(' ')
            .next_back()
            .ok_or(anyhow!("no version found"))?;
        Ok(version.to_string())
    }

    pub async fn update_core(&mut self, core_type: &ClashCore) -> Result<usize> {
        self.mirror_speed_test().await?;
        let (artifact, tag) = self
            .manifest_version
            .get_matches(core_type)
            .ok_or(anyhow!("no matches found for core type: {:?}", core_type))?;
        let mirror = self.get_mirror().unwrap();
        let updater = Arc::new(
            instance::UpdaterBuilder::new()
                .set_client(self.client.clone())
                .set_core_type(*core_type)
                .set_mirror(mirror)
                .set_artifact(artifact)
                .set_tag(tag)
                .build()
                .await?,
        );
        let updater_ref = updater.clone();
        let updater_id = updater.get_updater_id();
        self.instances.insert(updater_id, updater);
        tokio::spawn(async move {
            updater_ref.start().await;
        });
        Ok(updater_id)
    }

    pub fn inspect_updater(&self, updater_id: usize) -> Option<UpdaterSummary> {
        let updater = self.instances.get(&updater_id)?;
        let report = updater.get_report();
        if matches!(
            report.state,
            instance::UpdaterState::Done | instance::UpdaterState::Failed(_)
        ) {
            let map = self.instances.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                map.remove(&updater_id);
            });
        }
        Some(report)
    }
}
