# Egress IP discovery without HTTP lookup services

Research date: 2026-11-06. Question: can the app learn its public (egress) address,
for TCP and for UDP, without asking an HTTP echo service such as `api.ip.sb`? This
report covers STUN, DNS echo, and the GeoIP databases the app already reads. It
records findings and a recommendation; it is not an implementation plan.

## Current state in this repository

| Feature                             | Where                                               | Transport                                                      | Route                                                           |
| ----------------------------------- | --------------------------------------------------- | -------------------------------------------------------------- | --------------------------------------------------------------- |
| Direct egress (IPv4/IPv6 only)      | `backend/nyanpasu-core/src/client/direct_egress.rs` | HTTPS GET `ipv4.ddnspod.com` / `ipv6.ddnspod.com`, 5 s timeout | `.no_proxy()`; skipped in TUN mode (`DirectEgress::TunEnabled`) |
| Proxy egress + geo (`get_ipsb_asn`) | `backend/nyanpasu-core/src/utils/net.rs`            | HTTPS GET `api.ip.sb/geoip`, raw JSON                          | Through the core's mixed-port as an HTTP proxy                  |
| GeoIP / ASN lookup                  | `backend/nyanpasu-geodata` (`maxminddb` 0.32)       | In-process mmdb / `.dat` / `.metadb` reads                     | No network                                                      |

Observations:

- `DirectEgressProbe` is already a port with a mockable trait. A STUN probe is
  another implementation of the same port, not a new subsystem.
- `get_ipsb_asn` has RPC bindings but no rendered consumer in the frontend.
- `nyanpasu-geodata` already has `IpIndex` (country) and `AsnIndex::lookup`
  (ASN number and organization). Only country tagging for traffic regions uses
  it in the app; `AsnIndex` is used only in tests.
- No STUN, WebRTC, or SOCKS5 client crate is in the backend. All app HTTP
  traffic that goes through the core uses `reqwest::Proxy` against an HTTP proxy
  URL, which cannot carry UDP.

## What STUN gives

