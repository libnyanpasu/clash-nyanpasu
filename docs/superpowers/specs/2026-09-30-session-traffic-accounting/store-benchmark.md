# Historical redb / embedded Turso store comparison

> Historical measurement, before the application-only ownership revision. The
> tables below describe the executable and source hashes recorded in the original
> experiment, not the revised application or current benchmark executable.
> The old runtime/application drafts are superseded. The current actor source
> lives only in `backend/tauri/src/core/traffic`; the optional Turso adapter and
> benchmark are isolated evaluation crates. This avoids Turso SDK's
> `parking_lot/send_guard` feature conflicting with the application's existing
> `parking_lot/deadlock_detection` feature. It is not a service implementation.

Original experiment: complete. Storage validation and all 24 fresh-process release runs passed. The default production store remains redb. After measurement, the implementation was committed and submitted for draft review at the user's request; runtime commit `4218c60` is in [PR #433](https://github.com/libnyanpasu/nyanpasu-runtime/pull/433). No release or default-store switch is included.

The current harness and build instructions live in
[`tools/traffic-store-benchmark`](../../../../tools/traffic-store-benchmark/README.md).
The commands and source hashes below reproduce the **original experiment**;
they do not describe the replacement application's dependency graph.

## Scope and implementation identity

This compares two complete `TrafficStore` adapters through the same traffic actor and deterministic fixture. It does not isolate engine syscalls, and does not measure the upper bound of a normalized SQL implementation. Turso is the local Rust database engine (`turso = "=0.8.1"`, `default-features = false`), not libSQL, an embedded cloud replica, or Turso Cloud. redb is locked at 4.2.0. The new adapter is behind the optional `turso-store` feature; all trait methods are implemented. Existing redb databases cannot be opened as Turso files; no migration tool is supplied.

The baseline code was uncommitted, so HEAD alone does not identify its implementation. SHA-256 before this experiment:

| Source                          | SHA-256                                                            |
| ------------------------------- | ------------------------------------------------------------------ |
| `src/adapters/redb.rs`          | `48CD71FC22672714C1896CBAEACAC5B4B27DD5631497B13503B1885AFA817A5C` |
| original `tests/performance.rs` | `D7797BABE6944B048A0C7F98B3AAB6DA5EF91080584BD785CE6A261B300E21B7` |

Frozen measurement code:

| Source                                     | SHA-256                                                            |
| ------------------------------------------ | ------------------------------------------------------------------ |
| `src/adapters/redb.rs` (unchanged)         | `48CD71FC22672714C1896CBAEACAC5B4B27DD5631497B13503B1885AFA817A5C` |
| `src/adapters/turso.rs`                    | `7F8E929F0E1D4381DF53D5C12179B248231DA1294E01FBFB993A9FBA57090C50` |
| updated four-thread `tests/performance.rs` | `0EBEC78DF0A49462A3DC7C35A28431BE5CB0CB0CA511C3135477FBDB8C5F83D6` |
| `tests/performance_support/mod.rs`         | `D2826BE4161B291CDABB98BF2090424A9A898A31D79178A2A2DA6A8F50BED515` |

## Persistence and schema

Both adapters durably commit the whole observation, including the session, changed connection rows, closed rows, attribution facts, filter indexes, all group totals, top500 caches, path members/counts, and the latest replay receipt. No history is omitted, and no fake store or whole-database serialization is used. Values remain the same JSON evidence/domain records, reducing serialization differences. Facts merge by session/connection/segment/minute; only the current replay receipt is retained, as in redb.

Turso uses ten separate SQL tables with composite TEXT primary keys: sessions (session), connections (session/id), ordering (session/first sequence/id), facts (session/id/segment/minute), members (session/path/id), path_counts (session/path), totals (session/group/key), rankings (session/group), filters (session/kind/key/first sequence/id), and receipts (session/sequence). The primary-key indexes support prefix scans, ordering, active-only queries, and rule/other filters. Historical filter keys survive metadata changes; only lifecycle keys are replaced. Queries test remaining constraints against records/facts, matching redb's logic. Recovery still scans archived connections in each unfinished session, matching the baseline's startup limitation.

