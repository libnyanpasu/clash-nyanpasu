# PR-3 T01 — specta 接入与 profile 类型导出 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** nyanpasu-config profile 域类型可生成 TS 绑定(add-only,命令面不变),并建立「绑定新鲜度」CI 检查;specta 2.x 对嵌套 tagged enum 的推导风险在本卡暴露并解决。

**Architecture:** 把 lib.rs 内联的 tauri-specta builder 构造抽成 `specta_export::build_specta_builder()`(单一事实源),在其上追加 `.typ::<T>()` 注册 profile 域类型;新增一个「导出即测试」的单测——它重生成 `bindings.ts`、跑 prettier、断言全部命名类型在场;CI 在 `test_unit` 作业后用 `git diff --exit-code` 保证提交的绑定与代码同步。

**Tech Stack:** tauri-specta 2.0.0-rc.25(`Builder::typ<T: Type>()`)、specta-typescript 0.0.12、prettier(经 npx)、GitHub Actions(`.github/workflows/ci.yml` 的 `test_unit` 作业)。

## Global Constraints(源自 task.md §0,逐条适用)

- 不新增 `::global()` / 静态可变服务;依赖显式注入(CLAUDE.md §7)。
- add-only:命令集不变、旧代码零行为变化;每个 commit `cargo build` + `cargo test` 绿。
- **wire 格式冻结**:不得为 TS 生成便利修改任何 `#[serde(...)]` 属性(域模型 #4840 已定稿);如 specta 推导失败,只允许加 `#[specta(...)]` 侧属性或手写 `Type` impl(不影响 serde)。
- 提交信息用 conventional commits;本卡建议 commit 见各 Task。
- 契约变更规则(task.md §5.3):实际导出的 TS 类型名清单须回写 T01 卡 Produces。

**基线事实(2026-07-06 实测,执行前可信):**

- `backend/tauri/Cargo.toml:33` 已有 `nyanpasu-config = { path = "../nyanpasu-config" }`(PR-2b 引入)——**本卡不需要加依赖**。
- specta builder 内联于 `backend/tauri/src/lib.rs:198-314`;导出仅发生在 debug 启动(`lib.rs:316-339`),路径 `../../frontend/interface/src/ipc/bindings.ts`,导出后 `npx prettier --write`。
- `.github/workflows/ci.yml` 无任何绑定检查;`test_unit` 作业(`ci.yml:195`)三 OS 矩阵跑 `pnpm test`(= `cargo test --manifest-path ./backend/Cargo.toml --all-features`)。
- profile 域类型全部已 derive `specta::Type`(含 struct_patch 生成的 `ProfileMetadataPatch`/`RemoteProfileOptionsPatch`);风险点 = `ProfileItem` 对 tagged enum 的 `#[serde(flatten)]`、`ProfileSource`/`LocalBinding` 的 tag+flatten 组合。

---

### Task 1: 抽出 `specta_export` 模块(纯搬移,单一事实源)

**Files:**

- Create: `backend/tauri/src/specta_export.rs`
- Modify: `backend/tauri/src/lib.rs:39`(删除 `use tauri_specta::{collect_commands, collect_events};`)、`lib.rs:197-314`(内联 builder 替换为函数调用)、模块声明区(加 `mod specta_export;`,与现有 `mod` 声明放一起)

**Interfaces:**

- Consumes: `lib.rs:198-314` 现有 builder 链(命令/事件清单逐字搬移)。
- Produces: `pub(crate) fn specta_export::build_specta_builder() -> tauri_specta::Builder<tauri::Wry>`(Task 2 与 lib.rs 共用)。

- [ ] **Step 1: 创建 `backend/tauri/src/specta_export.rs`**

内容 = 下述骨架,其中 `collect_commands![...]` 与 `collect_events![...]` 的参数列表**逐字复制** `lib.rs:199-306` 与 `lib.rs:307-313` 的现有列表(含注释行),不增不删:

```rust
//! Single source of truth for the tauri-specta builder.
//! Shared by `lib.rs` (runtime registration + debug export) and the
//! `export_typescript_bindings` test (CI freshness).

use tauri_specta::{collect_commands, collect_events};

use crate::{core, ipc, window};

pub(crate) fn build_specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            // ←—— 这里逐字粘贴 lib.rs:200-305 的整个命令清单(从 `// common` 到
            //      `ipc::get_system_accent_color,`),一行不改 ——→
        ])
        .events(collect_events![
            core::clash::ClashConnectionsEvent,
            core::clash::ws::ClashWsEvent,
            window::WindowMessageEvent,
            window::WindowReadyEvent,
            core::storage::StorageValueChangedEvent
        ])
        .dangerously_cast_bigints_to_number()
}
```

注意:`use crate::{core, ipc, window};` 让搬移过来的 `ipc::xxx` / `core::xxx` / `window::xxx` 路径原样可用(显式 use 优先于 extern prelude 的 `core`)。

- [ ] **Step 2: 修改 `lib.rs`**

1. 模块声明区加一行 `mod specta_export;`(按现有 mod 声明的字母序位置插入)。
2. 删除 `lib.rs:39` 的 `use tauri_specta::{collect_commands, collect_events};`(搬移后此处不再使用;`use specta_typescript::Typescript;` **保留**,debug 导出块仍用)。
3. `lib.rs:197-314` 的整个内联构造(`// setup specta` 注释起,到 `.dangerously_cast_bigints_to_number();` 止)替换为:

```rust
    // setup specta
    let specta_builder = specta_export::build_specta_builder();
```

`lib.rs:316-339` 的 `#[cfg(debug_assertions)]` 导出块与 `:353` `.invoke_handler(specta_builder.invoke_handler())`、`:364` `.mount_events` **不动**。

- [ ] **Step 3: 编译验证(行为不变)**

Run: `cargo check --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 编译通过,无 unused import 警告(若报 `collect_commands` unused 说明 Step 2.2 漏删)。

- [ ] **Step 4: Commit**

```bash
git add backend/tauri/src/specta_export.rs backend/tauri/src/lib.rs
git commit -m "refactor(tauri): extract specta builder into specta_export module"
```

---

### Task 2: 注册 profile 域类型 + 「导出即测试」(风险探针)

**Files:**

- Modify: `backend/tauri/src/specta_export.rs`(`.typ` 注册 + 测试模块)
- Generated: `frontend/interface/src/ipc/bindings.ts`(重生成,仅新增类型)

**Interfaces:**

- Consumes: `nyanpasu_config::profile::{Profiles, ProfileMetadataPatch, RemoteProfileOptionsPatch, ProfileValidationError}`(其余类型经字段引用传递导出)。
- Produces(T08/T09 依赖的 TS 命名类型,执行后以 bindings.ts 实际内容为准回写 T01 卡):`Profiles`、`ProfileItem`、`ProfileDefinition`、`ConfigDefinition`、`FileConfig`、`CompositionConfig`、`TransformDefinition`、`OverlayTransform`、`ScriptTransform`、`ScriptRuntime`、`ProfileSource`、`LocalBinding`、`ExternalMode`、`MaterializedFile`、`RemoteProfileOptions`、`SubscriptionInfo`、`ProfileMetadataPatch`、`RemoteProfileOptionsPatch`、`ProfileValidationError`(+ 传递闭包内的 `TransformOwner`/`CompositionMemberRole`;透明 newtype `ProfileId`/`ManagedProfilePath`/`ExternalProfilePath` 允许内联为 `string` 或命名别名,两者皆可接受)。

- [ ] **Step 1: 写失败测试(先于注册)**

在 `specta_export.rs` 末尾追加:

```rust
#[cfg(test)]
mod tests {
    use specta_typescript::Typescript;

    use super::build_specta_builder;

    const BINDINGS_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../frontend/interface/src/ipc/bindings.ts"
    );

    /// Regenerates the committed TS bindings in place (same path and header as
    /// the debug-run export in lib.rs), then asserts every profile domain type
    /// exports as a named TS type. CI enforces freshness via
    /// `git diff --exit-code` after `pnpm test` (ci.yml test_unit job).
    #[test]
    fn export_typescript_bindings() {
        build_specta_builder()
            .export(
                Typescript::default().header("/* oxlint-disable */\n// @ts-nocheck"),
                BINDINGS_PATH,
            )
            .expect("failed to export typescript bindings");

        let npx = if cfg!(target_os = "windows") {
            "npx.cmd"
        } else {
            "npx"
        };
        let status = std::process::Command::new(npx)
            .args(["prettier", "--write", BINDINGS_PATH])
            .status()
            .expect("failed to spawn prettier");
        assert!(status.success(), "prettier --write failed on bindings.ts");

        let generated =
            std::fs::read_to_string(BINDINGS_PATH).expect("bindings.ts must exist after export");
        for name in [
            "Profiles",
            "ProfileItem",
            "ProfileDefinition",
            "ConfigDefinition",
            "FileConfig",
            "CompositionConfig",
            "TransformDefinition",
            "OverlayTransform",
            "ScriptTransform",
            "ScriptRuntime",
            "ProfileSource",
            "LocalBinding",
            "ExternalMode",
            "MaterializedFile",
            "RemoteProfileOptions",
            "SubscriptionInfo",
            "ProfileMetadataPatch",
            "RemoteProfileOptionsPatch",
            "ProfileValidationError",
        ] {
            assert!(
                generated.contains(&format!("export type {name}"))
                    || generated.contains(&format!("export interface {name}")),
                "expected named TS export for {name}"
            );
        }
    }
}
```

- [ ] **Step 2: 跑测试确认失败(红)**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu export_typescript_bindings`
Expected: FAIL,断言信息 `expected named TS export for Profiles`(导出成功但类型未注册)。
副作用提示:此步已重写 bindings.ts(无新类型版),属预期;Step 4 会再次重生成。

- [ ] **Step 3: 注册类型(绿的最小实现)**

`build_specta_builder()` 的链尾(`.dangerously_cast_bigints_to_number()` 之后)追加:

