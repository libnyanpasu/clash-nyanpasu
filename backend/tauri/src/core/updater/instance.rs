use nyanpasu_core::network::{ReqwestSpeedTestExt, parse_gh_url};
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
use tokio_util::sync::CancellationToken;

use super::{
    ManifestVersion,
    ports::{UpdaterBackend, UpdaterProgress},
    shared::{self, CoreTypeMeta},
};
use crate::{
    client::core_lifecycle::ports::PreparedCoreBinary,
    core::download::{DownloadSession, DownloadStatus},
};
use nyanpasu_config::application::ClashCore;

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

#[derive(Debug, Clone, Serialize, Type)]
pub struct UpdaterSummary {
    pub id: usize,
    pub state: UpdaterState,
    pub downloader: DownloadStatus,
}

pub(crate) struct HttpUpdaterBackend {
    proxy_port: Arc<dyn nyanpasu_core::network::SelfProxyPortSource>,
    destination_dir: PathBuf,
    user_agent: String,
}

impl HttpUpdaterBackend {
    pub fn new(
        destination_dir: PathBuf,
        proxy_port: Arc<dyn nyanpasu_core::network::SelfProxyPortSource>,
        user_agent: String,
    ) -> Self {
        Self {
            proxy_port,
            destination_dir,
            user_agent,
        }
    }

    fn http_client(&self) -> anyhow::Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder()
            .user_agent(&self.user_agent)
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
                    nyanpasu_core::network::INTERNAL_MIRRORS,
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
        shutdown: &CancellationToken,
    ) -> anyhow::Result<PreparedCoreBinary> {
        let staging = Arc::new(TempDir::new()?);
        let mut url = url::Url::parse("https://github.com")?;
        let meow_version = match &tag {
            CoreTypeMeta::Meow(version) if core_type == ClashCore::Meow => {
                Some((core_type, version.to_string()))
            }
            CoreTypeMeta::MeowAlpha(version) if core_type == ClashCore::MeowAlpha => {
                Some((core_type, version.to_string()))
            }
            _ => None,
        };
        url.set_path(&shared::get_download_path(tag, &artifact));
        let url = parse_gh_url(&mirror, url.as_str())?;
        let session =
            DownloadSession::new(self.http_client()?, url, staging.path().join(&artifact));
        let downloader = OwnedDownload(tokio::select! {
            session = session => session?,
            () = shutdown.cancelled() => anyhow::bail!("updater shut down"),
        });
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
                () = shutdown.cancelled() => anyhow::bail!("updater shut down"),
            }
        }
        progress.report(UpdaterState::Decompressing, None);
        let filename = format!(
            "{}{}",
            core_type.binary_name(),
            std::env::consts::EXE_SUFFIX
        );
        let prepared_dir = staging.path().join("prepared");
        tokio::fs::create_dir(&prepared_dir).await?;
        let source = prepared_dir.join(&filename);
        let extraction_staging = staging.clone();
        let extraction_source = source.clone();
        let expected_archive_binary = match core_type {
            ClashCore::Meow | ClashCore::MeowAlpha => {
                Some(format!("meow{}", std::env::consts::EXE_SUFFIX))
            }
            _ => None,
        };
        // A blocking extraction cannot be aborted midway. It owns staging until
        // completion, and the shutdown waits for it.
        tokio::task::spawn_blocking(move || {
            extract_core(
                extraction_staging.path().join(&artifact),
                &artifact,
                expected_archive_binary.as_deref(),
                extraction_source,
            )
        })
        .await
        .map_err(|error| match error.try_into_panic() {
            Ok(panic) => std::panic::resume_unwind(panic),
            Err(error) => error,
        })??;
        if let Some((core, expected_version)) = meow_version {
            let output = tokio::process::Command::new(&source)
                .arg("-v")
                .output()
                .await?;
            anyhow::ensure!(output.status.success(), "Meow version probe failed");
            let banner = String::from_utf8_lossy(&output.stdout);
            verify_meow_version(core, &expected_version, &banner)?;
        }
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

