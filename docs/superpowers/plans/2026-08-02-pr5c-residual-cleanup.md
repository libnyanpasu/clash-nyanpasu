# PR-5c 实施计划 — 可证死代码删除与 ledger 扫描器修复

**日期：** 2026-08-02（v5 拆分于 2026-08-03）
**版本：** v5（**用户裁定拆分**：本阶段只做删除；C2 运行模式与 C3 macOS DNS 独立为 **PR-5d**）
**分支基线：** `refactor/core-manager-actor` @ `899b069f5`（PR-5b 阶段门已关闭，467 passed / 1 ignored）
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/task.md` 卡 C1、C4 + 文末最终删除清单
**平台：** Windows 11 / PowerShell

> **拆分原则（用它判断每一项归哪边）：本阶段只包含「正确性可以靠『这东西没有活的调用者』来证明」的改动。** 凡是需要**先建一个替代机制**才能成立的，归 PR-5d。
>
> 这条缝落在两轮对抗审验证过的分界上：本计划在「删什么」上两轮**零驳回**，而「删掉后由什么顶上」两轮共 **16 条 BLOCKING 几乎全部集中在那里**。C2/C3 不是清理，是与 5b 同量级的并发设计工作。

---

## 0. 本阶段的边界

**做：**

1. **S1 — ledger 块注释扫描器修复**（独立 bug，自带验证方法，**必须第一步**——否则本阶段自己的删除账目不可核）；
2. **C1 — 删 `Logger` global**（写入者不可达、命令无消费者，按 D1=A 只删不建）；
3. **C4 — 删真正失去调用者的 residual**：`core/manager.rs`、`core/state.rs`、`core/clash/core.rs` 的**死面**；
4. roadmap / ledger 更新。

**不做（**移交 PR-5d**，非放弃）：**

| 项                                                                                                   | 去向          |
| ---------------------------------------------------------------------------------------------------- | ------------- |
| **C2 全部**：探针、九处调用点、Service→Normal、轮询线程与三个 statics                                | **PR-5d**     |
| **C3 全部**：`MacosDnsPort`、`SetTunDns`、guard、双适配器、读回校验、pre-control 拆除                | **PR-5d**     |
| **D2**（删 `impl Default for RunType`）——它的目的是解开 statics 删除的前置阻塞，statics 走了它跟着走 | **PR-5d**     |
| **D3 / D4**、附录 B 整体、smoke 2 与 smoke 3                                                         | **PR-5d**     |
| `UpdaterManager::global()` 本体（F26：core 耦合已收敛）                                              | PR-6d         |
| `ProxiesGuard` / `Handle` / `Sysopt` / `WindowManager` / `Hotkey`                                    | 各自 owner PR |
| `core/clash/ws.rs` 四条流与前端消费面（F5：**活的**日志通路）                                        | 不动          |
| `backend/tauri/src/logging/` 死模块（F8）                                                            | PR-7          |

> **两处纠缠，别踩：**
>
> 1. **`core/clash/core.rs` 不能整文件删**——`RunType`、`find_binary_path`、`change_default_network_dns` 都是活的，最后一个要等 C3。本阶段**只删该文件的死面，文件保留**；
> 2. **`CoreManager::global()` 本阶段不归零**——它唯一的活调用者是 `feat.rs:419` 的 `change_default_network_dns`，而那是 C3 的迁移目标。**出口判据不得写 `rg 'CoreManager'` 为 0**（原 v4 那条是错的，见 §5）。

---

## 1. 已核验事实

> **事实编号保持原样，F9–F24 / F33–F36 的空缺是有意的**——那些是 C2/C3 的事实，**已随附录 B 一同移交 PR-5d**，编号不重排以便两份计划交叉引用。

### 1.1 C1 —— 状态与日志

| ID  | 事实                                                                                                                                                                                                                      | 锚点                                                        |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| F1  | 「status read 不走 mailbox RPC」在 5a/5b **已经满足**：`CoreClient::status()` 是 `status_rx.borrow().clone()`，零 mailbox。C1 该项**已达成**，本阶段只核对不回退                                                          | `client/core.rs:146-148`、`:150-152`                        |
| F2  | `RefreshStatus` 守卫消息的**生产调用点为零**（16 处全在测试内）；`RefreshHint` 唯一生产调用点是 `NyanpasuClient::core_status`                                                                                             | `client/core.rs:165-174`；`client/mod.rs:483`               |
| F3  | **`Logger` global 的三个写入者全部不可达**：它们都在 `Instance::start` 内，而 `Instance::try_new` **零调用点**、`CoreManager.instance` 初始化为 `None` 后**从未被赋值**。因此 `get_clash_logs` 今天**恒返回空**           | `core/clash/core.rs:186,191,200`、`:94`、`:381`             |
| F4  | `Logger` 本身**已经是 ring**，但**稳定在 101 条不是 100**：`if logs.len() > LOGS_QUEUE_LEN { pop_front() }` 在 `push_back` **之前**——len 为 100 时不 pop、推入即 101。`clear_log` 零调用点。（**不影响 D1**：队列恒为空） | `core/logger.rs:5,26-29`、`:32`                             |
| F5  | **活的日志通路是 clash WS**：四条流（logs/traffic/memory/connections），`ClashWsHistory.logs` 上限 **1024**，经 `ClashWsEvent` 发到前端并被 `use-clash-logs.ts` 消费                                                      | `core/clash/ws.rs:24,215,246,199-210`；`clash/mod.rs:46-52` |
| F6  | **`get_clash_logs` 没有任何前端消费者**：源码侧命中共 2 处，**全在 `bindings.ts` 的 binding 定义自身**；另 2 处在 `frontend/interface/dist/`，那是**构建产物**。**复审时请勿把 `dist/` 命中当成使用者**                   | `ipc.rs:522-526`；`bindings.ts:31-32`                       |
| F7  | **`LogFrame` 类型在 tauri crate 内不存在**；service 侧 `/ws/events` 在 IPC crate 里有，但 **tauri 侧零消费**                                                                                                              | `nyanpasu_ipc/src/client/shortcuts.rs:73,82,110`            |
| F8  | `backend/tauri/src/logging/` 整个模块**由构造即死**：`#![allow(dead_code)]` + `setup.rs:68` 的 `setup()` 调用被注释掉                                                                                                     | `logging/mod.rs:1`；`setup.rs:68`                           |

