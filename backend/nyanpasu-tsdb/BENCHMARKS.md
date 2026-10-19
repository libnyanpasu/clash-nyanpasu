# Crate-only measurements

All results here are provisional **synthetic approximations**, not captured user workloads or
profile recommendations. No application, GUI, proxy, network or consumer reconstruction runs. The
timing harness uses only this crate's public facade. The space experiment below also inspects
redb's public closed-file statistics. No preset or format is changed.

## Method

`benches/tsdb.rs` is a harness-free binary using `Instant` and `black_box`, without a new benchmark
dependency. Inputs, canonical labels and payloads are generated before opening the measured store.
Borrowed batches, including event sequences, are prepared before each write clock starts. A separate
tiny store warms up writes, queries, open and close. Query warmup on the measured dataset precedes
read samples. Verification, CSV formatting, sample allocation and file metadata reads occur outside
timers. Result destruction follows the timed return.

Each shape runs independently with Consistent and Durable input writes, on memory and (with `redb`)
a private file. The whole day's physical counters include initial open, maintenance, recovery, both
suspend checkpoints, final close and compaction, plus verification reads. They exclude the
disposable warmup and copying an explicitly supplied source. These are total I/O per simulated input
day, not the sum of the displayed API timings. Counter values repeat on that day's operation rows:
do not sum them again. Maintenance normally gets one round per
16 batches, then converges after input; the stalled shape deliberately omits interim calls.
A retained snapshot spans subsequent writes and maintenance, then is dropped before replay open. A
write may run the facade's mandatory catch-up round while behind; that is part of its timing.

Samples time facade calls only. Capture is separate from series/page timings. Queries scan the whole
day, return one total series group or the first 16 events; correctness scans every event page in
both directions and compares generated keys, labels, nulls, identities, text and sums. Consistent
data is checkpointed before the timed replay open, ahead of final convergence; `replay_bytes`
records the public journal bytes at that point. Recovery is the real suspend/recover/restore path,
including WaitingForReaders, not an injected backend failure. A close sample ends after the
backend's close callback. Memory compaction is Unsupported; redb compaction is attempted with no
live snapshot. No healthy reader is revoked for it.

All maintenance kinds have an additional tiny workload, isolated to series or events. Public
Maintained counters identify expiration, cleanup, recycling, tier consolidation, delta merge, event
consolidation and sorted-run merge (overlap above R). Every category must be observed, not inferred
from a zero duration. These isolated rows use scale `smoke` even in an S/M/L run. Their explicit
experiment overrides are journal_bytes=1, quiet_time=1ms, event_buffer_rows=1, consolidate_chunks=2,
max_sorted_runs=2 and cleanup_chunks=2. Retention expires their data only after consolidation and
merging. Visible sums and the emptied catalog are checked.

Nearest-rank p50 is always reported with n; p99 is blank below 1,000 samples. Small lifecycle and
maintenance samples are descriptive observations, not tail-latency claims. Write samples cover the
whole growing workload rather than a stationary distribution. A known 1..1000 ns fixture checks
percentile arithmetic and the CSV parser checks every output row.

## Fixtures

One seeded xorshift generator produces caller-owned descriptors. The default decimal seed
is 5645615412495340373. Zero maps to one; the chosen seed is recorded on every result row.

| Shape                   | Synthetic approximation                                                         |
| ----------------------- | ------------------------------------------------------------------------------- |
| hourly                  | S: 90,000 labels / 160,000 points; M: 240,000 / 590,000; L: 600,000 / 2,540,000 |
| sparse-desktop          | 1,297 labels, 58 inputs in every hour (median 58), 1,392/day                    |
| minute                  | Same scale inputs, grouped in minute buckets instead of hour buckets            |
| closed-20k / closed-50k | 20,000 / 50,000 mirrored series/text/identity events per day                    |
| gateway-200k            | 200,000 events per day through the same schema                                  |
| text-nullable           | 512-byte text, every seventh row null                                           |
| large-value             | 64 events, 54 with 1 MiB non-null texts; small batches remain under batch_bytes |
| late-30s / late-7d      | Delayed arrival of a day's old timestamps, including old bucket deltas          |
| churn-pinned            | Expire/recycle while pinned, unpin/release, then admit replacement labels       |
| full-catalog            | Deliberately restricted catalog exercising overflow admission                   |
| stalled-maintenance     | All bounded maintenance delayed until input finishes                            |
| retained-reader         | An old snapshot remains charged across writes and maintenance                   |

