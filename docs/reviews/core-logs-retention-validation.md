# Core logs retention and viewer validation

Windows, Chromium, October 4, 2026. Implementation follows
[the accepted spec](../design/core-logs-retention.md).

## Simplification audit

The requested read-only subagent audit preceded implementation. The stack removes
the actor/store JSON round trip, redundant cursor comparisons, a second clear/status
transfer, unused flush timers, unchanged flush notifications and ordinary socket
failure discard counts. The final audit also retained 250 ms coalescing of discard notifications: the first storage failure publishes immediately, but 100 subsequent failures update the count without 100 broadcasts. The viewer replaces periodic status/body polling with
status events and one in-flight query plus a catch-up flag. It merges sorted pages
without a map or sort and keeps unchanged row references and callbacks.

Instance/generation fencing, confirmed-stop rereads, directory ownership,
close-before-delete, bounded admission/decoding/scans and one detail request remain.
They protect real lifecycle and memory constraints. There is no new actor, repair
worker, recovery manifest, retry queue or cross-session history format.

## Browser measurements

Existing `logs-open` benchmark, Core source, 500 cached rows, Chromium, CPU throttle
1, two runs before and after on the same Windows machine. The benchmark uses the
real renderer with a fixture hook; RPC reductions are checked separately.

| Metric (ms unless stated)        | Before run 1 / 2 | After run 1 / 2 |
| -------------------------------- | ---------------- | --------------- |
| Open: average maximum frame gap  | 18.5 / 16.8      | 16.8 / 16.8     |
| Open: synchronous work           | 9.7 / 4.3        | 7.3 / 7.3       |
| Open: React rendering            | 14.8 / 7.4       | 10.3 / 10.6     |
| Incoming logs: synchronous work  | 3.7 / 2.5        | 2.1 / 2.2       |
| Incoming logs: React rendering   | 1.9 / 1.2        | 0.9 / 0.8       |
| Incoming logs: maximum frame gap | 16.7 / 16.7      | 16.8 / 16.8     |
| Frames over 50 ms / long tasks   | 0 / 0            | 0 / 0           |
| Rendered rows                    | 15 / 15          | 15 / 15         |

Opening time remains within measurement noise. The repeatable gain is lower
rendering work while records arrive; this is not evidence of a broad page-open
speedup. No frame-drop regression was observed in this fixture.

Hook tests verify one initial body query after subscribing, zero status RPCs for
ordinary updates, no polling during ten seconds of simulated idle time, and stable
idle references. A burst of 100 notifications during an in-flight query needs only
one additional catch-up query. Hidden pages trim evicted previews without fetching
bodies, then resync on visibility restoration. Tests also cover pause/resume,
transport resync, expired cursors, in-flight clears and listener disposal. Existing
page tests preserve virtual scrolling and the visible anchor while loading older
records. FileLog tests still pass.

```powershell
$env:VITE_PERF_SOURCE='core'
$env:VITE_PERF_LOGS='500'
pnpm -F @nyanpasu/nyanpasu bench logs-open --api.port=5179 --api.host=127.0.0.1
```

The system reserves the default Vitest port 63315. Browser regression tests used
a temporary config importing the existing `vitest.config.ts` and setting each
project's `test.api` to `127.0.0.1:5180`. No production or tracked test configuration
was changed for this environment issue.

## Storage measurements

The codec round-trip test uses 14,000 synthetic TCP, UDP, DNS and configuration
records, totaling 3,557,316 JSON bytes. Raw envelopes use 3,641,316 bytes; preset
compression uses 514,021 bytes; runtime training uses 515,857 bytes. These numbers
exclude redb pages and indexes and do not predict compression of arbitrary logs.

The storage benchmark writes 10,000 synthetic connection records in batches of
128, then reads every record through production paging (200 rows per request).
These are unoptimized test-profile measurements, including filesystem work:

| Mode    | Actual redb bytes | Append (ms) | Read all pages (ms) |
| ------- | ----------------: | ----------: | ------------------: |
| None    |         4,214,784 |         245 |                 124 |
| Preset  |         1,265,664 |         272 |                 121 |
| Trained |         1,888,256 |         487 |                 409 |

Training costs include the bounded synchronous training call and the extra shard;
queries reopen sealed shards. Runtime training did not outperform the preset in
this fixture. Preset remains the default. Retention counts actual file sizes,
including dictionaries and indexes, rather than the much smaller encoded payload.

```sh
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features core::logs::tests::storage_compression_benchmark -- --exact --ignored --nocapture
```

## Correctness boundaries

Backend tests cover the capture-level effect, silent/resume and reopening only
the logs socket; cross-shard pagination and eviction; UTF-8 previews and original-size scan
limits; dictionary switching while old shards remain readable; cleanup on stop,
start, clear and next launch; and failure without replay. Windows tests hold an
external file handle to block rotation, verify writes stop, then release it and
verify manual clearing restores capture.

The real HTTP browser fixture passes with 10,000 compressed records, full detail
retrieval and clearing across two windows. Its isolated configuration explicitly
selects English for accessible-name assertions; its headless setup forwards the
real log owner's watch to the HTTP EventBus, replacing the Tauri bridge that is
present in the desktop composition root. The old polling viewer had masked this
missing fixture bridge.

The final focused backend run passes 37 log-related tests. Separate checks pass
12 stream tests, 26 configuration impact tests, retention defaults/validation and
5 Unified RPC tests. The combined frontend run passes 36 tests including the Core
and FileLog viewers and both desktop/HTTP command and event adapters; the final
hidden-clear refinement passes all 15 Core log unit/browser tests. Desktop
transport checks use its mocked IPC boundary, not a packaged WebView.

The stack regenerates RPC types and runs package TypeScript checks, Clippy,
formatting, frontend boundaries and the architecture ledger gate. Linux/macOS
packaged execution and full desktop WebView memory measurements are outside this
Windows validation run.
