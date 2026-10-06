# 运行时快照内存占用优化

**日期：** 2026-10-06

**状态：** P1–P3 已实施，§8 的决策均按推荐执行；S5 已在真机测量（§9.2）。P4 未做。实施记录见 §9。

**基线：** 起草时为 `main@019cfd300`；实施与测量在 rebase 后的 `main@5a5d16ecb`（runtime 子模块 rc.13）上完成。

**来源：** [Windows 内存剖析报告](../../review/2026-10-06-windows-memory-profile.md) §4.2 与 §5 第 5 项；一份外部分析（用户提供的 ChatGPT 对话），其论断已逐条对照源码核验，结论见附录 A。

**需求：** 降低 `RuntimeSnapshotStore` 所持运行时快照的常驻内存与构建峰值。报告中运行时配置快照合计 4.83 MiB（dev 配置每份约 0.45 MiB，约十份），总量随订阅规模成倍增长。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec > 后续实施计划。

## 1. 决策与范围

1. **常驻可还原的表示，不常驻展开的表示。** 配置快照图以现有的 Full/Delta 编码常驻，节点内容在读取时还原。
2. **同一份数据只保留一种常驻形态。** YAML 文本、JSON、diff 等派生视图在读取时计算。
3. **对外行为不变。** `RuntimeInspection`、`RuntimeInspectionContent`、`get_runtime_yaml`、`get_runtime_config` 的输出逐字段不变；inspection id 的失效语义、比较父节点、独立分支基线、日志归属不变；产物文件与发给内核的文本逐字节不变。
4. **不新增缓存、服务或全局状态。** 按需还原的第一版不加缓存。

**包含：** nyanpasu-config 的快照图与 executor 输出；tauri `client/runtime.rs`、`client/runtime_inspection.rs` 中快照的表示；apply 路径上的快照副本；产物文本与 intent 文本的共享。

**不包含：**

- 代理缓存的 fingerprint（报告 §4.4）：分支 `perf/proxy-fingerprint-hash`（`d547f07a4`）已在处理；代理名称 interning 另议。
- core-manager 的三份配置副本（报告 §4.2 第三点）：位于 runtime 子模块，自有仓库与工作区；本 spec 只记录事实（§2.5），改动作为后续 runtime PR（§7 P4）。
- 前端改动。

## 2. 已确认事实

### 2.1 配置快照图

| 事实                                                                                                                                                             | 位置                                                                                                           |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `ConfigValue` 是结构共享的有序树：字符串 `Arc<str>`、数组 `Arc<[ConfigValue]>`、对象 `Arc<IndexMap<Arc<str>, ConfigValue>>`；路径更新只复制被触及的脊            | `backend/nyanpasu-config/src/runtime/value/mod.rs:15-30`                                                       |
| builder 的每个节点缓存 `full: Arc<ConfigValue>`；`push` 时由 `KeyframePolicy::encode` 决定存 `Full(Arc<ConfigValue>)` 还是 `Delta(json_patch::Patch)`            | `backend/nyanpasu-config/src/runtime/snapshot.rs:336-339`、`:463-469`、`:527-541`                              |
| `encode`：patch 序列化长度超过完整值的 50% 时选 Full；patch 回放失败或对象顺序不一致时也选 Full。无论选哪种，patch 都已经算出                                    | `snapshot.rs:399-418`                                                                                          |
| 独立分支的根强制为 Full，baseline 为 `Independent`                                                                                                               | `snapshot.rs:548-591`                                                                                          |
| 存储图 `StoredConfigSnapshotsGraph` 的节点只有 payload、tag、baseline、next，不含 `changed_fields`                                                               | `snapshot.rs:341-375`                                                                                          |
| executor 末尾调用 `builder.build()`，即 `build_stored()?.materialize()`；`RuntimeArtifact.graph` 是展开图                                                        | `backend/nyanpasu-config/src/runtime/executor/mod.rs:372`；`snapshot.rs:603-605`；`executor/artifact.rs:57-65` |
| 展开图的每个节点持有一份完整的 `serde_json::Value`（开启了 `preserve_order`）。Full 且以父节点为基线的节点，其 `changed_fields` 要在展开时与父节点重新 diff 得到 | `snapshot.rs:33-39`、`:865-886`；`backend/nyanpasu-config/Cargo.toml:38`                                       |
| `materialize_node` 递归时，每一层都持有本节点的 config，直到整棵子树处理完；线性管线的展开峰值约为 2N 棵树（N 份存入结果，N 份在栈上）                           | `snapshot.rs:880-904`                                                                                          |
| `encode` 每次 push 都把父、子各 `to_json` 一次，并用 `serde_json::to_vec` 序列化完整值，只为取长度；上一步的子 JSON 就是下一步的父 JSON，却被重算                | `snapshot.rs:399-404`、`:996-1000`                                                                             |
| `build_tree()` / `ConfigSnapshotTreeNode`（每个节点一个 `Arc<ConfigValue>`）已经存在，生产代码未使用                                                             | `snapshot.rs:377-380`、`:593-596`                                                                              |
| `StoredConfigSnapshotsGraph` 没有持久化；除 `snapshot.rs` 外只有 `invalidation.rs` 以引用读取它                                                                  | `backend/nyanpasu-config/src/runtime/invalidation.rs:43`、`:111`                                               |

