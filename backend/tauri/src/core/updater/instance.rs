use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde::Serialize;
use specta::Type;
#[cfg(target_family = "unix")]
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

use super::{
    ManifestVersion,
    ports::{UpdaterBackend, UpdaterProgress},
    shared::{self, CoreTypeMeta},
};
use crate::{
    client::core_lifecycle::ports::PreparedCoreBinary,
    config::nyanpasu::ClashCore,
    core::download::{DownloadSession, DownloadStatus},
    utils::candy::{ReqwestSpeedTestExt, parse_gh_url},
};

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
    Pending(String),
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct UpdaterSummary {
    pub id: usize,
    pub state: UpdaterState,
    pub downloader: DownloadStatus,
}

pub(crate) struct HttpUpdaterBackend {
    proxy_port: Arc<dyn crate::service::profile_file::SelfProxyPortSource>,
    destination_dir: PathBuf,
}

impl HttpUpdaterBackend {
    pub fn new(
        destination_dir: PathBuf,
        proxy_port: Arc<dyn crate::service::profile_file::SelfProxyPortSource>,
    ) -> Self {
        Self {
            proxy_port,
            destination_dir,
        }
    }

    fn http_client(&self) -> anyhow::Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .user_agent(concat!("clash-nyanpasu/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(120));
        if let Some(port) = self.proxy_port.mixed_port() {
            builder = builder.proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))?);
        }
        Ok(builder.build()?)
    }
}

// DownloadSession drives background IO internally; cancelling its owner must also
// cancel those tasks before the worker releases its staging directory.
struct OwnedDownload(DownloadSession);

impl Drop for OwnedDownload {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[async_trait]
impl UpdaterBackend for HttpUpdaterBackend {
    async fn fetch_manifest(
        &self,
        mirror: Option<(String, Instant)>,
    ) -> anyhow::Result<(ManifestVersion, (String, Instant))> {
        let client = self.http_client()?;
        let mirror = match mirror {
            Some(cached) if cached.1.elapsed() < Duration::from_secs(3600) => cached,
            _ => {
                let results = client.mirror_speed_test(
                    crate::utils::candy::INTERNAL_MIRRORS,
                    "https://github.com/libnyanpasu/clash-nyanpasu/raw/main/manifest/version.json",
                ).await?;
                let (mirror, speed) = results
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("no mirrors found"))?;
                if speed - 1.0 < 0.0001 {
                    anyhow::bail!("all mirrors are too slow");
                }
                (mirror.to_string(), Instant::now())
            }
        };
        let url = parse_gh_url(
            &mirror.0,
            "/libnyanpasu/clash-nyanpasu/raw/main/manifest/version.json",
        )?;
        let manifest = client
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok((manifest, mirror))
    }

    async fn prepare(
        &self,
        core_type: ClashCore,
        mirror: String,
        artifact: String,
        tag: CoreTypeMeta,
        progress: UpdaterProgress,
    ) -> anyhow::Result<PreparedCoreBinary> {
        let staging = Arc::new(TempDir::new()?);
        let mut url = url::Url::parse("https://github.com")?;
        url.set_path(&shared::get_download_path(tag, &artifact));
        let url = parse_gh_url(&mirror, url.as_str())?;
        let downloader = OwnedDownload(
            DownloadSession::new(self.http_client()?, url, staging.path().join(&artifact)).await?,
        );
        let download = downloader.0.start();
        tokio::pin!(download);
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        loop {
            tokio::select! {
                result = &mut download => {
                    progress.report(UpdaterState::Downloading, Some(downloader.0.status()));
                    result?;
                    break;
                }
                _ = interval.tick() => {
                    progress.report(UpdaterState::Downloading, Some(downloader.0.status()));
                }
            }
        }
        progress.report(UpdaterState::Decompressing, None);
        let filename = format!("{}{}", core_type, std::env::consts::EXE_SUFFIX);
        let prepared_dir = staging.path().join("prepared");
        tokio::fs::create_dir(&prepared_dir).await?;
        let source = prepared_dir.join(&filename);
        let extraction_staging = staging.clone();
        let extraction_source = source.clone();
        // A blocking extraction cannot be aborted midway. It owns staging until
        // completion, including when its async worker is cancelled during shutdown.
        tokio::task::spawn_blocking(move || {
            extract_core(
                extraction_staging.path().join(&artifact),
                &artifact,
                extraction_source,
            )
        })
        .await??;
        progress.report(UpdaterState::Replacing, None);
        Ok(PreparedCoreBinary {
            target: core_type,
            source,
            destination: self.destination_dir.join(filename),
            staging,
            progress: Arc::new(progress),
        })
    }
}

