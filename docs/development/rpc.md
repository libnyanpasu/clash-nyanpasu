# Unified RPC

Use this guide when adding or changing an application command, frontend call, or
shared event. See [architecture](architecture.md) for dependency and actor rules.

Application commands use the unified RPC framework, with Tauri IPC and HTTP as transport adapters. Commands should be thin adapters. They should:

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
- Keep command metadata in `backend/tauri/src/specta_export.rs`. Register read-only operations as queries and side-effecting operations as mutations, even if their names start with `get` or `query`. Regenerate TypeScript bindings through the existing export workflow; do not hand-edit generated bindings.
- Frontend application calls use `frontend/interface/src/ipc/rpc.ts`, generated `rpc-bindings.ts`, and the shared query/mutation helpers. Transport selection belongs in `command-transport.ts`. Do not call raw Tauri `invoke`, import legacy transport `bindings.ts` for application commands, or add ad hoc HTTP fetches in pages/hooks.
- Native Tauri plugin APIs and existing channel-based transport commands remain boundary-specific exceptions. Do not extend those exceptions to ordinary application operations; browser-accessible UI must guard native capabilities and provide an appropriate browser path or explicit unsupported state.

### Declare capabilities and preserve transport semantics

- HTTP access is explicit opt-in via `rpc(http)`, not inferred authorization from a compatible signature. Review the operation's effects before enabling it. Desktop OS operations, arbitrary outbound URL diagnostics, and desktop-only server controls stay desktop-only unless deliberately adapted and reviewed.
- Shared operations receive explicit dependencies and call `NyanpasuClient`. Assemble `RpcDependencies`, routers, and server adapters in the composition root. The facade must not accept `axum::Router` or expose transport infrastructure; avoid strong reference cycles between the server/router and client.
- Use `rpc(owner)` and the injected `RpcOwner` for caller-owned resources such as log sessions. Enforce ownership on every resource operation; do not use Tauri window identity as the shared domain identity or trust a client-supplied owner.
- Use `rpc(result)` when a Result alias needs explicit fallible-return handling. Preserve structured `RpcError` metadata and domain errors across both transports; never serialize an error as a successful payload or flatten it into an unstructured string.
- Keep HTTP RPC, SSE, and the development proxy behind the existing per-start access credential and Host/Origin checks. A session cookie identifies an owner; it does not by itself authenticate access. Keep the server disabled by default and bound to loopback.
- Shared frontend event subscriptions use `event-transport.ts` through `rpc.events`. Preserve shared connection disposal and state resynchronization on connection, reconnection, and event-buffer overflow. New shared events must be registered in the transport metadata and event bridge; do not create desktop-only listeners for shared state.
- The unified transport does not change actor ownership rules: in-process RPC waits for the real result without a timeout, and dropping a caller does not cancel owner-started work. Network/IPC and connection-draining deadlines stay at their respective boundaries. A transport timeout does not prove cancellation or safe retry.
- Verify changed shared commands on both transports, including error mapping, query/mutation classification, HTTP capability restrictions, and owner isolation where applicable.
