# Linux transparent proxy: first version

## Scope

The first version targets Linux desktops and Linux gateways using Mihomo or
Mihomo Alpha. OpenWrt and LuCI are future hosts, not implementation requirements.
macOS and Windows retain their existing system-proxy and TUN paths.

Two independent managed listeners are available: `redir-port` for TCP REDIRECT
and `tproxy-port` for TCP/UDP TPROXY. Configuring a listener does not itself enable
traffic capture. Capture selects one mechanism and explicitly selects local
applications and/or incoming gateway interfaces. It is disabled by default.
Existing profile-provided listeners remain compatible when no managed port is
configured. Managed values take precedence after profile transforms.

This is a Linux service-mode feature. The GUI remains unprivileged; the service
that owns the core also owns system-network changes. Falling back to a local core
must never leave interception targeting the old service listener.

## Compatibility

| Core             | REDIRECT                            | TPROXY                                                  | First version   |
| ---------------- | ----------------------------------- | ------------------------------------------------------- | --------------- |
| Mihomo / Alpha   | TCP, Linux and macOS                | TCP/UDP, Linux                                          | Linux supported |
| Clash-rs / Alpha | TCP, Linux, build feature           | TCP/UDP, Linux, build feature                           | Deferred        |
| Meow / Alpha     | Different listener/config semantics | TCP REDIRECT plus separately configured IPv4 UDP TPROXY | Deferred        |
| Clash Premium    | Version-specific legacy behavior    | Requires binary verification                            | Deferred        |

Do not infer compatibility from a shared YAML key. Meow's top-level
`tproxy-port` automatically manages local TCP REDIRECT rules; it is not Mihomo's
kernel TPROXY contract. No automatic key substitution is performed.

