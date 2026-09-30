# Implementation and PR split

The user superseded the original service-owned drafts with application-only
traffic ownership. Offline coverage gaps are explicitly accepted. The design in
`design.md` is authoritative; earlier daemon collection and traffic IPC plans no
longer apply.

## Review slices

| Slice | Contents | Verification |
| --- | --- | --- |
| Runtime lifecycle | Generic ordered Started/Exited notification port | Process lifecycle and native-store coexistence |
| Domain | Application-local models, pure accounting/topology, ports | Pure values and fake storage contracts |
| redb | Default disk store and indexes | Atomicity, recovery, pagination, ranking, retention |
| Actor | Actor/client/source in tauri core | Acknowledged frames, suspend/rebind, shutdown |
| Integration | Composition, offline history, service API bridge, facade/UI projection | Rust, IPC boundary inspection, TypeScript and frontend tests |
| Turso | Optional embedded storage adapter | Shared contracts, integer bounds and transaction cleanup |
| Benchmark | Opt-in application actor workload and runner | Harness smoke checks; historical results labeled separately |

Each application PR targets its prerequisite branch. Runtime changes have a
separate small PR; service and service IPC must remain free of traffic changes.

## Acceptance checklist

- [x] Domain/storage crate contains no actors or service dependencies.
- [ ] All traffic actors and source tasks live in tauri core.
- [ ] Service and IPC have no traffic database, collector, capability or routes.
- [ ] Local lifecycle notification preserves native storage ordering and cleanup.
- [ ] Service mode collects directly through existing instance-bound API access.
- [ ] Disconnect stops collection without fabricating process exit.
- [ ] Reconnecting the same instance preserves identity and counters.
- [ ] History queries remain available while the controller is disconnected.
- [ ] App shutdown preserves remote sessions with unknown end/coverage gaps.
- [ ] Pure domain, store and actor tests pass on the revised layout.
- [ ] Application Clippy/format/type checks and relevant frontend tests pass.
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

The repository hook requires a complete frontend installation, so commits use a
task-local empty hooks directory after these manual Rust checks.

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
