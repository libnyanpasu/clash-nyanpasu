//! Direct-route HTTP echo adapter, isolated from system and self proxies.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    time::Duration,
};

use nyanpasu_contracts::net::DirectEgressProbe;

/// Each host resolves in one address family only, so a request to it leaves
/// over that family.
const DNSPOD_IPV4: &str = "https://ipv4.ddnspod.com";
const DNSPOD_IPV6: &str = "https://ipv6.ddnspod.com";

/// A family without a route ends here rather than at the OS connect timeout.
const ECHO_TIMEOUT: Duration = Duration::from_secs(5);

/// Probes each address family directly, bypassing system and application proxies.
pub struct HttpDirectEgressProbe {
    ipv4: String,
    ipv6: String,
}

impl HttpDirectEgressProbe {
    /// Uses the DNSPod family-specific echo endpoints with the existing five-second timeout.
    pub fn dnspod() -> Self {
        Self::new(DNSPOD_IPV4.into(), DNSPOD_IPV6.into())
    }

    fn new(ipv4: String, ipv6: String) -> Self {
        Self { ipv4, ipv6 }
    }

    async fn echo(url: &str) -> Option<IpAddr> {
        match Self::fetch(url).await {
            Ok(body) => {
                let address = body.trim().parse().ok();
                if address.is_none() {
                    tracing::debug!(
                        url,
                        "the echo service answered something other than an address"
                    );
                }
                address
            }
            Err(error) => {
                tracing::debug!(url, %error, "the echo service could not be reached");
                None
            }
        }
    }

    async fn fetch(url: &str) -> reqwest::Result<String> {
        reqwest::Client::builder()
            // The system proxy and the core's ports would both route the request by rule.
            .no_proxy()
            .timeout(ECHO_TIMEOUT)
            .build()?
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await
    }
}

#[async_trait::async_trait]
impl DirectEgressProbe for HttpDirectEgressProbe {
    async fn ipv4(&self) -> Option<Ipv4Addr> {
        match Self::echo(&self.ipv4).await? {
            IpAddr::V4(address) => Some(address),
            IpAddr::V6(_) => None,
        }
    }

    async fn ipv6(&self) -> Option<Ipv6Addr> {
        match Self::echo(&self.ipv6).await? {
            IpAddr::V6(address) => Some(address),
            IpAddr::V4(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{Router, http::StatusCode, routing::get};

    use super::*;

    async fn serve(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        url
    }

    async fn answering(body: &'static str) -> String {
        serve(Router::new().route("/", get(move || async move { body }))).await
    }

    #[tokio::test]
    async fn each_family_reads_the_address_its_echo_answers() {
        let probe = HttpDirectEgressProbe::new(
            answering("203.0.113.7\n").await,
            answering("2001:db8::7").await,
        );

        assert_eq!(probe.ipv4().await, Some(Ipv4Addr::new(203, 0, 113, 7)));
        assert_eq!(probe.ipv6().await, Some("2001:db8::7".parse().unwrap()));
    }

    #[tokio::test]
    async fn an_address_of_the_other_family_is_no_answer() {
        let probe = HttpDirectEgressProbe::new(
            answering("2001:db8::7").await,
            answering("203.0.113.7").await,
        );

        assert_eq!(probe.ipv4().await, None);
        assert_eq!(probe.ipv6().await, None);
    }

    #[tokio::test]
    async fn a_page_or_an_error_status_is_no_answer() {
        let failing = serve(Router::new().route(
            "/",
            get(|| async { (StatusCode::SERVICE_UNAVAILABLE, "203.0.113.7") }),
        ))
        .await;
        let probe = HttpDirectEgressProbe::new(answering("<html>sign in</html>").await, failing);

        assert_eq!(probe.ipv4().await, None);
        assert_eq!(probe.ipv6().await, None);
    }

    #[tokio::test]
    async fn an_unreachable_echo_is_no_answer() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed = format!("http://{}/", listener.local_addr().unwrap());
        drop(listener);
        let probe = HttpDirectEgressProbe::new(closed.clone(), closed);

        assert_eq!(probe.ipv4().await, None);
    }
}
