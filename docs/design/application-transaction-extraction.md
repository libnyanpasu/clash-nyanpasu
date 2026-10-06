# Shared application facade and profile transaction extraction

Status: implementation plan, 2026-10-06. This document follows
[Application and platform extraction](application-platform-extraction.md) and
defines the next reusable application boundary after the shared runtime and
script-engine slice. It is a design and migration plan; it does not claim that
the transaction or facade has already moved.

## Goal and boundary

Move the existing profile/configuration/runtime transaction out of the Tauri
crate without changing its commit semantics, and make the same application
facade serve the desktop and OpenWrt hosts. Keep the existing `NyanpasuClient`
use cases as the source of truth. The OpenWrt host must not acquire a second
profile store, mutation journal, commit coordinator, workflow actor, or facade
that reimplements those use cases.

The complete transaction boundary includes all three persistent source domains
(`application`, `clash_config`, and `profiles`), their prepare/commit/rollback
participants, the runtime workflow that votes and settles each participant, and
the receipts returned to callers. A source actor cannot move independently of
the workflow it contacts through `MutationCoordinator`.

The composition roots remain distinct:

```text
Tauri host                                             OpenWrt host
  desktop paths, storage, GUI capabilities               router paths, procd, ubus
  Tauri RPC and event adapters                            LuCI rpcd/ubus adapter
            |                                                       |
            +------------- nyanpasu-application -------------------+
                           canonical NyanpasuClient
                           application/config/profile actors
                           MutationCoordinator + ApplicationWorkflow
                           CoreClient + runtime transaction models
                                      |
                           consumed ports / domain models
                                      |
                         nyanpasu-platform implementations
                         filesystem, network, script, process, core
                                      |
                     nyanpasu-config / nyanpasu-core / core-manager
```

`nyanpasu-application` owns the decisions and sequencing. The host owns paths,
concrete IO and process control, host lifecycle, and transport. Application
code may consume ports and shared domain models; it must not import Tauri,
`PathResolver`, egui, a host RPC router, or the platform crate.

## The existing call path

Today the desktop composition starts the state actors before it starts their
runtime/effect owners. `NyanpasuClient::try_new_with_args` creates a pending
`MutationCoordinator`, opens application/session/clash state and the profile
service, and then enters `with_parts`. `with_parts` creates the effects owner
and `ApplicationWorkflowActor`, then connects the coordinator. The connection
is the write-admission barrier: the source actors cannot commit a production
mutation while the workflow is still being assembled.

The cross-domain path is:

```text
NyanpasuClient public operation
  -> ApplicationClient / ClashConfigClient / ProfilesClient
      -> ApplicationActor / ClashConfigActor / ProfilesActor
          -> MutationCoordinator::participant (when runtime impact requires it)
              -> ApplicationMutationParticipant::on_prepare
                  -> ApplicationWorkflowActor::BeginMutation
                      -> freeze committed app/clash/profile inputs and content
                      -> build candidate RuntimeIntent
                      -> validate/check according to host capability
                      -> submit/reconcile through CoreClient
              <- authoritative commit/rollback decision
          -> source persistence + profile resource promotion/compensation
      -> MutationReceipt / CommitReceipt / structured degradations
  -> post-commit notifications to the relevant owner
```

The runtime participant votes before the state manager commits a required
runtime change. A rejection or undecided vote aborts the source transaction.
Once the source has committed, failures from downstream effects are returned as
degradations or recovery work, not represented as an unsuccessful save. The
application workflow owns the runtime half of that decision, while
`ProfilesActor` owns the durable profile-file half. Both protocols must stay
intact and ordered.

Profile materialization is a separate resource transaction nested in the
profile actor's state transaction. The existing protocol covers state-first
and file-first updates, durable prepare/promote/complete steps, compensation,
cleanup fencing, and restart reconciliation. The subscription scheduler and
external watchers submit profile actor messages; they do not write profile
state around its transaction participant.

