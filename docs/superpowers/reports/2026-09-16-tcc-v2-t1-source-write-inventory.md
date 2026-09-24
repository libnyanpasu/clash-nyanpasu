# T1 · Source-config write inventory (selective TCC v2)

Scope: every foreground/background entry point that writes a **source configuration
domain** — Application (`NyanpasuAppConfig`), Clash (`ClashConfig`, including
`overrides`), Profiles (`Profiles`). Session state is listed separately because it is
not part of the workflow's execution domain.

Line numbers are as of commit `4b4cc7269` (after T1).

Target-treatment keys: **T6** = per-mutation Required participant; **T9** = legacy
routing removal; **T10** = startup/shutdown ordering; **T7** = post-commit dispatch.

## 1. Facade write entry points (`backend/tauri/src/client/mod.rs`)

| Entry point                           | file:line            | Domain                                 | Goes through the workflow?                           | Target treatment                       |
| ------------------------------------- | -------------------- | -------------------------------------- | ---------------------------------------------------- | -------------------------------------- |
| `update_core`                         | `client/mod.rs:486`  | Application                            | commit at facade, then `reconcile`                   | T6 (`MustApply`) — **relocated in T1** |
| `set_execution_host`                  | `client/mod.rs:507`  | Application                            | commit at facade, then `set_execution_host` apply    | T6 (`MustApply`) — **relocated in T1** |
| `patch_app_config`                    | `client/mod.rs:606`  | Application                            | no; `commit_and_reconcile` gate                      | T6                                     |
| `replace_app_config`                  | `client/mod.rs:621`  | Application                            | no; `commit_and_reconcile` gate                      | T6                                     |
| `patch_runtime_overrides`             | `client/mod.rs:696`  | Clash overrides                        | commit at facade, then `apply_clash_overrides`       | T6 — **relocated in T1**               |
| `patch_clash_config`                  | `client/mod.rs:712`  | Clash                                  | no; `commit_and_reconcile` gate                      | T6                                     |
| `replace_clash_config`                | `client/mod.rs:724`  | Clash                                  | no; `commit_and_reconcile` gate                      | T6                                     |
| `apply_legacy_verge_patch_saga`       | `client/mod.rs:756`  | Application + Session + Clash          | no; `commit_and_reconcile` gate                      | T9                                     |
| `apply_legacy_verge_replacement_saga` | `client/mod.rs:806`  | Application + Session + Clash          | no; `commit_and_reconcile` gate                      | T9                                     |
| `try_auto_activate_if_none`           | `client/mod.rs:1164` | Profiles                               | commit at facade (`set_current_if_none`), then apply | T6 — **relocated in T1**               |
| `add_profile`                         | `client/mod.rs:1187` | Profiles                               | no; `collect_post_commit_degradations`               | T6 (`SaveOnly` / auto-activate)        |
| `create_profile`                      | `client/mod.rs:1215` | Profiles                               | no                                                   | T6                                     |
| `import_profile`                      | `client/mod.rs:1247` | Profiles                               | no                                                   | T6                                     |
| `delete_profile`                      | `client/mod.rs:1290` | Profiles                               | no                                                   | T6                                     |
| `reorder_profile`                     | `client/mod.rs:1295` | Profiles                               | no                                                   | T6                                     |
| `reorder_profiles_by_list`            | `client/mod.rs:1308` | Profiles                               | no                                                   | T6                                     |
| `refresh_profile`                     | `client/mod.rs:1316` | Profiles                               | no                                                   | T6                                     |
| `patch_profile_metadata`              | `client/mod.rs:1325` | Profiles                               | no                                                   | T6                                     |
| `patch_remote_profile_options`        | `client/mod.rs:1334` | Profiles                               | no                                                   | T6                                     |
| `replace_profile_definition`          | `client/mod.rs:1343` | Profiles                               | no                                                   | T6                                     |
| `activate_profile`                    | `client/mod.rs:1356` | Profiles                               | commit at facade (`set_current`), then apply         | T6 (`MustApply`) — **relocated in T1** |
| `set_global_transforms`               | `client/mod.rs:1368` | Profiles                               | no                                                   | T6                                     |
| `set_profile_valid_fields`            | `client/mod.rs:1376` | Profiles                               | no                                                   | T6                                     |
| `save_profile_file`                   | `client/mod.rs:1424` | managed profile file (no state commit) | no                                                   | T6 (D5 managed edit)                   |
| `patch_session_state`                 | `client/mod.rs:658`  | Session                                | no                                                   | outside the execution domain           |
| `replace_session_state`               | `client/mod.rs:670`  | Session                                | no                                                   | outside the execution domain           |

The five relocated writes are the T1 deliverable: `activate_profile`,
`try_auto_activate_if_none`, `patch_runtime_overrides`, `update_core` and
`set_execution_host`. Before T1 they lived at
`client/application_workflow/profiles.rs:49,53`,
`client/application_workflow/workflow.rs:74` and
`client/core_lifecycle/workflow.rs:131,143`.

## 2. IPC commands (`backend/tauri/src/ipc.rs`)