Sources: [Mihomo ports](https://wiki.metacubex.one/config/inbound/port/),
[Clash-rs support](https://github.com/Watfaq/clash-rs#-protocol-support),
[Meow gateway](https://github.com/meow-rs/meow-rs/blob/main/docs/openwrt.md).

## Ownership and application

`nyanpasu-config` owns configuration and deterministic validation. The runtime
builder materializes managed listeners and the outbound routing mark. The
application's committed effect projection supplies capture intent and confirmed
ports. The execution-host adapter forwards a typed request over protected local
IPC; it never accepts shell commands, executable paths or nft source from the UI.
The privileged service owns reconciliation, network resources and observed status.

Capture is allowed only for a running Mihomo instance whose applied revision and
listener match the request. Required runtime application votes precede source
commit; firewall application is a post-commit effect. Its failure reports degraded
state and does not claim that the configuration was rolled back. The UI must show
whether capture is active and expose diagnostic errors.

The service serializes install, replacement and cleanup. A stopped, failed or
replaced core invalidates capture. Normal service shutdown removes interception
before network resources are released. Cleanup failures retain diagnostic state
and must not be reported as successful removal. Process death can bypass orderly
cleanup; startup recovery and Linux runtime tests must verify residual resources.

## Packet paths

REDIRECT uses NAT hooks, TCP only. Gateway rules match only the selected incoming
interfaces. Local rules run in OUTPUT and exclude the service/core's root UID,
loopback, local destinations, reserved/local networks and marked core egress.
Root-owned local applications are outside local capture in this first version;
running the whole GUI as root is not required.

TPROXY gateway rules run in PREROUTING. Local applications are marked in a route
OUTPUT chain, policy-routed to `lo`, then intercepted in PREROUTING. This local
path requires real Linux TCP/UDP validation. TPROXY does not change destination
addresses; policy routing delivers marked packets to a local route.

Use distinct capture and bypass mark bits (`0x10000` and `0x20000`). Preserve
unrelated mark bits. The private routing table and rule priority must be checked
for collisions before modification. The service must never flush the global
ruleset or delete routes/rules it does not own. Missing modules, tools, permissions,
interfaces or resource conflicts are explicit failures.

IPv6 capture is opt-in and requires corresponding IPv6 rules and local routes.
IPv4 success is not evidence of IPv6 success. A later INPUT filter can reject a
TPROXY-delivered packet even after PREROUTING accepts it; compatibility with an
existing default-deny firewall must be tested and diagnosed, not bypassed globally.

Sources: [Linux TPROXY](https://docs.kernel.org/networking/tproxy.html),
[nftables packet marks](https://wiki.nftables.org/wiki-nftables/index.php/Setting_packet_metainformation).

## DNS and interaction boundaries

This version does not reconfigure the system resolver, dnsmasq or encrypted DNS.
Domain-based routing requires the existing Mihomo DNS/Fake-IP or sniffer setup.
Fake-IP ranges must remain eligible for interception. Local/LAN bypass does not
include the default `198.18.0.0/16` fake-IP pool. Custom fake-IP pools need explicit
validation against bypass networks before claiming reliable domain routing.

TUN and managed capture cannot target the same traffic simultaneously; enabling
capture while TUN is enabled is rejected. Listener-only settings may coexist with
TUN because they do not install system rules. Firewall/VPN policy marks and network
managers remain externally owned; detected conflicts prevent activation.

## Using the first version

On Linux, select Mihomo or Mihomo Alpha, enable service mode, and disable TUN.
The host needs `nft`, `ip` and `ss`; TPROXY also requires kernel support for
`NFT_TPROXY` and policy routing. Tool and permission failures are reported by the
service rather than silently changing the host installation.
In Clash settings choose REDIRECT for TCP capture or TPROXY for TCP and UDP.
The corresponding managed listener is initialized to port 7893 or 7894 when it
has not been configured. Optional listeners can also be configured with capture
disabled, for an externally managed firewall.

Select local capture for ordinary non-root desktop applications. Add incoming
interface names, separated by commas, for gateway traffic; local capture and
gateway interfaces can be enabled together. Leave IPv6 disabled until the host
supports IPv6 routing and the selected listener. Check the observed capture status
and diagnostic message after applying a setting. A saved setting alone does not
prove firewall installation succeeded.

Gateway capture requires the actual transparent listener to bind a wildcard
address for each requested address family. Check Mihomo's `allow-lan` and
`bind-address` settings if the service rejects a loopback-only listener. This
version does not automatically broaden those settings because they also affect
other inbound ports. IPv6 capture similarly rejects an IPv4-only listener.

The status operation is desktop-only. Existing authenticated configuration
mutations retain their shared transport and apply the same validation and
committed-effect path; they cannot supply executable commands or raw firewall
rules. Network reconciliation itself stays on protected service IPC.

## Commit and verification plan

1. Design and configuration: defaults, optional port patch behavior, validation,
   dual-protocol reservation and guarded runtime materialization.
2. Privileged service: typed IPC, Linux rules, conflict checks, compensation,
   revision-aware lifetime and cleanup tests.
3. Desktop integration: committed effects, service-only capability, generated RPC
   bindings, Linux settings, status and errors.

Each implementation commit must build with its dependencies. Runtime-submodule
changes are committed in that repository before the parent records its gitlink.
Keep protocol/build version alignment explicit; an old downloaded service must
report unsupported capability rather than silently succeeding.

The MVP uses an explicitly approved local development pin for the runtime
submodule. `prepare:check` still downloads the preceding service release; that
binary does not implement capture. Build the service from the pinned sources on
Linux (`cargo build --manifest-path backend/nyanpasu-runtime/Cargo.toml -p
nyanpasu-service --release`) and install that binary through the existing service
installation procedure. Publish the runtime and restore a released-tag pin before
shipping the feature. No release or push is part of this implementation.

Unit tests cover validation, rule scope, conflict handling and failed-install
cleanup with injected adapters. Repository boundary gates, Rust checks and frontend
type checks cover integration. Real Linux acceptance must cover local and gateway
TCP/UDP, IPv4/IPv6, original destinations, core egress bypass, Fake-IP, competing
firewall INPUT policy, restart/crash, disabling, interface changes and resource
ownership. Tests run in disposable network namespaces or a VM. macOS unit tests
and compilation do not establish Linux network support.

### Initial Linux packet checks

The generated rules were exercised on 2026-10-08 in disposable privileged Alpine
3.22 containers, using nftables 1.1.3 and the OrbStack Linux 7.0.14 kernel.
Local clients ran as UID 1000; gateway clients ran in a separate network namespace
connected through a veth pair. Transparent test listeners checked TCP original
destinations and UDP original-destination ancillary data.

| Mechanism    | Local IPv4 | Gateway IPv4 | Local IPv6 | Gateway IPv6 |
| ------------ | ---------- | ------------ | ---------- | ------------ |
| REDIRECT TCP | Passed     | Passed       | Passed     | Passed       |
| TPROXY TCP   | Passed     | Passed       | Passed     | Passed       |
| TPROXY UDP   | Passed     | Passed       | Passed     | Passed       |

IPv4 tests targeted the default fake-IP pool. IPv6 was explicitly enabled in the
container interfaces before testing. Additional local IPv4 checks used the actual
Mihomo v1.19.32 binary and a DIRECT origin in a separate network namespace:
REDIRECT TCP and TPROXY TCP/UDP completed through the core successfully.
These checks validate packet paths and core listeners, not a complete installed
service deployment. Full service installation,
upgrades, abrupt process death, existing host firewalls and custom fake-IP pools
remain acceptance work for the Linux host environment.

## Future hosts

The application contract describes intent and observed outcome, not fw4/procd or
LuCI. Future OpenWrt integration can replace the host adapter while retaining
configuration and validation. No router packages, ubus methods or LuCI screens are
introduced by this first version.
