# Rule provider API migration

## Scope and checks

1. Preserve missing metadata and unknown enum strings in clash-api RuleProvider.
   Verify typed known variants, unknown-value round trips, invalid fields and
   HTTP map order.
2. Route provider listing through NyanpasuClient and the instance-bound ApiClient.
   Remove its legacy URL-building function. Verify revoked handles, ordered maps
   and conversion to the existing frontend DTO.
3. Run runtime and application tests, Clippy, formatting, generated binding and
   frontend type checks, then deliver linked runtime/application PRs.

## Response contract

Provider names remain required. Behavior, format, count, provider/vehicle type
and update time may be absent. Present values remain typed; timestamps must be
valid RFC3339. A malformed present timestamp is rejected, whereas the old
application DTO accepted any string. The facade formats valid timestamps as
RFC3339 and preserves their offset, though spelling may normalize (Z to +00:00).

Behavior/format and the shared provider/vehicle enums retain known variants and
store unknown values in Unknown(String), with lossless serialization and as_str
accessors. This intentionally removes Copy from those enums. The shared enums
also occur in proxy responses; their known variants retain the existing wire
representation. No proxy caller is migrated by this change.

Rule counts stay optional i64 in the protocol model. The existing frontend DTO
accepts u32, so conversion checks bounds and returns an error for negative or
oversized counts rather than truncating them. IndexMap order is retained all the
way to IPC; no unordered response map is introduced.

## Lifecycle and capabilities

The operation uses the existing ApiClient preflight/postflight checks and shared
revocation. No Service IPC contract or minimum-version change is needed.
Current FeatureSupport flags describe controller transports, not provider fields;
response decoding cannot infer complete metadata from those capabilities.

## Remaining migration

This completes the rule-provider listing path after rule reads and provider
refresh were migrated in the previous PR. Config responses/writes, proxy cache
ownership and workflows, policy-triggered actions, and guarded streams remain.

## Delivery and validation

Runtime dependency: [nyanpasu-runtime #403](https://github.com/libnyanpasu/nyanpasu-runtime/pull/403), pinned at `28df6d9`. Merge that PR first. Service rc.3 remains sufficient.

Validation on macOS:

- Runtime: 20 unit, 5 REST, 2 stream and 5 traffic tests passed; the existing
  real-Mihomo test remains ignored. Clippy across all targets passed.
- Application: full-workspace/all-feature tests passed; application library
  468 passed, 1 existing ignored. Tests cover provider ordering, revoked reads,
  lossless DTO conversion and invalid count bounds. Full-target/full-feature
  Clippy passed with existing warnings.
- Specta binding export produced no diff. Architecture ledger, frontend types,
  Oxlint and Rust formatting passed. The isolated worktree dependencies were
  refreshed from the frozen lockfile after main's table-library upgrade.
- Windows transports and live-core behavior were not exercised locally.