### 1.2 C4 —— residual 与 ledger

| ID   | 事实                                                                                                                                                                                                                                                   | 锚点                                                              |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------- |
| F25  | **`attach_core_port` 不存在**——全仓零命中。C4 该项是 **no-op**                                                                                                                                                                                         | 全仓 grep                                                         |
| F26  | Updater 的 core 耦合**已经是收敛形态**：`CoreClient` 按调用传入，manager 本身不持有；唯一残留是 `client/mod.rs:562` 那行 `UpdaterManager::global()`                                                                                                    | `core/updater/mod.rs:223`；`client/mod.rs:558-567`                |
| F27  | **两个文件已完全失去调用者**：`core/manager.rs`（84 行，`grant_permission` / `escape` 均零调用）与 `core/state.rs`（233 行，`#![allow(dead_code)]`）                                                                                                   | 见左 + `core/mod.rs:8,20`                                         |
| F28  | `core/clash/core.rs`（**480 行**）**约 75% 已死**：`enum Instance` 及其整个 impl、`CoreManager.instance` 字段、`CoreManager::status`。**活的只有** `RunType`、`find_binary_path`、`change_default_network_dns`（后两者本阶段不动）                     | `core/clash/core.rs:80-368`、`:387-402`                           |
| F29  | 删掉 `manager.rs` 与 `Instance` 后，`find_binary_path` 只剩**一个**活调用者 `utils/dirs.rs:345`；`setup.rs:90-103` 已有可注入的替代 `OsCoreBinaryResolver`                                                                                             | 见左                                                              |
| F30  | ledger 现值：`config_calls` 102、`service_globals` 58、`migration_markers` 15、`legacy_dto_refs` 299、`test_real_dirs` 0；gate 当前为绿                                                                                                                | `scripts/architecture-ledger.snapshot.json`                       |
| F31  | **ledger 有 bug：`core/clash/core.rs` 第 52–480 行（该文件共 480 行）全部对 ledger 不可见。** 块注释追踪器在 `:51` 的 doc 注释里看到字面量 `/core/*` 就置 `inBlockComment = true`，而该文件**全文没有 `*/`**（实测 0 处）                              | `scripts/architecture-ledger.ts:493-507`；`core/clash/core.rs:51` |
| F32  | **该 bug 的损害限于逐文件**：`inBlockComment` 声明在**逐文件处理函数体内**（`:485`），每文件重置、**不跨文件泄漏**。修复范围就是这一处逻辑，**不需要全仓复查**                                                                                         | `scripts/architecture-ledger.ts:485`                              |
| F32b | **同一模式命中两个文件**（双方独立筛查一致）：`core/clash/core.rs:51` 与 `utils/candy.rs:12`（glob 串 `"{}/*.{}.app.log"`，13–143 行不可见）。**但 `candy.rs` 隐藏区内 ledger 关心的四类计数全为 0**，**不参与基线修正**——它是**潜伏盲区**而非现存偏差 | `core/clash/core.rs:51`；`utils/candy.rs:12`                      |
| F37  | **`core/clash/core.rs` 的三处 `Config::` 只有一处随本阶段删除**：`:64` 在 `RunType::default()` 内（D2 → 5d，**留**）、`:96` 在 `Instance::try_new` 内（**本阶段删**）、`:415` 在 `change_default_network_dns` 内（C3 → 5d，**留**）                    | `core/clash/core.rs:64,96,415`                                    |
| F38  | **`process_core_bridge.rs:4` 的模块 doc 里含 `CoreManager::global()`**——那是一条**禁止性注释**（"Must never call …"），列举了若干测试隔离禁项                                                                                                          | `client/process_core_bridge.rs:4`                                 |
| F39  | **fixed-port 集成测试已经存在**，不需要新建：`s09_process_fixed_port_hold_conflict_and_frees_after_stop`                                                                                                                                               | `client/process_core_bridge.rs:1129`                              |

