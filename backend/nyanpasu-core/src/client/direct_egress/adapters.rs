use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use nyanpasu_config::application::StunServerCapabilities;
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpStream, UdpSocket},
    time::Instant,
};

use super::{
    ports::{DirectEgressProbe, ProbeSources, StunAttempt, StunServerTest},
    stun::{self, HEADER_LEN, TransactionId},
};

/// A STUN server that does not resolve and answer within this is skipped, so a long
/// server list stays usable when several are unreachable.
const STUN_DEADLINE: Duration = Duration::from_millis(2500);
/// Delays before each UDP retransmission (RFC 8489 starts at 500 ms and
/// doubles), cut short by [`STUN_DEADLINE`].
const UDP_RETRANSMISSIONS: [Duration; 2] =
    [Duration::from_millis(500), Duration::from_millis(1000)];
/// A family without a route ends here rather than at the OS connect timeout.
const ECHO_TIMEOUT: Duration = Duration::from_secs(5);
/// Large enough for any Binding response a server sends.
const MAX_MESSAGE_LEN: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Ipv4,
    Ipv6,
}

impl Family {
    fn matches(self, ip: IpAddr) -> bool {
        matches!(
            (self, ip),
            (Self::Ipv4, IpAddr::V4(_)) | (Self::Ipv6, IpAddr::V6(_))
        )
    }

    fn unspecified(self) -> SocketAddr {
        match self {
            Self::Ipv4 => (Ipv4Addr::UNSPECIFIED, 0).into(),
            Self::Ipv6 => (Ipv6Addr::UNSPECIFIED, 0).into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    Udp,
    Tcp,
}

/// UDP when the server answers over it, since that is what STUN is built for.
fn transport_for(capabilities: StunServerCapabilities, family: Family) -> Option<Transport> {
    let supports = |families: nyanpasu_config::application::StunFamilies| match family {
        Family::Ipv4 => families.ipv4,
        Family::Ipv6 => families.ipv6,
    };
    if supports(capabilities.udp) {
        Some(Transport::Udp)
    } else if supports(capabilities.tcp) {
        Some(Transport::Tcp)
    } else {
        None
    }
}

/// Asks STUN servers one at a time, so only the operators needed for an
/// answer learn the address, then the accepted HTTP lookup providers.
pub struct NetworkDirectEgressProbe {
    /// The addresses a STUN server may be asked at.
    destinations: fn(IpAddr) -> bool,
}

impl Default for NetworkDirectEgressProbe {
    fn default() -> Self {
        Self {
            destinations: is_public,
        }
    }
}

/// A server on this machine or the local network sees a local address, never
/// the public one, and asking one would let whoever edits the server list
/// reach those hosts. Fake-IP answers under TUN are left to the core's rules.
fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast())
        }
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(ip) => is_public(ip.into()),
            None => {
                !(ip.is_loopback()
                    || ip.is_unspecified()
                    || ip.is_multicast()
                    || ip.is_unique_local()
                    || ip.is_unicast_link_local())
            }
        },
    }
}

impl NetworkDirectEgressProbe {
    async fn probe(&self, sources: &ProbeSources, family: Family) -> Option<IpAddr> {
        for target in &sources.stun {
            let Some(transport) = transport_for(target.capabilities, family) else {
                continue;
            };
            if !sources.permission.allows_stun(target) {
                continue;
            }
            if let StunAttempt::Answered { address, .. } = ask_stun(
                &target.host,
                target.port,
                transport,
                family,
                self.destinations,
            )
            .await
                && family.matches(address)
            {
                return Some(address);
            }
        }
        for provider in &sources.lookups {
            if !sources.permission.allows_lookup(*provider) {
                continue;
            }
            let url = match family {
                Family::Ipv4 => provider.ipv4_url(),
                Family::Ipv6 => provider.ipv6_url(),
            };
            if let Some(address) = echo(url).await.filter(|address| family.matches(*address)) {
                return Some(address);
            }
            tracing::debug!(?provider, ?family, "the IP lookup provider gave no address");
        }
        None
    }
}

#[async_trait::async_trait]
impl DirectEgressProbe for NetworkDirectEgressProbe {
    async fn ipv4(&self, sources: &ProbeSources) -> Option<Ipv4Addr> {
        match self.probe(sources, Family::Ipv4).await? {
            IpAddr::V4(address) => Some(address),
            IpAddr::V6(_) => None,
        }
    }

