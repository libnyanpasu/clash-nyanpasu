# PR-5 R0 — nyanpasu-runtime 错误分类协议收敛 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `error_kind` 的字符串表从「ipc 常量模块 + service 侧 `map_error_kind` 匹配表」两份收敛为一份 —— `nyanpasu-core-metadata::CoreErrorKind` enum;分类逻辑下沉到错误定义所在的 `nyanpasu-core-manager::Error::kind()`;wire 字节完全不变;submodule bump 后 clash app 可直接消费 typed kind,不必在 app 侧复制第三份字符串表。

**Architecture:** 单向依赖已经现成,不需要新 crate:

```text
nyanpasu-core-metadata  (CoreErrorKind —— 唯一字符串表)
        ↑                        ↑
nyanpasu-core-manager      nyanpasu-ipc
 (Error::kind() 分类表)    (api 重导出 + client 解析)
        ↑                        ↑
        └── nyanpasu-service-runtime (只做 error → envelope 搬运)
```

`nyanpasu_ipc/Cargo.toml` 已有 `nyanpasu-core-metadata = { path = "../crates/nyanpasu-core-metadata" }`,`crates/nyanpasu-core-manager/Cargo.toml` 同样已有 —— **本 PR 不新增任何依赖项**。

**Tech Stack:** Rust nightly(`rust-toolchain.toml` 为浮动 `channel = "nightly"`)、serde、specta、thiserror。

**Spec:** `docs/superpowers/specs/2026-08-01-pr5-core-actor/design.md` §4.1「复用 runtime 类型」+ `task.md` §R0。
（注:该目录下两文件的内容与文件名曾互换,leader 已于 2026-08-01 修正 —— 现在 `design.md` = 设计正文、`task.md` = 任务清单,本行引用按修正后的文件名。）

---

## Global Constraints

- **作用域仅限 submodule** `backend/nyanpasu-runtime`(独立 git 仓库 + 独立 cargo workspace)。app 侧 `backend/tauri` 一行不改。
- **不新建 crate、不引入 `CoreEngine` trait/工厂、不引入 transport-neutral 抽象层。**
- **不 bump 任何 crate 版本。** 尤其 `nyanpasu_service/Cargo.toml` 的 `version = "2.0.0-rc.1"` —— app 的 `scripts/check.ts:674-691` 直接读这个文件的 `version` 字段拼出下载 tag `v2.0.0-rc.1`,改动它会让 sidecar 下载 404。`crates/nyanpasu-service-runtime` 的版本同样不动(它是 `consts::APP_VERSION`,ServiceCompat 的 major fail-closed 判据)。
- **wire 字节不变是硬闸门。** envelope 字段 `R.error_kind` 的类型保持 `Option<Cow<'a, str>>`,12 个字符串一个字符不改;`crates/nyanpasu-service-runtime/src/server/routing/tests.rs:388,414,448,475` 四条端到端断言必须**原样不动地通过** —— 它们是「wire 没动」的最直接证据。
- **不推送、不开 PR。** 全部工作是 submodule 内的本地 commit。推送到 `libnyanpasu/nyanpasu-runtime` 与开上游 PR 需要用户显式授权,见 §「交接边界」。
- **不动 app 侧 submodule pin(gitlink)。** 见 §「交接边界」中的 interim consumption 说明,那是 leader 的决策不是本计划的执行项。
- **不动嵌套 submodule** `crates/nyanpasu-utils`(当前 `3cb3af0`)。理由见 Task 0 Step 4。
- pre-commit 与 CI 跑 `cargo clippy --all-targets --all-features`(`.github/workflows/ci.yml:52`)。`.cargo/config.toml` 已注入 `--cfg tokio_unstable --cfg tracing_unstable`,不需要手动带。
- ICE 处置:本机 nightly 偶发 `query stack during panic`,**直接重跑**,不得据此 pin toolchain 日期。

---

## Facts established before planning (do not re-derive)

| Fact                    | Value                                                                                                                                                 | Source                                                                   |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ | ---------- |
| submodule 当前状态      | 分支 `main` @ `0d2993a`(**不是 detached**),`= v2.0.0-rc.1`,落后 `origin/main` 1 个 commit                                                             | `git -C backend/nyanpasu-runtime status -sb`                             |
| tag→main 分歧           | `v2.0.0-rc.1..origin/main` = **仅 1 个 commit** `0c67f56 chore: update docs`(只改 `README.md`,+115/-18)                                               | `git log --oneline v2.0.0-rc.1..origin/main` / `git show --stat 0c67f56` |
| 反向分歧                | `origin/main..v2.0.0-rc.1` = **空**,tag 是 main 的严格祖先(纯快进)                                                                                    | `git log --oneline origin/main..v2.0.0-rc.1`                             |
| 字符串常量表            | 12 个 `pub const`                                                                                                                                     | `nyanpasu_ipc/src/api/mod.rs:38-66`                                      |
| 分类匹配表              | `map_error_kind(&ManagerError) -> Option<&'static str>`                                                                                               | `crates/nyanpasu-service-runtime/src/server/manager_bridge.rs:646-665`   |
| `ManagerError` 变体总数 | **25**(报告写「23」是 2026-07-30 的旧计数)                                                                                                            | `crates/nyanpasu-core-manager/src/error.rs:7-72`                         |
| client 侧               | 只搬运 `Option<String>`,**今天没有任何字符串表**                                                                                                      | `nyanpasu_ipc/src/client/mod.rs:51-53,142,187`                           |
| app 侧消费              | `rg 'error_kind                                                                                                                                       | ClientError::Server' backend/tauri/src` = **0 命中**                     | 计划期实测 |
| 依赖就绪                | ipc → core-metadata ✓;core-manager → core-metadata ✓;service-runtime → 两者 ✓                                                                         | 各 `Cargo.toml`                                                          |
| 重导出先例              | `nyanpasu_ipc/src/api/ws/events.rs:9-11`、`crates/nyanpasu-core-manager/src/log.rs:12`、`.../kind.rs:10` 都是 `pub use nyanpasu_core_metadata::{...}` | 同上                                                                     |
| 同层 enum 先例          | `ApplyOutcomeKind`:`#[serde(rename_all = "snake_case")]`,**未标 `#[non_exhaustive]`**                                                                 | `nyanpasu_ipc/src/api/core/apply.rs:34-60`                               |
| 覆盖率闸门              | `cargo llvm-cov -p nyanpasu-service-runtime -p nyanpasu-ipc --fail-under-lines 53`(2026-07-29 基线 55.42%)                                            | `.github/workflows/integration.yml:127-134`                              |

---

## error_kind 全量清单(计划的核心,codex 不必重新发现)

### 一、字符串值(12 个,全部定义于 `nyanpasu_ipc/src/api/mod.rs:38-66`)

