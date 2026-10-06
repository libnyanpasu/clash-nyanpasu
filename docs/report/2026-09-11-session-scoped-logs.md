# Session-scoped Logs implementation and validation

Date: 2026-09-11. Main baseline: `705650033c13ae345b058c1a29a81b80017caf73`.
Runtime baseline: `3d99071438ed133d0fdfb7f3db46c040870c4a3d`.

The implementation provides Kernel, Application and Service sources in Logs.
Application and service file indexes live in their respective processes, behind
injected typed clients. The existing kernel stream keeps its existing lifecycle.
Opening the viewer does not install or start the privileged service.

## Delivery and merge order

| Step | Repository       | Change                                                      | Dependency   |
| ---- | ---------------- | ----------------------------------------------------------- | ------------ |
| P0   | clash-nyanpasu   | Archived baseline, allocation and string benchmarks         | main         |
| P1   | nyanpasu-runtime | Owned physical-offset index and local lasso interner        | runtime main |
| P2   | nyanpasu-runtime | File adapter, leases, bounded workers and history           | P1           |
| P4   | nyanpasu-runtime | Service catalog/open/query/close RPC and capability         | P2           |
| P3   | clash-nyanpasu   | Injected app actor, service port, facade and Tauri commands | P0 + P4      |
| P5   | clash-nyanpasu   | File viewer, source selection and route session lifecycle   | P3           |
| P6   | clash-nyanpasu   | Production benchmark and this acceptance report             | P5           |

The remaining main stack is rebased onto `dae9de0e6`, which includes P0. Every
integration branch pins the published runtime tag `v2.0.0-rc.7`, commit
`028d3a02dad640bccefe4e1cd00467d725572d51`. This release includes the logging and
gxhash runtime stack. Both Cargo lockfiles are updated; the regenerated IPC
bindings are unchanged. The previous runtime publication/tag gate is satisfied.

The published Windows x86_64 service archive was downloaded and its SHA-256
verified against the release attachment. Running the extracted binary with
`--version` reports `v2.0.0-rc.7` and the pinned commit. The archive hash is
`911bdda56ac75d47e5559168c57d1ad60733b17ac0f5c3a0d38978d760b0f723`.
This verifies asset availability and binary identity without installing or
starting the privileged service. Main facade/binding export, both frontend type
checks, four cache tests and eleven benchmark tests passed after the rebase.

## Implemented behavior

- Normal owned entries and compact target IDs replace arena-owned `String`/`Vec`
  values and raw entry pointers. The interner is index-local and reclaimed with it.
- Physical file offsets preserve duplicate or backwards timestamps. Time filters
  test each candidate; malformed complete records remain visible as unparsed rows.
  Incomplete lines wait for a newline; oversized lines advance to the next boundary.
- A 45-second renewable lease handles crashed or suspended viewers. Closing the
  last viewer starts a 30-second idle grace. Multiple viewers share file slots.
  After the grace, indexes and pools are dropped; file handles are scoped to reads.
- One bounded refresh runs per file. Active queries refresh at approximately one
  second; cold builds use bounded continuation batches. No new file scan starts
  without an active session. An already-running bounded read may finish after close.
- Retained live windows are limited to 32 MiB / 250,000 entries. Earlier history
  uses a temporary index over at most 1 MiB per request, preserving the live window.
  Each process allows four file slots and sixteen sessions. Unique target bytes
  are capped at 8 MiB; target length at 4 KiB and complete lines at 256 KiB.
- Responses contain at most 200 rows and 1 MiB serialized data. Raw/message fields
  are individually truncated at a UTF-8 boundary when needed, including expansion
  from invalid UTF-8. Continuation cursors never skip unconsumed budget-limited rows.
- Rotation, truncation and file identity changes reset cursor generations. File
  names are catalog entries within an injected directory; symlinks are rejected.
  Arbitrary in-place rewriting with the same prefix and nondecreasing size is not
  a supported producer behavior. Selecting a historical file never changes the
  source's logging permissions, retention or writer settings.
- The service advertises an optional capability. Old/unavailable services degrade
  only that source. Successful log query routes bypass HTTP request tracing and
  do not send results through the global event stream.
- GUI queries support levels, exact target, local-time range and literal text.
  History and partial search coverage are explicit. The virtual list supports
  raw-record expansion/copy, follow-latest, unseen counts and history scroll anchors.
  Clear-current-display uses a local physical cursor floor; it does not delete files.
  The client cache is limited to 5,000 rows / an estimated 4 MiB of retained strings
  and metadata, independently of backend index bounds.

## Measurements

Environment: Windows x86_64, i9-14900KF, rustc
`1.100.0-nightly (67eda617e 2026-09-10)`, release, System allocator.
Commands and raw results are in `tools/log-index-bench`.