The current concrete locations and their target owners are:

| Current path                                                                                                                   | Target                                                                                 | Reason and boundary                                                                                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `backend/tauri/src/client/application.rs`, `clash_config.rs`                                                                   | `nyanpasu-application`                                                                 | Typed clients and their persistent state actors are shared domain owners.                                                                                             |
| `backend/tauri/src/client/profiles.rs`                                                                                         | `nyanpasu-application`                                                                 | Existing profile facade and typed client; callers must retain its receipts and source status behavior.                                                                |
| `backend/tauri/src/state/application.rs`, `clash_config.rs`                                                                    | `nyanpasu-application`                                                                 | Source actor handlers and versioned commits.                                                                                                                          |
| `backend/tauri/src/state/profiles/{actor,error,ports,sources,scheduler,jobs}.rs`                                               | `nyanpasu-application::state::profiles::{actor,error,ports,sources,scheduler,jobs}`    | Profile actor owns profile state, refresh fencing, materialization protocol and source ledger. Its ports are consumer-owned.                                          |
| `backend/tauri/src/state/mutation.rs`                                                                                          | `nyanpasu-application`                                                                 | The startup-injected connection, runtime participant construction, settlement and source-facing commit errors are part of the same transaction boundary.              |
| `backend/tauri/src/client/application_workflow/**`                                                                             | `nyanpasu-application`                                                                 | Admission, runtime voting, confirm/cancel, recovery, startup reconcile and retries are one actor-owned workflow. Move its behavior and tests, not a forwarding shell. |
| `backend/tauri/src/client/runtime.rs` transaction portion (snapshot/store/receipt/outcome)                                     | `nyanpasu-application::client::runtime`                                                | Immutable product state and recovery receipts; no filesystem paths or Tauri UI types.                                                                                 |
| `backend/tauri/src/client/runtime.rs` filesystem portion (`RuntimePaths`, `CandidateFile`, `write_product`, candidate cleanup) | `nyanpasu-platform::runtime`                                                           | Explicit paths and atomic/private filesystem operations; no `PathResolver`.                                                                                           |
| `backend/tauri/src/client/runtime_error.rs`                                                                                    | `nyanpasu-application::client::runtime_error`                                          | Structured runtime errors crossing shared operations and mutation results.                                                                                            |
| `backend/tauri/src/client/ports.rs`                                                                                            | `nyanpasu-application::client::ports`                                                  | Session candidate/confirmed resolution is shared state; port allocation is an injected `PortProbe` consumed by application.                                           |
| `backend/tauri/src/service/profile_file.rs`                                                                                    | `nyanpasu-platform`                                                                    | Filesystem journal/materialization and subscription HTTP implementation are concrete adapters. Paths and self-proxy port source must be constructor inputs.           |
| `backend/tauri/src/enhance/artifact_snapshot.rs`                                                                               | Tauri host initially                                                                   | Desktop command projection remains at the host edge; only move its mapper if it becomes pure and has a real second consumer.                                          |
| `backend/tauri/src/client/runtime_inspection.rs` graph/step-log source data                                                    | `nyanpasu-application` domain diagnostics                                              | `ConfigSnapshotsGraph`, `StepLog`, and pure graph mutations are reusable diagnostic data.                                                                             |
| `backend/tauri/src/client/runtime_inspection.rs` inspection response DTOs/query                                                | Tauri host projection initially                                                        | Specta/UI DTOs, display strings, and event correlation are desktop-specific.                                                                                          |
| `backend/tauri/src/enhance/chain.rs` `PostProcessingOutput` and log spans                                                      | `nyanpasu-application::enhance`                                                        | Pure serde diagnostics produced by the shared transform pipeline.                                                                                                     |
| `backend/tauri/src/client/application_workflow/adapters.rs`                                                                    | Split: application port definitions; concrete adapters in platform or host composition | It currently combines profile reads, `RuntimeBuilder`, blocking execution, Tauri runtime paths, core mapping and UI artifact projection.                              |
| `backend/tauri/src/core/migration/modules/profiles.rs` `ProfilesDocument`/`ProfilesFormat`                                     | `nyanpasu-application::state::profiles::format`                                        | Move only the plain `StampedDocument` and `StampedYamlFormat` declaration; migration steps remain in Tauri bootstrap.                                                 |
| `backend/tauri/src/core/migration/modules/profiles.rs` migration runner                                                        | Tauri bootstrap                                                                        | Its `Ctx`, migration step, and filesystem runner are host-owned and run before the shared state manager opens.                                                        |

