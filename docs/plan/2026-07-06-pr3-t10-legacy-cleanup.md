# PR-3 T10 — legacy 清算(切换组终点)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 编译期保证 legacy profiles 面零残留:`config/profile/**`、`ProfilesJob`、`Config::profiles()`、`feat::update_profile`、旧 `enhance()` 管线全部删除;`grep` 判据(design §16 判据 1)全过。**本卡结束 = 应用恢复端到端可运行**,进入 T11。

**Architecture:** 纯删除 + 收尾清理。删除顺序按依赖反向:任务先删「消费者」(jobs/feat)再删「被消费者」(config/profile 类型),每步保持编译绿。`enhance` 模块**部分存活**:`ScriptType`/`Logs`/`LogSpan`/`PostProcessingOutput`/`create_lua_context`/builtin 脚本文件/T06 三件套/golden 是新管线依赖,legacy 链装配(`enhance()`/chain 装配/merge 应用/field 白名单/tun 注入/advice 分析)整体退役。

**Tech Stack:** grep 驱动的现场盘点(卡片原则:「plan 时以 grep 现场盘点为准,不硬编码文件列表」——本 plan 的清单是 2026-07-06 实测,执行时以 Step 内 grep 复核)。

## Global Constraints

- 一切 cargo 操作前设 `$env:CARGO_TARGET_DIR='F:\codex-target\clash-nyanpasu-pr3'`。
- 每 commit `cargo build` + `cargo test` 绿;T06A golden 套件全绿。
- 只删「本卡清单 + 因删除而成孤儿」的代码;与 profiles 无关的 legacy(verge/clash 全局等)不动。

## 基线事实(2026-07-06 实测;执行时逐条 grep 复核)

- `Config::profiles()` 剩余调用(T08 已清 ipc.rs 18 处):`feat.rs:449/456/462`(update_profile 本体)、`enhance/mod.rs:47`(legacy enhance,T07 已挂 dead_code)、`core/tasks/jobs/profiles.rs:89/179`。
- `feat::update_profile` 调用者:仅 `core/tasks/jobs/profiles.rs:33`(ipc.rs:255 已在 T08 改走 facade)。
- `crate::config::profile` 直接 import:仅 `enhance/utils.rs:5`(`ProfileUid`/`Profiles`)。
- `ProfilesJobGuard` 消费:`ipc.rs:6` import(T08 后若成孤儿则 T08 已剪;残留则本卡剪)+ jobs 注册点(`core/tasks/` 内注册 ProfilesJob 的 registry/scheduler,grep `ProfilesJob` 定位)。
- `Config` 结构(config/core.rs:15-20):`profiles_config: ManagedState<Profiles>` 字段 + `profiles()` accessor(:42-44);`IRuntime`/`Draft` 等其余字段保留(PR-4 范围)。
- `enhance/mod.rs` 现状:`enhance()` 已 `#[allow(dead_code)]`(T07);其专属 helpers:`merge_profiles`/`process_chain`/`convert_uids_to_scripts`(utils.rs)、chain 装配(chain.rs 的 ChainItem/装配逻辑)、`use_whitelist_fields_filter`/`HANDLE_FIELDS`(field.rs)、tun 注入(tun.rs)、advice 分析(advice.rs)、merge 应用(merge.rs 的 use_merge 部分)。
- **新管线仍依赖**:`chain.rs::ScriptType` + `PostProcessingOutput`(mod.rs:17/20 pub use;adapter/artifact_bridge/ipc 消费)、`utils.rs::{Logs, LogsExt, LogSpan}`、`merge.rs::create_lua_context`(adapter eval)、`enhance/builtin/*.{js,lua}`(runtime_builder include_str)、`script/**`、`content_source.rs`/`runtime_builder.rs`/`artifact_bridge.rs`/`golden.rs`。
- **advice BC(T07 既定)**:legacy `advice.rs` 的配置分析建议随 enhance 退役;新 `PostProcessingOutput.advice` 装 executor 的 Guard/Whitelist/Finalizing 日志(`artifact_bridge` 的兜底 match 臂)。前端 advice 展示内容变化,记 T11 复核。

