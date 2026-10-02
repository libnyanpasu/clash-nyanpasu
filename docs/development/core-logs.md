# Core console logs

The Core tab reads the managed core's stdout/stderr JSONL archive through the
same bounded file viewer as Application and Service logs. Startup failures and
exit diagnostics remain readable while the core is stopped or failed. Console
log levels follow the core's configuration; the application does not subscribe to
the kernel API's `/logs` WebSocket or keep a duplicate redb history.

## Sources and ownership

`nyanpasu-logging` owns the console record model, pure record decoding, and the
shared `LogsActor` file indexes and query sessions. The runtime manager alone
writes and rotates `core-*.jsonl` archives, approximately 4 MiB per file and five
files by default. Explicit gap records expose broadcast capture loss.

The composition root injects a local `FsCoreLogFiles` adapter for
`<app config directory>/runtime/control/logs` and a service IPC adapter for the
service-owned archive. The facade exposes `core_local` and `core_service` alongside
`app` and `service` through unified `list_log_files`, `open_log_session`,
`query_logs`, and `close_log_session` operations. RPC injects the caller identity;
browsers cannot select another owner's sessions. Catalog operations are queries;
session operations are mutations because they allocate or renew resources.

The frontend selects Core's source from actual `get_core_status().host` and
remounts when it changes. Explicit sources keep existing sessions bound to their
original host, including their close calls. An unavailable service does not fall
back to local files. Services without the Core log-query capability report
unsupported. Transport deadlines stay in the IPC adapter.

## File viewer behavior and bounds

The current-file choice follows rotation; a selected retained archive stays
pinned and reports `FileGone` when retention removes it. Traversal, symlinks, and
non-Core archive names are rejected. Incomplete trailing JSONL lines wait for the
next poll. Rows preserve archive append order; `at` supplies the displayed timestamp
and time-filter clock. Severity, target, and message also support filtering. Raw
inspection retains the console envelope within the response bounds; `truncated`
reports both parser truncation and response clipping.

The shared file engine retains its response, scan, memory, and session limits.
The mounted viewer uses bounded previews, top-triggered history pagination,
virtual rows, scroll anchoring, filters, JSON/copy, and follow-latest. Clear removes
the display's retained rows; it never deletes diagnostic archives. Application
shutdown stops both local query owners. Obsolete `current.redb` files are no longer
opened or cleaned up by this feature.

## Validation

Runtime tests cover console wire shape, gap records, truncation, filtering,
incomplete tails, filesystem boundaries, rotation, retention, and service session
owner/source isolation. Application tests use injected local/service archives and
verify stopped-core access, structured errors, and shared RPC ownership. The
remaining Clash WebSocket tests verify connections, traffic, memory, and socket
cleanup. The real HTTP browser fixture writes 10,000 Core JSONL records.

```sh
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features client::logs
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features core::clash::ws
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features unified_rpc
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features specta_export::tests::export_typescript_bindings -- --exact
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features unified_rpc::tests::browser_debug_page_and_real_rpc -- --exact --ignored --nocapture
```

The browser fixture requires a production frontend build and Playwright Chromium.
