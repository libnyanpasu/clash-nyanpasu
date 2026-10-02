# Core log storage

Core subscriptions request `debug` independently of the configured kernel level. Nyanpasu filters saved records by level and literal, case-insensitive message text. Meow uses the same subscription level; its upstream debug feedback behavior is accepted.

## Current session

The composition root opens `RedbCoreLogStore` in `<app logs directory>/core` and injects it into `CoreLogsActor`. The actor owns one lazily created `current.redb` file. Records are uncompressed JSON values with capture and kernel instance metadata, receipt time, level and complete message. There is no disk size limit, shard rotation, retention policy or cross-session history recovery.

Application startup removes the previous session's file, including remnants of the earlier shard implementation. Binding a new kernel instance clears committed and pending records before admitting its samples. A confirmed stopped state deletes the current file. Ordinary socket reconnects, controller changes and configuration updates for the same instance preserve logs. An unavailable endpoint alone does not prove the kernel stopped. Application shutdown deletes its session file; an abrupt exit leaves a file that the next startup removes.

All transactions and file operations run serially through the log actor on a blocking thread. Clearing closes the database before deleting the file, changes the in-memory history generation and publishes status to all windows. The next write creates a new database. A directory ownership lock protects deletion and lazy creation from another application process.

## Memory and queries

The writer has a **1 MiB redb cache**. Records are committed in batches after **64 KiB or 250 ms**. Stream delivery awaits admission, avoiding a separate growing queue. WebSocket subscriptions use the transport's existing message and frame limits.

`query_core_logs` returns up to **200 preview rows / approximately 256 KiB**, with **4 KiB UTF-8 previews**. Level filtering uses an index. Keyword matching examines the complete stored message. Each call examines at most **2,000 candidates / 4 MiB** and advances its cursor over nonmatches without skipping a matching row excluded by the response budget. Cursors contain only generation and sequence. Clearing or changing sessions invalidates old cursors and details.

The mounted viewer retains up to **500 previews / approximately 2 MiB of estimated data**. Complete records are loaded on demand, with one outstanding detail request. Unmounting releases previews and listeners. Pausing stops following but continues capture; session changes still invalidate paused windows. Log bodies are absent from shared snapshots, events and the global frontend provider.

## Failures

Storage failures stop saving new records, release the failed batch, report the error and count discarded samples. Failed commits are not replayed and no growing memory fallback is used. Manual clearing or a new kernel session reinitializes storage. Clearing failure is reported and prevents further writes until a successful reset. Malformed or oversized samples are rejected without disabling valid capture.

## Validation

Tests cover complete pagination, UTF-8 previews, scan continuation, lifecycle invalidation, controller rebinding, deletion after releasing database handles, startup cleanup after abrupt exit, write failures and memory use. Shared queries, details and clearing use the same typed errors over Tauri and HTTP RPC. The real HTTP browser fixture verifies 10,000 records and clearing across windows.

```sh
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features core::logs
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features core::clash::ws
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features unified_rpc::tests::browser_debug_page_and_real_rpc -- --exact --ignored --nocapture
CORE_LOG_BENCH_BYTES=1024 cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features core::logs::tests::actor_memory_under_sustained_capture -- --exact --ignored --nocapture
```

The memory benchmark uses an isolated Linux process running the production actor, redb adapter and real page queries. It reports RSS, peak RSS and database bytes at 1,000, 10,000 and 100,000 records; these include test-process and allocator overhead rather than a complete desktop WebView. Repeat with `CORE_LOG_BENCH_BYTES=8192` for larger messages. Windows and macOS still require platform validation of file-lock release and lifecycle deletion.