Smoke keeps every shape, both durabilities, both engines and every verification, with 24 labels, 96
points, 32 events (four large-value events); full-catalog uses 4,096 labels and 4,096 events to fill
its small catalog. Retained-reader smoke uses 4,096 points in 16 batches, asserting a
subsequent write and a maintenance call while the first snapshot is held. It never samples away a
scenario. Labels are single-dimensional ASCII, point
values include zero, and text is intentionally repeated `x`. Every event uses the same valid
identity UUID. Compression is optimistic and these shapes are not an application schema. Churn
advances the clock, expires the old extent with one pin still held, checks the surviving
combination, unpins it and recycles, then writes 16 distinct replacement labels Durable and checks
their decoded answers. These extra transitions count in the input day's physical I/O. Each stream
has one-day extents. Input rows and combinations are generator cardinalities; folding and
aggregation can reduce resident combinations and stored representation rows.

The baseline is Profile::desktop: head=32MB, catalog=32MB, query=56MB, staging=8MB, backend
cache=56MB and overhead=8MB; journal cap=64MB, batch cap=4MiB, consolidate_chunks=64,
consolidate_bytes=16MiB, cleanup_chunks=64 and max_sorted_runs=8. The experiment's
durable_interval=1 day lets maintenance honor the selected workload without the preset's
always-Durable policy. Writes request their printed mode directly. Checkpoints, open and compaction
remain Durable. Full-catalog uses a 64KiB catalog with enough labels to fill it and overflow; no
profile floor is recommended.

## Running

From `backend/` (Cargo runs this binary with the crate directory as its working directory):

```powershell
cargo bench -p nyanpasu-tsdb --all-features -- --smoke
cargo bench -p nyanpasu-tsdb --all-features -- --output G:/bench/S.csv
cargo bench -p nyanpasu-tsdb --all-features -- --scale M --shape hourly --output G:/bench/M.csv
cargo bench -p nyanpasu-tsdb -- --smoke
cargo bench -p nyanpasu-tsdb --all-features -- --shape hourly --samples 1000
```

Default scale is S; M and L require `--scale`. Optional `--seed <decimal u64>` changes inputs;
`--shape <name>` selects a named shape while retaining the tiny maintenance checks. `--samples N`
controls read repetitions (default 16, smoke 4); choose at least 1,000 for read p99s, and repeat
whole processes separately to assess variance. Stdout is CSV unless `--output` is given; stderr
contains progress and total elapsed time. Output parent directories must already exist. Outputs use
`create_new`: an existing destination is refused, including a symlink or hard link to the copy-mode
source. Fresh redb files live under Cargo's `CARGO_TARGET_TMPDIR` on the target filesystem and are
removed after each workload; an aborted run can leave files there, never in the crate source tree.
No live store is read.

A later owner-supplied sanitized redb copy can use `--path <copy> --shape <compatible-shape>`. This
copies the supplied file into a private target temporary directory before opening it; the source is
never opened as a database or mutated. The schema must match the chosen synthetic schema. The range
is derived from public status bounds. This mode shares the same timed reads and CSV path, and
measures open/replay, reads, status and actual close. It does not generate or replay input onto that
copy. The requested `--scale` is retained in its CSV. Its source column is `owner-supplied-copy`; it
must not be presented as a synthetic result.

### Exact charged-memory peaks

```powershell
$env:TSDB_BENCH_SCALE = 'S' # smoke, S, M or L
$env:TSDB_BENCH_SHAPE = 'hourly' # any generator shape
cargo test -p nyanpasu-tsdb --release --all-features status::bench_tests -- --ignored --nocapture
Remove-Item Env:TSDB_BENCH_SCALE, Env:TSDB_BENCH_SHAPE
```