| #   | 常量                            | wire 字符串             | 目标 enum 变体        | `rename_all="snake_case"` 是否精确匹配 |
| --- | ------------------------------- | ----------------------- | --------------------- | -------------------------------------- |
| 1   | `NOT_STARTED` (`:40`)           | `not_started`           | `NotStarted`          | ✓                                      |
| 2   | `ALREADY_RUNNING` (`:42`)       | `already_running`       | `AlreadyRunning`      | ✓                                      |
| 3   | `REVISION_CONFLICT` (`:45`)     | `revision_conflict`     | `RevisionConflict`    | ✓                                      |
| 4   | `QUARANTINED` (`:49`)           | `quarantined`           | `Quarantined`         | ✓                                      |
| 5   | `CONFIG_CHECK_FAILED` (`:51`)   | `config_check_failed`   | `ConfigCheckFailed`   | ✓                                      |
| 6   | `CONFIG_NOT_FOUND` (`:52`)      | `config_not_found`      | `ConfigNotFound`      | ✓                                      |
| 7   | `BINARY_NOT_FOUND` (`:53`)      | `binary_not_found`      | `BinaryNotFound`      | ✓                                      |
| 8   | `INVALID_CONFIG` (`:55`)        | `invalid_config`        | `InvalidConfig`       | ✓                                      |
| 9   | `CONTROLLER_MISSING` (`:58`)    | `controller_missing`    | `ControllerMissing`   | ✓                                      |
| 10  | `APPLY_FAILED` (`:60`)          | `apply_failed`          | `ApplyFailed`         | ✓                                      |
| 11  | `APPLY_ROLLBACK_FAILED` (`:62`) | `apply_rollback_failed` | `ApplyRollbackFailed` | ✓                                      |
| 12  | `STOP_UNCONFIRMED` (`:65`)      | `stop_unconfirmed`      | `StopUnconfirmed`     | ✓                                      |

**12/12 精确匹配 `rename_all = "snake_case"`,不需要任何 `#[serde(rename = "...")]` 逐项覆写。**

### 二、生产端(谁往 wire 上写 kind)

| 位置                                                          | 输入                                                 | 产出 kind                              | R0 后                                                     |
| ------------------------------------------------------------- | ---------------------------------------------------- | -------------------------------------- | --------------------------------------------------------- | --- | --------------------------- |
| `manager_bridge.rs:646-665` `map_error_kind`                  | `&ManagerError`                                      | 12 个中的一个或 `None`                 | **整函数删除**,改调 `error.kind()`                        |
| `manager_bridge.rs:89` `From<ManagerError> for OpError`       | `ManagerError`                                       | 同上                                   | `kind: error.kind()`                                      |
| `manager_bridge.rs:404`                                       | `find_binary_path` 的 `io::Error`                    | `BINARY_NOT_FOUND`                     | `CoreErrorKind::BinaryNotFound`(**逐点分类,不是表**,保留) |
| `manager_bridge.rs:791`                                       | `canonical_config_path` 的 `io::ErrorKind::NotFound` | `CONFIG_NOT_FOUND`                     | `CoreErrorKind::ConfigNotFound`(同上,保留)                |
| `manager_bridge.rs:82` `OpError::into_envelope`               | `OpError`                                            | 落到 `RBuilder::other_error_with_kind` | 签名收敛为 `Option<CoreErrorKind>`                        |
| `nyanpasu_ipc/src/api/mod.rs:138-147` `other_error_with_kind` | `Option<Cow<str>>`                                   | 写入 `R.error_kind`                    | 入参改 `Option<CoreErrorKind>`,内部 `.map(                | k   | Cow::Borrowed(k.as_str()))` |

唯一写 `R.error_kind` 非 `None` 的构造器就是 `other_error_with_kind`(`api/mod.rs:130`/`:156` 的另两个构造器恒写 `None`)。收敛它的签名 = 从类型上杜绝手写字符串上 wire。

### 三、消费端(谁读 kind)

| 位置                                          | 现状                                                                                     | R0 后                                                |
| --------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| `nyanpasu_ipc/src/client/mod.rs:142` (`send`) | `envelope.error_kind.map(into_owned)` → `ClientError::Server.error_kind: Option<String>` | **不变**(保留 raw,前向兼容)                          |
| `nyanpasu_ipc/src/client/mod.rs:187` (`call`) | 同上                                                                                     | **不变**                                             |
| app `backend/tauri`                           | 0 命中                                                                                   | 仍 0 —— R0 只把 typed 消费**变为可能**,不在 app 落地 |

### 四、`ManagerError` 全 25 变体 → kind 映射(`Error::kind()` 的完整真值表)

| #   | 变体(`error.rs` 行号)                     | kind                                                     |
| --- | ----------------------------------------- | -------------------------------------------------------- |
| 1   | `AlreadyRunning` (`:9`)                   | `AlreadyRunning`                                         |
| 2   | `NotStarted` (`:11`)                      | `NotStarted`                                             |
| 3   | `ConfigNotFound(_)` (`:13`)               | `ConfigNotFound`                                         |
| 4   | `BinaryNotFound(_)` (`:15`)               | `BinaryNotFound`                                         |
| 5   | `CoreVersionProbeFailed{..}` (`:17`)      | `None`                                                   |
| 6   | `ControllerMissing` (`:22`)               | `ControllerMissing`                                      |
| 7   | `RequiredLocalIpcUnsupported{..}` (`:26`) | `None`                                                   |
| 8   | `ConfigCheckFailed(_)` (`:28`)            | `ConfigCheckFailed`                                      |
| 9   | `InvalidConfig(_)` (`:30`)                | `InvalidConfig`                                          |
| 10  | `InvalidManagerOptions(_)` (`:32`)        | `None`                                                   |
| 11  | `InvalidHealthPolicy(_)` (`:34`)          | `None`                                                   |
| 12  | `UnsafeRuntimeArtifact(_)` (`:36`)        | `None`                                                   |
| 13  | `RuntimeDirectoryOwned(_)` (`:38`)        | `None`                                                   |
| 14  | `StopUnconfirmed(_)` (`:40`)              | `StopUnconfirmed`                                        |
| 15  | `ManagerQuarantined{..}` (`:42`)          | `Quarantined`                                            |
| 16  | `RevisionConflict{..}` (`:44`)            | `RevisionConflict`                                       |
| 17  | `ApplyFailed(_)` (`:49`)                  | `ApplyFailed`                                            |
| 18  | `ApplyRollbackFailed{..}` (`:51`)         | `ApplyRollbackFailed`                                    |
| 19  | `DurabilityUncertain{source,..}` (`:53`)  | **`source.kind()`(递归)**                                |
| 20  | `StartupTimeout{..}` (`:61`)              | `None`                                                   |
| 21  | `StartupFailed{..}` (`:63`)               | `None`                                                   |
| 22  | `Process(_)` (`:65`)                      | `None`                                                   |
| 23  | `Api(_)` (`:67`)                          | `None`                                                   |
| 24  | `Yaml(_)` (`:69`)                         | `InvalidConfig`(与 `InvalidConfig` 合并,沿用现状 `:655`) |
| 25  | `Io(_)` (`:71`)                           | `None`                                                   |

合计:13 个变体 → 12 个 kind;11 个变体 → `None`;1 个递归。**R0 不改变这个分类集合** —— 把 11 个 `None` 补齐是报告的 P3,属于 wire 新增,不在本 PR。

---

## 关键设计决策(已裁定,codex 无需再选)

### D1 — 枚举放 `nyanpasu-core-metadata`

理由与 `LogFrame` 完全同构(`crates/nyanpasu-core-metadata/src/log.rs:1-6` 的模块注释就是这个论证):三层需要同一个类型 —— core-manager 产出、ipc 承载、service 搬运、app 消费,而 core-metadata 是 ipc 与 core-manager **唯一的公共下游**。放 ipc 会让 core-manager 反向依赖 ipc;放 core-manager 会让 ipc 依赖整个 manager。

### D2 — `Error::kind()` 返回 `Option<CoreErrorKind>`

