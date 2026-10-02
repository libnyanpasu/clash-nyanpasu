# Core log storage

Core log subscriptions request `debug` independently of the configured kernel log level. Nyanpasu filters saved records by level and literal, case-insensitive message text. Meow uses the same subscription level; its upstream debug feedback behavior is accepted.

## Ownership and persistence

The composition root opens `RedbCoreLogStore` in `<app logs directory>/core` and injects it into `CoreLogsActor`. `CoreLogsClient` exposes typed asynchronous operations through `NyanpasuClient`. The stream actor awaits log admission, so capture applies backpressure rather than accumulating a separate queue. Blocking storage calls transfer the store to a blocking thread and return it to its actor owner.

Records include a capture identifier, kernel instance and kind, receipt time, level and complete message. Historical records survive kernel restarts and application restarts. Core log bodies are absent from the shared stream snapshot, stream events and global frontend provider.

## Rolling limits

All Core instances share one directory budget of **64 MiB**, including database files and control files. Each redb shard is capped at **20 MiB**. Before creating or reopening the writer, the store reserves its entire possible growth plus 16 KiB of control space and removes the oldest shards until that reservation fits. The storage backend rejects writes or file extensions past the shard cap before they modify the file. Retained history is therefore a contiguous suffix; retained record count depends on message size and database overhead.

Only the active writer stays open, with a 1 MiB redb cache. A historical read opens one shard with a 256 KiB cache and closes its transaction and handle before returning. The directory lock admits one store owner. Readers and rotation are serialized by the log actor.

The actor commits batches with redb's immediate durability after 64 KiB or 250 ms. A valid record larger than the batch threshold is flushed immediately. Log WebSocket messages and frames have a 1 MiB limit; connection subscriptions retain their existing limits. Encoded records allow an additional 4 KiB for Nyanpasu metadata.

## Queries and the viewer

`query_core_logs` returns at most 200 preview rows and approximately 256 KiB of serialized response data. Message previews end on a UTF-8 boundary at 4 KiB. `get_core_log` reads the complete record on demand. Level filtering uses a stored index, including normalization of `warning` and `warn`; keyword matching examines the complete message before truncating its preview.

A keyword query scans at most 2,000 candidates or 4 MiB per call. Its continuation cursor advances over examined nonmatches. A matching row excluded by the response budget remains available to the next request. Cursors identify the history generation, shard and sequence; rolling expiration returns `cursor_expired`, while a missing detail returns `record_gone`.

The mounted Core page retains at most 500 previews with a 2 MiB estimated string/object budget. It keeps one outstanding page request and one outstanding detail request. Only one row can display a complete detail at a time. Unmounting releases the page cache and listeners. Pausing the viewer leaves capture running and still applies rolling and clear invalidation. Loading earlier records pauses following; resuming following reloads the current tail.

`clear_core_logs` clears saved Core history globally and changes its generation. A durable generation change records the clear intent, allowing startup to finish an interrupted clear. All windows discard records from the old generation through status events or polling. The command, query and detail errors have the same typed representation over Tauri RPC and HTTP RPC.

## Failure and shutdown behavior

A storage failure retains only the bounded pending batch. New samples are discarded and counted until retry succeeds; no growing memory fallback is used. Recovery checks the transaction header and stored batch before replay, including when reopening the database temporarily fails, so a committed batch is not duplicated. Malformed or oversized input is discarded without poisoning a valid pending batch.

Unavailable or corrupt storage is reported in the viewer. Corrupt current database files remain available for investigation. Shutdown stops admission and flushes the pending batch in the owner's cleanup. An abrupt exit can lose the uncommitted batch; immediately committed records are recovered on restart.

## Validation

Storage and actor boundary tests live in `backend/tauri/src/core/logs/`. Frontend cache tests and browser hook tests live in `frontend/interface/tests/`; complete record interactions live in `frontend/nyanpasu/tests/log-record.browser.test.tsx`. The optional real HTTP browser test seeds 10,000 stored records and verifies previews, complete details and clearing across windows.

```sh
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features core::logs
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features unified_rpc::tests::browser_debug_page_and_real_rpc -- --exact --ignored --nocapture
CORE_LOG_BENCH_BYTES=1024 cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features core::logs::tests::actor_memory_under_sustained_capture -- --exact --ignored --nocapture
```

The memory benchmark runs the production actor and redb adapter in an isolated Linux test process. It reports RSS, peak RSS and directory bytes at 1,000, 10,000 and 100,000 captured records. These measurements include the test process and allocator overhead; they do not measure a complete desktop application's WebView. Repeat with `CORE_LOG_BENCH_BYTES=8192` for larger messages. Windows and macOS require platform validation of file locking, rotation and abrupt-exit recovery.
