# PR-4S S10 — Smoke Evidence Matrix

**Status:** OPEN — local Full QA (WSL2) + target-tip multi-OS CI recorded in §5; all manual / true-core `E-01`…`E-11` rows remain **PENDING**.
**Does not close PR-4S.** Tip CI green is not manual smoke completion.
**Spec anchors:** `design.md` §13.2; PR-4 `#4932` five manual scenarios; `task.md` S10.
**Evidence branch / tip (CI closeout):** `refactor/pr-4s` @ `10c837cd0068bb217e6195d286d6d022d9930f60`
**Target-tip CI run:** [https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676](https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676) — conclusion **success**
**CI fix commits (useful context):** `a88730fd`, `10c837cd`
**Matrix revised:** 2026-07-18 (UTC)
**Operator of this document:** documentation agent (S10 writer) — not a platform smoke operator

---

## 0. Honesty rules

1. Only a **canonical execution record** (`E-xxx`) may carry `result = PASS|FAIL|BLOCKED`. Scenario IDs (`P4-S*`, `D13-*`) are **coverage labels** that map to one or more `E-xxx` rows; they cannot PASS by referencing each other.
2. **PENDING** means “not executed / not recorded yet.” It is not a soft pass.
3. Automated coverage (`A`) is recorded separately and never substitutes for manual true-core / UI / network / OS-privileged paths (`M`).
4. Do not invent log paths, CI run URLs, screenshots, core version strings, operator names, or full-suite green claims.
5. Windows service-mode, macOS/Linux TUN, true-core UI, and live-network scenarios remain **manual** by design (design §6.13 / S09). S09 `fake-core` is std-only and never a production sidecar.

### Required fields on every canonical execution record

| Field            | Meaning                                                                                                      |
| ---------------- | ------------------------------------------------------------------------------------------------------------ |
| `commit`         | Exact git SHA of the build under test                                                                        |
| `build`          | How the binary / suite was produced (`pnpm build:debug`, artifact id, CI run URL, local command)             |
| `os`             | OS + version + arch                                                                                          |
| `app_version`    | App / package version when known                                                                             |
| `core_versions`  | Real mihomo / clash-rs / premium identities exercised (manual true-core only; use `n/a` for pure command QA) |
| `steps`          | Exact operator steps for this record (not a pointer to another scenario id)                                  |
| `result`         | `PASS` / `FAIL` / `BLOCKED` / `PENDING` + one-line outcome                                                   |
| `logs_artifacts` | Absolute path, gist/CI artifact URL, or `none-captured` with reason                                          |
| `operator`       | Human operator identity                                                                                      |
| `date`           | ISO date (`YYYY-MM-DD`) of execution                                                                         |

Until those fields are filled with real values, the record stays **PENDING**.

---

## 1. Coverage taxonomy

| Class                           | What counts                                                                 | Closeout role                           |
| ------------------------------- | --------------------------------------------------------------------------- | --------------------------------------- |
| **A — Automated focused**       | TempDir / fake adapters / `fake-core` / ledger unit tests                   | Correctness contracts only              |
| **M — Manual true environment** | Real app, real cores, real UI, real privileges, real network where required | Required for PR-4 five + §13.2 closeout |
| **G — Full QA commands**        | Workspace test/lint/typecheck/bindings + multi-OS tip CI                    | Required for S10 final QA; see §5       |

PR-4S smoke closeout requires every in-scope **M** `E-xxx` record to be PASS (or BLOCKED with owner + follow-up), plus §5 Full QA evidence. Mapping tables in §3 cannot satisfy that alone.

---

## 2. Tooling status (not smoke closeout)

These items are **implemented on the branch** (scripts / package integration / focused ledger tests + committed snapshot). Target-tip multi-OS CI evidence is now attached in §5 (Q-09…Q-11).