The profile ports already express most of the adapter boundary:

- `ProfileFsPort` reads, writes, removes, checks and links managed/external
  files.
- `ProfileMaterializationPort` owns durable materialization and cleanup handles,
  including prepare, promote, complete, compensate and reconcile operations.
- `SubscriptionFetcher` returns content and normalized metadata; its adapter
  owns HTTP deadlines.
- `ProfileContentSource` and `ScriptRunner` are the runtime-builder inputs.

Those traits currently live beside the Tauri actors. Move each trait to the
application/domain layer that consumes it and move concrete implementations to
platform or a host only when they require a host capability. Do not replace
these task-specific ports with a generic adapter registry.

The concrete profile adapter receives an explicit `profiles_root`. Its private
materialization directory remains inside that root and retains the current
symlink/reparse checks, same-filesystem journal transitions, hash fences,
directory sync, private permissions, retry behavior and startup reconciliation.
Split `ProfileFileService` into the filesystem/materialization adapter and a
distinct HTTP `SubscriptionFetcher`; this separates network dependencies
without changing the profile actor's port protocol. The platform crate owns
only its narrow `SelfProxyPortSource` interface. Tauri supplies an adapter
backed by confirmed session ports; platform code must not reach into
`client::SessionPortResolver`. System-proxy lookup and device identity are
explicit fetcher construction inputs so router builds do not inherit Tauri
globals or claim desktop OS capabilities.

## Stage 2: finish existing Core and session slices first

Stage 2 is two independent, buildable actor migrations. It is not permission to
introduce a router-specific application facade.

### Stage 2A: CoreActor v2

Move only the host-independent CoreActor v2 slice from
`backend/tauri/src/core/actor_v2/` into `nyanpasu-application::core`: the typed
`CoreClient`, actor and its messages/state, endpoint contract, status/owner
projection types, and instance-bound API client/lease. The current draft in
`backend/nyanpasu-application/src/core/{actor,api,endpoint}.rs` and
`backend/nyanpasu-platform/src/core/endpoint.rs` is the intended starting
point. The actor owns endpoint selection, owner generation, status projection,
operation waiting and revocable instance-bound API leases.

Do not claim that the higher-level `CoreFacade` or Tauri `ServiceClient` has
moved in this stage. `facade.rs`, `service_actor.rs`, service readiness and
desktop service lifecycle remain host/workflow dependencies until Stage 3
decides their exact application port and ownership. This boundary keeps Stage 2
buildable without prematurely moving the application workflow/facade graph.

Keep `ControlEndpoint` as the consumed boundary. The local in-process endpoint,
IPC/service endpoint and their connection watchers are concrete adapters. Put
host-independent process-control implementations in platform only when they
do not depend on the host's packaging or supervisor; retain Tauri service
installation and desktop service lifecycle in the Tauri composition root.
OpenWrt implements fixed procd/core endpoints behind the same application
contract.

The commit migrates direct callers of the moved typed `CoreClient` and its
endpoint/API types, moves the corresponding actor tests, and removes those old
Tauri implementations. Callers that still need `CoreFacade` or `ServiceClient`
continue to use their Tauri-owned implementations until Stage 3. A Tauri module
that merely re-exports moved actor/client types is not the target. Verify the
same handoff, unknown owner, stale notification, effective API binding, and
shutdown behavior.

