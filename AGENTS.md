# AGENTS.md

This repository is migrating away from `::global()` singletons and Tauri-coupled services toward explicit dependency injection, actor-owned state, and pure domain services.

## Mandatory Reading: Development Standards

Before starting any repository work, agents MUST read [docs/development/README.md](docs/development/README.md) and all standards guides: [architecture](docs/development/architecture.md), [unified RPC](docs/development/rpc.md), [testing and review](docs/development/testing.md), [workflow](docs/development/workflow.md), [Rust code style](docs/development/rust.md), [TypeScript and React code style](docs/development/typescript.md), and [repository scripts](docs/development/scripts.md).

Agents MUST strictly follow these development standards together with the instructions below. Reading this file alone is insufficient. The guides are mandatory project requirements, not optional background or suggestions. If a guide is unavailable or the current requirements conflict, report the issue and resolve it before proceeding with affected work.

Behavioral guidelines reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

## 0. Synchronization Policy

Keep `CLAUDE.md` and `AGENTS.md` synchronized as much as possible.

Update the corresponding `docs/development/` guide in the same change whenever a shared development rule changes. These guides and `AGENTS.md` must describe the same requirements; historical plans do not override current rules. `CLAUDE.md` currently imports `AGENTS.md`.

- When changing an architectural rule in one file, mirror it in the other file.
- Differences should be limited to tool-specific wording, if any.
- Prefer the same section order, same terminology, and same examples.
- Do not create a Claude-only or agent-only exception unless the tool truly requires it.

## 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:

- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

For small, obvious tasks, do this briefly. For architecture, migration, or cross-module work, be explicit.

## 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

This rule does not override the architectural migration direction. Do not use `::global()` or hidden mutable process state merely because it is fewer lines.

## 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:

- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:

- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

For actor/DI migration work, the allowed scope is the smallest call path needed to migrate the touched service or API without leaving a hidden compatibility layer behind.

## 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:

- "Add validation" -> "Write tests for invalid inputs, then make them pass"
- "Fix the bug" -> "Write a test that reproduces it, then make it pass"
- "Refactor X" -> "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:

```text
1. [Step] -> verify: [check]
2. [Step] -> verify: [check]
3. [Step] -> verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

## 5. Target Architecture

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

## 6. ractor Primer for Agents

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

If a mature ractor actor client already exists for a capability, use it instead of adding a new global singleton, raw channel loop, or direct Tauri-coupled service call.

## 7. Mandatory Architecture Rules

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

If an existing `::global()` service must still be used during migration, isolate it at the edge of a migration step and add an explicit comment:

```rust
// TODO(actor-migration): temporary bridge to the legacy global service.
// Reason: <why full migration is blocked>.
// Remove when: <service-name> is injected through NyanpasuClient.
```

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

## 8. Service Classification

Before adding or migrating a service, classify it as an actor service, pure service, or adapter/port.

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

## 9. When You Touch Legacy Global Code

If you see patterns such as:

```rust
Config::global()
Config::verge()
Config::clash()
Config::profiles()
Config::runtime()
CoreManager::global()
Sysopt::global()
Hotkey::global()
Logger::global()
Handle::global()
ProxiesGuard::global()
UpdaterManager::global()
WindowManager::global()
consts::app_handle()
```

prefer replacing the call path with one of:

```rust
client.some_domain_operation(...).await?;
state_client.some_state_operation(...).await?;
core_client.some_core_operation(...).await?;
system_proxy_client.some_system_operation(...).await?;
service.method(...)?;
```

Do not add a new wrapper that simply hides the global unless full migration is blocked. If blocked, document it:

```rust
// TODO(actor-migration): temporary bridge to <legacy global>.
// Reason: <specific blocker>.
// Remove when: <specific migration step>.
```

## 10. State and Configuration Migration

Configuration must be migrated before dependent services whenever possible.

Preferred direction:

1. Move state ownership into `StateActor` or a state manager owned by `StateActor`.
2. Keep schema and patch operations in pure services or domain types.
3. Generate runtime config from snapshots rather than mutating runtime globals.
4. Commit state first, then trigger side effects through actor messages. A Required participant (the runtime) is a vote before the commit, not a side effect; this rule covers ordinary side effects.
5. Report post-commit side-effect failures as degraded results instead of silently rolling back persisted state.

Avoid preserving old global configuration APIs. Prefer a migratable breaking change that updates callers to the new injected client/service API.

## 11. Migration Policy: Prefer Migratable Breaking Changes

When refactoring or migrating services:

- Prefer fully migrating callers to the new injected/actor/pure-service API.
- Prefer migratable breaking changes over compatibility layers.
- Do not add a compatibility layer simply to avoid updating call sites.
- Add a compatibility or migration layer only when a full migration is not currently possible due to cyclic dependencies, public API constraints, external plugin behavior, large cross-cutting risk, platform limitation, or staged release requirements.
- Every compatibility layer must be explicitly marked with `TODO(actor-migration)` or `FIXME(actor-migration)` and must explain the reason and removal condition.
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

## 12. Unified RPC and Command Rules

Application commands use the unified RPC framework, with Tauri IPC and HTTP as transport adapters. Commands should be thin adapters. They should:

- parse request DTOs;
- call `NyanpasuClient`;
- map domain errors into command errors;
- never perform business orchestration directly;
- never read or mutate config through globals;
- never spawn core/service background tasks directly.

Allowed shape:

```rust
#[nyanpasu_macro::rpc(http)]
pub async fn patch_verge_config(
    client: tauri::State<'_, NyanpasuClient>,
    patch: NyanpasuAppConfigPatch,
) -> Result<()> {
    client.patch_app_config(patch).await?;
    Ok(())
}
```

Avoid shape:

```rust
#[tauri::command]
pub async fn patch_verge_config(patch: IVerge) -> Result<()> {
    Config::verge().draft().patch_config(patch)?;
    CoreManager::global().update_config().await?;
    Config::verge().apply();
    Ok(())
}
```

### Use the unified RPC surface

- Declare new application commands with `#[nyanpasu_macro::rpc(...)]`; the macro generates transport handlers and registers them with `UnifiedRpc`. Do not add standalone `#[tauri::command]` application APIs, direct `generate_handler!` registration, or a separate HTTP implementation of the same operation.
- Keep command metadata in `backend/tauri/src/specta_export.rs`. Register read-only operations as queries and side-effecting operations as mutations, even if their names start with `get` or `query`. Regenerate TypeScript bindings through the existing export workflow; do not hand-edit generated bindings.
- Frontend application calls use `@nyanpasu/rpc` generated bindings and transport adapters, then `@nyanpasu/query` for query/mutation hooks. RPC has no React Query dependency; query receives its RPC client/context explicitly. Transport selection belongs in the RPC package. Do not call raw Tauri `invoke`, import legacy transport bindings for application commands, or add ad hoc HTTP fetches in pages/hooks.
- Native Tauri plugin APIs and frame delivery remain boundary-specific adapters. Channel subscription control is an application operation: declare it with `#[nyanpasu_macro::rpc]`, register it as a mutation, and call it through `rpc`. The desktop dispatcher binds Channel descriptors to the invoking webview; a Channel does not opt an operation into HTTP. Browser-accessible UI must guard native capabilities and provide an appropriate browser path or explicit unsupported state.

### Declare capabilities and preserve transport semantics