| Item                                                    | Status          | Notes                                                                                                                                                                                                                       |
| ------------------------------------------------------- | --------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Architecture ledger **gate mode**                       | **Implemented** | `scripts/architecture-ledger.ts --mode=gate`: exact stable-snapshot compare + hard fail when `test_real_dirs.total != 0`                                                                                                    |
| Committed stable snapshot                               | **Implemented** | `scripts/architecture-ledger.snapshot.json`                                                                                                                                                                                 |
| Package lint integration                                | **Implemented** | `package.json` → `lint:architecture-ledger` (pulled in by `pnpm lint` via `run-s lint:*`); report via `pnpm architecture-ledger`                                                                                            |
| Local focused ledger QA (script unit tests / gate path) | **Implemented** | `pnpm test:architecture-ledger` (+ gate machinery); **local run recorded in §5** (not tip CI)                                                                                                                               |
| Target-tip multi-OS CI run evidence for current tip     | **PASS**        | Tip `10c837cd0068bb217e6195d286d6d022d9930f60`; run https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676 (success). Per-OS lint/build/unit job URLs in Q-09…Q-11. CI fix commits `a88730fd` / `10c837cd`. |

---

## 3. Scenario maps (labels only — cannot PASS)

### 3.1 PR-4 original five (`#4932` body)

| Label | Scenario                                                              | Class | Primary canonical record(s) | Automated proxy (non-closing)                           |
| ----- | --------------------------------------------------------------------- | ----- | --------------------------- | ------------------------------------------------------- |
| P4-S1 | Boot first-forge + delete product fallback through check pipeline     | M     | **E-01**                    | A: boot/check/promote + S09 check-fail process row      |
| P4-S2 | Profile switch + clash `mode` / `allow-lan`                           | M     | **E-02**                    | A: profile mutation / wire tests                        |
| P4-S3 | mihomo ↔ clash-rs success + forced new-core failure hard rollback     | M     | **E-03**, **E-04**          | A: S04 lease barrier + S09 process change_core rollback |
| P4-S4 | Remote-dependent profile under network cut → committed-degraded toast | M     | **E-05**                    | A: import cancel/fail contracts only                    |
| P4-S5 | API-first clash patch when rebuild fails → Applied compensation       | M     | **E-06**                    | A: S05 Set/Remove / fence / exact-bytes tests           |

### 3.2 Design §13.2

| Label  | Scenario                                            | Class | Primary canonical record(s) | Automated proxy (non-closing)                  |
| ------ | --------------------------------------------------- | ----- | --------------------------- | ---------------------------------------------- |
| D13-01 | 首次启动 + 删除产品后的 fallback                    | M     | **E-01**                    | same as P4-S1 A proxy                          |
| D13-02 | profile 切换 + mode / allow-lan / **ipv6**          | M     | **E-02**                    | #4893 IPv6 fixture is A only                   |
| D13-03 | mixed-port fixed/random 与即时生效                  | M     | **E-07**                    | port fixtures where present; not OS bind proof |
| D13-04 | mihomo ↔ clash-rs 成功换核                          | M     | **E-03**                    | fake-core isolation only                       |
| D13-05 | 新核二进制故障时硬回滚                              | M     | **E-04**                    | S03/S04/S09 process proxies                    |
| D13-06 | remote-dependent 断网 committed-degraded            | M     | **E-05**                    | S07/S08 A only                                 |
| D13-07 | patch rebuild 失败后的 Applied-based compensation   | M     | **E-06**                    | S05 A only                                     |
| D13-08 | local / remote / composition 创建、导入、刷新、删除 | M     | **E-08**                    | S07/S08 materialization + import wire          |
| D13-09 | Windows service mode                                | M     | **E-09**                    | **None** in fake-core                          |
| D13-10 | macOS / Linux TUN 权限路径                          | M     | **E-10**, **E-11**          | **None** in fake-core                          |

A label is satisfied for closeout only when **every** listed `E-xxx` record is PASS (or BLOCKED with owner). Labels do not inherit status from sibling labels.

---

## 4. Canonical execution records (`E-xxx`)

All rows below are **PENDING**. Steps are the executable script for that record (self-contained).