### 2.2 管线节点与脚本步骤

| 事实                                                                                                                                                                                   | 位置                                                                       |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| 主线节点 = 根 + 全局 transform 数 + WhitelistFieldFilter + GuardOverrides + 内建脚本数 + IncludeAllExpansion（可选）+ Finalizing；scoped transform、composition 成员另成节点或独立分支 | `executor/mod.rs:222-372`                                                  |
| 应用成功后，`with_effective_config` 再追加一个 CoreController 节点                                                                                                                     | `backend/tauri/src/client/runtime_inspection.rs:60-114`                    |
| mihomo 系有 3 个内建脚本（`verge_hy_alpn`、`verge_meta_guard`、`config_fixer`，均为 JS）；clash-rs 有 `config_fixer` 与 `clash_rs_comp`                                                | `backend/nyanpasu-application/src/enhance/runtime_builder.rs:77-110`       |
| 脚本步骤的路径是 `ConfigValue` → `serde_yaml::Mapping` → 脚本 → `Mapping` → `ConfigValue`，输出是一棵全新的树，与输入不共享任何子树                                                    | `backend/nyanpasu-platform/src/enhance/script/adapter.rs:34`、`:59`、`:72` |

### 2.3 `RuntimeSnapshot` 与 `RuntimeSnapshotStore`

