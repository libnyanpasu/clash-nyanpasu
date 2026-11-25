use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::{Router, extract::Query, http::StatusCode, routing::get};
use nyanpasu_config::application::{
    IpLookupProvider, StunFamilies, StunServer, StunServerCapabilities,
};
use nyanpasu_contracts::net::{
    DirectEgressProbe, EgressAnswer, EgressSource, ProbePermission, ProbeSources, StunAttempt,
    StunTransport,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, UdpSocket},
    time::Instant,
};

use super::{
    AllowAllProbe, FakeIpFallback, Family, NetworkDirectEgressProbe, ProbeRuntime, ResolvedTargets,
    STUN_DEADLINE, SystemDnsOutcome, classify_dns_addresses,
    doh::{DohEndpointOutcome, MAX_DOH_BODY_LEN, build_direct_http_client, parse_doh_ips},
    is_fake_ip, is_public,
    stun::{DecodeError, HEADER_LEN, TransactionId, tests::binding_response},
    transport::{
        ExchangeError, MAX_MESSAGE_LEN, WSAEMSGSIZE, exchange_tcp, exchange_tcp_once, exchange_udp,
        is_transient_udp_recv_error, is_udp_msgsize, is_unroutable_error,
    },
};

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

#[tokio::test]
async fn a_silent_udp_server_is_no_answer() {
    let (port, hits) = silent_udp_server().await;
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
    assert!(
        hits.load(Ordering::Relaxed) >= 3,
        "expected UDP retransmissions before timeout, got {} hits",
        hits.load(Ordering::Relaxed)
    );
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

#[tokio::test]
async fn hedged_probing_switches_to_second_server_before_first_deadline() {
    let (silent, silent_hits) = silent_udp_server().await;
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
    assert!(silent_hits.load(Ordering::Relaxed) >= 1);
    assert!(
        started.elapsed() < STUN_DEADLINE,
        "hedged probe should not wait for full primary deadline: {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn multi_ip_udp_fails_over_to_working_address() {
    let (silent, silent_hits) = silent_udp_server().await;
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
    assert!(silent_hits.load(Ordering::Relaxed) >= 1);
}

#[tokio::test]
async fn multi_ip_udp_tries_all_distinct_ips_on_fast_cadence() {
    let (silent1, hits1) = silent_udp_server().await;
    let (silent2, hits2) = silent_udp_server().await;
    let working = udp_stun_server(0).await;
    let servers = [
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), silent1),
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), silent2),
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), working),
    ];

    let started = Instant::now();
    let mapped = exchange_udp(
        &servers,
        Family::Ipv4,
        TransactionId::random().unwrap(),
        ANYWHERE,
        None,
    )
    .await
    .unwrap();
    let elapsed = started.elapsed();

    assert_eq!(mapped.ip(), IpAddr::from(Ipv4Addr::LOCALHOST));
    assert!(hits1.load(Ordering::Relaxed) >= 1);
    assert!(hits2.load(Ordering::Relaxed) >= 1);
    assert!(
        elapsed < Duration::from_millis(600),
        "expected fast initial failover across all 3 IPs, took {elapsed:?}"
    );
}

#[tokio::test]
async fn multi_ip_tcp_fails_over_to_working_address() {
    let closed = closed_port();
    let banner = tcp_http_banner_server().await;
    let working = tcp_stun_server().await;
    let servers = [
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), closed),
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), banner),
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), working),
    ];

    let mapped = exchange_tcp(
        &servers,
        Family::Ipv4,
        TransactionId::random().unwrap(),
        ANYWHERE,
        Instant::now() + STUN_DEADLINE,
        None,
    )
    .await
    .unwrap();

    assert_eq!(mapped.ip(), IpAddr::from(Ipv4Addr::LOCALHOST));
}

