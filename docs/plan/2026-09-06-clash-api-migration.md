# Clash API caller migration

## Goal and scope

Route application Clash REST and streaming callers through the runtime workspace's
`clash-api`, with an explicitly injected, instance-bound API capability issued by
CoreClient. Tauri commands remain adapters over NyanpasuClient. The frontend keeps
using generated commands/events; dashboard links and unrelated HTTP services are
outside this migration.

The baseline is application cab2330b and runtime a3512f9. Runtime is a separate
repository: any runtime changes require a dependency PR and an updated gitlink in
the application PR. Deliver reviewable changes; do not merge either PR.

## Lifecycle contract

- Runtime epoch is not process identity: restart and rollback may reuse an epoch,
  and the process supervisor can respawn inside one Instance.
- Publish a new opaque process identity for each actual process start, including
  automatic respawn. Preserve it across hot patch and in-process reload.
- Bind each issued API capability to controller generation, actual endpoint,
  credentials, and process identity. The endpoint comes from the applied runtime,
  not the desired config's legacy global mirror.
- Revoke old capabilities on replacement, handoff, stop, shutdown, unavailable
  endpoint, or changed credentials. Clones share revocation; they never revive.
- Check before requests and before returning results; cancel in-flight operations
  and streams when revoked. Discard delayed events from retired identities.
- Do not transparently replay mutations on another process. Cancellation cannot
  undo an already accepted request; a lost mutation reply has an uncertain outcome.
- A remote status feed has a detection window. Strict prevention of an HTTP request
  reaching a replacement process on a reused address requires a runtime-side
  identity fence or an instance-specific transport. Do not claim that polling or
  cancellation closes this distributed race.

## Architecture and service classification

CoreActor owns the capability lifetime (actor service). ApiClient is an injected
infrastructure adapter wrapping the typed protocol client with revocation; it must
not expose a raw Client, RequestBuilder, or unguarded socket. Request execution is
outside the CoreActor mailbox. ProxiesActor owns proxy cache/selection orchestration;
Clash streaming actor owns connections/history/subscriptions. DTO conversion and
connection interruption policy are pure functions. UI event emission stays behind
ports, with wiring in the composition root.

Configuration/lifecycle writes keep the existing lifecycle actor as their owner;
opening a typed API must not create a second config writer or expose unmanaged
restart/upgrade operations.

## Work sequence and checks

1. Establish runtime process identity and controller projection.
   Verify same-epoch restart/respawn changes identity, patch preserves identity,
   and Local/Service projections agree. Missing identity is unavailable, never a
   guessed PID/epoch fallback.
2. Implement the revocable typed API and CoreClient acquisition.
   Verify cloned handles, cancellation while reading a response, handoff and
   shutdown, endpoint/secret changes, hot patch stability, and stale snapshots.
3. Migrate REST commands through NyanpasuClient, then injected proxy cache and
   connection interruption call paths. Remove the old URL/request implementation.
   Verify response fixtures for Mihomo, clash-rs and Premium before replacing DTOs;
   retain absence/unknown-field semantics instead of manufacturing values.
4. Migrate streaming connections to clash-api and fence stream updates by
   capability identity. Verify reconnection, recording controls, history limits,
   and rejection of old frames after restart.
5. Regenerate frontend bindings and adapt consumers to changed application DTOs.
   Verify TypeScript and relevant UI build checks. No frontend controller secrets
   or generic request transport are introduced.
6. Review diffs, run focused Rust tests and application compile checks, then
   commit related paths explicitly and create PR(s) with actual validation and
   any remaining migration boundary documented here.

## Compatibility risks to resolve

The pinned clash-api is Mihomo-oriented: RuntimeConfig and Proxy require fields
other cores omit; SubscriptionInfo currently requires every PascalCase field;
ConnectionsSnapshot requires memory, and Connection requires providerChains.
Existing application fixtures cover several of these missing-field cases.
WebSocket helpers currently return raw sockets and need guarded typed consumption.
The application router polls status every two seconds and can accept a late status
frame from before RefreshStatus. This display cache is not an authority for
issuing or reviving an API capability. Runtime IPC publishes the controller but
intentionally excludes its secret: credential provenance must be explicit.

## Execution record

