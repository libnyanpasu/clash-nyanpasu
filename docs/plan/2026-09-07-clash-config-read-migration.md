# Config read migration

## Scope and verification

1. Make RuntimeConfig top-level fields optional so partial core responses retain
   absence. Cover known/unknown response enums, malformed values and zero/false.
2. Keep mutation/subscription enums closed. Check core-manager runtime projection
   against missing fields so absence cannot confirm a successful hot patch.
3. Migrate GET /configs through NyanpasuClient and the bound ApiClient, deleting
   the old request function. Verify revocation, port bounds and stable IPC types.
4. Run affected runtime suites, application regression tests, Clippy, frontend
   types, architecture ledger and formatting; deliver linked PRs.

## Response contract and capabilities

RuntimeConfig previously required the complete Mihomo top-level response. All
its top-level fields now preserve absence with Option and omit absent fields
when serialized. Present nested structures retain their existing models/defaults;
this does not claim that every field inside a returned TUN object is observed.
Present known fields remain validated, including extensions the old application
DTO ignored. A malformed extension can therefore reject a response previously
accepted by the smaller DTO. Unknown unmodeled JSON fields remain ignored.

ConfigEnum<T> is a response-only string fallback around TunnelMode and LogLevel.
Known strings stay typed, unknown strings round-trip; ConfigPatch and LogQuery
keep their existing closed enums. FeatureSupport currently describes transports,
so it cannot guarantee response field presence. Future API-specific feature gates
must remain bound to the applied instance and cannot replace wire validation.

The facade preserves the existing ClashConfig JSON shape, including optional
socket-port, external-controller and secret when the core actually returns them.
It does not fill these from the private capability binding or configuration.
All six port fields are range-checked against the existing u16 DTO contract.
No new Service IPC or minimum version is required.

## Remaining migration

Config writes remain in existing state/apply workflows; moving them needs to
preserve state commits and post-commit side-effect semantics. Proxy/cache ownership,
policy-triggered actions and guarded subscriptions remain in the migration queue.

## Delivery and validation

Runtime dependency: [nyanpasu-runtime #404](https://github.com/libnyanpasu/nyanpasu-runtime/pull/404), pinned at `44a8c54`. Merge it before the application PR.

Validation on macOS:

- clash-api: 20 unit, 7 REST, 2 stream and 5 traffic tests passed; the existing
  real-Mihomo test remains ignored.
- core-manager: 10 configuration projection tests and 17 graceful-switch tests
  passed. Clippy across both affected runtime crates and all targets passed.
- Application full-workspace/all-feature tests passed; application library:
  470 passed, 1 existing ignored. Tests cover the partial read path, revocation,
  absence, unknown strings and all six port bounds.
- Generated IPC bindings have no diff. Frontend type checks, Oxlint, formatting
  and the architecture ledger passed.
- Windows transport behavior was not executed locally.