## 契约修正(执行后回写 task.md T10 卡,§5.3)

1. 删除面按实测扩充:`enhance/{advice,field,tun}.rs` 全删;`chain.rs`/`merge.rs`/`utils.rs` 部分删(存活面见上);`enhance/mod.rs` 的 `enhance()` 及其 use 链清理。
2. `client/mod.rs:80 patch_profiles_config` 条目作废(方法不存在;T08 已删命令与 helper),本卡仅 grep 复核零残留。
3. advice 分析建议退役为已接受 BC(前端 advice 面显示 executor 阶段日志)。

---

### Task 1: 删 `ProfilesJob`(定时刷新旧路径)

**Files:**

- Delete: `backend/tauri/src/core/tasks/jobs/profiles.rs`
- Modify: `backend/tauri/src/core/tasks/jobs/mod.rs`(去 `mod profiles;` 与 re-export)
- Modify: jobs 注册点(grep 定位)

- [ ] **Step 1: 现场盘点**

Run: `Select-String -Path backend/tauri/src -Pattern "ProfilesJob" -Recurse`
Expected: 命中 jobs/profiles.rs 本体 + mod.rs + 注册点(tasks registry/guard)+ 可能的 ipc.rs import 残留。逐个记录。

- [ ] **Step 2: 删除**——`jobs/profiles.rs` 整文件;`mod.rs` 去声明;注册点去 `ProfilesJob` 项与 `ProfilesJobGuard` 类型(若 guard 定义在 jobs/profiles.rs 内则随文件亡;若在别处且无他用一并删);ipc.rs 残留 import 剪除。

- [ ] **Step 3: 验证 + Commit**

Run: `cargo build --manifest-path ./backend/Cargo.toml -p clash-nyanpasu && cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 绿;`Select-String -Pattern "ProfilesJob"` 零命中。

```powershell
git add -A backend/tauri/src
git commit -m "refactor(tauri)!: remove legacy profiles refresh job"
```

---

### Task 2: 删 `feat::update_profile` 本体

**Files:**

- Modify: `backend/tauri/src/feat.rs`(:444-470 附近整函数删除;`Borrow` 等孤儿 import 清理)

- [ ] **Step 1: 删除后验证**

Run: `cargo build --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 绿(Task 1 已删唯一调用者)。
Run: `Select-String -Path backend/tauri/src/feat.rs -Pattern "Config::profiles\(\)"`
Expected: 零命中。

- [ ] **Step 2: Commit**

```powershell
git add backend/tauri/src/feat.rs
git commit -m "refactor(tauri)!: remove feat::update_profile legacy path"
```

---

### Task 3: 退役 legacy enhance 管线(部分存活拆解)

**Files:**

- Modify: `backend/tauri/src/enhance/mod.rs`(删 `enhance()` 与其 use 链;保留新管线 re-export)
- Delete: `backend/tauri/src/enhance/advice.rs`、`field.rs`、`tun.rs`
- Modify: `backend/tauri/src/enhance/chain.rs`(仅保 `ScriptType`、`PostProcessingOutput` 及其直接依赖;chain 装配/ChainItem/builtin 旧表删除)
- Modify: `backend/tauri/src/enhance/merge.rs`(保 `create_lua_context` 与逐项求值 helpers;`use_merge` 旧应用删除)
- Modify: `backend/tauri/src/enhance/utils.rs`(保 `Logs`/`LogsExt`/`LogSpan`;`merge_profiles`/`process_chain`/`convert_uids_to_scripts` 及 `crate::config::profile` import 删除)

- [ ] **Step 1: 存活面反向确认**(删前跑,记录消费者)

Run: `Select-String -Path backend/tauri/src -Pattern "ScriptType|PostProcessingOutput|create_lua_context|LogsExt|LogSpan" -Recurse | Select-String -NotMatch "enhance/(mod|chain|merge|utils)"`
Expected: adapter.rs / artifact_bridge.rs / ipc.rs(get_postprocessing 类命令)/ runtime_builder.rs 等新面命中——这些符号必须存活。

