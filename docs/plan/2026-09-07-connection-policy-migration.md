# Connection interruption policy migration

## Scope

ApplicationWorkflowActor coordinates mode/profile commits, runtime application,
and connection interruption. Frontend, tray and hotkeys obey the same typed
settings. The actor-owned core lifecycle component consumes RuntimeApplyOptions
and an instance-bound RuntimeApplyContext; it has no profile selection APIs or
profile policy reads. Proxy selection retains its own actor admission and shares
the ConnectionScope execution function.

## Ordering and identity

For a mode-bearing config request with interruption enabled, capture the source
core's revocable ApiClient before committing the config. A confirmed stopped core
needs no interruption; unavailable/unknown source identity is reported as degraded
if config application succeeds. Never acquire a replacement capability to retry
closure. Reconcile failure prevents closure. After successful reconcile, close
only through the source capability. Confirmed Started/Restarted/Switched outcomes
need no closure. Otherwise a retired capability or failed request reports a
committed-degraded result with no automatic retry or config rollback.

The application workflow actor holds admission through closure, so subsequent config/host
work cannot overtake it. Requests without mode and disabled policies do not query
the API. Re-selecting a mode retains the prior mode-bearing request behavior.

## Verification

Test enabled/disabled policy, failed reconcile, failed closure, same-instance hot
patch, replacement with the same URL, unavailable source, stopped startup, and
lifecycle admission while closure is pending. Run application tests, frontend
checks and architecture gate; deliver one application PR.

## Delivered changes and validation

- Removed the last hand-built REST request helper from core/clash/api.rs, its URL
  construction test, and the legacy interruption module. Explicit user connection
  closure remains available through its already migrated IPC/facade capability.
- Configuration-only tests explicitly disable interruption; new policy tests use
  an injected endpoint and HTTP fixture with actual DELETE requests.
- Eight policy tests cover the cases above plus credential rotation during a
  pending close. The application library suite passed (489 passed, one ignored)
  before the final boundary refinement; all 25 lifecycle tests passed afterward.
- Workspace Clippy/all targets/all features, interface build, all frontend
  TypeScript checks, frontend lint and formatting passed. Existing Rust warnings
  remain. The architecture ledger removes three global config reads and two
  temporary migration markers.

No runtime dependency change is required. Real GUI/core smoke tests were not run.
Remaining transport migration is the WebSocket connector. Enabling profile-change
interruption and implementing precise chain-based closure are separate behavior
changes, not replacements for active legacy callers.

## PR-6 follow-up: profile and proxy policies

The PR-6 implementation completes the behavior changes deferred above:

- Explicit activation/deselection and conditional create/import activation now
  enter ApplicationWorkflowActor. Deleting the current profile remains rejected by
  domain validation; deleting other profiles does not introduce interruption. Only a change in the current profile triggers the
  typed `on_profile_change` policy; editing profile contents retains the existing
  rebuild path. Capture the source capability before committing, reconcile, then
  close through that same capability. A failed reconcile skips closure; a failed
  post-commit closure returns `profile_interruption_failed` degradation. Confirmed
  core replacement and stopped sources need no additional closure.
- ProxiesActor is the sole owner of proxy-selection interruption. `ProxyGroup`
  reads connections from the selected source instance and closes only IDs whose
  chains contain the selected group; `All` closes all and `Off` does neither.
  Source revocation fences both the query and each close. Selection success is
  returned as `MutationOutcome<()>`, with separate interruption/cache degradation
  so IPC, tray and frontend do not report a successful selection as a failed
  mutation or automatically repeat it.
- Regression tests cover policy gates, actual profile changes, rejected commits,
  reconcile/close failures, source replacement with the same URL, and lifecycle
  admission while closure is pending. Proxy tests cover group membership and
  post-selection failures without publishing responses from retired instances.

## Runtime apply boundary

The existing admission queue, dirty/recovery scheduling, updater installer port,
and shutdown admission now belong to ApplicationWorkflowActor. The core lifecycle
component is exclusively owned by its tracked operation task; there is no second
upper-level queue or caller-owned permit.

Application workflows resolve interruption scope and capture the source capability
before committing. Profile application builds from CommitReport.snapshot; mode
application uses the committed clash snapshot. Runtime preparation derives local
IPC settings from the same clash input used to build the configuration. Internal
host/installation recovery requests prepare the latest inputs at their original
execution phase through an injected RuntimePreparationPort.

RuntimeApplyContext is consumed by one application and is never replayed by dirty
rebuilds. A failed build, publication or reconciliation preserves the commit and
skips interruption. Existing promoted/applied snapshot and source revocation
semantics remain intact. The implementation and regression matrix are recorded
in [the runtime apply plan](2026-09-13-runtime-apply-options.md).