### Stage 2B: persistent session state

Use the existing `nyanpasu-application::session_state::{actor,client,error}`
implementation for both hosts. Tauri opens its persistent manager and passes it
to `SessionStateClient::new`, including the main-window label it owns. OpenWrt
does not create a window state consumer. Remove the duplicate Tauri
`state/session_state.rs` actor and `client/session_state.rs` client after all
desktop geometry callers use the shared client.

Session state is deliberately independent of the profile/runtime transaction:
it has no mutation participant and does not participate in runtime commits.
Its actor owns only persistent session/window state; the Tauri host remains the
owner of actual window placement and supplies the label/geometry API.

## Stage 3: one canonical facade and the complete transaction path

After Stage 2 is integrated into the desktop composition, move the complete
profile/config/runtime path together. The application crate exposes one
canonical `NyanpasuClient`, created by an application supervisor from explicit
startup arguments. Both the Tauri host and the OpenWrt host call that facade.
The OpenWrt prototype `openwrt_application::NyanpasuClient` and `StateActor`
must remain unexported and must not become a parallel profile or runtime
implementation. Retire the prototype once the canonical path provides its
required operations.

The canonical startup arguments should be the minimal shared graph, not a copy
of Tauri's present `ClientSetupArgs`:

```text
ApplicationSetupArgs
  application/clash/profile/session persistent managers or consumed stores
  profile fs/materialization/fetch ports
  runtime compiler/content/script and candidate check/publish ports
  CoreClient endpoint supply
  optional commit notification port(s)
  root CancellationToken + TaskTracker
  installed ReleaseChannel and other genuinely shared policy inputs
```

The composition root opens paths, performs required schema migration, creates
adapters, spawns actors and connects runtime participants only after all owners
are ready. The shared facade must not accept `AppHandle`, `PathResolver`,
`BundleMetadata`, `Window`, `HttpRoutes`, Tauri `Storage`, an RPC router or an
arbitrary map of services. For persisted state, either pass the opened typed
`PersistentStateManager`s or define a narrow domain store port; do not pass a
Tauri path resolver into application. Preserve the existing stamp/schema
format. The Tauri migration runner can continue to run before shared startup
until the profile document migrator is separately made host-neutral.

Move the following connected implementation as one transaction-owning slice:

1. `ApplicationActor`, `ClashConfigActor`, `ProfilesActor`, their typed clients,
   source snapshots/errors and the profile scheduler/watchers/jobs integration.
2. `MutationCoordinator`, its single runtime `StateParticipant` adapter,
   receipts and structured `CommitAborted`/`RuntimeAftermath` results.
3. `ApplicationWorkflowActor`, runtime preparation/inputs, impact/policy
   classification, `Try`/`Confirm`/`Cancel`, restore and recovery journal,
   startup reconciliation, convergence and the source/effect notification
   interface. Keep the serial owner and its root cancellation/task tracker.
4. The shared portion of `NyanpasuClient`: app/clash/profile reads and writes,
   import/refresh/activate/reorder operations, runtime build/apply/status,
   core start/stop/reconcile. Preserve method semantics and structured error
   variants where they are application wire contracts.
5. Desktop RPC handlers and all callers switch to this canonical client in
   the same migration series. OpenWrt ubus maps an explicit allowlist onto the
   same methods; it does not implement mutation, runtime or profile logic.

Source actors and the workflow actor move together because there is a real
cycle of protocol types: profile/config commits classify runtime impact and
create a required participant; the workflow replies with the authoritative
decision and receipt; the source returns the final receipt to its caller. The
cycle is internal to the application crate and expressed by typed clients and
ports, not by a cross-crate re-export or `ActorRef` map.

### Shared runtime product versus host projection

