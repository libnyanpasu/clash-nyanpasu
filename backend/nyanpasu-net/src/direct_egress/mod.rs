//! Direct-route egress probe, isolated from system and self proxies: STUN
//! servers first (UDP phase, then TCP fallback phase with staggered hedging),
//! then the accepted HTTP lookup services. The STUN codec is pure.
mod stun;

use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use nyanpasu_config::application::{IpLookupProvider, StunServer, StunServerCapabilities};
use nyanpasu_contracts::net::{
    DirectEgressProbe, EgressAnswer, EgressSource, ProbePermission, ProbeSources, StunAttempt,
    StunServerTest, StunTransport,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpStream, UdpSocket},
    sync::{OnceCell, oneshot},
    task::JoinSet,
    time::{Instant, Sleep},
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
/// Budget for the initial primary exchange when fallback DoH may still run
/// inside the same [`STUN_DEADLINE`].
const PRIMARY_WITH_FALLBACK_BUDGET: Duration = Duration::from_millis(1500);
/// Minimum slice reserved for a retry exchange after fallback DoH within [`STUN_DEADLINE`].
const MIN_FALLBACK_RETRY_WINDOW: Duration = Duration::from_millis(500);
/// Minimum per-address TCP connect/exchange slice when a host resolves to several IPs.
const MIN_TCP_ATTEMPT_WINDOW: Duration = Duration::from_millis(800);
/// A family without a route ends here rather than at the OS connect timeout.
const ECHO_TIMEOUT: Duration = Duration::from_secs(5);
/// Large enough for any Binding response a server sends.
const MAX_MESSAGE_LEN: usize = 1024;
/// Maximum HTTP response body accepted from a DoH JSON endpoint.
const MAX_DOH_BODY_LEN: usize = 16 * 1024;
/// Maximum HTTP response body accepted from a plain-text IP echo provider.
const MAX_ECHO_BODY_LEN: usize = 256;
/// IP-literal DoH JSON endpoints used only when system DNS returns Fake-IPs
/// (`198.18.0.0/15`), omits the requested family, fails, or returns unreachable
/// (e.g. DNS-poisoned) addresses. Using IP literals avoids depending on system DNS.
const DEFAULT_DOH_ENDPOINTS: [&str; 3] = [
    "https://223.5.5.5/resolve",
    "https://1.1.1.1/dns-query",
    "https://1.12.12.12/resolve",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    // The system proxy and the core's ports would both route requests by rule,
    // and redirects could bounce a probe onto an unintended or internal host.
    match reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(client) => Some(client),
        Err(error) => {
            tracing::warn!(%error, "failed to construct direct HTTP client for egress probing");
            None
        }
    }
}

fn is_unroutable_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NetworkUnreachable
            | std::io::ErrorKind::HostUnreachable
            | std::io::ErrorKind::AddrNotAvailable
    )
}

/// Checks whether the local host has a route for `family` without sending any
/// packet, so IPv4-only hosts do not fire DoH lookups for IPv6.
fn is_family_routable(family: Family) -> bool {
    let target: SocketAddr = match family {
        Family::Ipv4 => ([1, 1, 1, 1], 53).into(),
        Family::Ipv6 => (
            Ipv6Addr::new(0x2606, 0x4700, 0x4700, 0, 0, 0, 0, 0x1111),
            53,
        )
            .into(),
    };
    let Ok(socket) = std::net::UdpSocket::bind(family.unspecified()) else {
        return false;
    };
    match socket.connect(target) {
        Ok(()) => true,
        Err(error) => !is_unroutable_error(&error),
    }
}

/// Per-probe cache of resolved IPs keyed by `(lowercase_host, family)` so
/// UDP and TCP phases (and concurrent UDP/TCP checks in `test_stun_server`)
/// avoid repeating system DNS or DoH lookups.
type TargetCacheMap = HashMap<(String, Family), Arc<CachedFamilyTargets>>;

#[derive(Clone, Default)]
struct TargetCache {
    entries: Arc<Mutex<TargetCacheMap>>,
}

impl TargetCache {
    fn entry(&self, host: &str, family: Family) -> Arc<CachedFamilyTargets> {
        let key = (host.to_ascii_lowercase(), family);
        let mut guard = self.entries.lock().expect("target cache lock poisoned");
        Arc::clone(guard.entry(key).or_default())
    }
}

#[derive(Default)]
struct CachedFamilyTargets {
    resolved: OnceCell<ResolvedHostIps>,
    fallback_doh: OnceCell<Vec<IpAddr>>,
}

#[derive(Debug, Clone)]
struct ResolvedHostIps {
    primary: Vec<IpAddr>,
    fake: Vec<IpAddr>,
    can_fallback_doh: bool,
}

impl ResolvedHostIps {
    fn at_port(&self, port: u16) -> ResolvedTargets {
        ResolvedTargets {
            primary: self
                .primary
                .iter()
                .map(|&ip| SocketAddr::new(ip, port))
                .collect(),
            fake: self
                .fake
                .iter()
                .map(|&ip| SocketAddr::new(ip, port))
                .collect(),
            can_fallback_doh: self.can_fallback_doh,
        }
    }
}

struct AllowAllProbe;

impl ProbePermission for AllowAllProbe {
    fn allows_stun(&self, _: &StunServer) -> bool {
        true
    }
    fn allows_lookup(&self, _: IpLookupProvider) -> bool {
        true
    }
}

/// Shared, cheaply cloneable runtime state for probing tasks.
#[derive(Clone)]
struct ProbeRuntime {
    /// Predicate for acceptable STUN server, response-source, and mapped addresses.
    destinations: fn(IpAddr) -> bool,
    /// Predicate indicating whether `family` is locally routable on this host.
    family_routable: fn(Family) -> bool,
    /// Shared no-proxy HTTP client for DoH fallback and HTTP lookup providers.
    http: Option<reqwest::Client>,
    /// DoH JSON endpoints queried sequentially on fallback when system DNS cannot
    /// supply working real addresses.
    doh_endpoints: Arc<[String]>,
    /// Optional single-probe cache shared across UDP and TCP phases.
    target_cache: Option<TargetCache>,
}

impl ProbeRuntime {
    fn with_target_cache(&self) -> Self {
        let mut next = self.clone();
        next.target_cache = Some(TargetCache::default());
        next
    }
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
                family_routable: is_family_routable,
                http: build_direct_http_client(),
                doh_endpoints: Arc::from(DEFAULT_DOH_ENDPOINTS.map(str::to_owned)),
                target_cache: None,
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
            let [a, b, ..] = ip.octets();
            let is_cgnat = a == 100 && (b & 0xc0) == 64;
            let is_this_network = a == 0;
            let is_reserved_high = a >= 240;
            !(ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.is_multicast()
                || is_cgnat
                || is_this_network
                || is_reserved_high)
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

/// Whether `ask_stun_resolved` may fall back to Fake-IP addresses over TCP
/// when no real addresses succeeded (allowed only for manual server tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeIpFallback {
    Deny,
    AllowTcp,
}