fn extract_core(
    archive_path: PathBuf,
    artifact: &str,
    expected_archive_binary: Option<&str>,
    destination: PathBuf,
) -> anyhow::Result<()> {
    let mut source = std::fs::File::open(archive_path)?;
    if artifact.ends_with(".gz") {
        if artifact.ends_with(".tar.gz") {
            let decoder = flate2::read::GzDecoder::new(source);
            let mut archive = tar::Archive::new(decoder);
            let expected = expected_archive_binary.map(|name| {
                name.strip_suffix(std::env::consts::EXE_SUFFIX)
                    .unwrap_or(name)
            });
            let mut found = false;
            for entry in archive.entries()? {
                let mut entry = entry?;
                if !entry.header().entry_type().is_file() {
                    continue;
                }
                let path = entry.path()?;
                if path.file_name().and_then(|name| name.to_str()) == expected {
                    let mut output = std::fs::File::create(&destination)?;
                    std::io::copy(&mut entry, &mut output)?;
                    found = true;
                    break;
                }
            }
            anyhow::ensure!(found, "failed to find core file in a tar.gz archive");
        } else {
            let mut output = std::fs::File::create(&destination)?;
            std::io::copy(&mut flate2::read::GzDecoder::new(source), &mut output)?;
        }
    } else if artifact.ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(source)?;
        let mut found = false;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            let matches = expected_archive_binary.map_or_else(
                || {
                    file.name().ends_with(".exe")
                        && ["mihomo", "clash", "meow"]
                            .iter()
                            .any(|name| file.name().contains(name))
                },
                |expected| file.name().rsplit('/').next() == Some(expected),
            );
            if !file.is_dir() && matches {
                let mut output = std::fs::File::create(&destination)?;
                std::io::copy(&mut file, &mut output)?;
                found = true;
                break;
            }
        }
        anyhow::ensure!(found, "failed to find core file in a zip archive");
    } else {
        let mut output = std::fs::File::create(&destination)?;
        std::io::copy(&mut source, &mut output)?;
    }
    #[cfg(target_family = "unix")]
    std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

