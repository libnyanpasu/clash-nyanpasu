# Core log storage

Core subscriptions follow the committed Clash configuration log level. A changed
level reaches the streams actor as the `core_log_level` configuration effect, and
the actor reopens only the logs socket at that level; a frame the replaced socket
already queued is kept. `silent` closes the socket without deleting history. A
reconnect uses the current level. Changing capture level does not alter the
viewer's exact-level or literal, case-insensitive message filters.

## Current session

The composition root opens `RedbCoreLogStore` in `<app logs directory>/core` and injects it into `CoreLogsActor`. The actor owns one lazily created active `current.redb` file and sealed shards. Records contain JSON with capture and kernel instance metadata, receipt time, level and complete message. The actor validates and serializes each admitted record once; the store uses its prepared level index without decoding it again.

Application configuration `core_logs` defaults to `shard_size_mib: 16` and `max_size_mib: 64`, and the Clash settings page edits it. A committed change reaches the log actor as the `core_log_storage` configuration effect and applies to the current session: a smaller budget evicts oldest shards at once, and a new shard size applies from the next write. A setting the actor rejects leaves storage untouched; a failed eviction or seal stops writes like any other storage failure. A shard must be at least 4 MiB and the total budget at least twice its size. The store rotates at the shard threshold and evicts oldest files using actual redb file sizes, including indexes. One bounded transaction may exceed a threshold temporarily; eviction finishes before the next write. Only the active database remains open between calls. Sealed databases open on demand with the same bounded cache. There is no persisted manifest or cross-session recovery.

Application startup removes the previous session's file, including remnants of the earlier shard implementation. Binding a new kernel instance clears committed and pending records before admitting its samples. A confirmed stopped state deletes all session files. Ordinary socket reconnects, controller changes and configuration updates for the same instance preserve logs. An unavailable endpoint alone does not prove the kernel stopped. Application shutdown deletes its session files; an abrupt exit leaves a file that the next startup removes.

All transactions and file operations run serially through the log actor on a blocking thread. Clearing closes the database before deleting the file, changes the in-memory history generation and publishes status to all windows. The next write creates a new database. A directory ownership lock protects deletion and lazy creation from another application process.

## Compression

`core_logs.compression` accepts `none`, `preset` (default), or `trained`. Changing it seals the active shard, so each shard keeps the one dictionary its records were encoded with; reapplying the same mode keeps the encoder and any dictionary trained this session. Each record has an independent versioned envelope with encoding and original length. Zstd level 3 reuses the writer context; messages that do not shrink use raw envelopes. Each shard stores its immutable dictionary in redb, so detail and page queries can decode both older and newer shards independently.

The bundled 32 KiB dictionary is trained exclusively on synthetic TCP, UDP, DNS and configuration messages using reserved example addresses and names. Runtime mode starts with this preset and collects at most 2 MiB of samples, capped at 16 KiB per record. It trains once on the existing blocking storage call, releases the samples, and starts a new shard on success. Training failure keeps the preset without retries. Clearing ends the samples and dictionary lifecycle. Dictionary contents in runtime mode may contain log text and are deleted with their shard; compression is not encryption.

Decode allocation and scan budgets use original sizes. Malformed envelopes, oversized lengths and length mismatches fail before returning records. Record preview and complete-message search behavior are unchanged.

To regenerate the preset with the locked Zstd version:

```sh
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features core::logs::codec::tests::regenerate_preset_dictionary -- --exact --ignored
```

## Memory and queries

The writer and each temporary shard reader have a **1 MiB redb cache**. Zstd contexts, query results and runtime training samples use additional bounded memory. Records are committed in batches after **64 KiB or 250 ms**. Stream delivery awaits admission, avoiding a separate growing queue. WebSocket subscriptions use the transport's existing message and frame limits.

`query_core_logs` returns up to **200 preview rows / approximately 256 KiB**, with **4 KiB UTF-8 previews**. Level filtering uses an index. Keyword matching examines the complete stored message. Each call examines at most **2,000 candidates / 4 MiB** and advances its cursor over nonmatches without skipping a matching row excluded by the response budget. Cursors contain only generation and sequence. Rotation preserves the generation and increasing sequence. Evicted cursors return `CursorExpired`; evicted details return `RecordGone`. Clearing or changing sessions invalidates old cursors and details.

The viewer subscribes before its initial query and consumes status events directly. It does not poll while idle or fetch bodies while hidden. Burst notifications share one in-flight query and one catch-up flag. Transport resync and visibility restoration refresh status; a retained cursor follows new records without another status RPC. Eviction trims paused or hidden previews, and stale in-flight generations cannot restore cleared rows. Sorted pages merge linearly while keeping unchanged row references and memoized row callbacks.

The mounted viewer retains up to **500 previews / approximately 2 MiB of estimated data**. Complete records are loaded on demand, with one outstanding detail request. Unmounting releases previews and listeners. Pausing stops following but continues capture; session changes still invalidate paused windows. Log bodies are absent from shared snapshots, events and the global frontend provider.

## Failures

Write or rotation/deletion failures stop saving new records, release the failed batch, report the error and count discarded samples. The first storage failure is published immediately; subsequent discard counts share the existing 250 ms flush timer. Unchanged empty flushes do not publish status. Failed commits are not replayed and no growing memory fallback is used. Manual clearing or a new kernel session reinitializes storage. Clearing failure is reported and prevents further writes until a successful reset. Malformed or oversized samples are rejected without disabling valid capture.

## Validation

Tests cover complete pagination, UTF-8 previews, scan continuation, lifecycle invalidation, controller rebinding, deletion after releasing database handles, startup cleanup after abrupt exit, write failures and memory use. Shared queries, details and clearing use the same typed errors over Tauri and HTTP RPC. The real HTTP browser fixture verifies 10,000 records and clearing across windows.

```sh
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features core::logs
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features core::clash::ws
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features unified_rpc::tests::browser_debug_page_and_real_rpc -- --exact --ignored --nocapture
CORE_LOG_BENCH_BYTES=1024 cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features core::logs::tests::actor_memory_under_sustained_capture -- --exact --ignored --nocapture
```

The memory benchmark uses an isolated Linux process running the production actor, redb adapter and real page queries. It reports RSS, peak RSS and database bytes at 1,000, 10,000 and 100,000 records; these include test-process and allocator overhead rather than a complete desktop WebView. Repeat with `CORE_LOG_BENCH_BYTES=8192` for larger messages. Windows tests exercise deletion after releasing database handles and a rotation blocked by an external file handle. macOS still requires platform validation of file-lock release and lifecycle deletion.