Sequence, segment, and minute keys use fixed 20-digit TEXT, preserving the entire unsigned u64 range and lexicographic order. Persisted UInt values retain decimal-string JSON encoding. Arithmetic, overflow checks, and ranking weights run in Rust; upload + download uses u128 without SQL INTEGER narrowing. The seven group rankings persist only their best 500 entries, while complete totals and facts persist separately. Results conserve listed groups + other = total. Topology members deduplicate each connection/path pair.

These are ordinary rowid tables: Turso stores rows and their composite primary-key indexes separately. This duplicates key storage relative to a single B-tree keyed table and is a material adapter/schema difference. `WITHOUT ROWID` requires an experimental builder option in 0.8.1; this experiment leaves experimental features off. SQL-native dimension columns, composite filter intersections, aggregate arithmetic, and alternative normalized ranking indexes were not evaluated.

The SQL adapter uses `INSERT OR REPLACE`, which can delete and reinsert a row and its primary-key index entries, rather than `ON CONFLICT DO UPDATE`. It also formats TEXT keys and executes cached SQL statements for individual logical operations. Those VM, allocation, rowid/index, and replacement costs belong to this adapter comparison; they must not all be attributed to the engine alone. Alternative upsert strategies are a separate future experiment, not an unmeasured optimization claim.

redb uses a 32 MiB page-cache budget and `Durability::Immediate` on every write. Turso uses one real database connection, `BEGIN IMMEDIATE`, WAL mode, `PRAGMA synchronous = FULL`, and `PRAGMA cache_size = -32768`. Open reads back WAL/FULL and refuses unsupported settings. The settings read from the connection executing actual adapter commands are `("wal", 2, -32768)`. A store-level connection lock protects transactions; it is not an actor queue or scheduler. Error paths explicitly roll back. Commit errors preserve UnknownOutcome; deferred SDK transaction cleanup runs before cached reads, and a connection whose cleanup fails refuses further use.