| 事实                                                                                                                                                                                                                                             | 位置                                                                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `RuntimeSnapshot` 持有 `product_bytes: Arc<[u8]>`、按值持有的 `config: serde_yaml::Mapping`、`effective: Option<CoreEffectiveConfig>`（其 `config` 是内核回读的 YAML 文本），以及 `inspection: Arc<RuntimeInspectionData>`（展开图 + step_logs） | `backend/tauri/src/client/runtime.rs:64-78`；`runtime_inspection.rs:18-22`                                                                                                                                              |
| `product_bytes` 由文件头加上 `serde_yaml::to_string(&config)` 组成；intent 的 `config_text` 对同一个 `Mapping` 再序列化一次；receipt 的 `config_text` 又从 intent 复制出一份 `Arc<str>`                                                          | `backend/tauri/src/client/application_workflow/adapters.rs:81-85`；`application_workflow/preparation.rs:139`；`backend/tauri/src/core/actor_v2/intent.rs:41`；`backend/tauri/src/client/core_lifecycle/workflow.rs:626` |
| `config: Mapping` 只有三处读取，都是为了再序列化：intent 文本、`get_runtime_config` 的 JSON、`get_runtime_yaml` 的文本                                                                                                                           | `preparation.rs:139`；`runtime_inspection.rs:324-341`                                                                                                                                                                   |
| artifact 转换时把 `final_config` 转成 `serde_yaml::Value`，再 `as_mapping().cloned()`；转换后 `final_config` 不再保留                                                                                                                            | `backend/tauri/src/enhance/artifact_snapshot.rs:95-121`                                                                                                                                                                 |
| store 的状态是 `promoted`、`confirmed { receipt, artifact, inspection, available }` 与 `transition`                                                                                                                                              | `runtime.rs:174-183`                                                                                                                                                                                                    |
| 一次成功的 apply 会把 `RuntimeSnapshot` 整份 clone 三次，每次都深拷 `Mapping`：写入 `applied_binding`、`record_confirmed_apply` 内的 `without_effective_config`（没有 effective 时也整份 clone）、`with_effective_config`                        | `core_lifecycle/workflow.rs:617`；`runtime.rs:234`；`runtime_inspection.rs:108`、`:130`                                                                                                                                 |
| `with_effective_config` 深拷整个 inspection（全部节点的 JSON），`append_transition` 再 clone 一次整张候选图；新增的 CoreController 节点 JSON 与 `effective.config` 文本是同一内容的两种形态                                                      | `runtime_inspection.rs:76`、`:98-107`；`snapshot.rs:293`                                                                                                                                                                |
| `without_effective_config` 再深拷一次 inspection，并截掉最后一个节点                                                                                                                                                                             | `runtime_inspection.rs:128-156`                                                                                                                                                                                         |
| 稳态收敛：effective 就绪后，`applied()` 让 promoted 与 confirmed.artifact 指向同一个 Arc。effective 未就绪时，二者是两次 clone 得到的不同 Arc，各持一份 `Mapping`，共享 inspection                                                               | `runtime.rs:210-226`、`:290-314`；`core_lifecycle/workflow.rs:670-675`                                                                                                                                                  |
| `inspection_summary` 只用到 tag、next、key、changed_fields 与日志；`inspection_content` 按节点读取本节点与比较父节点的配置，并且已经在 `spawn_blocking` 中执行                                                                                   | `runtime_inspection.rs:158-247`、`:305-321`                                                                                                                                                                             |
| 前端只消费 `RuntimeInspection` / `RuntimeInspectionContent` DTO，展开图类型不出现在前端绑定中。inspect 页按需调用 `inspectRuntimeNode`；`getRuntimeYaml` 由三个设置页通过 `useRuntimeProfile` 读取                                               | `runtime_inspection.rs:24-57`；`frontend/nyanpasu/src/pages/(main)/main/profiles/inspect/route.tsx:39-109`；`frontend/query/src/ipc/use-runtime-profile.ts:14-19`                                                       |

### 2.4 估算与测量

一份已应用的快照在稳态常驻：（主线节点数 + 1）棵 `serde_json` 树、1 棵 `Mapping`、产物文本、receipt 文本与 effective 文本。mihomo、无用户 transform、开启内建脚本时，主线约 7–8 个节点。按节点数推算，展开图是报告 4.83 MiB 中最大的一项。

M1、M2 的实测（合成配置：500 个节点、50 个组、10000 条规则，YAML 约 0.5 MB；临时代码，未入库）：

| 测量                                                         | 结果                                                                                                                                              |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| M1：mihomo 主线（含 1 个全局 overlay、3 个真实 JS 内建脚本） | 根为 Full，其余 8 个节点全部是 Delta。3 个脚本节点均为 0 个操作，没有因数字表示、键序或数组重排被迫选 Full。GuardOverrides 为 10 个操作、496 字节 |
| M2：单棵树的存活字节                                         | `serde_json::Value` 1.70 MB；`ConfigValue` 1.29 MB；`serde_yaml::Mapping` 2.38 MB；YAML 文本 0.52 MB                                              |
| M2：7 节点主线的图                                           | 存储图除共享的根之外 2.9 KB；按节点展开为 JSON 共 12.0 MB                                                                                         |

结论：D1、D3 按推荐执行。M3 与 S5 一起在 rebase 后的基线上测量，见 §9.2。

### 2.5 core-manager（runtime 子模块，不在本 spec 范围）

| 事实                                                                   | 位置                                                                                           |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `prepare` 从 `self.document.clone()` 开始                              | `backend/nyanpasu-runtime/crates/nyanpasu-core-manager/src/config/mod.rs:145`                  |
| `EpochPlan` 同时持有 `source_document`（克隆）与 `effective_document`  | `nyanpasu-core-manager/src/manager/switching.rs:419-420`、`:486-487`；`manager/mod.rs:201-202` |
| 发布 config commit 时再执行一次 `Arc::new(effective_document.clone())` | `nyanpasu-core-manager/src/manager/publish.rs:51`；`manager/mod.rs:595`                        |