/// Classification of what system DNS returned for a hostname and family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SystemDnsOutcome {
    /// At least one usable, non-fake address matching `destinations`.
    Real,
    /// Only Fake-IP (`198.18.0.0/15`) addresses matched `family`.
    FakeIp,
    /// Only private/local/non-public addresses matched `family` (never trigger DoH).
    LocalOnly,
    /// System DNS succeeded but returned no records for `family`.
    Empty,
    /// System DNS lookup failed or timed out.
    Error,
}

/// Outcome of querying a single DoH JSON endpoint.
#[derive(Debug, PartialEq, Eq)]
enum DohEndpointOutcome {
    /// Endpoint returned usable, public, non-fake addresses for the family.
    Addresses(Vec<IpAddr>),
    /// Endpoint returned HTTP 200 with valid JSON `Status == 0` and no matching
    /// `A`/`AAAA` answer records (authoritative NOERROR/NODATA).
    AuthoritativeEmpty,
    /// Endpoint failed, timed out, returned non-zero status, or yielded only unusable IPs.
    RetryNext,
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
    /// Resolves `server.host` for `family`, classifying system DNS outcomes and
    /// triggering sequential DoH only when appropriate (`FakeIp`, or `Empty`/`Error`
    /// when `family` is locally routable), never for `LocalOnly` answers.
    async fn resolve_host(
        &self,
        server: &StunServer,
        family: Family,
        deadline: Instant,
        permission: &dyn ProbePermission,
        fake_ip_fallback: FakeIpFallback,
    ) -> ResolvedTargets {
        if let Ok(ip) = server.host.parse::<IpAddr>() {
            let mut primary = Vec::new();
            let mut fake = Vec::new();
            if family.matches(ip) && (self.destinations)(ip) {
                let socket_addr = SocketAddr::new(ip, server.port);
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

        if let Some(cache) = &self.target_cache {
            let entry = cache.entry(&server.host, family);
            if let Some(cached) = entry.resolved.get() {
                return cached.at_port(server.port);
            }
            if !permission.allows_stun(server) {
                return ResolvedTargets {
                    primary: Vec::new(),
                    fake: Vec::new(),
                    can_fallback_doh: false,
                };
            }
            let ips = entry
                .resolved
                .get_or_init(|| {
                    self.resolve_hostname_ips(
                        server,
                        family,
                        deadline,
                        permission,
                        fake_ip_fallback,
                    )
                })
                .await;
            return ips.at_port(server.port);
        }

        self.resolve_hostname_ips(server, family, deadline, permission, fake_ip_fallback)
            .await
            .at_port(server.port)
    }

    async fn resolve_hostname_ips(
        &self,
        server: &StunServer,
        family: Family,
        deadline: Instant,
        permission: &dyn ProbePermission,
        fake_ip_fallback: FakeIpFallback,
    ) -> ResolvedHostIps {
        let host = server.host.as_str();
        let port = server.port;
        let resolved =
            tokio::time::timeout_at(deadline, tokio::net::lookup_host((host, port))).await;
        let mut primary = Vec::new();
        let mut fake = Vec::new();
        let mut saw_local = false;

        let outcome = match resolved {
            Ok(Ok(addresses)) => {
                for address in addresses {
                    let ip = address.ip();
                    if !family.matches(ip) {
                        continue;
                    }
                    if !(self.destinations)(ip) {
                        saw_local = true;
                        continue;
                    }
                    if is_fake_ip(ip) {
                        if fake.len() < MAX_RESOLVED_IPS && !fake.contains(&ip) {
                            fake.push(ip);
                        }
                    } else if primary.len() < MAX_RESOLVED_IPS && !primary.contains(&ip) {
                        primary.push(ip);
                    }
                }
                if !primary.is_empty() {
                    SystemDnsOutcome::Real
                } else if !fake.is_empty() {
                    SystemDnsOutcome::FakeIp
                } else if saw_local {
                    SystemDnsOutcome::LocalOnly
                } else {
                    SystemDnsOutcome::Empty
                }
            }
            Ok(Err(error)) => {
                tracing::debug!(host, ?family, %error, "the STUN server did not resolve");
                SystemDnsOutcome::Error
            }
            Err(_) => {
                tracing::debug!(host, ?family, "the STUN server did not resolve in time");
                SystemDnsOutcome::Error
            }
        };

        let routable = (self.family_routable)(family);
        let has_doh = !self.doh_endpoints.is_empty();

        match outcome {
            SystemDnsOutcome::Real => ResolvedHostIps {
                primary,
                fake,
                can_fallback_doh: has_doh && routable,
            },
            SystemDnsOutcome::LocalOnly => {
                tracing::debug!(
                    host,
                    ?family,
                    "system DNS returned only local/private addresses; skipping DoH"
                );
                ResolvedHostIps {
                    primary: Vec::new(),
                    fake: Vec::new(),
                    can_fallback_doh: false,
                }
            }
            SystemDnsOutcome::FakeIp | SystemDnsOutcome::Empty | SystemDnsOutcome::Error => {
                if !has_doh || !routable || !permission.allows_stun(server) {
                    return ResolvedHostIps {
                        primary,
                        fake,
                        can_fallback_doh: false,
                    };
                }
                if outcome == SystemDnsOutcome::FakeIp {
                    tracing::debug!(
                        host,
                        ?family,
                        "system DNS returned only Fake-IP addresses; resolving via DoH"
                    );
                }
                let doh_deadline =
                    if fake_ip_fallback == FakeIpFallback::AllowTcp && !fake.is_empty() {
                        deadline
                            .checked_sub(MIN_TCP_ATTEMPT_WINDOW)
                            .unwrap_or(deadline)
                    } else {
                        deadline
                    };
                let doh_ips = self.resolve_doh_ips(host, family, doh_deadline).await;
                ResolvedHostIps {
                    primary: doh_ips,
                    fake,
                    can_fallback_doh: false,
                }
            }
        }
    }

    /// Queries configured DoH JSON endpoints sequentially in priority order
    /// within `deadline`, stopping as soon as one returns usable addresses or an
    /// authoritative empty (`Status == 0`, no matching records) response.
    async fn resolve_doh_ips(&self, host: &str, family: Family, deadline: Instant) -> Vec<IpAddr> {
        let Some(client) = self.http.clone() else {
            return Vec::new();
        };
        if self.doh_endpoints.is_empty() {
            return Vec::new();
        }
        let doh_deadline = deadline.min(Instant::now() + DOH_TIMEOUT);
        for (index, endpoint) in self.doh_endpoints.iter().enumerate() {
            let now = Instant::now();
            let Some(remaining) = doh_deadline.checked_duration_since(now) else {
                break;
            };
            if remaining.is_zero() {
                break;
            }
            let remaining_endpoints = (self.doh_endpoints.len() - index) as u32;
            let timeout = (remaining / remaining_endpoints)
                .max(Duration::from_millis(250))
                .min(remaining);
            match query_doh_endpoint(&client, endpoint, host, family, self.destinations, timeout)
                .await
            {
                DohEndpointOutcome::Addresses(addresses) => return addresses,
                DohEndpointOutcome::AuthoritativeEmpty => return Vec::new(),
                DohEndpointOutcome::RetryNext => {}
            }
        }
        Vec::new()
    }

    #[cfg(test)]
    async fn resolve_doh(
        &self,
        host: &str,
        port: u16,
        family: Family,
        deadline: Instant,
    ) -> Vec<SocketAddr> {
        self.resolve_doh_ips(host, family, deadline)
            .await
            .into_iter()
            .map(|ip| SocketAddr::new(ip, port))
            .collect()
    }

    async fn resolve_fallback_doh(
        &self,
        server: &StunServer,
        family: Family,
        deadline: Instant,
    ) -> Vec<SocketAddr> {
        let ips = if let Some(cache) = &self.target_cache {
            let entry = cache.entry(&server.host, family);
            entry
                .fallback_doh
                .get_or_init(|| self.resolve_doh_ips(&server.host, family, deadline))
                .await
                .clone()
        } else {
            self.resolve_doh_ips(&server.host, family, deadline).await
        };
        ips.into_iter()
            .map(|ip| SocketAddr::new(ip, server.port))
            .collect()
    }

    async fn ask_stun(
        &self,
        server: &StunServer,
        transport: StunTransport,
        family: Family,
        permission: &dyn ProbePermission,
        first_send: Option<oneshot::Sender<()>>,
    ) -> StunAttempt {
        if !permission.allows_stun(server) {
            return StunAttempt::NoAddress;
        }
        let deadline = Instant::now() + STUN_DEADLINE;
        let targets = self
            .resolve_host(server, family, deadline, permission, FakeIpFallback::Deny)
            .await;
        self.ask_stun_resolved(
            server,
            transport,
            family,
            &targets,
            deadline,
            permission,
            FakeIpFallback::Deny,
            first_send,
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "probe state, permission, fallback policy, and send-signal channel are distinct concerns"
    )]
    async fn ask_stun_resolved(
        &self,
        server: &StunServer,
        transport: StunTransport,
        family: Family,
        targets: &ResolvedTargets,
        deadline: Instant,
        permission: &dyn ProbePermission,
        fake_ip_fallback: FakeIpFallback,
        mut first_send: Option<oneshot::Sender<()>>,
    ) -> StunAttempt {
        let mut tried_any = false;

        if !targets.primary.is_empty() {
            if !permission.allows_stun(server) {
                return StunAttempt::NoAddress;
            }
            tried_any = true;
            let primary_deadline = if targets.can_fallback_doh {
                deadline.min(Instant::now() + PRIMARY_WITH_FALLBACK_BUDGET)
            } else {
                deadline
            };
            match self
                .exchange_any(
                    &targets.primary,
                    transport,
                    family,
                    primary_deadline,
                    first_send.take(),
                )
                .await
            {
                Ok(attempt) => return attempt,
                Err(ExchangeError::Io { ref source }) if is_unroutable_error(source) => {
                    return StunAttempt::NoAnswer;
                }
                Err(_) => {}
            }
        }

        // If system DNS returned non-fake IPs that failed (for example due to
        // DNS poisoning, not a local unroutable route), try DoH once within the
        // remaining outer deadline.
        if targets.can_fallback_doh && permission.allows_stun(server) {
            let now = Instant::now();
            if let Some(remaining) = deadline.checked_duration_since(now)
                && !remaining.is_zero()
            {
                let doh_deadline = deadline
                    .checked_sub(MIN_FALLBACK_RETRY_WINDOW)
                    .filter(|reserved| *reserved > now)
                    .unwrap_or(deadline);
                let extra: Vec<SocketAddr> = self
                    .resolve_fallback_doh(server, family, doh_deadline)
                    .await
                    .into_iter()
                    .filter(|addr| !targets.primary.contains(addr))
                    .collect();
                if !extra.is_empty() && permission.allows_stun(server) {
                    tried_any = true;
                    if let Ok(attempt) = self
                        .exchange_any(&extra, transport, family, deadline, first_send.take())
                        .await
                    {
                        return attempt;
                    }
                }
            }
        }

        // Last-resort TCP fallback for Fake-IPs when DoH yielded nothing (only for
        // manual server tests while TUN is active and DoH is blocked; regular
        // probes must never connect to Fake-IPs).
        if fake_ip_fallback == FakeIpFallback::AllowTcp
            && targets.primary.is_empty()
            && !targets.fake.is_empty()
            && transport == StunTransport::Tcp
            && permission.allows_stun(server)
        {
            tried_any = true;
            if let Ok(attempt) = self
                .exchange_any(
                    &targets.fake,
                    transport,
                    family,
                    deadline,
                    first_send.take(),
                )
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
        first_send: Option<oneshot::Sender<()>>,
    ) -> Result<StunAttempt, ExchangeError> {
        let now = Instant::now();
        if deadline <= now {
            return Err(ExchangeError::Timeout);
        }
        let id = match TransactionId::random() {
            Ok(id) => id,
            Err(error) => {
                tracing::warn!(%error, "no randomness for a STUN transaction ID");
                return Err(ExchangeError::NoResponse);
            }
        };
        let started = Instant::now();
        let destinations = self.destinations;
        let exchange = async {
            match transport {
                StunTransport::Udp => {
                    exchange_udp(servers, family, id, destinations, first_send).await
                }
                StunTransport::Tcp => {
                    exchange_tcp(servers, family, id, destinations, deadline, first_send).await
                }
            }
        };
        match tokio::time::timeout_at(deadline, exchange).await {
            Ok(Ok(mapped)) => Ok(StunAttempt::Answered {
                address: mapped.ip(),
                latency_ms: started.elapsed().as_millis() as u64,
            }),
            Ok(Err(error)) => {
                tracing::debug!(?servers, ?transport, %error, "the STUN exchange failed");
                Err(error)
            }
            Err(_) => Err(ExchangeError::Timeout),
        }
    }

    async fn probe_stun_phase(
        &self,
        targets: &[StunServer],
        transport: StunTransport,
        family: Family,
        permission: &Arc<dyn ProbePermission>,
    ) -> Option<EgressAnswer<IpAddr>> {
        enum HedgeGate {
            Idle,
            WaitingForSend(oneshot::Receiver<()>),
            Armed(Pin<Box<Sleep>>),
        }

        let mut candidates = targets
            .iter()
            .filter(|target| supports_transport(target.capabilities, transport, family))
            .cloned()
            .collect::<VecDeque<_>>();
        let mut in_flight: JoinSet<Option<EgressAnswer<IpAddr>>> = JoinSet::new();
        let mut hedge_gate = HedgeGate::Idle;

        loop {
            while in_flight.is_empty() {
                let rx = self.spawn_next_permitted(
                    &mut candidates,
                    &mut in_flight,
                    transport,
                    family,
                    permission,
                )?;
                hedge_gate = HedgeGate::WaitingForSend(rx);
            }

            let can_hedge = !candidates.is_empty() && in_flight.len() < MAX_CONCURRENT_STUN;
            if can_hedge {
                match &mut hedge_gate {
                    HedgeGate::WaitingForSend(rx) => {
                        tokio::select! {
                            joined = in_flight.join_next() => {
                                if let Some(answer) = unwrap_probe_join(joined, &mut in_flight) {
                                    return Some(answer);
                                }
                                if in_flight.len() < MAX_CONCURRENT_STUN
                                    && let Some(new_rx) = self.spawn_next_permitted(
                                        &mut candidates,
                                        &mut in_flight,
                                        transport,
                                        family,
                                        permission,
                                    )
                                {
                                    hedge_gate = HedgeGate::WaitingForSend(new_rx);
                                }
                            }
                            sent = rx => {
                                hedge_gate = match sent {
                                    Ok(()) => HedgeGate::Armed(Box::pin(tokio::time::sleep(STUN_HEDGE_DELAY))),
                                    Err(_) => HedgeGate::Idle,
                                };
                            }
                        }
                    }
                    HedgeGate::Armed(sleep) => {
                        tokio::select! {
                            joined = in_flight.join_next() => {
                                if let Some(answer) = unwrap_probe_join(joined, &mut in_flight) {
                                    return Some(answer);
                                }
                                if in_flight.len() < MAX_CONCURRENT_STUN {
                                    let _ = self.spawn_next_permitted(
                                        &mut candidates,
                                        &mut in_flight,
                                        transport,
                                        family,
                                        permission,
                                    );
                                }
                            }
                            () = sleep => {
                                if let Some(new_rx) = self.spawn_next_permitted(
                                    &mut candidates,
                                    &mut in_flight,
                                    transport,
                                    family,
                                    permission,
                                ) {
                                    hedge_gate = HedgeGate::WaitingForSend(new_rx);
                                } else {
                                    hedge_gate = HedgeGate::Idle;
                                }
                            }
                        }
                    }
                    HedgeGate::Idle => {
                        let joined = in_flight.join_next().await;
                        if let Some(answer) = unwrap_probe_join(joined, &mut in_flight) {
                            return Some(answer);
                        }
                        if in_flight.len() < MAX_CONCURRENT_STUN
                            && let Some(new_rx) = self.spawn_next_permitted(
                                &mut candidates,
                                &mut in_flight,
                                transport,
                                family,
                                permission,
                            )
                        {
                            hedge_gate = HedgeGate::WaitingForSend(new_rx);
                        }
                    }
                }
            } else {
                let joined = in_flight.join_next().await;
                if let Some(answer) = unwrap_probe_join(joined, &mut in_flight) {
                    return Some(answer);
                }
                if in_flight.len() < MAX_CONCURRENT_STUN
                    && let Some(new_rx) = self.spawn_next_permitted(
                        &mut candidates,
                        &mut in_flight,
                        transport,
                        family,
                        permission,
                    )
                    && !matches!(hedge_gate, HedgeGate::Armed(_))
                {
                    hedge_gate = HedgeGate::WaitingForSend(new_rx);
                }
            }
        }
    }

    fn spawn_next_permitted(
        &self,
        candidates: &mut VecDeque<StunServer>,
        in_flight: &mut JoinSet<Option<EgressAnswer<IpAddr>>>,
        transport: StunTransport,
        family: Family,
        permission: &Arc<dyn ProbePermission>,
    ) -> Option<oneshot::Receiver<()>> {
        while let Some(target) = candidates.pop_front() {
            if permission.allows_stun(&target) {
                let (tx, rx) = oneshot::channel();
                self.spawn_stun_target(
                    in_flight,
                    target,
                    transport,
                    family,
                    Arc::clone(permission),
                    Some(tx),
                );
                return Some(rx);
            }
        }
        None
    }

    fn spawn_stun_target(
        &self,
        in_flight: &mut JoinSet<Option<EgressAnswer<IpAddr>>>,
        target: StunServer,
        transport: StunTransport,
        family: Family,
        permission: Arc<dyn ProbePermission>,
        first_send: Option<oneshot::Sender<()>>,
    ) {
        let runtime = self.clone();
        in_flight.spawn(async move {
            match runtime
                .ask_stun(&target, transport, family, permission.as_ref(), first_send)
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

    async fn test_family(
        &self,
        server: &StunServer,
        family: Family,
        deadline: Instant,
    ) -> (StunAttempt, StunAttempt) {
        let targets = self
            .resolve_host(
                server,
                family,
                deadline,
                &AllowAllProbe,
                FakeIpFallback::AllowTcp,
            )
            .await;
        tokio::join!(
            self.ask_stun_resolved(
                server,
                StunTransport::Udp,
                family,
                &targets,
                deadline,
                &AllowAllProbe,
                FakeIpFallback::AllowTcp,
                None,
            ),
            self.ask_stun_resolved(
                server,
                StunTransport::Tcp,
                family,
                &targets,
                deadline,
                &AllowAllProbe,
                FakeIpFallback::AllowTcp,
                None,
            ),
        )
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
    family: Family,
    destinations: fn(IpAddr) -> bool,
    timeout: Duration,
) -> DohEndpointOutcome {
    let Ok(mut url) = url::Url::parse(endpoint) else {
        return DohEndpointOutcome::RetryNext;
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
            Ok(ok) => match read_capped_body(ok, MAX_DOH_BODY_LEN).await {
                Ok(bytes) => match serde_json::from_slice(&bytes) {
                    Ok(json) => json,
                    Err(error) => {
                        tracing::debug!(endpoint, host, %error, "DoH response was not valid JSON");
                        return DohEndpointOutcome::RetryNext;
                    }
                },
                Err(error) => {
                    tracing::debug!(endpoint, host, %error, "DoH response body exceeded limit or failed");
                    return DohEndpointOutcome::RetryNext;
                }
            },
            Err(error) => {
                tracing::debug!(endpoint, host, %error, "DoH endpoint returned an error status");
                return DohEndpointOutcome::RetryNext;
            }
        },
        Err(error) => {
            tracing::debug!(endpoint, host, %error, "DoH request failed");
            return DohEndpointOutcome::RetryNext;
        }
    };
    parse_doh_ips(&body, family, destinations)
}

fn parse_doh_ips(
    body: &serde_json::Value,
    family: Family,
    destinations: fn(IpAddr) -> bool,
) -> DohEndpointOutcome {
    if body.get("Status").and_then(serde_json::Value::as_u64) != Some(0) {
        return DohEndpointOutcome::RetryNext;
    }
    let expected_type = family.dns_type_code();
    let Some(answers) = body.get("Answer").and_then(serde_json::Value::as_array) else {
        return DohEndpointOutcome::AuthoritativeEmpty;
    };
    let mut saw_matching_record = false;
    let mut addresses = Vec::new();
    for answer in answers {
        if answer.get("type").and_then(serde_json::Value::as_u64) != Some(expected_type) {
            continue;
        }
        saw_matching_record = true;
        let Some(ip) = answer
            .get("data")
            .and_then(serde_json::Value::as_str)
            .and_then(|raw| raw.parse::<IpAddr>().ok())
        else {
            continue;
        };
        if family.matches(ip) && destinations(ip) && !is_fake_ip(ip) && !addresses.contains(&ip) {
            addresses.push(ip);
            if addresses.len() >= MAX_RESOLVED_IPS {
                break;
            }
        }
    }
    if !addresses.is_empty() {
        DohEndpointOutcome::Addresses(addresses)
    } else if saw_matching_record {
        // Matching-type records existed but were all Fake-IP, private, or unparseable.
        DohEndpointOutcome::RetryNext
    } else {
        DohEndpointOutcome::AuthoritativeEmpty
    }
}

impl NetworkDirectEgressProbe {
    async fn probe(&self, sources: &ProbeSources, family: Family) -> Option<EgressAnswer<IpAddr>> {
        let runtime = self.runtime.with_target_cache();
        // Prefer UDP across all eligible servers first: behind TCP-only
        // transparent proxies, UDP to real IPs still reveals the direct egress
        // whereas TCP can hit the proxy exit. Fall back to TCP only when no
        // UDP server answers (e.g. firewalls blocking outbound UDP).
        if let Some(answer) = runtime
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
        if let Some(answer) = runtime
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
            if let Some(address) = runtime
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
        let runtime = self.runtime.with_target_cache();
        let server = StunServer {
            host: host.into(),
            port,
            enabled: true,
            capabilities: StunServerCapabilities::default(),
            builtin: false,
        };
        let deadline = Instant::now() + STUN_DEADLINE;
        let ((udp_ipv4, tcp_ipv4), (udp_ipv6, tcp_ipv6)) = tokio::join!(
            runtime.test_family(&server, Family::Ipv4, deadline),
            runtime.test_family(&server, Family::Ipv6, deadline),
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
    #[snafu(display("the STUN exchange timed out"))]
    Timeout,
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
    mut first_send: Option<oneshot::Sender<()>>,
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
                    if let Some(tx) = first_send.take() {
                        let _ = tx.send(());
                    }
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
    mut first_send: Option<oneshot::Sender<()>>,
) -> Result<SocketAddr, ExchangeError> {
    let mut last_error = ExchangeError::NoResponse;
    let mut saw_non_unroutable = false;
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
        if let Some(tx) = first_send.take() {
            let _ = tx.send(());
        }
        match tokio::time::timeout_at(
            attempt_deadline,
            exchange_tcp_once(server, family, id, destinations),
        )
        .await
        {
            Ok(Ok(mapped)) => return Ok(mapped),
            Ok(Err(error)) => {
                if !matches!(&error, ExchangeError::Io { source } if is_unroutable_error(source)) {
                    saw_non_unroutable = true;
                }
                last_error = error;
            }
            Err(_) => {
                saw_non_unroutable = true;
                last_error = ExchangeError::NoResponse;
            }
        }
    }
    if saw_non_unroutable
        && matches!(&last_error, ExchangeError::Io { source } if is_unroutable_error(source))
    {
        return Err(ExchangeError::NoResponse);
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

#[derive(Debug, snafu::Snafu)]
enum HttpBodyError {
    #[snafu(context(false), display("HTTP request error"))]
    Request { source: reqwest::Error },
    #[snafu(display("HTTP response body exceeded {limit} bytes"))]
    TooLarge { limit: usize },
    #[snafu(context(false), display("HTTP response was not valid UTF-8"))]
    InvalidUtf8 { source: std::string::FromUtf8Error },
}

async fn read_capped_body(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, HttpBodyError> {
    if let Some(len) = response.content_length()
        && len > limit as u64
    {
        return Err(HttpBodyError::TooLarge { limit });
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(HttpBodyError::TooLarge { limit });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<String, HttpBodyError> {
    let response = client
        .get(url)
        .timeout(ECHO_TIMEOUT)
        .send()
        .await?
        .error_for_status()?;
    let bytes = read_capped_body(response, MAX_ECHO_BODY_LEN).await?;
    Ok(String::from_utf8(bytes)?)
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
                family_routable: |_| true,
                http: build_direct_http_client(),
                doh_endpoints: Arc::from([]),
                target_cache: None,
            },
        }
    }

    fn loopback_probe_with_doh(endpoints: Vec<String>) -> NetworkDirectEgressProbe {
        loopback_probe_with_routable(endpoints, |_| true)
    }

    fn loopback_probe_with_routable(
        endpoints: Vec<String>,
        family_routable: fn(Family) -> bool,
    ) -> NetworkDirectEgressProbe {
        NetworkDirectEgressProbe {
            runtime: ProbeRuntime {
                destinations: ANYWHERE,
                family_routable,
                http: build_direct_http_client(),
                doh_endpoints: Arc::from(endpoints),
                target_cache: None,
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
            .ask_stun(
                &target(port, true, false),
                StunTransport::Udp,
                Family::Ipv4,
                &AllowAllProbe,
                None,
            )
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
            .ask_stun(
                &target(port, true, false),
                StunTransport::Udp,
                Family::Ipv4,
                &AllowAllProbe,
                None,
            )
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
            .ask_stun(
                &target(port, false, true),
                StunTransport::Tcp,
                Family::Ipv4,
                &AllowAllProbe,
                None,
            )
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
            .ask_stun(
                &target(3478, true, false),
                StunTransport::Udp,
                Family::Ipv6,
                &AllowAllProbe,
                None,
            )
            .await;

        assert_eq!(attempt, StunAttempt::NoAddress);
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_udp_server_is_no_answer() {
        let (port, _) = silent_udp_server().await;
        let attempt = loopback_probe()
            .runtime
            .ask_stun(
                &target(port, true, false),
                StunTransport::Udp,
                Family::Ipv4,
                &AllowAllProbe,
                None,
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
            None,
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
        let server = StunServer {
            host: "stun.test.invalid".into(),
            ..target(udp, true, false)
        };
        let attempt = probe
            .runtime
            .ask_stun(
                &server,
                StunTransport::Udp,
                Family::Ipv4,
                &AllowAllProbe,
                None,
            )
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
            .ask_stun(
                &target(banner, false, true),
                StunTransport::Tcp,
                Family::Ipv4,
                &AllowAllProbe,
                None,
            )
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
            .ask_stun(
                &target(port, true, false),
                StunTransport::Udp,
                Family::Ipv4,
                &AllowAllProbe,
                None,
            )
            .await;

        assert_eq!(attempt, StunAttempt::NoAnswer);
    }

    #[tokio::test]
    async fn probe_never_connects_to_fake_ip_over_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp_port = listener.local_addr().unwrap().port();
        let tcp_hits = Arc::new(AtomicUsize::new(0));
        let hits = tcp_hits.clone();
        tokio::spawn(async move {
            loop {
                let (mut stream, from) = listener.accept().await.unwrap();
                hits.fetch_add(1, Ordering::Relaxed);
                let mut request = [0; HEADER_LEN];
                if stream.read_exact(&mut request).await.is_err() {
                    continue;
                }
                let response = binding_response(&request, from);
                let _ = stream.write_all(&response).await;
            }
        });

        let targets = ResolvedTargets {
            primary: Vec::new(),
            fake: vec![SocketAddr::new(Ipv4Addr::LOCALHOST.into(), tcp_port)],
            can_fallback_doh: false,
        };
        let deadline = Instant::now() + STUN_DEADLINE;
        let probe = loopback_probe();
        let server = target(tcp_port, false, true);

        let denied = probe
            .runtime
            .ask_stun_resolved(
                &server,
                StunTransport::Tcp,
                Family::Ipv4,
                &targets,
                deadline,
                &AllowAllProbe,
                FakeIpFallback::Deny,
                None,
            )
            .await;
        assert_eq!(denied, StunAttempt::NoAddress);
        assert_eq!(tcp_hits.load(Ordering::Relaxed), 0);

        let allowed = probe
            .runtime
            .ask_stun_resolved(
                &server,
                StunTransport::Tcp,
                Family::Ipv4,
                &targets,
                deadline,
                &AllowAllProbe,
                FakeIpFallback::AllowTcp,
                None,
            )
            .await;
        assert!(matches!(allowed, StunAttempt::Answered { .. }));
        assert_eq!(tcp_hits.load(Ordering::Relaxed), 1);

        let fake_server = StunServer {
            host: "198.18.0.1".into(),
            port: tcp_port,
            enabled: true,
            builtin: false,
            capabilities: StunServerCapabilities {
                udp: StunFamilies::default(),
                tcp: StunFamilies {
                    ipv4: true,
                    ipv6: false,
                },
            },
        };
        let sources = sources(vec![fake_server], AllowingPorts(vec![tcp_port]));
        assert_eq!(probe.ipv4(&sources).await, None);
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
            "100.64.0.1",
            "100.127.255.254",
            "0.0.0.0",
            "0.1.2.3",
            "240.0.0.1",
            "254.0.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:192.168.1.1",
            "::ffff:100.64.0.1",
        ] {
            assert!(!is_public(local.parse().unwrap()), "{local}");
        }
        for public in [
            "203.0.113.7",
            "198.18.0.1",
            "100.63.255.255",
            "100.128.0.0",
            "2001:db8::7",
        ] {
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
            parse_doh_ips(&body, Family::Ipv4, is_public),
            DohEndpointOutcome::Addresses(vec!["162.159.207.0".parse().unwrap()])
        );
        assert_eq!(
            parse_doh_ips(&body, Family::Ipv6, is_public),
            DohEndpointOutcome::Addresses(vec!["2606:4700:49::".parse().unwrap()])
        );

        let nxdomain = serde_json::json!({
            "Status": 3,
            "Answer": [{ "type": 1, "data": "162.159.207.0" }]
        });
        assert_eq!(
            parse_doh_ips(&nxdomain, Family::Ipv4, is_public),
            DohEndpointOutcome::RetryNext
        );

        let missing_status = serde_json::json!({
            "Answer": [{ "type": 1, "data": "162.159.207.0" }]
        });
        assert_eq!(
            parse_doh_ips(&missing_status, Family::Ipv4, is_public),
            DohEndpointOutcome::RetryNext
        );

        let authoritative_empty = serde_json::json!({
            "Status": 0,
            "Answer": []
        });
        assert_eq!(
            parse_doh_ips(&authoritative_empty, Family::Ipv4, is_public),
            DohEndpointOutcome::AuthoritativeEmpty
        );
    }

    #[tokio::test]
    async fn direct_http_client_refuses_redirects_and_caps_response_bodies() {
        let probe = loopback_probe();
        let base_url = serve(
            Router::new()
                .route("/target", get(|| async { "203.0.113.7" }))
                .route(
                    "/redirect",
                    get(|| async { axum::response::Redirect::temporary("/target") }),
                )
                .route(
                    "/large-echo",
                    get(|| async { "203.0.113.7\n".repeat(40) }),
                )
                .route(
                    "/large-doh",
                    get(|| async move {
                        let padding = "x".repeat(MAX_DOH_BODY_LEN + 1024);
                        format!(
                            r#"{{"Status":0,"Comment":"{padding}","Answer":[{{"type":1,"data":"127.0.0.1"}}]}}"#
                        )
                    }),
                ),
        )
        .await;

        assert_eq!(
            probe.runtime.echo(&format!("{base_url}redirect")).await,
            None
        );
        assert_eq!(
            probe.runtime.echo(&format!("{base_url}large-echo")).await,
            None
        );

        let doh_probe = loopback_probe_with_doh(vec![format!("{base_url}large-doh")]);
        let doh_ips = doh_probe
            .runtime
            .resolve_doh(
                "stun.test.invalid",
                3478,
                Family::Ipv4,
                Instant::now() + STUN_DEADLINE,
            )
            .await;
        assert!(doh_ips.is_empty());
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
    async fn local_only_dns_never_triggers_doh() {
        let doh_hits = Arc::new(AtomicUsize::new(0));
        let hits = doh_hits.clone();
        let doh_url = serve(Router::new().route(
            "/resolve",
            get(move || {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::Relaxed);
                    axum::Json(serde_json::json!({
                        "Status": 0,
                        "Answer": [{ "type": 1, "data": "203.0.113.7" }]
                    }))
                }
            }),
        ))
        .await;

        let probe = NetworkDirectEgressProbe {
            runtime: ProbeRuntime {
                destinations: is_public,
                family_routable: |_| true,
                http: build_direct_http_client(),
                doh_endpoints: Arc::from([format!("{doh_url}resolve")]),
                target_cache: None,
            },
        };
        let server = StunServer {
            host: "localhost".into(),
            ..target(3478, true, false)
        };
        let attempt = probe
            .runtime
            .ask_stun(
                &server,
                StunTransport::Udp,
                Family::Ipv4,
                &AllowAllProbe,
                None,
            )
            .await;

        assert_eq!(attempt, StunAttempt::NoAddress);
        assert_eq!(doh_hits.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn unroutable_family_skips_doh_on_empty_or_error() {
        let doh_hits = Arc::new(AtomicUsize::new(0));
        let hits = doh_hits.clone();
        let doh_url = serve(Router::new().route(
            "/resolve",
            get(move || {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::Relaxed);
                    axum::Json(serde_json::json!({
                        "Status": 0,
                        "Answer": [{ "type": 28, "data": "2001:db8::1" }]
                    }))
                }
            }),
        ))
        .await;

        let probe = loopback_probe_with_routable(vec![format!("{doh_url}resolve")], |family| {
            family == Family::Ipv4
        });
        let server = StunServer {
            host: "stun.test.invalid".into(),
            ..target(3478, true, false)
        };
        let attempt = probe
            .runtime
            .ask_stun(
                &server,
                StunTransport::Udp,
                Family::Ipv6,
                &AllowAllProbe,
                None,
            )
            .await;

        assert_eq!(attempt, StunAttempt::NoAddress);
        assert_eq!(doh_hits.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn unroutable_socket_errors_are_classified_accurately() {
        for kind in [
            std::io::ErrorKind::NetworkUnreachable,
            std::io::ErrorKind::HostUnreachable,
            std::io::ErrorKind::AddrNotAvailable,
        ] {
            assert!(is_unroutable_error(&std::io::Error::from(kind)), "{kind:?}");
        }
        for kind in [
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::ConnectionRefused,
            std::io::ErrorKind::ConnectionReset,
        ] {
            assert!(
                !is_unroutable_error(&std::io::Error::from(kind)),
                "{kind:?}"
            );
        }
    }

    #[tokio::test]
    async fn resolve_doh_queries_endpoints_sequentially_and_stops_on_authoritative_empty() {
        let log = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let make_router = |name: &'static str, status: StatusCode, body: serde_json::Value| {
            let log = log.clone();
            Router::new().route(
                "/resolve",
                get(move || {
                    let log = log.clone();
                    let body = body.clone();
                    async move {
                        log.lock().unwrap().push(name);
                        (status, axum::Json(body))
                    }
                }),
            )
        };

        let e1 = serve(make_router(
            "e1",
            StatusCode::OK,
            serde_json::json!({ "Status": 0, "Answer": [] }),
        ))
        .await;
        let e2 = serve(make_router(
            "e2",
            StatusCode::OK,
            serde_json::json!({ "Status": 0, "Answer": [{ "type": 1, "data": "127.0.0.1" }] }),
        ))
        .await;
        let e3 = serve(make_router(
            "e3",
            StatusCode::OK,
            serde_json::json!({ "Status": 0, "Answer": [{ "type": 1, "data": "127.0.0.1" }] }),
        ))
        .await;

        let probe_auth_empty = loopback_probe_with_doh(vec![
            format!("{e1}resolve"),
            format!("{e2}resolve"),
            format!("{e3}resolve"),
        ]);
        let ips = probe_auth_empty
            .runtime
            .resolve_doh(
                "stun.test.invalid",
                3478,
                Family::Ipv4,
                Instant::now() + STUN_DEADLINE,
            )
            .await;
        assert!(ips.is_empty());
        assert_eq!(*log.lock().unwrap(), vec!["e1"]);

        log.lock().unwrap().clear();
        let fail1 = serve(make_router(
            "fail1",
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({}),
        ))
        .await;
        let servfail2 = serve(make_router(
            "servfail2",
            StatusCode::OK,
            serde_json::json!({ "Status": 2, "Answer": [] }),
        ))
        .await;
        let ok3 = serve(make_router(
            "ok3",
            StatusCode::OK,
            serde_json::json!({ "Status": 0, "Answer": [{ "type": 1, "data": "127.0.0.1" }] }),
        ))
        .await;

        let probe_seq = loopback_probe_with_doh(vec![
            format!("{fail1}resolve"),
            format!("{servfail2}resolve"),
            format!("{ok3}resolve"),
        ]);
        let ips = probe_seq
            .runtime
            .resolve_doh(
                "stun.test.invalid",
                3478,
                Family::Ipv4,
                Instant::now() + STUN_DEADLINE,
            )
            .await;
        assert_eq!(ips, vec!["127.0.0.1:3478".parse().unwrap()]);
        assert_eq!(*log.lock().unwrap(), vec!["fail1", "servfail2", "ok3"]);
    }

    #[tokio::test]
    async fn ask_stun_never_exceeds_stun_deadline_even_with_fallback_doh() {
        let (silent_port, _) = silent_udp_server().await;
        let doh_url = serve(Router::new().route(
            "/resolve",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(4)).await;
                axum::Json(serde_json::json!({
                    "Status": 0,
                    "Answer": [{ "type": 1, "data": "127.0.0.1" }]
                }))
            }),
        ))
        .await;
        let probe = loopback_probe_with_doh(vec![format!("{doh_url}resolve")]);
        let server = target(silent_port, true, false);
        let targets = ResolvedTargets {
            primary: vec![SocketAddr::new(Ipv4Addr::LOCALHOST.into(), silent_port)],
            fake: Vec::new(),
            can_fallback_doh: true,
        };

        let started = Instant::now();
        let attempt = probe
            .runtime
            .ask_stun_resolved(
                &server,
                StunTransport::Udp,
                Family::Ipv4,
                &targets,
                started + STUN_DEADLINE,
                &AllowAllProbe,
                FakeIpFallback::Deny,
                None,
            )
            .await;

        assert_eq!(attempt, StunAttempt::NoAnswer);
        assert!(
            started.elapsed() < STUN_DEADLINE + Duration::from_millis(350),
            "exceeded STUN_DEADLINE: {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn mid_probe_permission_revocation_stops_fallback_doh_and_retry() {
        use std::sync::atomic::AtomicBool;

        struct FlippingPermission {
            allowed: Arc<AtomicBool>,
            flip_after_checks: usize,
            checks: AtomicUsize,
        }
        impl ProbePermission for FlippingPermission {
            fn allows_stun(&self, _: &StunServer) -> bool {
                let count = self.checks.fetch_add(1, Ordering::Relaxed) + 1;
                if count > self.flip_after_checks {
                    self.allowed.store(false, Ordering::Relaxed);
                }
                self.allowed.load(Ordering::Relaxed)
            }
            fn allows_lookup(&self, _: IpLookupProvider) -> bool {
                false
            }
        }

        // Case 1: permission revoked after primary fails, before fallback DoH.
        let bad_port = udp_stun_server_returning("198.18.0.42:3478".parse().unwrap()).await;
        let doh_hits = Arc::new(AtomicUsize::new(0));
        let hits_clone = doh_hits.clone();
        let doh_url = serve(Router::new().route(
            "/resolve",
            get(move || {
                let hits = hits_clone.clone();
                async move {
                    hits.fetch_add(1, Ordering::Relaxed);
                    axum::Json(serde_json::json!({
                        "Status": 0,
                        "Answer": [{ "type": 1, "data": "127.0.0.1" }]
                    }))
                }
            }),
        ))
        .await;

        let probe = loopback_probe_with_doh(vec![format!("{doh_url}resolve")]);
        let server = target(bad_port, true, false);
        let targets = ResolvedTargets {
            primary: vec![SocketAddr::new(Ipv4Addr::LOCALHOST.into(), bad_port)],
            fake: Vec::new(),
            can_fallback_doh: true,
        };
        let perm1 = FlippingPermission {
            allowed: Arc::new(AtomicBool::new(true)),
            flip_after_checks: 1,
            checks: AtomicUsize::new(0),
        };
        let attempt = probe
            .runtime
            .ask_stun_resolved(
                &server,
                StunTransport::Udp,
                Family::Ipv4,
                &targets,
                Instant::now() + STUN_DEADLINE,
                &perm1,
                FakeIpFallback::Deny,
                None,
            )
            .await;
        assert_eq!(attempt, StunAttempt::NoAnswer);
        assert_eq!(doh_hits.load(Ordering::Relaxed), 0);

        // Case 2: permission revoked by the time fallback DoH finishes, before retry exchange.
        let retry_port = tcp_stun_server().await;
        let allowed_flag = Arc::new(AtomicBool::new(true));
        let flag_in_doh = allowed_flag.clone();
        let doh_url2 = serve(Router::new().route(
            "/resolve",
            get(move || {
                flag_in_doh.store(false, Ordering::Relaxed);
                async {
                    axum::Json(serde_json::json!({
                        "Status": 0,
                        "Answer": [{ "type": 1, "data": "127.0.0.1" }]
                    }))
                }
            }),
        ))
        .await;
        let probe2 = loopback_probe_with_doh(vec![format!("{doh_url2}resolve")]);
        let banner_port = tcp_http_banner_server().await;
        let targets2 = ResolvedTargets {
            primary: vec![SocketAddr::new(Ipv4Addr::LOCALHOST.into(), banner_port)],
            fake: Vec::new(),
            can_fallback_doh: true,
        };
        let perm2 = FlippingPermission {
            allowed: allowed_flag,
            flip_after_checks: usize::MAX,
            checks: AtomicUsize::new(0),
        };
        let attempt2 = probe2
            .runtime
            .ask_stun_resolved(
                &target(retry_port, false, true),
                StunTransport::Tcp,
                Family::Ipv4,
                &targets2,
                Instant::now() + STUN_DEADLINE,
                &perm2,
                FakeIpFallback::Deny,
                None,
            )
            .await;
        assert_eq!(attempt2, StunAttempt::NoAnswer);
    }

    #[tokio::test]
    async fn probe_stun_phase_backfills_max_concurrent_slots_immediately() {
        async fn delayed_closing_tcp_server(delay: Duration) -> u16 {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            tokio::spawn(async move {
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    tokio::spawn(async move {
                        tokio::time::sleep(delay).await;
                        drop(stream);
                    });
                }
            });
            port
        }

        // S1 closes at 950ms (after S2 at 400ms and S3 at 800ms are both in-flight,
        // filling MAX_CONCURRENT_STUN == 3). S4 must backfill immediately at ~950ms
        // rather than waiting another STUN_HEDGE_DELAY or S2/S3's 2500ms deadline.
        let s1 = delayed_closing_tcp_server(Duration::from_millis(950)).await;
        let s2 = delayed_closing_tcp_server(Duration::from_secs(5)).await;
        let s3 = delayed_closing_tcp_server(Duration::from_secs(5)).await;
        let s4 = tcp_stun_server().await;

        let sources = sources(
            vec![
                target(s1, false, true),
                target(s2, false, true),
                target(s3, false, true),
                target(s4, false, true),
            ],
            AllowingPorts(vec![s1, s2, s3, s4]),
        );

        let started = Instant::now();
        let answer = loopback_probe().ipv4(&sources).await;
        let elapsed = started.elapsed();

        assert_eq!(
            answer,
            Some(EgressAnswer {
                address: Ipv4Addr::LOCALHOST,
                source: EgressSource::Stun {
                    host: LOCALHOST.into(),
                    port: s4,
                    transport: StunTransport::Tcp,
                },
            })
        );
        assert!(
            elapsed < Duration::from_millis(1300),
            "expected immediate backfill when a slot freed from MAX_CONCURRENT_STUN, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn hedged_probing_delays_hedge_timer_until_first_send() {
        let (silent, _) = silent_udp_server().await;
        let fast = udp_stun_server(0).await;
        let doh_url = serve(Router::new().route(
            "/resolve",
            get(|| async {
                tokio::time::sleep(Duration::from_millis(500)).await;
                axum::Json(serde_json::json!({
                    "Status": 0,
                    "Answer": [{ "type": 1, "data": "127.0.0.1" }]
                }))
            }),
        ))
        .await;

        let slow_primary = StunServer {
            host: "slow-dns.test.invalid".into(),
            ..target(silent, true, false)
        };
        let sources = sources(
            vec![slow_primary, target(fast, true, false)],
            AllowingPorts(vec![silent, fast]),
        );

        let started = Instant::now();
        let answer = loopback_probe_with_doh(vec![format!("{doh_url}resolve")])
            .ipv4(&sources)
            .await;
        let elapsed = started.elapsed();

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
            elapsed >= Duration::from_millis(800) && elapsed < STUN_DEADLINE,
            "hedge timer should start only after primary DoH + first send (~500ms + 400ms), elapsed: {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn probe_caches_dns_and_doh_across_udp_and_tcp_phases() {
        let tcp_port = tcp_stun_server().await;
        let doh_hits = Arc::new(AtomicUsize::new(0));
        let hits = doh_hits.clone();
        let doh_url = serve(Router::new().route(
            "/resolve",
            get(move || {
                let hits = hits.clone();
                async move {
                    hits.fetch_add(1, Ordering::Relaxed);
                    axum::Json(serde_json::json!({
                        "Status": 0,
                        "Answer": [{ "type": 1, "data": "127.0.0.1" }]
                    }))
                }
            }),
        ))
        .await;

        let server = StunServer {
            host: "cached-udp-tcp.test.invalid".into(),
            ..target(tcp_port, true, true)
        };
        let sources = sources(vec![server], AllowingPorts(vec![tcp_port]));
        let answer = loopback_probe_with_doh(vec![format!("{doh_url}resolve")])
            .ipv4(&sources)
            .await;

        assert!(matches!(
            answer,
            Some(EgressAnswer {
                source: EgressSource::Stun {
                    transport: StunTransport::Tcp,
                    ..
                },
                ..
            })
        ));
        assert_eq!(doh_hits.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn test_stun_server_decouples_ipv4_and_ipv6_and_shares_resolution_between_udp_and_tcp() {
        let tcp_port = tcp_stun_server().await;
        let v4_doh_hits = Arc::new(AtomicUsize::new(0));
        let hits = v4_doh_hits.clone();
        let doh_url = serve(Router::new().route(
            "/resolve",
            get(
                move |Query(params): Query<std::collections::HashMap<String, String>>| {
                    let hits = hits.clone();
                    async move {
                        if params.get("type").map(String::as_str) == Some("A") {
                            hits.fetch_add(1, Ordering::Relaxed);
                            axum::Json(serde_json::json!({
                                "Status": 0,
                                "Answer": [{ "type": 1, "data": "127.0.0.1" }]
                            }))
                        } else {
                            tokio::time::sleep(Duration::from_millis(600)).await;
                            axum::Json(serde_json::json!({ "Status": 0, "Answer": [] }))
                        }
                    }
                },
            ),
        ))
        .await;

        let result = loopback_probe_with_doh(vec![format!("{doh_url}resolve")])
            .test_stun_server("decoupled.test.invalid", tcp_port)
            .await;

        assert!(matches!(result.tcp_ipv4, StunAttempt::Answered { .. }));
        assert_eq!(result.udp_ipv6, StunAttempt::NoAddress);
        assert_eq!(result.tcp_ipv6, StunAttempt::NoAddress);
        assert_eq!(v4_doh_hits.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    #[ignore = "requires live internet access"]
    async fn live_default_stun_probe_discovers_egress() {
        let probe = NetworkDirectEgressProbe::default();
        let sources = ProbeSources {
            stun: nyanpasu_config::application::default_stun_servers()
                .into_iter()
                .filter(|server| server.enabled)
                .collect(),
            lookups: Vec::new(),
            permission: Arc::new(AllowAllProbe),
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
