# Core console log migration tasks

See [design.md](design.md) for contracts and review order.

## Preparation

- [x] Commit and open the existing log viewer fix as the first app PR
      ([#5559](https://github.com/libnyanpasu/clash-nyanpasu/pull/5559)).
- [x] Record the design and implementation/verification tasks before delegation.
- [x] Create runtime and stacked application branches using existing build caches.

## Runtime (sol subagent)

- [x] Move console log types from metadata into `nyanpasu-logging`; update callers
      and verify unchanged wire representations and dependency direction.
- [x] Add a Core JSONL filesystem adapter and decoding/index support, including
      gap records, truncation, incomplete tails, rotation and retained file selection.
- [x] Add typed Core file/session IPC operations, client helpers, capability and
      server composition/shutdown wiring using the existing query owner.
- [x] Test file boundaries, paging, filtering and source/owner isolation; run
      relevant runtime Rust checks and prepare an atomic runtime commit.

Runtime: [PR #438](https://github.com/libnyanpasu/nyanpasu-runtime/pull/438),
commit `f8e96828c6dde531d15f826e9678e564106d4612`. Relevant tests, Rustfmt and
Clippy passed. The Unix IPC roundtrip cases skip in this environment because
`/var/run` is not writable; privileged service and real-core cases retain their
existing environment requirements. Runtime PR CI passed on macOS, Linux and
Windows, including IPC and real-core tests.

## Application backend (sol subagent)

- [x] Inject local/service Core archive dependencies and extend shared log sources.
- [x] Keep unified RPC transport capabilities and owner/session isolation intact.
- [x] Remove CoreLogsActor/redb and the kernel `/logs` capture leg, their facade
      methods/events and replaced fixtures. Preserve other Clash streams.
- [x] Update boundary tests and real HTTP fixtures for file-backed Core logs.
- [x] Regenerate Specta bindings and validate backend checks with the runtime pin.

Backend validation: all-feature library compilation and 25 selected checks passed,
including source/owner isolation, host handoff, stopped archives, remaining Clash
streams, RPC registration, binding export and the real HTTP browser fixture. The
HTTP fixture verifies raw stderr JSON, independent displays and retained archives.

## Frontend (sol subagent)

- [x] Route Core to the shared file viewer using actual execution-host status.
- [x] Preserve MDY selection, upward pagination, filters, inspection/copy and
      follow-latest; define clear as display-only and dispose sessions on host change.
- [x] Remove obsolete Core preview hooks/state/types and update tests/perf fixtures.
- [x] Verify browser behavior, TypeScript, style/lint and production build.

Frontend validation: 34 affected tests, full TypeScript checks, Oxlint, formatting,
production build and two performance-fixture smoke cases passed. Full-suite runs
had browser timing failures (320/321 and 318/321); focused reruns passed. These are
not claimed as clean full-suite or performance results. Follow-up reproduction
found an anchor race when a final page completed before deferred rows rendered;
both viewers now wait for the current rows before restoring the anchor. The
first PR includes that correction and its existing five regression tests pass.

## Integration and delivery

- [x] Review all agent changes against the design and current development standards.
- [x] Update developer documentation and this task list with final checks/limits.
- [x] Publish runtime PR and pin its tested commit in the application change.

Publication follows this implementation commit: open the second app PR with
`fix/core-log-viewer` as its base, add dependency links and validation limits to
all PR descriptions, and deliver the URLs without merging.