---

## 2. 决策点

### D1 —— C1 的「100 条 `LogFrame` ring」建不建 —— **裁定 A（只删不建）**

三条事实叠加后这件事的性质就变了：`Logger` **已经是** ring（F4），但**没有写入者**（F3）；`get_clash_logs` **没有消费者**（F6）；`LogFrame` 在 tauri 侧不存在（F7）。**它是一个没有数据源的 ring 喂着一个没有消费者的命令。**

**裁定 A：只删不建。** 删 `Logger` global 与三个不可达写入者；`get_clash_logs` **保留命令与 wire**（避免 bindings 变化），内部返回空并标注去向。

> **卡面项的收窄必须留痕（不允许静默丢弃）：** 卡 C1 的原意是**让核进程日志走 actor**，其前提是「这条路径是活的」——**经核实不成立**。**若将来真要做**，需重新设计**数据源**（Local 模式的 stdout/stderr 从哪采——`Instance::start` 那条已死；Service 有 `/ws/events` 但两者不对称）与**消费者**（今天前端只消费 WS）。**这不是「漏了」，是「查证后主动收窄」。**

> **D2 / D3 / D4 已随 C2/C3 移交 PR-5d**（D4 是 smoke 3 的用户裁定，**本阶段因此不再被它阻塞**）。

---

## 3. 实施步骤

### S1 — 先修 ledger 的块注释**扫描器**（**必须最先做**）

> **本步不叫「每文件重置」**：`inBlockComment` **本来就已经每文件重置**（F32），那不是缺陷所在。真正的缺陷是**扫描器不区分字符串 / 行注释 / 块注释三种上下文**。

**为什么必须最先做**：本阶段的判据是「删除后的 ledger 差异恰好是这些」。**基线本身错了，判据就失效**（F31：`Logger::global()` 真 4 报 1）。

**改法：单趟三态扫描器。**

| 当前状态 | 遇 `/*`                  | 遇 `//`                  | 遇 `*/`  |
| -------- | ------------------------ | ------------------------ | -------- |
| 普通代码 | 进块注释                 | 进行注释（本行剩余忽略） | 照原样   |
| 字符串内 | **忽略**                 | **忽略**                 | 忽略     |
| 行注释内 | **忽略**（本行剩余忽略） | 忽略                     | 忽略     |
| 块注释内 | 忽略                     | **忽略**                 | 出块注释 |

它同时解决两个触发点：`candy.rs` 的 `/*` 在**字符串**里被忽略；`core.rs:51` 的 `/core/*` 在 **`///` 行注释**里被忽略——**与反引号无关**，`///` 本身就是行注释，扫描器不必认识 markdown。

**这是标准词法扫描，不是特调启发式**——「为什么不是特调」本身就是这条修法的正当性：它天然覆盖将来任何新写法，而特调规则只挡住已知的两处。

**两个被否方案，连同理由记下（否则下一个人会重新提）：**

| 方案                            | 为什么否                                                                                                                                                                                                                                                                                                                                                                    |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 先剥字符串字面量（复用 `:202`） | **只修得了 `candy.rs`。** 那两行只处理 `"…"` 与 `'…'`，而 `core.rs:51` 的 `/core/*` 在 `///` 注释里、用 markdown 反引号包着，**不在任何 Rust 字符串内**。净效果是**修好零影响的、漏掉真正污染基线的**                                                                                                                                                                       |
| 先剥行注释、再扫块注释          | **引入静默且难以归因的定时缺陷。** `stripLineComment`（`:261-280`）跟踪引号但**不感知块注释**，块注释内一行 `参考 http://example.com  */` 会在 `//` 处截断、**把 `*/` 一起删掉**。实测该模式**今日全仓 0 命中**——所以是**潜在**而非现存缺陷，这**加强**而非削弱否决理由：发作时症状是「ledger 数字莫名变小」，而触发它的改动（加一行带 URL 的注释）看起来与 ledger 毫无关系 |

**验证（两段式，先实测再比对）：**