保留 `Option` 而不是加 `Unknown`/`Unclassified` 变体:现状注释(`manager_bridge.rs:641-645`)写得很清楚 —— 「未分类」必须是「没有 kind」,不能是猜的 kind。`None` 在 wire 上表现为**整个 `error_kind` 键被省略**(`api/mod.rs:84` 的 `skip_serializing_if`),这是既有 wire 语义,加变体会破坏它。

### D3 — 匹配必须**穷尽**,禁止 `_ => None` 通配

`map_error_kind` 今天必须写通配符,因为 `ManagerError` 对**下游 crate**是 `#[non_exhaustive]`。`kind()` 搬进 core-manager **自身** crate 后,`#[non_exhaustive]` 不再生效,可以写穷尽匹配。**这是本 PR 最大的实质收益**:今后给 `Error` 加变体会得到编译错误而不是静默 `None`。

> codex 注意:11 个未分类变体必须**逐个显式列出** `=> None`,不得偷懒写 `_ => None`。写了通配符 = 本 PR 白做。

### D4 — `R.error_kind` 字段类型**不变**,仍是 `Option<Cow<'a, str>>`

这是前向兼容的关键。若改成 `Option<CoreErrorKind>`:一个**更新的 service** 发来老 client 不认识的 kind(P3 会新增),会让**整个 envelope 解码失败**,而不是丢一个字段 —— 正是 `docs/superpowers/specs/2026-07-30-ipc-protocol-evolution-report.md:129` 拒绝动 `ResponseCode` 枚举的那个失败模式。app 通过 auto-update 让 daemon 版本 ≥ GUI 版本是常态,「新 service + 老 client」真实存在。

同理 `ClientError::Server.error_kind` 保持 `Option<String>`(原始字符串不丢),typed 访问通过新增的 `ClientError::core_error_kind()` 访问器提供。

### D5 — `CoreErrorKind` **不标** `#[non_exhaustive]`

对齐同协议层的 `ApplyOutcomeKind`(`api/core/apply.rs:34`,未标)。理由:解码侧 `from_wire` 返回 `Option`,未知 wire 字符串**永远不会**构造出未知变体,类型本身可以是封闭的;而封闭 enum 让「上游新增 kind」在 app 侧变成编译错误而不是被通配臂静默吞掉 —— service 与 GUI 同版本分发,这正是想要的行为。
（此项是判断题,已在报告中标为 leader 复核项,见 §「需 leader 裁决」。）

### D6 — 删除 `pub mod error_kind` 常量模块,不保留 deprecated 别名

Exit 判据要求「不再各自维护第二份表」。保留 12 个 `pub const NOT_STARTED: &str = CoreErrorKind::NotStarted.as_str();` 虽然只有一份真值,但那是 CLAUDE.md §11 意义上的兼容层,且没有被阻塞的理由 —— 外部消费者实测为 0(app 侧 `rg` 零命中,ipc crate 未发布到 crates.io)。按「优先可迁移的破坏性变更」直接删。

### D7 — `as_str()` 与 serde 的双表用测试钉死

`as_str() -> &'static str` 是必需的(envelope 需要 `&'static str` 做 `Cow::Borrowed`,不能走 serde)。它与 `#[serde(rename_all)]` 构成两种表示 —— 这是 Rust 的标准写法,`ClashCoreKind`(`crates/nyanpasu-core-metadata/src/kind.rs:31-39` 的 `AsRef<str>` + 逐项 `#[serde(rename)]`)在同一个 crate 里就是这么写的。用一条测试遍历 `ALL` 断言 `serde_json::to_string(&kind) == format!("\"{}\"", kind.as_str())` 把两者钉死即可,**不允许再手写第三份表**:`from_wire` 必须通过扫描 `ALL` + `as_str()` 实现。

---

## Git 策略

- **分支名:** `feat/core-error-kind`
- **base:** `origin/main`(= `0c67f56`),**不是** tag `v2.0.0-rc.1`。
  理由:两者只差一个纯 `README.md` 的 doc commit,`origin/main..v2.0.0-rc.1` 为空(tag 是 main 的严格祖先),因此以 main 为基**零冲突风险、零代码差异**;而以 tag 为基会让未来的上游 PR 一开始就落后 main、合并前还要 rebase 一次。
- **工作流:** 只在 submodule 内做本地 commit。

```powershell
git -C G:\Programs\Rust\clash-nyanpasu\backend\nyanpasu-runtime fetch origin
git -C G:\Programs\Rust\clash-nyanpasu\backend\nyanpasu-runtime switch -c feat/core-error-kind origin/main
```

- **commit 规范:** Conventional Commits,scope 用 crate 名。建议 5 个 commit(Task 1–5 各一),而不是一个大 commit —— 每个 Task 结束时树都是编译绿 + 测试绿的。
- **禁止:** `git push`、`gh pr create`、`git tag`、改任何 `version =`。

### 交接边界(需用户显式授权才能越过)

1. **推送 `feat/core-error-kind` 到 `libnyanpasu/nyanpasu-runtime` 并开上游 PR** —— 计划执行到本地 commit 为止,推送必须停下来问。
2. **app 侧 submodule pin(gitlink)何时移动** —— 上游合并前,把 app 的 gitlink 指向一个**未推送**的本地 commit 会让 CI 的 `git submodule update` 直接失败。可选的过渡消费方式(继续用 `v2.0.0-rc.1` pin 而 R0 只在本地工作树生效 / 先推 branch 再 pin branch commit / 等合并后 pin main)**是 leader 的决策,本计划不预设**。
3. **release / tag** —— R0 不发版。sidecar 仍下载已发布的 `v2.0.0-rc.1`,因为 wire 没动,该二进制与打了 R0 的 app 完全兼容。

---

## Task 0: 分支与基线

**Files:** 无代码变更。以下所有命令均在 `G:\Programs\Rust\clash-nyanpasu\backend\nyanpasu-runtime` 下执行(该目录是独立 cargo workspace)。

- [ ] **Step 1: 建分支**

```powershell
cd G:\Programs\Rust\clash-nyanpasu\backend\nyanpasu-runtime
git fetch origin
git switch -c feat/core-error-kind origin/main
git log --oneline -2
```

Expected: HEAD = `0c67f56 chore: update docs`,其父为 `0d2993a chore: bump version to v2.0.0-rc.1`。

- [ ] **Step 2: 基线断言(改动前先确认现状为绿)**

```powershell
cargo test -p nyanpasu-ipc --all-features
cargo test -p nyanpasu-service-runtime --lib
```

Expected: 全绿。`nyanpasu-ipc --all-features` 覆盖 `wire_golden`(默认 features 即可)与 `roundtrip`(`required-features = ["client","server"]`,见 `nyanpasu_ipc/Cargo.toml:7-10`)。

- [ ] **Step 3: 记录版本基线(收尾时要比对未变)**

```powershell
Select-String -Path nyanpasu_service/Cargo.toml,crates/nyanpasu-service-runtime/Cargo.toml,nyanpasu_ipc/Cargo.toml,crates/nyanpasu-core-manager/Cargo.toml,crates/nyanpasu-core-metadata/Cargo.toml -Pattern '^version' | Select-Object Path,Line
```

Expected: `2.0.0-rc.1` / `2.0.0-rc.1` / `2.0.0-rc.1` / `1.0.0-rc.1` / `1.0.0-rc.1`。**Task 5 结束时必须完全一致。**