| ID   | Covers labels           | Class | commit | build | os                                      | app_version | core_versions | steps                                                                                                                                                                                                                                                                  | result      | logs_artifacts | operator | date |
| ---- | ----------------------- | ----- | ------ | ----- | --------------------------------------- | ----------- | ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | -------------- | -------- | ---- |
| E-01 | P4-S1, D13-01           | M     | —      | —     | —                                       | —           | —             | 1. Launch app on real data; confirm core + proxies. 2. Stop app. 3. Delete `runtime/clash-config.yaml`. 4. Relaunch. 5. Confirm fallback used check→promote; no unchecked product on disk; core starts or fails visibly. 6. Record product hash before/after.          | **PENDING** | none           | —        | —    |
| E-02 | P4-S2, D13-02           | M     | —      | —     | —                                       | —           | —             | 1. Switch profile; confirm path updates. 2. Toggle `mode`, `allow-lan`, and `ipv6`. 3. Confirm immediate effect. 4. Confirm runtime read IPC/UI reflects **Promoted**.                                                                                                 | **PENDING** | none           | —        | —    |
| E-03 | P4-S3 (success), D13-04 | M     | —      | —     | —                                       | —           | —             | 1. With real sidecars, switch mihomo→clash-rs and reverse. 2. Confirm proxies work. 3. Confirm Promoted/Applied advance together on success.                                                                                                                           | **PENDING** | none           | —        | —    |
| E-04 | P4-S3 (failure), D13-05 | M     | —      | —     | —                                       | —           | —             | 1. Capture product bytes + core status. 2. Break/rename target core binary. 3. Attempt switch. 4. Confirm old core + old product/Promoted/Applied restore; error carries rollback chain.                                                                               | **PENDING** | none           | —        | —    |
| E-05 | P4-S4, D13-06           | M     | —      | —     | —                                       | —           | —             | 1. Use a remote-dependent profile target. 2. Cut live network. 3. Switch/activate. 4. Confirm list/state committed; warning toast for degradation; logs show phase/code; not silent hard success.                                                                      | **PENDING** | none           | —        | —    |
| E-06 | P4-S5, D13-07           | M     | —      | —     | —                                       | —           | —             | 1. Force rebuild failure (bad profile / injected rebuild error). 2. API-first patch `mode` (or equivalent). 3. Confirm error/degraded path. 4. Confirm running core compensated from **Applied** (Promoted may stay ahead; P3/P1 matrix).                              | **PENDING** | none           | —        | —    |
| E-07 | D13-03                  | M     | —      | —     | —                                       | —           | —             | 1. Fixed mixed-port: set, apply/restart, confirm listen + immediate effect. 2. Random/ephemeral path: confirm reseed, no stale consumer. 3. Optional: port held by old process → structured failure/recover.                                                           | **PENDING** | none           | —        | —    |
| E-08 | D13-08                  | M     | —      | —     | —                                       | —           | —             | 1. Create local profile. 2. Import remote (live network success). 3. Create composition if UI allows. 4. Manual remote refresh. 5. Delete each. 6. Confirm no orphan files; materialization failure not naked success; cancel import mid-fetch leaves zero state/file. | **PENDING** | none           | —        | —    |
| E-09 | D13-09                  | M     | —      | —     | Windows only                            | —           | —             | 1. Install/enable service mode. 2. Start/stop/change-core/recover via service path. 3. Confirm lifecycle serialization. 4. Capture service logs.                                                                                                                       | **PENDING** | none           | —        | —    |
| E-10 | D13-10 (macOS)          | M     | —      | —     | macOS                                   | —           | —             | 1. Enable TUN. 2. Exercise permission grant/deny. 3. Confirm desired stays committed on effect failure; no unchecked product. 4. Capture OS prompts + app logs.                                                                                                        | **PENDING** | none           | —        | —    |
| E-11 | D13-10 (Linux)          | M     | —      | —     | Linux (non-WSL preferred for TUN claim) | —           | —             | Same as E-10 on Linux. WSL2 is **not** automatically valid TUN evidence.                                                                                                                                                                                               | **PENDING** | none           | —        | —    |

### Platform board (derived from E-records; not independent PASS source)

