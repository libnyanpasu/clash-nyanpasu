//! ProfileFsPort + SubscriptionFetcher over the real filesystem, reqwest and
//! the OS proxy state (design §7). Tauri-free and legacy-Config-free; the
//! self-proxy port arrives via [`SelfProxyPortSource`] instead of the legacy
//! global read (config/profile/item/remote.rs:130-136 FIXME).

use std::{io::Write, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use nyanpasu_config::profile::{
    ExternalProfilePath, ManagedProfilePath, RemoteProfileOptions, SubscriptionInfo,
};
use url::Url;

use crate::{
    state::profiles::ports::{FetchedSubscription, ProfileFsPort, SubscriptionFetcher},
    utils::path::PathResolver,
};

/// Where the fetcher looks up the app's own mixed port when `self_proxy` is
/// requested. Wired at the composition root (T07); tests inject a constant.
#[cfg_attr(test, mockall::automock)]
pub trait SelfProxyPortSource: Send + Sync + 'static {
    fn mixed_port(&self) -> Option<u16>;
}

pub struct ProfileFileService {
    paths: PathResolver,
    self_proxy_port: Arc<dyn SelfProxyPortSource>,
    http_timeout: Duration,
}

impl ProfileFileService {
    pub fn new(paths: PathResolver, self_proxy_port: Arc<dyn SelfProxyPortSource>) -> Self {
        Self {
            paths,
            self_proxy_port,
            http_timeout: Duration::from_secs(30),
        }
    }

    #[cfg(test)]
    fn with_http_timeout(mut self, timeout: Duration) -> Self {
        self.http_timeout = timeout;
        self
    }

    fn resolve(&self, path: &ManagedProfilePath) -> PathBuf {
        self.paths.app_profiles_dir().join(path.as_path())
    }
}

/// Parse and reserialize a YAML mapping so editor saves and File-config reads
/// share one canonical shape (legacy `read_profile_file` normalization).
pub fn normalize_yaml_document(content: &str) -> anyhow::Result<String> {
    let mapping: serde_yaml::Mapping =
        serde_yaml::from_str(content).context("document is not a YAML mapping")?;
    serde_yaml::to_string(&mapping).context("failed to reserialize YAML mapping")
}

impl ProfileFsPort for ProfileFileService {
    fn read(&self, path: &ManagedProfilePath) -> anyhow::Result<String> {
        let full = self.resolve(path);
        std::fs::read_to_string(&full)
            .with_context(|| format!("read profile file {}", full.display()))
    }