- [ ] **Step 4: 确认嵌套 submodule 无需改动**

```powershell
git submodule status
```

Expected: `3cb3af02222ced3972d95ade599949098b159202 crates/nyanpasu-utils`。
判定:`nyanpasu-utils` 只通过 `Error::Process(ProcessError)`(变体 22)与 `From<AtomicFsError>`(→ 变体 12/13/25)进入 `ManagerError`,这三条在 R0 后全部映射 `None` 且分类集合不变 —— **`crates/nyanpasu-utils` 零改动、不 bump**。本 Step 只是确认,不执行任何命令式变更。

---

## Task 1: `nyanpasu-core-metadata` 新增 `CoreErrorKind`

**Files:**

- Create: `crates/nyanpasu-core-metadata/src/error_kind.rs`
- Modify: `crates/nyanpasu-core-metadata/src/lib.rs`(12 行,加 `mod` + `pub use`)

- [ ] **Step 1: 新建 `crates/nyanpasu-core-metadata/src/error_kind.rs`**

模块头注释请沿用 `log.rs:1-6` 的论证口吻(说明「为什么住在这里而不是 core-manager」)。类型定义:

```rust
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreErrorKind {
    /// The operation needs a running core and there is none.
    NotStarted,
    /// The operation needs a stopped core and one is running.
    AlreadyRunning,
    /// `expected_revision` did not match the running revision. Nothing was
    /// applied; re-read `/status` for the current one and retry.
    RevisionConflict,
    /// An epoch whose death could not be confirmed has latched the manager.
    /// Every lifecycle operation is refused until `POST /core/recover` clears it.
    Quarantined,
    /// The core itself rejected the config in a dry run.
    ConfigCheckFailed,
    ConfigNotFound,
    BinaryNotFound,
    /// The config could not be parsed or canonicalized.
    InvalidConfig,
    /// The config declares no external controller, so the core cannot be
    /// health-probed.
    ControllerMissing,
    /// The apply failed and the previous revision was restored.
    ApplyFailed,
    /// The apply failed and so did the rollback: no epoch is running.
    ApplyRollbackFailed,
    /// A core process could not be proven dead; the manager is now quarantined.
    StopUnconfirmed,
}
```

> 12 条 doc comment 直接从 `nyanpasu_ipc/src/api/mod.rs:39-65` 原样搬运(那是被删掉的常量的文档,不能丢)。变体顺序也照搬,便于 diff 对读。

方法:

```rust
impl CoreErrorKind {
    /// Every kind, in wire-declaration order. The single place a new kind has to
    /// be listed besides the enum itself; `from_wire` and the golden test both
    /// walk it.
    pub const ALL: &'static [Self] = &[
        Self::NotStarted,
        Self::AlreadyRunning,
        Self::RevisionConflict,
        Self::Quarantined,
        Self::ConfigCheckFailed,
        Self::ConfigNotFound,
        Self::BinaryNotFound,
        Self::InvalidConfig,
        Self::ControllerMissing,
        Self::ApplyFailed,
        Self::ApplyRollbackFailed,
        Self::StopUnconfirmed,
    ];

    /// The wire string. `serde` derives the same spelling from
    /// `rename_all = "snake_case"`; the two are pinned equal by this module's
    /// tests, and this one exists because an envelope needs a `&'static str`.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::AlreadyRunning => "already_running",
            Self::RevisionConflict => "revision_conflict",
            Self::Quarantined => "quarantined",
            Self::ConfigCheckFailed => "config_check_failed",
            Self::ConfigNotFound => "config_not_found",
            Self::BinaryNotFound => "binary_not_found",
            Self::InvalidConfig => "invalid_config",
            Self::ControllerMissing => "controller_missing",
            Self::ApplyFailed => "apply_failed",
            Self::ApplyRollbackFailed => "apply_rollback_failed",
            Self::StopUnconfirmed => "stop_unconfirmed",
        }
    }

    /// The kind a wire string names, or `None` when this build does not know it.
    ///
    /// `None` is not an error: a newer service may classify a failure this build
    /// has no variant for, and the raw string stays available to the caller.
    pub fn from_wire(value: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_str() == value)
    }
}

impl std::fmt::Display for CoreErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
```

- [ ] **Step 2: 模块内测试(同文件 `#[cfg(test)] mod tests`)**

三条:

1. `every_kind_serializes_to_its_wire_string` —— 遍历 `ALL`,断言 `serde_json::to_string(kind).unwrap() == format!("\"{}\"", kind.as_str())`,并**逐条硬编码**断言 12 个字符串字面量(不要只写循环 —— 循环只证明两种表示一致,硬编码才钉住字符串本身)。
2. `every_wire_string_parses_back` —— 遍历 `ALL`,`from_wire(kind.as_str()) == Some(*kind)`;并 `serde_json::from_str::<CoreErrorKind>(&format!("\"{}\"", kind.as_str())).unwrap() == *kind`。
3. `an_unknown_wire_string_has_no_kind` —— `from_wire("a_future_kind").is_none()`。

- [ ] **Step 3: 注册模块 —— `crates/nyanpasu-core-metadata/src/lib.rs`**

```rust
mod dist;
mod error_kind;   // 新增(按字母序插在 dist 之后)
mod feature;
mod kind;
mod log;

pub use dist::{CoreDistribution, VariantTag};
pub use error_kind::CoreErrorKind;   // 新增
pub use feature::{...};
```

- [ ] **Step 4: 验证**

```powershell
cargo test -p nyanpasu-core-metadata
cargo clippy -p nyanpasu-core-metadata --all-targets
```

Expected: 新增 3 条测试全绿;无 clippy 警告。`serde_json` 已在 `[dev-dependencies]`(`crates/nyanpasu-core-metadata/Cargo.toml:15-16`),**不需要加依赖**。

Commit: `feat(core-metadata): add CoreErrorKind, the single error-kind wire table`

---

## Task 2: `nyanpasu-core-manager::Error::kind()`

**Files:**

- Modify: `crates/nyanpasu-core-manager/src/error.rs`
- Modify: `crates/nyanpasu-core-manager/src/lib.rs:21`

- [ ] **Step 1: `error.rs` 顶部重导出**

在 `use camino::Utf8PathBuf;` 之后加(与 `log.rs:12`、`kind.rs:10` 同款):

```rust
pub use nyanpasu_core_metadata::CoreErrorKind;
```

- [ ] **Step 2: 在 `pub enum Error {...}`(`:7-72`)之后、`impl From<AtomicFsError>`(`:74`)之前插入**

