# Profile Inspect runtime snapshots

## Scope and assumptions

Show the latest promoted runtime pipeline in the existing Profile Inspect route, read-only, including line diffs. Promoted means generated/published, not necessarily applied by the core. No preview execution, historical archive, editing, rollback, or new actor is needed.

## Plan

1. Preserve the pipeline graph and step logs in the immutable runtime snapshot owned by the existing lifecycle. Project a narrow inspection read model through NyanpasuClient and thin Tauri commands. Fetch metadata first and YAML/logs for one node on demand. Reject node requests after the selected snapshot is replaced; never mix builds.
   Verification: projection tests cover branches, logs, missing nodes, and stale snapshot requests.
2. Replace the Profile Inspect placeholder with a step browser, source metadata, changed fields, highlighted read-only YAML and diff, logs, refresh, and loading/error/empty states. Use generated TypeScript bindings and explicit query keys per snapshot/node.
   Verification: TypeScript checks, formatting/lint, and frontend build.
3. Run focused Rust tests and regenerate bindings, inspect the final diff, then create one atomic feature commit and a PR against main with validation evidence and limitations.

## Service classification

The existing actor owns the runtime lifecycle; immutable snapshot projection is a pure operation; Tauri commands remain boundary adapters. No global service or compatibility bridge is introduced.

## Implementation and validation

Completed the read-only facade/IPC path and Profile Inspect browser. The runtime retains the materialized graph and logs by ownership transfer, shared through an immutable Arc; node YAML is serialized on a blocking worker only when requested. Each build gets an opaque inspection ID so identical products from different lifetimes cannot alias. The UI keeps one build selected until refresh and never treats it as confirmation of core application.

Passed on macOS:

- `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib runtime_inspection --offline` (5 tests, including real build/facade integration with injected adapters).
- The `client::runtime` test filter (16 tests), `enhance::artifact_bridge`, and `export_typescript_bindings`.
- `cargo clippy --manifest-path backend/Cargo.toml -p clash-nyanpasu --lib --tests --offline` (existing repository warnings remain).
- Interface and application TypeScript checks, affected-file Prettier/Oxlint, Cargo formatting, and architecture ledger gate.
- `pnpm web:build`.
- Headless Chrome with a temporary isolated harness and mocked IPC: node selection, YAML/logs, stale request errors, refresh/reset, empty/error states, and 390px layout without horizontal overflow. The harness was removed after validation.

The integration fixture binds local ephemeral ports, and existing compile-time JS embedding fetches CDN modules; those checks require network/local-port access. Browser checks use mock IPC, not a live Tauri/core session. Retaining materialized snapshots increases memory with pipeline size; no history is retained. The graph represents branches, not a chronological execution trace; changed-fields metadata may be absent for unchanged steps or independent baselines.

## YAML highlighting and diff display

Use a container-width breakpoint (40rem of available inspection width) for the step/content columns. Reuse the existing Shiki 4.4.3 highlighter with lazy YAML grammar loading and light/dark themes; show plain text while highlighting loads or if it fails. Source matching and cancellation prevent an old highlight result from appearing after a node switch.

Preserve SnapshotBaseline in the materialized graph and expose backend-computed comparison hunks in the same node-content response as the current YAML. Independent roots have no diff; identical before/after text has no changes. Stored archive layout/version is unchanged.

The default view is a unified YAML line diff with red deletions, green additions, old/new line numbers and three context lines per hunk. A pure snapshot helper computes the hunks on the backend blocking worker with Similar/Myers and a 200ms search budget; the frontend only renders the returned lines. The budget limits the diff search, not serialization, IPC, or rendering. Full YAML remains available with Shiki highlighting.

The node summary carries has_logs, which only counts non-empty entries for that node's semantic key. By default the navigation shows nodes with changed fields or logs. An explicit full-process-chain switch reveals all nodes; selection falls back to the first visible node when filtering hides it. An empty filtered chain does not fetch node content and offers the full-chain switch.

Additional validation: 23 domain snapshot tests with persistence enabled; 5 inspection tests; regenerated bindings; TypeScript, Oxlint and Stylelint; headless Chrome verifies syntax tokens, theme colors, container-width columns, default diff, red/green rows, old/new line numbers, log-only/quiet filtering, full-chain toggle and selection fallback, independent roots, refresh/stale handling, and narrow layout.

## Backend diff and ordering investigation

1. Move YAML line comparison into a pure snapshot helper, keep IPC behind the facade and its existing blocking worker, remove frontend jsdiff. Verify hunk offsets, unchanged/independent nodes, and 10,000-line inputs.
2. Reproduce spurious ordering changes in snapshot materialization. JSON Patch remove uses swap_remove; object equality also ignores intentional reorder-only changes. Keep a full keyframe when patch replay cannot preserve the executor's exact order, including nested objects. Verify filtering preserves source order and snapshot round trips preserve order.
3. Regenerate bindings, validate frontend/build and browser rendering, then update the review PR.

The whitelist retains DEFAULT_FIELDS plus configured valid entries from OTHERS_FIELDS, removes top-level guarded/unknown fields, and preserves retained order. GuardOverrides reapplies application-owned values. Finalizing intentionally sorts top-level keys as HANDLE_FIELDS, OTHERS_FIELDS, DEFAULT_FIELDS, then remaining keys. That real final ordering remains visible; the snapshot fix removes artificial reorders introduced by delta storage. The archive schema is unchanged; previously encoded deltas cannot recover order that was never stored.

Backend-diff validation passed: 141 domain tests with persistence, 5 facade inspection tests, binding generation, TypeScript/Oxlint, production build, and the mocked-IPC browser suite. The 10,000-entry whitelist fixture produces only `-mode: rule`, with no block moves. Debug diff-only timings were approximately 33–34ms for two edits and 35–38ms for complete replacement; these are local observations, not latency guarantees.