#[tokio::test]
async fn tcp_rejects_oversized_stun_body_header_before_reading() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; HEADER_LEN];
        stream.read_exact(&mut request).await.unwrap();
        let mut header = [0; HEADER_LEN];
        header[0..2].copy_from_slice(&0x0101u16.to_be_bytes());
        header[2..4].copy_from_slice(&2048u16.to_be_bytes());
        header[4..20].copy_from_slice(&request[4..20]);
        stream.write_all(&header).await.unwrap();
        // Keep stream open without sending the declared 2048 bytes.
        tokio::time::sleep(Duration::from_secs(5)).await;
    });

    let started = Instant::now();
    let err = exchange_tcp_once(
        SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port),
        Family::Ipv4,
        TransactionId::random().unwrap(),
        ANYWHERE,
    )
    .await
    .unwrap_err();

    assert!(
        matches!(
            err,
            ExchangeError::Decode {
                source: DecodeError::TooLarge { len: 2048 }
            }
        ),
        "{err:?}"
    );
    assert!(started.elapsed() < Duration::from_millis(500));
}

#[tokio::test]
async fn doh_fallback_resolves_when_system_dns_fails() {
    let udp = udp_stun_server(0).await;
    let doh_url = serve(Router::new().route(
        "/resolve",
        get(|Query(params): Query<HashMap<String, String>>| async move {
            let record_type = params.get("type").map(String::as_str);
            let body = match record_type {
                Some("A") => serde_json::json!({
                    "Status": 0,
                    "Answer": [{ "type": 1, "data": "127.0.0.1" }]
                }),
                _ => serde_json::json!({ "Status": 0, "Answer": [] }),
            };
            axum::Json(body)
        }),
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

#[test]
fn classify_dns_addresses_handles_all_outcomes() {
    let real_v4: IpAddr = "162.159.207.0".parse().unwrap();
    let fake_v4: IpAddr = "198.18.0.5".parse().unwrap();
    let local_v4: IpAddr = "192.168.1.1".parse().unwrap();

    let (primary, fake, outcome) =
        classify_dns_addresses([real_v4].into_iter(), Family::Ipv4, is_public);
    assert_eq!(primary, vec![real_v4]);
    assert!(fake.is_empty());
    assert_eq!(outcome, SystemDnsOutcome::Real);

    let (primary, fake, outcome) =
        classify_dns_addresses([fake_v4].into_iter(), Family::Ipv4, is_public);
    assert!(primary.is_empty());
    assert_eq!(fake, vec![fake_v4]);
    assert_eq!(outcome, SystemDnsOutcome::FakeIp);

    let (primary, fake, outcome) =
        classify_dns_addresses([local_v4].into_iter(), Family::Ipv4, is_public);
    assert!(primary.is_empty());
    assert!(fake.is_empty());
    assert_eq!(outcome, SystemDnsOutcome::LocalOnly);

    let (primary, fake, outcome) =
        classify_dns_addresses([real_v4].into_iter(), Family::Ipv6, is_public);
    assert!(primary.is_empty());
    assert!(fake.is_empty());
    assert_eq!(outcome, SystemDnsOutcome::Empty);
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
async fn fake_ip_mapped_address_is_rejected_and_fast_fails_for_single_target() {
    let port = udp_stun_server_returning("198.18.0.42:3478".parse().unwrap()).await;
    let probe = loopback_probe();
    let started = Instant::now();

    let err = exchange_udp(
        &[SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port)],
        Family::Ipv4,
        TransactionId::random().unwrap(),
        ANYWHERE,
        None,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, ExchangeError::InvalidMappedAddress),
        "{err:?}"
    );
    assert!(started.elapsed() < Duration::from_millis(300));

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

#[test]
fn transient_udp_errors_classify_wsaemsgsize_and_resets() {
    let msgsize = std::io::Error::from_raw_os_error(WSAEMSGSIZE);
    assert!(is_udp_msgsize(&msgsize));
    assert!(is_transient_udp_recv_error(&msgsize));

    let reset = std::io::Error::from(std::io::ErrorKind::ConnectionReset);
    assert!(!is_udp_msgsize(&reset));
    assert!(is_transient_udp_recv_error(&reset));

    let refused = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
    assert!(is_transient_udp_recv_error(&refused));
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
        .resolve_doh_ips(
            "stun.test.invalid",
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
        .resolve_doh_ips(
            "stun.test.invalid",
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
        .resolve_doh_ips(
            "stun.test.invalid",
            Family::Ipv4,
            Instant::now() + STUN_DEADLINE,
        )
        .await;
    assert_eq!(ips, vec!["127.0.0.1".parse::<IpAddr>().unwrap()]);
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
        get(move |Query(params): Query<HashMap<String, String>>| {
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
        }),
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