| Platform | Blocking E-records still PENDING | Notes                                   |
| -------- | -------------------------------- | --------------------------------------- |
| Windows  | E-01…E-09 as in scope            | E-09 required for service-mode closeout |
| macOS    | E-01…E-08, E-10                  | E-10 for TUN                            |
| Linux    | E-01…E-08, E-11                  | E-11 for TUN                            |

Authoring note (not an execution record): matrix revised on WSL2 Linux at `b70aefb0`. No manual smoke was executed for this revision.

---

## 5. Full QA command log (S10 only)

**Purpose:** auditable command-level evidence for S10 final QA.

### 5.1 Shared local environment (Q-01…Q-08, Q-12…Q-17)

| Field             | Value                                                                                        |
| ----------------- | -------------------------------------------------------------------------------------------- |
| `commit`          | Base: `b70aefb0` **plus uncommitted S10 working tree** (not a pure tip SHA; not multi-OS CI) |
| `build` / context | Local commands on WSL2 checkout `refactor/pr-4s`                                             |
| `platform` / `os` | WSL2 Linux (`Linux … microsoft-standard-WSL2`, x86_64)                                       |
| `operator`        | Claude Code local QA                                                                         |
| `date`            | 2026-07-18                                                                                   |
| `logs_artifacts`  | Durable **in-document** result strings below (no external CI URL)                            |

**Honesty bound:** Q-01…Q-08 and Q-12…Q-17 remain **local** durable records (base `b70aefb0` + earlier uncommitted S10 WT). Target-tip multi-OS CI is separately recorded in Q-09…Q-11 at tip `10c837cd0068bb217e6195d286d6d022d9930f60`. Local rows do **not** substitute for manual `E-xxx` smoke.

### 5.2 Command matrix

| ID   | command                                                                                        | commit                                     | build / cwd context           | platform   | exit / result                                                                                                                                               | log / CI URL                                                                                                                                                                                                                                                                                                                                                                 | operator             | date       |
| ---- | ---------------------------------------------------------------------------------------------- | ------------------------------------------ | ----------------------------- | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- | ---------- |
| Q-01 | `pnpm lint:architecture-ledger`                                                                | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0; gate metrics `config_calls=120` / `service_globals=80` / `migration_markers=18` / `legacy_dto=300` / `test_real_dirs=0` / `bridge_files=8` | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-02 | `pnpm test:architecture-ledger`                                                                | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0; **21/21** tests                                                                                                                            | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-03 | `cargo test --manifest-path ./backend/Cargo.toml --all-features`                               | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0; **644 passed / 0 failed / 2 ignored** (exact full Cargo Test local run)                                                                    | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-04 | `cargo test --manifest-path ./backend/Cargo.toml --workspace --all-features`                   | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0; **644 passed / 0 failed / 2 ignored**                                                                                                      | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-05 | `cargo clippy --manifest-path ./backend/Cargo.toml --all-targets --all-features`               | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0; pre-existing warnings only (no new blocking errors)                                                                                        | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-06 | `pnpm lint` (includes `lint:architecture-ledger` gate)                                         | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0; proves gate orchestration under `lint` (prettier/oxlint/deno/styles/ts/clippy/rustfmt/architecture-ledger)                                 | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-07 | interface + nyanpasu TS lint (`pnpm lint:ts:interface`, `pnpm lint:ts:nyanpasu`)               | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0                                                                                                                                             | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-08 | Specta / bindings freshness (`git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`) | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0 (no bindings drift)                                                                                                                         | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-09 | Target-tip CI — Windows                                                                        | `10c837cd0068bb217e6195d286d6d022d9930f60` | CI run 29635372676 on tip SHA | Windows    | **PASS** conclusion success (lint + build + unit jobs)                                                                                                      | run https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676 (success); lint https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676/job/88056673388; build https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676/job/88057370252; unit https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676/job/88057370281 | GitHub Actions       | 2026-07-18 |
| Q-10 | Target-tip CI — macOS                                                                          | `10c837cd0068bb217e6195d286d6d022d9930f60` | CI run 29635372676 on tip SHA | macOS      | **PASS** conclusion success (lint + build + unit jobs)                                                                                                      | run https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676 (success); lint https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676/job/88056673401; build https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676/job/88057370261; unit https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676/job/88057370250 | GitHub Actions       | 2026-07-18 |
| Q-11 | Target-tip CI — Linux                                                                          | `10c837cd0068bb217e6195d286d6d022d9930f60` | CI run 29635372676 on tip SHA | Linux      | **PASS** conclusion success (lint + build + unit jobs)                                                                                                      | run https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676 (success); lint https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676/job/88056673370; build https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676/job/88057370256; unit https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676/job/88057370269 | GitHub Actions       | 2026-07-18 |
| Q-12 | Deno fmt + Deno check (`pnpm lint:deno` / deno fmt --check + deno check scripts)               | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0                                                                                                                                             | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-13 | `cargo build --manifest-path ./backend/Cargo.toml -p fake-core`                                | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0                                                                                                                                             | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-14 | `cargo fmt --manifest-path ./backend/Cargo.toml --all -- --check`                              | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0                                                                                                                                             | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-15 | `pnpm --filter=@nyanpasu/interface build` (interface build)                                    | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0                                                                                                                                             | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-16 | `pnpm web:build` (web / nyanpasu build)                                                        | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0                                                                                                                                             | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |
| Q-17 | `git diff --check` (owned S10 evidence files / working tree whitespace)                        | `b70aefb0` + uncommitted S10 WT            | local WSL2                    | WSL2 Linux | **PASS** exit 0                                                                                                                                             | in-document only                                                                                                                                                                                                                                                                                                                                                             | Claude Code local QA | 2026-07-18 |

