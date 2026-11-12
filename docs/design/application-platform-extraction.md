# Shared core, shells and package split

Status: the Tauri extraction baseline is delivered through Part 24
([#6108](https://github.com/libnyanpasu/clash-nyanpasu/pull/6108)).
`NyanpasuClient` and the shared application layer live in `nyanpasu-core`, and
core no longer depends on Tauri. The Phase A foundation and implementation split
is also delivered, and the transitional `nyanpasu-platform` crate is retired.
Phases B–D and A2b are delivered as well; remaining platform/musl/browser
execution checks are recorded in tasks §4.
Package ownership is not final. This document plans the next stage: the shells
and RPC targets the backend serves, the core package family, and where
target-specific and concrete
implementations live. It replaces the earlier definition of
`nyanpasu-platform` as the home for every concrete filesystem, script, process,
network and OS implementation.
[Backend package standards](../development/backend-packages.md) define the
rules that apply today.

Crate names in this document are working names; the PR that creates each crate
settles its name. LOC figures count physical lines, tests included, on `main`
at `0c22ade7`, except the Phase D engine closure re-counted at `fa7e5050b`.

## Target ownership

```text
Shells (composition roots, RPC hosts)
  backend/tauri              nyanpasu-server           mobile shell (later)
  desktop GUI, in-process    headless, long-lived
       |      |                |        |
       |      +- target crates +--------+   desktop or OS integration for
       |         (by target)                the shell's target
       |                       |
       +-- nyanpasu-rpc -------+   neutral commands and registry
                  |
             nyanpasu-core         facade and assembly
                  |
             core package family   application, owners, persistence
                  |
             nyanpasu-contracts    values and consumed ports
                  |
             nyanpasu-config       schemas, pure runtime executor and its ports

Foundation layer, usable by every crate above it:
  leaves: nyanpasu-config, nyanpasu-contracts, nyanpasu-persistence, nyanpasu-profile-transform
  delivered upper foundation: nyanpasu-runtime-values (no reverse edge from leaves)

Implementation crates, injected by shells, depend on the foundation layer only:
  nyanpasu-scripts, nyanpasu-profile-io, nyanpasu-net, storage backends

nyanpasu-cli --> nyanpasu-rpc-protocol + local-socket client --> a running shell
```

Shared application decisions, profile/configuration transactions, core control,
internal receipts, recovery and observations belong in the core package family.
Public mutation results and their runtime/profile error closure live in the
upper-foundation `nyanpasu-runtime-values`, delivered in D2. Config
retains schemas, pure execution and its consumer-owned ports. Classify each
component by responsibility, not merely by absence of Tauri types. Headless
startup must not require fake GUI dependencies or successful no-op business
effects. Keep explicit availability/outcomes for optional capabilities.

### Shells and host forms

A shell is a binary that composes the application and serves it over one or
more RPC targets. Only a shell constructs `NyanpasuClient`.

| Shell                       | Form                                                     | Owns `NyanpasuClient` | RPC targets served                                             | Target crates                                       |
| --------------------------- | -------------------------------------------------------- | --------------------- | -------------------------------------------------------------- | --------------------------------------------------- |
| `backend/tauri`             | Desktop GUI                                              | Yes, in process       | Tauri IPC; WebUI over HTTP (optional); local socket (optional) | Desktop interaction and desktop OS integration      |
| `nyanpasu-server` (planned) | Headless service: router, Linux server, headless desktop | Yes, one per host     | Local socket; WebUI over HTTP                                  | OS integration for its target; no interaction crate |
| `nyanpasu-cli` (planned)    | Command-line client                                      | No                    | Client of the local socket                                     | None                                                |
| Mobile shell (later)        | Mobile app                                               | Yes, in process       | Its platform IPC                                               | Mobile target crate                                 |

- The router form is client/server: LuCI drives `nyanpasu-cli`, the CLI talks to
  `nyanpasu-server` over the local socket, and procd supervises the server.
- The CLI never constructs an application graph per invocation. One long-lived
  shell owns state, runtime lifecycle and background jobs; the CLI sends
  commands and reads structured results.
- The CLI depends on the RPC protocol and the local-socket client only, not on
  core, so a router CLI does not link the script engines or the actor graph.
- When the desktop shell serves the local socket, the same CLI drives a running
  desktop app.

### RPC targets

The planned RPC layer serves three targets; HEAD currently serves Tauri IPC and HTTP. Each command declares which targets expose
it and which shell capabilities it needs.
Every dispatched call passes one audit seam in the RPC layer, whatever the
transport. It records metadata only (transport, command, caller, duration and
outcome), never payloads; the shell decides where the records go.
E2b moved `dispatch_audited`, its injected synchronous sink port and the neutral
production `TracingRpcAuditSink` with its level/secret-exclusion tests into
`nyanpasu-rpc`; both shells reuse this implementation. E3's local socket/CLI enters the same kernel at INFO,
as HTTP does; Tauri IPC stays at DEBUG regardless of outcome. Direct HTTP archive
and subscription establishment also enter this kernel, without auditing frames
or retaining streams.

| Target | Transport                                                        | Caller                              | Exposed commands                                                                                                                                                                                                      | Authorization                                                                                           |
| ------ | ---------------------------------------------------------------- | ----------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Tauri  | Tauri IPC                                                        | Desktop webview                     | All desktop commands, including those that need shell capabilities                                                                                                                                                    | Process boundary                                                                                        |
| WebUI  | HTTP and SSE, loopback, per-start credential, Host/Origin checks | Browser                             | Explicit, reviewed allowlist (`rpc(http)` at `b70bb66d7`: 104 of 163 declarations across cfg branches). Direct privileged commands stay restricted; indirect desktop service paths remain the documented O-5/V-19 gap | Credential and session cookie, as in [Unified RPC](../development/rpc.md)                               |
| CLI    | Local socket (Unix socket, Windows named pipe)                   | `nyanpasu-cli`, and LuCI through it | Explicit allowlist (working name `rpc(local)`): administrative operations a headless host can perform; no presentation capabilities                                                                                   | OS permissions on the socket (owner and mode, pipe ACL); LuCI applies rpcd ACLs before invoking the CLI |

Command exposure and shell capabilities:

- Tauri IPC is the full desktop surface. WebUI and CLI exposure are separate
  explicit opt-ins, each reviewed for that caller; one does not imply the other.
- A command that needs a shell capability (clipboard, window identity, a
  webview Channel, native dialogs, notifications, elevation consent) declares
  it. Known commands remain registered when a shell lacks a required capability.
  Missing capability or target opt-in returns structured `unsupported`; only an
  unknown command name returns `unknown_method`,
  never a silent no-op.
- Owner identity stays transport-neutral (`RpcOwner`): a Tauri window, an HTTP
  session cookie, or a local-socket connection.
- Domain observations are published once to a neutral event bus. Tauri events,
  SSE and local-socket subscriptions are subscribers of that bus.

RPC packages:

| Package (planned)       | Owns                                                                                                               | Depends on                                                                                      |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------- |
| `nyanpasu-rpc-protocol` | Command names, request/response types, structured `RpcError`, event names, exposure and capability metadata        | Approved foundation values, including D2e runtime-values; no GUI, CORE_FAMILY or IMPLEMENTATION |
| `nyanpasu-rpc`          | Neutral command implementations and registry over `NyanpasuClient`, neutral event bus                              | Core and protocol; not Tauri                                                                    |
| HTTP (WebUI) transport  | API-only router with optional static assets, SSE, credential and Host/Origin checks                                | `nyanpasu-rpc`                                                                                  |
| Local-socket transport  | Negotiated length-prefixed JSON; server/client features separate; reuse local-channel endpoint security techniques | Protocol; server side also `nyanpasu-rpc`                                                       |
| Tauri IPC binding       | Stays in `backend/tauri`                                                                                           | `nyanpasu-rpc`                                                                                  |

The machinery at `b70bb66d7` blocks this in five places:

- `CommandEntry` holds a `TauriRpcHandler` typed over `AppHandle`, `Window` and
  `Webview` (`backend/tauri/src/unified_rpc.rs:249-255`), so the registry cannot
  leave the Tauri crate.
- `nyanpasu-macro` always emits a Tauri handler next to the HTTP handler
  (`backend/nyanpasu-macro/src/unified_command.rs:176-279`). It needs a neutral
  declaration with per-transport wrappers and per-target opt-ins.
- The 163 command declarations (160 distinct names across cfg branches) live in `backend/tauri/src/ipc.rs`; 16 of them
  inject `AppHandle` and are desktop-only.
- The HTTP server refuses to start without frontend assets
  (`backend/tauri/src/server/debug_http.rs:138-141`), so there is no API-only
  mode at this baseline. E3a removes this refusal in `nyanpasu-rpc/src/http`.
- HTTP events are bridged from Tauri events and named by Tauri-Specta types
  (`backend/tauri/src/unified_rpc.rs:173-215`).

Phase E is planned in [tasks §5](../spec/2026-11-03-core-package-split/tasks.md#5-phase-e-的-pr-划分). The neutral protocol owns complete wire/receiving closure, not every domain definition: canonical foundation values remain in place; explicit receiving projections handle skipped operational sources. Error downcasts belong in the registry. Core/foundation/implementation crates cannot depend on `nyanpasu-rpc`; the registry cannot depend on implementation crates. CLI production closure excludes registry, server, CORE_FAMILY, IMPLEMENTATION and GUI, while foundation values through protocol are allowed.

E3 requires version/features negotiation before dispatch, strictly increasing never-reused connection IDs, bounded queues/resource tables, acknowledged connection-owned subscriptions and server-generated transport-tagged owners. Unix sockets and Windows named pipes are both acceptance platforms. A dedicated transport task owns the client connection; dropping a call future cannot cancel partial IO. Timeouts/transport failures close the connection, report unknown outcome and never retry mutations automatically. Successful late resource-producing completions register returned IDs and promptly close results after disconnect, including App/Service log sessions. Unconfirmed Service opens whose upstream reply is lost or exceeds the five-second timeout have no ID and rely on the existing 45-second session lease without renewal (V-61); runtime service-protocol changes remain out of scope. All transports preserve fatal panics; no JoinError-to-RpcError conversion. The bus delivers native events synchronously through an injected sink, preserving single-window/type/all-window targeting and missing-window errors. SSE/local receivers are bounded with lag/resync; source order is preserved without a global causal order. Forwarders subscribe synchronously before reconciliation, stream activation and listener admission, after client owners are constructed. Migrated events no longer use a Tauri `listen_any` bridge.

The maintainer chose selective adaptation of keiko233's POC (`4a4a3a0d3`): macro split, command grouping, bus-first forwarding, API-only listener and small CLI. Server assembly, connection lifecycle, refusal and audit integration are rebuilt against B–D. Fake effect substitutes and panic conversion are rejected. Durable stop, host-bound core and new lifecycle commands need separate behavior cards; E5 and LAN ticket login stay paused.

### Core package family

`nyanpasu-core` is one crate of about 100k lines. An edit anywhere in it
recompiles that crate as one incremental unit and rebuilds its desktop shell
consumer (`backend/tauri`). Implementation crates now depend on the foundation
layer and compile independently of core. The remaining split aims at two things:

- Implementation and target crates depend on the small foundation layer instead
  of all of core, so they compile in parallel with core and do not rebuild when
  core internals change. Only the shells and the RPC registry depend on the
  facade.
- Owners that change rarely and own heavy dependencies sit in their own crates
  below the application layer, so editing the facade or a workflow does not
  rebuild them.

| Layer          | Package (working name)                | Moves from `nyanpasu-core/src`                                                                                                   | LOC                       | Owns                                                                                 |
| -------------- | ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ------------------------- | ------------------------------------------------------------------------------------ |
| Facade         | `nyanpasu-core`                       | `client/mod.rs` and assembly                                                                                                     | ~3,600                    | `NyanpasuClient`, `ClientSetupArgs`                                                  |
| Application    | `nyanpasu-core` modules; no new crate | `client/application_workflow/`, lifecycle/effects and source owners in `state/` stay in core                                     | Not a crate extraction    | Workflow, source participation and internal receipts                                 |
| Core control   | `nyanpasu-core` modules; no new crate | `actor_v2/`, `clash/`, `proxies.rs`, `connections.rs` stay in core                                                               | ~14,000                   | Core-manager/IPC/API control; cannot import `client`                                 |
| Owners         | `nyanpasu-observers`                  | Owner parts of `logs/`, `traffic/`, `geo/`; their redb, file-watch and Clash adapters stay in core                               | ~6,700                    | Log sessions, traffic accounting, geodata                                            |
| Updater        | `nyanpasu-updater`                    | `updater/`, `download/`; the shell injects the backend through a factory                                                         | ~3,100                    | `zip`, `tar`, `flate2`, `bolt-load`                                                  |
| State engine   | `nyanpasu-state-engine`               | Generic manager/transaction files plus ack, builder, coordinator, decision, error, snapshot, version, root definitions and tests | ~7,000                    | CORE_FAMILY leaf; only persistence among parent-workspace dependencies, defaults off |
| Migration      | `nyanpasu-migration`                  | `migration/`, `backup.rs`, `session_state.rs`, after storage ports exist                                                         | ~10,000                   | Legacy schema migration, backups, session-state recovery                             |
| Persistence    | `nyanpasu-persistence`                | Generic recovery from `corrupt.rs`, `format.rs`, generic atomic writes                                                           | ~600                      | Foundation layer: formats and file recovery; no redb or core-manager                 |
| Contracts      | `nyanpasu-contracts`                  | `BundleMetadata` and ports implemented outside core, with their type closure                                                     | Unknown                   | Foundation layer; no `ractor`, `reqwest`, `redb`, Tauri or widget IPC                |
| Runtime values | `nyanpasu-runtime-values`             | Public mutation results, degradation and runtime/profile error closure                                                           | Inventory in tasks §4 D2e | Upper foundation; no core-family/engine/implementation/GUI/core-manager dependency   |
| Actor support  | `nyanpasu-owner-support`              | `owner_shutdown.rs` and `utils/blocking.rs`, once the first owner crate leaves core                                              | ~55                       | Owner drain and blocking-join helpers over `ractor`                                  |

The 2026-11-07 refinement keeps application/control/facade in one core crate:
the server needs all three and the CLI needs none. The original seven-file
engine estimate is 3,247 lines at `fa7e5050b`; the full generic closure is about
7,000 lines. New crates enforce dependency direction, not a measured desktop
compile-time gain. See design §8.1 V-23–V-29 and tasks §4.

Dependency rules:

- Package dependencies point toward their permitted lower owners; the module rows
  above are not additional crates. Implementation and target crates depend
  on the foundation layer only, never on the facade, application or owner
  crates, and implementation crates do not depend on each other. Only shells
  and the RPC registry depend on the facade.
- Contracts grow with real consumers: a port moves when a crate outside core
  implements it. Capability ports move with their target crates (phase C);
  public runtime values and their error closure live in runtime-values (delivered D2).
  Original foundation leaves cannot depend on this upper-foundation package;
  it cannot depend on core family, engine, implementations, GUI or core manager.
  Internal `MutationReceipt`/`RuntimeApplyReceipt` stay in core's `state/mutation/`.
- Contracts keep implementation types out of their errors. Causes such as
  `reqwest::Error` in `SubscriptionFetchError` (`state/profiles/error.rs:288`)
  and `WidgetIpcError` in `WidgetError` (`client/effects/ports.rs:13`) become
  boxed `Error + Send + Sync` sources, preserving serialized variants, `Display`
  and source chains.
- Domain profile owners stay in core; D2b inverted mutation participation.
  The concrete workflow client formerly stored at `state/mutation.rs:34`
  (`fa7e5050b`) has been replaced by a typed factory. The state engine never depends on the workflow to get a
  runtime vote. The factory is consumed in `state/mutation/ports.rs` and
  implemented in `client/application_workflow/participant.rs`. D2f checks that
  `state/` does not directly import the workflow or its concrete client; effects,
  jobs and value/validation contracts remain allowed. `CommitNotifications` and
  `EffectKind` stay in `client/effects`. Control modules cannot import `client`;
  application may import control. The policy is a bounded syntactic guard over explicit use trees and paths,
  including tests; it indexes same-core provider re-exports, allows local service
  bridges and external `nyanpasu_ipc::client`. It accepts `self::*`/`super::*`
  only from policed modules and recursively resolves inherited alias bindings at
  their declaration scope. Other globs (including provider re-export globs),
  `r#` anywhere in checked paths and unsupported forms are rejected. It is not a Rust resolver or a proof of module independence.
- Fewer, larger crates are cheaper than many small ones. Merge rows that turn
  out small or always change together; measure before adding a boundary
  ([Build measurement](#build-measurement)).

Historical baseline (`0c22ade7`): the top-level core modules formed one strongly
connected component of 16 modules. SCC/caller counts below are historical, not
a measurement of HEAD; the D-specific anchors and cuts are updated for
`fa7e5050b`. Specify each cut by symbol and destination owner:

| Cycle                                                          | Cause                                                                                       | Cut                                                                                                                                                                                                          |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| logs, traffic, geo, clash, actor_v2, proxies, updater → client | `client/owner_shutdown.rs` lives under the facade                                           | Move it to a root core module                                                                                                                                                                                |
| migration, backup, utils, service, updater → client            | `client/bundle_metadata.rs` lives under the facade                                          | Move it to contracts                                                                                                                                                                                         |
| state → client (24 lines, 62 paths)                            | Runtime values, plus operational calls: effect planning, commit notifications, jobs waiting | Move public runtime values/errors below application; invert only the concrete workflow participation dependency. Effect planning, commit notifications and jobs waiting remain permitted source dependencies |
| enhance ↔ client                                               | `enhance/artifact_snapshot.rs:95-120` builds `client::runtime` snapshot types               | Keep the conversion in application, above the transform crate                                                                                                                                                |
| effects ↔ hotkey, system_proxy                                 | The executor calls owners that return effect status types                                   | Move effect result values below both                                                                                                                                                                         |
| state ↔ application_workflow                                   | `state/mutation.rs:34` at `fa7e5050b` stores the concrete workflow client                   | Inject the mutation participant boundary                                                                                                                                                                     |
| updater → client                                               | `updater/ports.rs:8` uses `client::core_lifecycle` artifact types; version parsing, mirrors | Move the artifact types and mirrors below both, version parsing to contracts (B4)                                                                                                                            |
| migration ↔ session_state                                      | `session_state.rs:27,72` uses `migration::fs` errors and atomic writes                      | Move generic atomic writes to persistence (B1a)                                                                                                                                                              |

### Script runtimes and profile transform

Script runtimes are part of profile enhancement, not platform infrastructure.

- `nyanpasu-scripts` provides the JavaScript and Lua runners and their
  extension definitions. It depends only on the `nyanpasu-config` ports
  `ScriptRunner` and `ProfileContentSource` (`runtime/executor/ports.rs`), not
  on core. `ScriptType` (`enhance/mod.rs:23`) duplicates config's
  `ScriptRuntime` and is removed; `ScriptWrapper` becomes private to the script
  crate.
- `nyanpasu-profile-transform` takes the runtime builder and builtin transforms
  from `nyanpasu-core::enhance` and depends on config. Artifact-to-snapshot
  conversion stays in the application layer, because it builds application
  runtime types.
- The transform crate does not depend on the script crate. The shell injects a
  runner constructor, so the engines compile in parallel with transform and
  core, and a shell can leave out an engine through features.
- What a script may reach is part of its extension definition and is injected:
  module loading (including the JavaScript HTTP module loader), network,
  filesystem and console. A2b made determinism conditional on fixed injected
  capability observations, not a frozen reproducible mode. Both engine features
  default on; shells disable dependency defaults and select engines explicitly.
  A2b-a delivered injected extensions without changing behavior. A2b-b delivered one atomic behavior
  commit: selectable engine features, config's typed `StartEngine`, every pipeline
  branch, wire/bindings, six locales and the browser regression landed together.
  Factory engine methods decline by default; both a compiled scripts engine
  and shell-provided extensions are required, otherwise `StartEngine` is returned.
  This keeps shell dependencies optional under Cargo feature unification.
  Disabled Lua never becomes a false predicate or silent no-op in any commit.

### System capabilities and target crates

System capabilities vary along two independent axes.

| Kind                       | Varies with                         | Capabilities                                                                                                          | Owner                                                                                                                                                 |
| -------------------------- | ----------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| Presentation / interaction | Shell form: desktop, mobile, server | Global hotkeys, windows, tray, widgets, dialogs, notifications, clipboard, opener, deep links, single instance        | The shell; the desktop mechanisms live in `backend/tauri/src/desktop/`, the single-instance claim shared by both shells in `nyanpasu-single-instance` |
| OS integration             | Operating system and privileges     | System proxy, PAC, autostart, service install and control, DNS flush, device identity, core binary install, elevation | An OS integration crate per target family, used by every shell on that target                                                                         |

An OS integration crate sits over the foundation layer like an implementation
crate: it implements contracts, the shells inject it, and core never depends
on it. Desktop interaction has no crate of its own; it stays in the Tauri
shell.

| Crate (working name)                          | Used by                                           | Holds                                                                                                                                                      |
| --------------------------------------------- | ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `backend/tauri/src/desktop/` (a shell module) | The Tauri desktop shell                           | The hotkey owner, main-thread handoff and presentation-effect execution (locale, tray, widget); core keeps the ports and the shared convergence of effects |
| `nyanpasu-os`                                 | Desktop shells and the server on desktop or Linux | System proxy, PAC and autostart, service install and control, DNS flush, core binary install, device identity, elevation mechanism, sidecar version probe  |
| `nyanpasu-openwrt` (on resume)                | The server on OpenWrt                             | procd lifecycle control, router paths and policy, later firewall integration                                                                               |
| Mobile target (later)                         | Mobile shell                                      | Mobile OS and interaction integration                                                                                                                      |

- The server does not link the Tauri shell, so it carries none of the desktop
  mechanisms. Tauri-bound mechanisms
  (global-shortcut registrar, tray and window rendering, Tauri dialog,
  notification and clipboard plugins, the app package installer) stay in
  `backend/tauri`.
- Presentation capabilities are optional in core. A shell that lacks one passes
  none, and the dependent operation reports it as unsupported; an explicit
  hotkey edit is rejected before it is saved. Sinks that only observe (UI
  invalidations, frontend diagnostics) stay required, and a shell without a GUI
  passes a neutral one.
- Elevation crosses both axes: the OS crate owns the mechanism, and the shell
  injects how consent is obtained (an authorization dialog on a GUI shell, a
  refusal or a pre-elevated service on the server). On the desktop the OS
  authorization dialog is the consent; there is no second confirmation. Core
  never shows an OS dialog itself.
- The `platform` name is retired. It says nothing about the target or the
  owner, and the target ownership has no catch-all crate.

### Implementation crates

`nyanpasu-platform` is dissolved by owner rather than renamed. Its contents, and
the concrete infrastructure still in core, move as follows. Groups that stay
small may share a crate; heavy-dependency ownership decides the boundary.

| Group                                     | Current location                                                                               | Target                                      |
| ----------------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------- |
| Script runners                            | `nyanpasu-scripts`                                                                             | Delivered                                   |
| Profile files and materialization         | `nyanpasu-profile-io` over `nyanpasu-persistence`                                              | Delivered                                   |
| Subscription HTTP                         | `nyanpasu-net::subscription_http` and `subscription`                                           | Delivered                                   |
| Runtime-build orchestration               | `nyanpasu-core::client::runtime_build`                                                         | Delivered in core's application layer       |
| Changelog, egress, icon, diagnostics HTTP | `nyanpasu-net`                                                                                 | Delivered                                   |
| File watchers                             | `geo/adapters.rs`, `state/profiles/scheduler.rs` (`notify-debouncer-full`)                     | Injected adapters with their IO crate       |
| Storage backends                          | redb in `storage.rs`, `logs/redb.rs`, `traffic/storage.rs`; `nyanpasu-traffic` legacy importer | Storage backends behind owner storage ports |

## Delivered baseline versus target

The Tauri extraction and Phase A package split are delivered in the current tree:
`backend/nyanpasu-core/src/client/` owns the facade, typed clients and
workflows; the sibling domain modules own their actors, services and
transactions. Core's `client::runtime_build::RuntimeBuildService` owns
runtime-build orchestration and receives a runner constructor and content source.
`nyanpasu-profile-transform` owns the runtime builder and builtins;
`nyanpasu-scripts` owns JavaScript and Lua runners over config's ports.
`nyanpasu-persistence` owns generic filesystem recovery,
`nyanpasu-profile-io` owns profile files, materialization and content access,
and `nyanpasu-net` owns subscription, changelog, egress, icon and diagnostics
HTTP adapters and shared client policy. These implementations depend on the
foundation layer only.
`backend/tauri/src/setup.rs` constructs and injects these implementations and
the GUI adapters. Tauri retains presentation, desktop plugins, frontend events,
IPC and HTTP transport.

Application integration tests live in core with profile-io and net as
dev-dependencies. Core's exported test support remains parameterized by
`ClientHostAdapters`; Tauri owns its host test assembly. Golden and real-engine
runtime tests live in transform, with scripts as a dev-dependency.
`nyanpasu-platform` and the former `nyanpasu-application` crate are removed.

Phase B (B1a–B4) is delivered: `nyanpasu-migration` owns upgrades, backups
and session document reads over storage ports; `nyanpasu-owner-support` owns
shutdown tracking and blocking-task result handling; `nyanpasu-observers` owns
logs, traffic and geodata observation; and `nyanpasu-updater` owns core updates,
HTTP downloads and unpacking through a shell-injected backend factory. Generic
formats and atomic writes live in persistence, and facade storage inputs are
ports with traffic's legacy redb importer behind a feature.

In phase C, presentation effects (locale, hotkeys, widget, tray) run in their own
executor behind a core-owned port, apart from the shared effects. C3a moves
presentation execution, the hotkey owner and the main-thread handoff into the
Tauri shell; core keeps hotkey parsing, action dispatch and shared effect
convergence, and no core-family package depends on the widget protocol
directly. Window control, hotkey validation, presentation
execution and debug HTTP are optional: missing capabilities report structured
unsupported results, and explicit hotkey edits are rejected before persistence.
Unsupported effects neither advance applied revisions nor retry automatically.
Observer sinks (`UiEventSink` and `FrontendLogSink`) remain required; headless
shells provide neutral implementations. C2b–C2c move proxy reads/writes, PAC,
autostart, device identity, sidecar version probes, service control, DNS flush
and binary installation into `nyanpasu-os`. C2d adds the synchronous contracts
consent port: desktop permits the existing OS prompt, while `RefuseElevation`
prevents elevated launches and pauses automatic service recovery on refusal.
E4 must inject that refusing policy into service control, DNS flush and binary
installation in the independently assembled server.

Known gaps against the target ownership:

- C2e rejects HTTP `update_core` before job admission with unsupported / 501;
  desktop updates retain the binary install copy fallback
  ([#6157](https://github.com/libnyanpasu/clash-nyanpasu/issues/6157)). The embedded
  WebUI still shares the desktop elevation policy when `patch_app_config` turns
  service mode on or off, during `retry_configuration_runtime` and transaction
  compensation (`client/application_workflow/tcc.rs`). C2 adds no per-call
  permission context or second client. The separately assembled server (E4)
  must inject the refusing policy and verify that all these paths cannot show
  an authorization dialog. This does not change the embedded desktop WebUI
  policy or introduce per-call authority.
- Runtime candidate file IO is implemented by `RuntimePaths`.
- Watcher and service-log IO remains in core: geodata and external-profile
  watchers, and service-log IPC (`client/logs.rs`).
- E2b moved the shared RPC registry, commands, audit and HTTP server to
  `nyanpasu-rpc`. Native IPC and asset resolution stay in Tauri. E2c replaces
  the event input bridge with bus-first publication and synchronous targeted
  native delivery. E3a makes frontend assets optional for HTTP startup.

### Core split stack

The following parts are present through Part 23.

| Part    | PR / commit                                                      | Change                                                 |
| ------- | ---------------------------------------------------------------- | ------------------------------------------------------ |
| 01      | [#6065](https://github.com/libnyanpasu/clash-nyanpasu/pull/6065) | Keep bundle host logic out of client code              |
| 02      | [#6066](https://github.com/libnyanpasu/clash-nyanpasu/pull/6066) | Extract owner shutdown helpers                         |
| 03      | [#6068](https://github.com/libnyanpasu/clash-nyanpasu/pull/6068) | Separate YAML and key-value parsing from GUI helpers   |
| 04      | [#6071](https://github.com/libnyanpasu/clash-nyanpasu/pull/6071) | Inject host build metadata                             |
| 05      | [#6072](https://github.com/libnyanpasu/clash-nyanpasu/pull/6072) | Move presentation ports to their effects consumer      |
| 06      | [#6073](https://github.com/libnyanpasu/clash-nyanpasu/pull/6073) | Separate Tauri delivery adapters from shared ports     |
| 07      | [#6074](https://github.com/libnyanpasu/clash-nyanpasu/pull/6074) | Gather GUI adapters under `adapters/`                  |
| 08      | [#6075](https://github.com/libnyanpasu/clash-nyanpasu/pull/6075) | Own core status events at the GUI boundary             |
| 09      | [#6076](https://github.com/libnyanpasu/clash-nyanpasu/pull/6076) | Own stream and storage events at the GUI boundary      |
| 10      | [#6077](https://github.com/libnyanpasu/clash-nyanpasu/pull/6077) | Own client notification events at the GUI boundary     |
| 11      | [#6078](https://github.com/libnyanpasu/clash-nyanpasu/pull/6078) | Split profile file ports, helpers and concrete service |
| 12a     | [#6079](https://github.com/libnyanpasu/clash-nyanpasu/pull/6079) | Inject concrete adapters from the composition root     |
| 12b     | [#6080](https://github.com/libnyanpasu/clash-nyanpasu/pull/6080) | Inject the host runtime-build port                     |
| 13      | [#6081](https://github.com/libnyanpasu/clash-nyanpasu/pull/6081) | Run application tests with Tokio                       |
| 14a     | [#6082](https://github.com/libnyanpasu/clash-nyanpasu/pull/6082) | Give shared fixtures independently movable owners      |
| 14b     | [#6085](https://github.com/libnyanpasu/clash-nyanpasu/pull/6085) | Keep platform test subjects at host boundaries         |
| 15 (M1) | [#6086](https://github.com/libnyanpasu/clash-nyanpasu/pull/6086) | Move leaf utilities into core                          |
| 16 (M2) | [#6087](https://github.com/libnyanpasu/clash-nyanpasu/pull/6087) | Move service control and downloads into core           |
| 17 (M3) | [#6088](https://github.com/libnyanpasu/clash-nyanpasu/pull/6088) | Move storage, backups and migrations into core         |
| 18 (M4) | [#6090](https://github.com/libnyanpasu/clash-nyanpasu/pull/6090) | Move the core control plane into core                  |
| 18b     | [#6092](https://github.com/libnyanpasu/clash-nyanpasu/pull/6092) | Prepare log and geodata fixtures for extraction        |
| 19 (M5) | [#6093](https://github.com/libnyanpasu/clash-nyanpasu/pull/6093) | Move core logs and geodata into core                   |
| 19b     | [#6094](https://github.com/libnyanpasu/clash-nyanpasu/pull/6094) | Keep connection-detail subscription state in the GUI   |
| 20 (M6) | [#6102](https://github.com/libnyanpasu/clash-nyanpasu/pull/6102) | Move the Clash API client and streams into core        |
| 21 (M7) | [#6103](https://github.com/libnyanpasu/clash-nyanpasu/pull/6103) | Move traffic accounting into core                      |
| 21b     | [#6105](https://github.com/libnyanpasu/clash-nyanpasu/pull/6105) | Isolate profile file integration test ownership        |
| 22 (M8) | [#6106](https://github.com/libnyanpasu/clash-nyanpasu/pull/6106) | Move the facade and transaction graph into core        |
| 23 (M9) | [#6107](https://github.com/libnyanpasu/clash-nyanpasu/pull/6107) | Move concrete infrastructure adapters into platform    |
| 24      | [#6108](https://github.com/libnyanpasu/clash-nyanpasu/pull/6108) | Document the core and GUI crate split                  |

Part 24 (M10) documents the delivered layout. Review of that PR recorded the ownership
correction above
([comment](https://github.com/libnyanpasu/clash-nyanpasu/pull/6108#issuecomment-6545500519)).

E2a uses schema-only Specta collection, retaining only `call_rpc` in the Tauri transport builder and byte-identical generated files. Its complete 104-command HTTP disposition table preserves host behavior through injected ports, including Tauri executable resolution and pure icon/file queries. E1b explicitly projects and decodes `CoreAgentSkill` as well as runtime values.

E3c delivers optional endpoint infrastructure sharing one default endpoint per user/app data directory across desktop and server; the first binder owns it, a second live binder fails, and CLI may use `--endpoint`. Unix only reclaims a valid private-directory user-owned socket on connect refused; Windows uses first-instance exclusivity without stale cleanup. E3b delivers version/features, canonical JSON and the pure subscribe/unsubscribe/lag state machine, without a production listener. E4a delivers the server composition library with the current Redb log store and observers fallback. E4b adds the resident executable, installs a neutral process panic policy before tasks start, and supplies HTTP/local/background-owner integration tests that launch the real `nyanpasu-server` executable for non-zero exit with original panic evidence and no RPC-error conversion. A default-off `panic-injection` feature is enabled by the server's self dev-dependency; the independent production closure rejects leaks and the musl production build leaves it off. Musl runs the `panic_process` integration target alongside lib/bin unit tests. Forwarders subscribe before activation; local and optional authenticated loopback HTTP share one application. Listener readiness and reverse startup cleanup are explicit, and transport drain does not cancel owner work. Windows and Unix listener behavior remain platform-specific acceptance checks; E4c/d CLI and session integration remain planned.

E2a moved the four shared HTTP response definitions before RPC extraction, with the icon-mode value owned by protocol and asset functions owned by the shell. E2b includes runtime-values unconditionally in dependency roots and reverse-edge policy. E3b now retains only full-value configuration/core/service status, app-update state when available, and core-log status, with an initial snapshot and fresh reset after local lag. The separate connection-detail contract is unchanged. All other event identities are excluded from E3 local subscriptions. Real-connection reset tests cover only those five identities; the thirteen-command allowlist is unchanged. O-46/V-63 defer invalidation/storage recovery to a separate reviewed card with round-3 findings 1–3 as acceptance criteria. E4a's real OS refusal-adapter tests use a narrowly scoped, default-off OS test-support feature enabled only by the server dev-dependency; production feature closure excludes it. Decisions O-40–O-45 and deviations V-56–V-62 record these changes and the Service-open lease limitation.

## Next stage

Each card is one buildable commit; a PR may contain an ordered short stack.
Callers migrate together without old-path shims, and moves never include behavior
changes. [Tasks §4](../spec/2026-11-03-core-package-split/tasks.md#4-a2b-与-phase-d-的-pr-划分)
records the delivered PR-D1 → PR-D2 → PR-D3 → PR-D4 → PR-A2b stack. A2b landed after D4 to add its
pipeline variant against final value ownership, not because it depends on D.
[Tasks §5](../spec/2026-11-03-core-package-split/tasks.md#5-phase-e-的-pr-划分)
fixes the remaining order: PR-E1 → PR-E2 → PR-E3 → PR-E4 → PR-E5 →
PR-E6 → PR-E7 → PR-E8, after the delivered D/A2b stack;
their consumer lists remain unchanged. E assumes D2e runtime-values exists.
PR-E4/E5 complete DTO receiving closure after E3e; PR-E6–E8 assemble and verify
the server/CLI after E1b. Every E package and feature updates Cargo,
dependency/static policy tests and both musl target lists. These E cards remain
planned; their Rust/platform verification has not been run by this docs change.

**Phase A: break the cycles and stop the leak**

| Step | Change                                                                                                                                      | Done when                                                                                                                                          | Status                                     |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| A1   | Move owner shutdown support to a root core module and `BundleMetadata` into `nyanpasu-contracts`; replace `ScriptType` with `ScriptRuntime` | No lower module imports `client::owner_shutdown` or `client::bundle_metadata`; `ScriptType` is gone                                                | Delivered                                  |
| A2   | Extract `nyanpasu-scripts` and inject a runner constructor into runtime building                                                            | Script engines do not depend on core; JS/Lua golden behavior unchanged                                                                             | Delivered                                  |
| A2b  | Inject extensions; default-on JS/Lua features selected explicitly by shell; typed unavailable-engine errors reach the pipeline and wire     | No internal reqwest client construction; feature matrix and overlay refusal tests; six locales/Paraglide/frontend regression; tasks §4 A2b-a–A2b-b | Delivered (browser/musl execution pending) |
| A3   | Extract `nyanpasu-profile-transform`                                                                                                        | Transform depends on config only; artifact-to-snapshot conversion stays in application code                                                        | Delivered                                  |
| A4   | Move the ports implemented outside core into `nyanpasu-contracts`: profile and materialization, then network                                | Each group moves with its full type closure; contract errors box implementation causes, wire shape unchanged                                       | Delivered                                  |
| A5   | Move runtime-build orchestration into core; extract `nyanpasu-persistence`, `nyanpasu-profile-io` and `nyanpasu-net`; retire platform       | Implementation crates depend on the foundation layer only; application integration tests move into core                                            | Delivered                                  |

**Phase B: leaf owners and heavy dependencies**

| Step | Change                                                             | Done when                                                                                               | Status    |
| ---- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------- | --------- |
| B1a  | Add formats and generic atomic writes to persistence               | Session state no longer depends on migration; generic recovery does not examine core-manager errors     | Delivered |
| B1b  | Add storage ports for backup and migration, then extract migration | Migration and backup do not depend on redb                                                              | Delivered |
| B2   | Move storage backends out of the facade signature                  | No concrete redb type in `ClientSetupArgs`; traffic's legacy redb importer is feature-gated or separate | Delivered |
| B3   | Extract the log, traffic and geodata owners into one owner crate   | Owners consume normalized frames and injected storage and watchers                                      | Delivered |
| B4   | Extract the core updater and inject its backend                    | The facade no longer constructs `HttpUpdaterBackend`; archive dependencies stay with the updater        | Delivered |

**Phase C: capabilities and target crates**

| Step | Change                                                                                                           | Done when                                                                                                                                                          |
| ---- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| C1   | Make window control, hotkey validation, presentation-effect execution and debug HTTP optional                    | A headless assembly passes none of them; dependent operations report unsupported, and an explicit hotkey edit is rejected before it is saved                       |
| C2   | Create `nyanpasu-os` with an elevation-consent port; remove `update_core` from the WebUI; add the RPC audit seam | Core runs no elevation itself; a refusing policy starts no elevated command; the WebUI's `update_core` reports unsupported                                         |
| C3   | Move the desktop mechanisms into the Tauri shell; no new crate                                                   | The hotkey owner, main-thread handoff and presentation-effect execution leave the core package family; core drops its direct `nyanpasu-widget-protocol` dependency |

**Phase D: transaction graph**

Delivered: #6224 PR-D1 (D1a), #6229 PR-D2 (D2a/D2b), #6231 PR-D3
(D2c) and #6232 PR-D4 (D2d/D2e/D2f). The complete generic engine and its
102 tests live in `nyanpasu-state-engine`; public results/errors live in
`nyanpasu-runtime-values`. Source owners consume a typed participant factory,
`RecoveryRequired` waits for the source decision, and the module guard enforces
the scoped directions. D2d's generated RPC bindings changed by one JSDoc line
(`CoreError` link to plain text); wire shapes are unchanged by D moves/seams.
No Phase D implementation remains open. Docker/musl and Linux/macOS execution
remain pending as listed in tasks §4. A2b's browser regression is implemented,
but local execution still needs the Playwright browser.

| Step | Change                                                                                                                                                      | Done when                                                                                                                                                               | Status    |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------- |
| D1   | Extract the complete generic `nyanpasu-state-engine` closure as a core-family leaf                                                                          | All 102 engine tests retained; only persistence among parent-workspace dependencies; package/static/musl gates registered; tasks §4 D1a                                 | Delivered |
| D2   | Source-owned participant factory; separate strict-settlement behavior fix; public error/value closure into runtime-values; control/application stay modules | No source → concrete workflow client; strict terminal settlement; source-import policy enforces scoped module directions; bindings unchanged by moves; tasks §4 D2a–D2f | Delivered |

**Phase E: RPC targets and shells**

| Step | Change                                                                                                                                                  | Done when                                                                                                                                          |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| E1a  | Neutral/native macro, protocol envelope, independent targets and capability admission; tasks E1a-a–d                                                    | Missing required capability is unsupported, not unknown; existing bindings and HTTP behavior unchanged                                             |
| E1b  | Canonical foundation values, protocol `types` receiving closure and typed contracts for the thirteen local operations; tasks E1b-a/b as revised by V-67 | Skipped operational sources not fabricated; typed client has no core-family dependency; three bindings unchanged                                   |
| E2   | Move registry/commands/HTTP/audit and invert event publication; tasks E2a–c                                                                             | No Tauri types in registry/kernel; one audit per admission; bus-first delivery without feedback                                                    |
| E3   | API-only HTTP and negotiated Unix/Windows local RPC; tasks E3a–e                                                                                        | Unchanged HTTP middleware; target/capability refusal; owner cleanup, INFO audit, bounded queues, timeout invalidation and fatal panic              |
| E4   | E4a/b composition and resident executable delivered; CLI and session integration remain E4c/d                                                           | One client and registry with service/DNS/install refusal, supervised listeners and owner drain; consecutive CLI process acceptance remains planned |
| E5   | OpenWrt and LAN ticket login remain paused                                                                                                              | E4 complete and explicit user resume, then separate authorization/procd/package/hardware cards                                                     |

Extend `pnpm run lint:backend-boundaries` as each crate appears, so the
direction rules in [Target ownership](#target-ownership) stay enforced: no
implementation crate depends on core, the server and CLI do not depend on Tauri,
and the CLI does not depend on core.

### Build measurement

Compile time is a goal of the split, so measure it rather than assume it.
Phases A and B measured no desktop compile-time gain from the new boundaries:
the `clash-nyanpasu` library and the link still dominate incremental builds.
Phase C therefore creates only `nyanpasu-os`, which the server needs, and keeps
the desktop mechanisms in the Tauri shell. Phase D adds only the engine and
public-value boundaries for dependency enforcement; it does not create control
or workflow crates for presumed build gains.

- Before A1, record a clean `cargo build --timings` of the desktop shell and
  incremental rebuild times after a one-line edit in each of: the facade
  (`client/mod.rs`), an application workflow, a script runner and a desktop
  adapter. Re-measure after phases A and B, and record the results in the PR
  that ends each phase.
- Stop adding boundaries where the gain flattens; merge crates that always
  change together.
- Phase A disables default features on core and config dependency edges and
  forwards the required features explicitly. Enabling core's `test-support`
  through Tauri's dev-dependency still gives core a second feature set, so
  `cargo build` and `cargo test` compile core separately. O-3 remains pending
  the Phase A build measurements.

## Migration requirements

- Implementation crates depend on the foundation layer only. Do not add
  mandatory presentation capabilities to
  `ClientSetupArgs`. New code follows the target ownership; if its crate does
  not exist yet, keep the code behind a consumer-owned port next to its current
  consumer.
- Move complete bottom-up call paths and update desktop consumers in each PR.
- Preserve actor ownership, generation fences, instance API leases, caller-drop
  behavior, real in-process replies, panic semantics and orderly shutdown.
- Move profile/configuration owners and their runtime mutation participant as a
  coherent transaction boundary. Preserve voting, commit, materialization,
  promotion/compensation, degradation, retry and restart reconciliation.
- Split shared convergence from presentation effects before headless assembly.
  Frontend invalidation hints are not commit/effect receipts; preserve actual
  owner observation intent and lifecycle refresh behavior at transport bridges.
- Inject explicit paths, script/content sources, runtime candidate IO, validation
  and process control. Shared core construction uses ordinary async/Tokio;
  no Tauri runtime, frontend server/assets or GUI capability is mandatory.
- Expose a command to WebUI or the CLI only through its explicit opt-in, after
  reviewing its effects for that caller.
- Run relevant existing behavior/golden tests, desktop checks, formatting/Clippy,
  architecture and dependency gates. Extend source scanning to migrated owners.
  Preserve structured errors and serialization; regenerate changed bindings.

## OpenWrt integration boundary

OpenWrt remains paused. Its
[roadmap](openwrt-roadmap.md) does not reorder or implement these upstream PRs.
After explicit user resume, reassess main and assemble `nyanpasu-server` with
the OpenWrt target crate; LuCI drives `nyanpasu-cli`, which talks to the server
over the local socket. Do not add a router profile store, mutation journal or
parallel application facade to bypass missing core behavior.

Shared musl tests are not server, procd, SDK package or hardware acceptance.
Validate these separately after resume, including native dependencies, CPU
requirements and JS/Lua resource costs. The
[MVP definition](openwrt-mvp.md) retains the product scope and deferred policies.

### E2b delivered ownership

`nyanpasu-rpc` now owns shared commands/responses, application error mapping,
archive collection, registry/dependencies, the C2f audit kernel and production
tracing sink, EventBus, and the neutral HTTP actor/router/assets port. Tauri keeps
native dispatch, event derives, and the executable and asset resolvers. HTTP is
default-off, bound to loopback and enabled explicitly by the desktop. E3a allows
API-only startup without assets or a dev frontend, with authenticated missing
static pages returning 404. Per-start credentials, owner cookies, Host/Origin
checks and transport exposure are unchanged. E2c now publishes all seven observation sources
and the UI/app-update sinks through the bus, with synchronous targeted native
delivery and bounded SSE lag/resync. Forwarders subscribe at construction before
reconciliation, streams and listeners; root tasks own their shutdown. Deep links
and window messages publish explicitly, and connection details remain demand
streams outside the global broadcast. E3b delivers local version-1 JSON frames
with big-endian u32 lengths, the 4 MiB bound, explicit feature negotiation,
never-reused request IDs and subscription ack/reset/lag ordering. Pure frame and
session rules work without IO; the default-off `frame-io` feature adds Tokio
helpers and is enabled explicitly by musl tests. E3c adds default-off
`nyanpasu-rpc/local-server` endpoint listeners and shared UID/SID plus canonical
app-data discovery. Unix uses private directories, a persistent OS-locked claim,
refused-only stale reclamation and identity-checked cleanup; Windows uses a
protected current-user DACL and first-instance exclusivity across repeated accepts.
Musl enables local-server on the existing RPC library target; no package or static
prefix is added. The local-channel parent-child API is unchanged. E3d delivers
bounded connection dispatch, full-value subscriptions and separate detail demand,
with server-generated instance/connection owners and the shared INFO audit kernel.
Accepted work survives disconnect and normal shutdown drains its completions;
successful late App/Service log-session IDs are registered and closed. A lost or
timed-out Service reply has no ID and relies on the existing 45-second lease
without renewal, measured from creation/last renewal; physical stores have a
separate 30-second grace period. No service protocol changes are implied.
Subscription resets read the facade with bounded race retries; notifications
refresh the current full value instead of replaying older queued values. Source
order and bounded lag/resync do not promise cross-source causal order. Unsubscribe
releases demand before acknowledgement. Task panics retain their original payload
through root supervision. E3e delivers the protocol-only local client, whole-value
reset application and opt-in desktop listener. Its dedicated transport task keeps
partial exchanges synchronized when a call future is dropped; timeout or any
possibly submitted transport failure closes the connection with outcome unknown,
without mutation retry. The desktop enables the listener only when
`NYANPASU_LOCAL_RPC=1`; `NYANPASU_LOCAL_RPC_ENDPOINT` optionally overrides discovery.
It joins root shutdown and retains existing desktop elevation policy. The exact
local allowlist is `get_profiles`, `create_profile`, `delete_profile`,
`read_profile_file`, `get_configuration_status`, `get_core_status`,
`get_storage_item`, `set_storage_item`, `remove_storage_item`, `list_log_files`,
`open_log_session`, `query_logs`, `close_log_session`; HTTP exposure is unchanged.
Subscriptions use local control frames. Musl explicitly enables protocol/local-client
and RPC/local-server on their existing library targets; no package/prefix is added.
The complete typed receiving closure and headless shell remain E1b/E4 work.

| E3c platform             | Implementation | Validation                                                                                                       |
| ------------------------ | -------------- | ---------------------------------------------------------------------------------------------------------------- |
| Windows named pipes      | Delivered      | Native reconnect, real DACL and competing-binder tests                                                           |
| Linux/macOS Unix sockets | Delivered      | Permissions, refused-only stale, racing binders and replacement cleanup tests; platform execution required in CI |

Desktop local service is not automatically enabled by these endpoint adapters.
