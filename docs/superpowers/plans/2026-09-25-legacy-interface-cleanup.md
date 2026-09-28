# Legacy interface cleanup (actor-migration PR-7 + selective TCC T10/T11)

Date: 2026-09-25
Baseline: `main @ 4f59ca781` (TCC T0–T9 merged via #5320–#5324, #5366–#5368, #5374)
Worktree: `/Users/a632079/Programs/clash-nyanpasu-legacy-cleanup`

## Spec

Authority order: approved design/spec > roadmap > this plan > code comments.

- `docs/design/actor-migration-roadmap.md` — §1 (locked principles), §8 PR-7a/PR-7b (deletion lists and order), §9 (outcome model), §11–§12 (ledger gate, final success criteria).
- `docs/plan/2026-09-14-application-workflow-selective-tcc-v2.md` — §8 (post-commit effects), §10 (GUI/tray notification), §11 (startup, shutdown, recovery), §12 T10/T11, §13 V36–V40.
- `AGENTS.md` — architecture rules (§5–§16) and commit rules (§18).

Goal: remove every remaining legacy interface — `Config::global()/verge()/clash()`, `Draft<T>`, the `bridge/` mirrors, legacy `IVerge`/`IClashTemp` IPC DTOs, `feat.rs` orchestration, service `::global()` accessors and mutable service statics, the legacy startup effects pipeline and the duplicate mechanisms T11 names — and deliver the result as a stack of PRs whose every commit builds.

## Global Constraints

1. Follow `AGENTS.md`: no new `::global()`/`OnceCell`/`OnceLock`/`Lazy` service state; dependencies explicit (constructor/builder/actor args); Tauri types only in boundary adapters; `NyanpasuClient` stays a facade (no service lookup, no raw `ActorRef` exposure); no compatibility layers — prefer migratable breaking changes and migrate callers in the same task.
2. IPC wire changes are allowed only when every frontend caller is migrated in the same commit. `frontend/interface/src/ipc/bindings.ts` is regenerated, never hand-edited: `cargo test --manifest-path backend/Cargo.toml --all-features -p clash-nyanpasu export_typescript_bindings`.
3. Every commit must build: `cargo check --manifest-path backend/Cargo.toml --all-targets --all-features` and, if frontend files or bindings changed, `pnpm typecheck`. The pre-commit hook (lint-staged: clippy, rustfmt, prettier, oxlint, tsc) must run — never `--no-verify`.
4. Before reporting a task done, run and report: `cargo test --manifest-path backend/Cargo.toml --all-features -p clash-nyanpasu --lib -- --test-threads=8` (plus any other crate the task touched), `pnpm lint:architecture-ledger`, `pnpm test:architecture-ledger` if the ledger script changed, and for frontend changes `pnpm typecheck`, `pnpm test:frontend`, `pnpm exec oxlint <changed files>`, `pnpm exec prettier --check <changed files>`.
5. Architecture ledger: `scripts/architecture-ledger.ts` has no write mode. When a task lowers a metric, hand-edit `scripts/architecture-ledger.snapshot.json` to the exact numbers `pnpm architecture-ledger` reports and confirm `pnpm lint:architecture-ledger` exits 0. Metrics may only go down; bridge files may only be removed.
6. Tests never touch real user directories, the real system proxy, real global shortcuts or real core binaries. Use injected `PathResolver`/`RuntimePaths`, fake ports and TempDir.
7. Never modify `backend/nyanpasu-runtime` (upstream submodule).
8. Work only in the worktree above, on the branch named by the task. No push, no PR creation, no rebase of other branches. Stage explicit paths only; one commit = one indivisible change; no fix-up commits — fold review fixes into the owning commit (`git commit --fixup=<sha>` then `GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash <task-base>`), only on this task's own unpublished commits.
9. Commit messages follow `AGENTS.md` §18: imperative subject ≤ 72 chars, body explains why for non-trivial changes, no file-by-file lists, no "tests pass" lines.
10. Behavior outside the task scope must not change. Where the task removes a legacy path whose behavior differs from the typed path, the typed path's behavior wins and the difference is listed in the report.

## PR stack

| Layer | Branch                               | Base   | Tasks   | Theme                                                                       |
| ----- | ------------------------------------ | ------ | ------- | --------------------------------------------------------------------------- |
| L1    | `refactor/typed-config-ipc`          | `main` | 1, 2    | Typed config IPC; frontend leaves `IVerge`                                  |
| L2    | `refactor/remove-legacy-config`      | L1     | 3, 4    | PR-7a: legacy readers → typed; delete mirrors, `Config`, `Draft`, `bridge/` |
| L3    | `feat/tcc-startup-shutdown`          | L2     | 5, 6, 7 | TCC T10: StartupReconcile, producer gating, ordered shutdown                |
| L4    | `refactor/inject-app-infrastructure` | L3     | 8, 9    | PR-7b: `Handle`/`WindowManager`/tray/logging/infra statics                  |
| L5    | `refactor/tcc-t11-cleanup`           | L4     | 10, 11  | TCC T11 deletions, docs, final ledger gate                                  |

Out of scope (record, do not implement): the `nyanpasu-runtime` submodule marker (`crates/nyanpasu-core-manager/src/spec.rs:43`); manual GUI/OS smoke (maintainer attestation); `enhance/artifact_bridge.rs` (not a legacy mirror — it maps executor artifacts to `PostProcessingOutput`); `after_commit` renames; cross-window settings sync.

---

### Task 1: Typed configuration IPC (backend, additive + renames)

Branch `refactor/typed-config-ipc` (create from `main @ 4f59ca781`). The plan document itself is committed first on this branch as `docs: plan the legacy interface cleanup stack` (already written at `docs/superpowers/plans/2026-09-25-legacy-interface-cleanup.md`).

Context: the frontend reads and writes all settings through the legacy `get_verge_config`/`patch_verge_config` IPC (`IVerge`). `NyanpasuClient` already has typed `get_app_config`, `patch_app_config`, `get_clash_config`, `patch_clash_config`, `patch_runtime_overrides` (`backend/tauri/src/client/mod.rs` ~469–730) but none is exposed as IPC. The existing IPC `patch_clash_config` (`backend/tauri/src/ipc.rs` ~451) actually patches runtime overrides via a whitelisted `PatchRuntimeConfig` DTO and silently drops unknown fields such as `secret`/`external-controller`. `get_clash_info` (`ipc.rs` ~321) reads `Config::clash().latest().get_client_info()`.

Requirements:

1. Add Tauri commands (registered in `backend/tauri/src/specta_export.rs`) that are thin adapters over `NyanpasuClient`:
   - `get_app_config() -> NyanpasuAppConfig`
   - `patch_app_config(patch: NyanpasuAppConfigPatch) -> MutationOutcome<()>`
   - `get_clash_config() -> nyanpasu_config::clash::config::ClashConfig`
   - `patch_clash_config(patch: ClashConfigPatch) -> MutationOutcome<()>` (typed ClashConfig patch; the name now matches the facade method)
   - `patch_runtime_overrides(patch: ClashGuardOverridesPatch) -> MutationOutcome<()>` — replaces the old overrides command. Keep an explicit-field tracing line that logs which fields are present and never logs the `secret` value.
     SessionState IPC is not added (no frontend consumer; window geometry is saved by the backend).
2. `get_clash_info` keeps its name and `ClashInfo { port, server, secret }` wire shape, but is served by a new facade method (e.g. `NyanpasuClient::clash_info()`), derived from typed sources only: `port`/`server` from the confirmed `session_ports()` binding when present, otherwise from the typed `ClashConfig` (`mixed_port.start_port`, `external_controller.host:port.start_port`); an unspecified host (`0.0.0.0`, `::`) maps to `127.0.0.1` exactly as the legacy `guard_client_ctrl` did; `secret` from the typed overrides. Add read-only getters on `ClashGuardOverrides` (`secret()`, `mode()`) in `backend/nyanpasu-config` — getters only, no new mutators. Move the `ClashInfo` struct out of `config/clash` to the client/IPC layer. Unit-test the derivation: confirmed binding wins, fallback to configured values, unspecified host normalization.
3. Every IPC signature that still mentions a legacy `crate::config::nyanpasu::*` type (e.g. `ClashCore` in `change_clash_core`, `fetch_latest_core_versions`, `get_core_version`, `update_core`, and any other) switches to the typed `nyanpasu_config` equivalent when the serde wire values are identical; if a legacy type has no identical typed equivalent, keep it and list it in the report. `get_verge_config`/`patch_verge_config` stay untouched in this task (Task 2 deletes them).
4. Specta export must produce no ambiguous TS names. Typed domain types keep their natural names. Resolve collisions by renaming the other side's TS name with `#[specta(rename = "...")]` (for example the clash-API `/configs` DTO in `core/clash/api.rs` becomes `ClashApiConfig`; legacy `config::nyanpasu` types still exported only because `IVerge` is still exported get a `Legacy` prefix, which disappears with `IVerge` in Task 2).
5. Update the frontend callers that the renames break so `pnpm typecheck` stays green in this commit: `frontend/interface/src/ipc/use-clash-config.ts` switches to `patchRuntimeOverrides`; consumers of renamed TS types are updated mechanically. No other frontend behavior change in this task.
6. Regenerate bindings (Global Constraint 2) and commit them with the backend change.

Acceptance: all Global Constraint 4 checks green; new unit tests for `clash_info` derivation; `rg -n "Config::clash\(\)" backend/tauri/src/ipc.rs` returns nothing; bindings export `NyanpasuAppConfig`, `NyanpasuAppConfigPatch`, typed `ClashConfig`, `ClashConfigPatch`, `ClashGuardOverridesPatch`.

### Task 2: Frontend moves to typed settings; delete the legacy verge IPC

Branch `refactor/typed-config-ipc` (continues Task 1).

Context: 46 `useSetting(key)` call sites (32 distinct `IVerge` keys) plus `useSettings()` in the language provider and `WindowReveal` (`frontend/nyanpasu/src/pages/__root.tsx`), all backed by `getVergeConfig`/`patchVergeConfig` (`frontend/interface/src/ipc/use-settings.ts`). Legacy→typed key mapping is defined by `backend/tauri/src/bridge/{mod,verge,clash,mapping}.rs`. 20 keys belong to `NyanpasuAppConfig`, 12 to typed `ClashConfig`, none to SessionState.

Requirements:

1. `useSettings()`/`useSetting(key)` read and patch typed `NyanpasuAppConfig` via `getAppConfig`/`patchAppConfig`; add `useClashSettings()`/`useClashSetting(key)` for typed `ClashConfig` via `getClashConfig`/`patchClashConfig`. Both return the full `MutationOutcome` from `mutationFn` (the committed-degraded toast depends on it) and keep the current `{ value, upsert, isPending, refetch }` surface. Keys use typed field names (`clash_core` → `core`, `clash_tray_selector` → `tray_selector_mode`, etc.).
2. Migrate every consumer. Shape changes must preserve today's UI semantics:
   - `network_statistic_widget`: flat `'disabled'|'large'|'small'` → tagged `{kind:'disabled'} | {kind:'enabled', value:'large'|'small'}`.
   - `verge_mixed_port` + `enable_random_port` → `mixed_port: PortStrategy { kind, start_port }`; the random switch toggles `random` ↔ `allow_fallback` (the legacy mapping of `false`), the port input writes `start_port` and preserves `kind`.
   - `clash_strategy` → `external_controller.port.kind` (preserve `host` and `start_port`).
   - `break_when_{proxy,profile,mode}_change` → `break_connection.{on_proxy_change (off|proxy_group|all), on_profile_change, on_mode_change}`.
   - `theme_color` typed value is a non-empty CSS color; `system_proxy_bypass` empty is `""` not `null`.
   - `core-secret-config.tsx` writes `secret` through `patchRuntimeOverrides`; `external-controller-config.tsx` writes `external_controller` through `patchClashConfig` (both were silently dropped before — list this fix in the report).
   - `clash_core` reads use `core`; writes keep going through `changeClashCore`.
     Pure shape conversions live in small helpers with unit tests.
3. Keep both startup gates: `WindowReveal` reveals once the app-config query settles; the language provider renders after it settles.
4. `frontend/interface/src/provider/mutation-provider.tsx`: `nyanpasu_config` refetches `getAppConfig` (+ `getSysProxy`); `clash_config`/`profiles` refetch `getClashConfig`, `getClashInfo` and the existing keys. `use-clash-cores.ts` invalidates `getAppConfig`. `frontend/nyanpasu/src/components/configuration-status.tsx` `CONFIGURATION_MUTATIONS` replaces `patchVergeConfig` with `patchAppConfig`, `patchClashConfig`, `patchRuntimeOverrides`.
5. Delete: the `get_verge_config`/`patch_verge_config` commands and their specta registration; the Tauri-managed `LegacyVergeBridge` IPC instance (`backend/tauri/src/setup.rs` `app.manage`) and the IPC-only code in `bridge/verge.rs` (patch validation/routing, `get_verge_config` projection). The actor-side mirror (`VergeLegacyBridge` impl and `prepare`) stays until Task 4. Also delete the unused frontend `VergeConfig` type (`frontend/interface/src/service/types.ts`) and the unused `useSetting` import in `frontend/nyanpasu/src/components/settings/system-proxy.tsx`. After this task no `IVerge`/`Legacy*` TS type is exported.
6. Tests: a vitest browser test (pattern: `frontend/interface/tests/release-channel.browser.test.tsx`, mocked `invoke`) covering typed read, patch payload shape and outcome passthrough for both hook families; unit tests for the conversion helpers.

Acceptance: Global Constraint 4 checks green, plus `pnpm web:build`; `rg -n "getVergeConfig|patchVergeConfig|IVerge" frontend --glob '!**/node_modules/**' --glob '!**/dist/**'` returns nothing; ledger snapshot lowered.

### Task 3: Backend legacy readers read typed state

Branch `refactor/remove-legacy-config` (create from the L1 head).

Context (production readers of the legacy mirror, all with typed equivalents):

- Tray: `core/tray/mod.rs` (`tray_menu_mode`, `clash_core`, `enable_system_proxy`, `enable_tun_mode`, `enable_tray_text`), `core/tray/proxies.rs` (`clash_tray_selector`, `TODO(actor-migration)` at line 1), clash mode via `utils/config.rs::get_current_clash_mode`.
- `utils/config.rs::get_self_proxy` (used by `check_update` and `utils/candy.rs::get_reqwest_client` → `server/mod.rs`, `utils/net.rs`).
- `feat.rs::copy_clash_env` (IPC and tray).
- `window.rs` ~489 (`always_on_top` default at window creation).
- `utils/resolve.rs`: `enable_silent_start` (~219), `MainWindow::get_window_state` (~310, reads `window_size_state`), the resize projection `set_window_state` (~314, `TODO(actor-migration)` at ~451), `tray_menu_close_behavior` (~553), the session-port write-back into legacy mirrors (~168–188, `FIXME(actor-migration)`).
- `lib.rs` ~225 locale from `Config::verge()`; `utils/init/logging.rs` ~142–147 thread reading legacy log settings.

Requirements:

1. Tray consumes a typed view, not globals: the effects plan (`client/effects/plan.rs`) already projects `TrayMenuDesired`/`TrayPartDesired`; extend the projection with the missing typed inputs (`NyanpasuAppConfig.core`, clash `mode` via the Task 1 getter) and pass the desired view to the `TrayRefresher` port (`client/ui_effects/ports.rs`). `TauriTrayRefresher` stores the latest view in the Tauri-managed `TrayState`; every tray builder/updater and proxy-selector check reads only that cached view. The tray's own re-render triggers (proxy changes) reuse the cached view. Seed the view before the tray is first built from the current typed snapshots, so the tray never renders from defaults while a typed config exists. A tray refresh that arrives before the tray exists logs and returns instead of panicking (`expect("tray not found")` today). Remove the `TODO(actor-migration)` in `core/tray/proxies.rs`.
2. `get_self_proxy`, `copy_clash_env`, `check_update`, `get_reqwest_client`: take the port from an injected source (confirmed `session_ports()` mixed port, falling back to typed `ClashConfig.mixed_port.start_port`) passed by the caller; no global lookups. The port-selection rule is a pure function with a unit test.
3. `window.rs` default `always_on_top`, `resolve.rs` silent start and tray-menu close behavior read the typed `NyanpasuAppConfig` snapshot. `MainWindow` restore geometry reads the typed SessionState snapshot (`window_state` for the main label). Delete the in-memory resize projection and its `TODO` (geometry is persisted by `save_main_window_geometry` on close/cleanup). Delete the session-port write-back block and its `FIXME`.
4. Locale: set it from the typed app-config snapshot in the composition root right after `NyanpasuClient` is constructed and before any tray/window is built; remove the `Config::verge()` read in `lib.rs`. Delete the legacy log-settings thread in `utils/init/logging.rs`; the typed logger effect already applies settings during startup reconcile (report the brief default-level window this leaves).
5. No mirror deletion in this task (Task 4). After this task the only `Config::verge()/clash()` hits are in `bridge/`, `config/`, the dead `feat.rs::patch_clash*`, dead `client/rebuild.rs`, dead `core/tasks/jobs/logger.rs`.

Acceptance: Global Constraint 4 checks; unit tests for the tray view projection (core and mode included), the port-selection rule, and main-window geometry restore from SessionState; `rg -n "Config::(verge|clash)\(\)" backend/tauri/src` shows only the files listed in requirement 5; ledger snapshot lowered (config calls, markers).

### Task 4: Delete legacy mirrors, `Config`, `Draft` and `bridge/`

Branch `refactor/remove-legacy-config` (continues Task 3).

Context: roadmap §8.1 fixes the order — last production caller already migrated (Tasks 2–3) → delete prepared-mirror types and actor/client messages → simplify the three actors' commit paths. Inventory: `state/mirror.rs` (`PreparedLegacyMirror`, `PreparedTypedReplace`, `VergeLegacyBridge`, `WindowLegacyBridge`, `ClashLegacyBridge`), actor messages `PrepareReplace`/`ReplacePreparedIfVersion` in `state/{application,session_state,clash_config}.rs`, client wrappers `prepare_replace`/`replace_prepared_if_version`/`replace_if_version` in `client/{application,session_state,clash_config}.rs` (test-only callers), `LegacyBridgeSet` + `ClientSetupArgs.bridges` + `sync_legacy_mirrors` + legacy seeding in `client/mod.rs` (~93–173, ~279–285), `legacy_lock`/`ConfigLegacyVergeStore` in `setup.rs`, `config/core.rs` (`Config`), `config/draft.rs` (`Draft`), `bridge/*`. The legacy on-disk migration (`core/migration/modules/typed_config.rs`) still needs the `IVerge`/`IClashTemp` serde shape and the legacy→typed converters (`application_from_legacy`, `persistent_state_from_legacy`, `clash_config_from_legacy`, `typed_config_from_legacy_parts`).

Requirements:

1. Delete the mirror machinery listed above. Actors commit typed state directly; a missing typed file loads the typed default (migrations already create typed files). If `PersistentStateManager::replace_if_version` has a non-legacy production caller, keep a typed `ReplaceIfVersion { expected_version, state }` message; otherwise delete the conditional actor/client API (roadmap §8.1).
2. Move the legacy on-disk schema into `backend/tauri/src/core/migration/legacy_schema/` (module private to `core::migration`): the `IVerge`/`IClashTemp` structs and only the nested types their deserialization needs, the template/guard helpers, and the legacy→typed converters. Delete every typed→legacy projection (`apply_*_to_legacy_verge`, `legacy_iverge_from_typed`, `typed_patches_from_legacy_patch`, `TypedConfigPatchPlan`). Keep a migration test asserting every legacy field is either converted or explicitly discarded (replacing `bridge/mapping.rs` coverage).
3. Delete `bridge/`, `config/core.rs`, `config/draft.rs`, and legacy code that only existed for the mirror or is dead: `feat.rs::{patch_clash, patch_clash_with_rebuild, requires_core_restart}`, `client/rebuild.rs` (all entries are dead wrappers over `reconcile_core`; drop its tests with it), `core/tasks/jobs/logger.rs` (`ClearLogsJob`, never registered), `core/state.rs` `ManagedState` if still unused, dead `IClash*` structs and helpers in `config/clash/mod.rs`, `utils/resolve.rs::find_unused_port`, `utils/help.rs::get_clash_external_port` if dead, the legacy template writes in `utils/init/mod.rs`, `config/nyanpasu/{logging,clash_strategy}.rs` `impl IVerge` helpers, and `DegradationPhase::LegacyMirror` (never constructed; regenerate bindings; update `frontend/nyanpasu/src/pages/__root.tsx`).
4. Remaining uses of legacy `config::nyanpasu` enums outside the migration schema (e.g. `ClashCore`, `LoggingLevel`, `TrayMenuMode` in updater/core-lifecycle/UI adapters) switch to the identical typed `nyanpasu_config` types. After this task `crate::config` contains no `IVerge`/`IClashTemp`/`Draft`/`Config`; if the module becomes empty, delete it.
5. Ledger: in `scripts/architecture-ledger.ts`, exclude `backend/tauri/src/core/migration/legacy_schema/` from `legacy_dto_refs` through an explicit, commented allowlist entry (reason: on-disk upgrade input schema, not an application DTO) with a test in `scripts/architecture-ledger_test.ts`; drop removed bridge files; lower the snapshot.

Acceptance: Global Constraint 4 checks (migration tests included); `rg -n "PreparedLegacyMirror|PreparedTypedReplace|PrepareReplace|ReplacePreparedIfVersion|LegacyBridge|Config::(global|verge|clash)\(\)|\bDraft<" backend/tauri/src` returns nothing; `rg -n "\bIVerge\b|\bIClashTemp\b" backend/tauri/src` hits only `core/migration/`; ledger `config_calls` 0, `bridge_files` 0.

### Task 5: T10 lifecycle design (document only)

Branch `feat/tcc-startup-shutdown` (create from the L2 head). Output: `docs/superpowers/specs/2026-09-25-tcc-t10-lifecycle/design.md`, committed as `docs(tcc): design the T10 startup and shutdown lifecycle`. No code changes.

Context: TCC plan §11 and T10. Current state (main @ 4f59ca781):

- Startup: no `StartupReconcile`; `utils/resolve.rs::resolve_setup` runs `probe_service` → `restore_execution_host` → `reconcile_core` (failure only logged) → legacy startup effects pipeline (`reconcile_application_effects` Full + direct tray emits) while every core command also calls `notify_committed(true)` (`client/application_workflow/workflow.rs` ~72–84). A boot failure leaves no deferred target, so status shows Healthy and RetryNow is a no-op (`tcc.rs` ~307). An adopted service core without a receipt makes later runtime mutations fail with `NoRestorableBaseline`.
- Producers: `ProfilesActor::post_start` arms the refresh scheduler (`catch_up=true`), external watchers and the materialization ticker before the workflow is connected and before any boot reconcile; scheduled refresh downloads run in untracked spawns; scheduled results/failures are dropped silently (`state/profiles/actor.rs` ~1500–1661); invalid external content only logs.
- Recovery: lifecycle `uncertain` without a mutation recovery context cannot be cleared (`tcc.rs` ~288–294); only app restart exits it.
- Shutdown: all exits funnel into `utils/help.rs::cleanup_processes` (~243–274), which is not single-flight (IPC `cleanup_processes` + exit, `restart_application` run it twice), saves SessionState first, seals effect owners and restores the system proxy before workflow Closing, never stops Profiles/Application/Clash/Session actors or background producers, and has only per-step timeouts (5 s proxy restore, 5 s hotkeys, 10 s effects RPC, 180 s workflow). `WidgetManager::drop` blocks (`widget.rs` ~199–215, fires on every clone).

The design must decide, with file-level placement and message/type names:

1. `StartupReconcile`: where it lives (workflow command, not `resolve.rs`), what it probes first (host, instance generation, running config identity — V37: never start a second instance whose ownership is unproven), its outcomes Ready / ReadyDegraded (a deferred target so RetryNow and the retry budget apply) / RecoveryRequired (with context), and that it ends with exactly one full effects notify, replacing the legacy startup pipeline and the per-command boot notifies. Random-port results update only the runtime binding.
2. Producer gating: background sources (refresh scheduler with catch-up, external watchers, materialization ticker) start only after StartupReconcile settles, via an explicit typed message from the composition root; scheduled download tasks become tracked and cancellable.
3. Subscription completion receipts and external ingestion: the minimum that satisfies T10 ("受控处理外部文件摄取与订阅完成回执") and plan V31/V33 — `RefreshOrigin` respected, scheduled outcomes (success/failure/superseded) observable (log + a typed status the UI can show, or an explicit reason why log-only suffices), invalid external content surfaced as an honest status instead of a warn-only log. Keep it minimal; no new UI surfaces unless required.
4. Explicit recovery for lifecycle `uncertain` without a mutation context: query the core/host, verify, then re-enter admission; which IPC triggers it (reuse `retry_configuration_runtime` if possible).
5. Shutdown: one single-flight orchestrator owned by the application layer (not `utils/help.rs`), in plan §11.3 order — Closing admission → stop producers (Profiles scheduler/watchers/ticker/downloads, hotkey pump, UI forwarders) → wait tracked TCC (AwaitDecision preserved) → seal effect owners (closed guard, idempotent) → wait in-flight OS writes → restore owned system proxy → stop core per lifecycle policy → save SessionState last → stop actors. One overall budget; a structured shutdown report that names incomplete cleanup; repeated calls return the first run's report. `WidgetManager` stops explicitly (no blocking `Drop`).
6. Test matrix: map every decision to fake-port tests, including Close during Try / AwaitDecision / Cancel / peripheral IO (V36), service residual instance on GUI restart (V37), startup failure → ReadyDegraded with working RetryNow, double cleanup, random-port boot without a source patch.
7. Explicit non-goals and what stays for T11.

Constraints for the design: reuse existing actors/ports; no new generic workflow engine; no new `::global()`; Tauri stays behind adapters; state which existing code each step deletes. Length: as long as needed, but every section must end in a decision, not options.

### Task 6: StartupReconcile, producer gating, lifecycle recovery

Branch `feat/tcc-startup-shutdown` (continues Task 5). Implements design §1–§4 (and the matching §6 tests) exactly as approved; the controller appends the approved design path and any review rulings to the dispatch. Deletes the legacy startup pipeline in `utils/resolve.rs` (`reconcile_core` boot call, `reconcile_application_effects` call and its degradation loop, direct `update_systray` emit, `Handle::update_systray_part` call) and the per-core-command boot notifies it replaces.

Acceptance: Global Constraint 4 checks; the design's startup/producer/recovery tests; `rg -n "reconcile_application_effects" backend/tauri/src/utils` returns nothing.

### Task 7: Ordered single-flight shutdown

Branch `feat/tcc-startup-shutdown` (continues Task 6). Implements design §5 (and the matching §6 tests) exactly as approved. `cleanup_processes` (IPC and exit path) and `restart_application` call the orchestrator; `utils/help.rs` keeps only the Tauri/OS boundary glue (e.g. Windows `set_ready_for_shutdown`). `WidgetManager` blocking `Drop` removed.

Acceptance: Global Constraint 4 checks; the design's shutdown tests (order, idempotence, Close in each phase, budget exhaustion reported as incomplete).

### Task 8: UI boundary globals

Branch `refactor/inject-app-infrastructure` (create from the L3 head).

Context (roadmap §8.2; inventory at `main @ 4f59ca781`, re-verify on the branch): `Handle::global()` (`core/handle.rs`) — callers `feat.rs::restart_clash_core` (duplicate `refresh_clash` + `notice_message` with no frontend listener), `lib.rs` panic hook and deep-link cold start, `core/tray/proxies.rs::update_selected_proxies`, `utils/dirs.rs::app_resources_dir`, `resolve.rs` init; `consts::APP_HANDLE`/`app_handle()`/`setup_app_handle` (no readers); `core/logger.rs` `Logger::global()` (never written; `get_clash_logs` always empty, no frontend caller); `WindowManager::global()` + `OPEN_WINDOWS_COUNTER` (`window.rs`); `utils/resolve.rs` `OPEN_WINDOWS_COUNTER` (never incremented, so `is_window_opened()` is always true), `TRAY_MENU_PERSISTENT`/`TRAY_MENU_READY`/`TRAY_MENU_IGNORE_BLUR_UNTIL_MS`; tray statics `UPDATE_SYSTRAY_MUTEX`, `ITEM_IDS`, `TRAY_ITEM_UPDATE_BARRIER`, `LINUX_TRAY_ID`; macOS `TRAFFIC_LIGHTS_WINDOW_DELEGATE_GUARD` (one slot shared by all windows); `UiEventSink::notice_message`/`update_systray*` and `Handle::update_systray*` dead or duplicate.

Requirements:

1. Delete `Handle` and `consts::APP_HANDLE`. Replace each live use with an object already in scope or injected: `restart_clash_core` goes through the facade (the typed pipeline already emits the clash refresh); the panic hook is installed where an `AppHandle` is available (inside `.setup`), with the pre-setup fallback kept as a plain `process::exit`; deep-link cold start uses the local handle; tray proxy updates use the handle the receiver already owns; resources dir resolved once into `PathResolver` (see Task 9 if it lands there — coordinate by doing it here).
2. Delete `core/logger.rs` and the `get_clash_logs` IPC (regenerate bindings). Delete the `nyanpasu://notice-message` emission and the dead `UiEventSink`/`Handle` tray methods.
3. Replace `WindowManager::global()` with a window registry owned by the composition root and reachable only through an injected adapter or Tauri-managed state (or derive instances from Tauri's own window registry if that removes the need for extra state). Fix the window-open wait: replace both `OPEN_WINDOWS_COUNTER`s with a correct signal (window-ready event or registry query) so the deep-link wait loops in `lib.rs` wait for a real window.
4. Move tray statics into the managed `TrayState` (or the tray refresher adapter); move the tray-menu window focus/blur state into a small tray-menu window controller in managed state; `LINUX_TRAY_ID` becomes a `const`; the macOS traffic-lights delegate guard becomes per-window state.

Acceptance: Global Constraint 4 checks; unit tests for the window registry and the tray-menu blur state machine (pure logic separated from Tauri); `rg -n "Handle::global|WindowManager::global|Logger::global|app_handle\(\)" backend/tauri/src` returns nothing; ledger `service_globals` lowered to what remains for Task 9.

### Task 9: Infrastructure statics and the static allowlist gate

Branch `refactor/inject-app-infrastructure` (continues Task 8).

Context: `utils/init/logging.rs` `CHANNEL` (+ `Channel::globals()`), `core/actor_v2/service_host_adapter.rs` `service_default()` (`TODO(actor-migration)`; `setup.rs` already builds a `nyanpasu_ipc` client), `utils/dirs.rs` four `static INIT: Once` guards, `server/mod.rs` `SERVER_PORT` (panics if no port), `enhance/script/js.rs` `CUSTOM_SCRIPTS_DIR` (stale after home-dir move), `core/service/mod.rs` `SERVICE_PATH`, `consts.rs` `IS_PORTABLE` (duplicates `BundleMetadata.is_portable`), `shutdown_hook.rs` Windows statics (Win32 callback without user data).

Requirements:

1. Logging reload sender is returned by `logging::init()` and injected into `TracingLoggerRefresher`; delete `CHANNEL`/`Channel::globals()`. The tracing global subscriber itself stays (process-wide by the `tracing` API).
2. Inject the service IPC client into `OsServiceHostAdapter`; remove the `TODO(actor-migration)`.
3. `PathResolver` gains the paths still resolved through statics (scripts dir, service binary path, resources dir if not done in Task 8); callers receive them explicitly; delete `CUSTOM_SCRIPTS_DIR`, `SERVICE_PATH`, the `Once` guards. `IS_PORTABLE` callers use `BundleMetadata`/`NyanpasuClient::is_portable()` where reachable; keep it only where no instance exists yet and list it.
4. `SERVER_PORT`: choose the port in the composition root, fail setup with an error instead of panicking, pass it to the server and to `get_server_port` via managed state.
5. Windows `shutdown_hook.rs`: move state into the window's user-data slot, or keep it as an explicit allowlisted exception with a comment explaining the Win32 constraint — decide and justify in the report.
6. Architecture ledger: add a `mutable_statics` metric counting `static` items with interior mutability or lazy init (`OnceCell`, `OnceLock`, `Lazy`, `LazyLock`, `lazy_static!`, `thread_local!`, `static ... Mutex|RwLock|Atomic*`) in `backend/tauri/src`, with an explicit allowlist (path + name + one-line reason) for immutable constants/lookup tables/feature flags and unavoidable third-party/OS globals (`BUILD_INFO`, migration step statics, `IS_APPIMAGE`, `DEVICE_INFO`, `BOA_LOGGER_LOCK`, dock `MARK`, and whatever requirement 5 keeps). Non-allowlisted count must be 0 in the snapshot. Tests in `scripts/architecture-ledger_test.ts`.

Acceptance: Global Constraint 4 checks; ledger `service_globals` 0, `migration_markers` reduced by the removed TODO, `mutable_statics` non-allowlisted 0.

### Task 10: T11 duplicate-mechanism deletion

Branch `refactor/tcc-t11-cleanup` (create from the L4 head).

Context (re-verify each on the branch; T10 may already have removed some): dead dirty wiring (`DirtyNotifier`, `DIRTY_WINDOW`, `dirty_rx`/`DirtyTick`, `Command::RuntimeDirty`, `RebuildNotifier` — sender dropped at `client/mod.rs` construction); dead workflow/facade routes (`ApplicationWorkflowClient::set_execution_host` + `Command::SetExecutionHost` + `set_host`, `ApplyControlChannel`, `NyanpasuClient::{apply_control_channel, regenerate_runtime, recover_core, change_execution_host}` without callers and `rebuild_running_config`, `promote_existing_runtime_product`, `start_promoted_runtime`, `stop_core` with test-only callers hidden by `#[allow(dead_code)]`); `client/effects/plan.rs` `runtime_apply_kind`/`RuntimeApplyKind`/`ClashRuntimeDesired` (test-only; superseded by `application_workflow/impact.rs`); `client/effects/mod.rs::reconcile_application_effects` if T10 left it; any remaining `update_systray` listener duplication.

Requirements: delete them with their tests; keep fencing that still serves owner concurrency or external events (TCC plan T11); tests that exercised dead routes are either deleted or rewritten against the live route when they covered a still-live behavior (say which in the report). Remove `#[allow(dead_code)]` attributes whose reason disappears.

Acceptance: Global Constraint 4 checks; `cargo clippy --manifest-path backend/Cargo.toml --all-targets --all-features` introduces no new warnings in touched files.

### Task 11: Documentation and final gate

Branch `refactor/tcc-t11-cleanup` (continues Task 10).

Requirements:

1. `docs/design/actor-migration-roadmap.md`: status table rows for PR-6, PR-7a, PR-7b updated to the delivered state (merge status stays "pending" until merged; acceptance pending maintainer smoke), §12 success criteria annotated with evidence (ledger numbers, test counts), residual list (submodule marker, manual smoke).
2. `docs/plan/2026-09-14-application-workflow-selective-tcc-v2.md` status line points to new records `docs/plan/2026-09-25-tcc-t10-implementation.md` and `docs/plan/2026-09-25-tcc-t11-implementation.md` (same style as the 2026-09-24 T6–T9 records: scope, decisions, verification with real commands and results, boundaries).
3. Final ledger snapshot equals the report; `config_calls`, `service_globals`, `migration_markers`, `legacy_dto_refs` (outside the allowlisted migration schema), `bridge_files`, non-allowlisted `mutable_statics` are all 0 — or each non-zero residual is named in the roadmap with owner and removal condition.
4. `AGENTS.md`/`CLAUDE.md` §9 legacy pattern list: keep in sync (both files) if any listed pattern no longer exists; otherwise untouched.

Acceptance: Global Constraint 4 checks; full `cargo test --manifest-path backend/Cargo.toml --all-features --workspace -- --test-threads=8`, `cargo clippy ... --all-targets --all-features`, `pnpm lint:rustfmt`, `pnpm typecheck`, `pnpm test:frontend`, `pnpm lint:architecture-ledger`, `pnpm test:architecture-ledger` all recorded with real output in the T11 record.
