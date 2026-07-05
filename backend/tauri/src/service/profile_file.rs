//! ProfileFsPort + SubscriptionFetcher over the real filesystem, reqwest and
//! the OS proxy state (design §7). Tauri-free and legacy-Config-free; the
//! self-proxy port arrives via [`SelfProxyPortSource`] instead of the legacy
//! global read (config/profile/item/remote.rs:130-136 FIXME).

use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

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

    fn resolve(&self, path: &ManagedProfilePath) -> anyhow::Result<PathBuf> {
        let full = self.paths.app_profiles_dir().join(path.as_path());
        self.ensure_parent_contained(&full)?;
        Ok(full)
    }

    fn ensure_parent_contained(&self, full: &Path) -> anyhow::Result<()> {
        let profiles_dir = self.paths.app_profiles_dir();
        let Some(parent) = full.parent() else {
            return Ok(());
        };
        let profiles_dir = match canonicalize_for_compare(&profiles_dir) {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).context("canonicalize profile directory"),
        };

        let mut ancestor = parent;
        loop {
            if ancestor.exists() || std::fs::symlink_metadata(ancestor).is_ok() {
                let ancestor = canonicalize_for_compare(ancestor).with_context(|| {
                    format!("canonicalize profile parent {}", ancestor.display())
                })?;
                if !ancestor.starts_with(&profiles_dir) {
                    bail!(
                        "profile path containment violation: {} escapes {}",
                        full.display(),
                        profiles_dir.display()
                    );
                }
                return Ok(());
            }
            if ancestor == profiles_dir {
                return Ok(());
            }
            let Some(parent) = ancestor.parent() else {
                return Ok(());
            };
            ancestor = parent;
        }
    }
}

fn canonicalize_for_compare(path: &Path) -> std::io::Result<PathBuf> {
    std::fs::canonicalize(path)
}

fn symlink_points_to(link: &Path, target: &Path) -> anyhow::Result<bool> {
    let existing = std::fs::read_link(link)?;
    let existing = if existing.is_absolute() {
        existing
    } else {
        link.parent()
            .map(|parent| parent.join(&existing))
            .unwrap_or(existing)
    };
    let Ok(existing) = canonicalize_for_compare(&existing) else {
        return Ok(false);
    };
    let Ok(target) = canonicalize_for_compare(target) else {
        return Ok(false);
    };
    Ok(existing == target)
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
        let full = self.resolve(path)?;
        std::fs::read_to_string(&full)
            .with_context(|| format!("read profile file {}", full.display()))
    }

    fn write_atomic(&self, path: &ManagedProfilePath, content: &str) -> anyhow::Result<()> {
        let full = self.resolve(path)?;
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        AtomicFile::new(&full, OverwriteBehavior::AllowOverwrite)
            .write(|file| file.write_all(content.as_bytes()))
            .with_context(|| format!("atomic write {}", full.display()))
    }

    fn remove(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        let full = self.resolve(path)?;
        match std::fs::remove_file(&full) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).with_context(|| format!("remove profile file {}", full.display())),
        }
    }

    fn read_external(&self, target: &ExternalProfilePath) -> anyhow::Result<String> {
        std::fs::read_to_string(target.as_path())
            .with_context(|| format!("read external profile target {target}"))
    }

    fn ensure_not_symlink(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        let full = self.resolve(path)?;
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
        let full = self.resolve(path)?;
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::symlink_metadata(&full) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if symlink_points_to(&full, target.as_path())? {
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
        } else {
            None
        };
        let proxy_url = proxy_url.or_else(|| {
            if options.with_proxy {
                match sysproxy::Sysproxy::get_system_proxy() {
                    Ok(p @ sysproxy::Sysproxy { enable: true, .. }) => {
                        Some(format!("http://{}:{}", p.host, p.port))
                    }
                    _ => None,
                }
            } else {
                None
            }
        });
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
        let content = if let Some(content) = content.strip_prefix('\u{feff}') {
            content.to_owned()
        } else {
            content
        };
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
            .and_then(|secs| i64::try_from(secs).ok())
            .and_then(|secs| time::OffsetDateTime::from_unix_timestamp(secs).ok()),
    }
}

