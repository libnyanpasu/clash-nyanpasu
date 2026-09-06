# Clash rule API migration

## Scope and verification

1. Make clash-api rule extensions optional without inventing values. Verify base,
   extended and malformed responses through an HTTP fixture.
2. Migrate rule reads and provider refresh through NyanpasuClient and the existing
   instance-bound ApiClient. Verify revocation and encoded provider names with a
   fake endpoint. Remove the corresponding legacy URL-building functions.
3. Run Rust tests, bindings generation, frontend types and the architecture gate;
   deliver runtime and application PRs with the dependency explicitly pinned.

## FeatureSet boundary

The current runtime metadata exposes FeatureSupport::features as an EnumSet.
Its features describe named-pipe IPC, Unix-socket IPC and disabling the TCP
controller. None describes rule indices, sizes, counters or provider metadata.
These flags cannot select a response schema safely.

Keep required common fields (type, payload, proxy) strongly typed and required.
Represent optional index/size fields explicitly as Option, as extra already is;
missing is not zero. Reject invalid types even for optional fields. The existing
UI consumes only common rule fields, so its DTO remains stable.

Future API-specific features should describe operations with verified core/version
support (for example rule disabling). They can gate those operations, but must
not synthesize absent response data or skip wire validation. Bind such metadata
to the same applied instance as the ApiClient, not the configured core choice.

Runtime dependency: [nyanpasu-runtime #402](https://github.com/libnyanpasu/nyanpasu-runtime/pull/402), pinned at `05fb304`. Merge it before the application PR.

## Service and lifecycle

This change requires a newer clash-api source dependency but introduces no new
Service IPC contract; rc.3 remains sufficient. Both operations use the existing
preflight/postflight authority checks, timeout and shared revocation. Provider
refresh is a mutation and is not retried automatically.

## Remaining callers

Provider listing still needs response-model work: optional metadata and unknown
string enum values must retain their meaning across cores. Config, proxy/cache,
policy and stream migration remain as listed in the preceding plan. This PR
migrates the complete rule-read and explicit rule-provider-refresh call paths.

## Validation

- Runtime: 19 unit, 4 REST, 2 stream and 5 traffic tests passed. The existing
  real-Mihomo integration test remains ignored; its index consumer was updated
  and compiled. Runtime Clippy across all targets and formatting passed.
- Application: full-workspace tests with all features passed; application library
  465 passed, 1 existing ignored. The added HTTP test verifies base-rule decoding,
  encoded provider names and rejection of both operations after revocation.
- Application full-target/full-feature Clippy passed with existing warnings.
- Specta binding export produced no diff. All three frontend TypeScript checks,
  Prettier on touched documents/bindings, Oxlint, Rust formatting and the exact
  architecture ledger gate passed.
- Validation ran on macOS. Real-core and Windows transport behavior were not
  exercised in this follow-up.
