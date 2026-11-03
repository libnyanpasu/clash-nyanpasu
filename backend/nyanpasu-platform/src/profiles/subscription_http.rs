//! Subscription HTTP fetching with injected proxy state and bundle metadata.

use std::{sync::Arc, time::Duration};

use nyanpasu_config::profile::RemoteProfileOptions;
use nyanpasu_contracts::{
    BoxedCause,
    bundle_metadata::BundleMetadata,
    net::SelfProxyPortSource,
    profiles::{
        error::{
            BuildHttpClientSnafu, ReadSubscriptionBodySnafu, RequestSubscriptionSnafu,
            SubscriptionFetchError, SubscriptionHttpStatusSnafu,
        },
        ports::{FetchedSubscription, SubscriptionFetcher},
    },
};
use snafu::{ResultExt, ensure};
use url::Url;

use super::subscription;

/// Fetches subscriptions using the current proxy port and the host's bundle metadata.
pub struct SubscriptionHttpFetcher {
    self_proxy_port: Arc<dyn SelfProxyPortSource>,
    http_timeout: Duration,
    bundle_metadata: BundleMetadata,
}

impl SubscriptionHttpFetcher {
    /// Resolves the self-proxy port on each request and uses the existing HTTP timeout.
    pub fn new(
        self_proxy_port: Arc<dyn SelfProxyPortSource>,
        bundle_metadata: BundleMetadata,
    ) -> Self {
        Self {
            self_proxy_port,
            http_timeout: Duration::from_secs(30),
            bundle_metadata,
        }
    }

    #[cfg(test)]
    fn with_http_timeout(mut self, timeout: Duration) -> Self {
        self.http_timeout = timeout;
        self
    }
}

#[async_trait::async_trait]
impl SubscriptionFetcher for SubscriptionHttpFetcher {
    async fn fetch(
        &self,
        url: &Url,
        options: &RemoteProfileOptions,
    ) -> Result<FetchedSubscription, SubscriptionFetchError> {
        use backon::Retryable;

        let mut builder = reqwest::ClientBuilder::new()
            .tls_backend_rustls()
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
            use nyanpasu_core::utils::config::NyanpasuReqwestProxyExt;
            builder = builder.swift_set_proxy(&proxy_url);
        }

        let user_agent = options
            .user_agent
            .clone()
            .unwrap_or_else(|| format!("clash-nyanpasu/v{}", self.bundle_metadata.app_version));
        let client = builder
            .user_agent(user_agent)
            .build()
            .map_err(BoxedCause::from)
            .context(BuildHttpClientSnafu)?;

        let device_info = nyanpasu_core::utils::hwid::get_device_info();
        let sanitize = nyanpasu_core::utils::hwid::sanitize_for_header;
        let perform = || async {
            let resp = client
                .get(url.as_str())
                .header("x-hwid", &device_info.hwid)
                .header("x-device-os", sanitize(&device_info.device_os))
                .header("x-ver-os", sanitize(&device_info.os_version))
                .header("x-device-model", sanitize(&device_info.device_model))
                .send()
                .await
                .map_err(BoxedCause::from)
                .context(RequestSubscriptionSnafu)?;
            let status = resp.status();
            ensure!(
                !(status.is_client_error() || status.is_server_error()),
                SubscriptionHttpStatusSnafu {
                    status: status.as_u16()
                }
            );
            Ok(resp)
        };
        let resp = perform
            .retry(backon::ExponentialBuilder::default())
            .when(|error: &SubscriptionFetchError| {
                !matches!(
                    error,
                    SubscriptionFetchError::SubscriptionHttpStatus {
                        status: 401 | 403 | 404
                    }
                )
            })
            .await?;

        let subscription = subscription::parse_subscription_userinfo(resp.headers());
        let filename = subscription::parse_profile_title(resp.headers());
        let suggested_update_interval_minutes =
            subscription::parse_suggested_update_interval(resp.headers());
        let content_type = subscription::parse_media_type(resp.headers());
        let file_name = subscription::parse_filename_from_content_disposition(resp.headers());
        let content = resp
            .text_with_charset("utf-8")
            .await
            .map_err(BoxedCause::from)
            .context(ReadSubscriptionBodySnafu)?;
        let content = if let Some(content) = content.strip_prefix('\u{feff}') {
            content.to_owned()
        } else {
            content
        };
        Ok(FetchedSubscription {
            content,
            filename,
            subscription,
            suggested_update_interval_minutes,
            content_type,
            file_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        http::{HeaderMap as AxumHeaderMap, StatusCode, header},
        response::IntoResponse,
        routing::get,
    };
    use nyanpasu_config::profile::RemoteSyncInterval;
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

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

    fn service() -> SubscriptionHttpFetcher {
        service_with(Arc::new(NoProxy))
    }

    fn service_with(self_proxy_port: Arc<dyn SelfProxyPortSource>) -> SubscriptionHttpFetcher {
        SubscriptionHttpFetcher::new(
            self_proxy_port,
            nyanpasu_contracts::bundle_metadata::TEST_BUNDLE_METADATA,
        )
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
            update_interval_minutes: RemoteSyncInterval::from_minutes(120),
        }
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
                headers.insert("profile-update-interval", "6".parse().unwrap());
                headers.insert(
                    header::CONTENT_TYPE,
                    "text/yaml; charset=utf-8".parse().unwrap(),
                );
                headers.insert(
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=sub.yaml".parse().unwrap(),
                );
                (headers, "proxies: []\n")
            }),
        );
        let (url, _server) = serve(router).await;
        let service = service();
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
        assert_eq!(fetched.suggested_update_interval_minutes, Some(360));
        assert_eq!(fetched.content_type.as_deref(), Some("text/yaml"));
        assert_eq!(fetched.file_name.as_deref(), Some("sub.yaml"));
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
        let service = service();
        service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();

        let (ua, has_hwid) = seen.lock().unwrap().clone().unwrap();
        assert_eq!(
            ua,
            format!("clash-nyanpasu/v{}", service.bundle_metadata.app_version)
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
        let service = service_with(Arc::new(CountingNoProxy {
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
        let service = service_with(Arc::new(CountingNoProxy {
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
        let service = service();
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
        let service = service();
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

#[cfg(test)]
mod error_contract_tests;