### 5.3 Local vs tip closeout

| Claim                                                                             | State                                                                                                                                                                                              |
| --------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Local focused ledger + gate + full cargo/clippy/lint/TS/build/bindings on WSL2 WT | **Recorded PASS** (Q-01…Q-08, Q-12…Q-17)                                                                                                                                                           |
| Final target commit re-run / linked on tip SHA                                    | **Complete** — tip `10c837cd0068bb217e6195d286d6d022d9930f60`; durable run https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676 (success); CI fix commits `a88730fd`, `10c837cd` |
| Multi-OS tip CI (Q-09…Q-11)                                                       | **PASS** — Windows/macOS/Linux lint+build+unit job URLs recorded                                                                                                                                   |
| Manual smoke `E-01`…`E-11`                                                        | **PENDING** (unchanged)                                                                                                                                                                            |

---

## 6. Artifact drop zone

When executing an `E-xxx` or `Q-xxx` row, attach durable artifacts, for example:

- App log fields: `runtime.revision`, `runtime.promoted_revision`, `runtime.applied_revision`, `mutation.outcome`
- Product hash before/after fallback or rollback
- Screenshot of committed-degraded toast (E-05)
- Service manager / TUN permission captures (E-09…E-11)
- Command transcripts or CI run URLs (Q-01…Q-17; tip CI for Q-09…Q-11)

Local Q rows use in-document result strings as durable artifacts. Tip CI Q-09…Q-11 use durable GitHub Actions URLs. Manual `E-xxx` rows still require external artifacts before PASS.

---

## 7. Closeout checklist (smoke + S10 QA)

- [ ] E-01…E-11 in-scope rows PASS or BLOCKED with owner + follow-up
- [ ] Every P4-S\* / D13-\* label’s mapped E-records are satisfied (no circular label PASS)
- [x] Local Full QA command log filled (Q-01…Q-08, Q-12…Q-17 on WSL2 WT @ `b70aefb0` + uncommitted S10)
- [x] Local results re-run/linked on final target commit (`10c837cd0068bb217e6195d286d6d022d9930f60` + run https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/29635372676)
- [x] Target-tip multi-OS CI evidence attached (Q-09…Q-11)
- [ ] No PASS without artifacts (local Q: in-document; tip CI: Actions URLs; E-records: still missing)
- [ ] A proxies not substituted for M records

**Current verdict:** local Full QA **recorded**; target-tip multi-OS CI **PASS** @ `10c837cd0068bb217e6195d286d6d022d9930f60`; manual smoke **incomplete**. PR-4S / S10 remain **OPEN**.
