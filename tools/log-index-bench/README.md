# Log index storage benchmark

This standalone workspace preserves the pre-session index implementation and its
defect probes, and benchmarks the replacement shared index. It does not compile Tauri. The four
`reproduces_*` tests assert known old behavior, not the desired replacement behavior.
The replacement must have separate regression tests asserting the correct result.

## Reproduce

Run from the repository root with the pinned dependencies in Cargo.lock:

```sh
cargo test --release --manifest-path tools/log-index-bench/Cargo.toml --lib
cargo run --release --manifest-path tools/log-index-bench/Cargo.toml --bin allocation_bench
cargo run --release --features alloc-stats --manifest-path tools/log-index-bench/Cargo.toml --bin allocation_bench
cargo run --release --manifest-path tools/log-index-bench/Cargo.toml --bin strings
cargo run --release --features alloc-stats --manifest-path tools/log-index-bench/Cargo.toml --bin strings
cargo run --release --manifest-path tools/log-index-bench/Cargo.toml --bin viewer -- 60
cargo run --release --features alloc-stats --manifest-path tools/log-index-bench/Cargo.toml --bin viewer
```

Pass `smoke` after the binary name for a smaller fixture. Allocation counters run
in a separate build; do not compare their elapsed times with normal timing runs.
`allocation_bench` pins its Windows thread to logical CPU 0; `strings` uses normal
scheduler placement. Do not directly compare elapsed times across these binaries.
Both use one warm-up and nine interleaved measured rounds. Raw samples are retained
for the string comparison; medians are useful here, nine rounds are insufficient
for a reliable production p95 latency estimate.

`reference-indexer.rs` is the frozen source. `generate_allocation_bench.py` generates
the layout comparison from that source without reading a developer's checkout.
The unsafe baseline is for measurement only: its explicit Drop assumes unique line
IDs, which the benchmark guarantees. It is not suitable as a production fix.

## Findings and selection (2026-09-11)

Environment: Windows x86_64, Intel i9-14900KF, rustc
`1.100.0-nightly (67eda617e 2026-09-10)`, release, codegen-units=1, System allocator.
The earlier full-layout results and methodology are in `results/allocation-report.md`.
The archived comparison fixes the row metadata, Vec layout, query algorithm and FxHasher
across interners. The current `strings` binary uses GxBuildHasher; its results are
archived separately with a `gxhash-` prefix. String and SmolStr store their values directly; their larger row
layout is an inherent part of that representation. Inputs are borrowed and prepared
outside timing, so this comparison excludes JSON parsing, filesystem, IPC and GUI.

| Target workload               | String build | SmolStr build | std Arc + ID |     lasso | string-interner |
| ----------------------------- | -----------: | ------------: | -----------: | --------: | --------------: |
| 100k rows, 32 targets, 23 B   |     3.546 ms |      2.077 ms |     1.694 ms |  1.700 ms |        1.721 ms |
| 100k rows, 32 targets, 24 B   |     3.539 ms |      4.268 ms |     1.499 ms |  1.531 ms |        1.573 ms |
| 100k rows, 50k targets, 128 B |     6.579 ms |      7.172 ms |     7.281 ms |  5.212 ms |        6.122 ms |
| 1m rows, 128 targets, 64 B    |    48.042 ms |     56.415 ms |    15.546 ms | 15.895 ms |       15.920 ms |

For the last workload, retained allocations including the appended 1,000 rows and
query output are approximately 117.10 MiB (String), 132.37 MiB (SmolStr), and
32.02 MiB (each interner). All measured string pools return to their pre-build
requested-byte baseline after rows, pool and query results are dropped. The archived
seven-layout suite additionally verifies 100 build/append/query/drop cycles per
variant. These are allocator-requested bytes, not process RSS or proof that Windows
immediately releases allocator pages.

Select **owned entries with compact IDs and an index-local lasso Rodeo** for the
next stage: normal actor-safe ownership, low allocation count, comparable repeated
target performance, and a measurable high-cardinality benefit over the hand-written
Arc pool. Do not use SmolStr as an interner. Do not introduce a global string pool.
Keep Bump as measured evidence, not a required part of the replacement. The full
layout comparison already shows that standard owned collections need not regress
build/append throughput; final query performance must be measured after fixing its
algorithm and timestamp correctness.

## Implementation limits to carry into P1/P2

- Maximum retained file window: 32 MiB; maximum indexed entries: 250,000.
- Maximum complete line: 256 KiB; maximum read/parse batch: 1 MiB.
- Maximum target: 4 KiB; cumulative unique target bytes: 8 MiB.
- Maximum response: 1 MiB / 200 rows; search text: 256 Unicode scalars.
- At most four retained file indexes and sixteen sessions per process.
- One in-flight refresh per file; query/scan continuations expose partial coverage.

These limits bound inputs and retained structures rather than claiming an exact RSS
cap. The final actor measurements and remaining native acceptance checks are in
`../../docs/superpowers/reports/2026-09-11-session-scoped-logs.md`.
The workload used for algorithm correctness must include duplicate timestamps,
clock rollback, escaped JSON, malformed records and incomplete trailing lines;
the frozen full-layout comparison intentionally does not repair its old query
semantics and cannot substitute for those tests.

## Production actor measurements

Initialize the runtime submodule before running this workspace. `viewer` writes a
100,000-row JSON file, opens the real filesystem adapter and LogsClient, then
appends 1,000 rows per second. It polls every second and drains continuation pages
immediately. Each appended row must appear exactly once. Latency runs include JSON
parsing, filesystem reads, actor messages and owned response projection; they
exclude Tauri, service IPC and GUI rendering. No allocator instrumentation runs
in this mode. It uses normal scheduler placement and one measured 60-second run.

The separate `alloc-stats` mode counts allocations for the production pure Index,
including parsing, then checks 100 build/query/drop cycles. It measures requested
heap bytes, not RSS or actor/GUI heap size. See the JSONL files under `results/`.

## gxhash follow-up

The production index, lasso target pool and session tables now use gxhash 3.5.0.
Interners in `strings` use the same randomized
[GxBuildHasher](https://docs.rs/gxhash/3.5.0/gxhash/struct.GxBuildHasher.html).
The frozen `reference-indexer`, defect probes and `allocation_bench` keep their
original hashers so the archived baseline remains reproducible. The root Cargo
configuration already enables hardware AES; the standalone runtime now does too,
including its musl targets and coverage environment override.

The `gxhash-strings-timing.jsonl` run uses the same warm-up and nine measured rounds
after this task's compilation jobs completed. Lasso's median build time is
5.326 ms for 100k rows / 50k targets / 128 B, and 16.753 ms for 1m rows / 128 targets /
64 B. These are separate wall-clock runs from the archived FxHash measurements,
not a paired experiment establishing a causal performance difference.

Production index memory remains 7,345,692 live requested bytes for 100k rows and
zero unreleased bytes after 100 build/query/drop cycles. The `gxhash-viewer-*.jsonl`
files contain the repeated production measurements; their scope remains file to
actor, excluding native IPC/GUI rendering and process RSS.