The current desktop `RuntimeSnapshot` is not suitable as the shared runtime
transaction type. It contains a runtime file product, but also a Tauri
`PostProcessingOutput`, inspection identity/data, effective-config UI state,
and path/candidate operations. Split it before moving the workflow:

| Shared application value                                                                                               | Host/projection value                                                    |
| ---------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| Candidate revision and target core                                                                                     | Desktop inspection id and inspector cache                                |
| Exact serialized config bytes and digest used for check/apply/recovery                                                 | `PostProcessingOutput` rendered for desktop UI                           |
| Immutable `ConfigSnapshotsGraph`, `StepLog` diagnostics and runtime provenance                                         | Desktop query DTOs, `exists_keys` and display-oriented inspection schema |
| Confirmed apply receipt: exact accepted bytes, core identity, endpoint owner/generation, run intent and resolved ports | Effective-config projection and Tauri event correlation                  |
| Build/check/reconcile errors and degraded status                                                                       | Specta inspection projection and Tauri event correlation                 |

The shared compiler consumes an immutable `RuntimeBuildInput`: committed
application/clash/profile snapshots, resolved port bindings, and frozen contents
for the selected profile dependency closure. Freeze content before entering
blocking script execution so the file reads and the bytes built cannot race.
The shared `RuntimeBuilder` and Boa/Lua adapters remain the implementation
established in stage 1. It produces the exact candidate config and diagnostics;
it does not open profile paths, stage files, call the core, or create a UI
projection.

The runtime ports have these responsibilities and data directions:

| Port                                                                                  | Application supplies                                                         | Adapter returns/does                                                                                                                  |
| ------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `ProfileContentSource` / runtime content capture                                      | Current committed `Profiles` and selected dependency closure                 | Frozen `path -> Result<text,error>` map; no later read during the build                                                               |
| `RuntimeCompiler` (or the existing `RuntimeBuildPort` after removing host data types) | Immutable snapshots, frozen content, resolved ports, strictness policy       | Candidate product: exact bytes, digest, target core, revision and build logs                                                          |
| `RuntimeValidatorPort`                                                                | Exact candidate intent/bytes, core spec, host capability                     | `Passed`, deterministic `Rejected`, or structured `Unavailable`; missing check is never silently a pass                               |
| `RuntimePublisherPort`                                                                | Candidate product plus previous confirmed product when compensation needs it | Validate/stage/promote/restore product atomically; report publication and cleanup failures                                            |
| `ControlEndpoint` through `CoreClient`                                                | Typed submit/status/check request                                            | Host-reported operation and status; the actor owns routing/generation, host owns lifecycle truth                                      |
| `CommitNotifications` (narrowed consumed port)                                        | Committed domain slice or confirmed runtime binding                          | Best-effort downstream notification after the source transaction; no write vote and no desktop `ApplicationEffectPlan` in application |

The concrete desktop adapter can keep producing the current runtime file and
inspection projection from the same candidate. The OpenWrt adapter writes the
fixed core config and exposes only fields required by its status response. Both
must check and apply the same bytes. The application stores the shared
product/receipt and graph diagnostics needed for recovery; a Tauri inspector
projects those into its richer inspection response at the host edge.

The shared application runtime snapshot/store owns candidate revision, target
core, immutable graph and step-log diagnostics, serialized product bytes and
digest, confirmed-apply receipt, effective-config state required for recovery,
and promotion/confirmation transitions. Keep graph data (currently
`RuntimeInspectionData`) in application as domain diagnostics; desktop
`RuntimeInspection`/node/content response DTOs, display strings, Specta query
shape and event correlation stay at the Tauri edge. Move the pure serde
`PostProcessingOutput` and log-span types under application `enhance`; keep
presentation mapping out. `artifact_snapshot` stays Tauri unless it becomes a
pure mapping with an actual second consumer.