STUN ([RFC 8489](https://www.rfc-editor.org/rfc/rfc8489)) is a request/response
protocol. A client sends a 20-byte Binding request with a random 96-bit
transaction ID; the server answers with `XOR-MAPPED-ADDRESS`, the source IP and
port it saw. The same exchange runs over UDP, TCP, TLS-over-TCP, and DTLS
(default port 3478, 5349 for TLS/DTLS). A Binding exchange is one round trip; the
codec is small (encode one header, parse a TLV list, XOR the address with the
magic cookie and transaction ID).

So STUN answers "which address did this flow leave from", per transport:

- **UDP egress**: Binding over UDP. This is the one thing an HTTP echo cannot
  measure at all.
- **TCP egress**: Binding over TCP (or TLS). Equivalent in result to an HTTP
  echo, without HTTP headers, cookies, or a user agent.
- **IPv4 vs IPv6**: query an A and an AAAA address of the server separately.
- **Mapped port**: comparing the mapped port from two server addresses tells
  whether the NAT (or the proxy's UDP relay) keeps one mapping per local socket
  ("endpoint-independent mapping", [RFC 4787](https://www.rfc-editor.org/rfc/rfc4787)).
  Full NAT type classification ([RFC 5780](https://www.rfc-editor.org/rfc/rfc5780))
  needs a server that answers `OTHER-ADDRESS` and honours `CHANGE-REQUEST`;
  few public servers do.

STUN returns only an address. It carries no country, ASN, or ISP; that part must
come from a local database (see below).

## Routing: direct egress vs proxy egress

In a proxy client "egress" means two different things, and STUN behaves
differently for each.

### Direct (no proxy)

A STUN probe from a socket that bypasses the core gives the machine's own public
address — what `direct_egress.rs` measures today with HTTP. In TUN mode the
app's own sockets are captured and routed by rule, so the result may be a proxy
exit; the existing `TunEnabled` guard applies equally to STUN.

### Through the core (proxy egress)

To measure the exit of a proxy node the probe must enter the core:

- **TCP**: SOCKS5 CONNECT (or HTTP CONNECT) to the STUN server's TCP port via
  mixed-port. Works with any proxy protocol that carries TCP.
- **UDP**: SOCKS5 UDP ASSOCIATE on mixed-port/socks-port, then send the
  Binding request inside SOCKS5 UDP datagrams. This needs a SOCKS5 client (a
  small hand-rolled one is enough). mihomo's socks and mixed listeners have a
  per-listener `udp` switch (default on); the UDP relay path itself was not
  read in source for this report. A server that does not relay UDP gives a
  timeout, which is itself a useful answer ("UDP is not relayed").
- **UDP can silently go DIRECT**: in rule mode mihomo skips a matched proxy
  whose `udp` flag is off and keeps matching later rules, ending at DIRECT if
  nothing else matches (`tunnel/tunnel.go`). A UDP probe through the core can
  therefore return the user's **real** address while looking like a proxy
  result. The probe must compare the answer against the known direct address,
  or check the connection's chain in the core's connections API, before
  labelling it a proxy exit.
- **Which node**: traffic through mixed-port is routed by rules, so the result
  is the exit of whatever rule matches the STUN server's address. To test a
  specific node or group, a dedicated mihomo listener bound to that proxy
  (`listeners:` with a per-listener `proxy:`) gives a deterministic path without
  touching rules. This is a runtime-config change and should be treated as its
  own design decision.

TCP and UDP egress addresses can legitimately differ — for example providers
that relay UDP through a different exit, chained proxies, or load-balanced
groups. Measuring both is the main functional gain over the current HTTP check.

## Live probe on the development machine

A one-off Python probe (Binding request over UDP and TCP, IPv4 and IPv6, 3 s
timeout, two UDP attempts) against seven public servers, run on 2026-11-06 from
a macOS machine with an active TUN interface:

| Server                        | UDP v4/v6 | TCP v4/v6                               |
| ----------------------------- | --------- | --------------------------------------- |
| `stun.l.google.com:19302`     | timeout   | timeout                                 |
| `stun.cloudflare.com:3478`    | timeout   | timeout                                 |
| `global.stun.twilio.com:3478` | timeout   | timeout                                 |
| `stun.nextcloud.com:3478`     | timeout   | **OK**, ~0.7 s, `OTHER-ADDRESS` present |
| `stun.miwifi.com:3478`        | timeout   | timeout                                 |
| `stun.qq.com:3478`            | timeout   | timeout                                 |
| `stun.chat.bilibili.com:3478` | timeout   | timeout                                 |

Reading the result:

- Cloudflare documents `stun.cloudflare.com` as 3478/UDP only, so its TCP
  timeouts are expected; no public statement says whether Google's endpoint
  accepts TCP.
- The machine was behind a TUN proxy. Both the IPv4 and the IPv6 TCP probes
  returned the **same IPv4 mapped address**, so the IPv6 attempt was carried by
  the proxy and exited over IPv4. Under TUN a "direct" STUN probe measures the
  proxy exit, as predicted above.
- Every UDP probe timed out, including servers that only speak UDP. The likely
  cause is the TUN path or the selected node not relaying UDP; this is exactly
  the case a UDP probe would surface to users, but it also shows that UDP
  results need a clear "no answer" state rather than an error.
- The probe did not isolate network effects from server availability, so it
  says nothing about the general uptime of these servers. It does show that a
  single hard-coded server is not enough.

## Privacy

- **Exposure is the same as an HTTP echo**: the STUN operator sees the source
  address and port and the time of the request. It sees less else: no HTTP
  headers, user agent, cookies, or TLS SNI/fingerprint. A STUN request can
  include a `SOFTWARE` attribute; the client should not send one.
- **Leak risk is the real concern**: a probe meant to measure the proxy exit
  that accidentally goes direct reveals the user's real address to a third
  party. This is the same class of problem as the WebRTC STUN leak in browsers,
  and mihomo's UDP fallback to DIRECT (see above) makes it a concrete risk, not
  a theoretical one.
  Each probe must state its intended route (direct or through the core) and the
  implementation must enforce it (direct = bypass proxies; proxy = only via the
  core's SOCKS5 port, never falling back to a direct socket).
- **Plain STUN is unauthenticated and unencrypted**. An on-path observer can
  read or forge the answer; the random transaction ID only protects against
  off-path spoofing. `FINGERPRINT` detects corruption, not forgery. STUN over
  TLS fixes this but few public servers offer it, and it is TCP-only. The result
  is display-only data, so this is acceptable as long as it is not used for any
  security decision.
- **Choice of operator**: a list that mixes global and mainland-China operators
  (Google, Cloudflare, Tencent, Xiaomi) sends the address to whichever answers.
  The server list should be user-visible and editable, and the default should
  be documented in the UI or settings help.
- **Frequency**: probe on demand or on a node switch, not on a timer; repeated
  probes build a timeline of addresses at the operator.

## Stability and reliability

- **No SLA**: public STUN servers are run for the operator's own WebRTC/VoIP
  products. They can change, rate-limit, or disappear without notice
  (`stun.qq.com`-style hostnames in community lists are often stale).
- **Regional reachability**: Google endpoints are blocked in mainland China,
  and some domestic servers are unreachable or slow from abroad. Direct probes
  need a server reachable without a proxy; proxy probes need one reachable from
  the proxy's exit.
- **Transport coverage differs**: many servers are UDP-only (Cloudflare's is
  documented as such), so TCP needs a separate server list (the live probe found
  only one TCP-capable server among seven from this network).
- **No usage policy**: Google publishes no usage terms for `stun.l.google.com`;
  treat it and similar endpoints as best-effort.
- **UDP loss**: UDP needs retransmission (RFC 8489, UDP only: initial RTO of at
  least 500 ms, doubling, up to 7 requests). A UI check should cap total time at a few seconds rather
  than following the full RFC schedule.
- **Proxy/UDP behaviour**: middleboxes, captive portals, and some proxy
  providers block UDP 3478 or STUN generally. A timeout must not be shown as
  "no internet".
- **Robust pattern**: query 2–3 servers in parallel per transport and family,
  accept the first valid answer whose transaction ID matches, and report
  disagreement (different addresses) instead of picking one silently.

## GeoIP and ASN databases

The bundled databases cannot discover the egress address: they map an address
to a country or ASN, and the address must come from somewhere else. They can
replace the **enrichment** half of `api.ip.sb/geoip` (country, ASN, organization)
once STUN supplies the address:

- `nyanpasu-geodata` already loads country (`Country.mmdb`, `geoip.dat`,
  `geoip.metadb`) and ASN (`ASN.mmdb`) data and exposes `AsnIndex::lookup`.
  No new dependency is needed.
- Coverage depends on what the user's core directory contains. MetaCubeX
  `meta-rules-dat` publishes `country.mmdb`, `geoip.metadb`, `GeoLite2-ASN.mmdb`,
  and a `country-lite.mmdb` that holds only CN and JP data; with the lite file
  the app cannot name the country of an arbitrary exit; the UI should
  show "unknown" rather than guess. `ASN.mmdb` may be missing entirely.
- Accuracy for proxy and VPS exits is limited: datacenter and anycast
  addresses are often geolocated to the registrant's country, not the exit's.
  `api.ip.sb` has the same limitation, so this is not a regression, but the UI
  should label it as database-derived.
- No city, ISP name beyond the ASN organization, or "is proxy/hosting" flags.
- Licensing: the `meta-rules-dat` README states no license per file, and the
  upstream of `GeoLite2-ASN.mmdb` is implied by its name, not documented. If it
  is MaxMind GeoLite2 data, MaxMind's license and attribution terms apply. The
  app only reads files the core already downloaded; check the license before
  bundling or redistributing any of them.

## Other options considered

| Option                                                                                                                                | TCP    | UDP | Geo info          | Verdict                                                                                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------- | ------ | --- | ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| HTTP echo (current)                                                                                                                   | Yes    | No  | From the service  | Works; heaviest metadata; no UDP                                                                                                                                                                       |
| STUN over UDP/TCP                                                                                                                     | Yes    | Yes | No (use local DB) | Recommended                                                                                                                                                                                            |
| DNS echo (`o-o.myaddr.l.google.com` TXT via `ns1.google.com`, `whoami.cloudflare` CH TXT via 1.1.1.1, `myip.opendns.com` via OpenDNS) | Partly | Yes | No                | Fallback only; none is officially documented; through the core, mihomo's DNS handling (fake-ip, hijack) can intercept the query, and the answer can be the resolver's address rather than the client's |
| Read the node's server address from the profile                                                                                       | —      | —   | —                 | Gives the entry, not the exit; wrong for relays and multi-hop                                                                                                                                          |
| Ask the core                                                                                                                          | —      | —   | —                 | mihomo exposes no egress-address API; the delay test is HTTP-only                                                                                                                                      |

## Recommendation

1. **Direct egress**: add a STUN implementation of `DirectEgressProbe`
   (UDP Binding to 2–3 servers, IPv4 and IPv6 separately). Keep the HTTP
   implementation as a fallback when no STUN server answers. Keep the TUN guard.
2. **Proxy egress**: implement TCP via SOCKS5 CONNECT and UDP via SOCKS5 UDP
   ASSOCIATE to the core's mixed-port, returning per-transport results with
   explicit "UDP not relayed" and "UDP went DIRECT" states. Decide separately whether per-node tests
   use a dedicated bound listener.
3. **Enrichment**: look up country and ASN locally through `nyanpasu-geodata`;
   retire `get_ipsb_asn` once nothing needs its extra fields.
4. **Placement**: the STUN codec and result types are pure code in
   `nyanpasu-core`; the socket and SOCKS5 transport are adapters behind the
   existing port, following [Architecture and ownership](../development/architecture.md).
   Use a maintained STUN codec crate or a ~150-line hand-rolled encoder/parser;
   either is small enough.
5. **Server list**: configurable, with separate UDP and TCP defaults and
   regional coverage; no probes on a timer.

Open questions for the implementation plan:

- Which default servers to ship for mainland-China and global users, and
  whether the list lives in settings or a remote-updated resource.
- Whether to show NAT mapping behaviour (useful for games and P2P) or only
  addresses.
- Whether a per-node probe through a bound listener is worth the runtime-config
  change.

## Sources

- STUN: [RFC 8489](https://www.rfc-editor.org/rfc/rfc8489),
  [RFC 5780](https://www.rfc-editor.org/rfc/rfc5780),
  [RFC 4787](https://www.rfc-editor.org/rfc/rfc4787)
- Cloudflare STUN/TURN ports: <https://developers.cloudflare.com/realtime/turn/>
- mihomo listener `proxy:`/`udp` options:
  [docs/config.yaml](https://github.com/MetaCubeX/mihomo/blob/Meta/docs/config.yaml);
  UDP rule fallback: [tunnel/tunnel.go](https://github.com/MetaCubeX/mihomo/blob/Meta/tunnel/tunnel.go);
  SOCKS UDP ASSOCIATE handling: [listener/socks/tcp.go](https://github.com/MetaCubeX/mihomo/blob/Meta/listener/socks/tcp.go)
- GeoIP files: [MetaCubeX/meta-rules-dat](https://github.com/MetaCubeX/meta-rules-dat)
- DNS echo: community descriptions only, for example
  [this post](https://dev.to/adityathebe/a-handy-way-to-know-your-public-ip-address-with-dns-servers-4nmn);
  Google's [Public DNS troubleshooting](https://developers.google.com/speed/public-dns/docs/troubleshooting)
  does not document it.