**第一段——立基线。** 修好后跑 `pnpm architecture-ledger`，**实测**新基线。预期修正（**精确值，逐项数过被隐藏的行**）：

| key                                  | 修正前  | 修正后   | 分解                                                                               |
| ------------------------------------ | ------- | -------- | ---------------------------------------------------------------------------------- |
| `config_calls`                       | 102     | **105**  | `Config::verge()` 76→**78**（`core.rs` 藏 2）；`Config::clash()` 26→**27**（藏 1） |
| `service_globals`                    | 58      | **61**   | `Logger::global()` 1→**4**（`core.rs` 藏 3）                                       |
| `migration_markers`                  | 15      | **15**   | 被隐藏区内 marker 为 **0**                                                         |
| `legacy_dto_refs` / `test_real_dirs` | 299 / 0 | **不变** | 被隐藏区内为 0                                                                     |

`candy.rs` 被隐藏的 13–143 行内上述四类**全为 0**，**不参与基线修正**（F32b）。

**若实测与上表不符，停下核查**——要么计数错了，要么修法漏了一处。

**第二段——立判据。** 以**实测所得**为后续比较基准，**本步单独 `--write-snapshot` 一次、单独成 commit**。

**复验命令**（发现它的方法也用来验收它，记进计划作为将来低成本复查手段）：

```bash
for f in $(find backend/tauri/src -name '*.rs'); do \
  o=$(grep -o '/\*' "$f" | wc -l); c=$(grep -o '\*/' "$f" | wc -l); \
  [ "$o" != "$c" ] && echo "$f open=$o close=$c"; done
```

> **修复后该命令仍会列出那两个文件**——**源码里的不平衡是事实，不是要改的东西**。判据是上表的基线数字，**不是这条命令输出为空**（v4 的 §5 曾与本节自相矛盾，已修正）。

### S2 — 删两个完全失去调用者的文件

删 `core/manager.rs`（84 行）与 `core/state.rs`（233 行）及 `core/mod.rs:8,20` 的 `pub mod`（F27）。

**验证：** `cargo check`；

```bash
rg 'crate::core::manager|crate::core::state\b|\bManagedState\b' backend/tauri/src
```

> **判据必须限定 `crate::`**（修 v4 的错）：裸写 `core::state` 会命中**四个活文件**里的 `nyanpasu_core::state::PersistentStateManagerSetup`（`client/{application,clash_config,profiles,session_state}.rs`）——那是**另一个 crate 的同名模块**，与本次删除无关。

### S3 — 删 `Logger` global（按 D1=A）

**删除面共四处（v5 只写了第一处，现补全）：**

| #   | 编辑                                  | 位置                                                                                     |
| --- | ------------------------------------- | ---------------------------------------------------------------------------------------- |
| 1   | 删整个文件                            | `backend/tauri/src/core/logger.rs`                                                       |
| 2   | 删 `pub mod logger;`                  | `core/mod.rs:7`                                                                          |
| 3   | 从 `use` 列表移除 `logger::Logger`    | `ipc.rs:5`                                                                               |
| 4   | 删 `use crate::core::logger::Logger;` | `core/clash/core.rs:4`（该行随 S4 删 `Instance` 时也会失去用户，但**导入本身要显式删**） |

`get_clash_logs` 保留命令与 wire，内部返回空 `VecDeque`，加**去向注记**。

**注记写成 `TODO(actor-migration)`**（已定）：该命令是「存在、恒返回空、去向待 PR-6/7 决定」——**这正是 marker 要表达的真实迁移欠账**。写成普通注释能让数字好看，但那是**用少记欠账换指标**。

**验证（两条，缺一不可）：**

```bash
rg 'Logger::global|crate::core::logger|logger::Logger' backend/tauri/src   # 调用与导入归零
test ! -f backend/tauri/src/core/logger.rs && echo "logger.rs removed"      # 模块本身不存在
```

> **为什么必须有第二条**：四处调用消失后第一条就能过，**而全局定义仍然在**。**删一个模块，门禁必须能证明「模块本身没了」，而不只是「没人调它了」。**

**bindings 零变化**（命令与签名都不动）。

### S4 — 删 `core/clash/core.rs` 的死面（**文件保留**）

删 `enum Instance` 及其整个 impl（`:80-368`）、`CoreManager.instance` 字段、`CoreManager::status`（`:387-402`）（F28）。

**保留**：`RunType` 及 `impl Default`（D2 → 5d）、`find_binary_path`（F29）、`change_default_network_dns` 与 `previous_dns`（C3 → 5d）、`CoreManager` 类型与 `CoreManager::global()`（它仍是 `change_default_network_dns` 的宿主）。