These ignored release tests reuse the exact generator by path. Each operation resets peaks to its
resident baseline, then reads head/staging/query root peaks and end charges. Before each reset,
`between-operations` rows record the prior peaks, including every untimed verification page and
status read; their ns=0 denotes an untimed observation, not zero work. The test-only AtomicU64
is raised under the admission mutex; even a reservation returned entirely inside one call
contributes. Each child has its own peak; measuring roots avoids counting a child's prepaid parent
envelope twice. `open` and `recover-new-roots` report peaks already recorded by newly constructed
roots; their ns=0 means timing not collected at that boundary. The `recover-old-roots` row has the
reused query root but retired head/staging roots. The tests check retained result charges and
complete drain after suspend/close. Test-only timing is not used as a performance result. The CSV
columns are source, engine, operation, ns, head_peak, staging_peak, query_peak, head_after,
staging_after, query_after (bytes).

### Process memory, measured externally

On Linux, run the built bench executable under `/usr/bin/time -v <binary> --smoke` (or omit
`--smoke` for S); maximum resident set size includes generator, engine and harness allocations. On
Windows, inspect the native OS counters while the binary is running in another terminal:

```powershell
Get-Process tsdb-* | Select-Object Id, WorkingSet64, PeakWorkingSet64
```

PeakWorkingSet64 is the OS high-water counter through that observation, not an allocation sampler.
An observation before process exit is a lower bound on the eventual lifetime peak. Record its
observation time and do not call it an exact end-of-process RSS result. These numbers include
generator, engine and harness allocations, unlike the charged roots. No process-memory observation
changes a store budget or a profile preset.

## CSV columns

`source, seed, scale, shape, engine, durability, operation` identify the synthetic scenario. `n,
p50_ns, p99_ns, total_ns` summarize only API timings; `units` and `units_per_second` use input rows
for writes, fixture points for the whole-range series query, up to 16 rows for a page, or one
call/round for other operations. These are workload units, not measured decoded chunk rows.
`logical_bytes` sums final visible stream bytes from status (fragment bytes plus head journal bytes,
not uncompressed logical values). `file_bytes` is the final redb file length. `day_write_bytes` and
`day_syncs` count successful StorageBackend writes and syncs, including actual close; the wrapper
forwards all range-lock methods. These are backend API bytes/calls, not SSD media writes or hardware
flush commands; set_len zeroing is excluded. A direct known I/O self-check verifies five written
bytes and one sync. Zero physical fields mean inapplicable on memory, not a claim of zero
memory-engine work. `journal_bytes, runs, crowding_marks, catalog_bytes` are final public status
observations. `replay_bytes` is the journal size before replay open, rather than backend read
traffic. `samples_ns` preserves each raw duration in encounter order, separated by semicolons; its
length, sum and percentiles are checked independently when validating the CSV. All counts are per
simulated input day.

Actual event-scan ways use the existing private ReadLog in one ignored metadata test under
query/events/tests/bench.rs. No production observer or public method is added. Six overlapping
chunks are measured before and after convergence at R=2; this complements public sorted-run counts,
which do not count L0 ways. Run without timing claims in either build configuration:

```powershell
cargo test -p nyanpasu-tsdb --all-features records_overlap_ways -- --ignored --nocapture
```

## Environment and provisional results

Measured 2026-10-19, Windows 11 Pro 10.0.26300, Intel Core i9-14900KF, x86_64-pc-windows-msvc;
worktree and timed redb temporary files on G: ReFS. Ignored redb peak tests use the OS temporary
directory (C: NTFS); their timings are unused. Compiler: rustc 1.101.0-nightly (822182db4
2026-10-13), LLVM 23.1.3, Rust 2024. Cargo bench uses the repository's release settings:
opt-level=s, LTO, one codegen unit, panic=unwind. Features: redb (all features). Seed as above.
Target rustflags: +aes,+sse2 from the repository configuration. The G: device is a ZHITAI TiPro7000
1TB NVMe. OS caches were not flushed; results are warm/uncontrolled cache observations, not
cold-start or power-loss measurements. Raw CSVs are not committed: they are machine-specific. Final
smoke completed all validations in 1.196s. Exact charged-root S measurements passed on both engines
(4.52s excluding compilation): head maximum 169,704 bytes; staging 739,626 bytes memory / 753,978
bytes redb; query maximum 6,716,776 bytes on either. All roots reached zero after close. These
measurements include every verification page, a retained query result and a nonempty-head recovery.
The release test build took 3m48s; the ignored tests add no execution time to the normal gates.

Final S completed in 173.003s excluding compilation: 836 summary rows and 25,026 raw API samples, on
both engines and requested write modes. Each row retains its own raw durations; no operation group
here reaches the 1,000-sample threshold, so S makes no p99 claim.