- HTTP access is explicit opt-in via `rpc(http)`, not inferred authorization from a compatible signature. Review the operation's effects before enabling it. Desktop OS operations, arbitrary outbound URL diagnostics, and desktop-only server controls stay desktop-only unless deliberately adapted and reviewed.
- Never inject `AppHandle` into an HTTP-enabled RPC command or add it to `RpcDependencies`; doing so grants the handler every capability reachable from the app. Route shared side effects through `NyanpasuClient` and the existing typed client or actor message that owns them. Do not add a one-off managed Tauri closure when an existing actor path already performs the operation. `EventBus` and `rpc.events` carry notifications from backend to frontend; they are not an inbound command channel.
- Shared operations receive explicit dependencies and call `NyanpasuClient`. Assemble `RpcDependencies`, routers, and server adapters in the composition root. The facade must not accept `axum::Router` or expose transport infrastructure; avoid strong reference cycles between the server/router and client.
- Use `rpc(owner)` and the injected `RpcOwner` for caller-owned resources such as log sessions. Enforce ownership on every resource operation; do not use Tauri window identity as the shared domain identity or trust a client-supplied owner.
- Use `rpc(result)` when a Result alias needs explicit fallible-return handling. Preserve structured `RpcError` metadata and domain errors across both transports; never serialize an error as a successful payload or flatten it into an unstructured string.
- Keep HTTP RPC, SSE, and the development proxy behind the existing per-start access credential and Host/Origin checks. A session cookie identifies an owner; it does not by itself authenticate access. Keep the server disabled by default and bound to loopback.
- Shared frontend event subscriptions use `event-transport.ts` through `rpc.events`. Preserve shared connection disposal and state resynchronization on connection, reconnection, and event-buffer overflow. New shared events must be registered in the transport metadata and event bridge; do not create desktop-only listeners for shared state.
- The unified transport does not change actor ownership rules: in-process RPC waits for the real result without a timeout, and dropping a caller does not cancel owner-started work. Network/IPC and connection-draining deadlines stay at their respective boundaries. A transport timeout does not prove cancellation or safe retry.
- Verify changed shared commands on both transports, including error mapping, query/mutation classification, HTTP capability restrictions, and owner isolation where applicable.

## 13. Testing and Mocking

- Prefer testing pure services directly with plain values.
- For infrastructure dependencies, define narrow traits and inject them.
- Traits that are intended to be mocked should be compatible with `mockall` / `automock` where practical.
- Keep mock-only APIs behind `#[cfg(test)]` or test-support modules.
- Do not use global test fixtures for application services. Construct a test `NyanpasuClient` or test-specific service graph.
- Actor tests should spawn the actor with fake adapters and send typed messages through its typed client.
- Avoid sleeping in actor tests. Prefer explicit acknowledgements, request/reply messages, or test hooks.

Example mockable trait:

```rust
#[cfg_attr(test, mockall::automock)]
pub trait UiEventSink: Send + Sync + 'static {
    fn emit_state_changed(&self, event: StateChanged) -> anyhow::Result<()>;
}
```

Another acceptable trait shape:

```rust
#[cfg_attr(test, mockall::automock)]
pub trait ConfigStore: Send + Sync + 'static {
    fn load(&self) -> anyhow::Result<Vec<u8>>;
    fn save(&self, bytes: &[u8]) -> anyhow::Result<()>;
}
```

## 14. Naming Guidelines

### Language code style

- Rust follows the existing nightly Rustfmt configuration and Clippy checks; see [Rust code style](docs/development/rust.md). Do not introduce a stricter Rust lint profile as incidental cleanup.
- TS/TSX follows Prettier, Oxlint, and package TypeScript checks; see [TypeScript and React code style](docs/development/typescript.md).
- Frontend code is split into the nine private source packages documented in [Frontend package boundaries](docs/development/frontend-packages.md). Shared packages expose their public API through `src/index.ts`, retain source subpaths where useful, and typecheck with `noEmit`; do not add an interface `dist` build prerequisite. The root TypeScript project is a reference index, while `pnpm typecheck` checks each package and then Node configuration/perf/tests. Each package includes only its own `src`; tests live in `frontend/<package>/tests/`.
- Keep package direction explicit: `rpc` has no React Query dependency; `query` depends on RPC and receives its client/context explicitly; `ui` has no app, platform, RPC, or query imports. The app alone owns routes and Paraglide. Tailwind scans shared packages that produce UI classes; the data-slot generator scans `frontend/*/src/**/*.tsx`. Platform-dependent behavior is injected through lazy adapters, and providers receive callbacks such as `onDegraded` as parameters rather than using globals.
- Use the existing feature/component namespace for i18n message keys. Dashboard widget text, including configuration controls, uses `dashboard_widget_*`, not a parallel `dashboard_config_*` prefix. Place new keys beside related messages in every locale file, preserving logical groups and order across locales; do not append unrelated keys at the end. Update locale sources and regenerate Paraglide output through its existing workflow; do not hand-edit generated message modules.
- Include the owning component's name for messages used only by that component: `core_service`-exclusive messages carry a `core_service` segment under their feature namespace. Dashboard options exclusive to `ProxyShortcutsWidget` or `CoreShortcutsWidget` use `dashboard_widget_proxy_shortcuts_*` or `dashboard_widget_core_shortcuts_*`; shared messages belong to the smallest common scope, such as `dashboard_widget_config_*` for shared configuration UI.
- Prefer descriptive, stable `data-slot` names on React component DOM roots and meaningful parts for readability and custom CSS. Preserve existing slots and forward data attributes; do not add DOM wrappers solely for a slot.
- Before adding constants or environment predicates, search for existing definitions. Reuse equivalent ones or promote genuinely shared local definitions to the smallest common scope; keep one-use details local and distinguish Tauri execution, OS identity, and viewport size.
- Separate different responsibilities with one blank line, especially query-client access, queries, mutations, derived values, handlers, and the final return. Keep related statements together.
- Prefer `@nyanpasu/ui` components. For missing controls, check Radix primitives and add styled, accessible wrappers in `frontend/ui/` before using them in features. Follow Material You and existing project tokens and interaction states. Oxlint restricts direct Radix imports to that UI layer.
- Semantic naming, reuse, grouping, and visual consistency remain mandatory review requirements even when formatting and lint pass.

