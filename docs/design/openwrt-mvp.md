# OpenWrt / ImmortalWrt MVP

Status: paused by user, 2026-10-08. This is a downstream product definition,
not an implementation contract. Wait for the related shared-backend and
shell/Tauri separation work to complete, then reassess against main when the
user explicitly resumes. See the [roadmap](openwrt-roadmap.md) for the resume gate.

## Product outcome

Provide a lightweight native LuCI interface over a separate Rust daemon that
uses the same shared application as the desktop shell. The daemon exposes native
ubus methods; LuCI uses rpcd sessions and ACLs. Do not run the desktop Tauri binary
or embed its React distribution on the router.

The first usable loop is:

1. Open Services → Nyanpasu in authenticated LuCI.
2. Import a small local YAML profile, optionally with ordered JavaScript/Lua
   transforms, and choose the selected profile.
3. Build and inspect the resulting Mihomo configuration through the accepted
   shared runtime pipeline; expose validation failures clearly.
4. Start/stop Mihomo through the router lifecycle adapter and inspect actual
   process/configuration state rather than an optimistic running flag.
5. Restart the daemon and retain committed profiles, script order and selection;
   reconcile runtime/process state according to the agreed recovery policy.

This loop is the end-to-end MVP. Headless composition and an ubus status probe
are feasibility milestones, not a claim that the MVP is delivered.

## Architecture after upstream completion

```text
Desktop shell / Tauri adapters          OpenWrt daemon / router adapters
                 \                         /
                  nyanpasu-core: NyanpasuClient and shared APIs
                  profile/config/runtime owners and consumed ports
                  consumed ports and observations
                               |
                  domain/config/transaction primitives

LuCI -> rpcd sessions + ACL -> ubus -> daemon transport -> shared application
procd supervises the core process through an injected router control adapter
```

The target shared facade and application graph live in `nyanpasu-core`; no
separate `nyanpasu-application` layer is planned. Its former runtime code now lives in `nyanpasu-core::enhance`; the old crate is
removed. The remaining facade/actor migration must settle before OpenWrt resumes.
Concrete infrastructure stays behind core-owned ports and is injected by hosts.

Use the reviewed upstream package boundaries and construction APIs rather than
freezing a second application architecture during the pause. Shared application
code owns decisions, transactions and actor state. Router code owns concrete
paths, process/native transport adapters and host composition. Presentation,
HTTP assets, Tauri events/windows/tray and desktop helper installation are not
mandatory daemon dependencies.

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
whether core survives daemon crashes and how identity/configuration are recovered
before implementing lifecycle promises.

## Transport and authorization

Use a fixed native ubus object and an explicit typed allowlist covering the MVP
operations. Keep handlers thin and preserve shared structured errors/receipts.
No generic desktop-command forwarding, new unauthenticated HTTP listener, or
browser-selected filesystem path/executable/argv.

Exact method names, DTOs, envelopes, frame limits and any macro support are
intentionally deferred until upstream APIs settle. A query classification does
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
Docker musl compilation does not establish libubus ABI, SDK package installation
or physical-router support. Build/test orchestration uses named repository Deno
tasks; the daemon does not require Node/Deno at runtime.

The future deliverables are the daemon/router adapters, narrow native
libubus/libubox integration, native LuCI view/menu/ACL, and target feed packages.
Package layout, init service names and storage paths are not frozen during pause.

## Completion evidence

- A GUI-free host constructs and executes the same application operations that
  the desktop consumes, preserving transaction/lifecycle behavior.
- Real native ubus and authenticated LuCI complete the import/select/script/build/
  inspect/start/stop loop, including visible failures and authorization denials.
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