```rust
impl Error {
    /// The machine-readable classification a caller can branch on, or `None`
    /// when this failure has none.
    ///
    /// `None` means "not classified", never "no error": naming a kind is a
    /// statement of fact about the failure, and a guessed one is worse than an
    /// absent one. The wire omits the field entirely for `None`.
    ///
    /// The match is exhaustive on purpose. `Error` is `#[non_exhaustive]` only
    /// to *downstream* crates, so this — the defining crate — is the one place
    /// a new variant can be made to fail the build instead of silently
    /// answering `None`. Do not collapse the unclassified arms into a wildcard.
    pub fn kind(&self) -> Option<CoreErrorKind> {
        match self {
            Self::AlreadyRunning => Some(CoreErrorKind::AlreadyRunning),
            Self::NotStarted => Some(CoreErrorKind::NotStarted),
            Self::ConfigNotFound(_) => Some(CoreErrorKind::ConfigNotFound),
            Self::BinaryNotFound(_) => Some(CoreErrorKind::BinaryNotFound),
            Self::ControllerMissing => Some(CoreErrorKind::ControllerMissing),
            Self::ConfigCheckFailed(_) => Some(CoreErrorKind::ConfigCheckFailed),
            Self::InvalidConfig(_) | Self::Yaml(_) => Some(CoreErrorKind::InvalidConfig),
            Self::StopUnconfirmed(_) => Some(CoreErrorKind::StopUnconfirmed),
            Self::ManagerQuarantined { .. } => Some(CoreErrorKind::Quarantined),
            Self::RevisionConflict { .. } => Some(CoreErrorKind::RevisionConflict),
            Self::ApplyFailed(_) => Some(CoreErrorKind::ApplyFailed),
            Self::ApplyRollbackFailed { .. } => Some(CoreErrorKind::ApplyRollbackFailed),
            // The durability wrapper is a warning around a real failure; report
            // the failure's kind so a caller can still branch on it.
            Self::DurabilityUncertain { source, .. } => source.kind(),
            // Unclassified, listed one by one so a new variant is a compile
            // error here. Mapping these is the protocol report's P3.
            Self::CoreVersionProbeFailed { .. }
            | Self::RequiredLocalIpcUnsupported { .. }
            | Self::InvalidManagerOptions(_)
            | Self::InvalidHealthPolicy(_)
            | Self::UnsafeRuntimeArtifact(_)
            | Self::RuntimeDirectoryOwned(_)
            | Self::StartupTimeout { .. }
            | Self::StartupFailed { .. }
            | Self::Process(_)
            | Self::Api(_)
            | Self::Io(_) => None,
        }
    }
}
```

> 对照 §「全量清单」表四逐行核对 25 个变体。若编译器报 `non-exhaustive patterns`,说明 `error.rs` 在计划期之后又加了变体 —— **补上显式臂并在报告里说明**,不要加 `_ =>`。

- [ ] **Step 3: `error.rs` 底部加 `#[cfg(test)] mod tests`**

四条:

1. `manager_errors_carry_their_wire_kind` —— 移植 `manager_bridge.rs:1238-1284` 的 8 个用例(断言值从 `Some("not_started")` 改成 `Some(CoreErrorKind::NotStarted)` 等),含 `StartupFailed → None`。
2. `the_durability_wrapper_reports_its_source_kind` —— `DurabilityUncertain{ source: ApplyFailed } → Some(ApplyFailed)`。
3. `a_nested_durability_wrapper_still_reaches_the_source` —— **新增**,双层 `DurabilityUncertain{ DurabilityUncertain{ RevisionConflict } } → Some(RevisionConflict)`,把「递归」而不是「剥一层」钉住。
4. `an_unclassified_failure_has_no_kind` —— 取 `Io(std::io::Error::other("boom"))` 与 `StartupTimeout{..}` 各断言 `None`。

- [ ] **Step 4: `lib.rs:21` 重导出**

```rust
pub use error::{CoreErrorKind, Error};
```

(替换原 `pub use error::Error;`。这样 `nyanpasu-service-runtime` 不必新增 core-metadata 依赖 —— 它今天只依赖 core-manager + ipc + utils。)

- [ ] **Step 5: 验证**

```powershell
cargo test -p nyanpasu-core-manager --lib
cargo clippy -p nyanpasu-core-manager --all-targets
```

Expected: 4 条新测试绿。**用 `--lib`**:`cargo test -p nyanpasu-core-manager` 不带 target 过滤会连带构建 `nyanpasu-fake-core`/`nyanpasu-manager-host` 两个测试辅助 bin 并跑全部集成测试(耗时数分钟,且部分需要真实核心二进制),本 Step 不需要。

Commit: `feat(core-manager): classify errors with Error::kind()`

---

## Task 3: service 侧改用 `Error::kind()`,删除 `map_error_kind`

**Files:**

- Modify: `crates/nyanpasu-service-runtime/src/server/manager_bridge.rs`

本 Task 结束时 `nyanpasu_ipc::api::error_kind` 常量模块**仍然存在**(Task 4 才删),`RBuilder::other_error_with_kind` 签名**仍是** `Option<Cow<str>>` —— 因此 `into_envelope` 需要临时用 `as_str()` 桥接一次。这是为了让每个 Task 结束时树都能编译;Task 4 会把那一行简化掉。

- [ ] **Step 1: import(`:8-23`)**

`nyanpasu_core_manager::{...}` 那一组里加 `CoreErrorKind`(按字母序,`ConfigRevision` 之后、`CoreKind` 之前);`nyanpasu_ipc::api::{...}` 那一组里的 `error_kind,`(`:17`)**保留到 Task 4**。

- [ ] **Step 2: `OpError`(`:53-84`)**

```rust
pub(crate) struct OpError {
    kind: Option<CoreErrorKind>,   // was Option<&'static str>
    message: String,
}
```

- `with_kind(kind: CoreErrorKind, message: impl Into<String>)`(`:70`)
- `into_envelope`(`:82`)临时改为:

  ```rust
  RBuilder::other_error_with_kind(
      Cow::Owned(self.message),
      self.kind.map(|kind| Cow::Borrowed(kind.as_str())),
  )
  ```

- [ ] **Step 3: `From<ManagerError> for OpError`(`:86-99`)**

`kind: map_error_kind(&error)` → `kind: error.kind()`。`message` 分支(`NotStarted` → `MSG_CORE_NOT_STARTED`)**一字不改**。

- [ ] **Step 4: 删除 `map_error_kind`(`:641-665` 整个函数含 doc comment)**

- [ ] **Step 5: 两处逐点分类改用 enum**

- `:404` → `.map_err(|error| OpError::with_kind(CoreErrorKind::BinaryNotFound, error.to_string()))`
- `:791` → `OpError::with_kind(CoreErrorKind::ConfigNotFound, message)`

这两处输入是 `io::Error` 而不是 `ManagerError`,**不是表**,保留原样语义。

- [ ] **Step 6: 测试(`:1237-1293`)**

- 删除 `manager_errors_map_onto_the_wire_error_kinds`(`:1237-1284`) —— 表已经不在这个 crate 了,它的 8 个用例已在 Task 2 Step 3 落到 `error.rs`。
- 保留并改写 `the_not_started_failure_keeps_the_legacy_wire_string`(`:1288-1293`):`assert_eq!(error.kind, Some(CoreErrorKind::NotStarted));`。这条测的是**桥的行为**(legacy message + kind 透传),该留在这里。
- `:1519` `assert_eq!(resolved.kind, Some(error_kind::CONFIG_NOT_FOUND));` → `Some(CoreErrorKind::ConfigNotFound)`
- `:1542` `assert_eq!(error.kind, Some(error_kind::BINARY_NOT_FOUND));` → `Some(CoreErrorKind::BinaryNotFound)`

- [ ] **Step 7: 验证**

```powershell
cargo test -p nyanpasu-service-runtime --lib
cargo clippy -p nyanpasu-service-runtime --all-targets
```

Expected: 全绿,**且 `routing/tests.rs:388,414,448,475` 四条端到端 `error_kind` 断言未经修改地通过** —— 这是 wire 未动的直接证据。

Commit: `refactor(service): read the error kind from the manager instead of remapping it`

---

## Task 4: ipc 侧删除常量表、收敛构造器签名