| Hourly engine / mode | Write n | Write p50 ms | Input points/s | Series p50 ms (n=16) |
| -------------------- | ------- | ------------ | -------------- | -------------------- |
| memory / Consistent  | 625     | 2.2611       | 39,387         | 2.8760               |
| memory / Durable     | 625     | 3.5042       | 33,954         | 3.0155               |
| redb / Consistent    | 625     | 3.0190       | 85,589         | 5.3018               |
| redb / Durable       | 625     | 3.9990       | 72,934         | 2.8425               |

Separate S read sweeps use `--samples 1000`: hourly took 27.603s and closed-20k took 4.869s. Each
table cell below has n=1,000 warm repetitions of the same query/page on a held snapshot; these are
empirical p99s, not process-run confidence intervals. Event pages have 16 nonempty rows.

| Engine / mode       | Series p99 ms (n=1,000) | Event page p99 ms (n=1,000) |
| ------------------- | ----------------------- | --------------------------- |
| memory / Consistent | 6.3202                  | 0.1661                      |
| memory / Durable    | 6.2435                  | 0.1374                      |
| redb / Consistent   | 6.4797                  | 0.1673                      |
| redb / Durable      | 6.3601                  | 0.2181                      |

These are workload-wide point throughputs, not the inverse of median batch latency. The hourly
logical stream bytes are 415,668 on both engines. Before replay open the journal contains 79,631
bytes. Redb replay open was 54.7052ms Consistent / 35.8667ms Durable (n=1 per mode). These single
warm/uncontrolled observations do not establish a mode ordering.

| Hourly redb mode | Final file bytes | Written bytes / input day | Syncs / input day |
| ---------------- | ---------------- | ------------------------- | ----------------- |
| Consistent       | 11,456,512       | 11,848,960                | 45                |
| Durable          | 22,847,488       | 61,289,344                | 710               |

Logical bytes agree despite the different final file sizes; these values include the full
checkpoint/replay/recovery/compact/close schedule, not just 625 writes. They are provisional
observations, not attribution of the space difference to durability alone. The separate overlap
observation measured six chunk ways before convergence and two after (on memory and redb, R=2; n=1
each). The counter-only ignored test took 0.07s in debug. It reuses the existing ReadLog from the
query test scope.

An external Windows observation during S's late-30s phase reported PeakWorkingSet64 =
149,749,760 bytes. This is an OS high-water observation through that phase, a lower bound on
lifetime process memory, and includes generated inputs and engine/harness allocations. It is not the
exact root-charge measurement or a completed-process RSS claim. M and L were not run: the complete S
matrix already takes minutes; larger sweeps remain explicit opt-ins. Per-shape charged-memory sweeps
beyond hourly also remain unrecorded. No profile value or memory floor is changed by these results.

## Limits

M/L and any missing profile/platform combinations remain hardware/time pending; nothing here
justifies new preset values. Backend cache/overhead are not measured by the facade roots. Allocator
overhead and actual page-cache residency require external process tools. File length is not
filesystem allocated blocks; no platform FFI or unsafe code was added. Process-run variance/paired
repeats and separate cache/overhead attribution remain pending. No cold-cache eviction, real data,
real power loss, consumer overlay/session rebuild, mobile termination, repeated-backup incarnation,
wear amplification on a physical device, or steady-state multi-day confidence interval was measured.
A per-day complete I/O total includes lifecycle exercises; it must not be extrapolated to normal
daily device wear without a matching host schedule. Single observations are not p99, and repeated
text is not a realistic compression ratio. Copy mode awaits an explicitly supplied compatible
sanitized fixture.

## E12: small-chunk redb space (provisional)

This experiment measures the current format only. Run it separately from timing CSV:

```powershell
cargo bench -p nyanpasu-tsdb --all-features -- --space --output G:/bench/space.csv
cargo bench -p nyanpasu-tsdb --all-features -- --space --smoke
```

