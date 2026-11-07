# Unified RPC

Use this guide when adding or changing an application command, frontend call, or
shared event. See [architecture](architecture.md) for dependency and actor rules.

Application commands use the neutral `nyanpasu-rpc` registry, with Tauri IPC and HTTP as transport adapters. The [target RPC layer](../design/application-platform-extraction.md#rpc-targets) will add the CLI over a local socket; the rules below currently cover Tauri IPC and HTTP (WebUI). Commands should be thin adapters. They should:

- parse request DTOs;
- call `NyanpasuClient`;
- map domain errors into command errors;
- never perform business orchestration directly;
- never read or mutate config through globals;
- never spawn core/service background tasks directly.

Allowed shape:

```rust
#[nyanpasu_macro::rpc(http)]
pub async fn patch_verge_config(
    client: nyanpasu_rpc::registry::RpcState<'_, NyanpasuClient>,
    patch: NyanpasuAppConfigPatch,
) -> Result<()> {
    client.patch_app_config(patch).await?;
    Ok(())
}
```

Avoid shape:

```rust
#[tauri::command]
pub async fn patch_verge_config(patch: IVerge) -> Result<()> {
    Config::verge().draft().patch_config(patch)?;
    CoreManager::global().update_config().await?;
    Config::verge().apply();
    Ok(())
}
```

### Use the unified RPC surface

- Declare new application commands with `#[nyanpasu_macro::rpc(...)]`; the macro generates transport handlers and registers them with `UnifiedRpc`. Do not add standalone `#[tauri::command]` application APIs, direct `generate_handler!` registration, or a separate HTTP implementation of the same operation.
- Keep command metadata in `backend/tauri/src/specta_export.rs`. Register read-only operations as queries and side-effecting operations as mutations, even if their names start with `get` or `query`. Regenerate TypeScript bindings through the existing export workflow; do not hand-edit generated bindings.
- Frontend application calls use generated protocol types and transport adapters from `@nyanpasu/rpc`, then query/mutation bindings from `@nyanpasu/query`. The app injects the RPC client/context into query providers. Transport selection belongs in the RPC package. Do not call raw Tauri `invoke`, import legacy transport bindings for application commands, or add ad hoc HTTP fetches in pages/hooks.
- Native Tauri plugin APIs and frame delivery remain boundary-specific adapters. Channel subscription control is an application operation: declare it with `#[nyanpasu_macro::rpc]`, register it as a mutation, and call it through `rpc`. The desktop dispatcher binds Channel descriptors to the invoking webview; a Channel does not opt an operation into HTTP. Browser-accessible UI must guard native capabilities and provide an appropriate browser path or explicit unsupported state.

### Neutral declarations and native wrappers

`rpc` generates a neutral handler and declaration metadata. An accompanying
`#[tauri::command]` additionally generates and registers the native wrapper that
extracts State, AppHandle, Window, Webview and Channel inputs. Shared declarations
live in `backend/nyanpasu-rpc/src/commands/` and have no native wrapper: they declare `RpcState<'_, NyanpasuClient>` (or `Storage`
or `PathResolver`) from the explicit `RpcDependencies` slots and use `rpc(owner)`
with `RpcCaller` for the transport-supplied caller. These neutral declaration
types live in `nyanpasu-rpc::registry` and are omitted from Specta's schema. The handler
passes borrowed dependencies and `RpcOwner` to the implementation; neither is
decoded from request JSON. Both paths preserve camelCase inputs, unknown-field
compatibility, Result aliases and empty-object/null handling for zero inputs.

`rpc(http)` and `rpc(local)` declare independent targets; desktop remains the
complete surface. `capabilities(...)` and `optional_capabilities(...)` record
required facilities and reachable conditional effects. Optional elevation is
valid with `rpc(local)`; it is not a blanket rejection or an execution policy.
Production declarations classify required facilities separately from conditional
effects. Reachability includes indirect tray refreshes after profile commits,
lifecycle completion and proxy-cache notifications, even for reads that refresh
that cache. These presentation effects remain optional. Native admission errors
name the unavailable shell command; HTTP retains its existing rejection message.
The composition root explicitly supplies available capabilities; metadata
describes availability, not authorization or elevation consent. Production local
opt-ins remain E3e work: the initial allowlist is independent of HTTP and no
command is exposed locally by signature compatibility.

The committed `nyanpasu-rpc-protocol/src/command_catalog.rs` is generated from
the same declarations as handler metadata. Its development-only extractor in
`nyanpasu-macro` follows the explicitly listed production roots (currently
`tauri/src/ipc.rs` and `nyanpasu-rpc/src/commands/mod.rs`) through inline and ordinary
external modules, inherits
ancestor `cfg`/`cfg_attr` guards and excludes test fixtures. It does not resolve
arbitrary Rust module systems or add a protocol build dependency on application
code. When commands move, migrate the roots, schema and handler inventories
together. Run from `backend/`:

```sh
cargo test -p nyanpasu-macro --features syn/full export_rpc_command_catalog -- --ignored
cargo test -p nyanpasu-macro --features syn/full rpc_command_catalog_is_fresh
```

The exporter writes explicitly; the freshness test only compares. Regenerate
after changing names, targets, capabilities or declaration guards. The registry
loads catalog metadata before associating linked handlers, so an unlinked native
wrapper leaves a known command returning `unsupported`. Coverage tests distinguish
native wrappers, shared handlers and unlinked catalog entries. Regenerate the
three frontend bindings as well; this seam must leave them unchanged.

`build_specta_builder` uses `schema_commands!` to collect Specta function schemas
from both native and shared declarations, without generating Tauri handlers.
`build_transport_builder` registers only `unified_rpc::native::call_rpc`.
The shell's `DesktopRpc` binds native handlers separately from `UnifiedRpc`;
both native calls and shared desktop calls enter the same audit kernel exactly
once. Shared HTTP commands retain their registry entries when an injected host
port is absent and return `unsupported`. Executable resolution, icon files,
service-install prompt formatting and accent queries use consumer-owned ports;
the shell supplies the AppImage process value explicitly.

### Declare capabilities and preserve transport semantics

The registry first recognizes the command name, then checks the requested target
and every required capability, and only then parses arguments and runs the handler.
Missing required capabilities or an unlinked native wrapper return structured
`unsupported`; only an unknown name returns `unknown_method`. All duplicate
handler names fail registration before availability is considered. Rejections
pass through `dispatch_audited`, and HTTP keeps its existing 501 mapping and
`update_core` rejection before updater job admission.

Use required metadata for clipboard, dialogs/notifications, native webviews and
channels, windows, lifecycle, openers, tray/widget refresh, deep-link queues,
application-update operations, and host accent/debug-HTTP facilities. Caller
ownership is `rpc(owner)`, not a native-webview requirement. Icon/file queries,
release-channel configuration, changelog HTTP reads and service-install prompt
formatting do not require presentation, installation or elevation facilities.
Direct DNS/service elevation operations require `elevation`; runtime service
switches, compensation and `update_core`'s copy fallback record optional elevation.
Config patch/hotkey/effect retries record their reachable optional effects, so
missing presentation or elevation does not reject unrelated edits. Existing domain
validation and injected execution policy still decide whether a particular effect
can run; these annotations do not close the embedded WebUI's indirect elevation
paths or provide per-call authority.

All dispatched application calls enter `dispatch_audited`, including direct HTTP
archive and subscription routes and dispatch-time parameter rejections. The
injected synchronous sink records only transport, fixed command name, caller
(the window label, or a per-process keyed hash of the HTTP/local owner; the HTTP
session cookie authorizes log sessions and never reaches a record), monotonic duration and
outcome (failure kind/code only). HTTP and local use INFO and Tauri
IPC uses DEBUG on `nyanpasu::rpc_audit`, regardless of outcome. Results, polling,
cancellation and panic propagation are unchanged; abort builds cannot record on
Drop. Streams are audited once when established, never per frame. Authentication
rejections, static assets and envelopes rejected by Tauri before `call_rpc` are
outside this boundary. E2b moved the kernel and its port with the registry; E3's
local socket/CLI must enter this same kernel at INFO.

- HTTP access is explicit opt-in via `rpc(http)`, not inferred authorization from a compatible signature. Review the operation's effects before enabling it. Desktop OS operations, arbitrary outbound URL diagnostics, and desktop-only server controls stay desktop-only unless deliberately adapted and reviewed.
- Never inject `AppHandle` into an HTTP-enabled RPC command or add it to `RpcDependencies`; doing so grants the handler every capability reachable from the app. Route shared side effects through `NyanpasuClient` and the existing typed client or actor message that owns them. Do not add a one-off managed Tauri closure when an existing actor path already performs the operation. `EventBus` and `rpc.events` carry notifications from backend to frontend; they are not an inbound command channel.
- Shared operations receive explicit dependencies and call `NyanpasuClient`. Assemble `RpcDependencies`, routers, and server adapters in the composition root. The facade must not accept `axum::Router` or expose transport infrastructure; avoid strong reference cycles between the server/router and client.
- Use `rpc(owner)` and the injected `RpcOwner` for caller-owned resources such as log sessions. Enforce ownership on every resource operation; do not use Tauri window identity as the shared domain identity or trust a client-supplied owner.
- Use `rpc(result)` when a Result alias needs explicit fallible-return handling. Preserve structured `RpcError` metadata and domain errors across both transports; never serialize an error as a successful payload or flatten it into an unstructured string.
- Application error conversion belongs to the registry consumer: `nyanpasu_rpc::error::application_error` and its borrowed helper walk the source chain and preserve domain payloads and core retry metadata. Keep these conversions as free functions, outside the `RpcError` envelope. HTTP owner cookie parsing and status mapping are likewise free functions in the HTTP adapter; protocol values do not own transport parsing or classification.
- Keep HTTP RPC, SSE, and the development proxy behind the existing per-start access credential and Host/Origin checks. A session cookie identifies an owner; it does not by itself authenticate access. Keep the server disabled by default and bound to loopback. Frontend assets and the dev frontend are optional: without either, authenticated API calls work and missing static pages return 404.
- Shared frontend event subscriptions use `event-transport.ts` through `rpc.events`. Preserve shared connection disposal and state resynchronization on connection, reconnection, and event-buffer overflow. Register shared events in the transport metadata and publish them through the bus and its delivery adapters; do not create desktop-only listeners for shared state.
- The unified transport does not change actor ownership rules: in-process RPC waits for the real result without a timeout, and dropping a caller does not cancel owner-started work. Network/IPC and connection-draining deadlines stay at their respective boundaries. A transport timeout does not prove cancellation or safe retry.
- Verify changed shared commands on both transports, including error mapping, query/mutation classification, HTTP capability restrictions, and owner isolation where applicable.

## Local frame and session contract

E3b implements `nyanpasu-rpc-protocol::frame` and `session` without exposing a
listener. A frame is a big-endian u32 byte length followed by UTF-8 JSON, limited
to 4 MiB excluding the prefix. The incremental codec consumes one bounded frame
at a time, supports fragmented/coalesced input, and becomes terminal on malformed,
unknown, oversized or truncated input. `frame-io` is default-off and adds Tokio
read/write helpers; pure values and session rules require no IO runtime.

The first client frame is hello with `versions`, `requiredFeatures` and
`optionalFeatures`. Version 1 is currently the only supported version; the server
selects the highest common version. Both peers must support required `rpc`.
Known optional features are `events` and `connection-details`; unknown optional
features are ignored, unknown required features reject the connection. Accept
passes through the sole ordered writer before request admission, and hello cannot
be repeated. Invalid input/state produces an outer reject when safe to write,
then closes; it never executes a command or adds a variant to `RpcError`.

Request, subscribe and unsubscribe share strictly increasing positive u64 JSON
integer IDs for the entire connection lifetime, including completed/rejected
requests. They cannot wrap; exhaustion requires a new connection. Responses
repeat the request ID and contain exactly one of `result`/`error`. Business
errors preserve existing RpcError JSON and do not close the connection. Until
E1b, result/error reception owns JSON rather than deserializing operational error
sources or static strings. Frame failures never provide request payloads to audit.

Each events subscription names exactly one of configuration status, core status,
service status, app-update state (when available), or core-log status. The
server-generated opaque subscription ID is distinct from its request ID and is
never reused. The ordered output is subscribed acknowledgement → initial reset
→ complete events; overflow requires lag → fresh reset → complete events. Reset
contains the full current value, including the `{"status": ...}` wrapper for
core logs, not log rows. The receiver replaces that stream's current value.
Other event identities return `unknown_stream`; recovery does not add commands
or issue recovery queries. There is no cross-source total-order promise.

Connection details use separate subscribe/data frames, outside EVENT_NAMES/bus.
After acknowledgement the adapter sends the current Some frame, or waits for the
first Some; empty connections are valid. It coalesces to the latest bounded value
and permits sequence gaps without lag/reset. Unsubscribe stops production and
discards pending output before its idempotent acknowledgement; no event/data is
permitted afterwards, including for repeated/unknown unsubscribe IDs.

Establishment errors use `subscription_error` with request `id` and no ack;
post-establishment errors use `subscription` and end that stream. Codes are
`feature_not_negotiated`, `unsupported`, `unknown_stream`, `resource_limit` and
`snapshot_unavailable`. The session bounds pending operations and subscriptions.
E3d implements capability checks, snapshots/race handling, continuous overflow
failure, resource cleanup (including late completions), and audited admission.
E3e owns real client reset application. Frame-only tests do not claim those
dispatcher or demand-release behaviors; E3d tests those through real local connections.

`local::server::LocalServer` owns accepted connections and drains them on shutdown.
Each connection has a fresh instance/connection owner, bounded in-flight work,
output queues and one capacity budget for pending/established resources and closed
log IDs retained for overlapping opens. Admission counts queued and dispatched
operations together against `in_flight` (default 16); coalesced requests within
that budget remain valid. The reader never waits for work capacity: an operation
that finds the budget exhausted closes only that connection, while disconnects
remain visible and all admitted work, including queued requests, still drains.
Request JSON never supplies its owner. Reader/writer closure stops delivery, while
accepted command and establishment tasks finish; every returned log-session ID is
registered and closed with its original source and owner, including late results.
Cleanup settles accepted log opens and closes before sweeping deduplicated session
IDs. A successful explicit close of a tracked session wins over opens issued
before it, even when their duplicate ID arrives after the close reply or disconnect;
closed IDs retain their resource slot until pending log opens and closes settle. Cleanup
therefore never closes that session again, and duplicate successful opens cause
only one cleanup close.
Unsubscribe releases demand before its acknowledgement and discards late queued
frames for that subscription. The root observes every task result and resumes
original panic payloads; the shell must supervise the server future.

Snapshots come from the existing facade, without recovery queries. Bus receivers
are acquired before snapshot reads; publication during a read triggers a bounded
retry. Event notifications read the authoritative full value, so delayed older
publications cannot regress a reset. This permits coalescing and promises only
per-source order, with lag followed by a fresh reset. Continuous snapshot churn,
oversized payloads and failed reads end the subscription explicitly. Missing
app-update capability returns the `unsupported` subscription code and records
`unsupported` in the audit kernel.

Successful Service opens have an ID and are closed promptly. A lost upstream reply
or the existing five-second `IpcServiceLogs` timeout returns `Unavailable` without
an ID; no close ID can be invented. Without renewal, that session expires under
the service's 45-second lease measured from creation/last renewal. Physical stores
have a separate 30-second grace period; this is not a disconnect-to-reclamation
deadline. The runtime service protocol is unchanged.

## Phase E ownership and planned transports

E1a-b moved `RpcOwner`, `RpcError`, `RpcValue`, the request envelope and the eleven shared `EventName` identities into `nyanpasu-rpc-protocol`. Desktop event wrappers remain in the shell and are checked against the protocol names. HTTP cookie parsing, status mapping and application error downcasts remain consumer-side free functions. This move preserves existing owner labels, HTTP errors and bindings.

E1a-c separates neutral declarations from native wrappers and generates independent target/capability metadata and the neutral catalog. E1a-d annotates production declarations and enforces required-capability admission while retaining known commands as `unsupported`; `unknown_method` remains reserved for unknown names. Desktop/HTTP exposure and all three bindings are unchanged. Optional effect metadata does not deny unrelated configuration edits and does not replace elevation policy. The remaining changes are planned in [tasks §5](../spec/2026-11-03-core-package-split/tasks.md#5-phase-e-的-pr-划分).

E2b moved shared commands, registry, error conversion, HTTP infrastructure, event bus and `dispatch_audited`/`RpcAuditSink` and the Tauri-free production `TracingRpcAuditSink` into `nyanpasu-rpc`; the desktop injects that sink, with the same implementation available to the planned headless shell. Native IPC/window/Channel binding and Tauri asset resolution stay in the shell. A schema-only Specta collector accepts neutral commands without generating Tauri handlers; `build_transport_builder` registers only `call_rpc`. Retained host HTTP facilities use explicitly injected ports, including the original Tauri executable resolver and pure icon/file queries. E2a compares the full HTTP exposure set before and after extraction. `nyanpasu-rpc-protocol` owns the envelope, identifiers and complete wire/receiving type closure below CORE_FAMILY; application downcasts stay in the registry. E1b references canonical foundation values, including D2e runtime-values, and uses receiving DTOs where skipped operational sources cannot be reconstructed.

E2c makes observations publish first to `nyanpasu-rpc::events::EventBus` through the seven-source `forward` module and bus adapters for state invalidations and app-update snapshots. Its injected Tauri delivery sink runs synchronously, preserving single-window/type/all-window targets, missing-window errors and payloads; it cannot lag behind an asynchronous native queue. SSE/local receivers are bounded and retain lag/resync. The `listen_any` input bridge is removed; native delivery must never re-enter publication. Acquire forwarder subscriptions synchronously after client construction but before reconciliation, stream activation and listener admission, with initial snapshots where needed. Preserve source order without promising a causal total order across independent producers. Full connection details remain demand-driven, outside the shared broadcast. Shell-originated shared events publish explicitly into the bus.

E3a delivers API-only HTTP startup with optional frontend assets, preserving per-start credentials, owner cookies, loopback binding, default-disabled state and Host/Origin middleware. The frontend fallback is mounted only when supplied; authenticated missing static pages return 404. E3b delivers the pure local frame/session contract described below; E3c delivers optional endpoint listeners; E3d delivers connection dispatch and resource cleanup; clients remain E3e work. Unix sockets and Windows named pipes both require actual platform tests and secure endpoint ownership. One default endpoint per user and app data directory belongs to whichever shell binds first; a second live binder fails with a typed error. CLI uses this default or `--endpoint`. Unix only reclaims a valid user-owned socket in its private directory when connect is refused; all other existing endpoints are refused. Windows uses first-instance pipe exclusivity and needs no stale cleanup. The server generates transport-tagged connection owners; request JSON cannot supply one. Disconnect releases connection resources and subscriptions, including late App/Service log-session completions, without cancelling owner-started work. Successful completion tasks register returned IDs even after closure and promptly close them. A Service open whose upstream reply is lost or exceeds IpcServiceLogs’ five-second timeout has no ID to close; that unconfirmed session relies on the existing 45-second service lease (without renewal, measured from creation/last renewal). This is the explicit V-61 limitation, not a service-protocol change.

Local dispatch and subscription admission enter the existing audit kernel at INFO, with no payloads or per-frame records; Tauri stays DEBUG and HTTP INFO. Every transport preserves fatal panic propagation, with no panic/JoinError conversion into `RpcError`. A dedicated transport task owns the client connection; dropping a call future does not interrupt a partial exchange or desynchronize the stream. Timeout/transport error closes the connection and reports an unknown outcome for potentially submitted calls; mutations are never retried automatically. The initial local allowlist and default-off desktop listener are specified by E3e, independently of HTTP exposure.

E2a moved the four shared HTTP response definitions with their commands before E2b extraction, and puts the shared `TrayIcon` value in protocol; shell asset operations become free functions. E1b completes the remaining downward projections and receiving closure. Runtime-values is an unconditional E2b policy member.

E3 local subscriptions retain only full-value configuration, core and service status, app-update state when available, and core-log status, plus the separate connection-detail demand stream. Each retained event replaces the complete value it describes; subscription starts with a snapshot and local lag triggers a fresh reset. Tasks E3b lists the source evidence and payloads. Clash WS updates, state invalidations, storage changes/resync, deep links and window messages return the existing unknown-stream outcome and allocate no subscription. Their recovery requires a separately reviewed card; the deferred item records round-3 invalidation, storage mapping and watermark acceptance criteria. Connection-detail frames, negotiation, current-after-ack delivery, coalescing and demand release remain unchanged. Real local-connection reset tests cover only the five retained event identities, with no recovery queries or command-allowlist expansion. Tauri/SSE payloads and admission-only auditing are unchanged.

All E cards follow D2 → D3 → D4 → A2b, then E1a → E2 → E3 → E1b → E4. E4 installs a neutral process panic policy before any tasks start; HTTP/local/background-owner subprocess tests must demonstrate non-zero fatal exit with original panic evidence and no synthesized RPC error. E4's separately composed server supplies `RefuseElevation` to service, DNS and binary-install adapters and tests config-driven service changes, runtime retry, compensation and copy fallback. Embedded desktop WebUI policy is unchanged; per-call authority remains outside O-5's scope. E5/OpenWrt and ticket-based LAN login remain paused. Existing frontend transport code, command/event wire shapes and all three generated bindings remain unchanged by E1a–E4.

## Connection-detail streams

`subscribe_clash_connection_details` and `unsubscribe_clash_connection_details`
are desktop-only UnifiedRpc mutations, generated in `rpc-bindings.ts`. Subscription
control must go through `rpc`/`call_rpc`, even though frames use a native Tauri
Channel. The macro parses the serialized Channel descriptor and constructs the
Channel on the invoking webview; unsubscribe verifies the same webview owns the
subscription. Page reload, window destruction, explicit unsubscribe, and application
shutdown release its receiver. A late subscribe result must still be unsubscribed
when its frontend consumer has already unmounted.

Browsers retain `/bridge/connection-details` as a dedicated SSE delivery adapter;
closing the SSE body releases demand. Neither desktop subscription command is
HTTP-enabled. Keep full detail frames out of the shared state event broadcast so
inactive pages do not deserialize them. Native Channel delivery and HTTP SSE are
transport adapters, not exceptions permitting standalone application commands.

`build_transport_builder` registers only `unified_rpc::native::call_rpc`, the framework's
Tauri entrypoint. Put application metadata in `build_specta_builder` and regenerate
all three binding files (`frontend/rpc/src/tauri-bindings.ts`,
`frontend/rpc/src/rpc-bindings.ts`, and `frontend/query/src/query-bindings.ts`)
with `specta_export::tests::export_typescript_bindings`. Do not
add a direct application command to the transport builder. The macro source tests
check that every IPC application command has a working UnifiedRpc desktop handler.

E2b preserves the C2f kernel and production sink without changing audit or wire
behavior. Shared commands and archive collection live in `nyanpasu-rpc/commands`,
application error conversion in `error.rs`, and HTTP routes/actor/assets ports in
`http/`. The Tauri shell explicitly enables `http` and injects synchronous native delivery
into the bus. It explicitly publishes deep links and window messages; resolved
window-type instances keep their individual payloads and targeted delivery. E3a allows HTTP enable without frontend assets or a dev frontend, through the same authenticated API server.
No forwarding modules remain at the old shell paths.

## Local endpoint infrastructure

E3c adds `nyanpasu-rpc/local-server`, off by default, with `protocol/frame-io`
and OS socket dependencies. `local::endpoint::Endpoint::discover` is the shared
mapping for desktop, server and CLI: canonicalize the existing app data directory,
then derive a stable 128-bit SHA-256 identity from that path and the current UID/SID.
No shell name or PID enters the identity. An explicit location uses the same
validation as the default. Discovery performs synchronous filesystem inspection;
async hosts call it on a blocking thread. Binding already uses a blocking task.

| Platform           | Location and ownership                                                                                                                                               | Acceptance tests                                                                                                                                                     |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Unix (Linux/macOS) | Socket in canonical `/tmp/nyanpasu-rpc-<uid>` (0700), socket 0600; override requires an absolute path in a current-user private directory without symlink components | Consecutive accepts, first/racing binder, refused-only stale reclamation, inaccessible/foreign/mode/link refusal and replacement-safe cleanup; run on Linux/macOS CI |
| Windows            | `\\.\pipe\nyanpasu-rpc-<sid>-<identity>`; override retains this user namespace, protected current-user-only DACL on every instance, first-instance exclusivity       | Real named-pipe reconnects, actual pipe DACL, first/racing binder and pipe-name competition                                                                          |

Unix claims retain a nonblocking OS lock on a validated 0600 sidecar file for the
listener's lifetime. The lock file remains after shutdown so binders always lock
the same inode. Only a valid current-user socket with an explicitly refused
connection can be reclaimed, under that lock and after identity revalidation.
Every Unix ancestor must be a real directory owned by root or the current user,
without group/other write permission unless sticky; the endpoint directory itself
must be owned by the current user with mode 0700. This applies to defaults and
overrides. Cleanup checks directory, claim and socket identity and preserves
replacements detected by those checks. The claim serializes binders and the
validated directory chain prevents other users from replacing entries; the same
user and root are trusted between the final identity check and unlink.
Windows has no stale cleanup; accepted instances can retain the name after the
listener closes. The byte-stream library can defer a Windows stream's close while
flushing unread output; connection lifecycle code must account for that ownership.
Neither endpoint construction nor this feature enables desktop service or dispatch.
The parent-child `nyanpasu-local-channel` API and wire protocol are unchanged.
Musl explicitly enables `nyanpasu-rpc/local-server` on the existing RPC library target.
