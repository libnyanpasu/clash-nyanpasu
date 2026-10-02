# Core console logs through the shared file viewer

## Objective

Use the managed core's stdout/stderr archive as the source of the Core log tab.
Retain startup failures and exit diagnostics, accept bounded file rotation, and
remove the application's duplicate WebSocket capture and redb history service.
The preceding viewer fix remains an independently reviewable first PR.

## Current behavior

`nyanpasu-core-manager` parses both process pipes into `LogFrame`, publishes a
bounded broadcast stream, and writes `core-*.jsonl` under its runtime `logs/`
directory. Its default rotation is approximately 4 MiB per file and five files.
The writer emits explicit gap records when its broadcast subscription lags.
Local archives live under the app configuration directory's `runtime/control/logs`.
Service archives belong to the service and must be read through service IPC.

`LogFrame` is currently defined in `nyanpasu-core-metadata`, despite being shared
by the parser, archive writer, and IPC events. The application instead captures
the kernel API's `/logs` WebSocket in `ClashWsActor` and persists another copy in
`CoreLogsActor`/redb. That history is cleared on a confirmed stop and is unavailable
for failures before the API starts. Application and service log tabs already use
the bounded `nyanpasu-logging` file index/session actor and an upward-paginated viewer.

## Decisions

### One console record model

Move `LogFrame`, `LogStream`, `LogTimestamp`, `LogField`, and their `LogLevel` into
`nyanpasu-logging`. Preserve their serialization byte shape, including enum
spellings, timestamp provenance, epoch, core kind, raw text, and truncation.
`nyanpasu-logging` may depend on `nyanpasu-core-metadata` for `ClashCoreKind`;
metadata must not depend on logging. Update manager and IPC consumers directly,
without leaving the old metadata export as a compatibility API. Do not conflate
the file index's existing `Level::Warn` wire spelling with `LogLevel::Warning`.

### Reuse the bounded file query engine

Add a narrow filesystem adapter for Core JSONL (for example `FsCoreLogFiles`),
reusing the existing bounded reads and file identity checks. Permit only the
manager's file naming scheme, reject traversal and symlinks, and order archives
consistently with the writer. A current-file session follows rotation; a named
archive remains pinned and reports `FileGone` if retention removes it.

Extend record decoding/indexing to understand the Core envelope:

- `t: log`: use `at` for timestamp display/filtering, retain physical append order,
  normalize level/target/message for filtering, and preserve the full JSON record
  in raw inspection within the shared response bounds.
- `t: gap`: expose a readable warning with observation time and dropped count;
  do not silently hide capture loss.
- Preserve the parser's `truncated` flag. Ignore an unfinished trailing JSONL
  line until complete, as the existing file reader does.
- Keep app/service record parsing and response, scan, memory, and session bounds.

No second persistent index/database is needed. The shared `LogsActor` remains
the owner of each source's query sessions and in-memory indexes; pure parsing
stays outside orchestration. No query is executed through the core lifecycle
actor. Only the runtime writer mutates archive files.

### Service IPC

Add typed Core archive operations alongside service log operations:

| Operation      | Endpoint              | Result             |
| -------------- | --------------------- | ------------------ |
| `CoreLogFiles` | `/v1/core/logs/files` | `Vec<LogFileInfo>` |
| `CoreLogOpen`  | `/v1/core/logs/open`  | `LogSession`       |
| `CoreLogQuery` | `/v1/core/logs/query` | `LogPage`          |
| `CoreLogClose` | `/v1/core/logs/close` | `()`               |

Open/query/close use `OwnedLogRequest` and the same ownership checks as existing
service log sessions. Construct the service's Core `LogsClient` at its server
composition root using the manager's archive directory. Add an explicit Core
log-query capability/version to service status; older services report unsupported
instead of falling back to kernel `/logs`. Keep existing service logging operations
and capability semantics intact. IPC deadlines remain in the transport adapters.
Preserve the `CoreLog(LogFrame)` live event, but this viewer uses archive polling
for replayable history rather than adding another stream consumer.

### Application facade and RPC

Extend the shared application `LogSource` with explicit `core_local` and
`core_service` sources. Preserve `app` and `service`. The existing unified
`list_log_files`, `open_log_session`, `query_logs`, and `close_log_session`
operations expose those sources through both desktop and HTTP transports.
New service IPC calls sit behind a narrow injected adapter, parallel to
`IpcServiceLogs`. The composition root constructs local Core filesystem/query
dependencies and their shutdown wiring.

Explicit sources make a session's host stable even if the core changes execution
host. The frontend derives its selection from actual `get_core_status().type`,
not the desired service-mode setting, and remounts the viewer when that host
changes. Closing an old session still addresses its original source. Queries
must remain useful when the core is stopped or failed. Do not silently show
local logs while service status is unavailable.

Owner identity remains injected by `rpc(owner)`, never provided by the browser.
Catalog calls remain queries; session open/query/close retain mutation metadata
because they allocate or renew resources. Generate bindings through Specta.

### Frontend behavior

Keep the three visible tabs: Core, Application, Service. Core uses the same
`FileLogs`/`useFileLogs` implementation and standard MDY file select as the other
tabs, with display source separated from RPC source where necessary. Preserve
top-triggered pagination, virtual rows, scroll anchoring, filters, JSON/copy,
follow-latest, and bounded preview retention. The current-file choice follows
rotation; retained files are selectable. Clear means clear the display, not delete
diagnostic archives. Host changes dispose the old session. Session/file expiry
and unsupported services use explicit existing error states.

Console output is the chosen source even where its level configuration differs
from the former API debug subscription. Do not keep `/logs` as a fallback or merge
the two sources, which would duplicate records. Do not change core log-level
configuration as an incidental part of this migration.

### Remove the replaced path

Remove app `CoreLogsActor`, `RedbCoreLogStore`, their setup, old core-log RPCs,
status event, and frontend core-preview hook/state. Remove only the logs leg of
`ClashWsActor`; connections, traffic, and memory streams remain. Update direct
callers, fixtures, perf coverage, generated metadata and current developer docs.
Retire tests that only exercise the removed database and replace their user
behavior coverage at the file/RPC boundaries. Existing obsolete `current.redb`
files are no longer opened; no broad filesystem cleanup/migration is introduced.

## Verification

- Runtime: model wire-shape tests, Core JSONL parsing/filtering/raw/truncation,
  gap records, incomplete tails, catalog/path rejection, rotation and file loss,
  and cross-owner/session/source isolation on the new service endpoints.
- Application: injected local and service sources, stopped-core access, structured
  errors and source selection, desktop/HTTP registration and owner isolation.
- Browser: Core file selection, upward history, clear-display behavior, follow
  rotation, source changes and session cleanup; retain the first PR's regression
  coverage where it still applies.
- Run relevant Rust tests and format/Clippy checks, generated binding workflow,
  frontend tests/typecheck/lint/build, and architecture/boundary gates. Record any
  platform or environment limitations rather than claiming unexecuted coverage.

## Delivery and review order

1. App PR: the existing zero-header, upward pagination, and MDY footer fix.
2. Runtime PR against runtime main: shared model ownership and Core archive IPC.
3. App PR stacked on (1): this specification, runtime pin, facade/RPC migration,
   shared viewer, and removal of the replaced capture/store. It depends on (2).

Each repository receives atomic buildable commits. No PR is merged by the agent;
the user reviews the stack and the cross-repository dependency links.
