# Application and platform extraction

> Historical record: sequencing and package boundaries are superseded by the [approved core extraction plan](tauri-core-extraction-plan.md). Validation below describes its original source checkpoint, not the current restructuring.

Status: original implementation plan, 2026-10-06, with the current ownership
correction below. This is the prerequisite to the [OpenWrt MVP](openwrt-mvp.md).
The extraction takes priority over building a router-specific facade. Each
completed stage is a separate, buildable commit.

Current target: shared application behavior belongs in `nyanpasu-core`.
`nyanpasu-application` and `nyanpasu-platform` have now been removed after migrating
their current implementation, original tests and all consumers into core. The
runtime consolidation below is a separate task from the earlier device-input
extraction. The original diagram, stages and first-slice delivery later in this
document are historical, not instructions to recreate those packages. Follow
[Backend packages](../development/backend-packages.md) for current rules.

## Runtime configuration consolidation into core

The user reviewed the implementation and authorized this atomic commit/push.
The single shared owner is `nyanpasu_core::runtime::config`, under the runtime
capability rather than a generic `enhance` module. It contains RuntimeBuilder,
inputs/errors/logs, builtin sources and rules, script descriptors, the explicit
filesystem content source and the Boa/Lua/script adapters. The script adapter is
named `RuntimeConfigScriptRunner`; there are no old module/type forwarding paths.
Original integration coverage lives in
[`nyanpasu-core/tests/runtime_builder.rs`](../../backend/nyanpasu-core/tests/runtime_builder.rs).

This migrates the newer main implementation, not a rollback to the old core
runtime implementation. Snapshot graph, single serialization, build/preparation
error separation, frozen-content policy, JS/Lua ordering, failed-script logs,
private blocking runtime and shutdown behavior are unchanged. Config still owns
the pure executor and its ports. Tauri retains artifact/log projection, runtime
preparation, source/workflow owners, facade and frontend composition. This does
not complete independent headless bootstrap or advance host-input tasks 4–7.

The workspace, GUI consumers, exclusive dependencies, lockfile, dependency gate
and musl harness now use core. The gate checks core/config for transitive GUI
leaks and rejects config-to-core dependencies. The musl harness builds the two
core test binaries but selects only `runtime::config::` unit tests plus the
runtime-builder integration test; its original runtime/script scope is not
expanded to unrelated core tests.

### Verification and limitations

Before the move, application had seven passing tests and platform had 49 unit
plus one integration test passing. The existing core device-model assertion
failed in this environment before editing. An unfiltered core run after the
initial move, before the namespace rename, had 195 passes and the same one
failure; an explicit skip rerun had 195 unit passes plus the integration test.
The assertion and device implementation were not changed. After the user limited
verification to affected areas, only focused tests were run; the interrupted
complete GUI/script test runs are not counted as successful verification.

Final focused results after the runtime/config naming change:

| Scope                                                                  | Result                                                     |
| ---------------------------------------------------------------------- | ---------------------------------------------------------- |
| Core `runtime::config::` original unit tests                           | 56 passed; 140 unrelated tests filtered out                |
| Original runtime-builder integration                                   | 1 passed                                                   |
| GUI enhance/artifact/golden                                            | 6 passed, including five goldens                           |
| GUI application workflow                                               | 224 passed                                                 |
| Affected facade construction/runtime paths                             | 9 original tests passed                                    |
| Specta export                                                          | 1 passed; both generated bindings byte-identical           |
| Backend dependency policy                                              | 3 original tests passed                                    |
| Affected-package all-targets/all-features Clippy, Rustfmt, Deno checks | Passed                                                     |
| Architecture ledger and backend dependency gates                       | Passed; 408 Rust files, 39 static allowances, no residuals |

Reproducible final checks from the repository root:

```sh
cargo test --manifest-path backend/Cargo.toml -p nyanpasu-core --lib runtime::config::
cargo test --manifest-path backend/Cargo.toml -p nyanpasu-core --test runtime_builder
GSETTINGS_BACKEND=memory cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib enhance:: -- --test-threads=1
GSETTINGS_BACKEND=memory cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib client::application_workflow:: -- --test-threads=1
GSETTINGS_BACKEND=memory cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib specta_export::tests::export_typescript_bindings
cargo clippy --manifest-path backend/Cargo.toml -p nyanpasu-core -p clash-nyanpasu --all-targets --all-features
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
deno task test:backend-boundaries
deno task lint:deno
deno task lint:architecture-ledger
deno task lint:backend-boundaries
```

The nine facade checks use exact original test names:

```sh
for name in \
  client_constructs_with_mandatory_typed_config_clients \
  typed_setup_loads_persisted_state \
  try_new_with_args_constructs_typed_config_facade \
  runtime_lifecycle_is_empty_before_first_rebuild \
  reconcile_publishes_the_runtime_product_read_model \
  runtime_inspection_tracks_promoted_builds \
  repeated_reconcile_advances_the_runtime_revision \
  repeated_core_updates_advance_the_runtime_revision \
  managed_edit_uses_candidate_bytes_and_rejects_invalid_runtime_without_overwriting_source
do
  GSETTINGS_BACKEND=memory cargo test --manifest-path backend/Cargo.toml \
    -p clash-nyanpasu --lib "client::tests::$name" -- --exact --test-threads=1
done
```

The all-target, all-feature normal/build core dependency tree contains no Tauri,
egui/eframe or removed application/platform package. Semantic comparison of all
15 migrated source/fixture/test files against the parent implementation found
only import/module wiring and the approved runner rename; builtin source bytes
and original assertions are intact. The lockfile has no version/checksum drift,
only package removals and core/GUI dependency wiring changes.

Docker/rootfs verification is blocked by permission to `/var/run/docker.sock`;
`docker info` returned 1, so the musl harness was not executed in this round.
Windows/macOS compilation/runtime, hardware router execution, independent
headless composition and remote CI/merge are not established by these checks.
The earlier first-slice Docker result below remains historical, not a result for
this consolidated core. The runtime submodule and retained user stashes are
unchanged. Concurrent audit/roadmap edits are outside this commit.

## Objective

Tauri is a host environment and GUI/IPC adapter, not the owner of reusable
application behavior. Desktop and OpenWrt must use the same profile, runtime,
script and core application services. Building a second router application that
copies the desktop workflow would leave the architectural problem unresolved.

The original dependency direction was:

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

## Original boundaries and known coupling

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

### Original first-slice delivery (before consolidation)

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
The aarch64 run passed all 57 shared tests in the ImmortalWrt 24.10.4 rootfs.
The harness checks the actual rootfs musl loader because its ARM image tag
currently publishes amd64 Docker metadata. Builder architecture stays explicit.

The next actor slice is the existing CoreActor/CoreClient and its API lease,
with local/service endpoints implemented in platform, plus persistent session
state with host-independent actor construction. Profile transactions, application
workflow, facade composition and RPC registration still belong to the desktop
host until their whole dependency paths can be moved.

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