### Repository scripts

- Repository automation uses Deno TypeScript, with source grouped by responsibility under `scripts/src/` and co-located `*_test.ts` files. Keep configuration, lockfile, documentation and editor settings at `scripts/`; put non-source fixtures in `scripts/fixtures/`. The upstream runtime submodule owns its own tooling.
- The root `deno.jsonc` is the only task catalog; `scripts/deno.jsonc` and `scripts/deno.lock` own runtime configuration and locked dependencies separately from pnpm. Every public CLI has a named task and description. Reuse the package scripts' colon-separated operation names.
- External callers (CI, package scripts, hooks, current docs and application tests) use `deno task <name>`. Existing pnpm commands may delegate to tasks. Do not add direct `deno run`, `node`, `tsx` or script-file calls outside the task catalog. External tool invocations inside scripts and fixture-only subprocesses in script unit tests are implementation details; historical reports retain their original commands.
- Split CLI orchestration, pure computation and infrastructure by concrete responsibility. Imported library modules must not run a CLI, download files or launch processes. Avoid broad `utils/` buckets, generic frameworks and old-path compatibility wrappers or exports.
- Tasks run at the repository root, with arguments forwarded without an extra `--`. Use `shared/repo-paths.ts` for repository paths and explicit parameters for alternate workspaces. Preserve parameters, environment variables, permissions and exit status; never store credentials in tasks.
- Use Deno-compatible `jsr:`, `npm:` or supported `node:` imports, pin new npm imports/types and update the Deno lockfile. Playwright scripts run through Deno but still require Chromium; the external Tauri signing CLI may use Node.
- Run `deno task lint:deno`, `deno task test:scripts` and the architecture gate as relevant; compare behavior before/after reorganizing, check arguments and paths, and verify migrated generator/browser behavior. Do not publish, upload or send notifications merely to verify a refactor. Document checks that require unavailable platforms or environments.
- Follow [Repository scripts](docs/development/scripts.md) for the category layout and verification commands; update that guide alongside shared rule changes.

### GitHub workflow names

- Use `[Category] Action Object` for workflow display names, with the categories `CI`, `Release`, `Maintenance`, and `Reusable`.
- Use `Reusable` for workflows exposed through `workflow_call`, even when they also support manual dispatch. Other categories describe the entry workflow's purpose.
- Name the actual operation and output: distinguish nightly publication, release package publication, draft release preparation, core version manifests, and app updater manifests. Avoid scope labels such as `Entire` and `Single`.
- Preserve workflow file paths and CI job names during display-name cleanup; review callers, badges, documentation, and required checks before renaming those identifiers.
- Remove workflows only after checking reusable callers and automatic/manual entry points; lack of recent runs alone does not prove a workflow is unused.
- Separate adjacent workflow steps with one blank line, keeping each step's explanatory comments after the separator.

### Role names

Use names that reveal the role:

- `StateActor`, `CoreActor`, `SystemProxyActor`, `HotkeyActor`, `ProxiesActor`, `UpdaterActor` for actors.
- `StateClient`, `CoreClient`, `SystemProxyClient`, `HotkeyClient`, `ProxiesClient` for typed actor clients.
- `RuntimeBuilder`, `ProfileMerger`, `ConfigMigrator`, `PatchValidator` for pure services.
- `TauriUiEventSink`, `FsConfigStore`, `OsProxyBackend`, `ProcessRunner` for adapters.
- `AppSupervisor` or `NyanpasuBootstrap` for the composition root.

