# OpenAPI P0 feasibility probe

This isolated probe checks whether a Rust Axum API can produce an OpenAPI JSON contract, dispatch through an in-process HTTP router, and generate a TypeScript/TanStack Query client whose transport can be configured with Tauri `invoke`. It is not a production API and does not change existing commands or bindings.

The Rust `FakeApplication` only stands in for the injected `NyanpasuClient`; the route handlers call its methods rather than owning production state. The Rust bridge DTOs match the frontend transport request/response shape. `tauri-host.ts` adapts Tauri's `invoke` to the generic client transport; a production Tauri command would provide the real `api_dispatch` command and application router.

## Verify

From the repository root:

```sh
cargo test --manifest-path docs/probes/openapi-p0/Cargo.toml --locked
cargo run --manifest-path docs/probes/openapi-p0/Cargo.toml --example export_openapi
pnpm --dir docs/probes/openapi-p0/frontend install --frozen-lockfile
pnpm --dir docs/probes/openapi-p0/frontend run generate
pnpm --dir docs/probes/openapi-p0/frontend run typecheck
pnpm --dir docs/probes/openapi-p0/frontend run smoke
```

The generated `openapi.json` and TypeScript client are checked-in probe artifacts. Regenerate them after changing Rust API routes or DTOs.

## Scope and limits

- The API returns typed profile DTOs, accepts a JSON mutation body, and documents a structured 404 response.
- The Rust test exercises OpenAPI generation, normal in-process Axum dispatch, and dispatch through a Tauri-shaped request/response adapter without opening a port.
- Orval generates TanStack Query v5 hooks and client code. The adapter changes the transport while generated call sites remain stable.
- The TypeScript smoke check verifies GET query strings, JSON mutation bodies, and structured responses against a fake transport. Typechecking also checks the adapter against the real Tauri `invoke` signature.
- The adapter is JSON/UTF-8 only and caps response bodies at 1 MiB. Tauri invoke does not expose HTTP streaming or abort an in-flight Rust command, so event streams and cancellation need separate APIs.
- The probe does not prove that the current `NyanpasuClient` can run outside Tauri. It is currently in `backend/tauri`; extracting the application composition root and host adapters remains a separate prerequisite for OpenWrt.
- The mock's in-memory lock is test scaffolding only. Production mutable state must remain owned by the existing actors/clients, not by the HTTP handler.