Runtime dependency: [nyanpasu-runtime #401](https://github.com/libnyanpasu/nyanpasu-runtime/pull/401) is merged. The submodule pins released `v2.0.0-rc.3` (`b14f0d0`); Service compatibility requires at least `2.0.0-rc.3` for `/v2/core/api-connection`.

### First implementation / review boundary

This PR implements steps 1–2 and a bounded part of step 3; it is not a completed
migration of every caller. The remaining steps are follow-up work, not hidden
compatibility APIs for new callers.

Delivered:

- Runtime publishes `instance_id` (a UUID generated on each supervisor Started
  event), including same-epoch restart and automatic respawn. Identity-only
  changes wake status subscribers even when PID, epoch and health are unchanged.
- `CoreManager`/`CoreControl::api_connection()` reads the process identity and
  applied controller credentials under the control lock. Service exposes that
  binding at `/v2/core/api-connection` over the existing ACL-protected IPC socket;
  credentials are redacted in Debug and excluded from status/events.
- `CoreClient::api_client()` issues an `ApiClient` whose private protocol client
  and shared cancellation token belong to a CoreActor-owned lease. Acquisition
  reads the authority directly, rather than trusting the router's display cache.
  Dropping the lease on handoff, degradation, shutdown, or actor destruction
  revokes every clone. Reacquisition with the same binding reuses a live lease.
- A lease-owned monitor consumes Local watch / Service event notifications and
  rechecks the actual binding. A two-second recheck also covers lost/coalesced
  notifications and secret changes. Notification payloads never revive a lease.
  Binding/subscription reads are bounded at ten seconds; complete API operations
  (preflight, network/body decode, postflight) are bounded at thirty seconds.
- Version, proxy/provider-proxy delay, group delay and explicit close-connection(s)
  commands now use `NyanpasuClient` and the typed API. Requests and bodies are no
  longer manually assembled for these callers; the unused legacy query transport
  was removed. Premium's version flag is preserved by clash-api.
- Frontend command DTOs remain stable. Generated service-status bindings gain the
  optional `instance_id`; no credential-bearing type is exported to the frontend.

The monitor provides revocation on observed authority changes. Preflight and
postflight prevent knowingly using/accepting an obsolete binding; they do not
provide atomic HTTP admission against a concurrent process replacement. Neither
Local nor Service can undo an already transmitted mutation on a reused HTTP
address. Strict process-side fencing and guarded streaming remain future work.

Service deployments must include a daemon built with the runtime dependency PR.
The compatibility gate rejects older daemons lacking the new endpoint. The
matching service binary is published in `v2.0.0-rc.3`; the download script reads
that version from the pinned runtime manifest.

### Remaining migration inventory

| Callers                                                    | Required next change                                                                                                                                                                                               |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| configs / provider REST reads in `ipc.rs`                  | Make cross-core response absence and unknown enum values explicit before replacing old optional DTOs. Rule reads and provider refresh are covered by [the next migration](2026-09-07-clash-rule-api-migration.md). |
| `ProxiesGuard`, tray proxy actions, provider updates       | Introduce injected ProxiesActor ownership and migrate its cache, notifications and selection workflow together.                                                                                                    |
| `feat::change_clash_mode`, `ConnectionInterruptionService` | Move policy inputs and post-commit side effects into injected application workflows; do not add a second config writer.                                                                                            |
| `ClashConnectionsActor` and widget/UI subscriptions        | Guard typed stream consumption and queued messages by capability identity, then migrate shared history ownership and UI sinks.                                                                                     |

The remaining legacy controller lookup is explicitly marked `TODO(actor-migration)`
with this plan as its reason/removal condition. New callers must use the facade.

### Validation

Performed on macOS with debug symbols and incremental compilation disabled to
keep independent build artifacts small:

- Application library compile and `cargo clippy -p clash-nyanpasu --all-targets
--all-features --offline`: passed (existing warnings remain).
- Retained cross-core application API DTO tests: 11 passed.
- CoreActor tests: 74 passed, including clone invalidation, same-epoch/PID
  replacement, unchanged binding, credentials/endpoint changes, handoff,
  shutdown/destruction, and cancelling a partially received response body.
- Runtime core-manager: 102 unit tests passed, 1 existing ignored test;
  config_apply: 20 passed; instance_lifecycle: 16 passed. The latter suites
  exercise actual fake-core processes, preserving identity through patch/reload
  and changing identity on restart/respawn.
- Runtime clash-api / IPC / service library tests: 19 / 11 / 84 passed.
- IPC wire golden tests: 32 passed; clash-api REST / stream integration tests:
  3 / 2 passed.
- Specta binding generation and shape assertions, interface build, UI build and
  all frontend TypeScript checks: passed. The fresh worktree's UI build generates
  Paraglide assets before the final type check.

One existing core-manager test,
`config::tests::managed_bootstrap_zeroes_listeners_and_keeps_the_source_snapshot`,
is skipped on this machine because it unconditionally passes a Windows named-pipe
path to the Unix controller validator. Its source is unchanged by this PR.
`TMPDIR=/private/tmp` avoids macOS `/var` versus `/private/var` canonical-path
mismatches in the other Unix fixtures; socket/process tests run outside the
filesystem/network sandbox. Windows named-pipe behavior was not executed locally.

### CI and ordered response follow-up

- Refresh the architecture ledger snapshot for the documented remaining legacy
  controller lookup; the gate still compares exact counts. A temporary-worktree
  regression test ensures ancestor `tmp` directories do not exclude the source.
- Preserve clash-api's `IndexMap` order through the group-delay facade and IPC
  return type. Environment diagnostics return core versions as a `BTreeMap` for
  stable key order. Both retain the existing JSON object / TypeScript shapes.
- Other `HashMap` uses in the application are internal state, request inputs or
  persistence, rather than IPC response types. Runtime clash-api response maps
  already use ordered collections.
- Reject Service rc.2 explicitly: it lacks the API connection route even though
  its lifecycle routes are supported. The bundled-version guard covers rc.3.

Follow-up validation on macOS: `cargo test --all-features --offline` passed
(application library: 464 passed, 1 existing ignored), including the 12 Service
compatibility tests and IPC binding export. Full-workspace Clippy passed with
existing warnings. The ledger's 33 tests, exact snapshot gate, Prettier, Oxlint,
style checks, all frontend TypeScript checks and Rust formatting passed.
`lint:deno` formatting passed, but its full script type check could not finish:
JSR downloads for an unchanged notification script failed with a TLS handshake
error on both attempts. The changed ledger scripts were type-checked by their
passing Deno test run.
