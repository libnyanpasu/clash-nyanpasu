//! Direct-route egress probe, isolated from system and self proxies: STUN
//! servers first (UDP phase, then TCP fallback phase with staggered hedging),
//! then the accepted HTTP lookup services.

mod doh;
mod stun;
#[cfg(test)]
mod tests;
mod transport;

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
    sync::{OnceCell, oneshot},
    task::JoinSet,
    time::{Instant, Sleep},
};

use self::{
    doh::{DEFAULT_DOH_ENDPOINTS, build_direct_http_client, fetch, resolve_doh_ips},
    stun::TransactionId,
    transport::{
        ExchangeError, MIN_TCP_ATTEMPT_WINDOW, exchange_tcp, exchange_udp, is_family_routable,
        is_unroutable_error,
    },
};

/// A STUN server that does not resolve and answer within this is skipped, so a long
/// server list stays usable when several are unreachable.
const STUN_DEADLINE: Duration = Duration::from_millis(2500);
/// Delay before hedging the next STUN server in priority order after the
/// current primary has sent its first STUN packet/attempt.
const STUN_HEDGE_DELAY: Duration = Duration::from_millis(400);
/// Maximum STUN servers probed concurrently within a single transport phase.
const MAX_CONCURRENT_STUN: usize = 3;
/// Maximum distinct resolved addresses tried per STUN host and address family.
const MAX_RESOLVED_IPS: usize = 3;
/// Budget for the initial primary exchange when fallback DoH may still run
/// inside the same [`STUN_DEADLINE`].
const PRIMARY_WITH_FALLBACK_BUDGET: Duration = Duration::from_millis(1500);
/// Minimum slice reserved for a retry exchange after fallback DoH within [`STUN_DEADLINE`].
const MIN_FALLBACK_RETRY_WINDOW: Duration = Duration::from_millis(500);

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

type TargetCacheMap = HashMap<(String, Family), Arc<CachedFamilyTargets>>;

/// Per-probe cache of resolved IPs keyed by `(lowercase_host, family)` so
/// UDP and TCP phases (and concurrent UDP/TCP checks in `test_stun_server`)
/// avoid repeating system DNS or DoH lookups.
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

fn classify_dns_addresses(
    addresses: impl Iterator<Item = IpAddr>,
    family: Family,
    destinations: fn(IpAddr) -> bool,
) -> (Vec<IpAddr>, Vec<IpAddr>, SystemDnsOutcome) {
    let mut primary = Vec::new();
    let mut fake = Vec::new();
    let mut saw_local = false;

    for ip in addresses {
        if !family.matches(ip) {
            continue;
        }
        if !destinations(ip) {
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

    let outcome = if !primary.is_empty() {
        SystemDnsOutcome::Real
    } else if !fake.is_empty() {
        SystemDnsOutcome::FakeIp
    } else if saw_local {
        SystemDnsOutcome::LocalOnly
    } else {
        SystemDnsOutcome::Empty
    };
    (primary, fake, outcome)
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
        let (primary, fake, outcome) = match resolved {
            Ok(Ok(addresses)) => {
                classify_dns_addresses(addresses.map(|addr| addr.ip()), family, self.destinations)
            }
            Ok(Err(error)) => {
                tracing::debug!(host, ?family, %error, "the STUN server did not resolve");
                (Vec::new(), Vec::new(), SystemDnsOutcome::Error)
            }
            Err(_) => {
                tracing::debug!(host, ?family, "the STUN server did not resolve in time");
                (Vec::new(), Vec::new(), SystemDnsOutcome::Error)
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

    async fn resolve_doh_ips(&self, host: &str, family: Family, deadline: Instant) -> Vec<IpAddr> {
        let Some(client) = self.http.as_ref() else {
            return Vec::new();
        };
        resolve_doh_ips(
            client,
            &self.doh_endpoints,
            host,
            family,
            self.destinations,
            deadline,
        )
        .await
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
