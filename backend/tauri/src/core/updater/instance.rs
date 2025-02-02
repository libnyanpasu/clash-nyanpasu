use super::shared::{self, CoreTypeMeta};
use crate::{
    config::nyanpasu::ClashCore,
    core::CoreManager,
    utils::downloader::{DownloadStatus, Downloader, DownloaderBuilder, DownloaderState},
};
use anyhow::anyhow;
use runas::Command as RunasCommand;
use serde::Serialize;
use specta::Type;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use tempfile::TempDir;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Default, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdaterState {
    #[default]
    Idle,
    Downloading,
    Decompressing,
    Replacing,
    Restarting,
    Done,
    Failed(String),
}

type DownloaderWithDynCallback = Downloader<Box<dyn Fn(DownloaderState) + Send + Sync>>;

pub(super) struct Updater {
    id: usize,
    temp_dir: TempDir,
    core_type: ClashCore,
    artifact: String,
    inner: parking_lot::RwLock<UpdaterInner>,
    rx: Mutex<tokio::sync::mpsc::Receiver<DownloaderState>>,
    downloader: Arc<DownloaderWithDynCallback>,
}

struct UpdaterInner {
    state: UpdaterState,
}

#[derive(Debug, Serialize, Type)]
pub struct UpdaterSummary {
    pub id: usize,
    pub state: UpdaterState,
    pub downloader: DownloadStatus,
}

pub(super) struct UpdaterBuilder {
    client: Option<reqwest::Client>,
    core_type: Option<ClashCore>,
    mirror: Option<String>,
    artifact: Option<String>,
    tag: Option<CoreTypeMeta>,
}

impl UpdaterBuilder {
    pub fn new() -> Self {
        Self {
            client: None,
            core_type: None,
            mirror: None,
            artifact: None,
            tag: None,
        }
    }

    pub fn set_client(mut self, client: reqwest::Client) -> Self {
        self.client = Some(client);
        self
    }

    pub fn set_core_type(mut self, core_type: ClashCore) -> Self {
        self.core_type = Some(core_type);
        self
    }

    pub fn set_artifact(mut self, artifact: String) -> Self {
        self.artifact = Some(artifact);
        self
    }

    pub fn set_tag(mut self, tag: CoreTypeMeta) -> Self {
        self.tag = Some(tag);
        self
    }

    pub fn set_mirror(mut self, mirror: String) -> Self {
        self.mirror = Some(mirror);
        self
    }

    pub async fn build(self) -> anyhow::Result<Updater> {
        let client = self.client.ok_or(anyhow::anyhow!("client is required"))?;
        let core_type = self
            .core_type
            .ok_or(anyhow::anyhow!("core_type is required"))?;
        let artifact = self
            .artifact
            .ok_or(anyhow::anyhow!("artifact is required"))?;
        let tag = self.tag.ok_or(anyhow::anyhow!("tag is required"))?;
        let mirror = self.mirror.ok_or(anyhow::anyhow!("mirror is required"))?;

        let temp_dir = TempDir::new()?;
        let inner = UpdaterInner {
            state: UpdaterState::Idle,
        };

        // setup downloader
        let download_path = shared::get_download_path(tag, &artifact);
        let mut download_url = url::Url::parse("https://github.com")?;
        download_url.set_path(&download_path);
        let download_url = crate::utils::candy::parse_gh_url(&mirror, download_url.as_str())?;
        let file = tokio::fs::File::create(temp_dir.path().join(&artifact)).await?;
        tracing::debug!("downloader url: {}", download_url);
        tracing::debug!("downloader file: {:?}", file);
        let (tx, rx) = tokio::sync::mpsc::channel::<DownloaderState>(1);
        let callback: Box<dyn Fn(DownloaderState) + Send + Sync> = Box::new(move |state| {
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Err(e) = tx.send(state).await {
                    tracing::warn!("failed to send downloader state: {}", e);
                }
            });
        });
        let downloader = Arc::new(
            DownloaderBuilder::new()
                .set_client(client)
                .set_url(download_url)?
                .set_file(file)
                .set_event_callback(callback)
                .build()?,
        );
        Ok(Updater {
            id: rand::random::<u32>() as usize,
            temp_dir,
            core_type,
            inner: parking_lot::RwLock::new(inner),
            artifact,
            rx: Mutex::new(rx),
            downloader,
        })
    }
}

impl Updater {
    fn dispatch_state(&self, state: UpdaterState) {
        tracing::debug!("dispatching updater state: {:?}", state);
        let mut inner = self.inner.write();
        inner.state = state;
    }