## 3. 目标表示

```text
RuntimeSnapshot
  ├─ revision、target_core、binding、host ……（不变）
  ├─ 配置正文：只序列化一次的 Arc<str>；产物、intent、receipt、get_runtime_yaml 共用
  ├─ effective：内核回读文本（不变）+ stamp 时算出的 changed_fields
  └─ inspection: Arc<RuntimeInspectionData>
        ├─ graph: StoredConfigSnapshotsGraph（Full/Delta，节点带 changed_fields）
        └─ step_logs
```

### 3.1 快照图：常驻存储图，节点按需还原

- `StoredConfigSnapshotState` 增加 `changed_fields`，在 push 时由 `encode` 已经算出的 patch 得到；独立分支的根为 `None`。语义与当前展开结果一致（§6 S1）。
- 新增单节点还原：沿父链找到最近的 Full，转成 JSON 后依次应用 Delta。不展开兄弟分支，也不缓存中间结果。父链通过 `next` 反查（节点数为一到两位数，O(N) 可以接受，与现有 `comparison_parent` 的做法相同）。同一次请求中，本节点与比较父节点共用一次链式还原。
- executor 改为输出 `build_stored()`，`RuntimeArtifact.graph` 改为存储图。`materialize()` 退出生产路径，是否保留见 §8 D4。
- `encode` 复用上一步算出的子 JSON 作为下一步的父 JSON；长度比较改用计数写入器，不再分配完整的字节缓冲。这只影响构建期的 CPU 与峰值，不改变 Full/Delta 的选择结果（§6 S1 覆盖）。

### 3.2 effective：作为附加视图，不进入图

- `with_effective_config` 不再克隆 inspection，也不修改图。它只记录 effective 文本、host/generation（这两项已有），以及 stamp 时算出的 `changed_fields`（相对 Finalizing 节点）。
- 生成摘要时，把 effective 投影为 Finalizing 节点的子节点：id 等于图的节点数（与现在追加后的 id 相同），tag 仍为 `BuiltinStep { CoreController }`。读取内容时从 effective 文本解析，比较父节点是 Finalizing。
- `without_effective_config` 只清除 effective 相关字段并更换 `inspection_id`。"截掉最后一个节点"这个约定随 `append_transition` 一起删除。

### 3.3 `RuntimeSnapshot`：只保留一种配置形态

- 删除 `config: Mapping`，改为只序列化一次的正文 `Arc<str>`。产物字节 = 文件头 + 正文；intent 与 receipt 引用同一个 `Arc<str>`（`RuntimeIntent.config_text` 改为 `Arc<str>`）。
- `get_runtime_yaml` 直接返回正文；`get_runtime_config` 从正文解析出 JSON。
- 序列化的来源从 `Mapping` 改为 `final_config: ConfigValue`，需要验证产物字节不变（§6 S3）。如果有差异，就在 artifact 转换中仍经 `Mapping` 序列化一次，随后立即丢弃 `Mapping`（只影响构建期）。
- 完成后，`RuntimeSnapshot` 的所有重字段都在 Arc 之后，clone 的成本与配置规模无关，apply 路径上的三次 clone 不再需要单独处理。

## 4. 已否决的方案

| 方案                                                 | 否决理由                                                                                                                                                 |
| ---------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 每个节点常驻 `Arc<ConfigValue>`（`build_tree` 路线） | 脚本步骤的输出不共享子树（§2.2），mihomo 默认的 3 个脚本节点就是 3 棵完整的树；存储图在 Delta 命中时只存 patch。读取的即时性对低频的 inspection 并不重要 |
| 给节点加 `OnceLock` 完整配置缓存                     | 用户把节点点一遍，就退化为全量常驻                                                                                                                       |
| 把各节点的 `serde_json::Value` 包进 `Arc`            | 只能消除同一节点的多处持有，节点之间的重复依旧                                                                                                           |
| 只把 `config` 改为 `Arc<Mapping>`                    | 能消除 apply 中的深拷，但仍常驻一棵只用于再序列化的树；§3.3 一并解决。保留为 §3.3 序列化结果不一致时的退路                                               |
| 为恢复历史节点而重新执行脚本                         | 脚本状态、外部输入或文件内容的变化会让历史 inspection 失真；只还原当时记录下来的结果                                                                     |