fn verify_meow_version(core: ClashCore, expected: &str, banner: &str) -> anyhow::Result<()> {
    let parsed = crate::client::core_version::parse_version(core, banner)?;
    let actual = semver::Version::parse(parsed.strip_prefix('v').unwrap_or(&parsed))?;
    match core {
        ClashCore::Meow => {
            let expected = semver::Version::parse(expected.strip_prefix('v').unwrap_or(expected))?;
            anyhow::ensure!(
                actual == expected,
                "Meow binary version did not match the release tag"
            );
        }
        ClashCore::MeowAlpha => {
            let sha = expected
                .strip_prefix("alpha-")
                .ok_or_else(|| anyhow::anyhow!("invalid Meow Alpha version identity"))?;
            anyhow::ensure!(
                actual.build.as_str().eq_ignore_ascii_case(sha),
                "Meow Alpha binary version did not match the release commit"
            );
        }
        _ => anyhow::bail!("unsupported Meow core version verification"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct ProxyPort(std::sync::atomic::AtomicU16);

    impl nyanpasu_core::network::SelfProxyPortSource for ProxyPort {
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
        let request = String::from_utf8(request).unwrap();
        assert!(request.starts_with("GET http://updater.invalid/artifact HTTP/1.1"));
        assert!(request.contains("user-agent: clash-nyanpasu/test-runtime-version\r\n"));
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
        let backend = HttpUpdaterBackend::new(
            dir.path().to_path_buf(),
            port.clone(),
            "clash-nyanpasu/test-runtime-version".to_owned(),
        );
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

    /// A mirror that answers the download's HEAD probes, unless `stall_probe`,
    /// and then stalls: it reports the stalled request and never finishes it.
    async fn stalling_mirror(
        listener: tokio::net::TcpListener,
        stall_probe: bool,
        stalled: tokio::sync::mpsc::UnboundedSender<()>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let stalled = stalled.clone();
            tokio::spawn(async move {
                loop {
                    let mut request = Vec::new();
                    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                        let mut buffer = [0; 1024];
                        match socket.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => request.extend_from_slice(&buffer[..read]),
                        }
                    }
                    let head = b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\n\r\n";
                    let probe = request.starts_with(b"HEAD ");
                    if probe && !stall_probe {
                        if socket.write_all(head).await.is_err() {
                            return;
                        }
                        continue;
                    }
                    if !probe {
                        let _ = socket.write_all(head).await;
                        let _ = socket.write_all(b"partial").await;
                    }
                    let _ = stalled.send(());
                    std::future::pending::<()>().await;
                }
            });
        }
    }

    /// Prepares against a mirror that stalls, cancels the token once it has
    /// stalled, and returns the preparation's error.
    async fn prepare_cancelled_at_the_stall(stall_probe: bool) -> anyhow::Error {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = Arc::new(ProxyPort(std::sync::atomic::AtomicU16::new(
            listener.local_addr().unwrap().port(),
        )));
        let (stalled, mut stall) = tokio::sync::mpsc::unbounded_channel();
        let mirror = tokio::spawn(stalling_mirror(listener, stall_probe, stalled));
        let dir = TempDir::new().unwrap();
        let backend = HttpUpdaterBackend::new(
            dir.path().to_path_buf(),
            port,
            "clash-nyanpasu/test-runtime-version".to_owned(),
        );
        let token = CancellationToken::new();
        let prepare = tokio::spawn({
            let token = token.clone();
            async move {
                backend
                    .prepare(
                        ClashCore::Mihomo,
                        "http://updater.invalid".into(),
                        "mihomo.gz".into(),
                        CoreTypeMeta::Mihomo("v1".into()),
                        UpdaterProgress::new(|_, _| {}),
                        &token,
                    )
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(5), stall.recv())
            .await
            .unwrap()
            .unwrap();

        token.cancel();
        let error = tokio::time::timeout(Duration::from_secs(5), prepare)
            .await
            .expect("the token ends the download")
            .unwrap()
            .err()
            .unwrap();
        mirror.abort();
        error
    }

    #[tokio::test]
    async fn the_shutdown_ends_a_download_whose_body_never_finishes() {
        let error = prepare_cancelled_at_the_stall(false).await;
        assert_eq!(error.to_string(), "updater shut down");
    }

    #[tokio::test]
    async fn the_shutdown_ends_a_download_whose_probe_never_returns() {
        let error = prepare_cancelled_at_the_stall(true).await;
        assert_eq!(error.to_string(), "updater shut down");
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
        extract_core(archive.clone(), &filename, None, destination.clone()).unwrap();
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
        extract_core(archive, "core.gz", None, destination.clone()).unwrap();
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
        let error = extract_core(archive, "core.zip", None, dir.path().join("core")).unwrap_err();
        assert!(error.to_string().contains("failed to find core file"));
    }

    #[test]
    fn zip_extracts_core_without_using_archive_paths() {
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("core.zip");
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        writer
            .start_file(
                "nested/mihomo.exe",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.write_all(b"core binary").unwrap();
        writer.finish().unwrap();
        let destination = dir.path().join("core");
        extract_core(archive, "core.zip", None, destination.clone()).unwrap();
        assert_eq!(std::fs::read(destination).unwrap(), b"core binary");
        assert!(!dir.path().join("nested").exists());
    }

    #[test]
    fn tar_gz_selects_the_exact_meow_executable_instead_of_readme() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("meow.tar.gz");
        let encoder = flate2::write::GzEncoder::new(
            std::fs::File::create(&archive_path).unwrap(),
            flate2::Compression::default(),
        );
        let mut archive = tar::Builder::new(encoder);
        let mut readme = tar::Header::new_gnu();
        readme.set_size(b"Meow setup instructions".len() as u64);
        readme.set_mode(0o644);
        readme.set_cksum();
        archive
            .append_data(
                &mut readme,
                "README-meow.md",
                &b"Meow setup instructions"[..],
            )
            .unwrap();
        let mut binary = tar::Header::new_gnu();
        binary.set_size(b"meow executable".len() as u64);
        binary.set_mode(0o755);
        binary.set_cksum();
        archive
            .append_data(&mut binary, "release/meow", &b"meow executable"[..])
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap();

        let destination = dir.path().join("meow-alpha");
        extract_core(
            archive_path,
            "meow-alpha-3c27aca-aarch64-apple-darwin.tar.gz",
            Some("meow"),
            destination.clone(),
        )
        .unwrap();
        assert_eq!(std::fs::read(destination).unwrap(), b"meow executable");
    }

    #[test]
    fn meow_versions_are_verified_before_replacement() {
        verify_meow_version(ClashCore::Meow, "v0.22.0", "meow version 0.22.0").unwrap();
        verify_meow_version(
            ClashCore::MeowAlpha,
            "alpha-3c27aca",
            "meow version 0.22.0-alpha+3c27aca",
        )
        .unwrap();
        assert!(
            verify_meow_version(
                ClashCore::MeowAlpha,
                "alpha-3c27aca",
                "meow version 0.22.0-alpha+deadbee",
            )
            .is_err()
        );
        assert!(verify_meow_version(ClashCore::Meow, "v0.22.0", "not a version").is_err());
    }

    #[test]
    fn meow_zip_selects_the_exact_executable_member() {
        let dir = TempDir::new().unwrap();
        let archive_path = dir.path().join("meow.zip");
        let mut archive = zip::ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
        archive
            .start_file(
                "README-meow.exe.txt",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(b"documentation").unwrap();
        archive
            .start_file("release/meow.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"meow executable").unwrap();
        archive.finish().unwrap();

        let destination = dir.path().join("meow-alpha.exe");
        extract_core(
            archive_path,
            "meow-alpha-3c27aca-x86_64-pc-windows-msvc.zip",
            Some("meow.exe"),
            destination.clone(),
        )
        .unwrap();
        assert_eq!(std::fs::read(destination).unwrap(), b"meow executable");
    }
}