## 15. Comment Requirements

Use comments only where they clarify migration state, invariants, or actor lifecycle assumptions.

Required compatibility-layer comment:

```rust
// TODO(actor-migration): compatibility bridge for <legacy API>.
// Reason: <why full migration is blocked>.
// Remove when: <specific condition or tracking issue>.
```

Required temporary legacy behavior comment:

```rust
// FIXME(actor-migration): legacy behavior kept temporarily for <reason>.
// New code must use <new API>. Remove after <condition>.
```

## 16. Final Review Checklist

Before finishing a change, check:

- assumptions were stated when relevant;
- success criteria were verified;
- every changed line traces to the request;
- no new global singleton service was added;
- no new mutable static service state was added;
- dependencies are explicit;
- service classification is clear;
- actor state is not leaked through shared locks;
- Tauri is isolated behind adapters;
- compatibility layers are exceptional and documented;
- tests use injection, fakes, mocks, or pure values;
- `NyanpasuClient` remains a facade, not a service locator.

## 17. Worktree Setup and Resource Reuse

Feature/migration work runs in isolated git worktrees by default. Working in the current checkout is an option when the user chooses it after a cost assessment. Before implementation:

- Consider the task's scope, expected duration, concurrent work, and existing uncommitted changes in the current checkout.
- Weigh the isolation benefits against dependency installation, independent Cargo builds, disk usage, and preparation of gitignored build prerequisites. Reuse a suitable existing worktree when available.
- Summarize the relevant costs and benefits, state a recommendation, then ask the user whether to work in a worktree or the current checkout. Wait for their choice before implementation; if they have already specified a choice for the task, follow it without asking again.
- Keep isolated worktrees as the recommended default for feature/migration work. Small, isolated edits may be cheaper in the current checkout; broad changes or concurrent work strengthen the case for a worktree. The assessment adds a user-selectable alternative, not a replacement for the isolation and resource reuse policy.

When the user chooses a worktree, its location is the developer's choice (any path outside the repo tree). Worktrees share the main `.git`. The rule: reuse expensive **branch-independent** assets from the main checkout via symlink, and regenerate everything **branch-dependent** per worktree.

### Reuse policy

| Path (repo-relative)       | Approx size  | Policy                          | Reason                                                                                                                                         |
| -------------------------- | ------------ | ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `backend/tauri/sidecar/`   | ~213M        | **Symlink → main**              | gitignored downloaded cores (mihomo / clash-rs / clash / nyanpasu-service); branch-independent; re-fetch via `deno task prepare:check` is slow |
| `backend/tauri/resources/` | ~21M         | **Symlink → main**              | gitignored static assets (`geoip.dat`, `geosite.dat`, `Country.mmdb`, `wintun.dll`, service exes); branch-independent                          |
| `node_modules/`            | ~1.5G        | **Independent `pnpm install`**  | pnpm global store already hardlink-dedupes; sharing risks concurrent lock conflicts                                                            |
| `backend/target/`          | ~50G         | **Independent — never symlink** | sharing causes Cargo incremental-fingerprint churn + concurrent build-lock waits across diverged source trees                                  |
| `backend/tauri/tmp/dist/`  | build output | **Independent — never symlink** | branch-dependent frontend build; `emptyOutDir: true` means one worktree's `web:build` wipes the shared dir                                     |

Only `sidecar/` and `resources/` are symlink candidates.

### Gitignored build prerequisites a fresh worktree lacks

- **`backend/tauri/tmp/dist`** — `backend/tauri/build.rs` calls `tauri_build::build()`, which validates `frontendDist: ./tmp/dist` **at compile time**. When missing, every `cargo build` / `clippy` / `cargo test --all-features` / rust-analyzer run on the tauri crate fails. Resolve one of:
  - Rust-only worktree → drop a placeholder (cheapest, no vite build).
  - Runnable UI → `pnpm web:build` (workspace packages resolve from source; this clears and refills `tmp/dist`).