## 5. 实施前测量

- **M1 Delta 命中率：** 用真实的 `EnhanceScriptRunner` 与内建脚本（装配方式同 golden 套件，`backend/tauri/src/enhance/golden.rs`），构建一个合成的大配置（建议 500 个节点、50 个组、10000 条规则）。记录每个节点的 payload 种类、patch 操作数，以及 patch 与完整值的序列化长度比。重点看三个 JS 脚本节点会不会因为数字表示、键序或数组重排被迫选 Full。
- **M2 单棵树的存活字节：** 同一份配置分别以 `serde_json::Value`（preserve_order）、`serde_yaml::Mapping`、`ConfigValue` 和 YAML 文本持有；再比较展开图与存储图两种 `RuntimeInspectionData` 的存活字节。用计数 allocator 统计，先例为 `backend/nyanpasu-geodata/tests/build_heap.rs`。
- **M3 端到端基线：** 沿用报告的 Windows dev 配置与 `dhat-heap` feature，记录改造前 t-end 与峰值中快照相关调用栈的字节数，作为 §6 S5 的基线。

M1、M2 用临时代码执行，结果写回 §2.4，代码不入库（与报告的做法一致）。如果 M1 显示脚本节点普遍为 Full，§3.1 依然成立：Full 节点存的是 `Arc<ConfigValue>`，预期不比现在的 `serde_json` 树大（由 M2 确认），但预期收益要下调，§8 D1 需要重新确认。

## 6. 成功判据

- **S1 图等价：** 对 `snapshot.rs` 现有的图测试与 golden 各场景构建出的每张图，每个节点按需还原的 JSON 与 `materialize()` 的结果相等，存储的 `changed_fields` 与展开结果相等（包括独立分支根为 `None`、Full 且以父节点为基线的节点）。删掉 `changed_fields` 的记录，或删掉还原中任意一次 Delta 应用，这条都会失败。
- **S2 inspection 等价：** 在 golden 场景下（含 effective 节点），`RuntimeInspection` 摘要与每个节点的 `RuntimeInspectionContent`（yaml、diff、logs）改造前后逐字段相等；关于旧 inspection id 失效语义的现有测试保持通过。
- **S3 文本等价：** 在 golden 场景下，产物字节、intent 文本与 `get_runtime_yaml` 的输出改造前后逐字节相等，`get_runtime_config` 的 JSON 相等。
- **S4 结构性（计数 allocator 测试）：**
  - (a) 对每步只改一个顶层标量的管线，存储图的存活字节随步数的增量远小于单棵树：步数从 4 增加到 16，增量小于单棵 `ConfigValue` 的 10%。如果退回展开图，增量会是 12 棵树。
  - (b) `RuntimeSnapshot::clone` 分配的字节与配置规模无关：两种规模的配置下分配量相同。
- **S5 端到端：** 在 M3 的条件下，dhat t-end 中快照相关的字节下降。结果写回本 spec 或报告；不设硬阈值，但要说明与 §2.4 估算的差异。
- **S6 常规检查：** 相关的 `pnpm test:backend`、`pnpm lint:clippy`、`pnpm lint:rustfmt`、`deno task lint:architecture-ledger`。

## 7. 实施顺序

```text
P0 M1、M2、M3 测量                                        -> verify: §2.4 写入实测数字，确认 §8 D1、D3
P1 nyanpasu-config：存储节点带 changed_fields、单节点还原、encode 复用
                                                          -> verify: S1（新 API 与 materialize() 对照）
P2 executor 输出存储图；inspection 改读存储图；effective 改为附加视图
                                                          -> verify: S2、S4(a)
P3 RuntimeSnapshot 只留正文文本；intent、receipt 共享同一份
                                                          -> verify: S3、S4(b)，随后做 S5
P4 （runtime 子模块，另开 PR）core-manager 的 source/effective document 与 config commit 共享 Arc
                                                          -> verify: 子模块测试；本仓库 bump pin
```