Move the filesystem half of `client/runtime.rs` to
`nyanpasu-platform::runtime`: `RuntimePaths::new(product, candidate_dir)` takes
explicit paths, `write_product` keeps atomic publication, and candidate
creation/cleanup preserves private-directory permissions, no-symlink checks,
unique names, fsync and stale-age cleanup. Delete `RuntimePaths::from_resolver`.
The workflow/compiler boundary does not expose a concrete `CandidateFile`; a
validator adapter owns the staged candidate when its host's check operation
accepts a path. Application-owned `PublishRuntimeError` carries portable
structured data; map filesystem failures without exposing `atomicwrites` or a
Tauri path resolver through the application dependency graph.

Move `SessionPortResolver` into `nyanpasu-application::client::ports`, but make
port probing explicit. The application-owned `PortProbe` port accepts a
`PortStrategy` and returns `PickedPort` or `PickPortError`; production setup
must inject it, with no application default that performs socket IO. The
platform `SystemPortProbe` delegates to the existing config strategy's
`pick_and_try_port()`. The resolver continues to reuse confirmed bindings
from the shared runtime receipt and only probes changed strategy fields.
Platform defines `SelfProxyPortSource` locally and implements it for the app's
resolver by reading confirmed mixed port; this avoids a dependency from
application back to platform.

Profile materialization remains an application-owned protocol over
`ProfileMaterializationPort`; `ProfileFileService`'s journal and filesystem
operations move to platform. The platform implementation receives explicit
profile/config roots, and the existing self-proxy port source becomes an
injected value/port without a platform-to-Tauri import. Subscription HTTP keeps
its adapter-owned network timeout. The actor keeps refresh attempt fencing,
server metadata handling, scheduler ownership and restart reconciliation.

### Desktop-only capabilities

The present `backend/tauri/src/client/mod.rs` is both application facade and
desktop service container. Moving the file as a whole would pull its host
dependencies into the application crate. The following capabilities stay at
the Tauri host boundary and are not part of the shared MVP facade:

- debug HTTP server, frontend development routes and HTTP/IPC registration;
- app updater and release bundle metadata;
- backup/storage commands and desktop install/update channels;
- window geometry effects, tray refresh, hotkeys and main-thread work;
- system proxy, system DNS cache, direct egress diagnostics and desktop OS
  binary installation/service management;
- log event sinks, frontend event projection, traffic database/cache, geo index,
  and desktop-specific proxy/WebSocket UI streams.

After extraction, these operations live in Tauri bootstrap state and narrow
typed adapters/clients that receive the shared `NyanpasuClient` only where they
need shared use cases. They do not retain another profile/runtime facade and
do not call Tauri globals from application. RPC commands for the shared
operations accept the canonical application client; commands for these host
capabilities receive their explicit Tauri host state. Existing command names,
metadata, authorization and generated bindings stay unchanged unless the
public operation itself changes.

Core start/stop is shared as a use case but its execution is host-specific:
Tauri may use its existing local/service endpoint and desktop lifecycle
adapters; OpenWrt invokes its fixed procd service adapter. Service installation,
binary replacement and updater flows remain desktop-only unless they are later
specified as shared application operations. A host that cannot provide a
capability returns an explicit unsupported/unavailable result; a no-op adapter
must not claim success.

## Acceptance and regression coverage

Move tests with the behavior and run them against the shared owners. Keep host
projection goldens at Tauri level. The existing Tauri tests are the behavioral
specification during the move; representative current cases include:

- **Required vote and zero-write refusal:**
  `application_workflow/tests/mutations.rs` tests
  `a_closing_workflow_refuses_a_mutation_before_anything_is_committed`,
  `a_rejected_check_refuses_the_mutation_without_touching_the_runtime`,
  `a_running_core_with_no_confirmed_apply_refuses_a_critical_mutation`, and
  `a_preflight_failure_or_changed_revision_never_submits_or_isolates`.
  Assert the profile/config source version and materialized bytes do not change
  when a required runtime participant refuses.