**`process_core_bridge.rs:4` 的禁止性注释（F38）**：它列举的禁项里 `CoreManager::global()` **在本阶段仍然存在**，因此**不悬空、不改**。（该注释里另一条关于 `RunType::default()` 的警告在 `:251`，同样**本阶段不动**——`RunType::default()` 随 D2 去 5d。）

**同时必须更新 `core/clash/core.rs:51` 的 doc 注释**：它现在写着「……无法构造出会发 `/core/*` 的 `Instance::Service`」，而 `Instance` 本步就被删除——**注释会指向一个不存在的类型**。

> **它与 S1 是两件独立的事，两个都要做，别用一个替代另一个。** 巧合在于**同一行**：S1 修的是**扫描器**（根治——将来别的文件写 glob 串也不会中招），本步改的是**这句注释引用了被删的类型**。S1 修好后这行仍然会含 `/core/*` 而扫描器不再中招；本步改完注释后它不再提 `Instance`。两者互不替代。

**验证（判据限定到该文件，且匹配真实声明形态）：**

```bash
rg -n 'enum Instance|pub async fn status|instance:' backend/tauri/src/core/clash/core.rs
rg -n 'Instance::' backend/tauri/src/core/clash/core.rs        # 含 doc 注释里的引用
```

两条均应为 0；`cargo check`。

> **v5 那条判据是错的，两半都错**：`\.instance` **全局搜会命中六处活代码**（`core/storage.rs:120`、`widget.rs:71,97,101,145,179`）；而 `CoreManager::status` **在该文件里 grep 计数本就是 0**——声明形态是 `pub async fn status<'a>(&self)`，**那半条判据删除前就恒真**，等于没有判据。
>
> **不得写 `rg 'CoreManager'` 为 0**——该类型本阶段整个留着（§0 纠缠 2）。

### S5 — 门禁

**顺序本身就是判据的一部分**（v5 的顺序必然红，见下）：

```powershell
# 1) 常规门禁
pnpm fmt:backend
pnpm lint:rustfmt
pnpm lint:clippy
pnpm test:backend
pnpm lint:ts

# 2) bindings —— 与**基线**比，不与工作区比
git diff --exit-code 899b069f5..HEAD -- frontend/interface/src/ipc/bindings.ts

# 3) ledger 三步，顺序不可颠倒
pnpm architecture-ledger                     # report 模式：核对精确增量（见下表）
pnpm architecture-ledger --write-snapshot    # 核对无误后才写入最终快照
pnpm lint:architecture-ledger                # 此时 gate 才有正确的比对对象
```

> **v5 把 `lint:architecture-ledger` 排在最终 `--write-snapshot` 之前，那一步必然红。** S1 写入的是**修正后基线**，S2–S4 又故意改动每一项，而 gate 模式是**与已提交快照精确比对**——**让门禁去比对一个已知过期的基线，等于把判据写成必失败**。
>
> **同理，`git diff` 形态的判据只要跑在中间提交之后，就必须与基线比而不是与工作区比。** v5 的 `git diff <path>` 只看**未提交**改动，而 S1–S4 都先提交了——**即使某个提交改了 bindings，那条命令也返回空**，它是个恒真判据。**我按这个形态排查了全部门禁，「diff 应为空」形态只有 bindings 这一处**，已修正；其余门禁（fmt / lint / test / ledger）都不是差异比对形态。

**bindings 预期：零变化。** 本阶段不改任何命令签名（D1=A 保留 `get_clash_logs` 的 wire）。

**ledger 最终预期（对着 S1 实测基线算，逐项到来源）：**

| key                 | 变化                                                                                                       | 来源  |
| ------------------- | ---------------------------------------------------------------------------------------------------------- | ----- |
| `service_globals`   | **−4**：`Logger::global()` 4→0（S3 删文件 + S4 删三个写入者）。**`CoreManager::global()` 不变**（C3 → 5d） | S3/S4 |
| `config_calls`      | **−1**：仅 `core.rs:96` 的 `Config::verge()`（在 `Instance::try_new` 内）。`:64` 与 `:415` **留**（F37）   | S4    |
| `legacy_dto_refs`   | **−3**：`core/state.rs` 的三处 `IVerge`（`:176/:180/:183`）                                                | S2    |
| `migration_markers` | **+1**：`get_clash_logs` 的去向注记。`feat.rs:416` 与 `service/ipc.rs:126` 两条**均留**（C3/C2 → 5d）      | S3    |
| `test_real_dirs`    | **0**（硬门禁）                                                                                            | ——    |

> **`migration_markers` 上升 1 是预期且正确的。** 它与 S1 让基线上升是**同一回事**：**ledger 数字变大不代表代码变差，而是账本看见了此前看不见的欠账**。`get_clash_logs` 的欠账一直存在（一个恒返回空的命令），只是从未被记账。**为了让数字好看而不记，才是真正的退步。**

**最终 snapshot 变更单独成 commit**（与代码删除分开，便于归因与回滚）。

