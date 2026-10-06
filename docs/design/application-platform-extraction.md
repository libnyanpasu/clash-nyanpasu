# Application and platform extraction

Status: implementation plan, 2026-10-06. This is the prerequisite to the
[OpenWrt MVP](openwrt-mvp.md). The extraction takes priority over building a
router-specific facade. Each completed stage is a separate, buildable commit.

## Objective

Tauri is a host environment and GUI/IPC adapter, not the owner of reusable
application behavior. Desktop and OpenWrt must use the same profile, runtime,
script and core application services. Building a second router application that
copies the desktop workflow would leave the architectural problem unresolved.

The target dependency direction is:

```text
Tauri GUI / IPC host              OpenWrt daemon / ubus host
        |                                 |
        +------- nyanpasu-application -----+
        |        NyanpasuClient            |
        |        typed actor clients       |
        |        workflows + ports         |
        |                |                 |
        +------- nyanpasu-platform --------+
                 concrete infrastructure adapters
                 script engines / filesystem / OS / process
                         |
                 nyanpasu-config / nyanpasu-core
                 domain models + persistence primitives
```

This diagram shows construction dependencies as well as execution. The
application crate must never import the platform crate in production: it defines
consumed ports and receives implementations from the host composition root.
Platform may depend on application to implement those ports. Test-only reverse
dependencies should be avoided by placing cross-layer tests in platform or host
tests. Neither neutral crate imports the Tauri host.

Tauri-specific implementations stay in the Tauri host initially, rather than
enabling Tauri by default in the neutral platform crate. A future
`nyanpasu-platform-tauri` can house those adapters after their APIs settle.
OpenWrt-specific procd/ubus implementations belong to its host or platform module,
not to the shared application crate.

## Current boundaries and known coupling

| Existing module                                              | Target owner                               | Boundary to preserve                                           |
| ------------------------------------------------------------ | ------------------------------------------ | -------------------------------------------------------------- |
| `nyanpasu-config::profile/runtime`                           | domain/config                              | Plain schemas, pure executor and consumed ports                |
| `tauri::enhance::RuntimeBuilder`                             | application                                | Build from explicit snapshots and content/script ports         |
| `tauri::enhance::script`                                     | platform                                   | Boa/Lua execution, module cache and blocking runtime           |
| `tauri::enhance::FsProfileContentSource`                     | platform                                   | Explicit profile-directory filesystem adapter                  |
| `tauri::enhance::artifact_snapshot`                          | application after runtime model extraction | Map domain artifacts into inspection/read models               |
| `tauri::state::{application,clash_config,profiles,mutation}` | application                                | Actor ownership and transaction/required-participant semantics |
| `tauri::client::application_workflow`                        | application                                | Runtime voting, commit, compensation and recovery              |
| `tauri::client::profiles`                                    | application                                | Existing profile orchestration, not a new router journal       |
| `tauri::core::actor_v2`                                      | application plus platform adapters         | Application intent and host-independent core control           |
| `tauri::client::system_proxy/hotkey/ui_effects`              | application actors plus Tauri/OS adapters  | Platform capabilities are optional and explicit                |
| `tauri::unified_rpc`, macros, Specta export                  | transport package plus host                | Shared errors/metadata independent of Tauri dispatch           |
| setup, window, tray, plugins, updater adapter                | Tauri host                                 | Concrete desktop lifecycle and capabilities                    |

An important hidden dependency is config → egui for `StatisticWidgetVariant`.
That type is a wire enum, not GUI infrastructure. It must move to an existing
neutral owner (`nyanpasu-helper`) without changing serialization, Specta type name
or clap choices. egui consumes the neutral enum; config no longer imports egui.

The desktop facade currently references private runtime, core, storage, event,
profile, path, effect and server modules. Merely moving `client/mod.rs` would
produce a service locator, re-export shell or circular dependencies. The migration
must move the smallest complete call paths and then assemble the real facade from
those owners. Existing transactional behavior is part of the application API.

## Stage 1: shared runtime and concrete script platform

Create `backend/nyanpasu-application` and `backend/nyanpasu-platform`, and migrate
the real desktop runtime-build path to them.

Application owns `RuntimeBuildInput`, `RuntimeBuildError`, build diagnostics,
`RuntimeBuilder`, builtin transform definitions/sources, core gating and TUN
flavor derivation. These perform deterministic orchestration through the existing
config executor ports. It owns any pure script descriptor types still needed by
callers, without owning a script runtime, directory or module downloader.

Platform owns `EnhanceScriptRunner`, `ScriptDirs`, Boa/Lua runners, console/order
adapters and `FsProfileContentSource`. Script directories and content root are
explicit constructor parameters. There is no `PathResolver` dependency in a
neutral crate. Filesystem/network/script-engine access is classified as adapter
behavior, even when called from a deterministic build workflow.

Tauri's build adapter continues to freeze sources, run blocking work, validate
transforms and publish its product using the new crates. Preserve caller-visible
errors, source provenance, JS/Lua order, mapping key order, built-in gating and
failed-transform logs. Remove the old implementations rather than retaining a
second copy or old-path wrapper only to avoid updating callers.

Tests move with the behavior: pure assembly tests into application, engine and
cross-layer tests into platform; desktop projection/transaction tests remain with
the host until their dependent models move. No production app → platform cycle.

Acceptance:

- Both neutral crates compile independently of the Tauri package.
- Dependency trees exclude Tauri and egui/eframe.
- Application does not depend on Boa/Lua, platform or Tokio runtime construction.
- Desktop code uses the shared implementation for every migrated caller.
- Actual JS and Lua execution tests pass, including ordering and failure logs.
- Desktop library checks, relevant tests, format and architecture gates pass or
  an independently established environmental/baseline blocker is recorded.