`backend/tauri/tmp/git-info.json` is optional (`build.rs` guards it with `exists()`); run `deno task generate:git-info` only if accurate commit metadata must be baked in.

### Create a worktree

Commands shown for Windows / PowerShell (dir symlinks need Developer Mode, no elevation). `<worktree-path>` and `<type>/<name>` are yours to choose.

```powershell
$main = git rev-parse --show-toplevel                 # capture main checkout root
git worktree add <worktree-path> -b <type>/<name>
cd <worktree-path>

# Reuse branch-independent downloads (symlink back to main)
New-Item -ItemType SymbolicLink backend/tauri/sidecar   -Target "$main/backend/tauri/sidecar"
New-Item -ItemType SymbolicLink backend/tauri/resources -Target "$main/backend/tauri/resources"

pnpm install

# Satisfy tauri-build's frontendDist check — pick one:
New-Item -ItemType Directory -Force backend/tauri/tmp/dist | Out-Null            # A) Rust-only placeholder
Set-Content backend/tauri/tmp/dist/index.html '<!doctype html><title>dev</title>'
# pnpm web:build                                      # B) real UI (builds app and replaces tmp/dist)
```

### Remove a worktree

`git worktree remove` on Windows can fail with `Filename too long` because per-worktree `node_modules` / `target` hold paths over MAX_PATH. Force-delete with the extended-length prefix, then reconcile git:

```powershell
Remove-Item -LiteralPath "\\?\<absolute-worktree-path>" -Recurse -Force
git worktree prune
git worktree list
```

Removal reclaims only the worktree's own files and its symlinks (pointers back to main) — it never touches the main checkout's real `sidecar/` / `resources/`.

## 18. Git Commit Rules

### Stage only related files

Before committing, run `git status` to review the changes, stage only the files related to this change with explicit paths (`git add <specific-path>`), then verify with `git diff --cached --stat`.

Never use blanket staging such as `git add .`, `-A`, `--all`, or `*`. If something was staged by mistake, unstage it with `git reset HEAD <path>`.

### One commit does one thing

Every commit must be atomic, complete, and buildable.

- One indivisible task is one commit.
- Multiple independent tasks are split into multiple commits.
- Do not commit code you know is broken.
- Do not make fix-up (patch-style) commits on a development branch.

If a commit on a development branch is flawed and has not been pushed, fix it with `git reset --soft HEAD~1` and recommit. If it has already been pushed, any rewrite, amend, or force push requires explicit consent first.

Self-check before committing: does this change complete or correct the previous commit? If yes, fold it into the previous commit with `git reset --soft HEAD~1` and recommit instead of creating a new one. Even when two commits are each individually clean, a later commit that completes an earlier one is still a fix-up commit.

Exploratory work may live on `temp/`, `wip/`, or `scratch/` branches. Do not merge those directly; create a clean branch and reorganize the work into atomic commits.

### Commit message content

The subject states what changed; the body explains why when the problem or the fix is not obvious.

Subject rules:

- Use the imperative mood, stay within 72 characters, and do not end with a period.
- Describe the behavior or capability.

Body rules:

- A non-trivial change must have a body; the body may be omitted only when the subject is fully self-explanatory.
- Explain the root cause and the rationale for the fix: why this is a bug and why this change is needed.
- Do not enumerate changes file by file, and do not restate implementation steps that the diff already shows.
- Describe only the final state relative to the parent commit, not differences between intermediate versions of the same patch (e.g. "v2 fixes X").

### Trust the reader

Assume the reader is a competent developer familiar with the project; do not explain what they already know:

- How to build the project — that belongs in documentation, not in a commit message.
- Obvious statements of usage. Counter-example:

  ```text
  Example usage:
    # use mkv container:
    ffmpeg -hwaccel d3d12va -hwaccel_output_format d3d12 -i input.mp4 -c:v av1_d3d12va output.mkv
  ```

- "Build succeeded" or "all tests green" — the commit's existence already implies it passed.

Mention these only when they are genuinely non-obvious:

- New test commands or tools that do not yet exist in the project.
- Non-standard configuration required to reproduce the result.
- Unusual constraints that affect how the data should be interpreted.

---

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, clarifying questions come before implementation rather than after mistakes, and new code moves away from global singletons toward injected actor/pure-service composition.