    async fn ipv6(&self, sources: &ProbeSources) -> Option<Ipv6Addr> {
        match self.probe(sources, Family::Ipv6).await? {
            IpAddr::V6(address) => Some(address),
            IpAddr::V4(_) => None,
        }
    }

    async fn test_stun_server(&self, host: &str, port: u16) -> StunServerTest {
        let (udp_ipv4, udp_ipv6, tcp_ipv4, tcp_ipv6) = tokio::join!(
            ask_stun(host, port, Transport::Udp, Family::Ipv4, self.destinations),
            ask_stun(host, port, Transport::Udp, Family::Ipv6, self.destinations),
            ask_stun(host, port, Transport::Tcp, Family::Ipv4, self.destinations),
            ask_stun(host, port, Transport::Tcp, Family::Ipv6, self.destinations),
        );
        StunServerTest::new(udp_ipv4, udp_ipv6, tcp_ipv4, tcp_ipv6)
    }
}

/// One Binding request to the first address of `family` that `host` resolves
/// to among the `destinations`. The socket is a plain OS socket, so no proxy
/// setting applies.
async fn ask_stun(
    host: &str,
    port: u16,
    transport: Transport,
    family: Family,
    destinations: fn(IpAddr) -> bool,
) -> StunAttempt {
    // Resolution counts against the deadline, so a resolver that never
    // answers cannot hold up the next server.
    let deadline = Instant::now() + STUN_DEADLINE;
    let resolved = tokio::time::timeout_at(deadline, tokio::net::lookup_host((host, port))).await;
    let server = match resolved {
        Ok(Ok(mut addresses)) => {
            addresses.find(|address| family.matches(address.ip()) && destinations(address.ip()))
        }
        Ok(Err(error)) => {
            tracing::debug!(host, %error, "the STUN server did not resolve");
            None
        }
        Err(_) => {
            tracing::debug!(host, "the STUN server did not resolve in time");
            None
        }
    };
    let Some(server) = server else {
        return StunAttempt::NoAddress;
    };
    let id = match TransactionId::random() {
        Ok(id) => id,
        Err(error) => {
            tracing::warn!(%error, "no randomness for a STUN transaction ID");
            return StunAttempt::NoAnswer;
        }
    };
    let started = Instant::now();
    let exchange = async {
        match transport {
            Transport::Udp => exchange_udp(server, family, id).await,
            Transport::Tcp => exchange_tcp(server, id).await,
        }
    };
    match tokio::time::timeout_at(deadline, exchange).await {
        Ok(Ok(mapped)) => StunAttempt::Answered {
            address: mapped.ip(),
            latency_ms: started.elapsed().as_millis() as u64,
        },
        Ok(Err(error)) => {
            tracing::debug!(%server, ?transport, %error, "the STUN exchange failed");
            StunAttempt::NoAnswer
        }
        Err(_) => StunAttempt::NoAnswer,
    }
}

#[derive(Debug, snafu::Snafu)]
enum ExchangeError {
    #[snafu(context(false), display("socket error"))]
    Io { source: std::io::Error },
    #[snafu(context(false), display("the response is malformed"))]
    Decode { source: stun::DecodeError },
}

/// Sends the request again after each retransmission delay until a response
/// to it arrives. Datagrams that answer something else are ignored.
async fn exchange_udp(
    server: SocketAddr,
    family: Family,
    id: TransactionId,
) -> Result<SocketAddr, ExchangeError> {
    let socket = UdpSocket::bind(family.unspecified()).await?;
    socket.connect(server).await?;
    let request = stun::binding_request(id);
    let mut buffer = [0; MAX_MESSAGE_LEN];
    let mut delays = UDP_RETRANSMISSIONS.into_iter();
    loop {
        socket.send(&request).await?;
        let wait = delays.next();
        let resend_at = wait.map(|delay| Instant::now() + delay);
        loop {
            let received = match resend_at {
                Some(at) => match tokio::time::timeout_at(at, socket.recv(&mut buffer)).await {
                    Ok(received) => received?,
                    Err(_) => break,
                },
                // The last request waits until the caller's deadline.
                None => socket.recv(&mut buffer).await?,
            };
            if let Ok(mapped) = stun::mapped_address(&buffer[..received], id) {
                return Ok(mapped);
            }
        }
    }
}