| Command                        | file:line     | Domain                            | Facade method                                |
| ------------------------------ | ------------- | --------------------------------- | -------------------------------------------- |
| `import_profile`               | `ipc.rs:143`  | Profiles                          | `import_profile`                             |
| `create_profile`               | `ipc.rs:190`  | Profiles                          | `create_profile`                             |
| `reorder_profile`              | `ipc.rs:201`  | Profiles                          | `reorder_profile`                            |
| `reorder_profiles_by_list`     | `ipc.rs:211`  | Profiles                          | `reorder_profiles_by_list`                   |
| `update_profile`               | `ipc.rs:220`  | Profiles                          | `refresh_profile`                            |
| `delete_profile`               | `ipc.rs:230`  | Profiles                          | `delete_profile`                             |
| `activate_profile`             | `ipc.rs:239`  | Profiles                          | `activate_profile`                           |
| `set_profile_valid_fields`     | `ipc.rs:257`  | Profiles                          | `set_profile_valid_fields`                   |
| `patch_profile_metadata`       | `ipc.rs:266`  | Profiles                          | `patch_profile_metadata`                     |
| `patch_remote_profile_options` | `ipc.rs:276`  | Profiles                          | `patch_remote_profile_options`               |
| `replace_profile_definition`   | `ipc.rs:286`  | Profiles                          | `replace_profile_definition`                 |
| `save_profile_file`            | `ipc.rs:320`  | managed profile file              | `save_profile_file`                          |
| `patch_clash_config`           | `ipc.rs:461`  | Clash overrides                   | `patch_runtime_overrides`                    |
| `patch_verge_config`           | `ipc.rs:497`  | Application + Session + Clash     | `LegacyVergeBridge::patch_verge_config` (T9) |
| `change_clash_core`            | `ipc.rs:506`  | Application                       | `update_core`                                |
| `update_core`                  | `ipc.rs:663`  | core binary only, no state commit | `replace_core_binary`                        |
| `set_hotkeys`                  | `ipc.rs:1063` | Application                       | `patch_app_config`                           |

## 3. Legacy bridge route (T9)

| Entry point                                    | file:line             | Domain                                        |
| ---------------------------------------------- | --------------------- | --------------------------------------------- |
| `LegacyVergeBridge::patch_verge_config`        | `bridge/verge.rs:219` | Application + Session + Clash                 |
| `LegacyVergeBridge::replace_verge_config`      | `bridge/verge.rs:251` | Application + Session + Clash                 |
| `LegacyVergeBridge::run_legacy_verge_mutation` | `bridge/verge.rs:264` | wraps `crate::feat::patch_verge` side effects |
| `feat::patch_verge` service-mode branch        | `feat.rs:181`         | Application, via `set_execution_host`         |

## 4. Background / non-UI writers

| Writer                                            | file:line                                                              | Domain                     | Goes through the workflow?                        | Target treatment            |
| ------------------------------------------------- | ---------------------------------------------------------------------- | -------------------------- | ------------------------------------------------- | --------------------------- |
| Hotkey action pump (`set_clash_mode`)             | `client/hotkey/mod.rs:195`                                             | Clash overrides            | via `patch_runtime_overrides`                     | T6                          |
| Hotkey action pump (`set_system_proxy`)           | `client/hotkey/mod.rs:206`                                             | Application                | via `patch_app_config`                            | T6                          |
| Hotkey action pump (`set_tun_mode`)               | `client/hotkey/mod.rs:217`                                             | Clash                      | via `patch_clash_config`                          | T6                          |
| Tray menu → hotkey action                         | `core/tray/mod.rs:502`                                                 | as above                   | as above                                          | T6 / T7                     |
| Remote refresh scheduler                          | `state/profiles/scheduler.rs:188,198` → `state/profiles/actor.rs:1343` | Profiles                   | no; actor-internal commit                         | T6 (background participant) |
| External file watcher                             | `state/profiles/scheduler.rs:73` → `state/profiles/actor.rs:1762`      | Profiles                   | no; actor-internal commit, then `RebuildNotifier` | T6 (background participant) |
| Periodic materialization reconcile                | `state/profiles/actor.rs` (`ReconcileMaterializations`)                | Profiles journal only      | no                                                | T6                          |
| Updater (`UpdaterClient` → `replace_core_binary`) | `client/mod.rs:474`                                                    | none — binary install only | yes (workflow admission)                          | T8 budgets                  |

The background profile writers commit inside `ProfilesActor` and signal the workflow
only through `RebuildNotifier`. They are the "后台订阅提交" §5.2 warns about: they do
not sample the other two domains today, so they inherit T6's participant rule rather
than needing a separate bypass.

## 5. Startup / shutdown backfill (T10)