---

## 4. 测试矩阵

> **第三列是断言，不是描述。** 每条都按「删掉这一行生产代码，它真的会红吗」验证过。

| ID          | 断言                                                                                                          | **删掉哪行生产代码会让它红**                     |
| ----------- | ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| T-LEDGER-01 | 三态扫描器：**字符串内 `/*`**、**行注释内 `/*`**、**块注释内 `//`** 三类各一例，均不误触发                    | 扫描器里对应的三个状态分支，**逐个删逐个红**     |
| T-LEDGER-02 | 修复后 `core/clash/core.rs` 第 52 行之后的调用**可见**（喂该文件片段，断言 `Logger::global()` 计到 3 而非 0） | 行注释状态分支（删掉即退回 v4 行为，计数塌回 0） |

**本阶段无新增生产类型与消息**，因此**没有 Appendix A**（单点声明附录）——纯删除阶段没有可声明的接线面。这一点本身值得写明：**若实施中发现需要新建类型，说明范围溢出，停下核查。**

**回归契约（措辞必须能区分两种情况，否则它会在正确执行时报警）：**

| 情况                           | 是否允许   | 说明                                       |
| ------------------------------ | ---------- | ------------------------------------------ |
| **存活测试被迫修改**           | **不允许** | 意味着某个「已死」判定是错的——**停下核查** |
| **被删模块自带的单测随其消失** | **预期**   | 属主没了，测试跟着没，**不是回归**         |

**具体预期：后端测试 467 → 466。** 被删的那一个是 `core/state.rs:178` 的 `#[test] fn test_managed_state()`——它**属于 S2 要删的文件**，随属主一同消失。

> **v5 写的「467 条全绿、不应有任何测试被迫修改」是假的**：它没区分这两种情况。实施者删完看到 466 会以为判据破了，**然后要么补一个假测试凑数、要么去怀疑某个「已死」判定**。**一个会在正确执行时报警的判据，比没有判据更糟——它训练人忽略警报。**

> **T-LEDGER-01/02 属 Deno 测试套件，不进 `test:backend`**，不计入上面的后端计数。

**已存在、需在门禁中具名的集成测试**：`s09_process_fixed_port_hold_conflict_and_frees_after_stop`（`process_core_bridge.rs:1129`，F39）——卡面要求 fixed-port 作为自动化集成测试而非手工 smoke，**它已经存在**，S5 只需确认它在 `pnpm test:backend` 中真的跑到。

---

## 5. 契约归属：签名 / 测试 / 门禁

> **判别口诀**：签名能保证的只有**「这个值到得了这里」**与**「这个类型在此平台不存在」**两类；凡形如**「不会去做某事」**的契约，签名一律保证不了——要么测试、要么门禁、要么 `rg`，**并且必须说得出怎么验**。

| 契约                                            | 由谁保证         | 为什么可验证                                                                                                       |
| ----------------------------------------------- | ---------------- | ------------------------------------------------------------------------------------------------------------------ |
| `Logger` / `core::manager` / `core::state` 归零 | **`rg` 判据**    | 删除类不变量用 grep 最直接；**判据须限定 `crate::`**（S2）                                                         |
| `Instance` / `CoreManager::status` 归零         | **`rg` 判据**    | 同上；**不含 `CoreManager` 本身**（S4）                                                                            |
| 扫描器不再因单文件卡死                          | **可复跑的筛查** | S1 的计数比对命令——**同一方法既发现 bug 也验收修复**。注意**判据是基线数字，不是该命令输出为空**                   |
| Updater 的 core 耦合不回退为持有                | **签名**         | `update_core(&core_type, core)` 按调用传入，**manager 结构体上没有可存它的字段**——「结构上不可能」而非「约定不要」 |
| bindings 零变化                                 | **git diff**     | `git diff frontend/interface/src/ipc/bindings.ts` 为空                                                             |

---

## 6. 「永远发生在某阶段」类断言的复核

> 5b 实施期教训：leader 裁定的「守卫获取永远是 pre-commit」是错的。本节把本计划所有此类断言对着调用图列出。

| 断言                             | 依据                             | 复核结论                                                                                           |
| -------------------------------- | -------------------------------- | -------------------------------------------------------------------------------------------------- |
| 「`Logger` 的写入者永不执行」    | F3（`Instance::try_new` 零调用） | **成立**——依赖「没有第二条构造 `Instance` 的路径」，**实施时须再 grep 一次**                       |
| 「`core/manager.rs` 全无调用者」 | F27                              | **成立**（`grant_permission` / `escape` 各自零命中）                                               |
| 「`core/state.rs` 全无调用者」   | F27                              | **成立**，但**判据写法曾是错的**——裸 `core::state` 会命中另一个 crate 的同名模块（S2 已修）        |
| 「`CoreManager` 可随本阶段归零」 | ——                               | **不成立**——`change_default_network_dns` 仍活着且是 C3 的迁移目标。**v4 的出口判据据此写错，已修** |