/// TCP carries one message after another, so the header's length says where
/// the response ends.
async fn exchange_tcp(server: SocketAddr, id: TransactionId) -> Result<SocketAddr, ExchangeError> {
    let mut stream = TcpStream::connect(server).await?;
    stream.write_all(&stun::binding_request(id)).await?;
    let mut header = [0; HEADER_LEN];
    stream.read_exact(&mut header).await?;
    let mut message = header.to_vec();
    message.resize(HEADER_LEN + stun::body_len(&header), 0);
    stream.read_exact(&mut message[HEADER_LEN..]).await?;
    Ok(stun::mapped_address(&message, id)?)
}

/// The address a plain-text echo answers with. `None` for anything else.
async fn echo(url: &str) -> Option<IpAddr> {
    match fetch(url).await {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{Router, http::StatusCode, routing::get};
    use nyanpasu_config::application::{IpLookupProvider, StunFamilies, StunServer};
    use tokio::net::TcpListener;

    use super::*;
    use crate::client::direct_egress::{ProbePermission, stun::tests::binding_response};

    const LOCALHOST: &str = "127.0.0.1";
    /// The servers these tests run listen on loopback.
    const ANYWHERE: fn(IpAddr) -> bool = |_| true;
    const LOOPBACK_PROBE: NetworkDirectEgressProbe = NetworkDirectEgressProbe {
        destinations: ANYWHERE,
    };

    async fn serve(router: Router) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        url
    }

    async fn answering(body: &'static str) -> String {
        serve(Router::new().route("/", get(move || async move { body }))).await
    }

    /// Answers every request with the address it came from, after ignoring
    /// the first `drop` datagrams, the way a lossy path would.
    async fn udp_stun_server(drop: usize) -> u16 {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = socket.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut buffer = [0; MAX_MESSAGE_LEN];
            let mut dropped = 0;
            loop {
                let (len, from) = socket.recv_from(&mut buffer).await.unwrap();
                if dropped < drop {
                    dropped += 1;
                    continue;
                }
                let response = binding_response(&buffer[..len], from);
                socket.send_to(&response, from).await.unwrap();
            }
        });
        port
    }

    async fn tcp_stun_server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut stream, from) = listener.accept().await.unwrap();
                let mut request = [0; HEADER_LEN];
                stream.read_exact(&mut request).await.unwrap();
                let response = binding_response(&request, from);
                // Split the response to check that the client reads it whole.
                let (first, rest) = response.split_at(10);
                stream.write_all(first).await.unwrap();
                stream.flush().await.unwrap();
                stream.write_all(rest).await.unwrap();
            }
        });
        port
    }

    fn target(port: u16, udp: bool, tcp: bool) -> StunServer {
        StunServer {
            host: LOCALHOST.into(),
            port,
            enabled: true,
            builtin: false,
            capabilities: StunServerCapabilities {
                udp: StunFamilies {
                    ipv4: udp,
                    ipv6: false,
                },
                tcp: StunFamilies {
                    ipv4: tcp,
                    ipv6: false,
                },
            },
        }
    }

    /// Allows the servers on these ports, and no lookup provider.
    struct AllowingPorts(Vec<u16>);

    impl ProbePermission for AllowingPorts {
        fn allows_stun(&self, server: &StunServer) -> bool {
            self.0.contains(&server.port)
        }
        fn allows_lookup(&self, _: IpLookupProvider) -> bool {
            false
        }
    }

    fn sources(stun: Vec<StunServer>, permission: AllowingPorts) -> ProbeSources {
        ProbeSources {
            stun,
            lookups: Vec::new(),
            permission: Arc::new(permission),
        }
    }

    fn closed_port() -> u16 {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        socket.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn a_udp_server_names_the_address_the_request_came_from() {
        let port = udp_stun_server(0).await;

        let attempt = ask_stun(LOCALHOST, port, Transport::Udp, Family::Ipv4, ANYWHERE).await;

        assert!(matches!(
            attempt,
            StunAttempt::Answered { address, .. } if address == IpAddr::from(Ipv4Addr::LOCALHOST)
        ));
    }

    #[tokio::test]
    async fn a_lost_udp_request_is_sent_again() {
        let port = udp_stun_server(1).await;

        let attempt = ask_stun(LOCALHOST, port, Transport::Udp, Family::Ipv4, ANYWHERE).await;

        assert!(
            matches!(attempt, StunAttempt::Answered { .. }),
            "{attempt:?}"
        );
    }

    #[tokio::test]
    async fn a_tcp_server_names_the_address_the_request_came_from() {
        let port = tcp_stun_server().await;

        let attempt = ask_stun(LOCALHOST, port, Transport::Tcp, Family::Ipv4, ANYWHERE).await;

        assert!(matches!(
            attempt,
            StunAttempt::Answered { address, .. } if address == IpAddr::from(Ipv4Addr::LOCALHOST)
        ));
    }

    #[tokio::test]
    async fn a_host_without_an_address_of_the_family_is_not_asked() {
        let attempt = ask_stun(LOCALHOST, 3478, Transport::Udp, Family::Ipv6, ANYWHERE).await;

        assert_eq!(attempt, StunAttempt::NoAddress);
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_udp_server_is_no_answer() {
        let attempt = ask_stun(
            LOCALHOST,
            closed_port(),
            Transport::Udp,
            Family::Ipv4,
            ANYWHERE,
        )
        .await;

        assert_eq!(attempt, StunAttempt::NoAnswer);
    }

    #[tokio::test]
    async fn the_probe_skips_servers_that_do_not_answer_and_uses_tcp_only_where_needed() {
        let tcp = tcp_stun_server().await;
        let closed = closed_port();
        let sources = sources(
            vec![target(closed, true, false), target(tcp, false, true)],
            AllowingPorts(vec![closed, tcp]),
        );

        assert_eq!(
            LOOPBACK_PROBE.ipv4(&sources).await,
            Some(Ipv4Addr::LOCALHOST)
        );
        assert_eq!(LOOPBACK_PROBE.ipv6(&sources).await, None);
    }

    #[tokio::test]
    async fn a_server_the_permission_no_longer_allows_is_not_asked() {
        let udp = udp_stun_server(0).await;
        let sources = sources(vec![target(udp, true, false)], AllowingPorts(Vec::new()));

        assert_eq!(LOOPBACK_PROBE.ipv4(&sources).await, None);
    }

    #[tokio::test]
    async fn a_server_test_reports_each_transport_and_family() {
        let udp = udp_stun_server(0).await;

        let test = LOOPBACK_PROBE.test_stun_server(LOCALHOST, udp).await;

        assert!(matches!(test.udp_ipv4, StunAttempt::Answered { .. }));
        assert_eq!(test.udp_ipv6, StunAttempt::NoAddress);
        assert_eq!(test.tcp_ipv6, StunAttempt::NoAddress);
        assert_eq!(
            test.capabilities,
            StunServerCapabilities {
                udp: StunFamilies {
                    ipv4: true,
                    ipv6: false
                },
                tcp: StunFamilies::default(),
            }
        );
    }

    #[tokio::test]
    async fn servers_on_this_machine_or_the_local_network_are_not_asked() {
        let udp = udp_stun_server(0).await;

        let test = NetworkDirectEgressProbe::default()
            .test_stun_server(LOCALHOST, udp)
            .await;

        assert_eq!(test.udp_ipv4, StunAttempt::NoAddress);
        for local in [
            "127.0.0.1",
            "10.0.0.1",
            "192.168.1.1",
            "169.254.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
        ] {
            assert!(!is_public(local.parse().unwrap()), "{local}");
        }
        for public in ["203.0.113.7", "198.18.0.1", "2001:db8::7"] {
            assert!(is_public(public.parse().unwrap()), "{public}");
        }
    }

    #[tokio::test]
    async fn an_echo_reads_the_address_it_answers() {
        assert_eq!(
            echo(&answering("203.0.113.7\n").await).await,
            Some(Ipv4Addr::new(203, 0, 113, 7).into())
        );
        assert_eq!(
            echo(&answering("2001:db8::7").await).await,
            Some("2001:db8::7".parse().unwrap())
        );
    }

    #[tokio::test]
    async fn a_page_or_an_error_status_is_no_answer() {
        let failing = serve(Router::new().route(
            "/",
            get(|| async { (StatusCode::SERVICE_UNAVAILABLE, "203.0.113.7") }),
        ))
        .await;

        assert_eq!(echo(&answering("<html>sign in</html>").await).await, None);
        assert_eq!(echo(&failing).await, None);
    }

    #[tokio::test]
    async fn an_unreachable_echo_is_no_answer() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed = format!("http://{}/", listener.local_addr().unwrap());
        drop(listener);

        assert_eq!(echo(&closed).await, None);
    }

    #[test]
    fn every_provider_has_an_endpoint_per_family() {
        for provider in IpLookupProvider::ALL {
            assert!(provider.ipv4_url().starts_with("https://"), "{provider:?}");
            assert!(provider.ipv6_url().starts_with("https://"), "{provider:?}");
        }
    }
}