**Files:**

- Modify: `nyanpasu_ipc/src/api/mod.rs`
- Modify: `crates/nyanpasu-service-runtime/src/server/manager_bridge.rs`(2 行:import + `into_envelope`)
- Modify: `nyanpasu_ipc/tests/wire_golden.rs`
- Modify: `nyanpasu_ipc/tests/roundtrip.rs`

删常量模块会同时打断 3 个文件的 import,所以这 4 个文件必须在**同一个 Task 内**改完;Task 中途树不可编译是预期的。

- [ ] **Step 1: `api/mod.rs` —— 删模块,加重导出**

- 删除 `pub mod error_kind { ... }`(`:38-66`)整块。
- 在 `use serde::{...};`(`:8`)之后加 `pub use nyanpasu_core_metadata::CoreErrorKind;`,并把原模块文档(`:28-37`,「These strings are protocol / ResponseCode deliberately stays a two-variant enum / Absent means not classified」那三段)改写后挂到这个重导出上 —— 它解释的是**协议决策**,不能随模块一起删掉。写法参考 `api/ws/events.rs:8-11` 的重导出注释。

- [ ] **Step 2: `api/mod.rs` —— `other_error_with_kind`(`:134-147`)签名收敛**

```rust
/// An error envelope carrying a [`CoreErrorKind`] classification.
///
/// `kind` is `Option` because a failure the service cannot classify must still
/// be reportable — guessing a kind is worse than omitting it. Taking the enum
/// rather than a string is what keeps a hand-typed kind off the wire.
pub fn other_error_with_kind(msg: Cow<'a, str>, kind: Option<CoreErrorKind>) -> R<'a, T> {
    let code = ResponseCode::OtherError;
    R {
        code,
        msg,
        data: None,
        ts: crate::utils::get_current_ts(),
        error_kind: kind.map(|kind| Cow::Borrowed(kind.as_str())),
    }
}
```

**`R.error_kind` 字段本身(`:80-85`)不动** —— 类型仍是 `Option<Cow<'a, str>>`,`#[serde(default, skip_serializing_if = "Option::is_none")]` 仍在。见 D4。

- [ ] **Step 3: 修复 3 处失效的 intra-doc link**

rustdoc 的 `broken_intra_doc_links` 是默认 warn,`--all-targets` clippy 不一定报,但 doc 会脏:

- `api/mod.rs:80` `see [`error_kind`]` → `see [`CoreErrorKind`]`
- `api/mod.rs:134` `a [`error_kind`] classification` → `a [`CoreErrorKind`] classification`(已在 Step 2 覆盖)
- `client/mod.rs:52` `See [`crate::api::error_kind`].` → `See [`crate::api::CoreErrorKind`].`

（`api/core/apply.rs:27`、`check.rs:22`、`recover.rs:7` 里的 `error_kind = "revision_conflict"` 是**普通文字**不是链接,不要动。）

- [ ] **Step 4: `manager_bridge.rs` 两行收尾**

- `:17` 从 `nyanpasu_ipc::api::{...}` 的 import 列表里删掉 `error_kind,`。
- `into_envelope`(`:82`)简化为 `RBuilder::other_error_with_kind(Cow::Owned(self.message), self.kind)`。

- [ ] **Step 5: `wire_golden.rs`**

- import(`:22`)`error_kind,` → `CoreErrorKind,`。
- helper `error_envelope_with_kind`(`:56-64`)入参 `kind: &'static str` → `kind: CoreErrorKind`,body 不变(`Some(kind)` 直接传)。
- `the_error_kind_strings_are_pinned`(`:703-718`)改写为对 **serde 输出**的断言,并把注释升级:

```rust
#[test]
fn the_error_kind_strings_are_pinned() {
    // These are protocol: a caller branches on them. The enum lives in
    // nyanpasu-core-metadata now, so this pins what actually reaches the wire.
    for (kind, expected) in [
        (CoreErrorKind::NotStarted, r#""not_started""#),
        (CoreErrorKind::AlreadyRunning, r#""already_running""#),
        (CoreErrorKind::RevisionConflict, r#""revision_conflict""#),
        (CoreErrorKind::Quarantined, r#""quarantined""#),
        (CoreErrorKind::ConfigCheckFailed, r#""config_check_failed""#),
        (CoreErrorKind::ConfigNotFound, r#""config_not_found""#),
        (CoreErrorKind::BinaryNotFound, r#""binary_not_found""#),
        (CoreErrorKind::InvalidConfig, r#""invalid_config""#),
        (CoreErrorKind::ControllerMissing, r#""controller_missing""#),
        (CoreErrorKind::ApplyFailed, r#""apply_failed""#),
        (CoreErrorKind::ApplyRollbackFailed, r#""apply_rollback_failed""#),
        (CoreErrorKind::StopUnconfirmed, r#""stop_unconfirmed""#),
    ] {
        assert_eq!(serde_json::to_string(&kind).unwrap(), expected);
    }
    // Every kind is covered above; a new one must be added here too.
    assert_eq!(CoreErrorKind::ALL.len(), 12);
}
```

（形式与同文件 `the_apply_outcome_kinds_are_pinned`(`:687-701`)一致。）

- `an_error_envelope_carries_its_kind`(`:722-733`)的调用改成 `error_envelope_with_kind("config revision conflict", CoreErrorKind::RevisionConflict)`;**被断言的 JSON 字面量一个字符都不许改**。
- `a_pre_s8_envelope_still_decodes`(`:737-743`)**完全不动**。

- [ ] **Step 6: `roundtrip.rs`**

- import(`:51`)`error_kind,` → `CoreErrorKind,`(注意这一组在 `nyanpasu_ipc::api::{...}` 里,按字母序位置会变 —— `CoreErrorKind` 排在 `RBuilder` 之前)。
- `apply_config_conflict_handler`(`:399-407`)→ `Some(CoreErrorKind::RevisionConflict)`。
- `a_server_error_kind_reaches_the_client`(`:753-780`)的 `assert_eq!(error_kind.as_deref(), Some("revision_conflict"))` **保持不变**(字段仍是 `Option<String>`),Task 5 会在其后追加 typed 断言。

- [ ] **Step 7: 验证**

```powershell
cargo test -p nyanpasu-ipc --all-features
cargo test -p nyanpasu-service-runtime --lib
cargo doc -p nyanpasu-ipc --no-deps
```

Expected: 测试全绿;`cargo doc` 无 `unresolved link` 警告。

Commit: `refactor(ipc)!: replace the error_kind string constants with CoreErrorKind`

---

## Task 5: client 侧 typed 访问器

**Files:**

- Modify: `nyanpasu_ipc/src/client/mod.rs`
- Modify: `nyanpasu_ipc/tests/roundtrip.rs`

- [ ] **Step 1: `client/mod.rs` —— 在 `enum ClientError`(`:23-63`)之后加**

```rust
impl ClientError {
    /// The typed classification of a server-side failure, when there is one
    /// this build knows.
    ///
    /// `None` covers three different things and deliberately does not
    /// distinguish them: a transport failure with no envelope, an envelope the
    /// service did not classify, and a kind a newer service named that this
    /// build has no variant for. The raw string is still on
    /// [`Self::Server::error_kind`] for the last case.
    pub fn core_error_kind(&self) -> Option<CoreErrorKind> {
        match self {
            Self::Server { error_kind, .. } => {
                error_kind.as_deref().and_then(CoreErrorKind::from_wire)
            }
            _ => None,
        }
    }
}
```

