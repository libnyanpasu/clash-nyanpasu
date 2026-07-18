# PR-4S S10 — PR-4 Review Finding Disposition

**Status:** Code/test dispositions for S02/S03/S04/S08 are implemented on `refactor/pr-4s`; **all four GitHub review threads remain unresolved**. PR-4S / S10 stay **OPEN**.
**Source PR:** [#4932](https://github.com/libnyanpasu/clash-nyanpasu/pull/4932) — `refactor(tauri)!: runtime derivation cleanup — checked promote pipeline and RebuildOutcome (PR-4)` (MERGED 2026-07-12).
**Stabilization PR (docs context only):** [#4956](https://github.com/libnyanpasu/clash-nyanpasu/pull/4956) draft on `refactor/pr-4s` — review threads: none discovered via GraphQL at authoring time.
**Disposition revised:** 2026-07-18 (UTC)
**Discovery method:** read-only `gh api` + GraphQL `reviewThreads` on `#4932` / `#4956` (no mutations).

---

## 0. Rules

1. Every PR-4 unresolved finding must have: thread URL, original claim, PR-4S code/test disposition, owning S0x, and **explicit thread-gate state**.
2. Code disposition **does not** auto-resolve a GitHub thread. If GraphQL reports `isResolved: false`, the thread-gate remains **OPEN / PENDING** even when the fix is on the working branch.
3. Do not invent reply URLs, resolve timestamps, or “resolved by @…” claims without API evidence.
4. **Thread-gate (closeout blocker):** all four findings remain unresolved on GitHub. PR-4S review closeout requires, **for each finding**, one of the following durable outcomes — this document **does not choose** which path maintainers take:
   - **Path A — actual GitHub resolve:** thread `isResolved: true` with discoverable resolve evidence; or
   - **Path B — explicit maintainer disposition:** a signed/dated maintainer note (in this file or a linked issue/PR comment) that the finding is accepted as code-disposed without GitHub resolve, with rationale.
     Until Path A or Path B exists per finding, the review gate is **not** satisfied.

---

## 1. Summary table

| ID   | Finding (short)                                                                  | Thread URL                                                                     | GraphQL `isResolved` @ 2026-07-18 | Code/test owner            | Code disposition status                    | GitHub resolve status |
| ---- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ | --------------------------------- | -------------------------- | ------------------------------------------ | --------------------- |
| RF-1 | `createProfile` / `unwrapResult` can mask typed errors as success (`undefined`)  | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566317999 | **false**                         | S08                        | Implemented on branch (wire + frontend)    | **PENDING**           |
| RF-2 | Rollback unit test writes real user runtime product path                         | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566318012 | **false**                         | S02                        | Implemented (RuntimePaths / TempDir)       | **PENDING**           |
| RF-3 | `change_core` only holds `rebuild_gate`; races `CoreManager::run_core` / restart | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566323210 | **false**                         | S04 (+ S09 process matrix) | Implemented (CoreLifecycleLease + tests)   | **PENDING**           |
| RF-4 | Product rollback does not restore runtime read model                             | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566323213 | **false**                         | S03 (+ S04 lease span)     | Implemented (transaction snapshot restore) | **PENDING**           |

---

## 2. Finding dossiers

### RF-1 — `createProfile` / `unwrapResult` non-exhaustive success path

| Field             | Value                                                                          |
| ----------------- | ------------------------------------------------------------------------------ |
| Thread            | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566317999 |
| Review comment id | `3566317999`                                                                   |
| Path (at review)  | `frontend/interface/src/ipc/use-profile.ts`                                    |
| Author            | `copilot-pull-request-reviewer` (Copilot)                                      |
| Created           | 2026-07-12T12:43:47Z                                                           |
| GraphQL thread id | `PRRT_kwDOKroWZM6QMw0z`                                                        |
| GraphQL resolved  | **false** (queried 2026-07-18)                                                 |

**Original claim (faithful summary):**
`commands.createProfile(...)` is wrapped by `unwrapResult`, which returns `undefined` on typed error. Returning `{ uid: null, rebuild: undefined }` lets React Query treat the mutation as success and still invalidate, masking failure. The URL/import branch already throws when `unwrapResult` returns no result; the non-URL branch should do the same.

**PR-4S disposition (code/test — S08):**

- Public wire replaced legacy `RebuildOutcome` with `MutationOutcome<T>` (`applied` / `committed_degraded` only).
- `unwrapResult` is exhaustive over success shapes and surfaces typed failures instead of collapsing to `undefined`.
- `MutationCache` treats `committed_degraded` as mutation success (with warning) while hard errors remain errors.
- Specta freeze + bindings freshness reject legacy status tags / `RebuildOutcome`.
- Create/import return `MutationOutcome<ProfileId>`; H1/H2 retain committed ids under materialization/auto-activation degradation.

**Evidence pointers (branch, not GitHub resolve):** S08 task disposition in `task.md` / `design.md` §6.11; frontend `unwrapResult` + profile hooks on `refactor/pr-4s` (e.g. post-`1cd8f78b`).

**GitHub resolution:** **PENDING** — no resolve event discovered; thread still open on merged `#4932`.
**Resolve URL / actor / timestamp:** **PENDING / not discovered**.

---

### RF-2 — Tests write the real user runtime config path

| Field             | Value                                                                          |
| ----------------- | ------------------------------------------------------------------------------ |
| Thread            | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566318012 |
| Review comment id | `3566318012`                                                                   |
| Path (at review)  | `backend/tauri/src/client/rebuild.rs`                                          |
| Author            | `copilot-pull-request-reviewer` (Copilot)                                      |
| Created           | 2026-07-12T12:43:48Z                                                           |
| GraphQL thread id | `PRRT_kwDOKroWZM6QMw08`                                                        |
| GraphQL resolved  | **false** (queried 2026-07-18)                                                 |

**Original claim (faithful summary):**
A unit test writes to `runtime_config_path()` derived from `dirs::app_config_dir()`, i.e. the real per-user config directory on non-Windows platforms. Panic/abort before restore, or concurrent tests, can corrupt a developer’s real config and introduce cross-test flakiness via shared global filesystem state.

**PR-4S disposition (code/test — S02 + S10 tooling):**

- `RuntimePaths` is injected from the composition root / test `TempDir`.
- Runtime product/candidate paths no longer resolve through free global helpers in the migrated runtime path.
- Candidate files use private dir + random name + exclusive create + cleanup; tests must not touch real user dirs (design §8.4 denylist intent).
- S10 architecture-ledger **gate mode**, committed snapshot, and package lint integration are **implemented**: gate hard-fails when `test_real_dirs.total != 0` and exact-compares the committed stable snapshot; `pnpm lint` pulls in `lint:architecture-ledger`. See `smoke-evidence.md` §2 (tooling status) and §5 (Full QA command log: local Q-01/Q-02 **PASS** recorded; target-tip multi-OS CI Q-09…Q-11 still **PENDING**).

**Evidence pointers:** S02 `807f1733` lineage / task card; `design.md` §6.1–6.2; `smoke-evidence.md` §2 / §5.

**GitHub resolution:** **PENDING**.
**Resolve URL / actor / timestamp:** **PENDING / not discovered**.

---

### RF-3 — `change_core` not serialized with full core lifecycle / restart domain

| Field                     | Value                                                                          |
| ------------------------- | ------------------------------------------------------------------------------ |
| Thread                    | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566323210 |
| Review comment id         | `3566323210`                                                                   |
| Path (at review)          | `backend/tauri/src/client/rebuild.rs`                                          |
| Author                    | `chatgpt-codex-connector` (Codex)                                              |
| Created                   | 2026-07-12T12:47:00Z                                                           |
| GraphQL thread id         | `PRRT_kwDOKroWZM6QMxw3`                                                        |
| GraphQL resolved          | **false** (queried 2026-07-18)                                                 |
| Severity badge in comment | P2                                                                             |

**Original claim (faithful summary):**
Because `change_core` only holds `rebuild_gate`, it no longer serializes the full draft→promote→rollback transaction against `CoreManager::run_core` callers that only take `run_lock` (e.g. `restart_sidecar`). If new-core restart fails, draft may be discarded while the promoted product is still for the new core until rollback rebuild completes; a concurrent `run_core` can start the old core against the new-core product. Legacy `CoreManager::change_core` held `run_lock` for this sequence, so the race is introduced in PR-4.

**PR-4S disposition (code/test — S04; process reinforcement — S09):**

- `CoreLifecyclePort` / `CoreLifecycleLease` unify run/restart/stop/check/apply/recover locking (`CoreManager::lifecycle_lock`).
- `change_core` holds `rebuild_gate + lease` through rollback completion.
- Updater stop/swap/restart uses the same lease span.
- Deterministic test: `s04_concurrent_restart_waits_until_change_core_rollback_completes` (barrier/oneshot, no sleep).
- S09 `fake-core` process matrix adds process-level lease serialization and change_core new-start failure + old-core rollback (still not true-core UI smoke).

**Evidence pointers:** commits in lineage including `666b078d` (lifecycle serialization), S09 `b70aefb0` rebuild isolation; `design.md` §6.3 / §6.6 / §6.13.

**GitHub resolution:** **PENDING**.
**Resolve URL / actor / timestamp:** **PENDING / not discovered**.

---

### RF-4 — Product rollback leaves runtime read model on new-core state

| Field                     | Value                                                                          |
| ------------------------- | ------------------------------------------------------------------------------ |
| Thread                    | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566323213 |
| Review comment id         | `3566323213`                                                                   |
| Path (at review)          | `backend/tauri/src/client/rebuild.rs`                                          |
| Author                    | `chatgpt-codex-connector` (Codex)                                              |
| Created                   | 2026-07-12T12:47:00Z                                                           |
| GraphQL thread id         | `PRRT_kwDOKroWZM6QMxw5`                                                        |
| GraphQL resolved          | **false** (queried 2026-07-18)                                                 |
| Severity badge in comment | P2                                                                             |

**Original claim (faithful summary):**
In the rollback-rebuild-failure branch, restoring/removing the product leaves `self.inner.runtime` holding the new-core `RuntimeState` published by the first successful regenerate. After the error, `get_runtime_yaml` / `get_runtime_config` can serve a config that is no longer the promoted product. Capture the old runtime snapshot before the first regenerate and restore or clear it when `old_product` is restored or removed.

**PR-4S disposition (code/test — S03; lease integrity — S04):**

- Runtime store is explicit `RuntimeLifecycleState { promoted, applied }` with revision/core/hash/exact product bytes.
- Four runtime read IPCs read **Promoted**.
- `change_core` captures `RuntimeTransactionSnapshot` and restores product → Promoted → old-core restart → Applied on success path of rollback.
- Apply failure keeps Applied old while Promoted may advance; compensation uses Applied (S05), not Promoted-as-Applied.

**Evidence pointers:** S03 task disposition; runtime lifecycle work in branch history (e.g. `372f5493` applied snapshot restore lineage); `design.md` §4 / §6.4–6.5.

**GitHub resolution:** **PENDING**.
**Resolve URL / actor / timestamp:** **PENDING / not discovered**.

---

## 3. Why threads can remain open after code fix

`#4932` is already **MERGED**. GraphQL still returns four unresolved threads. Likely causes (non-exclusive):

- No maintainer clicked “Resolve conversation” after merge.
- Fixes landed on follow-up branch `refactor/pr-4s` (`#4956`) rather than as commits on `#4932`.
- Bot-authored threads were never re-reviewed post-fix.

None of the above is treated as silent resolution.

---

## 4. Thread-gate status (no path selected)

**Fact:** GraphQL reports **four** `#4932` threads with `isResolved: false` (RF-1…RF-4).

**Closeout requirement (unchosen):** for each of the four findings, maintainers must later supply **either** Path A (actual GitHub resolve) **or** Path B (explicit maintainer disposition recorded with date/rationale). This file records both as acceptable; it does **not** pick one.

| Finding | GraphQL resolved | Path A (GitHub resolve) | Path B (maintainer disposition note) | Thread-gate |
| ------- | ---------------- | ----------------------- | ------------------------------------ | ----------- |
| RF-1    | false            | **PENDING**             | **PENDING**                          | **OPEN**    |
| RF-2    | false            | **PENDING**             | **PENDING**                          | **OPEN**    |
| RF-3    | false            | **PENDING**             | **PENDING**                          | **OPEN**    |
| RF-4    | false            | **PENDING**             | **PENDING**                          | **OPEN**    |

| Supporting action                                                        | Owner                  | Status      |
| ------------------------------------------------------------------------ | ---------------------- | ----------- |
| Optional code-pointer replies on RF-1…RF-4 (helps either path)           | Maintainer / PR author | **PENDING** |
| Ensure `#4956` (or successor atomic PR) references this disposition file | PR author              | **PENDING** |
| Do not treat “code fixed on branch” as thread-gate close                 | Everyone               | Active rule |

---

## 5. Related but out-of-scope review surfaces

| Item                              | Notes                                                                                                                 |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `#4932` approval review by `4o3F` | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#pullrequestreview-4680010628 — approval ≠ finding disposition |
| Copilot PR overview review        | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#pullrequestreview-4680057410                                  |
| Codex review wrapper              | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#pullrequestreview-4680062051                                  |
| `#4956` review threads            | GraphQL returned **zero** threads at authoring time                                                                   |

---

## 6. Verdict

| Gate                                                      | State                                                                                                    |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| Four findings have identified URLs                        | **YES**                                                                                                  |
| Four findings have implemented S0x code/test dispositions | **YES** (on `refactor/pr-4s` working tree / commits; not a claim that `#4932` itself contains the fixes) |
| Four GitHub threads resolved (Path A)                     | **NO — PENDING**                                                                                         |
| Explicit maintainer disposition per finding (Path B)      | **NO — PENDING**                                                                                         |
| Thread-gate satisfied (Path A **or** Path B for each RF)  | **NO**                                                                                                   |
| PR-4S / S10 closable on review-disposition grounds        | **NO**                                                                                                   |

This file records disposition truth. It does **not** mark PR-4S complete.
