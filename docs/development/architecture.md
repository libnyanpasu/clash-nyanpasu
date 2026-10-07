# Architecture and ownership

These rules apply to all contributors. Application services use explicit
dependencies and serial state owners; global services and Tauri-coupled business
logic have been removed. The current goal is to move the application core out of
the Tauri GUI ([Core and frontend separation](#core-and-frontend-separation)).
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

The actor/dependency-injection migration is complete. The current architectural
goal is to separate the application core from the Tauri GUI:

```text
Tauri GUI (backend/tauri)      nyanpasu-cli (planned)      mobile shells (later)
  window / tray / webview        terminal adapters           platform adapters
  Tauri IPC, HTTP RPC
            \                         |                          /
             +------ build the core with concrete adapters ------+
                                      |
                       backend/nyanpasu-core
                         NyanpasuClient, typed clients, actors,
                         workflows, pure services, consumed ports
                                      |
                       nyanpasu-config
                         domain models and pure runtime executor
```

`NyanpasuClient` currently lives in `backend/tauri/src/client/` together with the
actors, services, and ports it depends on. These application capabilities move into
`backend/nyanpasu-core`, alongside its existing state transaction machinery.
No separate application or platform crate is required in the target layout.
Reusable non-GUI adapters belong in capability-local core modules behind narrow
ports, with explicit inputs supplied by the host. Each frontend
then supplies its concrete adapters and composes the core; the core never depends on
a frontend. This split is the prerequisite for mobile support. The
[OpenWrt reconciliation roadmap](../design/openwrt-roadmap.md) records the MVP
dependency subset and how the concurrent extraction PRs fit this layout.

Rules while the split is in progress:

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
- The architecture-ledger static gate covers `backend/tauri/src/` and
  `backend/nyanpasu-core/src/` (`STATIC_GATE_PREFIXES` in
  `scripts/src/architecture-ledger/policy.ts`), matching the core extraction PR.
  Both use the same explicit path-and-name allowlist; other backend library
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
- A panic means the code reached a state it must not reach, so it interrupts execution. Do not write `catch_unwind` in production code, and do not turn a panicking task's `JoinError` into an ordinary error.
- Run UI-thread work through the injected `MainThreadExecutor`; clients and actors do not call Tauri's main-thread APIs themselves.
- Notifications flow downstream only, one domain's slice from that domain's serial owner. An owner never reads or forwards a sibling domain's snapshot, so dependencies form a tree, not a graph.
- Preserve the source and intent of actor messages, event variants, and notification slices. If two sources share downstream handling, keep their distinct variants and handle them together; do not rename or reuse an existing source's variant to represent a new trigger.

If a mature ractor actor client already exists for a capability, use it instead of adding a new global singleton, raw channel loop, or direct Tauri-coupled service call.

## Dependency and boundary rules

### Backend package direction

Shared backend code follows [Backend packages](backend-packages.md):
`nyanpasu-core` owns shared use cases, typed clients, workflows and consumed
ports. The application/platform crates have been removed; runtime configuration
building, builtins and concrete FS/script adapters share the capability owner
`nyanpasu_core::runtime::config`. Non-GUI adapters stay behind narrow ports in
capability-local core modules, such as runtime config's script/content adapters
and `device::os`. Keep infrastructure IO out of pure services and retain explicit
construction. Core and config must not depend on Tauri or egui/eframe; config must
not depend on core. Tauri is a host composition root and GUI/transport adapter;
Tauri-specific implementations stay at that boundary. Move complete existing call
paths and update callers without retaining old-path wrappers solely to avoid
import changes. Run `deno task lint:backend-boundaries` and focused
`deno task test:backend-boundaries` after backend package-boundary changes.

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

Allowed statics are listed in `scripts/src/architecture-ledger/policy.ts`, and `deno task lint:architecture-ledger` rejects new ones.

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
- Every compatibility layer must be explicitly marked with `TODO(actor-migration)` or `FIXME(actor-migration)` and must explain the reason and removal condition. The marker keeps its historical name because `deno task lint:architecture-ledger` counts it.
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
