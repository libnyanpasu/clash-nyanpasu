pub mod proxy_env;

use anyhow::Result;
use std::time::Duration;
use sysproxy::Sysproxy;
use url::Url;

/// The current confirmed mixed port, or no port when no instance is running.
/// Consumers sample it when starting an HTTP operation, not during construction.
#[cfg_attr(test, mockall::automock)]
pub trait SelfProxyPortSource: Send + Sync + 'static {
    fn mixed_port(&self) -> Option<u16>;
}

/// The app's own mixed port as a proxy URL. Callers pass the port the facade
/// reports (`NyanpasuClient::clash_info`): the confirmed session binding, else
/// the configured start port.
pub fn get_self_proxy(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

pub fn get_system_proxy() -> Result<Option<String>> {
    let p = Sysproxy::get_system_proxy()?;
    if p.enable {
        let proxy_scheme = format!("http://{}:{}", p.host, p.port);
        return Ok(Some(proxy_scheme));
    }

    Ok(None)
}

pub trait NyanpasuReqwestProxyExt {
    fn swift_set_proxy(self, url: &str) -> Self;

    fn swift_set_nyanpasu_proxy(self, self_proxy_port: u16) -> Self;
}

impl NyanpasuReqwestProxyExt for reqwest::ClientBuilder {
    fn swift_set_proxy(self, url: &str) -> Self {
        let mut builder = self;
        if let Ok(proxy) = reqwest::Proxy::http(url) {
            builder = builder.proxy(proxy);
        }
        if let Ok(proxy) = reqwest::Proxy::https(url) {
            builder = builder.proxy(proxy);
        }
        if let Ok(proxy) = reqwest::Proxy::all(url) {
            builder = builder.proxy(proxy);
        }
        builder
    }

    // TODO: 修改成按枚举配置
    fn swift_set_nyanpasu_proxy(self, self_proxy_port: u16) -> Self {
        let mut builder = self.swift_set_proxy(&get_self_proxy(self_proxy_port));
        if let Ok(Some(proxy)) = get_system_proxy() {
            builder = builder.swift_set_proxy(&proxy);
        }
        builder
    }
}

pub fn get_reqwest_client(self_proxy_port: u16, user_agent: &str) -> Result<reqwest::Client> {
    let builder = reqwest::ClientBuilder::new();
    let client = builder
        .swift_set_nyanpasu_proxy(self_proxy_port)
        .user_agent(user_agent)
        .build()?;
    Ok(client)
}

pub const INTERNAL_MIRRORS: &[&str] = &[
    "https://github.com/",
    "https://nyanpasu-script.majokeiko.com/",
    // too many restrictions, not recommended
    // "https://gh.idayer.com/",
];

pub fn parse_gh_url(mirror: &str, path: &str) -> Result<Url, url::ParseError> {
    if mirror.contains("github.com") && !path.starts_with('/') {
        Url::parse(path)
    } else {
        let mut url = Url::parse(mirror)?;
        url.set_path(path);
        Ok(url)
    }
}

#[async_trait::async_trait]
pub trait ReqwestSpeedTestExt {
    async fn mirror_speed_test<'a>(
        &self,
        mirrors: &'a [&'a str],
        path: &'a str,
    ) -> Result<Vec<(&'a str, f64)>>;
}

#[async_trait::async_trait]
impl ReqwestSpeedTestExt for reqwest::Client {
    async fn mirror_speed_test<'a>(
        &self,
        mirrors: &'a [&'a str],
        path: &'a str,
    ) -> Result<Vec<(&'a str, f64)>> {
        let results = futures::future::join_all(mirrors.iter().map(|&mirror| {
            let client = self;
            async move {
                let start = tokio::time::Instant::now();
                // if mirror is github.com, we should use it directly
                let url = parse_gh_url(mirror, path)?;
                tracing::debug!("Testing {}", url.as_str());
                let _ =
                    tokio::time::timeout(Duration::from_secs(3), client.get(url.as_str()).send())
                        .await; // warm up
                let result: Result<reqwest::Response, anyhow::Error> =
                    tokio::time::timeout(Duration::from_secs(3), client.get(url).send())
                        .await
                        .map_err(anyhow::Error::msg)
                        .and_then(|v| v.map_err(anyhow::Error::msg))
                        .and_then(|v| v.error_for_status().map_err(anyhow::Error::msg));
                match result {
                    Ok(response) => {
                        let content_length = response.content_length().unwrap_or(0) as f64;
                        // should read all the response body to get the correct speed
                        match response.bytes().await {
                            Ok(_) => {
                                let elapsed = start.elapsed().as_secs_f64();
                                let speed = content_length / elapsed;
                                Ok((mirror, speed))
                            }
                            Err(e) => {
                                tracing::warn!("test mirror {} failed: {}", mirror, e);
                                Ok((mirror, 0.0))
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("test mirror {} failed: {}", mirror, e);
                        Ok((mirror, 0.0))
                    }
                }
            }
        }))
        .await;
        let collected_result: Result<Vec<_>, anyhow::Error> = results.into_iter().collect();
        let mut results = collected_result?;
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        Ok(results)
    }
}

#[cfg(test)]
mod test {
    #[allow(unused_imports)]
    use super::*;

    #[tokio::test]
    #[allow(clippy::needless_return)] // a bug in clippy
    async fn test_mirror_speed_test() {
        let client = reqwest::Client::builder().user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36"
        ).build().unwrap();
        let results = client
            .mirror_speed_test(
                INTERNAL_MIRRORS,
                "https://raw.githubusercontent.com/simonw/github-large-file-test/master/1.5mb.txt",
            )
            .await
            .unwrap();
        println!("{results:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_proxy_url_preserves_loopback_and_port() {
        assert_eq!(get_self_proxy(7890), "http://127.0.0.1:7890");
    }

    #[test]
    fn github_url_preserves_direct_and_mirrored_paths() {
        let path = "https://github.com/project/release/file";
        assert_eq!(
            parse_gh_url(INTERNAL_MIRRORS[0], path).unwrap().as_str(),
            path
        );
        assert_eq!(
            parse_gh_url(INTERNAL_MIRRORS[0], "/project/release/file")
                .unwrap()
                .as_str(),
            path,
        );
        let mirrored = parse_gh_url("https://mirror.invalid/", path).unwrap();
        assert_eq!(mirrored.host_str(), Some("mirror.invalid"));
        assert_eq!(mirrored.path(), "/https://github.com/project/release/file");
        assert!(parse_gh_url("not a url", "/file").is_err());
    }

    #[test]
    fn reqwest_client_validates_the_explicit_user_agent() {
        assert!(get_reqwest_client(7890, "clash-nyanpasu/test-product-version").is_ok());
        assert!(get_reqwest_client(7890, "invalid\nuser-agent").is_err());
    }

    #[tokio::test]
    async fn explicit_proxy_sends_the_user_agent_to_a_local_listener() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = reqwest::Client::builder()
            .no_proxy()
            .swift_set_proxy(&get_self_proxy(listener.local_addr().unwrap().port()))
            .user_agent("clash-nyanpasu/test-product-version")
            .build()
            .unwrap();
        let request = client.get("http://127.0.0.1:1/artifact").send();
        let response = async {
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
            assert!(request.starts_with("GET http://127.0.0.1:1/artifact HTTP/1.1"));
            assert!(request.contains("user-agent: clash-nyanpasu/test-product-version\r\n"));
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        };
        let (request, ()) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(request, response)
        })
        .await
        .unwrap();
        assert_eq!(request.unwrap().status(), reqwest::StatusCode::OK);
    }
}