    async fn decompress_and_set_permission(&self) -> anyhow::Result<()> {
        self.dispatch_state(UpdaterState::Decompressing);
        let path = self.temp_dir.path().join(&self.artifact);
        tracing::debug!("decompressing file: {:?}", path);
        let mut tmp_file = std::fs::File::open(path)?;
        tracing::debug!("file size: {}", tmp_file.metadata()?.len());
        let artifact = self.artifact.clone();
        let buff = tokio::task::spawn_blocking(move || {
            let mut buff = Vec::<u8>::new();
            match artifact {
                fname if fname.ends_with(".gz") => {
                    tracing::debug!("decompressing gz file");
                    let mut decoder = flate2::read::GzDecoder::new(&mut tmp_file);
                    std::io::copy(&mut decoder, &mut buff)?;
                }
                fname if fname.ends_with(".zip") => {
                    tracing::debug!("decompressing zip file");
                    let mut archive = zip::ZipArchive::new(tmp_file)?;
                    let len = archive.len();
                    for i in 0..len {
                        let mut file = archive.by_index(i)?;
                        let file_name = file.name();
                        tracing::debug!("Filename: {}", file.name());
                        // TODO: 在 enum 做点魔法
                        if file_name.contains("mihomo") || file_name.contains("clash") {
                            tracing::debug!("extract file: {}", file_name);
                            tracing::debug!("extract file size: {}", file.size());
                            std::io::copy(&mut file, &mut buff)?;
                            break;
                        }
                        if i == len - 1 {
                            anyhow::bail!("failed to find core file in a zip archive");
                        }
                    }
                }
                _ => {
                    tracing::debug!("directly copying file");
                    std::io::copy(&mut tmp_file, &mut buff)?;
                }
            };
            Ok::<_, anyhow::Error>(buff)
        })
        .await??;
        let tmp_core = self.temp_dir.path().join(format!(
            "{}{}",
            self.core_type,
            std::env::consts::EXE_SUFFIX
        ));
        tracing::debug!("writing core to {:?} ({} bytes)", tmp_core, buff.len());
        let mut core_file = tokio::fs::File::create(&tmp_core).await?;
        tokio::io::copy(&mut buff.as_slice(), &mut core_file).await?;
        #[cfg(target_family = "unix")]
        {
            std::fs::set_permissions(&tmp_core, std::fs::Permissions::from_mode(0o755))?;
        }
        Ok(())
    }

    async fn replace_core(&self) -> anyhow::Result<()> {
        self.dispatch_state(UpdaterState::Replacing);
        let current_core = crate::config::Config::verge()
            .latest()
            .clash_core
            .unwrap_or_default();
        tracing::debug!("current core: {}", current_core);
        if current_core == self.core_type {
            tracing::debug!("stopping core to replace");
            CoreManager::global().stop_core().await?;
        }
        #[cfg(target_os = "windows")]
        let target_core = format!("{}.exe", self.core_type);
        #[cfg(not(target_os = "windows"))]
        let target_core = self.core_type.clone().to_string();
        let core_dir = tauri::utils::platform::current_exe()?;
        let core_dir = core_dir.parent().ok_or(anyhow!("failed to get core dir"))?;
        let target_core = core_dir.join(target_core);
        tracing::debug!("copying core to {:?}", target_core);
        let tmp_core_path = self.temp_dir.path().join(format!(
            "{}{}",
            self.core_type,
            std::env::consts::EXE_SUFFIX
        ));
        match tokio::fs::copy(tmp_core_path.clone(), target_core.clone()).await {
            Ok(size) => {
                tracing::debug!("copied core to {:?} ({} bytes)", target_core, size);
            }
            Err(err) => {
                tracing::warn!(
                    "failed to copy core: {}, trying to use elevated permission to copy and override core",
                    err
                );
                let mut target_core_str = target_core.to_str().unwrap().to_string();
                if target_core_str.starts_with("\\\\?\\") {
                    target_core_str = target_core_str[4..].to_string();
                }
                tracing::debug!("tmp core path: {:?}", tmp_core_path);
                tracing::debug!("target core path: {:?}", target_core_str);
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

        if current_core == self.core_type {
            self.dispatch_state(UpdaterState::Restarting);
            CoreManager::global().run_core().await?;
        }

        Ok(())
    }

    pub async fn start(&self) {
        {
            let mut inner = self.inner.write();
            if !matches!(inner.state, UpdaterState::Idle) {
                return;
            }
            inner.state = UpdaterState::Downloading;
        }
        let downloader = self.downloader.clone();
        tokio::spawn(async move {
            if let Err(e) = downloader.start().await {
                tracing::error!("failed to start downloader: {}", e);
            }
        });
        let mut rx = self.rx.lock().await;
        loop {
            match rx.recv().await {
                Some(state) => match state {
                    DownloaderState::Downloading => {
                        tracing::debug!("start to download core.");
                        self.dispatch_state(UpdaterState::Downloading);
                    }
                    DownloaderState::Finished => {
                        tracing::debug!("download finished and start to incoming update logic");
                        if let Err(e) = self.decompress_and_set_permission().await {
                            tracing::error!("failed to decompress and set permission: {}", e);
                            self.dispatch_state(UpdaterState::Failed(e.to_string()));
                            return;
                        }
                        if let Err(e) = self.replace_core().await {
                            tracing::error!("failed to replace core: {}", e);
                            self.dispatch_state(UpdaterState::Failed(e.to_string()));
                            return;
                        }
                        self.dispatch_state(UpdaterState::Done);
                        break;
                    }
                    DownloaderState::Failed(e) => {
                        tracing::error!("download failed: {}", e);
                        self.dispatch_state(UpdaterState::Failed(e));
                        break;
                    }
                    _ => {
                        tracing::debug!("downloader enter state: {:?}", state);
                    }
                },
                None => {
                    tracing::error!("downloader channel closed");
                }
            }
        }
    }

    pub fn get_report(&self) -> UpdaterSummary {
        UpdaterSummary {
            id: self.id,
            state: self.inner.read().state.clone(),
            downloader: self.downloader.get_current_status(),
        }
    }

    pub fn get_updater_id(&self) -> usize {
        self.id
    }
}

unsafe impl Send for Updater {}