fn parse_profile_title(headers: &reqwest::header::HeaderMap) -> Option<String> {
    if let Some(value) = headers.get("profile-title").and_then(|v| v.to_str().ok()) {
        if value.trim().is_empty() {
            return parse_filename_from_content_disposition(headers);
        }
        if let Some(encoded) = value.strip_prefix("base64:") {
            use base64::Engine;
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                if let Ok(decoded) = String::from_utf8(bytes) {
                    if !decoded.trim().is_empty() {
                        return Some(decoded);
                    }
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
        .filter_map(|part| part.split_once('='))
        .find_map(|(name, value)| {
            let name = name.trim();
            let value = value.trim();
            if name.eq_ignore_ascii_case("filename*") {
                decode_rfc5987_filename(value)
            } else if name.eq_ignore_ascii_case("filename") {
                Some(value.trim_matches(['"', '\'']).to_owned())
            } else {
                None
            }
        })
        .filter(|filename| !filename.is_empty())
}

fn decode_rfc5987_filename(value: &str) -> Option<String> {
    let value = value.trim().trim_matches(['"', '\'']);
    let encoded = value
        .split_once('\'')
        .and_then(|(_, rest)| rest.split_once('\'').map(|(_, encoded)| encoded))
        .unwrap_or(value);
    percent_encoding::percent_decode(encoded.as_bytes())
        .decode_utf8()
        .ok()
        .map(|decoded| decoded.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::profiles::ports::SubscriptionFetcher;
    use axum::{
        Router,
        http::{HeaderMap as AxumHeaderMap, StatusCode, header},
        response::IntoResponse,
        routing::get,
    };
    use nyanpasu_config::profile::{ManagedProfilePath, RemoteProfileOptions};
    use std::{
        sync::{
            Arc, Mutex,
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

    struct CountingNoProxy {
        hits: Arc<AtomicUsize>,
    }

    impl SelfProxyPortSource for CountingNoProxy {
        fn mixed_port(&self) -> Option<u16> {
            self.hits.fetch_add(1, Ordering::SeqCst);
            None
        }
    }

    fn service() -> (tempfile::TempDir, ProfileFileService) {
        service_with(Arc::new(NoProxy))
    }

    fn service_with(
        self_proxy_port: Arc<dyn SelfProxyPortSource>,
    ) -> (tempfile::TempDir, ProfileFileService) {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::utils::path::PathResolver::with_base_dirs(
            temp.path().join("config"),
            temp.path().join("data"),
        );
        (temp, ProfileFileService::new(paths, self_proxy_port))
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
        options(false, false)
    }

    fn options(self_proxy: bool, with_proxy: bool) -> RemoteProfileOptions {
        RemoteProfileOptions {
            user_agent: None,
            with_proxy,
            self_proxy,
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
    fn write_atomic_rejects_parent_symlink_escape() {
        let (temp, service) = service();
        let profiles_dir = service.paths.app_profiles_dir();
        std::fs::create_dir_all(&profiles_dir).unwrap();
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let link_dir = profiles_dir.join("nested");
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(&outside, &link_dir);
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&outside, &link_dir);
        if made.is_err() {
            eprintln!("directory symlink unsupported in this environment, skipping");
            return;
        }

        let err = service
            .write_atomic(&managed("nested/x.yaml"), "escaped: true\n")
            .unwrap_err();
        assert!(format!("{err:#}").contains("containment"));
        assert!(!outside.join("x.yaml").exists());
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
        let target_file = service.resolve(&path).unwrap();
        let link_file = service.resolve(&link).unwrap();
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
    fn ensure_symlink_keeps_existing_link_when_canonical_targets_match() {
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

        let link_path = service.resolve(&link).unwrap();
        let before = std::fs::read_link(&link_path).unwrap();
        let canonical_target = std::fs::canonicalize(&outside).unwrap();
        let equivalent_target =
            nyanpasu_config::profile::ExternalProfilePath::new(canonical_target.to_string_lossy())
                .unwrap();

        service.ensure_symlink(&link, &equivalent_target).unwrap();
        assert_eq!(std::fs::read_link(&link_path).unwrap(), before);
    }

    #[test]
    fn normalize_yaml_document_round_trips_mappings_and_rejects_garbage() {
        let normalized = normalize_yaml_document("b: 2\na: 1\n").unwrap();
        let value: serde_yaml::Mapping = serde_yaml::from_str(&normalized).unwrap();
        assert_eq!(value.len(), 2);
        assert!(normalize_yaml_document(": not yaml [").is_err());
    }

    #[test]
    fn content_disposition_filename_parsing_matches_legacy_variants() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_static("attachment; Filename=\"a.yaml\""),
        );
        assert_eq!(
            parse_filename_from_content_disposition(&headers).as_deref(),
            Some("a.yaml")
        );

        headers.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_static(
                "attachment; filename*=UTF-8'en'my%20cfg.yaml",
            ),
        );
        assert_eq!(
            parse_filename_from_content_disposition(&headers).as_deref(),
            Some("my cfg.yaml")
        );

        headers.insert(
            "profile-title",
            reqwest::header::HeaderValue::from_static("   "),
        );
        headers.insert(
            reqwest::header::CONTENT_DISPOSITION,
            reqwest::header::HeaderValue::from_static("attachment; filename=fallback.yaml"),
        );
        assert_eq!(
            parse_profile_title(&headers).as_deref(),
            Some("fallback.yaml")
        );
    }

    #[test]
    fn subscription_expire_ignores_absurd_overflow_values() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "subscription-userinfo",
            reqwest::header::HeaderValue::from_static(
                "upload=1; download=2; total=3; expire=18446744073709551615",
            ),
        );
        let parsed = parse_subscription_userinfo(&headers);
        assert_eq!(parsed.upload, Some(1));
        assert!(parsed.expire.is_none());
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
    async fn fetch_sends_default_user_agent_and_hwid() {
        let seen = Arc::new(Mutex::new(None::<(String, bool)>));
        let seen_request = Arc::clone(&seen);
        let router = Router::new().route(
            "/",
            get(move |headers: AxumHeaderMap| {
                let seen_request = Arc::clone(&seen_request);
                async move {
                    let ua = headers
                        .get(header::USER_AGENT)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_owned();
                    let has_hwid = headers
                        .get("x-hwid")
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|value| !value.trim().is_empty());
                    *seen_request.lock().unwrap() = Some((ua, has_hwid));
                    "ok: true\n"
                }
            }),
        );
        let (url, _server) = serve(router).await;
        let (_temp, service) = service();
        service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();

        let (ua, has_hwid) = seen.lock().unwrap().clone().unwrap();
        assert_eq!(
            ua,
            format!("clash-nyanpasu/v{}", crate::utils::dirs::APP_VERSION)
        );
        assert!(has_hwid);
    }

    #[tokio::test]
    async fn fetch_falls_back_to_direct_when_self_proxy_port_is_unavailable() {
        let hits = Arc::new(AtomicUsize::new(0));
        let source_hits = Arc::new(AtomicUsize::new(0));
        let request_hits = Arc::clone(&hits);
        let router = Router::new().route(
            "/",
            get(move || {
                let request_hits = Arc::clone(&request_hits);
                async move {
                    request_hits.fetch_add(1, Ordering::SeqCst);
                    "ok: true\n"
                }
            }),
        );
        let (url, _server) = serve(router).await;
        let (_temp, service) = service_with(Arc::new(CountingNoProxy {
            hits: Arc::clone(&source_hits),
        }));

        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options(true, false))
            .await
            .unwrap();

        assert_eq!(fetched.content, "ok: true\n");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(source_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fetch_continues_proxy_chain_when_self_proxy_port_is_unavailable() {
        let source_hits = Arc::new(AtomicUsize::new(0));
        let router = Router::new().route("/", get(|| async { "ok: true\n" }));
        let (url, _server) = serve(router).await;
        let (_temp, service) = service_with(Arc::new(CountingNoProxy {
            hits: Arc::clone(&source_hits),
        }));

        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options(true, true))
            .await
            .unwrap();

        assert_eq!(fetched.content, "ok: true\n");
        assert_eq!(source_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fetch_retries_transient_errors_but_not_auth_failures() {
        let hits = Arc::new(AtomicUsize::new(0));
        let flaky_hits = Arc::clone(&hits);
        let flaky = Router::new().route(
            "/",
            get(move || {
                let flaky_hits = Arc::clone(&flaky_hits);
                async move {
                    if flaky_hits.fetch_add(1, Ordering::SeqCst) == 0 {
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    } else {
                        "ok: true\n".into_response()
                    }
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
        assert!(hits.load(Ordering::SeqCst) >= 2);

        for status in [
            StatusCode::FORBIDDEN,
            StatusCode::UNAUTHORIZED,
            StatusCode::NOT_FOUND,
        ] {
            let hits = Arc::new(AtomicUsize::new(0));
            let status_hits = Arc::clone(&hits);
            let router = Router::new().route(
                "/",
                get(move || {
                    let status_hits = Arc::clone(&status_hits);
                    async move {
                        status_hits.fetch_add(1, Ordering::SeqCst);
                        status
                    }
                }),
            );
            let (url, _server) = serve(router).await;
            assert!(
                service
                    .fetch(&Url::parse(&url).unwrap(), &options_direct())
                    .await
                    .is_err()
            );
            assert_eq!(hits.load(Ordering::SeqCst), 1);
        }
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