| Site                              | file:line                                   | Domain                                                                | Note                                     |
| --------------------------------- | ------------------------------------------- | --------------------------------------------------------------------- | ---------------------------------------- |
| `resolve_setup` port backfill     | `utils/resolve.rs:171,181`                  | Application + Clash, via legacy `Config::verge()` / `Config::clash()` | `FIXME(actor-migration)` already present |
| Post-`resolve_setup` typed reseed | `lib.rs:285`                                | Application + Session + Clash, via `replace_verge_config`             | `TODO(actor-migration)` already present  |
| `set_window_state`                | `utils/resolve.rs:312`                      | legacy verge window state                                             | mirrored into Session by the bridge      |
| `save_main_window_state`          | `utils/resolve.rs:469,484`                  | Session                                                               | `replace_session_state`                  |
| Legacy mirror sync after load     | `client/mod.rs:171` (`sync_legacy_mirrors`) | read-only projection into legacy stores                               | no commit                                |

## 6. Read paths (T1 completion condition)

Every committed read now goes through the coordinator's `StateSnapshot` handle, so no
read can queue behind a domain actor that is holding a transaction open.

| Reader                                                                                       | How it reads now                                                                                                                    |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `NyanpasuClient::get_app_config` / `get_clash_config` / `get_session_state` / `get_profiles` | `ApplicationClient::snapshot()` / `ClashConfigClient::snapshot()` / `SessionStateClient::snapshot()` / `ProfilesClient::snapshot()` |
| `NyanpasuClient::typed_config_snapshots` (legacy saga CAS baseline)                          | the three `snapshot()` calls; now synchronous                                                                                       |
| `NyanpasuClient::effect_inputs` (before/after sampling in `commit_and_reconcile`)            | `snapshot()`; the read is infallible, so the old `effect_inputs_unavailable` degradation is gone                                    |
| `RuntimePreparation`                                                                         | injected `StateSnapshot<NyanpasuAppConfig/ClashConfig/Profiles>`; it holds no domain client                                         |
| `ApplicationWorkflow`                                                                        | injected `StateSnapshot<Profiles>` and `StateSnapshot<ClashConfig>`, read **after** the actor admits the command                    |
| `CoreLifecycleWorkflow`                                                                      | injected `StateSnapshot<NyanpasuAppConfig>`                                                                                         |

No facade read deliberately needs the actor's own view; the domain actors' `Get`
messages had no remaining caller and were removed.

**Cross-domain sampling.** The workflow has no pre-admission sampling left. What the
facade computes before committing is derived from the submitted patch only
(`mode_changed = patch.mode.is_some()`), never from another domain's state. The
committed candidate is passed into the workflow, and the other domains are loaded from
their handles inside the admitted command.

**SessionState does not call application reconcile.** `patch_session_state` /
`replace_session_state` go through `commit_and_reconcile`, whose runtime step is chosen
by `runtime_apply_kind` (`client/effects/plan.rs:417`), which diffs only the clash
runtime desired. A session-only mutation yields `RuntimeApplyKind::None`. Verified, not
changed.

## 7. Static check

Implemented as a `#[test]` rather than an architecture-ledger metric:
`client::tests::application_workflow_sources_hold_no_source_config_client`
(`client/mod.rs`). It scans the non-test sources under `src/client/application_workflow`
and `src/client/core_lifecycle` for `ApplicationClient`, `ClashConfigClient` and
`ProfilesClient`. The ledger's only gate mechanisms are an exact snapshot compare and a
single hard denylist bound to `test_real_dirs`; adding a file-scoped forbidden-pattern
metric would mean a new metric id, a snapshot contract change and a gate rule, for a
check that belongs next to the code it protects.

## 8. Known gaps left for T5

### 8.1 Admission no longer gates the commit

The facade writes the domain and then submits the apply, so a workflow that is busy,
full, shutting down or uncertain refuses the apply for state that is already persisted.

### 8.2 Commit order and apply order can diverge, permanently

The two relocated fixed-candidate applies submit a candidate captured at commit time:

- `NyanpasuClient::activate_profile` (`client/mod.rs:1356`) and
  `try_auto_activate_if_none` (`client/mod.rs:1164`) pass the commit's `CommitReport`, and
  the workflow builds from `report.snapshot` (`client/application_workflow/profiles.rs:39`).
- `NyanpasuClient::patch_runtime_overrides` (`client/mod.rs:696`) passes the committed
  `ClashConfig`, and the workflow builds from it
  (`client/application_workflow/workflow.rs:75`).

Neither facade method holds the effects gate. Two concurrent callers therefore serialize
their commits in the owning actor and their applies in the workflow queue independently:
a task descheduled between its commit and its submission can have the workflow apply the
older candidate last. The runtime then reflects a superseded candidate while the
committed state is the newer one.

Nothing converges that divergence. `SetCurrent` / `SetCurrentIfNone` reach the commit
through `run_state_write`, which does not call `RebuildNotifier::request_rebuild`
(`state/profiles/actor.rs:1118,1126`; the notifier fires only at
`state/profiles/actor.rs:1582,1856,1884`, none of which is this path), and
`ClashConfigActor` has no notifier at all. So no dirty pass rebuilds from the newest
committed state afterwards.

This did not exist before T1: the commit and its apply were one admitted workflow
operation. T5 must therefore **restore ordering**, not only move the refusal of 8.1
earlier — making the workflow a Required participant of the transaction puts the commit
and the apply back in one serialized step.