---

## 7. 风险与回滚

| 风险                                 | 概率 | 影响                 | 缓解                                                                  |
| ------------------------------------ | ---- | -------------------- | --------------------------------------------------------------------- |
| ledger bug 未先修 → 删除后差异对不上 | 高   | 判据失效、误判为回归 | S1 强制最先做且单独成 commit + 单独 snapshot                          |
| 删 `Instance` 牵出未知调用者         | 低   | 编译红               | F28/F29 已逐项列出活面；`cargo check` 逐步验证；**S4 前再 grep 一次** |
| 删除面误伤 C3 要用的代码             | 中   | 5d 无法开工          | §0 纠缠清单 + S4 的「保留」列表逐项对照；**文件保留而非整删**         |
| `migration_markers` 上升被误读为退步 | 中   | 门禁被质疑           | S5 已写明理由；**账本变诚实不等于代码变差**                           |

**回滚**：本阶段全部是删除 + 一处脚本修复，改动彼此独立，任一 commit 可单独 revert。

---

## 8. 提交切分建议

1. `fix(scripts): teach the ledger scanner about strings and line comments` —— S1（**含 snapshot 单独更新**，且**必须最先**）；
2. `refactor(core): delete the unreachable manager and state modules` —— S2；
3. `refactor(core): delete the legacy logger global` —— S3；
4. `refactor(core): delete the dead core manager surface` —— S4；
5. `chore(architecture): record the post-cleanup ledger snapshot` —— S5 的最终 snapshot。

第 1 步**必须单独且最先**：它改的是**判据本身**，与被判据衡量的删除混在一起就无法归因。

---

## 9. Exit 判据映射

| task.md 要求                                                    | 交付步骤 | 验证                                                                                  |
| --------------------------------------------------------------- | -------- | ------------------------------------------------------------------------------------- |
| 删除真正已失去调用者的 core/logger 文件与 globals               | S2/S3/S4 | 三条 `rg` 判据（各自限定范围）+ `cargo check`                                         |
| Updater 不增加 `attach_core_port` 半迁移桥                      | ——       | **no-op**：该符号不存在（F25）；F26 证实耦合已收敛                                    |
| 更新 roadmap/ledger，不以 `CoreManager::global() == 0` 为硬指标 | S5       | ledger 逐 key 分解；**本阶段 `CoreManager::global()` 确实不归零**，正是该条要求的体现 |
| `test_real_dirs == 0`                                           | S5       | ledger 硬门禁                                                                         |
| smoke 1（Local patch/restart/core-switch rollback）             | S5       | 手工执行并把结论写进 PR 描述                                                          |

> **smoke 2 与 smoke 3 已随 C2/C3 移交 PR-5d**。smoke 2 验的是 v1→v2 服务升级（C2 的真正验收点），smoke 3 验的是 macOS TUN/DNS（C3）——两者都不在本阶段的改动面上。**D4 的用户裁定（smoke 3 记为未本地验证且不可由 CI 覆盖）随 smoke 3 一起移交**，因此**本阶段不再被它阻塞**。

---

## 10. 移交 PR-5d 的事实清单（**不属本阶段，勿作本阶段依据**）

> 这些事实是 C2/C3 的调查产物，**在本阶段既不成立为依据、也不该被删掉**。此处只留**索引与一句话摘要**；**全文见 `git show 5a02a1727:docs/superpowers/plans/2026-08-02-pr5c-residual-cleanup.md`**（v4 终态，含完整表格与附录 B 的全部设计）。5d 文档建好后从那里搬运。
>
> **不在此处内联全文**，是为了避免它们被误读成本阶段的判据——这正是拆分要解决的问题。

**C2 运行模式（F9–F17）**

| ID  | 一句话                                                                                              |
| --- | --------------------------------------------------------------------------------------------------- |
| F9  | `pending_run_type` 在 Rust 源码中**不存在**（仅设计文档命中）→ 卡面该项是 no-op                     |
| F10 | 「reconcile 走 `CoreOperationGuard`」**已满足**（`request.rs:87`）                                  |
| F11 | 5 s 轮询与三个 statics 全在 `service/ipc.rs`；`spawn_health_check` **4 处 spawn**                   |
| F12 | `get_ipc_state()` **5 处生产读**（含 `RunType::default()` 内那处）                                  |
| F13 | `RunType::default()` 读两个 global 且被 `CoreStatusView::initial()` 调用——**删 statics 的主阻塞点** |
| F14 | `set_backend` **生产调用点恰好一个**；**不存在 `set_mode`**                                         |
| F15 | `ServiceControlOps` 只有 install/start/stop/restart；**update / uninstall 不在 trait 上**           |
| F16 | `uninstall_service` **绕过 facade**；`install_service` **不 reconcile**（两处不对称，性质不同）     |
| F17 | `KILL_FLAG` 的 stop 路径用 **weak CAS**——但它随轮询线程一起删，**不单独修**                         |

