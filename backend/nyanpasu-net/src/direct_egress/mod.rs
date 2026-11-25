//! Direct-route egress probe, isolated from system and self proxies: STUN
//! servers first (UDP phase, then TCP fallback phase with staggered hedging),
//! then the accepted HTTP lookup services. The STUN codec is pure.
mod stun;

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use nyanpasu_config::application::{StunServer, StunServerCapabilities};
use nyanpasu_contracts::net::{
    DirectEgressProbe, EgressAnswer, EgressSource, ProbePermission, ProbeSources, StunAttempt,
    StunServerTest, StunTransport,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpStream, UdpSocket},
    task::JoinSet,
    time::Instant,
};

use self::stun::{HEADER_LEN, TransactionId};

/// A STUN server that does not resolve and answer within this is skipped, so a long
/// server list stays usable when several are unreachable.
const STUN_DEADLINE: Duration = Duration::from_millis(2500);
/// Delay before hedging the next STUN server in priority order when the current
/// one has not answered yet. Healthy servers reply well within this window, so
/// normal probes still contact only the first operator.
const STUN_HEDGE_DELAY: Duration = Duration::from_millis(400);
/// Maximum STUN servers probed concurrently within a single transport phase.
const MAX_CONCURRENT_STUN: usize = 3;
/// Delays before each UDP retransmission (round-robined across resolved IPs),
/// cut short by [`STUN_DEADLINE`].
const UDP_RETRANSMISSIONS: [Duration; 3] = [
    Duration::from_millis(400),
    Duration::from_millis(800),
    Duration::from_millis(1200),
];
/// Maximum distinct resolved addresses tried per STUN host and address family.
const MAX_RESOLVED_IPS: usize = 3;
/// Upper bound on an on-demand DoH fallback lookup.
const DOH_TIMEOUT: Duration = Duration::from_millis(1500);
/// Minimum per-address TCP connect/exchange slice when a host resolves to several IPs.
const MIN_TCP_ATTEMPT_WINDOW: Duration = Duration::from_millis(800);
/// A family without a route ends here rather than at the OS connect timeout.
const ECHO_TIMEOUT: Duration = Duration::from_secs(5);
/// Large enough for any Binding response a server sends.
const MAX_MESSAGE_LEN: usize = 1024;
/// IP-literal DoH JSON endpoints used only when system DNS returns Fake-IPs
/// (`198.18.0.0/15`), omits the requested family, fails, or returns unreachable
/// (e.g. DNS-poisoned) addresses. Using IP literals avoids depending on system DNS.
const DEFAULT_DOH_ENDPOINTS: [&str; 3] = [
    "https://223.5.5.5/resolve",
    "https://1.1.1.1/dns-query",
    "https://1.12.12.12/resolve",
];

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

    fn dns_record_type(self) -> &'static str {
        match self {
            Self::Ipv4 => "A",
            Self::Ipv6 => "AAAA",
        }
    }

    fn dns_type_code(self) -> u64 {
        match self {
            Self::Ipv4 => 1,
            Self::Ipv6 => 28,
        }
    }
}

fn supports_transport(
    capabilities: StunServerCapabilities,
    transport: StunTransport,
    family: Family,
) -> bool {
    let families = match transport {
        StunTransport::Udp => capabilities.udp,
        StunTransport::Tcp => capabilities.tcp,
    };
    match family {
        Family::Ipv4 => families.ipv4,
        Family::Ipv6 => families.ipv6,
    }
}

fn build_direct_http_client() -> Option<reqwest::Client> {
    // The system proxy and the core's ports would both route requests by rule.
    match reqwest::Client::builder().no_proxy().build() {
        Ok(client) => Some(client),
        Err(error) => {
            tracing::warn!(%error, "failed to construct direct HTTP client for egress probing");
            None
        }
    }
}

/// Shared, cheaply cloneable runtime state for probing tasks.
#[derive(Clone)]
struct ProbeRuntime {
    /// The addresses a STUN server may be asked at (and accepted from).
    destinations: fn(IpAddr) -> bool,
    /// Shared no-proxy HTTP client for DoH fallback and HTTP lookup providers.
    http: Option<reqwest::Client>,
    /// DoH JSON endpoints queried only on fallback when system DNS cannot supply
    /// working real addresses.
    doh_endpoints: Arc<[String]>,
}

/// Probes enabled STUN servers in priority order with staggered hedging (UDP
/// across eligible servers first, then TCP fallback if no UDP server answers),
/// followed by the accepted HTTP lookup providers.
pub struct NetworkDirectEgressProbe {
    runtime: ProbeRuntime,
}

impl Default for NetworkDirectEgressProbe {
    fn default() -> Self {
        Self {
            runtime: ProbeRuntime {
                destinations: is_public,
                http: build_direct_http_client(),
                doh_endpoints: Arc::from(DEFAULT_DOH_ENDPOINTS.map(str::to_owned)),
            },
        }
    }
}

/// RFC 2544 benchmarking range (`198.18.0.0/15`), widely used by Clash/Mihomo
/// and transparent proxy gateways as a Fake-IP pool.
fn is_fake_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            a == 198 && (b & 0xfe) == 18
        }
        IpAddr::V6(ip) => ip.to_ipv4_mapped().is_some_and(|v4| is_fake_ip(v4.into())),
    }
}

