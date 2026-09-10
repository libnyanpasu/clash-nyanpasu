# Connection interruption policy migration

## Scope

Move active mode-change interruption into CoreLifecycleActor. Frontend, tray and
hotkeys obey the same typed on_mode_change setting; remove their independent
close calls and retire the legacy ConnectionInterruptionService.
Profile-change and chain-specific legacy methods have no callers. Removing them
does not enable new profile-change triggers or implement chain filtering.
Proxy selection retains its already migrated actor policy.

## Ordering and identity

For a mode-bearing config request with interruption enabled, capture the source
core's revocable ApiClient before committing the config. A confirmed stopped core
needs no interruption; unavailable/unknown source identity is reported as degraded
if config application succeeds. Never acquire a replacement capability to retry
closure. Reconcile failure prevents closure. After successful reconcile, close
only through the source capability. Confirmed Started/Restarted/Switched outcomes
need no closure. Otherwise a retired capability or failed request reports a
committed-degraded result with no automatic retry or config rollback.

The lifecycle actor holds admission through closure, so subsequent config/host
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
  enter CoreLifecycleActor. Deleting the current profile remains rejected by
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
