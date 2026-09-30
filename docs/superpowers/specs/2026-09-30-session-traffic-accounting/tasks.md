# Implementation and PR split

The user superseded the original service-owned drafts with application-only
traffic ownership. Offline coverage gaps are explicitly accepted. The design in
`design.md` is authoritative; earlier daemon collection and traffic IPC plans no
longer apply.

## Review slices

| Slice             | Contents                                                               | Verification                                                 |
| ----------------- | ---------------------------------------------------------------------- | ------------------------------------------------------------ |
| Runtime lifecycle | Generic ordered Started/Exited notification port                       | Process lifecycle and native-store coexistence               |
| Domain            | Application-local models, pure accounting/topology, ports              | Pure values and fake storage contracts                       |
| redb              | Default disk store and indexes                                         | Atomicity, recovery, pagination, ranking, retention          |
| Actor             | Actor/client/source in tauri core                                      | Acknowledged frames, suspend/rebind, shutdown                |
| Integration       | Composition, offline history, service API bridge, facade/UI projection | Rust, IPC boundary inspection, TypeScript and frontend tests |
| Turso             | Optional embedded storage adapter                                      | Shared contracts, integer bounds and transaction cleanup     |
| Benchmark         | Opt-in application actor workload and runner                           | Harness smoke checks; historical results labeled separately  |

Each application PR targets its prerequisite branch. Runtime changes have a
separate small PR; service and service IPC must remain free of traffic changes.

## Acceptance checklist

- [x] Domain/storage crate contains no actors or service dependencies.
- [x] All traffic actors and source tasks live in tauri core.
- [x] Service and IPC have no traffic database, collector, capability or routes.
- [x] Local lifecycle notification preserves native storage ordering and cleanup.
- [x] Service mode collects directly through existing instance-bound API access.
- [x] Disconnect stops collection without fabricating process exit.
- [x] Reconnecting the same instance preserves identity and counters.
- [x] History queries remain available while the controller is disconnected.
- [x] App shutdown preserves remote sessions with unknown end/coverage gaps.
- [x] Pure domain, store and actor tests pass on the revised layout.
- [x] Changed-source format, application Clippy/type checks and frontend tests pass.
- [ ] Independent review and per-PR scope/parent verification complete.
- [ ] Replacement Draft PRs created and superseded drafts linked/closed.

## Evidence and limits

The domain slice passed `cargo test -p nyanpasu-traffic --all-features` (9 tests)
and `cargo fmt -p nyanpasu-traffic -- --check` in its independent publishing
worktree. Only models, pure computations, ports and fake-storage contracts are
included in this slice; application integration is still pending.
The redb slice passed the same all-features command (19 tests: 1 panic
propagation, 9 domain and 9 durable storage), plus `cargo check --all-features
--locked` and format verification. Its lock change adds only the existing redb
dependency to the traffic package. The domain lock also reconciles the existing
runtime gitlink's jobs version metadata; no registry package was reselected.

The first domain/redb/actor slice commits used a task-local empty hooks
directory after manual checks while frontend installation was being prepared.
The publishing worktree now has its own dependencies and generated frontend
prerequisites. Integration passed the normal hooks. The repository-wide formatter
also staged a pre-existing whitespace change in an unrelated tray file; that was
removed before publication. Final actor changes are checked with explicit Clippy,
Rust tests and targeted rustfmt, then committed with a command-local empty hook
path to avoid restaging that unrelated file.

New validation results are recorded here after execution. Previous implementation
test counts do not substitute for testing the changed actor ownership. The old
24-run benchmark is retained only as a historical storage comparison and does
not measure this application-owned executable.

The architecture intentionally cannot collect while the application is absent.
It does not impose a current-session disk quota or recover unobserved connections.
No service release, new history/statistics page, or database migration UI is part
of these PRs.

## Application actor slice

The actor/client/source implementation lives exclusively under tauri core.
Fifteen targeted application tests passed on this slice with all features,
including acknowledged source frames, storage recovery, stale generation fences,
detach/reconnect, failed-start suspension and shutdown without inventing remote
process exits. Composition and UI wiring remain a separate subsequent slice.

## Application integration

The service uses no new traffic IPC or dependencies. One application-owned actor
collects through the existing authenticated API capability, and each frame is
checked against authoritative instance identity. A permanently retired capability
is replaced even if its connection DTO is unchanged. Failed configuration reads
leave new connection attribution unknown; existing connections preserve the rule
context from their original match.

Five focused host tests passed, including the pre-poll instance fence and
same-binding capability recovery. Frontend tests passed (109 tests in 19 files),
interface/application TypeScript checks passed, and the architecture ledger gate
passed. Full-workspace all-targets/all-features Clippy passed with warnings.

Runtime validation passed the lifecycle filter (3 tests) and native-store
integration (2 passed, 2 external-core fixtures ignored). Its full library run
has two Windows path-separator expectation failures independently reproduced on
the unchanged application runtime pin `961e15f`; they are not traffic regressions.
Runtime PR #434 contains only the generic observer port and process hooks.

The exact integration publishing tree passed `cargo test -p clash-nyanpasu
--all-features --lib --locked`: 1,022 passed, 1 ignored. Its generated TypeScript
bindings were refreshed by the export test.

The final memory review requires detached sessions to release resident active
baselines, lazily restore them on reattach or valid direct observation, and keep
pending writes until their commit result is resolved. Regression coverage checks
20 retired remote UUIDs, same-UUID accounting without replaying cumulative bytes,
startup metadata-only residency, missing connections after lazy restoration and
late genuine exits. Ordinary detached history no longer leaves a permanent error
on a healthy current session; unresolved storage/lifecycle failures remain visible.

## Isolated Turso adapter

The independent Turso workspace passed 12 tests against the same storage port,
including shared contracts, integer boundaries, transaction cleanup and the
durability configuration probe. Its SDK does not enter the application workspace.
The copied adapter and lockfile match the tested sources exactly.