**C3 macOS DNS（F18–F24）**

| ID  | 一句话                                                                           |
| --- | -------------------------------------------------------------------------------- |
| F18 | `MacosDnsGuard` **不存在**（仅两条「等 PR-5c 建它」的注释）                      |
| F19 | 真正的覆写代码是 `CoreManager::change_default_network_dns` + `previous_dns` 状态 |
| F20 | 它读两个 global，且 **Service / Local 双路径在此分叉**                           |
| F21 | **IPC `set_dns` 已上线**（端点 + wire golden 均在）                              |
| F22 | **DNS 与 start/stop 今天毫无保序**；**走 restart 的路径根本不碰 DNS**；失败被吞  |
| F23 | **退出不恢复 DNS**——覆写跨崩溃/退出泄漏（**5c 之前就存在的缺陷**）               |
| F24 | `SystemDnsCache` 只管 flush，**与 TUN 的 DNS 覆写生命周期无关**，勿混淆          |

**smoke / CI（F33–F36）**

| ID  | 一句话                                                                                                  |
| --- | ------------------------------------------------------------------------------------------------------- |
| F33 | CI **有** macOS runner 且在 PR 上跑 `cargo test --all-features` → cfg 门控单测真实运行                  |
| F34 | **但没有任何作业能跑 smoke 3**——无作业启动应用；TUN 需签名扩展 + root，**能力边界非配置缺失**           |
| F35 | `IPC_STATE` 初值 `Disconnected` + bootstrap 先读 → **今天 bootstrap 恒判 `Normal`**，靠首次轮询异步纠正 |
| F36 | **探针两半已存在**：`control::status()` + 纯函数 `target_ipc_state()`；`health_check` = 两半 + 循环     |

**另需搬运**：附录 B 全部设计（C2 探针与九处调用点、C3 适配器与恢复拆分）、D2 / D3 / D4 三项裁定及其理由、以及第二轮对抗审的 **#2–#6 五条 BLOCKING**（不可线性化、`health_check` 的警告职责、拆 DNS 的守卫跨度与 `reconcile` 死锁、读路径同样不检查退出码、适配器接线不全）——**它们是审查者送的免费设计输入，5d 起草时直接作为已知待解问题**。

### 10.1 本阶段**携带**的已知活缺陷（不修，如实记账）

| 缺陷                                                                                                                        | 后果                                                                                           | 为何不在本阶段修                                                                                                                                                                 |
| --------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `core/service/control.rs:274`（`stop_service`）用 `compare_exchange_weak` 做**一次性**置位（`let _ =`，不在循环里、不重试） | 弱 CAS 允许伪失败；伪失败时 `KILL_FLAG` 未置位 → **健康检查线程在服务停止后仍按 5 s 周期空转** | 本阶段限定为**可证死代码的删除**，改活代码的行为会破坏 §4 那条最强判据（「不应有任何测试被迫修改」）——改了要么得配测试（即"有测试被迫改动"），要么不配（即引入未验证的行为变更） |

> 同文件 `:179`（`uninstall_service`）用的是**强** `compare_exchange`。**这个不对称本身就说明 `:274` 是疏漏而非有意**。
>
> **谁来收**：该健康检查线程在 **PR-5d** 随 C2 整体删除，届时这处 CAS **一并消失**——所以 5d 里它**不需要被修，只需要被删**。
>
> 记在这里而不是让它随拆分消失，与 §3 `migration_markers` 上升那段是同一条原则：**账本的价值在于如实记，不在于数字好看**。一个「知道它存在、知道后果、知道谁来收」的缺陷，比一个悄悄消失的缺陷强得多。

---

## 11. 修订索引

| 版本  | 变化                                                                                                                                                                    |
| ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| v1–v4 | C1–C4 全范围的四轮迭代（两轮对抗审 38 / 43 分，16 条 BLOCKING 几乎全在 C2/C3）。**终态见 `5a02a1727`**                                                                  |
| v5    | **用户裁定拆分**：整文件重写为纯删除计划；C2/C3/D2/D3/D4/附录 B/smoke 2/smoke 3 移交 PR-5d；ledger 预期按收窄范围重算；修 #7 的四条门禁错误；事实编号保持原号、空缺可见 |

> v1–v4 的逐版索引表**已随重写移除**：它们索引的是一份不同范围的文档，留在这里只会让读者去找不存在的小节。**版本沿革见 git 历史与上表。**