/// A server on this machine or the local network sees a local address, never
/// the public one, and asking one would let whoever edits the server list
/// reach those hosts. Fake-IP answers (`198.18.0.0/15`) are handled separately
/// by [`is_fake_ip`] so the resolver bypasses them via DoH before falling back.
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

/// Addresses resolved for one `(host, port, family)` lookup.
#[derive(Debug, Clone)]
struct ResolvedTargets {
    /// Non-fake usable addresses (from IP literal, system DNS, or immediate DoH).
    primary: Vec<SocketAddr>,
    /// Fake-IP addresses (`198.18.0.0/15`) returned by system DNS, retained only
    /// as a last-resort TCP fallback when DoH is unavailable (e.g. `test_stun_server`
    /// under TUN).
    fake: Vec<SocketAddr>,
    /// Whether DoH has not been queried yet and may be tried if `primary` fails
    /// (for example when ISP DNS poisoned a STUN hostname to an unreachable IP).
    can_fallback_doh: bool,
}

impl ProbeRuntime {
    /// Resolves `host` for `family`, bypassing Fake-IP (`198.18.0.0/15`) or
    /// missing system DNS records via on-demand DoH when configured.
    async fn resolve_host(
        &self,
        host: &str,
        port: u16,
        family: Family,
        deadline: Instant,
    ) -> ResolvedTargets {
        if let Ok(ip) = host.parse::<IpAddr>() {
            let mut primary = Vec::new();
            let mut fake = Vec::new();
            if family.matches(ip) && (self.destinations)(ip) {
                let socket_addr = SocketAddr::new(ip, port);
                if is_fake_ip(ip) {
                    fake.push(socket_addr);
                } else {
                    primary.push(socket_addr);
                }
            }
            return ResolvedTargets {
                primary,
                fake,
                can_fallback_doh: false,
            };
        }

        let resolved =
            tokio::time::timeout_at(deadline, tokio::net::lookup_host((host, port))).await;
        let mut primary = Vec::new();
        let mut fake = Vec::new();
        match resolved {
            Ok(Ok(addresses)) => {
                for address in addresses {
                    let ip = address.ip();
                    if !family.matches(ip) || !(self.destinations)(ip) {
                        continue;
                    }
                    let target = SocketAddr::new(ip, port);
                    if is_fake_ip(ip) {
                        if fake.len() < MAX_RESOLVED_IPS && !fake.contains(&target) {
                            fake.push(target);
                        }
                    } else if primary.len() < MAX_RESOLVED_IPS && !primary.contains(&target) {
                        primary.push(target);
                    }
                }
            }
            Ok(Err(error)) => {
                tracing::debug!(host, ?family, %error, "the STUN server did not resolve");
            }
            Err(_) => {
                tracing::debug!(host, ?family, "the STUN server did not resolve in time");
            }
        }

        if primary.is_empty() && !self.doh_endpoints.is_empty() {
            if !fake.is_empty() {
                tracing::debug!(
                    host,
                    ?family,
                    "system DNS returned only Fake-IP addresses; resolving via DoH"
                );
            }
            let doh_ips = self.resolve_doh(host, port, family, deadline).await;
            return ResolvedTargets {
                primary: doh_ips,
                fake,
                can_fallback_doh: false,
            };
        }

        ResolvedTargets {
            primary,
            fake,
            can_fallback_doh: !self.doh_endpoints.is_empty(),
        }
    }

    /// Races the configured DoH JSON endpoints and returns the first non-empty
    /// set of usable addresses for `(host, family)`.
    async fn resolve_doh(
        &self,
        host: &str,
        port: u16,
        family: Family,
        deadline: Instant,
    ) -> Vec<SocketAddr> {
        let Some(client) = self.http.clone() else {
            return Vec::new();
        };
        if self.doh_endpoints.is_empty() {
            return Vec::new();
        }
        let now = Instant::now();
        let doh_deadline = deadline.min(now + DOH_TIMEOUT);
        let Some(timeout) = doh_deadline.checked_duration_since(now) else {
            return Vec::new();
        };
        if timeout.is_zero() {
            return Vec::new();
        }

        let destinations = self.destinations;
        let mut queries: JoinSet<Vec<SocketAddr>> = JoinSet::new();
        for endpoint in self.doh_endpoints.iter() {
            let client = client.clone();
            let endpoint = endpoint.clone();
            let host = host.to_owned();
            queries.spawn(async move {
                query_doh_endpoint(
                    &client,
                    &endpoint,
                    &host,
                    port,
                    family,
                    destinations,
                    timeout,
                )
                .await
            });
        }

        while let Some(joined) = queries.join_next().await {
            match joined {
                Ok(addresses) if !addresses.is_empty() => {
                    queries.abort_all();
                    return addresses;
                }
                Ok(_) => {}
                Err(error) if error.is_panic() => {
                    std::panic::resume_unwind(error.into_panic());
                }
                Err(_) => {}
            }
        }
        Vec::new()
    }

    async fn ask_stun(
        &self,
        host: &str,
        port: u16,
        transport: StunTransport,
        family: Family,
    ) -> StunAttempt {
        let deadline = Instant::now() + STUN_DEADLINE;
        let targets = self.resolve_host(host, port, family, deadline).await;
        self.ask_stun_resolved(host, port, transport, family, &targets, deadline)
            .await
    }

