# Architecture and ownership

These rules apply to all contributors. Application services use explicit
dependencies and serial state owners; global services and Tauri-coupled business
logic have been removed. The application core lives in `nyanpasu-core` and GUI
adapters live in Tauri. Package ownership for scripts, OS integration and
infrastructure is being corrected ([Core and frontend
separation](#core-and-frontend-separation)).
For the development workflow, start with the [guide](README.md).

## Target architecture

The target architecture is:

```text
Tauri commands / UI adapters
    -> NyanpasuClient
        -> typed actor clients
        -> pure services
        -> adapter traits
            -> concrete Tauri / OS / filesystem / network implementations
```

Use these terms consistently:

- **Dependency Injection / Pure DI**: dependencies are passed explicitly through constructors, builders, function arguments, or actor startup arguments. Do not look them up through globals.
- **Composition Root**: the bootstrap / supervisor location that builds the full object graph and actor graph.
- **Ports and Adapters**: core/application code depends on traits; Tauri, filesystem, OS, network, and process implementations live behind adapters.
- **Actor service**: a ractor actor that owns mutable state, serializes commands, manages long-running resources, or supervises background work.
- **Pure service**: a stateless or short-lived service that performs deterministic computation, validation, conversion, config generation, serialization, or patch application without IPC or background lifecycle.
- **Adapter / port**: a narrow trait and concrete boundary implementation for infrastructure such as Tauri, filesystem, OS APIs, process spawning, HTTP, logging sinks, or storage.

`NyanpasuClient` is the application facade. The application bootstrap / supervisor is the composition root. It constructs concrete services, spawns actors, wires dependencies, and returns a ready-to-use `NyanpasuClient`.

## Core and frontend separation

The Tauri extraction baseline is complete: core no longer depends on Tauri. The
Phase A–D package split and A2b script extension work are delivered; the
remaining RPC and shell work is planned below. The current layout is:

```text
backend/tauri/src/
  setup.rs                 desktop composition root
  adapters/                GUI adapters and frontend event delivery
  desktop/                 hotkey owner, main-thread handoff and presentation effects
  ipc.rs, unified_rpc.rs   RPC transport adapters
          |
          | constructs and injects dependencies
          v
backend/nyanpasu-core/src/
  client/                  NyanpasuClient, typed clients, workflows, runtime builds and artifact conversion
  state/                   configuration source owners
  actor_v2/, clash/        core control and API streams
  logs/                    redb log storage and codec adapters
  geo/, traffic/           geodata and traffic accounting
  service/, utils/         consumed ports, services and shared utilities
          |
          v
nyanpasu-profile-transform  runtime builder, builtin transforms and output types
          |
          v
nyanpasu-config             domain models and pure runtime executor

backend/nyanpasu-rpc/src/          shared commands, registry, audit, event bus and optional HTTP adapter
backend/nyanpasu-rpc-protocol/src/ RPC owner values, request/error envelopes, JSON values and shared event names
backend/nyanpasu-state-engine/src/ generic state coordination, persistence and transaction settlement
backend/nyanpasu-observers/src/    log capture owner and storage ports
backend/nyanpasu-updater/src/      core update owner, HTTP backend, downloads and unpacking
backend/nyanpasu-migration/src/    configuration upgrades, backups and session document reads
backend/nyanpasu-persistence/src/  generic filesystem retry and evidence recovery
backend/nyanpasu-profile-io/src/   profile files, materialization and content adapter
backend/nyanpasu-net/src/          subscription, changelog, egress, icon, diagnostics and HTTP policy
backend/nyanpasu-os/src/           proxy, PAC, autostart, device identity, version probes, service control, DNS flush and binary installation
backend/nyanpasu-scripts/src/      JavaScript and Lua script runners over config ports
```

`NyanpasuClient` and its actor/workflow graph live in
`backend/nyanpasu-core/src/client/` and the domain modules beside it. Concrete
profile file implementations live in `nyanpasu-profile-io`; HTTP implementations
and shared client policy live in `nyanpasu-net` over foundation contracts.
Application integration tests live in core with profile-io and net as
dev-dependencies; Tauri owns its host test assembly.
The desktop composition root in
`backend/tauri/src/setup.rs` constructs these implementations and the GUI
adapters, injects them into the core and calls `NyanpasuClient::try_new`. GUI
adapters remain in `backend/tauri/src/adapters/`; windows, tray, widgets,
desktop plugins and transport delivery stay in the host. Core's exported test
support accepts injected host adapters without linking implementation crates.

The [target ownership](../design/application-platform-extraction.md#target-ownership)
is the plan for the next stage:

- **Shells** compose the application and serve it over RPC. The desktop shell
  runs core in process. `nyanpasu-server` has a headless composition library and
  long-lived executable. `nyanpasu-cli` (planned) is a
  client of a running shell, not a composition root. The router form is LuCI → CLI → server.
- **RPC targets** are Tauri IPC (the full desktop surface), WebUI over HTTP
  (an explicit, restricted allowlist) and the CLI over a local socket (a
  separate explicit allowlist). Planned E1a keeps known commands registered:
  missing required capabilities return structured `unsupported`; only unknown
  names return `unknown_method`. Capability metadata is not per-call authority.
- **RPC packages** include the E1a-b protocol base: owner values, request/error
  envelopes, transparent JSON and shared event identities, consuming approved
  foundation values alongside serialization and optional transport dependencies. Cookie parsing, HTTP status and application error conversion live
  in `nyanpasu-rpc`. `nyanpasu-rpc-protocol::types` contains the receiving
  error/mutation graph, owned error envelope and connection details for the
  thirteen local operations and their five full-value snapshots. `operation` and
  `operations` bind local request/sending/receiving/error types and method names
  on both the client and generated shared handlers. Ordinary values
  have canonical foundation owners; `nyanpasu-rpc` is the thin application adapter
  over core. Protocol forbids GUI,
  CORE_FAMILY and IMPLEMENTATION; foundation/core/implementation crates cannot
  depend on the RPC adapter. The CLI links protocol/local-client, never the
  registry/server/core-family/implementation/GUI production closure; foundation
  values through protocol remain allowed. E2b moved the registry, event bus and
  C2f audit kernel and neutral production `TracingRpcAuditSink` together.
  E2c publishes observations through `nyanpasu-rpc::events` and `forward`.
  The injected native sink finishes delivery synchronously, preserving window
  targeting/errors; SSE and future local receivers use the bounded broadcast
  with lag/resync. The shell input bridge is removed. Forwarder construction
  acquires all seven subscriptions before reconciliation/stream/listener
  activation; the composition root tracks their futures until shutdown.
  E2a first closes shared command signatures; protocol owns the shared icon-mode
  value, while asset access remains shell functions. Local subscription control
  supplies initial snapshots and lag resets only for full-value configuration,
  core, service, app-update and core-log status events; other event identities
  are excluded from E3 local subscriptions. The command allowlist is unchanged;
  connection details remain a separate demand stream. Successful late resource results are
  closed promptly; unconfirmed Service log opens rely on the existing service
  lease because no returned ID is available (design V-61).
  All E cards follow D2 → D3 → D4 → A2b, then E1a → E2 → E3 → E1b → E4. E3 adds negotiated local JSON framing on Unix and Windows.
  E3d delivers connection-owned resources and INFO audit through the same kernel.
  Its server drains accepted work after reader/writer closure, registers and
  closes successful late resources, and observes task panics with their original
  payload. A shared bounded capacity budget covers pending/established resources;
  unsubscribe releases receivers before acknowledgement. Full-value snapshots
  come from the facade, with bounded retry when publication races a read; local
  event notifications refresh that value so old queued observations cannot
  regress a reset. Independent sources retain no causal total-order promise.
  Service opens without a returned ID remain subject to the existing lease and
  separate physical-store grace, not an immediate disconnect cleanup guarantee. Fatal
  panics remain fatal; a dedicated client transport task owns each connection,
  so dropping a call future cannot desynchronize partial IO. A timeout/error
  invalidates the connection without cancelling owner work or retrying mutations.
  Typed calls go through the thirteen `RpcOperation` contracts: the client sends
  the contract's request and decodes its receiving response, while remote errors
  retain their complete envelope and separately decodable domain payload. The
  generic JSON API remains. Client-only consumers compile without the RPC adapter
  or application crates.
- **Server composition (E4a)** provides a library composition root with one
  client and registry, keeps shared effects
  and neutral sinks, and omits optional presentation capabilities. It injects
  `RefuseElevation` into service control, DNS flush and binary installation.
  Metadata, paths, executable source and logger are explicit host inputs. Script
  engines use server-owned extension and IO adapters. Redb log storage retains
  the observers unavailable fallback. E4b adds executable startup, a process
  panic policy installed before Tokio, forwarder activation and listener lifecycle.
  Forwarders subscribe before reconciliation and stream activation. Readiness
  follows listener binding; transport admission stops before application owners
  drain. Network drain deadlines do not bound owner cleanup. HTTP/local/owner
  integration tests launch the real executable through `CARGO_BIN_EXE_nyanpasu-server`,
  covering startup, composition and fatal exit without converting a panic to an RPC error.
  The default-off `panic-injection` feature is enabled only by the server's own
  dev-dependency; production closure checks reject it, and the separate musl
  production build leaves it off. Musl collects the `panic_process` integration
  target as well as the library and binary unit targets. Lifecycle tests gate
  reconciliation and stream activation, capture events before readiness and
  verify local and HTTP endpoint release after startup failure.
  This closes those paths only in the separate server; embedded desktop HTTP
  retains the desktop policy. E5/OpenWrt and LAN ticket login remain paused.
  The executable plan is [tasks §5](../spec/2026-11-03-core-package-split/tasks.md#5-phase-e-的-pr-划分).
- **The core package family** keeps facade, application and control as modules
  in `nyanpasu-core`, above migration, owner-support, observers and updater.
  D1 delivered `nyanpasu-state-engine` as a core-family leaf: among parent-workspace
  packages it depends only on persistence, with default features disabled; other
  core-family members and parent-workspace packages are forbidden. The purpose
  is dependency enforcement, not an assumed compile-time gain.
- **Foundation layers** retain config, contracts, persistence and profile transform
  as leaves. Phase D delivered `nyanpasu-runtime-values` above them for public mutation
  results, degradation and runtime/profile errors, with their complete value
  closure. Existing foundation leaves cannot depend on it. It may consume
  config/contracts/transform and neutral core metadata, but not core family,
  engine, implementation crates, GUI or core manager.
- **Script runtimes** belong to profile enhancement: a script crate provides the
  runners and extension definitions over the `nyanpasu-config` ports, and a
  profile transform crate below core holds the runtime builder.
- **Target crates** hold OS integration (system proxy, autostart, service
  control, DNS, device identity, elevation) over the foundation layer, and
  shells inject them. Desktop interaction mechanisms (hotkeys, main-thread
  handoff, tray, widget and locale effects) stay in the Tauri shell; there is
  no desktop interaction crate.
- **Implementation crates** (scripts, profile IO, HTTP, storage backends)
  depend on the foundation layer only, never on core or each other.

The delivered Phase D module boundary (tasks §4) classifies
`actor_v2/`, `clash/`, `proxies.rs` and `connections.rs` as control, `client/`
as application/facade, and `state/` as source ownership. Application may import
control; control must not import `client`. Source owners must not directly import
`client::application_workflow` or `ApplicationWorkflowClient`. They consume the
factory defined in `state/mutation/ports.rs`; `client/application_workflow/participant.rs`
implements it. Both internal receipts, `MutationReceipt` and `RuntimeApplyReceipt`,
stay under `state/mutation/`; public wire values live in runtime-values. The
notification contract remains in `client/effects`; `EffectKind` lives in
`nyanpasu-runtime-values::configuration`. The policy
allows source dependencies on effects, jobs and value/validation contracts. It
uses a bounded syntax guard for explicit use trees/paths and lexical scopes,
including tests. Same-core provider aliases/re-exports are followed for at most
eight hops; cycles, ambiguity and unsupported forms fail explicitly in
the policed scope. `self::*` and `super::*` are allowed only when the named module
is also policed, with inherited alias bindings resolved recursively at their
declaration scope. Other globs, including provider re-export globs, are rejected;
`r#` anywhere in a checked path is rejected too. Local service bridges and external `nyanpasu_ipc::client`
remain allowed. This is not Rust type/macro resolution or a proof of transitive
independence of `state` and `client`. D2f supplies fixtures for these exact rules
where Cargo cannot enforce an intra-crate direction. No control/application crates are
created; a future independent consumer requires its own extraction inventory.

D2c makes `RecoveryRequired` wait for the source's authoritative decision before
terminal receipt and journal settlement. A2b delivers shell-injected script
extensions and explicit JavaScript/Lua features. An engine is available only
when scripts compiles it and the shell supplies its extensions; the factory's
default methods decline with typed `StartEngine`. Pipeline and wire errors
preserve that runtime/operation, including Lua predicates and expressions.
Script determinism assumes equal observations of injected network, files and
clock. These boundaries make no desktop compile-time gain claim.

The extraction record lists the known gaps and the order of the next stage. The
[OpenWrt reconciliation roadmap](../design/openwrt-roadmap.md) records the
paused router work.

Rules for maintaining the separation:

- Do not add `tauri` dependencies to `NyanpasuClient`, typed clients, actors, or pure
  services: no `AppHandle`, `tauri::State`, `tauri::async_runtime`, Tauri events,
  windows, or tray handles. Use `tokio` directly and add a port trait for anything
  the frontend must provide.
- Port traits are owned by their application/domain consumers; Tauri implementations
  stay in the GUI crate. Classify contracts by their actual consumer: window, tray,
  widget, webview and main-thread presentation behavior belongs with frontend
  adapters, not mandatory headless dependencies. Retained optional capabilities
  need explicit availability and outcome semantics; no-op business effects must
  not report successful application.
- RPC commands, the Tauri IPC transport, and the HTTP server are transport adapters
  over `NyanpasuClient`; keep business orchestration out of them so they stay thin.
- Move code bottom-up: a module moves into its neutral owner once its dependencies are
  Tauri-free. Update callers to the new path; do not leave re-export shims in the
  Tauri crate.
- Tests of moved code construct the client or service graph with fake adapters and
  must not need the Tauri runtime.
- The architecture-ledger static gate covers `backend/tauri/src/`,
  `backend/nyanpasu-core/src/`, `backend/nyanpasu-state-engine/src/`,
  `backend/nyanpasu-runtime-values/src/`, `backend/nyanpasu-rpc-protocol/src/`,
  `backend/nyanpasu-rpc/src/`,
  `backend/nyanpasu-server/src/`,
  `backend/nyanpasu-contracts/src/`,
  `backend/nyanpasu-scripts/src/`, `backend/boa_utils/src/`,
  `backend/nyanpasu-profile-transform/src/`,
  `backend/nyanpasu-persistence/src/`, `backend/nyanpasu-profile-io/src/`,
  `backend/nyanpasu-net/src/`, `backend/nyanpasu-migration/src/`,
  `backend/nyanpasu-owner-support/src/`, `backend/nyanpasu-observers/src/`,
  `backend/nyanpasu-updater/src/` and `backend/nyanpasu-os/src/`
  (`STATIC_GATE_PREFIXES` in `scripts/src/architecture-ledger/policy.ts`).
  All nineteen use the same explicit path-and-name allowlist; other backend library
  statics remain outside this gate. The backend dependency gate complements
  this source scan; it does not replace it.

## Actor model and ownership

In this repository, `ractor` means the Rust `ractor` crate. It is an in-process actor framework used for long-lived services that own state and communicate through typed messages. It is not Tauri IPC and not Ruby Ractor.

Use this mental model:

```text
Actor = private mutable state + typed message enum + sequential message handling + lifecycle hooks
```

Core concepts:

- `Actor`: the implementation trait. An actor defines its `Msg`, `State`, `Arguments`, and lifecycle/message-handling methods.
- `ActorRef<Msg>`: a typed address used to send messages to an actor. Hide raw `ActorRef` values behind typed clients such as `StateClient` or `CoreClient`.
- Message enum: the actor's domain protocol. Prefer explicit messages such as `PatchAppConfig`, `RestartCore`, or `SelectProxy` over generic commands.
- `RpcReplyPort<T>`: the usual request/reply mechanism for queries and fallible operations that must return a value.
- Fire-and-forget messages: use only for notifications, invalidations, events, or best-effort work where the caller does not need a result.
- Startup arguments: the actor's dependency injection boundary. Pass dependencies when spawning the actor; do not fetch them from globals in actor code.

Project rules:

- Use actors for services with long-lived mutable state, serialized commands, background tasks, streams, timers, file watchers, process lifecycles, sockets, subscriptions, downloads, or child supervision.
- Do not use actors for deterministic computation. Use pure services for validation, schema conversion, patch application, runtime config building, serialization, and merge/enhance logic when it can be deterministic.
- Put infrastructure access behind adapter traits. Inject Tauri, OS, filesystem, network, process, and logging adapters into the actor or pure service that needs them.
- The composition root spawns actors, wires dependencies, and returns `NyanpasuClient`. Do not use a ractor registry, actor name lookup, or raw `ActorRef` map as a replacement for dependency injection.
- `NyanpasuClient` and typed actor clients should expose ordinary async Rust methods. Most callers should not use ractor APIs directly.
- In-process request/reply calls wait for the real result (no timeout). Only adapters that perform network IO define deadlines; IPC deadlines belong to the IPC client layer, not to its callers.
- Avoid synchronous cross-actor cycles such as `StateActor -> CoreActor -> StateActor`.
- A panic means the code reached a state it must not reach, so it interrupts execution. Do not write `catch_unwind` in production code, and do not turn a panicking task's `JoinError` into an ordinary error. The only recovery path is `nyanpasu_panics::catch_recoverable`, under the conditions in [Rust code guide](rust.md#73-panic-recovery).
- Run UI-thread work through the injected `MainThreadExecutor`; clients and actors do not call Tauri's main-thread APIs themselves.
- Notifications flow downstream only, one domain's slice from that domain's serial owner. An owner never reads or forwards a sibling domain's snapshot, so dependencies form a tree, not a graph.
- Preserve the source and intent of actor messages, event variants, and notification slices. If two sources share downstream handling, keep their distinct variants and handle them together; do not rename or reuse an existing source's variant to represent a new trigger.

If a mature ractor actor client already exists for a capability, use it instead of adding a new global singleton, raw channel loop, or direct Tauri-coupled service call.

## Dependency and boundary rules

### Backend package direction

Shared backend code follows [Backend packages](backend-packages.md):
`nyanpasu-core` owns use cases, typed actor clients and consumed ports;
implementation crates implement foundation contracts and depend on the foundation
layer only, never on core or each other. Core must not depend on an implementation
crate in production. See the [target
ownership](../design/application-platform-extraction.md#target-ownership).
Neutral crates must not depend on Tauri, egui/eframe or Slint. Tauri is a host composition
root and GUI/transport adapter; Tauri-specific implementations stay at that
boundary. Move complete existing call paths and update callers without retaining
old-path wrappers solely to avoid import changes. Run
`pnpm run lint:backend-boundaries` after backend package-boundary changes.

### Do not add new global service singletons

Do not introduce new service accessors such as:

```rust
Service::global()
get_global_service()
static SERVICE: OnceCell<Service>
static SERVICE: OnceLock<Service>
static SERVICE: Lazy<Service>
```

Exceptions are allowed only for immutable constants, static lookup tables, feature flags, or values that are truly process-wide and have no lifecycle, no mutable state, and no dependency graph.

Allowed statics are listed in `scripts/src/architecture-ledger/policy.ts`, and `pnpm run lint:architecture-ledger` rejects new ones.

### Prefer explicit construction

Services must be constructed through one of these forms:

```rust
Service::new(dependency_a, dependency_b)
ServiceBuilder::default().with_dependency(...).build()
AppSupervisor::start(args).await
```

Dependencies should be visible in struct fields, constructor parameters, builder parameters, function parameters, or actor startup arguments. Hidden dependencies are not allowed.

### Keep `NyanpasuClient` as a facade, not a service locator

`NyanpasuClient` may expose stable application APIs, for example:

```rust
client.get_app_config().await?;
client.patch_app_config(patch).await?;
client.get_profiles().await?;
client.restart_core().await?;
client.select_proxy(group, name).await?;
```

It must not expose arbitrary internal lookup APIs such as:

```rust
client.get_any_service::<T>()
client.resolve::<T>()
client.resolve("service-name")
client.get_service("name")
client.actor_registry()
client.get_actor_ref("state")
```

Internally, `NyanpasuClient` may hold typed clients such as `StateClient`, `CoreClient`, `SystemProxyClient`, `HotkeyClient`, `ProxiesClient`, and pure services or adapter trait objects. Callers should not depend on ractor `ActorRef` directly unless they are part of the actor layer itself.

### Keep Tauri at the boundary

Business logic must not depend directly on Tauri types such as `AppHandle`, `Window`, `Manager`, tray handles, global command state, or Tauri event emitters. Use adapter traits instead.

## Choosing a service

Before adding or moving a service, classify it as an actor service, pure service, or adapter/port.

### Use an actor service when the service:

- owns long-lived mutable state;
- must serialize commands to avoid races;
- manages background tasks, streams, timers, file watchers, process lifecycles, sockets, subscriptions, or downloads;
- supervises child tasks or child actors;
- needs request/reply or fire-and-forget messaging;
- coordinates side effects after state commits.

Expected examples:

- app/config state;
- core process lifecycle;
- system proxy state;
- hotkey registration;
- proxy cache and subscriptions;
- updater downloads;
- websocket connection managers;
- server lifecycle.

Actor implementation rules:

- Use a typed message enum.
- Keep owned mutable state inside the actor state.
- Use typed actor client wrappers for public calls.
- Do not expose raw `ActorRef` outside actor/application internals.
- Use request/reply for fallible operations and queries.
- Use fire-and-forget only for events, notifications, or best-effort work.
- Avoid cross-actor synchronous cycles.
- Do not add in-process RPC timeouts; deadlines belong to network IO adapters and the IPC client layer.
- The mailbox is the actor's only serialization. Do not build a second queue, scheduler, priority, or admission layer inside an actor; the handler awaits the whole command. State that is not an actor's uses a lock.
- Waiters do not own operations: dropping a caller does not cancel work the owner has started; the owner runs it to a terminal state.
- Shutdown is one root `CancellationToken` plus each owner's own cleanup: the handler refuses new work once the token is cancelled, and cleanup runs in `post_stop`. There is no global shutdown phase or budget.
- Actor startup arguments must contain all dependencies required to build the actor state.
- Do not share actor-owned mutable state with `Arc<Mutex<_>>` or `Arc<RwLock<_>>` unless it is a narrowly scoped implementation detail with a clear comment.

### Use a pure service when the service:

- performs deterministic computation;
- validates input;
- converts schemas;
- applies patches to owned data passed as parameters;
- builds runtime configuration from snapshots;
- serializes or deserializes data without owning long-lived state;
- has no background task and no independent lifecycle.

Expected examples:

- config validation;
- patch application;
- profile ordering;
- runtime config building;
- legacy schema conversion;
- serialization helpers;
- merge/enhance logic when it can be made deterministic.

Pure service rules:

- No global state.
- No background tasks.
- No hidden filesystem/network/Tauri access.
- All inputs must be explicit parameters.
- Return values or domain errors instead of mutating external state.

### Use an adapter / port when the service touches infrastructure:

- Tauri events, windows, tray, dialogs, clipboard;
- filesystem and app directories;
- OS proxy APIs;
- global shortcuts;
- HTTP clients;
- child process spawning;
- logging sinks;
- persistent storage backends.

Adapter rules:

- Core/application code depends on traits.
- Concrete adapters live at the boundary crate/module.
- Keep adapter traits narrow and task-oriented.
- Prefer mockable traits for tests.
- Prefer traits owned by the consuming crate/module when that improves boundary clarity.

## Legacy globals

The legacy `::global()` services (`Config::global()`, `CoreManager::global()`,
`Handle::global()`, and the rest) have been removed. Do not reintroduce them or a
wrapper that hides equivalent process-wide mutable state; add the operation to
`NyanpasuClient`, a typed client, or a pure service instead.

## State and configuration

State changes follow this order:

1. Keep state ownership in `StateActor` or a state manager owned by `StateActor`.
2. Keep schema and patch operations in pure services or domain types.
3. Generate runtime config from snapshots rather than mutating runtime globals.
4. Commit state first, then trigger side effects through actor messages. A Required participant (the runtime) is a vote before the commit, not a side effect; this rule covers ordinary side effects.
5. Report post-commit side-effect failures as degraded results instead of silently rolling back persisted state.

Prefer a migratable breaking change that updates callers over preserving an old configuration API.

## Compatibility policy

When refactoring or moving services (including into `nyanpasu-core`):

- Prefer fully migrating callers to the new injected/actor/pure-service API.
- Prefer migratable breaking changes over compatibility layers.
- Do not add a compatibility layer simply to avoid updating call sites.
- Add a compatibility or migration layer only when a full migration is not currently possible due to cyclic dependencies, public API constraints, external plugin behavior, large cross-cutting risk, platform limitation, or staged release requirements.
- Every compatibility layer must be explicitly marked with `TODO(actor-migration)` or `FIXME(actor-migration)` and must explain the reason and removal condition. The marker keeps its historical name because `pnpm run lint:architecture-ledger` counts it.
- New code must not call compatibility APIs unless the call site is itself part of a documented migration step.

Do this:

```rust
client.patch_app_config(patch).await?;
```

Do not do this unless blocked:

```rust
LegacyConfigCompat::patch_verge(patch).await?;
```

Required comment format:

```rust
// TODO(actor-migration): compatibility bridge for <legacy API>.
// Reason: <why full migration is blocked>.
// Remove when: <specific condition or tracking issue>.
```

or:

```rust
// FIXME(actor-migration): legacy behavior kept temporarily for <reason>.
// New code must use <new API>. Remove after <condition>.
```

## Naming

Use names that reveal the role:

- `StateActor`, `CoreActor`, `SystemProxyActor`, `HotkeyActor`, `ProxiesActor`, `UpdaterActor` for actors.
- `StateClient`, `CoreClient`, `SystemProxyClient`, `HotkeyClient`, `ProxiesClient` for typed actor clients.
- `RuntimeBuilder`, `ProfileMerger`, `ConfigMigrator`, `PatchValidator` for pure services.
- `TauriUiEventSink`, `FsConfigStore`, `OsProxyBackend`, `ProcessRunner` for adapters.
- `AppSupervisor` or `NyanpasuBootstrap` for the composition root.

E2b keeps native entry/Channel binding, Tauri event derives and asset resolution
in the shell. Since E3a the neutral HTTP actor starts without frontend assets and
mounts the frontend fallback only when assets are supplied. `http` is an explicit default-off feature of RPC, and its weak routes
retain the existing client shutdown behavior. Lower layers cannot depend on RPC;
RPC cannot depend on GUI or implementation crates.
