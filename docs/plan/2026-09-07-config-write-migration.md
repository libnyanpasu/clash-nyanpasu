# Configuration write migration

## Scope and acceptance

Unify frontend config changes and tray/hotkey mode changes through
NyanpasuClient and CoreLifecycleActor. Retire application PUT/PATCH HTTP helpers;
runtime reconciliation already owns applying typed configuration to the core.

1. Merge field-level override patches inside ClashConfigActor so concurrent
   updates cannot overwrite unrelated fields from stale snapshots.
2. Admit persistence and reconcile as one lifecycle command, serialized with
   host changes, shutdown and other reconciles. Retain persisted state and return
   MutationOutcome::CommittedDegraded if post-commit application or UI effects fail.
3. Keep IPC a DTO adapter; route tray/hotkey mode changes through the same facade.
   Preserve Premium script mode in the typed config enum.
4. Verify patch isolation, serialized application, durable state after failure,
   no core work after persistence failure, IPC exports and frontend type checks.

## Boundaries

No new actor or service singleton is needed. Configuration patch merging remains
inside the existing state actor; lifecycle admission and side effects stay in the
existing lifecycle actor. UI notifications use its injected UiEventSink.
The frontend keeps its command request shape and receives the existing structured
mutation outcome, already handled by the global MutationCache notifier.

Proxy-cache warming runs through the injected facade after the lifecycle result.
The tray/hotkey mode interruption policy remains a documented boundary bridge for
its separate migration, but runs only after successful application rather than
racing configuration changes in an independent task. The frontend retains its
existing close-all trigger, also moved after application success. No new interruption policy is introduced in this configuration-only batch.

ApiClient expiry rules remain unchanged: the runtime reconciler chooses hot patch
or replacement, and CoreActor observes the resulting instance identity. Do not
add a second direct PATCH path that bypasses runtime revision accounting.

## Delivery and validation

- Added PatchOverrides to ClashConfigActor and PatchRuntimeOverrides to the
  lifecycle actor. Both UI entry paths use NyanpasuClient::patch_runtime_overrides.
- Removed application PUT/PATCH helpers and their unused generic request-body
  machinery. Only legacy connection-interruption requests remain in that helper.
- IPC now returns MutationOutcome rather than unit. Existing frontend mutation
  notifications handle committed_degraded; the request DTO is unchanged.
- Seven new regression tests cover independent field patches (including script
  mode), concurrent application, lifecycle admission, mirror preparation failure,
  actual filesystem persistence failure, durable state after reconcile failure,
  and UI failure after successful reconciliation.
- Full application library suite passed (482 passed, one ignored before the final
  UI-failure test addition); all six lifecycle config tests passed after the final
  additions. Workspace Clippy/all targets/all features, interface build and all
  three frontend TypeScript checks passed. Existing application warnings remain.
- Generated bindings were formatted with the current lockfile's tools. The
  architecture ledger records two fewer Config::clash calls; its additional
  legacy DTO reference comes from the injected failure-test bridge.

No runtime dependency change or live GUI/core smoke test is included. Remaining
connection policy ownership and WebSocket migration stay in their separate batches.