fn extract_core(archive_path: PathBuf, artifact: &str, destination: PathBuf) -> anyhow::Result<()> {
    let mut source = std::fs::File::open(archive_path)?;
    let mut output = std::fs::File::create(&destination)?;
    if artifact.ends_with(".gz") {
        std::io::copy(&mut flate2::read::GzDecoder::new(source), &mut output)?;
    } else if artifact.ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(source)?;
        let mut found = false;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            if !file.is_dir()
                && ["mihomo", "clash", "meow"]
                    .iter()
                    .any(|name| file.name().contains(name))
            {
                std::io::copy(&mut file, &mut output)?;
                found = true;
                break;
            }
        }
        anyhow::ensure!(found, "failed to find core file in a zip archive");
    } else {
        std::io::copy(&mut source, &mut output)?;
    }
    #[cfg(target_family = "unix")]
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct ProxyPort(std::sync::atomic::AtomicU16);

    impl crate::service::profile_file::SelfProxyPortSource for ProxyPort {
        fn mixed_port(&self) -> Option<u16> {
            Some(self.0.load(std::sync::atomic::Ordering::SeqCst))
        }
    }

    async fn proxy_reply(listener: &tokio::net::TcpListener, body: &str) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0; 1024];
            let read = socket.read(&mut buffer).await.unwrap();
            assert_ne!(read, 0);
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        assert!(
            String::from_utf8(request)
                .unwrap()
                .starts_with("GET http://updater.invalid/artifact HTTP/1.1")
        );
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .as_bytes(),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn new_http_operation_uses_latest_actual_proxy_port() {
        let first = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let second = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = Arc::new(ProxyPort(std::sync::atomic::AtomicU16::new(
            first.local_addr().unwrap().port(),
        )));
        let dir = TempDir::new().unwrap();
        let backend = HttpUpdaterBackend::new(dir.path().to_path_buf(), port.clone());
        for (listener, expected) in [(&first, "first core"), (&second, "restarted core")] {
            port.0.store(
                listener.local_addr().unwrap().port(),
                std::sync::atomic::Ordering::SeqCst,
            );
            let client = backend.http_client().unwrap();
            let (response, ()) = tokio::time::timeout(Duration::from_secs(5), async {
                tokio::join!(
                    async {
                        client
                            .get("http://updater.invalid/artifact")
                            .send()
                            .await
                            .unwrap()
                            .text()
                            .await
                            .unwrap()
                    },
                    proxy_reply(listener, expected),
                )
            })
            .await
            .unwrap();
            assert_eq!(response, expected);
        }
    }

    #[test]
    fn raw_artifact_with_core_filename_is_preserved_during_extraction() {
        let dir = TempDir::new().unwrap();
        let filename = format!("mihomo{}", std::env::consts::EXE_SUFFIX);
        let archive = dir.path().join(&filename);
        std::fs::write(&archive, b"raw core binary").unwrap();
        let prepared_dir = dir.path().join("prepared");
        std::fs::create_dir(&prepared_dir).unwrap();
        let destination = prepared_dir.join(&filename);
        extract_core(archive.clone(), &filename, destination.clone()).unwrap();
        assert_eq!(std::fs::read(archive).unwrap(), b"raw core binary");
        assert_eq!(std::fs::read(destination).unwrap(), b"raw core binary");
    }

    #[test]
    fn extracts_gzip_and_sets_executable_permission() {
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("core.gz");
        let mut encoder = flate2::write::GzEncoder::new(
            std::fs::File::create(&archive).unwrap(),
            flate2::Compression::default(),
        );
        encoder.write_all(b"core binary").unwrap();
        encoder.finish().unwrap();
        let destination = dir.path().join("core");
        extract_core(archive, "core.gz", destination.clone()).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"core binary");
        #[cfg(target_family = "unix")]
        assert_eq!(
            std::fs::metadata(destination).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }

    #[test]
    fn zip_ignores_directories_and_rejects_missing_core() {
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("core.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        writer
            .add_directory("mihomo/", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.finish().unwrap();
        let error = extract_core(archive, "core.zip", dir.path().join("core")).unwrap_err();
        assert!(error.to_string().contains("failed to find core file"));
    }

    #[test]
    fn zip_extracts_core_without_using_archive_paths() {
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("core.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        writer
            .start_file("nested/mihomo", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"core binary").unwrap();
        writer.finish().unwrap();
        let destination = dir.path().join("core");
        extract_core(archive, "core.zip", destination.clone()).unwrap();
        assert_eq!(std::fs::read(destination).unwrap(), b"core binary");
        assert!(!dir.path().join("nested").exists());
    }
}