- [ ] **Step 2: 逐文件删改**——顺序:mod.rs 删 `enhance()` + `use self::{chain::*, field::*, merge::*, tun::*}` glob 改为显式最小 import;删三个整文件;chain/merge/utils 内删 legacy 段。每删一段即 `cargo build` 快检,编译器牵引孤儿清理(仅限因本删除成孤儿者)。

- [ ] **Step 3: 验证 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 全绿(golden 4 测原样过——新管线未被触碰)。
Run: `Select-String -Path backend/tauri/src -Pattern "fn enhance\(" -Recurse`
Expected: 零命中。

```powershell
git add -A backend/tauri/src/enhance
git commit -m "refactor(tauri)!: retire legacy enhance pipeline"
```

---

### Task 4: 删 `Config::profiles()` accessor 与 `config/profile/**`

**Files:**

- Modify: `backend/tauri/src/config/core.rs`(删 `profiles_config` 字段、`profiles()` accessor、`Profiles` import 与 `Config::global()` 初始化项)
- Delete: `backend/tauri/src/config/profile/`(整目录)
- Modify: `backend/tauri/src/config/mod.rs`(去 `pub mod profile;` 与 re-export)
- Modify: 编译器牵引的残余 import(如 ipc.rs 顶部 `config::{profile::ProfileBuilder, *}` 已在 T08 剪;`profile::item_type::ProfileItemType` 等)

- [ ] **Step 1: 删前盘点**

Run: `Select-String -Path backend/tauri/src -Pattern "Config::profiles\(\)|config::profile::|profile::item" -Recurse`
Expected: 仅 config/ 内部自引用(Task 1–3 已清外部);逐条确认后删。

- [ ] **Step 2: 删除 + 编译牵引清理**

Run: `cargo build --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 按编译错逐个剪残余 import,直至绿。

- [ ] **Step 3: 判据全套(design §16 判据 1)**

```powershell
Select-String -Path backend/tauri/src -Pattern "Config::profiles\(\)" -Recurse   # 期望 0
Select-String -Path backend/tauri/src -Pattern "config::profile::" -Recurse      # 期望 0
Select-String -Path backend/tauri/src -Pattern "ProfilesBuilder|ProfileBuilder|ProfilesJobGuard" -Recurse   # 期望 0
```

Run: `cargo build --manifest-path ./backend/Cargo.toml -p clash-nyanpasu && cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 全绿。

- [ ] **Step 4: Commit**

```powershell
git add -A backend/tauri/src
git commit -m "refactor(tauri)!: delete legacy profiles config module"
```

---

### Task 5: 契约回写 + 全量复核

**Files:**

- Modify: `docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md`(T10 卡尾部)

- [ ] **Step 1: 回写执行修正块**

```markdown
**2026-07-06 执行修正(T10 实物)**:

- 删除面实测:`enhance/{advice,field,tun}.rs` 全删;`chain.rs` 仅存 `ScriptType`/`PostProcessingOutput`;`merge.rs` 仅存 `create_lua_context` 与逐项求值;`utils.rs` 仅存 `Logs`/`LogsExt`/`LogSpan`。
- advice 分析建议随 legacy enhance 退役(已接受 BC;新 advice 面 = executor Guard/Whitelist/Finalizing 日志),T11 前端复核。
- `client/mod.rs:80` 条目为规划期误记,实际无此方法(grep 复核零残留)。
```

- [ ] **Step 2: 最终验证 + Commit**

Run: 全部 §16 判据 grep + `cargo build` + `cargo test`(全绿)。

```powershell
git add docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md
git commit -m "docs(pr3): record T10 execution addenda in task card"
```

---

## Self-Review 结论

- 覆盖:卡片 6 项删除面全部落位 + 勘误 3 条(utils.rs 增补/锚点作废/基数);删除顺序消费者先行,每步编译绿。
- 无占位符:清单为实测;各 Step 内 grep 即卡片要求的「现场盘点为准」。
- 类型一致性:存活面(ScriptType/PostProcessingOutput/Logs/create_lua_context)与 T06/T07 消费者逐一对应。