P1 与 P2 可以是同一个 PR 中的两个提交：`RuntimeArtifact.graph` 的类型变化要求 executor 与 tauri 调用方在同一个提交里一起修改。每一步都跑 S6。

## 8. 待确认的决策

1. **D1 常驻表示：** 存储图 + 按需还原（推荐），还是每个节点一个 `Arc<ConfigValue>`？理由见 §4；M1 之后复核。
2. **D2 effective 的 `changed_fields`：** stamp 时算一次并存储（推荐，读摘要零成本），还是每次读摘要时现算？
3. **D3 `RuntimeSnapshot` 的配置形态：** 只保留正文文本（推荐），改为 `Arc<ConfigValue>`，还是最小改动 `Arc<Mapping>`？
4. **D4 `materialize()` 的去留：** 推荐把 `materialize()`、`ConfigSnapshotsGraph`、`ConfigSnapshotState` 降为 `#[cfg(test)]`，作为 nyanpasu-config 内部的等价性对照；`append_transition` 与其测试删除。`ConfigSnapshot` 仍被 `inspection_content` 用来计算 diff，保留。
5. **D5 P4 的时机：** 推荐在 P3 之后另开 runtime PR。子模块独立发版，需要单独 bump pin。

## 9. 实施记录

### 9.1 与 §3 的偏差

| 偏差                                                                                                                            | 原因                                                                                                                                                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `encode` 不跨步复用子 JSON，只改用计数写入器                                                                                    | 复用要求 builder 在两次 push 之间持有一棵 JSON 树，而这段时间正是脚本与 transform 运行的时候，会抬高构建峰值。§3.1 原文"只影响构建期的 CPU 与峰值"的前提不成立；省下的只是每步一次 `to_json` 的 CPU                                                           |
| effective 视图是 `Option<Arc<EffectiveInspection>>`，替代原来的 `effective` 与 `effective_host` 两个字段                        | effective 文本随配置规模增长，不放在 Arc 后面，`RuntimeSnapshot::clone` 的成本就仍与配置规模相关（S4(b)）。节点 id、父节点、tag、`changed_fields` 都在 stamp 时算好                                                                                           |
| 还原失败用 `expect` 处理                                                                                                        | 存储图由 executor 构建并校验，每个 Delta 的回放在记录时已经验证过，还原失败只可能是不变量被破坏。没有为此新增 `RuntimeError` 变体                                                                                                                             |
| `get_runtime_config` 的 JSON 由 Finalizing 节点还原，而不是从正文解析                                                           | 解析正文会引入 YAML 解析器的 128 层嵌套上限：transform 产出更深的结构时，构建与产物都正常，只有这个视图失败。改造前的视图来自内存转换，没有这个上限。Finalizing 节点就是产物文档，还原也比解析便宜                                                            |
| 回放保真检查（`same_representation`）同时比较浮点的位                                                                           | `0.0 == -0.0`，只改变零的符号时 patch 为空，Delta 回放会丢掉负号。改造前的 inspection 内容就有这个失真；`get_runtime_config` 改由图还原后，它会让视图与产物不一致。改为在这种情况下记录 Full 关键帧，使"还原结果与 executor 的值逐字节一致"成为记录器的不变量 |
| `validate_tree_shape`、`validate_tree_links`、`MAX_MATERIALIZE_DEPTH` 收窄为 `cfg(any(test, feature = "snapshot-persistence"))` | 生产路径不再展开，它们只剩归档解码与测试在用                                                                                                                                                                                                                  |
| 归档格式版本升到 3                                                                                                              | 存储节点多了 `changed_fields`，线格式变化；旧版本按既有约定解码失败、视为缓存未命中                                                                                                                                                                           |
| 删除 `RuntimeBuildError::SerializeRuntimeConfig`，重新生成绑定，并删去 `ipc-error.ts` 中对应的一个 `case` 标签                  | 产物文本改在 artifact 转换中序列化（复用 `SerializeFinalConfig`），该变体不再被构造。这是本 spec 唯一的前端改动                                                                                                                                               |

### 9.2 判据核验