import 加 `crate::api::CoreErrorKind`(`:5-11` 那一组)。

`ClientError::Server` 的 `error_kind: Option<String>` 字段**不动** —— 见 D4。

- [ ] **Step 2: `client/mod.rs` —— 同文件加 `#[cfg(test)] mod tests`**

该文件今天没有测试模块,新建一个,两条(纯值构造,不起服务):

1. `a_known_kind_is_typed` —— 构造 `ClientError::Server { operation: "/core/apply", code: ResponseCode::OtherError, msg: "boom".into(), error_kind: Some("revision_conflict".into()) }`,断言 `core_error_kind() == Some(CoreErrorKind::RevisionConflict)`。
2. `an_unknown_kind_keeps_its_raw_string` —— 同样构造但 `error_kind: Some("a_future_kind".into())`,断言 `core_error_kind().is_none()` **且**原字段仍是 `Some("a_future_kind")`。这条是 D4 前向兼容契约的回归闸。

- [ ] **Step 3: `roundtrip.rs` —— `a_server_error_kind_reaches_the_client`(`:756-780`)追加 typed 断言**

现有 `match` 臂里已经解构出 `error_kind`,但访问器挂在 `ClientError` 上,所以改成先绑定整个错误:

```rust
    let error = client.apply_config(&apply_payload()).await.unwrap_err();
    assert_eq!(error.core_error_kind(), Some(CoreErrorKind::RevisionConflict));
    match error {
        ClientError::Server { code, msg, error_kind, .. } => {
            assert_eq!(code, ResponseCode::OtherError);
            assert_eq!(msg, "config revision conflict");
            assert_eq!(error_kind.as_deref(), Some("revision_conflict"));
        }
        other => panic!("expected a classified server error, got: {other:?}"),
    }
```

（保留原 `msg` / raw `error_kind` 断言 —— 它们钉的是 wire。）

- [ ] **Step 4: 验证**

```powershell
cargo test -p nyanpasu-ipc --all-features
```

Expected: 全绿。注意 `roundtrip.rs` 的 `run_server` 在无法绑定 socket 时返回 `None` 并静默跳过(Unix 下 `/var/run` 需要可写);Windows 走 named pipe,应当真实执行。

Commit: `feat(ipc): expose the typed error kind on the client error`

---

## Task 6: 全量验证与收尾

**Files:** 无代码变更。

- [ ] **Step 1: 对齐 CI 的全量检查**

```powershell
cd G:\Programs\Rust\clash-nyanpasu\backend\nyanpasu-runtime
cargo clippy --all-targets --all-features
cargo fmt --all -- --check
cargo test -p nyanpasu-core-metadata
cargo test -p nyanpasu-core-manager --lib
cargo test -p nyanpasu-ipc --all-features
cargo test -p nyanpasu-service-runtime --lib
```

Expected: 全绿。`cargo clippy --all-targets --all-features` 是 `ci.yml:52` 的原样命令。若出现 `query stack during panic` ICE —— **重跑**,不改 toolchain。

- [ ] **Step 2: 「不再有第二份表」的机械判据**

```powershell
rg 'map_error_kind|pub mod error_kind|error_kind::[A-Z_]+' --type rust .
```

Expected: **0 命中**。(`error_kind` 作为 envelope **字段名**的命中不算 —— 那是 wire,必须还在;上面的 pattern 已经排除了它。)

```powershell
rg '"not_started"|"already_running"|"revision_conflict"|"quarantined"|"config_check_failed"|"config_not_found"|"binary_not_found"|"invalid_config"|"controller_missing"|"apply_failed"|"apply_rollback_failed"|"stop_unconfirmed"' --type rust .
```

Expected: 命中只应出现在 —— `crates/nyanpasu-core-metadata/src/error_kind.rs`(`as_str()` 与其测试)、`nyanpasu_ipc/tests/wire_golden.rs`(golden)、`nyanpasu_ipc/tests/roundtrip.rs`(wire 断言)、`crates/nyanpasu-service-runtime/src/server/routing/tests.rs`(端到端 wire 断言)。**任何 `src/` 下的其它命中都是残留的第二份表。**

- [ ] **Step 3: 版本未动**

```powershell
git diff origin/main --stat -- '*/Cargo.toml' 'Cargo.toml' 'Cargo.lock'
```

Expected: **空**。R0 不加依赖、不改 manifest、不动 `Cargo.lock`。若 `Cargo.lock` 出现改动,说明误加了依赖 —— 回退。

- [ ] **Step 4: 嵌套 submodule 未动**

```powershell
git diff origin/main -- crates/nyanpasu-utils
git submodule status
```

Expected: 空 diff;`3cb3af0` 未变。

- [ ] **Step 5: 变更面复核**

```powershell
git diff origin/main --stat
```

Expected: 恰好 9 个文件 ——
`crates/nyanpasu-core-metadata/src/error_kind.rs`(新)、
`crates/nyanpasu-core-metadata/src/lib.rs`、
`crates/nyanpasu-core-manager/src/error.rs`、
`crates/nyanpasu-core-manager/src/lib.rs`、
`crates/nyanpasu-service-runtime/src/server/manager_bridge.rs`、
`nyanpasu_ipc/src/api/mod.rs`、
`nyanpasu_ipc/src/client/mod.rs`、
`nyanpasu_ipc/tests/wire_golden.rs`、
`nyanpasu_ipc/tests/roundtrip.rs`。
`crates/nyanpasu-service-runtime/src/server/routing/tests.rs` **不在列** —— 它出现在 diff 里就是 wire 动了的信号。

- [ ] **Step 6: 停下来交接**

**不要 push,不要开 PR。** 向 leader 报告:分支名 + commit 列表 + 上述判据结果 + 覆盖率影响观察(见风险 R6)。

---

## 测试计划总览

| 测试                                                             | 位置                               | 性质                          | R0 后                                       |
| ---------------------------------------------------------------- | ---------------------------------- | ----------------------------- | ------------------------------------------- |
| `every_kind_serializes_to_its_wire_string`                       | core-metadata `error_kind.rs`      | 新增                          | 12 个字符串字面量 + `as_str()`↔serde 一致性 |
| `every_wire_string_parses_back`                                  | core-metadata `error_kind.rs`      | 新增                          | `from_wire` / serde 双向 round-trip         |
| `an_unknown_wire_string_has_no_kind`                             | core-metadata `error_kind.rs`      | 新增                          | 前向兼容                                    |
| `manager_errors_carry_their_wire_kind`                           | core-manager `error.rs`            | 从 service 迁入               | 8 个用例(原 `manager_bridge.rs:1238`)       |
| `the_durability_wrapper_reports_its_source_kind`                 | core-manager `error.rs`            | 新增                          | 单层递归                                    |
| `a_nested_durability_wrapper_still_reaches_the_source`           | core-manager `error.rs`            | 新增                          | 双层递归                                    |
| `an_unclassified_failure_has_no_kind`                            | core-manager `error.rs`            | 新增                          | `None` 语义                                 |
| `the_not_started_failure_keeps_the_legacy_wire_string`           | `manager_bridge.rs:1288`           | 改断言类型                    | 桥的 legacy message + kind 透传             |
| `the_error_kind_strings_are_pinned`                              | `wire_golden.rs:703`               | 改写为 serde 断言             | **比原来更强**:钉的是真正上 wire 的表示     |
| `an_error_envelope_carries_its_kind`                             | `wire_golden.rs:722`               | 只改构造、**JSON 字面量不动** | wire 字节闸门                               |
| `a_pre_s8_envelope_still_decodes`                                | `wire_golden.rs:737`               | **完全不动**                  | 向后兼容闸门                                |
| `a_server_error_kind_reaches_the_client`                         | `roundtrip.rs:756`                 | 追加 typed 断言               | raw + typed 双证                            |
| `a_known_kind_is_typed` / `an_unknown_kind_keeps_its_raw_string` | `client/mod.rs`                    | 新增                          | D4 契约回归闸                               |
| routing 端到端 4 条                                              | `routing/tests.rs:388,414,448,475` | **完全不动**                  | wire 未动的最强证据                         |