- **Commit followed by degraded side effect:** preserve
  `add_complete_failure_returns_committed_degradation`,
  `refresh_complete_failure_commits_with_degradation`, and
  `confirm_and_cancel_each_hand_the_effects_owner_the_runtime_slice`. The
  caller must see a committed result plus structured degradation; it must not
  be told that the source write rolled back.
- **Rollback and required-participant failures:** preserve
  `a_failed_save_restores_the_verified_runtime_baseline`,
  `a_host_switch_moves_the_runtime_inside_the_try_and_back_on_cancel`, and
  `a_failed_apply_after_a_handoff_puts_the_original_runtime_back`. Assert the
  runtime receipt, owner binding and port receipt describe the verified
  restored baseline or explicitly report recovery required.
- **Restart and recovery:** keep
  `application_workflow/tests/recovery.rs` cases for a lost cancel restore and
  an accepted in-flight submission, plus startup tests
  `a_clean_local_start_applies_once_under_a_proven_owner`,
  `recovery_keeps_a_running_owner_only_when_it_runs_the_committed_target`, and
  `a_save_committed_before_startup_is_what_startup_applies`. Start a fresh
  composition over persisted state and prove reconciliation does not claim an
  unverified core state.
- **Profile materialization and compensation:** preserve the targeted tests in
  `client/profiles.rs` such as `materialization_reconciles_before_client_startup_returns`,
  `import_is_not_committed_when_materialization_promote_fails`,
  `add_promote_and_compensate_failure_reports_compound_error`,
  `refresh_promote_failure_compensates_without_advancing_metadata`,
  `delete_activate_failure_commits_deletion_with_cleanup_degradation`, and
  `refresh_persist_failure_restores_previous_materialized_bytes`. Also retain
  `external_background_commit_does_not_bypass_the_mutation_participant` and
  import/refresh fencing tests for caller cancellation and changed definitions.
- **Wire compatibility:** serde and Specta contract tests must preserve the
  existing profile/config schemas, `ScriptType` names, operation names,
  structured error tags, commit receipt/degradation fields, and Tauri RPC
  query/mutation metadata. Moving a Rust type alone must not rename its
  exported Specta type or alter its serialized representation.
- **Projection separation:** desktop runtime golden tests continue to verify
  the same final YAML, logs and artifact inspection payload after building
  through shared application/platform services. A platform test separately
  exercises the compiler and real JS/Lua adapters without importing Tauri.
- **Host parity:** an OpenWrt actor test drives the canonical facade with fake
  ConfigStore/profile/core adapters and asserts the same commit/receipt policy.
  A ubus allowlist test checks that requests route only to shared facade
  methods. Do not use a second mock application implementation as a substitute
  for desktop-to-shared caller migration.

Run focused application/platform tests first, then desktop crate checks and
transaction/profile tests, formatting, and the backend dependency architecture
gate. Docker aarch64/x86_64 musl tests validate the extracted neutral crates;
they do not prove procd, ubus, or hardware-router operation. No package is
complete until both hosts compile against the same application facade and the
shared behavior tests above pass.

## Commit sequencing

1. Finish CoreActor v2 migration and all desktop callers; stage only that
   buildable slice.
2. Replace the duplicate Tauri session actor with the shared session client;
   stage only that independent slice.
3. Move the profile/config/runtime transaction graph, its concrete adapter
   ports, and desktop call sites as one behavior-preserving migration. Split
   shared runtime product from desktop inspection projection before moving
   workflow types. Do not stage a halfway state with `MutationCoordinator` or
   source actors pointing at different workflows.
4. Move the shared facade startup/methods and rewire desktop RPC and OpenWrt
   ubus to it. Remove the prototype router business actor and the old Tauri
   implementations as part of the applicable migration commits.

Each commit must compile its consumers and update the dependency-boundary
documentation/gate when package ownership changes. Stage explicit paths; do not
include backend workspace/lock, OpenWrt packages, or docs owned by another
phase unless that commit's change requires them.