`--space` has a fixed matrix, independent of S/M/L. It accepts `--seed` and `--output`, but not
`--path`, `--shape`, `--samples`, M or L. The ordinary `--smoke` also runs the space smoke and
validates its CSV internally; stdout keeps the timing CSV schema. Without redb, `--space` fails
explicitly and the ordinary smoke retains its memory-engine behavior. Space CSV outputs also use
`create_new`, refusing any existing destination. Experiment files and their statistics copies live
under Cargo's `CARGO_TARGET_TMPDIR` on the target filesystem, outside the crate source tree.

The single parameterized generator uses one summed series field with seeded 32-bit values, or
one non-null fixed-width event column filled with seeded random bytes. Fixed bytes avoid the
optimistic repeated text of the earlier fixtures. Series extents contain 1/8/64/512 distinct
labels in one bucket; two identical input passes create L0 rows which consolidate to the stated
number of representation rows. Event extents contain distinct keys. For each boundary, input
payload is approximately 512 bytes below/above 4/8/16/32/64 KiB. Assertions check that actual data
fragment bytes straddle the boundary, not just input lengths. Fragment 0 is reported separately.
Controls reach the 65,536-row limit, or 3,800 x 1,024 random bytes (about 93% of the 4 MiB raw
chunk limit), and must still consolidate to exactly one chunk per extent.

There are eight extents per small fixture and two per full control. Each of three independent
files uses the same seed and inputs. All writes are Durable. Experiment overrides are
journal_bytes=1, event_buffer_rows=1, event_buffer_age=1ms and series_quiet_time=1ms; other values
are Profile::desktop. No preset, memory floor or stored format changes. Generation precedes
backend open. Physical accounting needs no timer warmup; no latency percentiles are claimed.

Fresh means input, quiet consolidation, convergence and explicit close. Retained reopens, expires
exactly the older half by age, completes cleanup/recycling, then closes. Compacted reopens the
retained store, calls facade compaction with no snapshots, verifies, then closes. Each phase
checks public status rows, zero journal/L0/cleanup, and decoded sums or every event in both page
directions. Chunk counts and unchanged fragment lengths are checked after close. A compacted
phase includes an incarnation open and checkpoint/close, so tiny files need not shrink.

Statistics are taken on a byte copy of each closed file. Opening that copy, an unmodified
`Database::begin_write().stats()`, and read-transaction `open_untyped_table().stats()` cannot
contaminate the original file or its counting backend. The typed fragment table is read only to
sum value lengths by fragment index; no codec, manifest or private facade observer is added.
All table stored-byte counts include keys: fragment keys are 12 bytes each. Namespace tables
remain separate, including catalog tables 1/2/3, journal 5, manifest 6 and diagnostics 15.

Important redb 4.4 details (checked against the installed source): `transactions.rs:2666` includes
free pages in database `fragmented_bytes()`. Here `free_pages = file_bytes/page_size - 1 -
allocated_pages`; newly created v3 files have one header page and no region headers
(`page_store/page_manager.rs:39`). Subtract free bytes to report allocated slack separately.
Table leaf/branch counts count nodes, including multi-page nodes, so table base pages come from
`(stored + metadata + fragmented)/page_size`. Database metadata includes master/system trees.
`unattributed_allocated_bytes` preserves the remaining allocated footprint, including deferred
pages; it is not falsely called live payload or free space. Every observation asserts:

```text
file = stored + metadata + allocated slack + unattributed allocation + free pages*page + header
```

Space CSV columns are `source,seed,shape,rows_per_extent,input_payload_bytes,repeat,state,table`,
then `live_chunks,fragment0_bytes,data_bytes,fragments`; `table_stored_bytes,table_metadata_bytes,
table_fragmented_bytes,table_pages`; `file_bytes,page_size,allocated_pages,stored_bytes,
metadata_bytes,fragmented_bytes,unattributed_allocated_bytes,free_pages,header_bytes`;
`phase_write_bytes,phase_syncs,total_write_bytes,total_syncs,write_bytes_per_chunk,syncs_per_chunk,
phase_write_to_live_payload,inline_upper_bytes,inline_boundary_penalty_bytes`.
Global values repeat on table rows; use `table=TOTAL` once, never sum repeated globals.
Physical counts include open, L0 writes, consolidation, retention/cleanup/recycling, compaction
and actual close, with no stats-copy I/O. Per-chunk denominators are surviving chunks in that
state. The fresh phase's write/live-value ratio is write amplification; subsequent phases add
no input, so their ratios are maintenance I/O per retained payload, not another ingest rate.