| 判据 | 结果                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| S1   | `assert_restore_matches_materialize` 覆盖 `snapshot.rs` 的图测试（含 Delta 链、强制 Full、键序回退、独立分支）与 executor 的组合测试。`materialize()` 降为 nyanpasu-config 内部的 `cfg(test)`，tauri 的 golden 场景改由 S2 的前后对照覆盖                                                                                                                                                                                                  |
| S2   | 临时对照（未入库）：7 个场景（clean-seed 组合、带 base 组合、mihomo 与 clash-rs 内建脚本、白名单、bare、上述大配置），改造前后的摘要与每个节点的内容（yaml、diff、logs）逐字节相同；stamp、`without_effective_config`、再次 stamp 后的视图同样相同。旧 inspection id 失效的现有测试通过                                                                                                                                                    |
| S3   | 同一对照中，产物字节、intent 文本与 digest、`get_runtime_yaml` 的文本、`get_runtime_config` 的 JSON 逐字节相同；另有测试覆盖超过 YAML 解析器嵌套上限的文档                                                                                                                                                                                                                                                                                 |
| S4   | (a) `nyanpasu-config/tests/snapshot_heap.rs`：计数 allocator 下，步数 4→16 的存活字节增量小于单棵树的 10%。(b) 以指针共享断言代替分配计数：stamp 后的快照 clone 与原快照共享正文、inspection 与 effective 视图。tauri 的 lib 测试没有可替换的全局 allocator。保证范围限于配置文档：`exists_keys` 与 `postprocessing_output`（脚本日志）仍随 clone 深拷，日志若输出整份配置，其大小也随配置规模增长；这是改造前就有的行为，不在本 spec 范围 |
| S5   | 见 [dhat 对照报告](../../review/2026-10-07-runtime-snapshot-memory-profile.md)：同一 dev 配置、窗口打开 90 s，Rust 堆 t-end 从 10.99 MiB 降到 8.23 MiB；快照相关存活分配从 3.63 MiB 降到 0.61 MiB（−83%），与 §2.4 一致。改造后剩余的快照分配主要是两个 `ConfigValue` 关键帧（根与一个内建脚本节点，共 0.45 MiB）                                                                                                                          |
| S6   | `cargo test`（nyanpasu-config 全 feature、clash-nyanpasu lib）、`pnpm lint:clippy`（改动文件无新增告警）、`pnpm lint:rustfmt`、`deno task lint:architecture-ledger`、`@nyanpasu/nyanpasu`/`rpc`/`query` 的 typecheck 均通过                                                                                                                                                                                                                |

## 附录 A：外部分析的核验结论

| 外部分析的论断                                                                      | 核验结果                                                                                                                                         |
| ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ConfigValue` 已经结构共享；已有 Full/Delta 存储与 `KeyframePolicy`                 | 属实（§2.1）                                                                                                                                     |
| executor 调用 `build()`，把整张图展开后常驻；inspection 的摘要与内容已经分开读取    | 属实（§2.1、§2.3）                                                                                                                               |
| 存储节点不含 `changed_fields`，Full 节点要重新 diff，因此应在记录时保存             | 属实。补充：`encode` 无论选哪种 payload 都已经算出 patch，记录不增加开销                                                                         |
| `with_effective_config`、`without_effective_config`、`append_transition` 会整份克隆 | 属实。补充：CoreController 节点与 `effective.config` 文本是同一内容的两份，附加视图可以直接删掉其中一份                                          |
| "最终配置仍然保留一份 `final_config: Arc<ConfigValue>`"                             | 不准确：转换之后 `final_config` 就被丢弃了，`RuntimeSnapshot` 中只有 `Mapping`                                                                   |
| promoted / pending / applied 之间重复持有同一构建                                   | 只部分成立：稳态会收敛到同一个 Arc；重复主要出现在 apply 过程中，以及 effective 未就绪时（§2.3）                                                 |
| "路线二需要新增结构"                                                                | 有遗漏：`build_tree()` / `ConfigSnapshotTreeNode` 已经存在                                                                                       |
| 未提及                                                                              | 本 spec 补充：`config: Mapping` 只用于再序列化，可以删除；产物文本与 receipt 文本重复；展开的递归峰值约 2N 棵树；`encode` 重复转换并序列化完整值 |