    async fn ask_stun_resolved(
        &self,
        host: &str,
        port: u16,
        transport: StunTransport,
        family: Family,
        targets: &ResolvedTargets,
        deadline: Instant,
    ) -> StunAttempt {
        let mut tried_any = false;

        if !targets.primary.is_empty() {
            tried_any = true;
            if let Some(attempt) = self
                .exchange_any(&targets.primary, transport, family, deadline)
                .await
            {
                return attempt;
            }
        }

        // If system DNS returned non-fake IPs that all failed to answer (for
        // example due to DNS poisoning), try DoH once for alternative real IPs.
        if targets.can_fallback_doh {
            let doh_deadline = Instant::now() + DOH_TIMEOUT;
            let extra: Vec<SocketAddr> = self
                .resolve_doh(host, port, family, doh_deadline)
                .await
                .into_iter()
                .filter(|addr| !targets.primary.contains(addr))
                .collect();
            if !extra.is_empty() {
                tried_any = true;
                let retry_deadline = Instant::now() + STUN_DEADLINE;
                if let Some(attempt) = self
                    .exchange_any(&extra, transport, family, retry_deadline)
                    .await
                {
                    return attempt;
                }
            }
        }

        // Last-resort TCP fallback for Fake-IPs when DoH yielded nothing (e.g.
        // a manual server test while TUN is active and DoH is blocked).
        if targets.primary.is_empty() && !targets.fake.is_empty() && transport == StunTransport::Tcp
        {
            tried_any = true;
            if let Some(attempt) = self
                .exchange_any(&targets.fake, transport, family, deadline)
                .await
            {
                return attempt;
            }
        }

        if tried_any {
            StunAttempt::NoAnswer
        } else {
            StunAttempt::NoAddress
        }
    }

    async fn exchange_any(
        &self,
        servers: &[SocketAddr],
        transport: StunTransport,
        family: Family,
        deadline: Instant,
    ) -> Option<StunAttempt> {
        let id = match TransactionId::random() {
            Ok(id) => id,
            Err(error) => {
                tracing::warn!(%error, "no randomness for a STUN transaction ID");
                return None;
            }
        };
        let started = Instant::now();
        let destinations = self.destinations;
        let exchange = async {
            match transport {
                StunTransport::Udp => exchange_udp(servers, family, id, destinations).await,
                StunTransport::Tcp => {
                    exchange_tcp(servers, family, id, destinations, deadline).await
                }
            }
        };
        match tokio::time::timeout_at(deadline, exchange).await {
            Ok(Ok(mapped)) => Some(StunAttempt::Answered {
                address: mapped.ip(),
                latency_ms: started.elapsed().as_millis() as u64,
            }),
            Ok(Err(error)) => {
                tracing::debug!(?servers, ?transport, %error, "the STUN exchange failed");
                None
            }
            Err(_) => None,
        }
    }

    async fn probe_stun_phase(
        &self,
        targets: &[StunServer],
        transport: StunTransport,
        family: Family,
        permission: &Arc<dyn ProbePermission>,
    ) -> Option<EgressAnswer<IpAddr>> {
        let mut candidates = targets
            .iter()
            .filter(|target| supports_transport(target.capabilities, transport, family))
            .cloned()
            .collect::<std::collections::VecDeque<_>>();
        let mut in_flight: JoinSet<Option<EgressAnswer<IpAddr>>> = JoinSet::new();

        loop {
            while in_flight.is_empty() {
                let target = candidates.pop_front()?;
                if !permission.allows_stun(&target) {
                    continue;
                }
                self.spawn_stun_target(&mut in_flight, target, transport, family);
            }

            let can_hedge = !candidates.is_empty() && in_flight.len() < MAX_CONCURRENT_STUN;
            if can_hedge {
                tokio::select! {
                    joined = in_flight.join_next() => {
                        if let Some(answer) = unwrap_probe_join(joined, &mut in_flight) {
                            return Some(answer);
                        }
                        if in_flight.len() < MAX_CONCURRENT_STUN {
                            while let Some(target) = candidates.pop_front() {
                                if permission.allows_stun(&target) {
                                    self.spawn_stun_target(
                                        &mut in_flight,
                                        target,
                                        transport,
                                        family,
                                    );
                                    break;
                                }
                            }
                        }
                    }
                    () = tokio::time::sleep(STUN_HEDGE_DELAY) => {
                        while let Some(target) = candidates.pop_front() {
                            if permission.allows_stun(&target) {
                                self.spawn_stun_target(
                                    &mut in_flight,
                                    target,
                                    transport,
                                    family,
                                );
                                break;
                            }
                        }
                    }
                }
            } else {
                let joined = in_flight.join_next().await;
                if let Some(answer) = unwrap_probe_join(joined, &mut in_flight) {
                    return Some(answer);
                }
            }
        }
    }

    fn spawn_stun_target(
        &self,
        in_flight: &mut JoinSet<Option<EgressAnswer<IpAddr>>>,
        target: StunServer,
        transport: StunTransport,
        family: Family,
    ) {
        let runtime = self.clone();
        in_flight.spawn(async move {
            match runtime
                .ask_stun(&target.host, target.port, transport, family)
                .await
            {
                StunAttempt::Answered { address, .. } if family.matches(address) => {
                    Some(EgressAnswer {
                        address,
                        source: EgressSource::Stun {
                            host: target.host,
                            port: target.port,
                            transport,
                        },
                    })
                }
                _ => None,
            }
        });
    }