    fn write_atomic(&self, path: &ManagedProfilePath, content: &str) -> anyhow::Result<()> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        AtomicFile::new(&full, OverwriteBehavior::AllowOverwrite)
            .write(|file| file.write_all(content.as_bytes()))
            .with_context(|| format!("atomic write {}", full.display()))
    }

    fn remove(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        let full = self.resolve(path);
        match std::fs::remove_file(&full) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("remove profile file {}", full.display())),
        }
    }

    fn ensure_not_symlink(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        let full = self.resolve(path);
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() => bail!(
                "refusing to write through unexpected symlink at {}",
                full.display()
            ),
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("inspect profile file {}", full.display())),
        }
    }

    fn ensure_symlink(
        &self,
        path: &ManagedProfilePath,
        target: &ExternalProfilePath,
    ) -> anyhow::Result<()> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if std::fs::read_link(&full)? == target.as_path() {
                    return Ok(());
                }
                std::fs::remove_file(&full)?;
            }
            Ok(_) => bail!(
                "existing non-symlink file at {}, refusing to replace",
                full.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => Err(e).with_context(|| format!("inspect profile file {}", full.display()))?,
        }
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(target.as_path(), &full)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target.as_path(), &full)?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl SubscriptionFetcher for ProfileFileService {
    async fn fetch(
        &self,
        url: &Url,
        options: &RemoteProfileOptions,
    ) -> anyhow::Result<FetchedSubscription> {
        use backon::Retryable;

        let mut builder = reqwest::ClientBuilder::new()
            .use_rustls_tls()
            .no_proxy()
            .timeout(self.http_timeout);

        // Proxy precedence mirrors the legacy subscriber (remote.rs:129-150):
        // self_proxy wins, then system proxy, else direct.
        let proxy_url = if options.self_proxy {
            self.self_proxy_port
                .mixed_port()
                .map(|port| format!("http://127.0.0.1:{port}"))
        } else if options.with_proxy {
            match sysproxy::Sysproxy::get_system_proxy() {
                Ok(p @ sysproxy::Sysproxy { enable: true, .. }) => {
                    Some(format!("http://{}:{}", p.host, p.port))
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(proxy_url) = proxy_url {
            use crate::utils::config::NyanpasuReqwestProxyExt;
            builder = builder.swift_set_proxy(&proxy_url);
        }

        let user_agent = options
            .user_agent
            .clone()
            .unwrap_or_else(|| format!("clash-nyanpasu/v{}", crate::utils::dirs::APP_VERSION));
        let client = builder.user_agent(user_agent).build()?;

        let device_info = crate::utils::hwid::get_device_info();
        let sanitize = crate::utils::hwid::sanitize_for_header;
        let perform = || async {
            client
                .get(url.as_str())
                .header("x-hwid", &device_info.hwid)
                .header("x-device-os", sanitize(&device_info.device_os))
                .header("x-ver-os", sanitize(&device_info.os_version))
                .header("x-device-model", sanitize(&device_info.device_model))
                .send()
                .await?
                .error_for_status()
        };
        let resp = perform
            .retry(backon::ExponentialBuilder::default())
            .when(|error: &reqwest::Error| {
                !error.is_status()
                    || error.status().is_some_and(|status| {
                        !matches!(
                            status,
                            reqwest::StatusCode::FORBIDDEN
                                | reqwest::StatusCode::NOT_FOUND
                                | reqwest::StatusCode::UNAUTHORIZED
                        )
                    })
            })
            .await
            .with_context(|| format!("subscription download failed: {url}"))?;

        let subscription = parse_subscription_userinfo(resp.headers());
        let filename = parse_profile_title(resp.headers());
        let content = resp
            .text_with_charset("utf-8")
            .await
            .with_context(|| format!("read subscription response body: {url}"))?;
        let content = content.trim_start_matches('\u{feff}').to_owned();
        Ok(FetchedSubscription {
            content,
            filename,
            subscription,
        })
    }
}

fn parse_subscription_userinfo(headers: &reqwest::header::HeaderMap) -> SubscriptionInfo {
    let Some(value) = headers
        .get("subscription-userinfo")
        .or_else(|| headers.get("Subscription-Userinfo"))
    else {
        return SubscriptionInfo::default();
    };
    let raw = value.to_str().unwrap_or("");
    let field = |key: &str| crate::utils::help::parse_str::<u64>(raw, key);
    SubscriptionInfo {
        upload: field("upload"),
        download: field("download"),
        total: field("total"),
        expire: field("expire")
            .filter(|secs| *secs != 0)
            .and_then(|secs| time::OffsetDateTime::from_unix_timestamp(secs as i64).ok()),
    }
}

fn parse_profile_title(headers: &reqwest::header::HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("profile-title").and_then(|v| v.to_str().ok()) {
        if let Some(encoded) = value.strip_prefix("base64:") {
            use base64::Engine;
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                if let Ok(decoded) = String::from_utf8(bytes) {
                    return Some(decoded);
                }
            }
        } else {
            return Some(value.to_owned());
        }
    }

    parse_filename_from_content_disposition(headers)
}

fn parse_filename_from_content_disposition(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let value = headers
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())?;

    value
        .split(';')
        .map(str::trim)
        .find_map(|part| {
            part.strip_prefix("filename*=")
                .and_then(decode_rfc5987_filename)
                .or_else(|| {
                    part.strip_prefix("filename=")
                        .map(|filename| filename.trim().trim_matches(['"', '\'']).to_owned())
                })
        })
        .filter(|filename| !filename.is_empty())
}

fn decode_rfc5987_filename(value: &str) -> Option<String> {
    let value = value.trim().trim_matches(['"', '\'']);
    let encoded = value.split("''").last().unwrap_or(value);
    percent_encoding::percent_decode(encoded.as_bytes())
        .decode_utf8()
        .ok()
        .map(|decoded| decoded.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::profiles::ports::SubscriptionFetcher;
    use axum::{Router, http::HeaderMap as AxumHeaderMap, response::IntoResponse, routing::get};
    use nyanpasu_config::profile::{ManagedProfilePath, RemoteProfileOptions};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    use url::Url;

    struct NoProxy;
    impl SelfProxyPortSource for NoProxy {
        fn mixed_port(&self) -> Option<u16> {
            None
        }
    }

    fn service() -> (tempfile::TempDir, ProfileFileService) {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::utils::path::PathResolver::with_base_dirs(
            temp.path().join("config"),
            temp.path().join("data"),
        );
        (temp, ProfileFileService::new(paths, Arc::new(NoProxy)))
    }

    fn managed(name: &str) -> ManagedProfilePath {
        ManagedProfilePath::new(name).unwrap()
    }

    async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{addr}/"), handle)
    }

    fn options_direct() -> RemoteProfileOptions {
        RemoteProfileOptions {
            user_agent: None,
            with_proxy: false,
            self_proxy: false,
            update_interval_minutes: 120,
        }
    }

    #[test]
    fn write_atomic_then_read_round_trips_and_creates_parent() {
        let (_temp, service) = service();
        let path = managed("abc.yaml");
        service.write_atomic(&path, "proxies: []\n").unwrap();
        assert_eq!(service.read(&path).unwrap(), "proxies: []\n");
        service.write_atomic(&path, "mode: rule\n").unwrap();
        assert_eq!(service.read(&path).unwrap(), "mode: rule\n");
    }

    #[test]
    fn remove_is_idempotent() {
        let (_temp, service) = service();
        let path = managed("gone.yaml");
        service.remove(&path).unwrap();
        service.write_atomic(&path, "x: 1\n").unwrap();
        service.remove(&path).unwrap();
        assert!(service.read(&path).is_err());
    }

    #[test]
    fn ensure_not_symlink_rejects_links_and_accepts_files() {
        let (_temp, service) = service();
        let path = managed("real.yaml");
        service.ensure_not_symlink(&path).unwrap();
        service.write_atomic(&path, "x: 1\n").unwrap();
        service.ensure_not_symlink(&path).unwrap();

        let link = managed("link.yaml");
        let target_file = service.resolve(&path);
        let link_file = service.resolve(&link);
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&target_file, &link_file);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&target_file, &link_file);
        if made.is_err() {
            eprintln!("symlink unsupported in this environment, skipping");
            return;
        }
        assert!(service.ensure_not_symlink(&link).is_err());
    }

    #[test]
    fn ensure_symlink_creates_and_repairs() {
        let (temp, service) = service();
        let outside = temp.path().join("outside.yaml");
        std::fs::write(&outside, "external: true\n").unwrap();
        let target =
            nyanpasu_config::profile::ExternalProfilePath::new(outside.to_string_lossy()).unwrap();
        let link = managed("ext.yaml");
        if service.ensure_symlink(&link, &target).is_err() {
            eprintln!("symlink unsupported in this environment, skipping");
            return;
        }
        assert_eq!(service.read(&link).unwrap(), "external: true\n");
        service.ensure_symlink(&link, &target).unwrap();

        let occupied = managed("occupied.yaml");
        service.write_atomic(&occupied, "x: 1\n").unwrap();
        assert!(service.ensure_symlink(&occupied, &target).is_err());
    }

    #[test]
    fn normalize_yaml_document_round_trips_mappings_and_rejects_garbage() {
        let normalized = normalize_yaml_document("b: 2\na: 1\n").unwrap();
        let value: serde_yaml::Mapping = serde_yaml::from_str(&normalized).unwrap();
        assert_eq!(value.len(), 2);
        assert!(normalize_yaml_document(": not yaml [").is_err());
    }

    #[tokio::test]
    async fn fetch_parses_userinfo_and_title_headers() {
        let router = Router::new().route(
            "/",
            get(|| async {
                let mut headers = AxumHeaderMap::new();
                headers.insert(
                    "subscription-userinfo",
                    "upload=1; download=2; total=3; expire=0".parse().unwrap(),
                );
                headers.insert("profile-title", "My Sub".parse().unwrap());
                (headers, "proxies: []\n")
            }),
        );
        let (url, _server) = serve(router).await;
        let (_temp, service) = service();
        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();
        assert_eq!(fetched.content, "proxies: []\n");
        assert_eq!(fetched.filename.as_deref(), Some("My Sub"));
        assert_eq!(fetched.subscription.upload, Some(1));
        assert_eq!(fetched.subscription.download, Some(2));
        assert_eq!(fetched.subscription.total, Some(3));
        assert!(fetched.subscription.expire.is_none());
    }

    #[tokio::test]
    async fn fetch_retries_transient_errors_but_not_auth_failures() {
        static HITS: AtomicUsize = AtomicUsize::new(0);
        let flaky = Router::new().route(
            "/",
            get(|| async {
                if HITS.fetch_add(1, Ordering::SeqCst) == 0 {
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
                } else {
                    "ok: true\n".into_response()
                }
            }),
        );
        let (url, _server) = serve(flaky).await;
        let (_temp, service) = service();
        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();
        assert_eq!(fetched.content, "ok: true\n");
        assert!(HITS.load(Ordering::SeqCst) >= 2);

        static AUTH_HITS: AtomicUsize = AtomicUsize::new(0);
        let forbidden = Router::new().route(
            "/",
            get(|| async {
                AUTH_HITS.fetch_add(1, Ordering::SeqCst);
                axum::http::StatusCode::FORBIDDEN
            }),
        );
        let (url, _server) = serve(forbidden).await;
        assert!(
            service
                .fetch(&Url::parse(&url).unwrap(), &options_direct())
                .await
                .is_err()
        );
        assert_eq!(AUTH_HITS.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fetch_timeout_is_managed_internally() {
        let slow = Router::new().route(
            "/",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                "never"
            }),
        );
        let (url, _server) = serve(slow).await;
        let (_temp, service) = service();
        let service = service.with_http_timeout(Duration::from_millis(300));
        let started = Instant::now();
        assert!(
            service
                .fetch(&Url::parse(&url).unwrap(), &options_direct())
                .await
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
