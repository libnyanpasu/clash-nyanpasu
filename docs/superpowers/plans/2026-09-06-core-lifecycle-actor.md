# Core lifecycle actor migration

## Objective and boundaries

Replace the application facade's independent host-transition and runtime-rebuild
gates with one explicitly injected ractor-owned lifecycle coordinator. Preserve
the endpoint router, service actor, host death proof, committed/degraded results,
and the distinction between promoted configuration and applied configuration.
No new global service, utility semaphore, generic closure command, or actor RPC
cycle. Work in an isolated worktree on `refactor/core-lifecycle-actor`.

## Design

- `NyanpasuClient` delegates complete core lifecycle requests to `CoreLifecycleClient`.
  This coordinates core workflows; application startup and shutdown orchestration
  remain outside this actor. Its status API is `core_lifecycle_status()`.
- `CoreLifecycleActor` admits at most one mutating operation at a time, has a
  bounded pending queue, coalesces dirty rebuild requests, and owns shutdown.
- A supervised operation task reports completion to the actor. Caller timeout
  or cancellation must not release the active operation. Failure must not admit
  a competing mutation while an earlier side effect is unresolved.
- Actor startup receives typed configuration clients, core/service clients,
  runtime build/storage adapters, and event sinks. It never receives the facade.
- Runtime revisions and promoted snapshots belong to the coordinator; readers
  receive immutable projections. Configuration remains owned by its existing
  actors; independent source reads are not claimed to be atomic snapshots.
- Runtime construction uses explicit inputs and injected infrastructure. Binary
  installation uses a prepared artifact owning its temporary files and an
  installer port, replacing the caller-supplied replacement closure.
- Host switching includes config commit, handoff, reconcile, and daemon stop.
  Binary replacement includes authoritative status, stop, unconditional recovery,
  installation, and optional reconcile. All mutation entry points participate.
- Shutdown closes mutation admission, settles pending callers, waits for safe
  completion of the current operation, and stops the core. Read projections
  remain available while a long operation runs.

## Implementation and verification

- [x] Inventory callers, regression tests, and bootstrap dependencies; initialize
      worktree/submodules, symlink only sidecar/resources, create Rust frontend stub.
- [x] Extract lifecycle workflows and infrastructure ports; preserve existing
      ordering and error semantics with focused regression tests.
- [x] Implement coordinator and typed client, bounded admission, completion
      identity, dirty coalescing, timeout/status reporting, and shutdown.
- [x] Wire bootstrap and migrate facade, updater, dirty notifications and runtime
      projections. Remove both gates, obsolete worker and closure replacement API.
- [x] Deterministically test handoff/uninstall and replacement/reconcile
      exclusion, dirty changes during execution, canceled callers, failure paths,
      shutdown, and temporary artifact lifetime. Use acknowledgments, not sleeps.
- [x] Run formatting, focused tests, and applicable Cargo checks. Review diff
      for hidden bypasses, globals, actor state shared through locks, and unrelated
      edits; record exact results and any environmental limitations here.

## Execution notes

- Base: `a51ca578f`; worktree:
  `G:/Programs/Rust/.ccg/clash-nyanpasu/core-lifecycle`.
- Initialized both runtime submodules at their pinned commits. Only sidecar and
  resources point to the main checkout. Cargo target and the Rust-only frontend
  placeholder are local to this worktree; no frontend dependencies were needed.
- Implementation lives in `client/core_lifecycle/{mod,workflow,ports,adapters}.rs`.
  Ports define dependencies; concrete filesystem implementations live in adapters.
  The active request, task handle, and shutdown flag form one `ActiveOperation`.
  The facade holds only a typed core lifecycle client plus its other existing
  domain dependencies. Core status
  subscriptions use a read-only `CoreObserver`, without mutation capability.
- The coordinator owns a 32-request pending queue and retains 32 recent results.
  A 180-second caller deadline does not cancel admitted or queued operations;
  errors identify the application operation and history retains backend IDs.
- Known terminal core failures and failed read-only preflights do not poison
  admission. Lost mutating replies, nonterminal operation results and workflow
  panics fail admission closed; shutdown remains available. Service mutation
  errors are conservative because their adapter API does not distinguish a
  completed infrastructure failure from a detached side effect still running.
- Dirty notifications use a capacity-one watch signal sampled by an actor-owned
  500 ms timer (up to one window of coalescing, rather than a fresh full delay
  after every burst). A change observed during execution survives for another
  build. Tests inject mailbox ticks instead of racing a wall-clock timer.
- Binary requests own their staging directory through completion. Installation
  and terminal progress notifications run independently of the requester's
  lifetime. The updater preserves a terminal success racing an RPC timeout.
- The exit adapter issues one shutdown request that drains the active operation,
  rejects pending mutations and stops the core. Repeated shutdowns share the
  terminal report. Both old gates and the callback rebuild worker are removed.
- No new utils semaphore, global service, shared actor-state lock or facade
  lookup was introduced. Existing source config actors remain authoritative;
  multi-actor reads are explicitly not claimed to be an atomic snapshot.

## Validation

- Baseline before implementation: `cargo test -p clash-nyanpasu --lib client::
--no-default-features`: 150 passed, one existing failure in
  `client::rebuild::tests::legacy_regen_inputs_conversion_reflects_drafted_fields`
  (`missing field unified-delay`). That schema fixture is outside this migration.
- Final `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu
--lib --all-features client:: -- --skip
client::rebuild::tests::legacy_regen_inputs_conversion_reflects_drafted_fields`:
  **148 passed**, including 11 lifecycle tests. The single known baseline
  failure was explicitly excluded; it was also reproduced after the migration
  before running this focused verification.
- Full-feature `core::actor_v2` regression: 68 passed.
- `cargo fmt --manifest-path backend/Cargo.toml --all -- --check`: passed.
- `cargo clippy --manifest-path backend/Cargo.toml -p clash-nyanpasu
--all-features --lib`: passed; existing unrelated warnings remain. No warnings
  point to the new lifecycle implementation.
- `git diff --check`: passed. Reviewed production mutation call sites: the
  application facade and updater use the coordinator; no old gate, callback
  replacement API, shared runtime lock store or rebuild worker remains.
- Validation is on Windows using fake endpoints/installers and a Rust-only
  frontend placeholder. No real core/service handoff, UAC installation or UI
  smoke test was performed.
- Validation logs are retained under the worktree's gitignored
  `backend/tauri/tmp/lifecycle-checks/` directory.

## Core naming and state simplification verification

- On macOS, `cargo test -p clash-nyanpasu --lib client::core_lifecycle::tests
--locked --offline`: 11 passed. The related `client::rebuild::tests::s09_`
  filter: 3 passed, covering shared coordinator ownership, independent graphs,
  and legacy entry-point delegation.
- Both runs used `TAURI_CONFIG='{"bundle":{"externalBin":[]}}'` because the
  local sidecar downloads lack `meow-aarch64-apple-darwin`. This verifies Rust
  behavior with fake endpoints/installers, not sidecar packaging. Compilation
  required network access for embedded JS modules, and rebuild tests required
  local port probing outside the sandbox.
- Formatting checks on changed Rust files and `git diff --check` passed.
  No old module path, facade field, or lifecycle status API references remain.
