use std::{
    ffi::OsStr,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context as _;
use async_trait::async_trait;
use serde::Serialize;
use sha2::{Digest, Sha256};
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
    client::core_lifecycle::ports::{PreparedCoreBinary, PreparedCoreCompanion},
    core::download::{DownloadSession, DownloadStatus},
    utils::candy::{ReqwestSpeedTestExt, parse_gh_url},
};
use nyanpasu_config::application::ClashCore;
use nyanpasu_paths::PathResolver;

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
    proxy_port: Arc<dyn crate::service::profile_file::SelfProxyPortSource>,
    destination_dir: PathBuf,
    /// Resolves where the agent skills a core archive ships go.
    paths: PathResolver,
}

impl HttpUpdaterBackend {
    pub fn new(
        destination_dir: PathBuf,
        paths: PathResolver,
        proxy_port: Arc<dyn crate::service::profile_file::SelfProxyPortSource>,
    ) -> Self {
        Self {
            proxy_port,
            destination_dir,
            paths,
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
        // Meow publishes `<artifact>.sha256` beside each archive.
        let checksum_url = meow_version
            .is_some()
            .then(|| parse_gh_url(&mirror, &format!("{url}.sha256")))
            .transpose()?;
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
        let checksum = match checksum_url {
            Some(url) => Some(tokio::select! {
                checksum = fetch_text(self.http_client()?, url) => {
                    checksum.context("could not fetch the archive's published SHA-256")?
                }
                () = shutdown.cancelled() => anyhow::bail!("updater shut down"),
            }),
            None => None,
        };
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
        let extraction_artifact = artifact.clone();
        // A blocking extraction cannot be aborted midway. It owns staging until
        // completion, and the shutdown waits for it.
        tokio::task::spawn_blocking(move || {
            let archive = extraction_staging.path().join(&extraction_artifact);
            if let Some(checksum) = checksum {
                verify_sha256(&archive, &extraction_artifact, &checksum)?;
            }
            extract_core(
                archive,
                &extraction_artifact,
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
        // Staged now and published by the installer once the core binary is in place.
        // Nothing Nyanpasu runs uses them, so failing to stage them fails no core update.
        let skills = core_type.agent_skills();
        let companion = if skills.is_empty() {
            None
        } else {
            let archive = staging.path().join(&artifact);
            let destination = self
                .paths
                .core_data_dir(core_type.binary_name())
                .into_std_path_buf();
            let staged = tokio::task::spawn_blocking(move || {
                stage_companion(&archive, &artifact, skills, destination)
            })
            .await
            .map_err(|error| match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(error) => error,
            })?;
            staged.unwrap_or_else(|error| {
                tracing::warn!(
                    error = format!("{error:#}"),
                    core = %core_type,
                    "could not stage the agent skills the core archive ships"
                );
                None
            })
        };
        progress.report(UpdaterState::Replacing, None);
        Ok(PreparedCoreBinary {
            target: core_type,
            source,
            destination: self.destination_dir.join(filename),
            staging,
            progress: Arc::new(progress),
            companion,
        })
    }
}

async fn fetch_text(client: reqwest::Client, url: url::Url) -> anyhow::Result<String> {
    Ok(client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?)
}

/// Checks `archive` against `checksum`, the contents of a release's
/// `<artifact>.sha256`: a hex digest, optionally followed by the file name.
fn verify_sha256(archive: &Path, artifact: &str, checksum: &str) -> anyhow::Result<()> {
    let mut fields = checksum.split_whitespace();
    let expected = fields
        .next()
        .ok_or_else(|| anyhow::anyhow!("the published SHA-256 of {artifact} is empty"))?;
    if let Some(name) = fields.next() {
        // `sha256sum` marks a file it read in binary mode with `*`.
        let name = name.strip_prefix('*').unwrap_or(name);
        anyhow::ensure!(
            name == artifact,
            "the published SHA-256 is for {name}, not {artifact}"
        );
    }
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(archive)?;
    let mut buffer = vec![0; 64 * 1024];
    loop {
        match file.read(&mut buffer)? {
            0 => break,
            read => hasher.update(&buffer[..read]),
        }
    }
    anyhow::ensure!(
        hex::encode(hasher.finalize()).eq_ignore_ascii_case(expected),
        "{artifact} does not match its published SHA-256"
    );
    Ok(())
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

/// Where `path`, a file in a core release archive, belongs among the core's
/// agent skills: one of the `skills` executables, or a file of one of their
/// folders under `skills/`. The archive's top-level directory is dropped, and
/// so is every other file.
fn companion_member(path: &Path, skills: &[&str]) -> Option<PathBuf> {
    let names = path
        .components()
        .map(|component| match component {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    for skill in skills {
        let folder = [OsStr::new("skills"), OsStr::new(skill)];
        if let Some(start) = names.windows(2).position(|pair| pair == folder) {
            return (names.len() > start + 2).then(|| names[start..].iter().collect());
        }
    }
    let file = names.last()?.to_str()?;
    skills
        .iter()
        .any(|skill| file == format!("{skill}{}", std::env::consts::EXE_SUFFIX))
        .then(|| PathBuf::from(file))
}

/// Extracts the agent `skills` and their executables from a core release archive
/// into `destination`. Returns whether the archive has all of them: releases
/// before the skills ship the core alone.
fn extract_companion(
    archive_path: &Path,
    artifact: &str,
    skills: &[&str],
    destination: &Path,
) -> anyhow::Result<bool> {
    let mut found = Vec::new();
    let mut extract = |path: &Path, contents: &mut dyn Read| -> anyhow::Result<()> {
        let Some(member) = companion_member(path, skills) else {
            return Ok(());
        };
        let target = destination.join(&member);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::io::copy(contents, &mut std::fs::File::create(&target)?)?;
        #[cfg(target_family = "unix")]
        if !member.starts_with("skills") {
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
        }
        found.push(member);
        Ok(())
    };
    let source = std::fs::File::open(archive_path)?;
    if artifact.ends_with(".tar.gz") {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(source));
        for entry in archive.entries()? {
            let mut entry = entry?;
            if entry.header().entry_type().is_file() {
                let path = entry.path()?.into_owned();
                extract(&path, &mut entry)?;
            }
        }
    } else if artifact.ends_with(".zip") {
        let mut archive = zip::ZipArchive::new(source)?;
        for index in 0..archive.len() {
            let mut file = archive.by_index(index)?;
            if let Some(path) = file.enclosed_name().filter(|_| file.is_file()) {
                extract(&path, &mut file)?;
            }
        }
    }
    Ok(skills.iter().all(|skill| {
        found.contains(&PathBuf::from(format!(
            "{skill}{}",
            std::env::consts::EXE_SUFFIX
        ))) && found.contains(&Path::new("skills").join(skill).join("SKILL.md"))
    }))
}

/// Stages the agent `skills` of a core release archive beside `destination`, for
/// the installer to publish there. An archive without them stages nothing, which
/// leaves the installed ones in place.
fn stage_companion(
    archive_path: &Path,
    artifact: &str,
    skills: &[&str],
    destination: PathBuf,
) -> anyhow::Result<Option<PreparedCoreCompanion>> {
    let (parent, name) = destination
        .parent()
        .zip(destination.file_name())
        .ok_or_else(|| anyhow::anyhow!("the core data directory has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let mut prefix = name.to_os_string();
    prefix.push(".staging-");
    let companion = PreparedCoreCompanion {
        staging: tempfile::Builder::new()
            .prefix(&prefix)
            .tempdir_in(parent)?,
        destination,
    };
    let complete = extract_companion(archive_path, artifact, skills, &companion.staged())?;
    Ok(complete.then_some(companion))
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

    fn test_paths(dir: &TempDir) -> PathResolver {
        let root = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        PathResolver::with_base_dirs(root.join("config"), root.join("data"))
    }

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
        let backend =
            HttpUpdaterBackend::new(dir.path().to_path_buf(), test_paths(&dir), port.clone());
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
        let backend = HttpUpdaterBackend::new(dir.path().to_path_buf(), test_paths(&dir), port);
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

    fn write_tar_gz(path: &Path, files: &[(&str, &[u8])]) {
        let encoder = flate2::write::GzEncoder::new(
            std::fs::File::create(path).unwrap(),
            flate2::Compression::default(),
        );
        let mut archive = tar::Builder::new(encoder);
        for (name, contents) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            archive.append_data(&mut header, name, *contents).unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap();
    }

    const MEOW_SKILLS: &[&str] = &["meow-cli"];

    #[test]
    fn companion_members_keep_the_archive_layout_without_its_top_directory() {
        let executable = format!("meow-cli{}", std::env::consts::EXE_SUFFIX);
        let member = |path: &str| companion_member(Path::new(path), MEOW_SKILLS);

        assert_eq!(
            member(&format!("meow-alpha-8793af9/{executable}")),
            Some(PathBuf::from(&executable))
        );
        assert_eq!(
            member("meow-alpha-8793af9/skills/meow-cli/SKILL.md"),
            Some(PathBuf::from("skills/meow-cli/SKILL.md"))
        );
        assert_eq!(
            member("meow-alpha-8793af9/skills/meow-cli/references/api.md"),
            Some(PathBuf::from("skills/meow-cli/references/api.md"))
        );
        for other in [
            "meow-alpha-8793af9/meow",
            "meow-alpha-8793af9/README.md",
            "meow-alpha-8793af9/skills/other/SKILL.md",
            "meow-alpha-8793af9/skills/meow-cli",
            "../skills/meow-cli/SKILL.md",
        ] {
            assert_eq!(member(other), None, "{other}");
        }
    }

    #[test]
    fn staging_leaves_the_installed_companion_untouched() {
        let dir = TempDir::new().unwrap();
        let executable = format!("meow-cli{}", std::env::consts::EXE_SUFFIX);
        let archive = dir.path().join("meow.tar.gz");
        write_tar_gz(
            &archive,
            &[
                ("meow-alpha/meow", b"meow executable"),
                (&format!("meow-alpha/{executable}"), b"new meow-cli"),
                ("meow-alpha/skills/meow-cli/SKILL.md", b"new skill"),
            ],
        );
        let destination = dir.path().join("data/cores/meow-alpha");
        std::fs::create_dir_all(destination.join("skills/meow-cli")).unwrap();
        std::fs::write(destination.join("skills/meow-cli/SKILL.md"), "old skill").unwrap();

        let companion = stage_companion(
            &archive,
            "meow-alpha-x.tar.gz",
            MEOW_SKILLS,
            destination.clone(),
        )
        .unwrap()
        .expect("the archive ships meow-cli");

        // The installer publishes it; until then the installed one stays.
        assert_eq!(
            std::fs::read(destination.join("skills/meow-cli/SKILL.md")).unwrap(),
            b"old skill"
        );
        assert_eq!(companion.destination, destination);
        let staged = companion.staged();
        assert_eq!(
            std::fs::read(staged.join(&executable)).unwrap(),
            b"new meow-cli"
        );
        assert_eq!(
            std::fs::read(staged.join("skills/meow-cli/SKILL.md")).unwrap(),
            b"new skill"
        );
        assert!(!staged.join("meow").exists());
        // Beside the destination, so that publishing it is a rename on one file system.
        assert_eq!(companion.staging.path().parent(), destination.parent());
        #[cfg(target_family = "unix")]
        assert_eq!(
            std::fs::metadata(staged.join(&executable))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        // Dropping it, as a failed core installation does, removes the staged files.
        drop(companion);
        assert_eq!(
            std::fs::read_dir(dir.path().join("data/cores"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn an_archive_without_the_skills_stages_nothing() {
        let dir = TempDir::new().unwrap();
        let archive = dir.path().join("meow.tar.gz");
        write_tar_gz(&archive, &[("meow-v0.26.1/meow", b"meow executable")]);
        let destination = dir.path().join("data/cores/meow");

        let companion = stage_companion(
            &archive,
            "meow-v0.26.1.tar.gz",
            MEOW_SKILLS,
            destination.clone(),
        )
        .unwrap();

        assert!(companion.is_none());
        assert_eq!(
            std::fs::read_dir(dir.path().join("data/cores"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn a_companion_stages_from_a_zip_archive() {
        let dir = TempDir::new().unwrap();
        let executable = format!("meow-cli{}", std::env::consts::EXE_SUFFIX);
        let archive_path = dir.path().join("meow.zip");
        let mut archive = zip::ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
        archive
            .add_directory(
                "meow-alpha/skills/",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        for (name, contents) in [
            (format!("meow-alpha/{executable}"), "meow-cli"),
            ("meow-alpha/skills/meow-cli/SKILL.md".to_owned(), "skill"),
        ] {
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents.as_bytes()).unwrap();
        }
        archive.finish().unwrap();

        let companion = stage_companion(
            &archive_path,
            "meow-alpha-x.zip",
            MEOW_SKILLS,
            dir.path().join("cores/meow-alpha"),
        )
        .unwrap()
        .expect("the archive ships meow-cli");

        let staged = companion.staged();
        assert_eq!(
            std::fs::read(staged.join(&executable)).unwrap(),
            b"meow-cli"
        );
        assert_eq!(
            std::fs::read(staged.join("skills/meow-cli/SKILL.md")).unwrap(),
            b"skill"
        );
    }

    #[test]
    fn archives_are_checked_against_their_published_sha256() {
        let dir = TempDir::new().unwrap();
        let artifact = "meow-alpha-33ec533-x86_64-pc-windows-msvc.zip";
        let archive = dir.path().join(artifact);
        std::fs::write(&archive, b"meow archive").unwrap();
        let digest = hex::encode(Sha256::digest(b"meow archive"));

        // As Meow publishes it, and as `sha256sum` writes it in text mode.
        verify_sha256(&archive, artifact, &format!("{digest} *{artifact}\n")).unwrap();
        verify_sha256(&archive, artifact, &format!("{digest}  {artifact}")).unwrap();
        verify_sha256(&archive, artifact, &digest.to_uppercase()).unwrap();

        let tampered = hex::encode(Sha256::digest(b"another archive"));
        let error = verify_sha256(&archive, artifact, &tampered).unwrap_err();
        assert!(error.to_string().contains("does not match"), "{error}");
        let error =
            verify_sha256(&archive, artifact, &format!("{digest} *meow-v0.26.1.zip")).unwrap_err();
        assert!(
            error.to_string().contains("is for meow-v0.26.1.zip"),
            "{error}"
        );
        verify_sha256(&archive, artifact, " \n").unwrap_err();
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