```rust
        // PR-3 T01: profile domain types, add-only. Commands referencing them
        // arrive with T08; explicit registration keeps them exported (and the
        // specta nested-tagged-enum risk probed) before any command exists.
        .typ::<nyanpasu_config::profile::Profiles>()
        .typ::<nyanpasu_config::profile::ProfileMetadataPatch>()
        .typ::<nyanpasu_config::profile::RemoteProfileOptionsPatch>()
        .typ::<nyanpasu_config::profile::ProfileValidationError>()
```

(`Profiles` 传递闭包覆盖 item/definition/source/materialized 全链;两个 Patch 类型与校验错误枚举不被 `Profiles` 引用,须显式注册。)

- [ ] **Step 4: 跑测试确认通过(绿)——这就是 D11 风险探针**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu export_typescript_bindings`
Expected: PASS。

**若在此步失败**(specta 对 `#[serde(flatten)]` + 内部 tagged enum 组合报错或产出 `any`):这是 design §17 风险 2 的预期暴露点,处理原则(按序尝试,全部**不得改动 serde 属性**):

1. 给问题字段加 `#[specta(...)]` 侧属性(如 `#[specta(type = ...)]` 指向手写的镜像 TS 形状);
2. 为问题类型手写 `impl specta::Type`(参照 `profiles.rs:28` 已有的 `#[specta(type = Vec<ProfileItem>)]` 先例);
3. 记录实际采用的方案到本 plan 文件与 T01 卡(方案调整只影响本卡)。

- [ ] **Step 5: 检查生成产物与前端类型检查**

Run: `git diff --stat frontend/interface/src/ipc/bindings.ts` —— Expected: 仅新增类型声明,`commands`/`events` 段零变化。
Run: `pnpm lint:ts:interface && pnpm lint:ts:nyanpasu` —— Expected: PASS(bindings.ts 头部 `@ts-nocheck` 在位,新增类型不影响现有前端编译)。
Run: `pnpm lint:prettier` —— Expected: PASS(测试内已跑过 prettier)。

- [ ] **Step 6: 全量回归 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 全绿。

```bash
git add backend/tauri/src/specta_export.rs frontend/interface/src/ipc/bindings.ts
git commit -m "feat(tauri): export nyanpasu-config profile types via specta"
```

---

### Task 3: CI 绑定新鲜度检查

**Files:**

- Modify: `.github/workflows/ci.yml`(`test_unit` 作业,`- name: Test` 步骤之后,约 `:295`)

**Interfaces:**

- Consumes: Task 2 的导出测试(`pnpm test` 已把 bindings.ts 重生成为最新)。
- Produces: CI 失败语义——「提交的 bindings.ts 与代码不同步 → test_unit 红」。

- [ ] **Step 1: 追加检查步骤**

在 `test_unit` 作业 `- name: Test\n  run: pnpm test` 之后追加:

```yaml
- name: Check typescript bindings freshness
  if: matrix.os == 'ubuntu-latest'
  run: git diff --exit-code -- frontend/interface/src/ipc/bindings.ts
```

(先只在 ubuntu 强制:若三 OS 的 specta 输出存在平台差异——如 uwp 双 mod 存根签名不同——会表现为其余 OS 的本地重生成 diff。执行时若 ubuntu 上 diff 非空而本地 Windows 生成的提交版本不同,说明存在跨平台输出差异:以 ubuntu 产物为准重新提交 bindings.ts,并在 T01 卡记录「绑定以 linux 生成为准」。)

- [ ] **Step 2: 本地演练检查语义**

Run: `git diff --exit-code -- frontend/interface/src/ipc/bindings.ts`
Expected: exit 0(Task 2 已提交最新产物;若非 0 说明工作区有未提交的重生成残留,先 `git status` 排查)。

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: enforce typescript bindings freshness in test_unit"
```

---

### Task 4: 契约回写(task.md §5.3)

**Files:**

- Modify: `docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md`(T01 卡 Produces 段)

- [ ] **Step 1: 用 bindings.ts 实际导出的类型名清单替换 T01 卡 Produces 段的预测性描述**

从生成文件提取实名(命令:`grep -E "^export (type|interface) (Profile|Profiles|Config|Transform|Overlay|Script|File|Composition|Materialized|Remote|Subscription|LocalBinding|ExternalMode)" frontend/interface/src/ipc/bindings.ts`),逐名列入卡内;透明 newtype 的实际形态(命名别名 or 内联 string)如实记录。

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md
git commit -m "docs(pr3): record actual TS export names on T01 card"
```

---

## 验证总表(对应 T01 卡验证段)

| 判据                               | 覆盖步骤                  |
| ---------------------------------- | ------------------------- |
| `cargo build -p clash-nyanpasu` 绿 | Task 1 Step 3             |
| TS 绑定生成成功且含全部命名类型    | Task 2 Step 4             |
| CI TS diff 检查在位                | Task 3 Step 1             |
| 风险探针在本卡暴露                 | Task 2 Step 4(含降级预案) |
| 前端类型检查不受影响               | Task 2 Step 5             |
