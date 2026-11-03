# OpenWrt / ImmortalWrt MVP

Status: paused by user, 2026-10-08. This is a downstream product definition,
not an implementation contract. Wait for the related shared-backend and
shell/Tauri separation work to complete, then reassess against main when the
user explicitly resumes. See the [roadmap](openwrt-roadmap.md) for the resume gate.

## Product outcome

Provide a lightweight native LuCI interface over a separate Rust server that
uses the same shared application as the desktop shell. The router form is
client/server: `nyanpasu-server` owns the application, LuCI drives
`nyanpasu-cli`, and the CLI talks to the server over a local socket. LuCI uses
rpcd sessions and ACLs to authorize each CLI invocation. Do not run the desktop
Tauri binary or embed its React distribution on the router.

The first usable loop is:

1. Open Services → Nyanpasu in authenticated LuCI.
2. Import a small local YAML profile, optionally with ordered JavaScript/Lua
   transforms, and choose the selected profile.
3. Build and inspect the resulting Mihomo configuration through the accepted
   shared runtime pipeline; expose validation failures clearly.
4. Start/stop Mihomo through the router lifecycle adapter and inspect actual
   process/configuration state rather than an optimistic running flag.
5. Restart the server and retain committed profiles, script order and selection;
   reconcile runtime/process state according to the agreed recovery policy.

This loop is the end-to-end MVP. Headless composition and a CLI status probe
are feasibility milestones, not a claim that the MVP is delivered.

## Architecture after upstream completion

```text
Desktop shell / Tauri adapters          nyanpasu-server / OpenWrt target crate
                 \                         /
                  nyanpasu-rpc: neutral commands and registry
                               |
                  nyanpasu-core: NyanpasuClient and shared APIs
                  profile/config/runtime owners and consumed ports
                               |
                  domain/config/transaction primitives

LuCI -> rpcd sessions + ACL -> nyanpasu-cli -> local socket -> nyanpasu-server
procd supervises the server; the server controls the core process through an
injected router control adapter
```

The shared facade and application graph live in the core package family
described in the [target ownership](application-platform-extraction.md#target-ownership).
The remaining facade/actor migration must settle before OpenWrt resumes.
Concrete infrastructure stays behind consumed ports and is injected by shells.

Use the reviewed upstream package boundaries and construction APIs rather than
freezing a second application architecture during the pause. Shared application
code owns decisions, transactions and actor state. Router code owns concrete
paths, process/native transport adapters and host composition. Presentation,
HTTP assets, Tauri events/windows/tray and desktop helper installation are not
mandatory server dependencies.

Reuse shared persistence/materialization, runtime receipts and recovery. Do not
introduce a router-only profile document, journal, StateActor or duplicate facade
to bypass incomplete extraction. Headless startup must not substitute successful
no-op business effects for unsupported desktop capabilities.

## Runtime and failure behavior

Use real shared JS/Lua execution, source provenance, ordering and validation.
Failed source persistence preserves committed state. Runtime preparation/apply
errors retain the shared transaction distinction between rejected changes and
committed changes with degradation/recovery; a transport timeout is not proof
of cancellation or safe retry.

Expose source revision, published candidate and observed applied state according
to the accepted upstream models. The UI must distinguish an available candidate
from proof that the process applied it. Choose Build/Apply/Start semantics only
after mapping to those models; the MVP does not impose a router-only stop/start
transaction convention on shared use cases.

MVP networking remains loopback-only, without TUN, DNS interception or
redir/tproxy listeners. Apply router policy after user transforms and before
validation/application, so imported configuration cannot silently take over router
networking. Final listener ports and supported policy fields are chosen on resume.

procd owns process supervision; shared application owners request and observe
lifecycle actions. Specify graceful shutdown and crash/restart separately. Decide
whether core survives server crashes and how identity/configuration are recovered
before implementing lifecycle promises.

## Transport and authorization

Use the shared RPC registry's CLI target: an explicit allowlist covering the MVP
operations, served by the server over a local socket that only the intended
user can open. LuCI invokes the CLI through rpcd with per-method ACLs. Keep
handlers thin and preserve shared structured errors/receipts.
No generic desktop-command forwarding, new unauthenticated HTTP listener, or
browser-selected filesystem path/executable/argv.

Exact CLI subcommands, DTOs, envelopes, frame limits and the rpcd integration
are intentionally deferred until upstream APIs settle. A query classification does
not imply read authorization. Enumerate ACL methods explicitly; decide whether
read-only sessions receive redacted previews or full YAML containing credentials.
LuCI write access is administrative access to imported script execution; audit
engine capabilities and resource limits before exposure.

## Target and delivery boundary

Retain aarch64 and x86_64 Linux/musl as the intended first targets, local Docker
verification and ImmortalWrt preference. No physical router is available for the
initial acceptance plan. Select exact release/SDK/target tuples on resume.

Measure binary size, JS/Lua peak RSS and CPU/native-library requirements in the
chosen targets. Do not import desktop AES assumptions into generic router builds.
Docker musl compilation does not establish SDK package installation or
physical-router support. Build/test orchestration uses named repository Deno
tasks; the server does not require Node/Deno at runtime.

The future deliverables are the OpenWrt target crate and server assembly, the
CLI, native LuCI view/menu/ACL, and target feed packages.
Package layout, init service names and storage paths are not frozen during pause.

## Completion evidence

- A GUI-free host constructs and executes the same application operations that
  the desktop consumes, preserving transaction/lifecycle behavior.
- Authenticated LuCI, through the CLI and the server, completes the
  import/select/script/build/inspect/start/stop loop, including visible failures
  and authorization denials.
- Real Mihomo validation and procd observations establish actual process state;
  restart/crash tests establish the selected recovery policy.
- Persistence, failed build/publication, concurrent commands and orderly shutdown
  retain the shared ownership and failure semantics.
- Both target results, release/SDK/native-library compatibility, package
  installation, size/RSS and unverified hardware boundaries are recorded.

Transparent proxy, firewall4/nftables integration, DNS takeover, TUN routing,
remote subscription scheduling, proxy selection, traffic history, updater and
full React reuse remain outside this MVP. Resume does not automatically authorize
implementation of these later capabilities.

## Reference implementation

Use [OpenWrt-nikki](https://github.com/nikkinikki-org/OpenWrt-nikki) as a concrete
reference for native LuCI, package separation, procd integration and SDK builds.
The [roadmap reference track](openwrt-roadmap.md#openwrt-nikki-reference-track)
records inspected source paths and the pinned review revision. It does not
replace the shared Rust application or expand the MVP into transparent proxying.
Reconcile reference details with the completed upstream shell and target release
when resuming; method DTOs, storage and package layout remain unfrozen.
