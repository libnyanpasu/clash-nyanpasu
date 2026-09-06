# ProxiesActor migration

## Scope and acceptance

1. Make the runtime proxy/provider response model accept missing extensions and
   partial subscription usage without losing unknown enum strings. Test wire
   decoding, port-independent paths and signed history delays.
2. Keep proxy/group assembly a pure service. Replace ProxiesGuard with an injected
   ProxiesActor owning the cache, freshness, refresh timer and subscriptions.
3. Move list/read/refresh/select/provider-update calls to NyanpasuClient. Retire
   legacy proxy REST functions and update frontend IPC, tray and cache warming.
4. Verify same-instance paired reads, cache expiry, invalidation, write ordering,
   post-write errors, shutdown and tray/IPC type checks. Deliver linked PRs.

## Ownership and lifecycle

CoreActor owns the revocable ApiClient. ProxiesActor owns an immutable published
snapshot and a three-second cache age; only its mailbox changes cache state.
The ten-second refresh loop waits for each refresh before scheduling the next,
so slow requests cannot accumulate timer messages. Actor teardown aborts its
background tasks. Typed clients use finite RPC timeouts.

A cache hit still checks the active CoreClient capability. Paired proxies/provider
reads run under one capability with a single postflight check, preventing mixed
process snapshots. Revocation clears the cache and notifies consumers; synchronous
tray projections also reject snapshots whose capability has been revoked.
Subscribers see immutable snapshots, not actor-owned state behind shared locks.

Selection and provider refresh execute once on a bound capability, invalidate
old cache, and refresh using that same capability. Errors after a successful
mutation explicitly report that it applied; no replay or rollback is attempted.

## Boundaries and policy

NyanpasuClient remains a facade. Its composition code injects CoreClient into
ProxiesActor. DTO conversion/group assembly are pure functions. Network I/O stays
inside the typed clash-api adapter; Tauri events and menu updates stay at the UI
boundary. The frontend's command names remain stable.

Proxy-change interruption reads the typed ClashConfig strategy, not a global.
Off skips closure; All closes all connections. The existing ProxyGroup fallback
to closing all is retained and documented, since chain-filtered closure is a
separate policy feature. Selection, closure and refresh share the same ApiClient.
Mode-change and other legacy policy paths remain outside this migration, except
that their proxy-cache warming is routed to the injected facade.

## Response compatibility

Keep name/type/history/udp required. Core-specific proxy extensions and provider
test metadata may be absent. Preserve unknown provider/vehicle strings. Validate
numeric conversions for the existing frontend DTO. Subscription usage retains
legacy PascalCase/lowercase aliases and default-zero missing counters. Configured
core kind/transport FeatureSupport cannot establish these response fields.

## Implementation and validation

- Added the injected ProxiesActor and retired ProxiesGuard plus legacy proxy REST
  functions. Removed the obsolete backoff/checksum and cache-warming helpers.
- IPC commands retain their names. Proxy selection now unwraps command errors in
  the frontend; actor change events refresh both proxy and provider queries.
  Tray selection runs asynchronously through the same facade and policy.
- Six actor tests cover shared cache/expiry, encoded mutation names, ordering,
  post-mutation refresh failures, idle and in-flight instance replacement,
  cancellation of queued mutations, and subscription shutdown.
- Application library tests: 476 passed, one ignored. Runtime clash-api tests:
  36 passed, one ignored. Runtime Clippy with warnings denied and application
  workspace Clippy/all targets/all features passed (existing application warnings).
- Generated IPC bindings preserve unknown provider/vehicle values as strings.
  Interface build, TypeScript checks and architecture ledger gate passed. No live GUI/core smoke test has been performed.

Runtime dependency: [nyanpasu-runtime #405](https://github.com/libnyanpasu/nyanpasu-runtime/pull/405),
pinned at `82092b0`. Merge that PR before the application migration.

## Remaining migration batches

1. Configuration PUT/PATCH and their reconciliation/lifecycle callers.
2. Remaining connection-interruption policy callers (including mode/profile
   changes and chain-aware ProxyGroup closure).
3. WebSocket stream ownership and transport through the typed runtime API.

Proxy/Provider REST callers are complete in this batch. Frontend REST hooks use
IPC; remaining stream work is in the backend connector, not frontend URL assembly.