| Measurement                                      |                                            Result | Scope                                    |
| ------------------------------------------------ | ------------------------------------------------: | ---------------------------------------- |
| Cold first page, 100,000 rows / 14,558,836 bytes |                                         120.53 ms | Real file adapter + actor                |
| 1,000 appended rows/s for 60 s                   | 60,000 received, no duplicate IDs or missing rows | 301 queries                              |
| Append-to-query latency p50 / p95 / max          |                            558 / 1,045 / 1,112 ms | One 60-second run, excluding GUI and IPC |
| Retained production index, 100,000 rows          |                                   7,345,692 bytes | Requested live heap bytes                |
| Peak allocation during that build                |                                   7,347,074 bytes | Allocator counter                        |
| 100 production index build/query/drop cycles     |                                0 unreleased bytes | 1,000 rows per cycle                     |

The final index allocation measurement was repeated after the history pagination
change and returned identical values. The latency workload preceded that change;
it uses only Latest/After, whose execution path is unchanged by historical reads.
This is evidence for second-level file-to-actor updates, not an end-to-end native
GUI latency measurement or an exact process RSS cap. Allocators may retain pages
after the owning Rust objects are dropped.

P0 compares both complete layouts and controlled string representations. Owned
collections do not inherently regress indexing performance; lasso combines compact
IDs with competitive repeated-target performance and better high-cardinality build
time than the measured hand-written Arc pool. SmolStr is retained as a measured
alternative, not treated as string interning. See the benchmark README for samples
and the limits of comparing the old, semantically incorrect query algorithm.

## Completed validation

- Shared logging: 12 all-feature tests and all-target/all-feature clippy with
  warnings denied. Includes sequential query oracle, duplicate/backwards timestamps,
  partial UTF-8, malformed/oversized rows, pool reclamation, invalid cursors,
  historical paging and budget continuation.
- Session lifecycle: injected clock tests cover shared leases, expiry, reopening
  during grace, owner isolation and 100 open/close/reclaim cycles. No sleeps are
  used to drive lifecycle assertions.
- Runtime: 57 IPC unit/golden/round-trip tests, including a real Windows named-pipe
  round trip for the four new operations; 106 service runtime tests. Routing tests
  include actual JSON log files and session owner rejection.
- Main: the app-file facade test and service degradation test, generated TypeScript
  binding export, all-target/all-feature clippy, and architecture-ledger gate.
  Existing unrelated Rust warnings remain.
- Frontend: interface/app TypeScript checks, affected-file oxlint, interface build,
  production web build, and four cache/cursor tests in
  `scripts/log-viewer-state.test.mjs`.
- Browser: real FileLogs component and hook under React StrictMode with a local
  mocked Tauri transport. Checked delayed open followed by source switch, late
  session cleanup, route exit, clear followed by append, old service capability,
  empty filtered results, history load and 390px layout. Exit left zero mock
  sessions; the final error probe recorded no errors. Desktop and narrow-screen
  screenshots were inspected. This harness does not simulate service permissions.
- Frozen benchmark: 11 tests; controlled string and full-layout measurements remain
  archived separately from production actor measurements.

## Release acceptance still required

1. Verify remaining platform sidecar preparation during CI/package acceptance.
   Runtime publication, the release tag pin and the Windows archive identity
   verification are complete.
2. Run the native packaged application with the matching elevated Windows service:
   query app/service logs, rotate files, suspend/resume the UI, close/reopen viewers,
   and confirm service log polling does not generate a feedback stream.
3. Run platform CI and Unix service transport checks. The local execution environment
   was Windows; Unix filesystem/IPC behavior was not executed here.
4. Measure combined native GUI + app + service RSS/heap after repeated viewer cycles
   and end-to-end render latency under representative user logs. Current evidence
   covers Rust ownership release, actor slot reclamation and a bounded client cache,
   not combined process memory or native rendering p95.

These release checks are explicit merge gates on the draft main stack, not claims
that the native acceptance suite already passed.

## Requested gxhash migration

The follow-up runtime PR switches the target interner, postings, session/slot maps
and lease-reaping map to randomized `GxBuildHasher`. The main follow-up updates the
gitlink and benchmarks. Runtime build flags cover all eight release triples, and
the coverage job preserves AES when overriding `RUSTFLAGS`. This establishes a
hardware-AES requirement for the service as already configured in the main app.

After this task's compilation jobs finished, a fresh 60-second measurement received
all 60,000 appended rows. Cold first page: 135.10 ms; p50/p95/max file-to-actor
latency: 710/1,181/1,461 ms. This remains second-level delivery; separate wall-clock
runs do not isolate the hasher's contribution from machine scheduling and IO.
Production index memory and the zero-unreleased-byte cycle result were unchanged.

The gxhash follow-up passed the same 12 shared logging tests, strict logging clippy,
57 IPC tests, 106 service runtime tests and main app facade test on Windows.
Original FxHash measurements above remain historical evidence; current gxhash
samples use separate `gxhash-` filenames under `tools/log-index-bench/results`.
Native release acceptance and non-Windows execution remain gated as described above.
