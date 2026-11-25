//! Unconnected UDP and multi-IP TCP STUN Binding exchanges and route checks.

use std::{
    net::{IpAddr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpStream, UdpSocket},
    sync::oneshot,
    time::Instant,
};

use super::{
    Family, is_fake_ip,
    stun::{self, HEADER_LEN, TransactionId},
};

/// Fast cadence used while trying each distinct resolved IP once before
/// switching to exponential retransmission backoff.
const UDP_IP_FAILOVER_DELAY: Duration = Duration::from_millis(150);
/// Delays before each UDP retransmission after all resolved IPs have been tried
/// once, cut short by [`super::STUN_DEADLINE`].
const UDP_RETRANSMISSIONS: [Duration; 3] = [
    Duration::from_millis(400),
    Duration::from_millis(800),
    Duration::from_millis(1200),
];
/// Minimum per-address TCP connect/exchange slice when a host resolves to several IPs.
pub(super) const MIN_TCP_ATTEMPT_WINDOW: Duration = Duration::from_millis(800);
/// Large enough for any Binding response a server sends.
pub(super) const MAX_MESSAGE_LEN: usize = 1024;
/// Windows socket error returned when an unconnected UDP datagram exceeds `buffer`.
pub(super) const WSAEMSGSIZE: i32 = 10040;

#[derive(Debug, snafu::Snafu)]
pub(super) enum ExchangeError {
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

pub(super) fn is_unroutable_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NetworkUnreachable
            | std::io::ErrorKind::HostUnreachable
            | std::io::ErrorKind::AddrNotAvailable
    )
}

/// Checks whether the local host has a route for `family` without sending any
/// packet, so IPv4-only hosts do not fire DoH lookups for IPv6.
pub(super) fn is_family_routable(family: Family) -> bool {
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

pub(super) fn is_udp_msgsize(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(WSAEMSGSIZE)
}

pub(super) fn is_transient_udp_recv_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionRefused
    ) || is_udp_msgsize(error)
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

/// Sends the request on an unconnected UDP socket, trying each distinct resolved
/// IP once on a fast initial cadence before backing off exponentially across
/// retransmissions. Using an unconnected socket accepts replies from multi-homed
/// STUN servers and avoids aborting failover on a single ICMP error.
pub(super) async fn exchange_udp(
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
    let mut unprobed_ips = servers.len();

    loop {
        // Send to the next target in round-robin order; if all targets reject
        // `send_to` (e.g. no IPv6 route), fail fast instead of waiting out the deadline.
        let mut sent_offset = None;
        let mut last_send_error = None;
        for offset in 0..servers.len() {
            let target = servers[(attempt + offset) % servers.len()];
            match socket.send_to(&request, target).await {
                Ok(_) => {
                    sent_offset = Some(offset);
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
        let Some(offset) = sent_offset else {
            return Err(last_send_error
                .map(ExchangeError::from)
                .unwrap_or(ExchangeError::NoResponse));
        };
        attempt += offset + 1;
        unprobed_ips = unprobed_ips.saturating_sub(offset + 1);

        let wait = if unprobed_ips > 0 {
            Some(UDP_IP_FAILOVER_DELAY)
        } else {
            delays.next()
        };
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
                    if let Ok(mapped) = stun::mapped_address(&buffer[..len], id) {
                        match accept_mapped_address(mapped, family, destinations) {
                            Ok(valid) => return Ok(valid),
                            Err(error) if servers.len() == 1 => return Err(error),
                            Err(_) => {}
                        }
                    }
                }
                Err(error) if is_transient_udp_recv_error(&error) => {
                    if is_udp_msgsize(&error) {
                        continue;
                    }
                    if servers.len() > 1 {
                        // A late ICMP Port Unreachable (`WSAECONNRESET`) or
                        // `ConnectionRefused` from a dead IP must not abort a
                        // healthy in-flight target.
                        if unprobed_ips > 0 && resend_at.is_some() {
                            break;
                        }
                        continue;
                    }
                    if resend_at.is_some() {
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
pub(super) async fn exchange_tcp(
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

pub(super) async fn exchange_tcp_once(
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
