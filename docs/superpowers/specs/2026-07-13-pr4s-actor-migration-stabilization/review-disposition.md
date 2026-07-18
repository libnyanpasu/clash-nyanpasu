# PR-4S S10 — PR-4 Review Finding Disposition

**Status:** Code/test dispositions for S02/S03/S04/S08 are implemented on `refactor/pr-4s`. **Path A complete** — all four `#4932` review threads resolved on GitHub (`isResolved: true`) by authenticated actor `4o3F` on **2026-07-18**. **Review thread-gate satisfied.** Target-tip multi-OS CI is **PASS** at tip `10c837cd0068bb217e6195d286d6d022d9930f60`, run [29635372676](https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676) (success; Q-09…Q-11). PR-4S / S10 remain **OPEN** because manual smoke (`smoke-evidence.md` `E-01`…`E-11`) is still **PENDING**.
**Source PR:** [#4932](https://github.com/libnyanpasu/clash-nyanpasu/pull/4932) — `refactor(tauri)!: runtime derivation cleanup — checked promote pipeline and RebuildOutcome (PR-4)` (MERGED 2026-07-12).
**Stabilization PR (docs context only):** [#4956](https://github.com/libnyanpasu/clash-nyanpasu/pull/4956) draft on `refactor/pr-4s`.
**Disposition revised:** 2026-07-18 (UTC)
**Resolve evidence method:** authenticated GitHub API mutation responses for each thread id returned `isResolved: true` (actor `4o3F`, date 2026-07-18). No more-precise timestamp or separate resolve URL is invented; thread discussion URLs below remain the durable links.

---

## 0. Rules

1. Every PR-4 finding must have: thread URL, GraphQL thread id, owning S0x, code/test evidence pointer, and **explicit thread-gate state**.
2. Code disposition alone does **not** close the thread-gate; Path A (GitHub resolve) or Path B (explicit maintainer note) is required.
3. Do not invent reply URLs, sub-second timestamps, or resolve evidence beyond API-confirmed facts.
4. **Thread-gate:** satisfied when each finding has Path A **or** Path B. As of 2026-07-18, all four findings use **Path A**.
5. Thread-gate satisfaction and tip CI green do **not** close PR-4S / S10 while manual smoke remains incomplete.
6. **Authority split:** this file is the authority for the concise four-thread disposition ledger and Path A resolve evidence (IDs, URLs, `isResolved`, actor/date, owners, gate state). `design.md` owns protocol/semantics for the cited stages. `task.md` owns stage execution cards and verification checklists. Cross-links below are pointers only — not a claim that deleted long original-claim dossiers still live elsewhere.

---

## 1. Auditable disposition summary

One row per finding. All four rows share: GraphQL `isResolved: **true**`; Path A actor **`4o3F`**; date **2026-07-18**; Path B not required.

| ID   | Finding (short)                                                                  | Discussion URL                                                                 | GraphQL thread id       | Owner                      | Code/test evidence (pointers to design protocol / task verification)                                                                                                                                                                                                                                | Thread-gate            |
| ---- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ | ----------------------- | -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- |
| RF-1 | `createProfile` / `unwrapResult` can mask typed errors as success (`undefined`)  | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566317999 | `PRRT_kwDOKroWZM6QMw0z` | S08                        | `MutationOutcome` wire + exhaustive `unwrapResult` + Specta/bindings freeze; `task.md` S08; `design.md` §6.11 / §9                                                                                                                                                                                  | **SATISFIED / Path A** |
| RF-2 | Rollback unit test writes real user runtime product path                         | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566318012 | `PRRT_kwDOKroWZM6QMw08` | S02 (+ S10 ledger tooling) | `RuntimePaths` / TempDir injection; architecture-ledger gate + tip CI; commit lineage `807f1733`; `task.md` S02; `design.md` §6.1–6.2 / §8.4; tip CI `10c837cd…` run [29635372676](https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676) (`smoke-evidence.md` Q-09…Q-11 **PASS**) | **SATISFIED / Path A** |
| RF-3 | `change_core` only holds `rebuild_gate`; races `CoreManager::run_core` / restart | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566323210 | `PRRT_kwDOKroWZM6QMxw3` | S04 (+ S09 process matrix) | `CoreLifecycleLease` + barrier test `s04_concurrent_restart_waits_until_change_core_rollback_completes`; S09 fake-core matrix; `task.md` S04/S09; `design.md` §6.3 / §6.6 / §6.13                                                                                                                   | **SATISFIED / Path A** |
| RF-4 | Product rollback does not restore runtime read model                             | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#discussion_r3566323213 | `PRRT_kwDOKroWZM6QMxw5` | S03 (+ S04 lease span)     | `RuntimeTransactionSnapshot` restore product → Promoted → Applied; four runtime read IPCs read Promoted; `task.md` S03; `design.md` §4 / §6.4–6.5                                                                                                                                                   | **SATISFIED / Path A** |

**Path A batch record:** on **2026-07-18**, actor **`4o3F`** resolved all four `#4932` threads above; API mutation responses returned `isResolved: true` for each GraphQL thread id. No distinct resolve-event URL is claimed beyond the discussion URLs.

**Supporting actions**

| Action                                                                   | Owner     | Status                |
| ------------------------------------------------------------------------ | --------- | --------------------- |
| Path A resolve RF-1…RF-4                                                 | `4o3F`    | **Done** (2026-07-18) |
| Ensure `#4956` (or successor atomic PR) references this disposition file | PR author | Recommended / open    |
| Do not treat thread-gate or tip CI alone as PR-4S closeout               | Everyone  | Active rule           |

---

## 2. Related but out-of-scope review surfaces

| Item                              | Notes                                                                                                                 |
| --------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `#4932` approval review by `4o3F` | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#pullrequestreview-4680010628 — approval ≠ finding disposition |
| Copilot PR overview review        | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#pullrequestreview-4680057410                                  |
| Codex review wrapper              | https://github.com/libnyanpasu/clash-nyanpasu/pull/4932#pullrequestreview-4680062051                                  |
| `#4956` review threads            | GraphQL returned **zero** threads at earlier authoring time                                                           |

---

## 3. Verdict

| Gate                                                       | State                                                                                                                                                                                          |
| ---------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Four findings have identified discussion URLs              | **YES**                                                                                                                                                                                        |
| Four findings have GraphQL thread ids + `isResolved: true` | **YES** — actor `4o3F`, date 2026-07-18                                                                                                                                                        |
| Four findings have implemented S0x code/test dispositions  | **YES** (on `refactor/pr-4s`; not a claim that `#4932` itself contains the fixes)                                                                                                              |
| Four GitHub threads resolved (Path A)                      | **YES**                                                                                                                                                                                        |
| Explicit maintainer disposition per finding (Path B)       | **Not required** (Path A used for all four)                                                                                                                                                    |
| Thread-gate satisfied (Path A **or** Path B for each RF)   | **YES**                                                                                                                                                                                        |
| Target-tip multi-OS CI complete                            | **YES — PASS** @ tip `10c837cd0068bb217e6195d286d6d022d9930f60`; run [29635372676](https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676) (see `smoke-evidence.md` Q-09…Q-11) |
| Manual smoke E-records complete                            | **NO — PENDING** (`smoke-evidence.md` `E-01`…`E-11`)                                                                                                                                           |
| PR-4S / S10 closable overall                               | **NO**                                                                                                                                                                                         |

**Review thread-gate is satisfied. Tip multi-OS CI is green.** This file does **not** mark PR-4S / S10 complete while manual smoke remains open.