---

## 风险

| #   | 风险                                                                   | 处置                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| --- | ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R1  | **未知 kind 的前向兼容** —— 更新的 service 发来老 client 不认识的 kind | 已决(D4):`R.error_kind` 与 `ClientError::Server.error_kind` 都保持原始字符串,typed 访问走 `from_wire → Option`。若把 wire 字段改成 `Option<CoreErrorKind>`,未知 kind 会让**整个 envelope 解码失败**,而不是丢一个字段 —— 与 `wire_golden` 保持不变的目标直接冲突,也正是报告 §4 P0-B 拒绝动 `ResponseCode` 的同一失败模式。`an_unknown_kind_keeps_its_raw_string` 是这条的回归闸。                                                                                                                        |
| R2  | 删 `pub mod error_kind` 是 `nyanpasu-ipc` 的**破坏性 API 变更**        | 已实测外部消费者为 0:app 侧 `rg 'error_kind\|ClientError::Server' backend/tauri/src` 零命中,ipc 未发布到 crates.io,唯一消费者是同 workspace 的 `nyanpasu-service-runtime`。按 CLAUDE.md §11「优先可迁移的破坏性变更」,不留 deprecated 别名。commit message 带 `!`。                                                                                                                                                                                                                                     |
| R3  | `Error::kind()` 写成 `_ => None` 通配                                  | 那样本 PR 就只是搬家,没拿到「新变体编译报错」的收益。Task 2 Step 2 明写禁止;review 时 `rg '_ => None' crates/nyanpasu-core-manager/src/error.rs` 应为 0。                                                                                                                                                                                                                                                                                                                                               |
| R4  | intra-doc link 断裂(3 处)                                              | Task 4 Step 3 逐条列出;`cargo doc -p nyanpasu-ipc --no-deps` 作为验证。                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| R5  | `CoreErrorKind` 是否该 `#[non_exhaustive]`                             | 已决(D5):不标,对齐 `ApplyOutcomeKind`。**属判断题,列入 leader 裁决项。** 若 leader 改判要标,则 app 侧未来 `match` 必须留通配臂,本计划其余部分不受影响。                                                                                                                                                                                                                                                                                                                                                 |
| R6  | **覆盖率闸门** `--fail-under-lines 53`(基线 55.42%)                    | `map_error_kind`(~19 行,被测试完全覆盖)从被测量集合(`-p nyanpasu-service-runtime -p nyanpasu-ipc`)中删除,而新增的 `kind()` 落在 core-manager —— 被 `--ignore-filename-regex 'crates/(clash-api\|nyanpasu-core-manager\|nyanpasu-core-metadata\|nyanpasu-utils)/'` 排除在外。**净效果是把一段高覆盖代码移出分母之外的分子**,理论上略微拉低百分比。19 行相对基线体量极小,2.4 个百分点的余量足够,但 codex 若本地装了 `cargo-llvm-cov` 应跑一次记录数字;没装则在交接报告里标注「未本地验证,依赖 CI 信号」。 |
| R7  | Task 4 中途树不可编译                                                  | 预期行为(删常量同时打断 3 个文件)。Task 4 的 5 个 Step 必须一次做完再验证,不要中途 `cargo check` 就判定失败。                                                                                                                                                                                                                                                                                                                                                                                           |
| R8  | `error.rs` 在计划期后新增了 `Error` 变体                               | 穷尽匹配会直接编译报错,补显式臂即可;**必须在交接报告里说明新变体及其 kind 判定**,不得静默塞进 `None` 组。                                                                                                                                                                                                                                                                                                                                                                                               |
| R9  | nightly ICE(`query stack during panic`)                                | 本机已知的非确定性问题,**重跑**。不得 pin toolchain 日期(用户已明确否决)。                                                                                                                                                                                                                                                                                                                                                                                                                              |

---

## 明确的 Out of scope

- ❌ 任何 crate 的 `version =` bump(尤其 `nyanpasu_service` 的 `2.0.0-rc.1` —— `scripts/check.ts:674-691` 的 sidecar 下载 lockstep 依赖它)
- ❌ 新建 crate
- ❌ `CoreEngine` trait / `CoreEngineFactory` / `EngineStatus` / `EngineError` 镜像
- ❌ 把剩余 11 个未分类 `ManagerError` 变体补上 kind(= wire 新增,属报告 P3)
- ❌ 改 `R.error_kind` 字段类型、改 `ResponseCode`、改任何已有 wire 字符串
- ❌ `git push` / 开上游 PR / 打 tag
- ❌ 动 app 仓(`backend/tauri`、`backend/Cargo.toml`、submodule gitlink)
- ❌ 动嵌套 submodule `crates/nyanpasu-utils`
- ❌ 动 `rust-toolchain.toml`

---

## 需 leader 裁决 → 裁定结果(2026-08-01)

1. **D5 `#[non_exhaustive]`** —— 计划已裁定「不标」(对齐 `ApplyOutcomeKind`,让上游加 kind 时 app 侧编译报错而非静默通配)。若倾向「标」,只需在 Task 1 Step 1 加一行属性,其余不变。
   **Leader 裁定:维持「不标」。** 解码走 `from_wire → Option`,封闭 enum 不会因未知 wire 串构造失败;service 与 GUI 同版本分发,上游新增 kind 触发 app 侧编译错误正是想要的行为。
2. **上游推送与 PR 时机** —— 计划止步于本地 commit。
   **Leader 裁定:确认交接边界。** R0 通过阶段审查后由 leader 向用户申请推送授权;授权前一切停在本地分支。
3. **上游合并前 app 如何消费 R0** —— 继续 pin `v2.0.0-rc.1` / 先推分支再 pin 分支 commit / 等合并后 pin main。三者对 PR-5-pre P1(切 path 依赖)的排期影响不同,由 leader 定。
   **Leader 裁定:PR-5-pre P1 继续 pin `v2.0.0-rc.1`,不依赖 R0;消费方式推迟到 PR-5a 启动门,与推送授权一并决。** 补充执行约束:R0 实施结束后 submodule 工作树停在 `feat/core-error-kind` 供审查;PR-5-pre 实施开始前由 leader 将其切回 `main`(= `0d2993a`),避免 path 依赖下的验证混入 R0 代码、也避免 gitlink 误提交。

附:leader 复核补充(2026-08-01) —— 计划 Task 1 使用 `specta::Type` derive 而未验证依赖,已核实 `crates/nyanpasu-core-metadata/Cargo.toml:14` 已有 `specta = { version = "^2.0.0-rc.25", features = ["derive"] }`,「不新增任何依赖」声明成立。
