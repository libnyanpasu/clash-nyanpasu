# Session traffic accounting

## Scope and revised ownership

This design replaces the service-owned design in the original draft PRs. The
user requires all traffic actors to live in the current application core
(`backend/tauri`, eventually `nyanpasu-core`). The service must not gain a traffic
actor, database, collector, protocol capability, or traffic IPC endpoints.

The backend provides retained closed connections, cumulative and current rule
usage, historical/live topology, and query primitives for a future statistics
page. New history/statistics UI pages are outside this change.

The user explicitly accepts gaps while the application is closed or disconnected
from the service. In service mode the application connects directly to the
instance-bound core API using the existing control endpoint capability.

## Layers

```text
Tauri commands and existing UI projections
    -> NyanpasuClient
        -> application-owned TrafficClient / TrafficActor
            -> pure accounting and topology services
            -> TrafficStore port -> redb (default), Turso (optional)
            -> TrafficSource port -> instance-bound core API

Application composition root
    -> injects store, source, clock, cancellation
    -> observes local process lifecycle or existing service API binding
```

- `backend/nyanpasu-traffic` contains domain records, deterministic accounting,
  topology projection, storage/source/clock ports and the default redb adapter. It has no
  actor, process supervisor, Tauri dependency, or service dependency.
- `backend/tauri/src/core/traffic` owns the traffic actor, typed client, source
  adapter and collector tasks. Actor state is private; no globals or second
  scheduling queue are introduced.
- `backend/nyanpasu-traffic-turso` is an isolated evaluation workspace. The
  embedded SDK enables `parking_lot/send_guard`, which conflicts with the app
  workspace's deadlock detection. It implements the same store port without
  changing the application dependency graph.
- The application facade exposes history independently of whether a controller
  is currently connected. A missing live controller does not erase local history.
- The runtime changes, if needed, are only generic ordered process lifecycle
  notifications. They carry no traffic concepts and do not start collectors.
- `nyanpasu-service-runtime` and `nyanpasu-ipc` do not depend on this library.

## Session identity and observation coverage

A session represents a real core process instance. Stable host/instance identity,
not PID alone or websocket lifetime, is the key. A websocket reconnection to the
same instance resumes that session. A different confirmed instance starts another.

Local process Started/Exited notifications supply exact lifecycle evidence.
Service mode uses existing instance-bound API information; attaching to an
already-running process is a late attachment. Coalesced status observations,
missing API access, app shutdown and disconnect are not proof of core exit.

Suspend stops the application's collector, clears current rates, and marks
coverage stale/unknown. It must not close remote connections with `CoreExited`
or manufacture `ended_at`. Recovery preserves stored counters and records a gap.
If a remote old instance can no longer be observed, its end remains unknown.

The collector uses the existing revocable `ApiClient` capability. Each decoded
frame passes the authoritative instance binding check before accounting, so a
restart cannot charge a new process to the old session between bridge polls.
A retired capability requires rebinding even if the connection DTO is unchanged.
Context is refreshed before a new source starts; failed configuration reads clear
context for newly observed connections. Existing connections keep the rule context
at their original match unless their reported rule changes.

App shutdown uses the root cancellation token and actor cleanup. Local owned
process cleanup may still deliver a genuine Exited event; the application does
not stop a service-owned process merely to finalize traffic records.

## Accounting invariants

Each complete connections frame becomes one atomic observation:

1. Compute changes from committed baselines using pure functions.
2. Commit session totals, connection changes, attribution facts, indexes and
   replay receipt together.
3. Advance the actor baseline and publish projections only after commit.
4. Acknowledge the source so it can supply the next frame.

One source has at most one unacknowledged frame; each owned instance has at most
one pending commit. Unknown commit outcomes are resolved through committed
position/digest rather than replaying increments blindly. Errors retain evidence
and invalidate current rates; one recovered instance cannot hide another error.

The first observed cumulative connection value is counted once. Later frames add
nonnegative deltas, with resets and invalid counters explicitly marked. Missing
connections close at detection time, without claiming exact final byte counters.
Connections born and closed between snapshots cannot be reconstructed.

Core-reported bytes and attributed bytes remain separate, with signed discrepancy.
Unknown intervals and first observations contribute time-unallocated bytes;
minute buckets must not fabricate when those bytes were transferred. Rates are
unknown across gaps and are never presented as zero merely because disconnected.

Rule identity includes kind/payload and available configuration context. Metadata
or path changes create attribution segments. Unidentifiable rules retain
conservative quality markers. Topology records process/source, rule, proxy path
and exit identity with unique membership; summing every edge is not session usage.

## Storage and queries

`TrafficStore` is an object-safe async port owned by the consuming domain. It
covers recovery, begin/commit/finish, exact session/connection reads, paginated
connections, grouped usage, topology, retention and flush. Concrete storage is
injected; callers never branch on redb/Turso or receive database handles.

redb remains the default. Transactions persist facts and indexes, including
active/filter indexes, rule totals, topology memberships and bounded top-500
rankings for seven dimensions. Complete group totals remain stored so `other`
is exact. Schema incompatibility is explicit. Live queries use active state;
indexed historical queries do not scan all closed connections unnecessarily.

Turso is an optional local embedded adapter, not a cloud service or production
setting switch. It must preserve the same contracts and integer ranges. Its
transaction cleanup must complete before cached statements are reused after a
failure. Switching engines does not solve unbounded history retention.

Connection queries use a stable session/filter/high-watermark cursor. Usage and
topology support Live, Session and UTC-aligned MinuteWindow scopes. Requests bound
page sizes, group results and time windows. Wire byte counters are decimal-string
u64 values; only existing display projections may convert to numeric UI values.

Closed records are paged from disk. Only attached sessions retain full active
baselines in the actor; detach releases them after preserving pending writes, and
rebind loads that session's active records through existing paginated queries.
Retired sessions retain lifecycle metadata rather than connection snapshots.
The current recovery port still materializes historical active records transiently
at startup; this change does not claim a bounded startup peak.
Retention protects active, selected and latest ended sessions. The initial scope
does not impose a hard disk quota or trim current-session detail. Cache budgets
are not process RSS or disk-size guarantees.

## Review and PR boundaries

Use separate, buildable PRs with explicit dependencies:

1. Runtime generic process lifecycle notifications, without traffic/service changes.
2. Application domain models, accounting, topology and storage contracts.
3. redb adapter, persistence/index contracts and regression tests.
4. Application traffic actor/source lifecycle and actor tests.
5. Application composition, query facade, Tauri commands and existing UI consumers.
6. Optional Turso adapter and shared storage contract tests.
7. Benchmark harness and clearly scoped measurement report.

Application PRs form a stack; each targets its immediate prerequisite so reviewers
see only that slice. The application integration pins the generic runtime change.
Do not include the optional comparison backend in the core actor PR. Superseded
drafts are linked and closed only after the replacements are available.

## Verification

Pure tests cover deltas, resets, metadata segments, unknown intervals and topology
conservation. Both stores exercise rollback/replay/reopen, full-width counters,
pagination, rankings and retention. Actor tests use explicit acknowledgements and
fake sources, including suspend/rebind and shutdown without invented process exit.

Application integration must cover service collection using existing API access,
disconnect/reconnect, history while offline, local lifecycle and native storage
coexistence. Frontend binding/type and projection tests remain required.

Previous benchmarks are historical evidence from the original architecture; moving
the harness into the application does not make those timings new measurements.
New results must identify the executable, database, durability and workload.
Cross-platform and OS fault tests are reported only when actually executed.