The [official PRAGMA reference](https://github.com/tursodatabase/turso/blob/v0.8.1/docs/sql-reference/pragmas.mdx) documents WAL and FULL/OFF support. The [0.8.1 Windows VFS](https://github.com/tursodatabase/turso/blob/v0.8.1/core/io/windows.rs) implements synchronization with `FlushFileBuffers`. The [August first-commit fsync issue](https://github.com/tursodatabase/turso/issues/8194) is closed; inspection of the pinned [0.8.1 pager](https://github.com/tursodatabase/turso/blob/v0.8.1/core/storage/pager.rs) found its WaitSync check includes nonempty prepared frames as well as WAL dirty state. This is source/API evidence and clean reopen evidence, not a power-loss fault test or a proof of equal crash guarantees. No synchronous=OFF or reduced durability run is substituted.

## Validation

`cargo test -p nyanpasu-traffic --all-features --tests` passed 44 tests, with the explicit benchmark ignored. Tests include the shared storage contract on redb/Turso; atomic rollback after record writes; replay/digest/predecessor conflicts; clean reopen and raw JSON; close/recovery; high-watermark pagination and historic filters; minute allocation/unallocated bytes; group indexes and bounded ranking promotion/ties/other (explicit query assertions primarily Rule/Target); unique topology membership; pruning; full-u64 byte values and u128 ranking weights; key ordering at 0/i64::MAX/u64::MAX; and the SDK deferred-rollback cleanup boundary. The actual Windows local probe asserts WAL/FULL/cache, rolls back an insert, commits another, reopens, verifies u64::MAX TEXT, and checkpoints. Strict all-features/all-targets Clippy passed.

Final `cargo test -p nyanpasu-traffic --no-default-features --features turso-store --tests` passed 33 tests (benchmark ignored), proving the adapter and its tests compile without redb. Final format check and strict Clippy passed. The source hashes above remained unchanged after these checks. Root independently audited the 24 raw reports: three rounds per store/case, frame and commit counts, active counts, page/rule/rare row counts, group/path counts, release/cache settings, and Turso WAL/FULL all match the fixture. This audits reported invariants, not every persisted record.

## Reproduction and method

Machine: Intel Core i9-14900KF (24 cores / 32 logical processors), Windows 11 Pro build 26200 / x86_64 MSVC; rustc `1.101.0-nightly (c1070d693 2026-09-28)`. G: is ReFS on a ZHITAI TiPro7000 1TB NVMe. The runtime workspace release profile is `opt-level = "s"`, `lto = true`, `codegen-units = 1`, `panic = "unwind"`. Target configuration enables AES/SSE2. Turso defaults, including mimalloc/FTS, are disabled; both runs use the same process allocator and compiled executable. Initial release compilation took 4m52s; rebuilding the four-thread harness took 1m35s. Neither duration is included in measurement.

The 0.8.1 SDK Windows build needs `rc.exe`; this machine has LLVM's `llvm-rc.exe`, with no default Windows Kits 10/bin installation. A task-local file symlink named `rc.exe` points to that real resource compiler; its containing directory was added to PATH, without changing dependency source. Its normal `/nologo`, `/fo...`, VERSIONINFO input compiled successfully. Turso's aegis dependency requires cc >= 1.4.7; the lock selects cc 1.5.1 and its required find-msvc-tools 0.1.14. ICU 2.3 dependencies also arise from Turso. The main app's default path metadata resolves without enabling Turso; its lock need not include this unused optional path-dependency feature.

Compile separately, then select the performance compiler-artifact executable from the JSON log:

```powershell
$env:PATH = "G:/Programs/Rust/.ccg/clash-nyanpasu/traffic-store-benchmark-tools;$env:PATH"
cargo test -p nyanpasu-traffic --release --features turso-store --test performance --no-run --message-format=json > traffic-store-release-build.jsonl
# From the runtime workspace, using that compiled artifact:
./crates/nyanpasu-traffic/tests/run_store_benchmark.ps1 -Executable <exe> -OutputRoot <new-results-directory> -TempRoot <dedicated-G-temp-directory> -Rounds 3
```

Each case/store/round runs a fresh child process with a fresh database on G:, using a fixed four-thread Tokio runtime for both stores. The order alternates by case and round; compilation does not overlap timed runs. This isolates the actor's serialization while allowing caller wakeups on other workers, but is not the full application's concurrent load. OS filesystem caches are not flushed, so these are fresh-database observations, not guaranteed cold-cache IO. Existing older performance files used system TEMP on C:, so no cross-drive comparison or engine-speed claim is derived from those files.

An initial single-thread harness run was stopped to correct a scheduling confound: on Windows Turso SQL/IO could occupy the runtime after the actor had replied to a query, preventing its caller from waking until the subsequently queued observation finished. That inflated query-first caller timings without corresponding independent-query latency. The six completed single-thread case/store results and partial million-closed log are diagnostic only, retained at `G:/Programs/Rust/.ccg/clash-nyanpasu/traffic-store-benchmark-results-20260930`; they are excluded from the formal comparison. The incomplete old single-thread Turso million-closed database remains at `G:/Programs/Rust/.ccg/clash-nyanpasu/traffic-store-benchmark-temp/traffic-store-YzgiRa`, occupying 1,280,784,656 logical bytes (DB 1,277,579,264 + WAL 3,205,392), about 1.19 GiB. Automatic approval review rejected recursive deletion with the reason `blocked by policy`; the benchmark child was stopped and this directory is preserved rather than deleted. Its files do not contribute to any formal database-size result.

Fixture: active cases ingest 1,000 or 10,000 connections for 21 full frames (100-byte upload / 200-byte download increments). Closed cases ingest 100,000 or 1,000,000 unique connections in batches of 1,000, with an empty close frame after every ingest frame (200 / 2,000 observations). Connection metadata has only a unique host; process/source/protocol are unspecified, and all samples share a fixed `DIRECT` / `benchmark-group` path. Every 10,000th sample uses a rare rule; other samples use Match. This limits the dimensional variety represented by the benchmark.

Each historical connection reports its 100 upload / 200 download bytes only in its creation snapshot and closes in the next frame, so those bytes are time-unallocated; no later interval delta is observed. The 21 active frames fit within one UTC minute. These benchmarks cover Session/Live and the specified filters, not MinuteWindow queries, long connections spanning many minutes, or high-cardinality paths. They do not establish performance for every statistics query.

Observation time covers actor mailbox delay, accounting, digest, adapter reads/writes, indexes, serialization, durability, and publication. The measured wrapper separately records `commit_observation` time, which still includes adapter/index/serialization/IO costs; it is not pure engine time. Mean/p50/p95/max use the same sorted-sample calculation, with floor-index percentiles. Active steady state excludes the first ingest; closed distributions include both ingest and close frames. Query timings use the same actor path. Individual query-first measurements use biased polling to enqueue the query before a full observation; this demonstrates mailbox blocking, without adding a queue. RSS is sampled before ingest, after ingest, and after queries; it is not peak RSS.

File totals enumerate all files in the dedicated database directory, including DB/WAL/shm if present. The first boundary is after query/blocking workloads and before explicit flush/checkpoint. The second is after flush: redb performs an Immediate empty transaction; Turso explicitly checkpoints the WAL, while each prior commit was already FULL. These are logical file lengths, not filesystem allocated blocks. Shutdown/temporary-file deletion occurs afterward; the runner preserves raw JSON/stdout/stderr and refuses overwriting existing result JSON.

## Measured results

Raw results, stdout/stderr, environment/source metadata, and the reproducible summary script are at `G:/Programs/Rust/.ccg/clash-nyanpasu/traffic-store-benchmark-results-mt4-20260930`. The measured executable SHA-256 is `2465298AC0EF467B138185CDD25F3547F4717E44D60DA4E856819A29F5D21D5B`. All 24 cases succeeded. Their dedicated temporary database directory `G:/Programs/Rust/.ccg/clash-nyanpasu/traffic-store-benchmark-temp-mt4` is empty after automatic tempfile cleanup. The separately documented old single-thread diagnostic database remains preserved.

Every cell below is **the median of three per-round statistics [minimum–maximum across those rounds]**. Thus the p50/p95 columns are medians of each round's percentile, not pooled percentiles. The max column likewise summarizes per-round maxima; its range's upper bound is the largest observed value. Each active round has 21 observations/commits, 20 steady observations; closed rounds have 200 or 2,000 observations/commits. Queries are one independent warm measurement per round, and query-first measurements have a separate table. Values are milliseconds unless labeled otherwise.

### Actor observations, all ingest/close frames

| Case           | Store | Elapsed s              | mean ms                | p50 ms                 | p95 ms                 | max ms                    |
| -------------- | ----- | ---------------------- | ---------------------- | ---------------------- | ---------------------- | ------------------------- |
| 1000-active    | redb  | 0.86 [0.86–0.89]       | 40.86 [40.81–42.34]    | 41.06 [41.01–42.36]    | 46.01 [43.87–47.31]    | 61.29 [61.06–62.42]       |
| 1000-active    | turso | 1.81 [1.77–1.89]       | 85.96 [83.95–89.77]    | 81.25 [80.34–85.37]    | 83.78 [83.02–101.55]   | 176.63 [169.68–193.30]    |
| 10000-active   | redb  | 9.95 [9.63–11.33]      | 470.86 [455.64–536.08] | 465.01 [455.69–556.67] | 558.59 [501.87–631.48] | 688.06 [683.01–715.46]    |
| 10000-active   | turso | 19.49 [19.15–19.67]    | 925.58 [908.95–933.88] | 860.78 [858.43–884.70] | 903.61 [881.13–945.70] | 1889.17 [1887.62–2198.88] |
| 100000-closed  | redb  | 11.56 [11.54–13.54]    | 57.68 [57.55–67.52]    | 49.60 [49.51–62.06]    | 72.51 [71.84–94.00]    | 80.79 [77.12–118.22]      |
| 100000-closed  | turso | 35.33 [34.38–39.18]    | 176.51 [171.78–195.68] | 172.31 [167.37–196.23] | 204.03 [197.78–242.01] | 217.14 [216.41–267.29]    |
| 1000000-closed | redb  | 126.73 [126.32–127.97] | 63.25 [63.05–63.87]    | 61.62 [61.60–61.86]    | 80.10 [79.37–80.54]    | 103.06 [101.31–115.34]    |
| 1000000-closed | turso | 454.82 [362.51–466.58] | 227.26 [181.14–233.13] | 204.49 [181.62–205.90] | 400.63 [209.51–406.75] | 439.04 [229.80–1337.87]   |

### Adapter commit_observation, same frames

| Case           | Store | mean ms                | p50 ms                 | p95 ms                 | max ms                    |
| -------------- | ----- | ---------------------- | ---------------------- | ---------------------- | ------------------------- |
| 1000-active    | redb  | 31.13 [31.10–32.44]    | 31.42 [31.40–32.68]    | 35.46 [34.20–37.34]    | 51.85 [51.57–53.07]       |
| 1000-active    | turso | 76.06 [74.10–79.85]    | 71.62 [70.91–75.28]    | 74.35 [73.18–91.07]    | 166.78 [160.26–184.07]    |
| 10000-active   | redb  | 368.29 [353.00–403.25] | 361.76 [350.77–410.08] | 451.99 [395.02–478.53] | 592.93 [589.18–617.10]    |
| 10000-active   | turso | 820.88 [802.95–824.92] | 753.99 [750.86–776.76] | 787.81 [775.64–840.10] | 1791.55 [1789.77–2102.67] |
| 100000-closed  | redb  | 50.90 [50.72–59.23]    | 45.61 [45.61–54.22]    | 63.51 [62.78–81.62]    | 71.57 [67.94–104.88]      |
| 100000-closed  | turso | 167.94 [163.33–185.66] | 167.07 [159.53–184.80] | 192.22 [186.14–226.49] | 204.29 [202.82–252.61]    |
| 1000000-closed | redb  | 56.84 [56.60–57.42]    | 52.37 [51.90–54.31]    | 71.23 [70.60–71.61]    | 92.78 [88.27–106.70]      |
| 1000000-closed | turso | 217.53 [173.34–222.79] | 193.70 [174.23–195.04] | 379.49 [198.58–384.48] | 416.18 [219.27–1315.13]   |

### Active steady observations, first ingest excluded

| Case         | Store | First ingest ms           | mean ms                | p50 ms                 | p95 ms                 | max ms                 |
| ------------ | ----- | ------------------------- | ---------------------- | ---------------------- | ---------------------- | ---------------------- |
| 1000-active  | redb  | 61.29 [61.06–62.42]       | 39.85 [39.78–41.34]    | 40.49 [38.16–42.18]    | 42.92 [42.09–45.42]    | 46.01 [43.87–47.31]    |
| 1000-active  | turso | 176.63 [169.68–193.30]    | 80.59 [79.66–85.43]    | 80.82 [79.76–84.76]    | 83.65 [82.81–96.44]    | 83.78 [83.02–101.55]   |
| 10000-active | redb  | 688.06 [683.01–715.46]    | 460.00 [444.27–527.11] | 462.76 [452.22–517.38] | 505.74 [471.23–625.96] | 558.59 [501.87–631.48] |
| 10000-active | turso | 1889.17 [1887.62–2198.88] | 861.91 [859.94–886.19] | 860.15 [854.95–883.56] | 897.13 [880.69–904.86] | 903.61 [881.13–945.70] |

### Independent warm actor queries: connections

| Case           | Store | Page100 ms          | Common rule page100 ms | Rare rule page100 ms |
| -------------- | ----- | ------------------- | ---------------------- | -------------------- |
| 1000-active    | redb  | 0.490 [0.423–0.493] | 0.389 [0.383–0.416]    | 0.051 [0.049–0.061]  |
| 1000-active    | turso | 1.053 [1.037–1.080] | 0.857 [0.826–0.976]    | 0.114 [0.087–0.121]  |
| 10000-active   | redb  | 0.600 [0.542–1.074] | 0.531 [0.522–0.865]    | 0.148 [0.145–0.175]  |
| 10000-active   | turso | 3.300 [2.923–3.758] | 1.015 [0.913–1.063]    | 0.254 [0.247–0.316]  |
| 100000-closed  | redb  | 0.999 [0.954–1.615] | 0.481 [0.447–0.727]    | 0.146 [0.128–0.383]  |
| 100000-closed  | turso | 1.575 [1.453–1.675] | 0.819 [0.791–0.823]    | 0.330 [0.307–0.360]  |
| 1000000-closed | redb  | 1.263 [1.151–1.310] | 0.529 [0.504–0.559]    | 1.219 [1.167–1.296]  |
| 1000000-closed | turso | 1.572 [1.569–1.667] | 0.880 [0.850–0.888]    | 2.406 [2.389–2.531]  |

### Independent warm actor queries: usage and topology

| Case           | Store | Rule usage ms       | Target top20 ms        | Session topology ms | Live topology ms          |
| -------------- | ----- | ------------------- | ---------------------- | ------------------- | ------------------------- |
| 1000-active    | redb  | 0.536 [0.522–0.567] | 0.945 [0.940–1.070]    | 0.441 [0.437–0.481] | 34.730 [33.940–34.733]    |
| 1000-active    | turso | 0.657 [0.639–0.668] | 1.000 [0.983–1.029]    | 0.605 [0.563–0.684] | 39.254 [38.603–39.892]    |
| 10000-active   | redb  | 6.071 [6.057–7.653] | 10.495 [10.156–14.704] | 7.348 [6.988–7.533] | 389.488 [387.288–422.302] |
| 10000-active   | turso | 6.486 [6.369–6.572] | 10.865 [10.805–11.267] | 7.861 [7.404–8.320] | 416.171 [413.498–427.213] |
| 100000-closed  | redb  | 0.038 [0.036–0.080] | 0.133 [0.128–0.219]    | 0.114 [0.096–0.193] | 0.056 [0.056–0.097]       |
| 100000-closed  | turso | 0.122 [0.105–0.137] | 0.182 [0.177–0.216]    | 0.260 [0.257–0.295] | 0.146 [0.114–0.195]       |
| 1000000-closed | redb  | 0.046 [0.045–0.054] | 0.130 [0.125–0.134]    | 0.112 [0.100–0.117] | 0.060 [0.049–0.061]       |
| 1000000-closed | turso | 0.155 [0.107–0.182] | 0.179 [0.174–0.184]    | 0.301 [0.256–0.308] | 0.149 [0.147–0.150]       |

### Query first: rule_usage

| Case           | Store | Query RPC ms         | Queued observation RPC ms |
| -------------- | ----- | -------------------- | ------------------------- |
| 1000-active    | redb  | 0.757 [0.718–0.814]  | 25.736 [25.397–25.803]    |
| 1000-active    | turso | 0.814 [0.776–0.819]  | 31.960 [31.803–32.184]    |
| 10000-active   | redb  | 8.635 [8.396–10.445] | 253.381 [253.118–373.735] |
| 10000-active   | turso | 9.008 [8.938–9.096]  | 382.370 [377.099–394.935] |
| 100000-closed  | redb  | 0.073 [0.040–0.089]  | 2.329 [2.303–2.545]       |
| 100000-closed  | turso | 0.067 [0.043–0.069]  | 2.287 [2.258–2.369]       |
| 1000000-closed | redb  | 0.042 [0.040–0.047]  | 2.294 [2.282–2.378]       |
| 1000000-closed | turso | 0.057 [0.053–0.065]  | 2.225 [2.150–2.293]       |

### Query first: target_usage

| Case           | Store | Query RPC ms           | Queued observation RPC ms |
| -------------- | ----- | ---------------------- | ------------------------- |
| 1000-active    | redb  | 1.228 [1.220–1.514]    | 21.654 [21.566–22.444]    |
| 1000-active    | turso | 1.345 [1.314–1.489]    | 32.758 [32.288–32.987]    |
| 10000-active   | redb  | 13.810 [13.569–23.133] | 205.725 [203.213–274.941] |
| 10000-active   | turso | 15.687 [15.563–16.254] | 392.915 [389.292–428.064] |
| 100000-closed  | redb  | 0.132 [0.126–0.229]    | 2.333 [2.174–2.846]       |
| 100000-closed  | turso | 0.201 [0.181–0.207]    | 2.210 [2.209–2.993]       |
| 1000000-closed | redb  | 0.150 [0.133–0.166]    | 2.399 [2.368–2.457]       |
| 1000000-closed | turso | 0.198 [0.186–0.199]    | 2.214 [2.178–2.249]       |

### Query first: rule_page

| Case           | Store | Query RPC ms        | Queued observation RPC ms |
| -------------- | ----- | ------------------- | ------------------------- |
| 1000-active    | redb  | 0.654 [0.644–0.718] | 25.575 [24.673–26.360]    |
| 1000-active    | turso | 0.982 [0.970–1.014] | 39.796 [39.106–40.195]    |
| 10000-active   | redb  | 3.107 [3.088–3.578] | 236.930 [232.983–315.359] |
| 10000-active   | turso | 4.630 [4.326–6.937] | 387.059 [376.111–549.016] |
| 100000-closed  | redb  | 0.540 [0.498–0.847] | 2.635 [2.483–3.276]       |
| 100000-closed  | turso | 0.775 [0.760–0.826] | 2.966 [2.816–3.057]       |
| 1000000-closed | redb  | 0.520 [0.481–0.584] | 2.657 [2.614–3.051]       |
| 1000000-closed | turso | 0.842 [0.792–0.920] | 3.030 [2.838–3.108]       |

### Query first: rare_page

| Case           | Store | Query RPC ms        | Queued observation RPC ms |
| -------------- | ----- | ------------------- | ------------------------- |
| 1000-active    | redb  | 0.298 [0.296–0.342] | 21.063 [20.747–21.827]    |
| 1000-active    | turso | 0.543 [0.514–0.550] | 34.822 [34.652–37.559]    |
| 10000-active   | redb  | 2.782 [2.753–3.914] | 193.587 [193.377–278.041] |
| 10000-active   | turso | 5.608 [3.628–5.824] | 383.750 [375.108–434.149] |
| 100000-closed  | redb  | 0.143 [0.105–0.145] | 2.335 [2.258–2.639]       |
| 100000-closed  | turso | 0.338 [0.245–0.533] | 2.400 [2.316–2.673]       |
| 1000000-closed | redb  | 0.597 [0.536–0.602] | 2.816 [2.793–2.843]       |
| 1000000-closed | turso | 1.033 [1.021–1.041] | 3.051 [3.029–3.087]       |

### Query first: topology

| Case           | Store | Query RPC ms           | Queued observation RPC ms |
| -------------- | ----- | ---------------------- | ------------------------- |
| 1000-active    | redb  | 0.683 [0.653–0.728]    | 25.442 [25.050–26.419]    |
| 1000-active    | turso | 0.857 [0.845–0.923]    | 32.550 [31.731–32.579]    |
| 10000-active   | redb  | 9.907 [9.863–18.350]   | 239.306 [238.970–362.225] |
| 10000-active   | turso | 12.227 [10.504–14.043] | 393.849 [384.546–495.848] |
| 100000-closed  | redb  | 0.136 [0.096–0.142]    | 2.473 [2.229–2.526]       |
| 100000-closed  | turso | 0.203 [0.180–0.320]    | 2.688 [2.280–3.001]       |
| 1000000-closed | redb  | 0.127 [0.114–0.135]    | 2.385 [2.242–2.528]       |
| 1000000-closed | turso | 0.219 [0.183–0.262]    | 2.257 [2.208–2.442]       |

### Query first: live_topology

| Case           | Store | Query RPC ms              | Queued observation RPC ms |
| -------------- | ----- | ------------------------- | ------------------------- |
| 1000-active    | redb  | 34.094 [33.812–35.524]    | 55.017 [54.863–56.912]    |
| 1000-active    | turso | 38.416 [38.348–39.690]    | 69.750 [69.428–70.839]    |
| 10000-active   | redb  | 381.605 [380.430–421.287] | 589.036 [588.191–766.707] |
| 10000-active   | turso | 416.786 [415.951–487.070] | 785.057 [782.863–950.992] |
| 100000-closed  | redb  | 0.062 [0.049–0.074]       | 2.267 [2.171–2.761]       |
| 100000-closed  | turso | 0.159 [0.125–0.194]       | 2.119 [2.102–2.336]       |
| 1000000-closed | redb  | 0.049 [0.048–0.093]       | 2.232 [2.203–2.305]       |
| 1000000-closed | turso | 0.132 [0.127–0.153]       | 2.218 [2.197–2.630]       |

### Sampled RSS (not peaks) and logical database file totals

| Case           | Store | RSS initial MiB     | RSS after ingest MiB   | RSS after queries MiB  | Files before checkpoint MiB | Files after checkpoint MiB |
| -------------- | ----- | ------------------- | ---------------------- | ---------------------- | --------------------------- | -------------------------- |
| 1000-active    | redb  | 10.06 [9.60–10.09]  | 25.59 [25.18–26.34]    | 27.45 [26.97–27.59]    | 16.07 [16.07–16.07]         | 16.07 [16.07–16.07]        |
| 1000-active    | turso | 13.63 [13.12–13.63] | 31.23 [30.65–31.27]    | 33.41 [31.34–33.45]    | 8.31 [8.31–8.31]            | 8.31 [8.31–8.31]           |
| 10000-active   | redb  | 10.05 [9.60–10.07]  | 135.27 [135.15–136.56] | 131.78 [130.06–132.22] | 128.50 [128.50–128.50]      | 128.50 [128.50–128.50]     |
| 10000-active   | turso | 13.57 [13.09–13.57] | 131.07 [126.88–133.98] | 132.61 [126.14–141.41] | 60.61 [60.61–60.61]         | 60.61 [60.61–60.61]        |
| 100000-closed  | redb  | 10.06 [9.59–10.06]  | 67.59 [67.45–67.65]    | 67.86 [67.70–67.91]    | 514.00 [514.00–514.00]      | 514.00 [514.00–514.00]     |
| 100000-closed  | turso | 13.62 [13.04–13.64] | 43.25 [34.29–44.23]    | 43.64 [35.08–44.59]    | 443.99 [443.99–443.99]      | 444.00 [444.00–444.00]     |
| 1000000-closed | redb  | 10.07 [10.07–10.07] | 69.84 [69.23–70.09]    | 70.10 [69.49–70.34]    | 6144.64 [6144.64–6144.64]   | 6144.64 [6144.64–6144.64]  |
| 1000000-closed | turso | 13.51 [13.08–13.59] | 45.85 [44.01–46.04]    | 46.22 [44.35–46.40]    | 4471.33 [4471.33–4471.33]   | 4471.35 [4471.35–4471.35]  |

### Files after checkpoint

| Case           | Store | DB MiB                    | WAL MiB             | shm    |
| -------------- | ----- | ------------------------- | ------------------- | ------ |
| 1000-active    | redb  | 16.07 [16.07–16.07]       | 0.00 [0.00–0.00]    | absent |
| 1000-active    | turso | 5.08 [5.08–5.08]          | 3.23 [3.23–3.23]    | absent |
| 10000-active   | redb  | 128.50 [128.50–128.50]    | 0.00 [0.00–0.00]    | absent |
| 10000-active   | turso | 50.14 [50.14–50.14]       | 10.47 [10.47–10.47] | absent |
| 100000-closed  | redb  | 514.00 [514.00–514.00]    | 0.00 [0.00–0.00]    | absent |
| 100000-closed  | turso | 440.86 [440.86–440.86]    | 3.14 [3.14–3.14]    | absent |
| 1000000-closed | redb  | 6144.64 [6144.64–6144.64] | 0.00 [0.00–0.00]    | absent |
| 1000000-closed | turso | 4468.13 [4468.13–4468.13] | 3.21 [3.21–3.21]    | absent |

## Interpretation and next steps

Keep redb as the default. On this exact fixture, Turso's median actor observation cost is about 2.10x at 1k active, 1.97x at 10k active, 3.06x at 100k closed, and 3.59x at 1M closed. At 1M the median adapter commit cost is 56.84 ms for redb versus 217.53 ms for Turso; the difference is primarily inside the current adapter path, but that includes SQL VM, rowid/index, replacement, serialization, and IO work. It is not a pure engine comparison.

Turso reduces the 1M pre-checkpoint logical file total from 6144.64 to 4471.33 MiB (about 27.2%) and median post-ingest RSS sampling from 69.84 to 45.85 MiB (about 34.4%). These are sampled RSS values and logical file lengths. The post-checkpoint Turso total is 4471.35 MiB; its WAL file remains allocated, so checkpointing is not equivalent to truncating that file.

Both stores keep archived Session queries fast: at 1M the median target top20 query is about 0.130 ms for redb and 0.179 ms for Turso. Live topology is about 0.060 / 0.149 ms when no connections are active, but at 10k active it is 389.49 / 416.17 ms. Switching engines alone would not resolve that active query's actor blocking. The 10k steady observation means are 460.00 / 861.91 ms, showing the first ingest alone does not explain the difference.

Repeat variability matters: Turso's million-closed elapsed times were 466.58, 362.51, and 454.82 s, versus redb's 126.32, 126.73, and 127.97 s. The largest observed Turso observation was 1337.87 ms in round 3. No best-run-only result, guaranteed peak memory bound, or cross-drive speedup is asserted.

The optional adapter implements the complete current trait and passes the shared contract; it is more than a test fake. Production replacement still requires engine crash/fault qualification, lifecycle/error-boundary review, and application composition/migration work. Useful next experiments would profile the active accounting/live projection costs and separately measure SQL update/upsert policy, normalized columns/filter pushdown, or WITHOUT ROWID under its experimental feature. Each should preserve the same durability and facts rather than remove work to improve a score.