### Observations (n=9 independent files per shape/state, across three processes)

Measured 2026-10-19 on Windows 11 Pro 10.0.26300, Core i9-14900KF, G: ReFS on ZHITAI TiPro7000
1TB NVMe; rustc 1.101.0-nightly (822182db4, 2026-10-13), LLVM 23.1.3, x86_64 MSVC, all features,
redb 4.4.0, existing release opt-level=s/LTO settings. Seed 5645615412495340373. OS/device cache
state was warm and uncontrolled; no cold-cache claim. Each of three processes measured 48 files /
144 closed-file observations, in 12.063/12.217/12.484s (excluding compilation). All 144 files /
432 observations passed. Raw machine-specific CSV stays outside the repository. File bytes and
phase write bytes/syncs had min=max across all nine measurements per cell: zero observed range,
not a claim of zero variance on other seeds, devices or larger workloads. The ordinary all-feature
smoke passed in 1.279s, including the 0.108s space smoke (12 observations).

F/R/C below mean fresh/retained/compacted. Payload is fragment values including directories and
checksums, excluding redb keys. Fragment allocation includes table keys, metadata and page slack.
The ~60% observation reproduces: the 8KiB- case stores 7,916 bytes/chunk in 12,800 bytes, a 61.7%
redb overhead; it is not a universal cost per fragment. Small fragments can share one leaf.

| Shape        | F payload B/chunk | Fragment allocation KiB/chunk F/R/C | F overhead |
| ------------ | ----------------: | ----------------------------------: | ---------: |
| 1 rows       |             202.2 |                         0.5 / 1 / 1 |     153.2% |
| 8 rows       |             248.2 |                         0.5 / 1 / 1 |     106.2% |
| 64 rows      |             515.2 |                         1.5 / 1 / 1 |     198.1% |
| 512 rows     |           2,338.2 |                         4.5 / 5 / 5 |      97.1% |
| 4KiB-        |           3,823.8 |                         4.5 / 5 / 5 |      20.5% |
| 4KiB+        |           4,848.8 |                      12.5 / 13 / 13 |     164.0% |
| 8KiB-        |           7,916.0 |                      12.5 / 13 / 13 |      61.7% |
| 8KiB+        |           8,939.0 |                      20.5 / 21 / 21 |     134.8% |
| 16KiB-       |          16,140.0 |                      20.5 / 21 / 21 |      30.1% |
| 16KiB+       |          17,166.0 |                      36.5 / 37 / 37 |     117.7% |
| 32KiB-       |          32,576.0 |                      36.5 / 37 / 37 |      14.7% |
| 32KiB+       |          33,636.0 |                      68.5 / 69 / 69 |     108.5% |
| 64KiB-       |          65,536.9 |                      68.5 / 69 / 69 |       7.0% |
| 64KiB+       |          66,570.9 |                      72.5 / 73 / 73 |      11.5% |
| 65,536 rows  |         272,509.0 |                     358 / 360 / 360 |      34.5% |
| 3,800 x 1KiB |       3,895,537.5 |                  3946 / 3944 / 3944 |       3.7% |

File-level costs and fresh physical I/O (the latter includes all fresh maintenance and close):

| Shape        |       File KiB F/R/C | F write KiB/chunk | F syncs/chunk | F write/payload |
| ------------ | -------------------: | ----------------: | ------------: | --------------: |
| 1 rows       |      124 / 108 / 108 |             133.2 |         4.125 |         674.65x |
| 8 rows       |      112 / 120 / 108 |             136.8 |         4.125 |         564.08x |
| 64 rows      |      140 / 112 / 108 |             146.8 |         4.125 |         291.65x |
| 512 rows     |      480 / 292 / 172 |             177.8 |         4.125 |          77.84x |
| 4KiB-        |      168 / 144 / 104 |             140.8 |         4.125 |          37.69x |
| 4KiB+        |      416 / 200 / 172 |             143.2 |         4.125 |          30.25x |
| 8KiB-        |      272 / 272 / 172 |             151.8 |         4.125 |          19.63x |
| 8KiB+        |      704 / 388 / 144 |             167.8 |         4.125 |          19.22x |
| 16KiB-       |      704 / 388 / 144 |             167.8 |         4.125 |          10.64x |
| 16KiB+       |      580 / 356 / 260 |             199.8 |         4.125 |          11.92x |
| 32KiB-       |      612 / 612 / 292 |             199.8 |         4.125 |           6.28x |
| 32KiB+       |      964 / 580 / 388 |             263.8 |         4.125 |           8.03x |
| 64KiB-       |    1028 / 1028 / 452 |             263.8 |         4.125 |           4.12x |
| 64KiB+       |    2060 / 2060 / 452 |             331.8 |          4.25 |           5.10x |
| 65,536 rows  | 16452 / 16452 / 8132 |           8,735.6 |          39.5 |          32.83x |
| 3,800 x 1KiB | 16140 / 16132 / 4004 |           8,734.2 |          22.5 |           2.30x |