    /// The address a plain-text echo answers with. `None` for anything else.
    async fn echo(&self, url: &str) -> Option<IpAddr> {
        let client = self.http.as_ref()?;
        match fetch(client, url).await {
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
}

fn unwrap_probe_join(
    joined: Option<Result<Option<EgressAnswer<IpAddr>>, tokio::task::JoinError>>,
    in_flight: &mut JoinSet<Option<EgressAnswer<IpAddr>>>,
) -> Option<EgressAnswer<IpAddr>> {
    match joined {
        Some(Ok(Some(answer))) => {
            in_flight.abort_all();
            Some(answer)
        }
        Some(Ok(None)) | None => None,
        Some(Err(error)) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Some(Err(_)) => None,
    }
}

async fn query_doh_endpoint(
    client: &reqwest::Client,
    endpoint: &str,
    host: &str,
    port: u16,
    family: Family,
    destinations: fn(IpAddr) -> bool,
    timeout: Duration,
) -> Vec<SocketAddr> {
    let Ok(mut url) = url::Url::parse(endpoint) else {
        return Vec::new();
    };
    url.query_pairs_mut()
        .append_pair("name", host)
        .append_pair("type", family.dns_record_type());
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/dns-json")
        .timeout(timeout)
        .send()
        .await;
    let body: serde_json::Value = match response {
        Ok(resp) => match resp.error_for_status() {
            Ok(ok) => match ok.json().await {
                Ok(json) => json,
                Err(error) => {
                    tracing::debug!(endpoint, host, %error, "DoH response was not valid JSON");
                    return Vec::new();
                }
            },
            Err(error) => {
                tracing::debug!(endpoint, host, %error, "DoH endpoint returned an error status");
                return Vec::new();
            }
        },
        Err(error) => {
            tracing::debug!(endpoint, host, %error, "DoH request failed");
            return Vec::new();
        }
    };
    parse_doh_ips(&body, port, family, destinations)
}

fn parse_doh_ips(
    body: &serde_json::Value,
    port: u16,
    family: Family,
    destinations: fn(IpAddr) -> bool,
) -> Vec<SocketAddr> {
    let expected_type = family.dns_type_code();
    let Some(answers) = body.get("Answer").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    let mut addresses = Vec::new();
    for answer in answers {
        if answer.get("type").and_then(serde_json::Value::as_u64) != Some(expected_type) {
            continue;
        }
        let Some(ip) = answer
            .get("data")
            .and_then(serde_json::Value::as_str)
            .and_then(|raw| raw.parse::<IpAddr>().ok())
        else {
            continue;
        };
        if family.matches(ip) && destinations(ip) && !is_fake_ip(ip) {
            let target = SocketAddr::new(ip, port);
            if !addresses.contains(&target) {
                addresses.push(target);
                if addresses.len() >= MAX_RESOLVED_IPS {
                    break;
                }
            }
        }
    }
    addresses
}

impl NetworkDirectEgressProbe {
    async fn probe(&self, sources: &ProbeSources, family: Family) -> Option<EgressAnswer<IpAddr>> {
        // Prefer UDP across all eligible servers first: behind TCP-only
        // transparent proxies, UDP to real IPs still reveals the direct egress
        // whereas TCP can hit the proxy exit. Fall back to TCP only when no
        // UDP server answers (e.g. firewalls blocking outbound UDP).
        if let Some(answer) = self
            .runtime
            .probe_stun_phase(
                &sources.stun,
                StunTransport::Udp,
                family,
                &sources.permission,
            )
            .await
        {
            return Some(answer);
        }
        if let Some(answer) = self
            .runtime
            .probe_stun_phase(
                &sources.stun,
                StunTransport::Tcp,
                family,
                &sources.permission,
            )
            .await
        {
            return Some(answer);
        }
        for provider in &sources.lookups {
            if !sources.permission.allows_lookup(*provider) {
                continue;
            }
            let url = match family {
                Family::Ipv4 => provider.ipv4_url(),
                Family::Ipv6 => provider.ipv6_url(),
            };
            if let Some(address) = self
                .runtime
                .echo(url)
                .await
                .filter(|address| family.matches(*address))
            {
                return Some(EgressAnswer {
                    address,
                    source: EgressSource::Lookup {
                        provider: *provider,
                    },
                });
            }
            tracing::debug!(?provider, ?family, "the IP lookup provider gave no address");
        }
        None
    }
}

#[async_trait::async_trait]
impl DirectEgressProbe for NetworkDirectEgressProbe {
    async fn ipv4(&self, sources: &ProbeSources) -> Option<EgressAnswer<Ipv4Addr>> {
        let EgressAnswer { address, source } = self.probe(sources, Family::Ipv4).await?;
        match address {
            IpAddr::V4(address) => Some(EgressAnswer { address, source }),
            IpAddr::V6(_) => None,
        }
    }

    async fn ipv6(&self, sources: &ProbeSources) -> Option<EgressAnswer<Ipv6Addr>> {
        let EgressAnswer { address, source } = self.probe(sources, Family::Ipv6).await?;
        match address {
            IpAddr::V6(address) => Some(EgressAnswer { address, source }),
            IpAddr::V4(_) => None,
        }
    }

    async fn test_stun_server(&self, host: &str, port: u16) -> StunServerTest {
        let deadline = Instant::now() + STUN_DEADLINE;
        let (v4_targets, v6_targets) = tokio::join!(
            self.runtime
                .resolve_host(host, port, Family::Ipv4, deadline),
            self.runtime
                .resolve_host(host, port, Family::Ipv6, deadline),
        );
        let (udp_ipv4, tcp_ipv4, udp_ipv6, tcp_ipv6) = tokio::join!(
            self.runtime.ask_stun_resolved(
                host,
                port,
                StunTransport::Udp,
                Family::Ipv4,
                &v4_targets,
                deadline
            ),
            self.runtime.ask_stun_resolved(
                host,
                port,
                StunTransport::Tcp,
                Family::Ipv4,
                &v4_targets,
                deadline
            ),
            self.runtime.ask_stun_resolved(
                host,
                port,
                StunTransport::Udp,
                Family::Ipv6,
                &v6_targets,
                deadline
            ),
            self.runtime.ask_stun_resolved(
                host,
                port,
                StunTransport::Tcp,
                Family::Ipv6,
                &v6_targets,
                deadline
            ),
        );
        StunServerTest::new(udp_ipv4, udp_ipv6, tcp_ipv4, tcp_ipv6)
    }
}

#[derive(Debug, snafu::Snafu)]
enum ExchangeError {
    #[snafu(context(false), display("socket error"))]
    Io { source: std::io::Error },
    #[snafu(context(false), display("the response is malformed"))]
    Decode { source: stun::DecodeError },
    #[snafu(display("the response reported a non-public or wrong-family address"))]
    InvalidMappedAddress,
    #[snafu(display("no STUN server address answered"))]
    NoResponse,
}

fn accept_mapped_address(
    mapped: SocketAddr,
    family: Family,
    destinations: fn(IpAddr) -> bool,
) -> Result<SocketAddr, ExchangeError> {
    let ip = mapped.ip();
    if family.matches(ip) && destinations(ip) && !is_fake_ip(ip) {
        Ok(mapped)
    } else {
        Err(ExchangeError::InvalidMappedAddress)
    }
}

/// Sends the request on an unconnected UDP socket, round-robining across the
/// resolved `servers` on each retransmission. Using an unconnected socket
/// accepts replies from multi-homed STUN servers and avoids aborting failover
/// on a single ICMP error.
async fn exchange_udp(
    servers: &[SocketAddr],
    family: Family,
    id: TransactionId,
    destinations: fn(IpAddr) -> bool,
) -> Result<SocketAddr, ExchangeError> {
    if servers.is_empty() {
        return Err(ExchangeError::NoResponse);
    }
    let socket = UdpSocket::bind(family.unspecified()).await?;
    let request = stun::binding_request(id);
    let mut buffer = [0; MAX_MESSAGE_LEN];
    let mut delays = UDP_RETRANSMISSIONS.into_iter();
    let mut attempt: usize = 0;

    loop {
        // Send to the next target in round-robin order; if all targets reject
        // `send_to` (e.g. no IPv6 route), fail fast instead of waiting out the deadline.
        let mut sent = false;
        let mut last_send_error = None;
        for offset in 0..servers.len() {
            let target = servers[(attempt + offset) % servers.len()];
            match socket.send_to(&request, target).await {
                Ok(_) => {
                    sent = true;
                    break;
                }
                Err(error) => {
                    last_send_error = Some(error);
                }
            }
        }
        if !sent {
            return Err(last_send_error
                .map(ExchangeError::from)
                .unwrap_or(ExchangeError::NoResponse));
        }
        attempt += 1;

        let wait = delays.next();
        let resend_at = wait.map(|delay| Instant::now() + delay);
        loop {
            let received = match resend_at {
                Some(at) => {
                    match tokio::time::timeout_at(at, socket.recv_from(&mut buffer)).await {
                        Ok(result) => result,
                        Err(_) => break,
                    }
                }
                // The last request waits until the caller's deadline.
                None => socket.recv_from(&mut buffer).await,
            };
            match received {
                Ok((len, from)) => {
                    if !destinations(from.ip()) {
                        continue;
                    }
                    if let Ok(mapped) = stun::mapped_address(&buffer[..len], id)
                        && let Ok(valid) = accept_mapped_address(mapped, family, destinations)
                    {
                        return Ok(valid);
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    // Windows raises WSAECONNRESET on unconnected UDP sockets
                    // when a remote host sends ICMP Port Unreachable. Fail over
                    // immediately while other addresses and retries remain, and
                    // stop once the last attempt is exhausted.
                    if servers.len() > 1 && resend_at.is_some() {
                        break;
                    }
                    return Err(error.into());
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
}

/// Tries each resolved TCP target within `deadline`, validating the STUN header
/// and declared body length before allocating and reading the body.
async fn exchange_tcp(
    servers: &[SocketAddr],
    family: Family,
    id: TransactionId,
    destinations: fn(IpAddr) -> bool,
    deadline: Instant,
) -> Result<SocketAddr, ExchangeError> {
    let mut last_error = ExchangeError::NoResponse;
    for (index, &server) in servers.iter().enumerate() {
        let now = Instant::now();
        let Some(remaining) = deadline.checked_duration_since(now) else {
            break;
        };
        if remaining.is_zero() {
            break;
        }
        let remaining_servers = (servers.len() - index) as u32;
        let attempt_budget = if remaining_servers <= 1 {
            remaining
        } else {
            (remaining / remaining_servers)
                .max(MIN_TCP_ATTEMPT_WINDOW)
                .min(remaining)
        };
        let attempt_deadline = now + attempt_budget;
        match tokio::time::timeout_at(
            attempt_deadline,
            exchange_tcp_once(server, family, id, destinations),
        )
        .await
        {
            Ok(Ok(mapped)) => return Ok(mapped),
            Ok(Err(error)) => last_error = error,
            Err(_) => last_error = ExchangeError::NoResponse,
        }
    }
    Err(last_error)
}

async fn exchange_tcp_once(
    server: SocketAddr,
    family: Family,
    id: TransactionId,
    destinations: fn(IpAddr) -> bool,
) -> Result<SocketAddr, ExchangeError> {
    let mut stream = TcpStream::connect(server).await?;
    stream.write_all(&stun::binding_request(id)).await?;
    let mut header = [0; HEADER_LEN];
    stream.read_exact(&mut header).await?;
    let body_len = stun::validate_header(&header, id, MAX_MESSAGE_LEN - HEADER_LEN)?;
    let mut message = Vec::with_capacity(HEADER_LEN + body_len);
    message.extend_from_slice(&header);
    message.resize(HEADER_LEN + body_len, 0);
    stream.read_exact(&mut message[HEADER_LEN..]).await?;
    let mapped = stun::mapped_address(&message, id)?;
    accept_mapped_address(mapped, family, destinations)
}

async fn fetch(client: &reqwest::Client, url: &str) -> reqwest::Result<String> {
    client
        .get(url)
        .timeout(ECHO_TIMEOUT)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use axum::{Router, extract::Query, http::StatusCode, routing::get};
    use nyanpasu_config::application::{IpLookupProvider, StunFamilies, StunServer};
    use nyanpasu_contracts::net::ProbePermission;
    use tokio::net::TcpListener;

    use super::{stun::tests::binding_response, *};

    const LOCALHOST: &str = "127.0.0.1";
    /// The servers these tests run listen on loopback.
    const ANYWHERE: fn(IpAddr) -> bool = |_| true;

    fn loopback_probe() -> NetworkDirectEgressProbe {
        NetworkDirectEgressProbe {
            runtime: ProbeRuntime {
                destinations: ANYWHERE,
                http: build_direct_http_client(),
                doh_endpoints: Arc::from([]),
            },
        }
    }

    fn loopback_probe_with_doh(endpoints: Vec<String>) -> NetworkDirectEgressProbe {
        NetworkDirectEgressProbe {
            runtime: ProbeRuntime {
                destinations: ANYWHERE,
                http: build_direct_http_client(),
                doh_endpoints: Arc::from(endpoints),
            },
        }
    }

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

    /// Answers every UDP STUN request with a fixed mapped address.
    async fn udp_stun_server_returning(mapped: SocketAddr) -> u16 {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = socket.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut buffer = [0; MAX_MESSAGE_LEN];
            loop {
                let (len, from) = socket.recv_from(&mut buffer).await.unwrap();
                let response = binding_response(&buffer[..len], mapped);
                socket.send_to(&response, from).await.unwrap();
            }
        });
        port
    }

    /// Listens on UDP and never replies, holding the port open so no ICMP
    /// Port Unreachable is generated.
    async fn silent_udp_server() -> (u16, Arc<AtomicUsize>) {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = socket.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        tokio::spawn(async move {
            let mut buffer = [0; MAX_MESSAGE_LEN];
            loop {
                if socket.recv_from(&mut buffer).await.is_ok() {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
        (port, hits)
    }

    async fn tcp_stun_server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut stream, from) = listener.accept().await.unwrap();
                let mut request = [0; HEADER_LEN];
                if stream.read_exact(&mut request).await.is_err() {
                    continue;
                }
                let response = binding_response(&request, from);
                // Split the response to check that the client reads it whole.
                let (first, rest) = response.split_at(10);
                let _ = stream.write_all(first).await;
                let _ = stream.flush().await;
                let _ = stream.write_all(rest).await;
            }
        });
        port
    }

    /// Accepts a TCP connection and replies with a non-STUN HTTP banner.
    async fn tcp_http_banner_server() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; HEADER_LEN];
                let _ = stream.read_exact(&mut request).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n")
                    .await;
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

        let attempt = loopback_probe()
            .runtime
            .ask_stun(LOCALHOST, port, StunTransport::Udp, Family::Ipv4)
            .await;

        assert!(matches!(
            attempt,
            StunAttempt::Answered { address, .. } if address == IpAddr::from(Ipv4Addr::LOCALHOST)
        ));
    }

    #[tokio::test]
    async fn a_lost_udp_request_is_sent_again() {
        let port = udp_stun_server(1).await;

        let attempt = loopback_probe()
            .runtime
            .ask_stun(LOCALHOST, port, StunTransport::Udp, Family::Ipv4)
            .await;

        assert!(
            matches!(attempt, StunAttempt::Answered { .. }),
            "{attempt:?}"
        );
    }

    #[tokio::test]
    async fn a_tcp_server_names_the_address_the_request_came_from() {
        let port = tcp_stun_server().await;

        let attempt = loopback_probe()
            .runtime
            .ask_stun(LOCALHOST, port, StunTransport::Tcp, Family::Ipv4)
            .await;

        assert!(matches!(
            attempt,
            StunAttempt::Answered { address, .. } if address == IpAddr::from(Ipv4Addr::LOCALHOST)
        ));
    }

    #[tokio::test]
    async fn a_host_without_an_address_of_the_family_is_not_asked() {
        let attempt = loopback_probe()
            .runtime
            .ask_stun(LOCALHOST, 3478, StunTransport::Udp, Family::Ipv6)
            .await;

        assert_eq!(attempt, StunAttempt::NoAddress);
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_udp_server_is_no_answer() {
        let (port, _) = silent_udp_server().await;
        let attempt = loopback_probe()
            .runtime
            .ask_stun(LOCALHOST, port, StunTransport::Udp, Family::Ipv4)
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
            loopback_probe().ipv4(&sources).await,
            Some(EgressAnswer {
                address: Ipv4Addr::LOCALHOST,
                source: EgressSource::Stun {
                    host: LOCALHOST.into(),
                    port: tcp,
                    transport: StunTransport::Tcp,
                },
            })
        );
        assert_eq!(loopback_probe().ipv6(&sources).await, None);
    }

    #[tokio::test]
    async fn dual_transport_server_falls_back_from_udp_to_tcp_when_udp_fails() {
        let tcp = tcp_stun_server().await;
        let sources = sources(vec![target(tcp, true, true)], AllowingPorts(vec![tcp]));

        assert_eq!(
            loopback_probe().ipv4(&sources).await,
            Some(EgressAnswer {
                address: Ipv4Addr::LOCALHOST,
                source: EgressSource::Stun {
                    host: LOCALHOST.into(),
                    port: tcp,
                    transport: StunTransport::Tcp,
                },
            })
        );
    }

    #[tokio::test]
    async fn udp_phase_runs_across_all_servers_before_tcp_fallback() {
        let tcp = tcp_stun_server().await;
        let udp = udp_stun_server(0).await;
        let sources = sources(
            vec![target(tcp, false, true), target(udp, true, false)],
            AllowingPorts(vec![tcp, udp]),
        );

        assert_eq!(
            loopback_probe().ipv4(&sources).await,
            Some(EgressAnswer {
                address: Ipv4Addr::LOCALHOST,
                source: EgressSource::Stun {
                    host: LOCALHOST.into(),
                    port: udp,
                    transport: StunTransport::Udp,
                },
            })
        );
    }

    #[tokio::test(start_paused = true)]
    async fn hedged_probing_switches_to_second_server_before_first_deadline() {
        let (silent, _) = silent_udp_server().await;
        let fast = udp_stun_server(0).await;
        let sources = sources(
            vec![target(silent, true, false), target(fast, true, false)],
            AllowingPorts(vec![silent, fast]),
        );
        let started = Instant::now();

        let answer = loopback_probe().ipv4(&sources).await;

        assert_eq!(
            answer,
            Some(EgressAnswer {
                address: Ipv4Addr::LOCALHOST,
                source: EgressSource::Stun {
                    host: LOCALHOST.into(),
                    port: fast,
                    transport: StunTransport::Udp,
                },
            })
        );
        assert!(
            started.elapsed() < STUN_DEADLINE,
            "hedged probe should not wait for full primary deadline: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn multi_ip_udp_fails_over_to_working_address() {
        let (silent, _) = silent_udp_server().await;
        let working = udp_stun_server(0).await;
        let servers = [
            SocketAddr::new(Ipv4Addr::LOCALHOST.into(), silent),
            SocketAddr::new(Ipv4Addr::LOCALHOST.into(), working),
        ];

        let mapped = exchange_udp(
            &servers,
            Family::Ipv4,
            TransactionId::random().unwrap(),
            ANYWHERE,
        )
        .await
        .unwrap();

        assert_eq!(mapped.ip(), IpAddr::from(Ipv4Addr::LOCALHOST));
    }

    #[tokio::test]
    async fn doh_fallback_resolves_when_system_dns_fails_or_strips_family() {
        let udp = udp_stun_server(0).await;
        let doh_url = serve(Router::new().route(
            "/resolve",
            get(
                |Query(params): Query<std::collections::HashMap<String, String>>| async move {
                    let record_type = params.get("type").map(String::as_str);
                    let body = match record_type {
                        Some("A") => serde_json::json!({
                            "Status": 0,
                            "Answer": [{ "type": 1, "data": "127.0.0.1" }]
                        }),
                        _ => serde_json::json!({ "Status": 0, "Answer": [] }),
                    };
                    axum::Json(body)
                },
            ),
        ))
        .await;
        let probe = loopback_probe_with_doh(vec![format!("{doh_url}resolve")]);

        // `.invalid` is RFC 2606 reserved and guaranteed to fail system DNS.
        let attempt = probe
            .runtime
            .ask_stun("stun.test.invalid", udp, StunTransport::Udp, Family::Ipv4)
            .await;

        assert!(matches!(
            attempt,
            StunAttempt::Answered { address, .. } if address == IpAddr::from(Ipv4Addr::LOCALHOST)
        ));
    }

    #[tokio::test]
    async fn tcp_rejects_non_stun_banner_immediately() {
        let banner = tcp_http_banner_server().await;
        let started = Instant::now();

        let attempt = loopback_probe()
            .runtime
            .ask_stun(LOCALHOST, banner, StunTransport::Tcp, Family::Ipv4)
            .await;

        assert_eq!(attempt, StunAttempt::NoAnswer);
        assert!(started.elapsed() < STUN_DEADLINE);
    }

    #[tokio::test]
    async fn fake_ip_mapped_address_is_rejected() {
        let port = udp_stun_server_returning("198.18.0.42:3478".parse().unwrap()).await;
        let probe = loopback_probe();

        let attempt = probe
            .runtime
            .ask_stun(LOCALHOST, port, StunTransport::Udp, Family::Ipv4)
            .await;

        assert_eq!(attempt, StunAttempt::NoAnswer);
    }

    #[tokio::test]
    async fn a_server_the_permission_no_longer_allows_is_not_asked() {
        let udp = udp_stun_server(0).await;
        let sources = sources(vec![target(udp, true, false)], AllowingPorts(Vec::new()));

        assert_eq!(loopback_probe().ipv4(&sources).await, None);
    }

    #[tokio::test]
    async fn a_server_test_reports_each_transport_and_family() {
        let udp = udp_stun_server(0).await;

        let test = loopback_probe().test_stun_server(LOCALHOST, udp).await;

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
        for fake in ["198.18.0.1", "198.19.255.254", "::ffff:198.18.6.4"] {
            assert!(is_fake_ip(fake.parse().unwrap()), "{fake}");
        }
        for real in [
            "198.17.255.254",
            "198.20.0.1",
            "203.0.113.7",
            "2606:4700:49::",
        ] {
            assert!(!is_fake_ip(real.parse().unwrap()), "{real}");
        }
    }

    #[tokio::test]
    async fn parse_doh_ips_filters_types_fake_ips_and_private_addresses() {
        let body = serde_json::json!({
            "Status": 0,
            "Answer": [
                { "type": 5, "data": "alias.example.com." },
                { "type": 1, "data": "198.18.6.4" },
                { "type": 1, "data": "127.0.0.1" },
                { "type": 1, "data": "162.159.207.0" },
                { "type": 1, "data": "162.159.207.0" },
                { "type": 28, "data": "2606:4700:49::" }
            ]
        });

        assert_eq!(
            parse_doh_ips(&body, 3478, Family::Ipv4, is_public),
            vec!["162.159.207.0:3478".parse().unwrap()]
        );
        assert_eq!(
            parse_doh_ips(&body, 3478, Family::Ipv6, is_public),
            vec!["[2606:4700:49::]:3478".parse().unwrap()]
        );
    }

    #[tokio::test]
    async fn an_echo_reads_the_address_it_answers() {
        let probe = loopback_probe();
        assert_eq!(
            probe.runtime.echo(&answering("203.0.113.7\n").await).await,
            Some(Ipv4Addr::new(203, 0, 113, 7).into())
        );
        assert_eq!(
            probe.runtime.echo(&answering("2001:db8::7").await).await,
            Some("2001:db8::7".parse().unwrap())
        );
    }

    #[tokio::test]
    async fn a_page_or_an_error_status_is_no_answer() {
        let probe = loopback_probe();
        let failing = serve(Router::new().route(
            "/",
            get(|| async { (StatusCode::SERVICE_UNAVAILABLE, "203.0.113.7") }),
        ))
        .await;

        assert_eq!(
            probe
                .runtime
                .echo(&answering("<html>sign in</html>").await)
                .await,
            None
        );
        assert_eq!(probe.runtime.echo(&failing).await, None);
    }

    #[tokio::test]
    async fn an_unreachable_echo_is_no_answer() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed = format!("http://{}/", listener.local_addr().unwrap());
        drop(listener);

        assert_eq!(loopback_probe().runtime.echo(&closed).await, None);
    }

    #[test]
    fn every_provider_has_an_endpoint_per_family() {
        for provider in IpLookupProvider::ALL {
            assert!(provider.ipv4_url().starts_with("https://"), "{provider:?}");
            assert!(provider.ipv6_url().starts_with("https://"), "{provider:?}");
        }
    }

    #[tokio::test]
    #[ignore = "requires live internet access"]
    async fn live_default_stun_probe_discovers_egress() {
        struct AllowAll;
        impl ProbePermission for AllowAll {
            fn allows_stun(&self, _: &StunServer) -> bool {
                true
            }
            fn allows_lookup(&self, _: IpLookupProvider) -> bool {
                false
            }
        }

        let probe = NetworkDirectEgressProbe::default();
        let sources = ProbeSources {
            stun: nyanpasu_config::application::default_stun_servers()
                .into_iter()
                .filter(|server| server.enabled)
                .collect(),
            lookups: Vec::new(),
            permission: Arc::new(AllowAll),
        };
        let started = Instant::now();
        let (v4, v6) = tokio::join!(probe.ipv4(&sources), probe.ipv6(&sources));
        eprintln!(
            "live STUN probe finished in {:?}: ipv4={v4:?}, ipv6={v6:?}",
            started.elapsed()
        );
        assert!(v4.is_some(), "expected IPv4 STUN answer");
    }
}