## Stage 2: application ports and existing actor ownership

Move existing neutral lifecycle/main-thread ports and complete actor/client
slices, updating desktop callers and real composition-root injection in the
same commit. Preserve ordinary async typed APIs and owner cleanup. Do not use
OpenWrt as a reason to add another queue, global or caller-owned operation.

For every candidate slice, list its actual dependencies before moving it:

1. Domain inputs/outputs and errors → application/config.
2. Mutable state and sequencing → application actor.
3. Storage/OS/process/UI access → consumed application port.
4. Concrete implementation → platform or host adapter.
5. Construction, root token and task tracking → host composition root.

Ports needed by only one domain belong to that domain, not a generic capability
registry. A main-thread executor can be shared because multiple domain adapters
already consume it. Actor references remain hidden behind typed clients.

The profile slice is complete only when its mutation coordinator, materialization
protocol, source state, runtime required-participant vote and post-commit effects
remain correct. There is no independent router store/journal while desktop
transactions remain locked inside Tauri. If that slice has a larger dependent
graph, document it as the next migration stage rather than replacing it with a
simpler behavioral fork.

## Stage 3: host-independent facade and RPC runtime

Move the real `NyanpasuClient` once its consumed domains/ports can reside in the
application crate. Define capability-specific setup arguments with no `AppHandle`,
`Window`, `Webview`, router or desktop server types. Hosts assemble supported
actors and adapters; unavailable desktop capabilities return an explicit
unsupported state at the boundary, not silently successful no-op effects.

Separate neutral RPC invocation/errors/query-mutation metadata from transport
dispatch. Desktop IPC and HTTP remain adapters with their existing authorization,
owner/resource isolation, event-resync and structured-error behavior. An ubus
adapter exposes an explicit allowlist of shared application operations; it does
not require JSON-RPC or OpenRPC and cannot forward arbitrary desktop commands.

The facade exposes use cases, never `get_service`, actor registry, transport router
or runtime downcasting. The Tauri host retains only composition, GUI/plugin
adapters, bootstrap/shutdown and transport registration.

## Stage 4: OpenWrt MVP host

Only after the required application slices are reusable should the OpenWrt host
compose those same services with filesystem/procd/libubus adapters and LuCI.
Use the [MVP design](openwrt-mvp.md) for its selected functionality, schemas and
Docker acceptance. Reconcile that contract with the extracted shared API rather
than creating a router-specific mirror of the facade.

The original ubus/LuCI prototype files are preparatory work, not evidence that
the application extraction or MVP is complete. They must not be committed as a
finished phase until integrated, built and tested against the shared services.

## Compatibility, verification and commits

### Implemented first slice

`nyanpasu-application` owns RuntimeBuilder, runtime build inputs/errors and script
descriptors. `nyanpasu-platform` owns filesystem profile reads, Boa/JavaScript,
Lua and the concrete script runner. The desktop workflow calls these crates
directly; the old host implementations have been removed. Desktop artifact/log
projection remains in the host. The configuration widget wire enum now lives in
the neutral helper crate, removing the transitive egui dependency from config.

Verification includes seven application tests, 49 platform unit tests, a real
filesystem/Boa/RuntimeBuilder integration test and four desktop runtime goldens.
The backend dependency gate checks transitive production/build dependencies on
all targets. `deno task test:backend-musl arm64` and `amd64` build the same shared
tests in Docker and run their binaries in ImmortalWrt rootfs containers; this is
a test harness, not an OpenWrt daemon or package acceptance test.
The aarch64 and x86_64 runs passed all 57 shared tests in their ImmortalWrt
24.10.4 rootfs containers.
The harness checks the actual rootfs musl loader because its ARM image tag
currently publishes amd64 Docker metadata. Builder architecture stays explicit.

The next actor slice is the existing CoreActor/CoreClient and its API lease,
with local/service endpoints implemented in platform, plus persistent session
state with host-independent actor construction. Profile transactions, application
workflow, facade composition and RPC registration still belong to the desktop
host until their whole dependency paths can be moved.

The independent session-state slice now uses
`nyanpasu_application::session_state::SessionStateClient` in the desktop host.
Platform opens the persistent manager; application receives it and the
host-provided window label, root cancellation token and task tracker. The old
desktop session actor/client have been removed. Calls accepted before shutdown
drain to completion; new requests fail once cancellation is observed. Desktop
maps shared errors to its original ConfigError variants before IPC serialization.

The subsequent full transaction path is detailed in
[Shared application facade and profile transaction extraction](application-transaction-extraction.md).

Do not change profile schema, generated frontend bindings or desktop RPC names
merely because code moves to a crate. Pure type movement must preserve names and
wire shape. Generated bindings are regenerated only through their existing export
workflow when the public export actually changes.

Existing facade methods may compose extracted clients during the migration.
That is a real composition boundary, not a legacy compatibility API. Old service
implementations and hidden globals are removed from the migrated path. A necessary
temporary bridge requires the existing `TODO(actor-migration)` reason and removal
condition, with no new callers through that bridge outside the migration.

For each phase:

1. Update the design with concrete owners and the remaining dependency graph.
2. Implement the entire chosen call path, preserving existing behavior.
3. Run focused behavior tests and inspect the crate dependency tree.
4. Check the desktop consumer, formatting and relevant architecture gates.
5. Stage explicit paths and create one atomic local commit with rationale.

No incomplete/fix-up phase commits, blanket staging, lockfile drift from discarded
prototypes or unrelated cleanup. User-requested commits are local unless a push is
separately authorized. Docker builds can validate neutral Linux/musl boundaries;
they do not establish hardware router, SDK package or production deployment.