Retention/cleanup adds 49.9–50.9 KiB and 3–3.25 syncs per surviving small chunk; compaction adds
28.6–73.3 KiB and 4.75–8.5 syncs. Full-row and full-byte controls add respectively 114.5/118.5 KiB
for retention and 1,588.4/4,053.3 KiB for compaction. These small-cohort totals amortize fixed
transactions poorly; they must not be extrapolated as application throughput or daily I/O.

For example, the compacted 8KiB- file has 32,271 stored bytes, 8,320 redb metadata bytes, 61,809
allocated-slack bytes, 8,192 unattributed allocated bytes, 15 free pages and one 4KiB header:
176,128 bytes total. Its per-table accounting is:

| Table        | Stored B | Metadata B | Slack B |
| ------------ | -------: | ---------: | ------: |
| fragments    |   31,744 |        348 |  21,156 |
| namespace/0  |       23 |         20 |   4,053 |
| namespace/10 |      116 |         36 |   3,944 |
| namespace/15 |      178 |         52 |   3,866 |
| namespace/5  |        0 |          0 |       0 |
| namespace/6  |      140 |         36 |   3,920 |
| namespace/7  |       60 |         36 |   4,000 |
| namespace/9  |       10 |         12 |   4,074 |

The difference between summed table metadata/slack and the database figures belongs to redb's
master/system trees. The full-row control also pays for its 65,536-label catalog: its 16,452 KiB
fresh file cannot be explained by its roughly 532 KiB of chunk values alone.

### Inline-tail decision: defer

`inline_upper_bytes` is an optimistic ceiling, **not a measured format saving**. For two-fragment
chunks with data below 64 KiB, it combines the values, removes the data CRC, adds no new directory
fields, assumes perfect packing without small-record/branch overhead, retains power-of-two
large-leaf allocation, then subtracts that ideal footprint from the actual fragment table. It
also credits existing slack/branch costs which ordinary packing could remove. Ineligible full
chunks get zero. Even this ceiling does not promise a shorter file: free pages, allocation
placement, other tables and compaction can dominate. No alternative encoding was written.

Across measured shapes the whole-file ceiling is 0–36 KiB, at most 13.9% of the actual file;
for 1/8-row chunks it is zero, for 64 rows 4/0/0 KiB F/R/C, and for 512 rows 16/8/8 KiB
(3.3%/2.7%/4.7% of each file). The 4KiB- boundary allows 4/4/4 KiB; 4KiB+ through 32KiB+
allow 36/20/20 KiB. Full controls and the 64KiB boundaries allow zero. Those ceilings exceed the
observed zero-byte run range where positive, but do not establish an achievable end-to-end gain.

The boundary penalty is concrete allocation arithmetic: 64KiB- has about 65,335 data bytes plus
202 directory bytes. Combining them plus redb's 20-byte leaf/key overhead crosses 64 KiB and
would round the leaf to 128 KiB: **64 KiB more per chunk** (512/256/256 KiB F/R/C). A bounded
inline policy would have to decline that case. In 8KiB-, metadata-only fragment 0 would grow
from 157 to 7,916 bytes, about 50x, increasing lent/charged bytes for selective and metadata
reads. New offsets/checksums/ownership would also require a format bump.

The measured small-chunk overhead is real, but removing a fragment does not remove all page
rounding or fixed tables. Full-chunk performance, selective reads, memory floors and writes under
an alternative format remain unmeasured; no lack of regression is claimed. Keep the current
format and defer the tail until an approved prototype can show actual savings and those checks.
No new production API or observer was needed, and no production line changes.
