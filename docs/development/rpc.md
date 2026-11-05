# Unified RPC

Use this guide when adding or changing an application command, frontend call, or
shared event. See [architecture](architecture.md) for dependency and actor rules.

Application commands use the unified RPC framework, with Tauri IPC and HTTP as transport adapters. The [target RPC layer](../design/application-platform-extraction.md#rpc-targets) adds a third target, the CLI over a local socket. The registry, the event bus, the WebUI router and the commands that need no shell context live in `nyanpasu-rpc`; the wire contract (error envelope, targets, shell capabilities, owners, event names and local-socket frames) lives in `nyanpasu-rpc-protocol`. The desktop shell keeps the Tauri IPC entrypoint and the Tauri-bound commands. Commands should be thin adapters. They should:

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
    client: tauri::State<'_, NyanpasuClient>,
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
- Put a command that needs no shell context in `backend/nyanpasu-rpc/src/commands.rs` with `#[specta::specta]` and the injected `nyanpasu_rpc::State`; a command that needs Tauri context stays in `backend/tauri/src/ipc.rs` with `#[tauri::command]`. Map `tauri::Error` at the Tauri adapter with `IpcError::unknown`; shared crates do not know Tauri errors.
- Keep command metadata in `backend/tauri/src/specta_export.rs`. Register read-only operations as queries and side-effecting operations as mutations, even if their names start with `get` or `query`. Regenerate TypeScript bindings through the existing export workflow; do not hand-edit generated bindings.
- Frontend application calls use generated protocol types and transport adapters from `@nyanpasu/rpc`, then query/mutation bindings from `@nyanpasu/query`. The app injects the RPC client/context into query providers. Transport selection belongs in the RPC package. Do not call raw Tauri `invoke`, import legacy transport bindings for application commands, or add ad hoc HTTP fetches in pages/hooks.
- Native Tauri plugin APIs and frame delivery remain boundary-specific adapters. Channel subscription control is an application operation: declare it with `#[nyanpasu_macro::rpc]`, register it as a mutation, and call it through `rpc`. The desktop dispatcher binds Channel descriptors to the invoking webview; a Channel does not opt an operation into HTTP. Browser-accessible UI must guard native capabilities and provide an appropriate browser path or explicit unsupported state.

### Declare capabilities and preserve transport semantics

- HTTP access is explicit opt-in via `rpc(http)`, not inferred authorization from a compatible signature. Review the operation's effects before enabling it. Desktop OS operations, arbitrary outbound URL diagnostics, and desktop-only server controls stay desktop-only unless deliberately adapted and reviewed.
- Local-socket (CLI) access is a separate opt-in via `rpc(local)`; HTTP exposure does not imply it, nor the reverse. The macro rejects `local` on a signature that needs Tauri context.
- Declare the shell capabilities a command can reach with `rpc(capabilities(...))` (the `ShellCapability` variants in `nyanpasu-rpc-protocol`, in snake case). Annotate by reachable effects, including effects a payload can trigger, not only by the signature. A command declaring `elevation` can never be `local`.
- A capability the command needs in order to work is `capabilities(...)`: a shell that lacks it does not register the command. A capability that the command's effects can only reach through the shell's optional effect owners, such as the system proxy, hotkeys or the tray behind `patch_app_config`, is `rpc(optional_capabilities(...))`. A shell without it still registers the command, and the effect is reported as unsupported through the effects port, never as applied. `elevation` stays in neither list for a `local` command.
- A command that acts on the caller's resources takes `caller: Caller` (the neutral counterpart of the desktop `Window`) with `rpc(owner)`; the transport supplies the `RpcOwner`.
- Never inject `AppHandle` into an HTTP-enabled RPC command or add it to `RpcDependencies`; doing so grants the handler every capability reachable from the app. Route shared side effects through `NyanpasuClient` and the existing typed client or actor message that owns them. Do not add a one-off managed Tauri closure when an existing actor path already performs the operation. `EventBus` and `rpc.events` carry notifications from backend to frontend; they are not an inbound command channel.
- Shared operations receive explicit dependencies and call `NyanpasuClient`. Assemble `RpcDependencies`, routers, and server adapters in the composition root. The facade must not accept `axum::Router` or expose transport infrastructure; avoid strong reference cycles between the server/router and client.
- Use `rpc(owner)` and the injected `RpcOwner` for caller-owned resources such as log sessions. Enforce ownership on every resource operation; do not use Tauri window identity as the shared domain identity or trust a client-supplied owner.
- Use `rpc(result)` when a Result alias needs explicit fallible-return handling. Preserve structured `RpcError` metadata and domain errors across both transports; never serialize an error as a successful payload or flatten it into an unstructured string.
- Keep HTTP RPC, SSE, and the development proxy behind a listener's access credential and Host/Origin checks. In the desktop mode a session cookie identifies an owner; it does not by itself authenticate access. The desktop server stays disabled by default and bound to loopback. Serve HTTP only through `nyanpasu_rpc::http::HttpListener` (desktop) or `RouterListener` (below), which apply their checks with or without frontend assets.
- A router or other headless shell serves the same router through `nyanpasu_rpc::http::RouterListener` instead, which binds an address the shell chooses. Its credential is a session cookie that only a one-time ticket can create (128 random bits, single use, about 60 s, issued by the local-only `issue_web_ticket` command through the injected `WebTickets` port); there is no fixed Host authority, a request that carries `Origin` must carry `http://{Host}`, and responses refuse framing (`X-Frame-Options: DENY`, `frame-ancestors 'none'`). Here the cookie does authenticate: whoever may run `issue_web_ticket` decides who gets in, and a session reaches the `rpc(http)` command set, never the local allowlist. The listener is plain HTTP; the shell owns its exposure. The listener owns the ticket and session book and returns the issuing half for the composition root to inject; a shell without a WebUI injects `WebTickets::unavailable()`.
- The local socket (`nyanpasu_rpc::serve_local`, and `LocalClient` in `nyanpasu-rpc-protocol`) is authenticated only by its permissions: the socket is `0600` in an owner-only directory. Each connection is one `RpcOwner`. A client deadline reports an unknown outcome, never a cancellation: the server finishes a call whose caller has left.
- Shared frontend event subscriptions use `event-transport.ts` through `rpc.events`. Preserve shared connection disposal and state resynchronization on connection, reconnection, and event-buffer overflow. New shared events must be added to `EventName` in `nyanpasu-rpc-protocol` and published on the `EventBus`; Tauri events, SSE and the local socket subscribe to it. Do not create desktop-only listeners for shared state.
- The unified transport does not change actor ownership rules: in-process RPC waits for the real result without a timeout, and dropping a caller does not cancel owner-started work. Network/IPC and connection-draining deadlines stay at their respective boundaries. A transport timeout does not prove cancellation or safe retry.
- Verify changed shared commands on both transports, including error mapping, query/mutation classification, HTTP capability restrictions, and owner isolation where applicable.

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

`build_transport_builder` registers only `unified_rpc::call_rpc`, the framework's
Tauri entrypoint. Put application metadata in `build_specta_builder` and regenerate
both binding files with `specta_export::tests::export_typescript_bindings`. Do not
add a direct application command to the transport builder. The macro source tests
check that every IPC application command has a working UnifiedRpc desktop handler
and that every shared command expands to a neutral handler only.
