# Shared core, shells and package split

Status: the Tauri extraction baseline is delivered through Part 24
([#6108](https://github.com/libnyanpasu/clash-nyanpasu/pull/6108)).
`NyanpasuClient` and the shared application layer live in `nyanpasu-core`, and
core no longer depends on Tauri. Package ownership is not final. This document
is the plan for the next stage: the shells and RPC targets the backend serves,
the core package family, and where target-specific and concrete
implementations live. It replaces the earlier definition of
`nyanpasu-platform` as the home for every concrete filesystem, script, process,
network and OS implementation.
[Backend package standards](../development/backend-packages.md) define the
rules that apply today.

Crate names in this document are working names; the PR that creates each crate
settles its name. LOC figures count physical lines, tests included, on `main`
at `0c22ade7`.

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

Implementation crates, injected by shells, depend on contracts or config only:
  nyanpasu-scripts, nyanpasu-profile-io, nyanpasu-net, storage backends

nyanpasu-cli --> nyanpasu-rpc-protocol + local-socket client --> a running shell
```

Shared application decisions, profile/configuration transactions, core control,
receipts, recovery and observations belong in the core package family. Config
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

The RPC layer serves three targets. Each command declares which targets expose
it and which shell capabilities it needs.

| Target | Transport                                                        | Caller                              | Exposed commands                                                                                                                                            | Authorization                                                                                           |
| ------ | ---------------------------------------------------------------- | ----------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Tauri  | Tauri IPC                                                        | Desktop webview                     | All desktop commands, including those that need shell capabilities                                                                                          | Process boundary                                                                                        |
| WebUI  | HTTP and SSE, loopback, per-start credential, Host/Origin checks | Browser                             | Explicit, reviewed allowlist (`rpc(http)` today: 104 of 161 declarations). No OS integration, elevation, desktop-only server controls or shell capabilities | Credential and session cookie, as in [Unified RPC](../development/rpc.md)                               |
| CLI    | Local socket (Unix socket, Windows named pipe)                   | `nyanpasu-cli`, and LuCI through it | Explicit allowlist (working name `rpc(local)`): administrative operations a headless host can perform; no presentation capabilities                         | OS permissions on the socket (owner and mode, pipe ACL); LuCI applies rpcd ACLs before invoking the CLI |

Command exposure and shell capabilities:

- Tauri IPC is the full desktop surface. WebUI and CLI exposure are separate
  explicit opt-ins, each reviewed for that caller; one does not imply the other.
- A command that needs a shell capability (clipboard, window identity, a
  webview Channel, native dialogs, notifications, elevation consent) declares
  it. A shell registers such a command only if it provides the capability.
  Another target receives a structured unsupported error from the registry,
  never a silent no-op.
- Owner identity stays transport-neutral (`RpcOwner`): a Tauri window, an HTTP
  session cookie, or a local-socket connection.
- Domain observations are published once to a neutral event bus. Tauri events,
  SSE and local-socket subscriptions are subscribers of that bus.

RPC packages:

| Package (planned)       | Owns                                                                                                        | Depends on                                |
| ----------------------- | ----------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| `nyanpasu-rpc-protocol` | Command names, request/response types, structured `RpcError`, event names, exposure and capability metadata | Config and contracts, not core            |
| `nyanpasu-rpc`          | Neutral command implementations and registry over `NyanpasuClient`, neutral event bus                       | Core and protocol; not Tauri              |
| HTTP (WebUI) transport  | API-only router with optional static assets, SSE, credential and Host/Origin checks                         | `nyanpasu-rpc`                            |
| Local-socket transport  | Server and client sides behind separate features; evaluate reusing the `nyanpasu-local-channel` framing     | Protocol; server side also `nyanpasu-rpc` |
| Tauri IPC binding       | Stays in `backend/tauri`                                                                                    | `nyanpasu-rpc`                            |

The current machinery blocks this in five places:

- `CommandEntry` holds a `TauriRpcHandler` typed over `AppHandle`, `Window` and
  `Webview` (`backend/tauri/src/unified_rpc.rs:98-99`), so the registry cannot
  leave the Tauri crate.
- `nyanpasu-macro` always emits a Tauri handler next to the HTTP handler
  (`backend/nyanpasu-macro/src/unified_command.rs:190-274`). It needs a neutral
  declaration with per-transport wrappers and per-target opt-ins.
- The 161 command declarations live in `backend/tauri/src/ipc.rs`; 16 of them
  inject `AppHandle` and are desktop-only.
- The HTTP server refuses to start without frontend assets
  (`backend/tauri/src/server/debug_http.rs:138-141`), so there is no API-only
  mode.
- HTTP events are bridged from Tauri events and named by Tauri-Specta types
  (`backend/tauri/src/unified_rpc.rs:28-71`).

### Core package family

`nyanpasu-core` is one crate of about 100k lines. An edit anywhere in it
recompiles that crate as one incremental unit and rebuilds every crate that
depends on it (`nyanpasu-platform`, `backend/tauri`), and every implementation
crate waits for core before it starts compiling. The split aims at two things:

- Implementation and target crates depend on a small contracts crate instead of
  all of core, so they compile in parallel with core and do not rebuild when
  core internals change. Only the shells and the RPC registry depend on the
  facade.
- Owners that change rarely and own heavy dependencies sit in their own crates
  below the application layer, so editing the facade or a workflow does not
  rebuild them.

| Layer         | Package (working name)  | Moves from `nyanpasu-core/src`                                                                     | LOC     | Owns                                                                  |
| ------------- | ----------------------- | -------------------------------------------------------------------------------------------------- | ------- | --------------------------------------------------------------------- |
| Facade        | `nyanpasu-core`         | `client/mod.rs` and assembly                                                                       | ~3,600  | `NyanpasuClient`, `ClientSetupArgs`                                   |
| Application   | `nyanpasu-workflow`     | `client/application_workflow/`, convergence, domain state owners in `state/profiles/` and siblings | ~25,000 | Workflows, mutation participants, profile owners                      |
| Core control  | `nyanpasu-core-control` | `actor_v2/`, `clash/`, `proxies.rs`, `connections.rs`                                              | ~14,000 | `nyanpasu-ipc`, `nyanpasu-core-manager`, `clash-api`                  |
| Owners        | one crate each          | `logs/`, `traffic/`, `geo/`                                                                        | ~6,700  | Log sessions, traffic accounting, geodata                             |
| Updater       | `nyanpasu-core-updater` | `updater/`, `download/`                                                                            | ~3,100  | `zip`, `tar`, `flate2`, `bolt-load`                                   |
| State engine  | `nyanpasu-state-engine` | `state/manager*`, `state/transaction*`                                                             | ~3,300  | Voting, CAS, rollback, settlement                                     |
| Migration     | `nyanpasu-migration`    | `migration/`                                                                                       | ~9,300  | Legacy schema migration                                               |
| Persistence   | `nyanpasu-persistence`  | `format.rs`, `corrupt.rs`, `storage.rs`, `backup.rs`, `session_state.rs`                           | ~2,000  | Formats, corruption recovery, backups                                 |
| Contracts     | `nyanpasu-contracts`    | Runtime values, receipts, `BundleMetadata`, consumed ports and their type closure                  | Unknown | Values and ports; no `ractor`, `reqwest`, `redb`, Tauri or widget IPC |
| Actor support | small crate             | `client/owner_shutdown.rs`                                                                         | 39      | Owner drain helpers over `ractor`                                     |

Dependency rules:

- Dependencies point down the table. Implementation and target crates depend
  on contracts and config only, never on the facade, application or owner
  crates. Only shells and the RPC registry depend on the facade.
- Contracts keep implementation causes out of their errors. Map
  `reqwest::Error` in `SubscriptionFetchError`
  (`state/profiles/error.rs:288`) and `WidgetIpcError` in `WidgetError`
  (`client/effects/ports.rs:13`) into stable consumed errors, preserving
  serialized variants and source diagnostics.
- Domain profile owners stay with the application workflow until mutation
  participation is inverted: `state/mutation.rs:15` stores the concrete
  workflow client. The state engine never depends on the workflow to get a
  runtime vote.
- Fewer, larger crates are cheaper than many small ones. Merge rows that turn
  out small or always change together; measure before adding a boundary
  ([Build measurement](#build-measurement)).

Today the top-level modules of core form one strongly connected component of 14
modules. These cuts remove most of it and come first:

| Cycle                                        | Cause                                                                         | Cut                                                                         |
| -------------------------------------------- | ----------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| logs, traffic, geo, clash, actor_v2 → client | `client/owner_shutdown.rs` lives under the facade                             | Move it to actor support                                                    |
| migration, backup, utils, service → client   | `client/bundle_metadata.rs` lives under the facade                            | Move it to contracts                                                        |
| state → client (24 references)               | Mostly receipt, degradation and mutation outcome types in `client/runtime.rs` | Move runtime values to contracts; keep operations and errors in application |
| enhance ↔ client                             | `enhance/artifact_snapshot.rs:95-120` builds `client::runtime` snapshot types | Keep the conversion in application, above the transform crate               |
| effects ↔ hotkey, system_proxy               | The executor calls owners that return effect status types                     | Move effect result values below both                                        |
| state ↔ application_workflow                 | `state/mutation.rs:15` stores the concrete workflow client                    | Inject the mutation participant boundary                                    |

### Script runtimes and profile transform

Script runtimes are part of profile enhancement, not platform infrastructure.

- `nyanpasu-scripts` provides the JavaScript and Lua runners and their
  extension definitions. It depends only on the `nyanpasu-config` ports
  `ScriptRunner` and `ProfileContentSource` (`runtime/executor/ports.rs`), not
  on core. `ScriptType` and `ScriptWrapper` (`enhance/mod.rs:23-32`) move below
  core first.
- `nyanpasu-profile-transform` takes the runtime builder and builtin transforms
  from `nyanpasu-core::enhance` and depends on config. Artifact-to-snapshot
  conversion stays in the application layer, because it builds application
  runtime types.
- The transform crate does not depend on the script crate. The shell injects the
  runner, so the engines compile in parallel with transform and core, and a
  shell can leave out an engine through features.
- What a script may reach is part of its extension definition and is injected:
  module loading (including the JavaScript HTTP module loader), network,
  filesystem and console. Runners keep the `ScriptRunner` determinism contract.

### System capabilities and target crates

System capabilities vary along two independent axes.

| Kind                       | Varies with                         | Capabilities                                                                                                          | Owner                                                                                                |
| -------------------------- | ----------------------------------- | --------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| Presentation / interaction | Shell form: desktop, mobile, server | Global hotkeys, windows, tray, widgets, dialogs, notifications, clipboard, opener, deep links, single instance        | The shell, or an interaction crate shared by the shells of one form when the mechanism is Tauri-free |
| OS integration             | Operating system and privileges     | System proxy, PAC, autostart, service install and control, DNS flush, device identity, core binary install, elevation | An OS integration crate per target family, used by every shell on that target                        |

Target crates sit above core and next to the shells that use them. They
implement contracts; core never depends on them.

| Crate (working name)           | Used by                                           | Holds                                                                                                                                                                                                                                                                                                |
| ------------------------------ | ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `nyanpasu-desktop`             | Desktop shells only                               | Tauri-free interaction mechanisms: deep-link registration (the `tauri-plugin-deep-link` fork already exposes callback APIs; replace its `tauri-utils` executable lookup), the single-instance lock, native `rfd` dialogs, the hotkey owner and its parsing and action contracts, main-thread handoff |
| `nyanpasu-os`                  | Desktop shells and the server on desktop or Linux | System proxy, PAC and autostart (now `backend/tauri/src/adapters/system_proxy.rs`), service install and control, DNS flush, core binary install, device identity, elevation mechanism, sidecar version probe                                                                                         |
| `nyanpasu-openwrt` (on resume) | The server on OpenWrt                             | procd lifecycle control, router paths and policy, later firewall integration                                                                                                                                                                                                                         |
| Mobile target (later)          | Mobile shell                                      | Mobile OS and interaction integration                                                                                                                                                                                                                                                                |

- The server never depends on an interaction crate. Tauri-bound mechanisms
  (global-shortcut registrar, tray and window rendering, Tauri dialog,
  notification and clipboard plugins, the app package installer) stay in
  `backend/tauri`.
- Presentation capabilities are optional in core. A shell that lacks one passes
  none, and the dependent operation reports it as unsupported.
- Elevation crosses both axes: the OS crate owns the mechanism, and the shell
  injects how consent is obtained (an authorization dialog on a GUI shell, a
  refusal or a pre-elevated service on the server). Core never shows an OS
  dialog itself.
- The `platform` name is retired. It says nothing about the target or the
  owner, and the target ownership has no catch-all crate.

### Implementation crates

`nyanpasu-platform` is dissolved by owner rather than renamed. Its contents, and
the concrete infrastructure still in core, move as follows. Groups that stay
small may share a crate; heavy-dependency ownership decides the boundary.

| Group                            | Current location                                                                                               | Target                                             |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------- |
| Script runners                   | `nyanpasu-platform/src/enhance/script/`                                                                        | `nyanpasu-scripts`                                 |
| Profile files, subscription HTTP | `nyanpasu-platform/src/profiles/`                                                                              | `nyanpasu-profile-io`                              |
| Runtime-build adapter            | `nyanpasu-platform/src/enhance/runtime_build.rs`; it calls `ProfilesActor::current_closure` and `RuntimePaths` | `nyanpasu-profile-io`, once it uses contracts only |
| Changelog, egress, icon HTTP     | `nyanpasu-platform/src/changelog_http.rs`, `client/direct_egress.rs`, `service/icon.rs`                        | `nyanpasu-net`                                     |
| File watchers                    | `geo/adapters.rs`, `state/profiles/scheduler.rs` (`notify-debouncer-full`)                                     | Injected adapters with their IO crate              |
| Storage backends                 | redb in `storage.rs`, `logs/redb.rs`, `traffic/storage.rs`; `nyanpasu-traffic` legacy importer                 | Storage backends behind owner storage ports        |

## Delivered baseline versus target

The Tauri extraction is delivered in the current tree:
`backend/nyanpasu-core/src/client/` owns the facade, typed clients and
workflows; the sibling domain modules own their actors, services and
transactions. `backend/nyanpasu-platform/src/profiles/`,
`enhance/runtime_build.rs` and `changelog_http.rs` own the concrete profile
file, runtime-build and changelog HTTP implementations.
`backend/tauri/src/setup.rs` constructs and injects these implementations and
the GUI adapters. Tauri retains presentation, desktop plugins, frontend events,
IPC and HTTP transport.

The runtime builder and builtins previously extracted into
`nyanpasu-application` are consolidated in `nyanpasu-core::enhance`. Desktop and
platform callers use core directly; there is no separate application crate.

Known gaps against the target ownership:

- Script runners live in `nyanpasu-platform/src/enhance/script/`. Their only
  core imports are `ScriptType` and `ScriptWrapper` from
  `nyanpasu-core::enhance`. `FsRuntimeBuildAdapter` constructs the runner itself
  instead of receiving it.
- `nyanpasu-platform` depends on core for more than ports: actor helpers,
  `RuntimePaths`, metadata, corruption helpers, YAML parsing, proxy helpers and
  device identity.
- OS integration implementations are split between core and the Tauri crate.
  In core: service install and control with elevation (`service/control.rs`,
  `utils/sudo.rs`, `actor_v2/service_host_adapter.rs`), DNS configuration and
  flush (`actor_v2/local_host.rs`, `client/system_dns.rs`), core binary install
  (`client/core_lifecycle/adapters.rs`), device identity (`utils/hwid.rs`) and
  system proxy reads (`utils/config.rs`). In Tauri: system proxy, PAC and
  autostart (`adapters/system_proxy.rs`) and the sidecar core version reader
  (`utils/core_version.rs`). Core's service install and DNS flush show OS
  authorization dialogs directly.
- `ClientSetupArgs` requires `WindowControl`, `AcceleratorValidator` and a
  `DebugHttpFactory`, so a headless shell must pass stand-ins.
- The updater backend is constructed inside the facade, runtime candidate file
  IO is implemented by `RuntimePaths`, and the traffic store is typed to the
  redb backend in `ClientSetupArgs`. `nyanpasu-traffic` depends on redb
  directly for its legacy importer.
- Concrete HTTP and watcher behavior remains in core: egress probes, icon
  fetching, geodata and external-profile watchers, and service-log IPC
  (`client/logs.rs`).
- Effect execution for hotkeys, tray, widgets and locale, and the main-thread
  handoff, still live in core.
- The RPC registry, command declarations and HTTP server live in the Tauri
  crate ([RPC targets](#rpc-targets)).

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

## Next stage

Each step is one reviewable PR or a short stack. Each leaves a buildable tree,
migrates its desktop callers in the same change and leaves no old-path
re-export shims. After phase A, phases B, C and the protocol half of phase E can
proceed as independent stacks. Phase D waits for B and C; the server and CLI
wait for C and E.

**Phase A: break the cycles and stop the leak**

| Step | Change                                                                                                                              | Done when                                                                                                               |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| A1   | Move owner shutdown support, `BundleMetadata` and the script descriptors below the facade                                           | No lower module imports `client::owner_shutdown`, `client::bundle_metadata` or `enhance::ScriptType`                    |
| A2   | Extract `nyanpasu-scripts` and inject the runner into the runtime-build adapter                                                     | Script engines do not depend on core; JS/Lua golden behavior unchanged                                                  |
| A3   | Extract `nyanpasu-profile-transform`                                                                                                | Transform depends on config only; artifact-to-snapshot conversion stays in application code                             |
| A4   | Extract `nyanpasu-contracts` in bounded groups: profile and materialization, capabilities and update, runtime values                | Each group moves with its full type closure; no `reqwest` or widget IPC cause in contract errors                        |
| A5   | Remove `nyanpasu-platform`'s production dependency on core, split it into `nyanpasu-profile-io` and `nyanpasu-net`, retire the name | Implementation crates depend on contracts and config only; application integration tests move to an upper test boundary |

**Phase B: leaf owners and heavy dependencies**

| Step | Change                                            | Done when                                                                                                  |
| ---- | ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| B1   | Extract persistence, then migration               | Generic recovery no longer examines core-manager errors; migration depends only on persistence and schemas |
| B2   | Move storage backends out of the facade signature | No concrete redb type in `ClientSetupArgs`; traffic's legacy redb importer is feature-gated or separate    |
| B3   | Extract the log, traffic and geodata owners       | Owners consume normalized frames and injected storage and watchers                                         |
| B4   | Extract the core updater and inject its backend   | The facade no longer constructs `HttpUpdaterBackend`; archive dependencies stay with the updater           |

**Phase C: capabilities and target crates**

| Step | Change                                                                                         | Done when                                                                                    |
| ---- | ---------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| C1   | Make presentation capabilities and debug HTTP optional in `ClientSetupArgs`                    | A headless assembly passes no stand-ins; dependent operations report unsupported             |
| C2   | Create `nyanpasu-os` with an elevation consent port; move OS integration out of core and Tauri | Core shows no OS dialog; the Tauri shell injects consent                                     |
| C3   | Create `nyanpasu-desktop`; split shared convergence from presentation effects                  | Hotkey owner, main-thread handoff and desktop effect execution leave the core package family |

**Phase D: transaction graph**

| Step | Change                                                                 | Done when                                                                                                                   |
| ---- | ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| D1   | Extract `nyanpasu-state-engine`                                        | Voting, CAS, rollback and settlement tests pass unchanged                                                                   |
| D2   | Invert mutation participation, then split core control and application | State stores no concrete workflow client; generation fencing, receipts, compensation and caller-drop behavior are preserved |

**Phase E: RPC targets and shells**

| Step | Change                                                                                                  | Done when                                                                                                  |
| ---- | ------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| E1   | Extract `nyanpasu-rpc-protocol`; give the macro neutral declarations and per-target opt-ins             | Command metadata, errors and event names have no Tauri type; bindings regenerate unchanged                 |
| E2   | Extract `nyanpasu-rpc` with the neutral registry and event bus; keep the Tauri IPC binding in the shell | No Tauri type in registration; Tauri events become one subscriber                                          |
| E3   | Add the API-only WebUI transport and the local-socket transport                                         | WebUI and CLI allowlists are enforced by the registry; owner and error behavior match across targets       |
| E4   | Assemble `nyanpasu-server`, then `nyanpasu-cli`                                                         | One long-lived server owns `NyanpasuClient`; the CLI links no core crate and preserves structured outcomes |
| E5   | OpenWrt, after explicit resume                                                                          | LuCI → CLI → server authorization, procd lifecycle, packaging and hardware acceptance verified separately  |

Extend `deno task lint:backend-boundaries` as each crate appears, so the
direction rules in [Target ownership](#target-ownership) stay enforced: no
implementation crate depends on core, the server and CLI do not depend on Tauri
or `nyanpasu-desktop`, and the CLI does not depend on core.

### Build measurement

Compile time is a goal of the split, so measure it rather than assume it.

- Before A1, record a clean `cargo build --timings` of the desktop shell and
  incremental rebuild times after a one-line edit in each of: the facade
  (`client/mod.rs`), an application workflow, a script runner and a desktop
  adapter. Re-measure after phases A and B, and record the results in the PR
  that ends each phase.
- Stop adding boundaries where the gain flattens; merge crates that always
  change together.
- Fix feature wiring that defeats the graph. `backend/tauri` depends on core
  without `default-features = false` (`backend/tauri/Cargo.toml:39`), so its
  `default-meta` forwarding cannot turn the feature off. Enabling
  `test-support` through dev-dependencies
  (`backend/tauri/Cargo.toml:266`, `backend/nyanpasu-platform/Cargo.toml:65`)
  gives core a second feature set, so `cargo build` and `cargo test` compile
  core separately.

## Migration requirements

- Do not add script runtime, OS integration or interaction code to
  `nyanpasu-platform`, and do not add mandatory presentation capabilities to
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
