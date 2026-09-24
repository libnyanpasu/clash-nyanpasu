# JSON-RPC/OpenRPC internal RPC probe

This isolated probe validates a contract-first OpenRPC document, Rust JSON-RPC method/subscription dispatch with `jsonrpsee`, TypeScript client generation, and Tauri IPC transport shapes. The current goal is to decouple application methods and domain events from Tauri commands/events inside the software. It does not test Electron, HTTP, Unix sockets, LuCI, or OpenWrt adapters, and it does not modify production commands, hooks, plugins, or generated bindings.

`openrpc.json` is the probe's API contract. Rust registers methods and subscriptions on an in-process `RpcModule`. Unary calls pass a serialized JSON-RPC request through the `rpc_dispatch` bridge. A long-lived subscription uses `rpc_subscribe`; the backend forwards JSON-RPC notifications through a Tauri `Channel`, and the frontend cancels it with the JSON-RPC unsubscribe method. The Rust test uses Tokio `mpsc` as a stand-in for the native `Channel` sink; the TypeScript smoke test constructs the actual `@tauri-apps/api/core` `Channel` and mocks only Tauri's callback/invoke runtime.

The follow-up probe derives JSON-RPC names from Rust operation paths such as `profiles::list`. Registration checks that the path resolves to a function, while the existing method-list check catches drift against the OpenRPC document. The OpenRPC schema and method metadata still require explicit maintenance.

## Verify

From the repository root:

```sh
cargo fmt --manifest-path docs/probes/openrpc-p0/Cargo.toml -- --check
cargo test --manifest-path docs/probes/openrpc-p0/Cargo.toml --locked
pnpm --dir docs/probes/openrpc-p0/frontend install --frozen-lockfile
pnpm --dir docs/probes/openrpc-p0/frontend run generate
pnpm --dir docs/probes/openrpc-p0/frontend run typecheck
pnpm --dir docs/probes/openrpc-p0/frontend run smoke
```

The OpenRPC generator emits a method client and schema types under `frontend/generated/`. Its current TypeScript template writes dotted method names as invalid member names and has a runtime validator mismatch for `by-name` parameters. `postprocess-generated.mjs` fixes the member syntax and type-only import; the probe uses `by-position` for the activation parameter. Keep this workaround under tests if the generator remains in use, and re-evaluate it when upgrading the generator.

## Verified

- `jsonrpsee` 0.26.0 can dispatch registered methods in-process through `RpcModule::raw_json_request`; no HTTP listener is required.
- The probe uses `server-core` and `jsonrpsee-types` without enabling the HTTP server feature. The normal dependency tree does not pull `hyper` or `jsonrpsee-server`.
- Rust tests compare the OpenRPC method list with the registered method list, assert list/activation wire shapes and structured application errors, verify `rpc.discover`, and exercise subscription notification envelopes, unsubscribe cancellation, and the Tauri-shaped stream forwarding seam.
- The bridge-shaped dispatcher caps each JSON-RPC request and response at 1 MiB; `raw_json_request`'s separate buffer argument is only the subscription notification queue size.
- The generated TypeScript method client works with the Tauri-shaped unary request/response bridge after the generator workaround. Typed calls map to `invoke("rpc_dispatch", { request })`.
- The TypeScript subscription client uses the real Tauri `Channel` class and maps subscription setup to `invoke("rpc_subscribe", { request, notifications })`; it maps cleanup to `rpc_dispatch` with the OpenRPC unsubscribe method.
- The Channel smoke test verifies callback registration, IPC channel serialization, notification parsing, out-of-order callback delivery handling, unsubscribe, and callback cleanup.

## Limits and production implications

- The method-list check does not prove that every OpenRPC JSON Schema constraint matches every Serde DTO. Production should generate DTOs from the spec where practical or add schema validation tests for representative payloads and Serde edge cases.
- The OpenRPC client generator does not generate React Query hooks or project-specific invalidation rules. Those need a small, separately generated or handwritten hook layer.
- The generator treats the documented subscribe/unsubscribe methods as ordinary unary methods and ignores the `x-subscriptions` relationship. The stream lifecycle and notification types need a project-owned typed wrapper or generator extension.
- The custom transport subclasses an internal `@open-rpc/client-js` transport module because the package does not export the base transport from its public entry point. That is a fragile integration seam. A project-owned small request client/template may be safer than depending on this internal module.
- The Tauri bridge remains mock-shaped rather than a registered production Tauri command. The native backend `Channel` lifecycle is represented by the Tokio sink test, not exercised inside a running Tauri application.
- No production `NyanpasuClient` operation is connected. This proves a feasible internal RPC/IPC seam, not that the current production application graph is Tauri-free.
- No frontend Tauri plugin has been migrated; this probe covers RPC calls and domain subscriptions, not desktop host capabilities.
- No HTTP, WebSocket server, Unix socket, Electron, LuCI, or OpenWrt adapter was implemented or tested; those are outside this probe's current acceptance scope.
