# PR-5b 实施计划 — 单一 runtime apply 管线

**日期：** 2026-08-02
**版本：** v6（四轮复审后定稿：NH4–NH9 + H1–H3 + 三项 Medium；**D1–D5 全裁 A** 不重开）
**分支基线：** `refactor/core-manager-actor` @ `9727ef1d4`（PR-5a 阶段门已关闭：`ae4e0f288` + `6a3878bba` + `5277482a5` + `9727ef1d4`）
**权威 spec：** `docs/superpowers/specs/2026-08-01-pr5-core-actor/task.md` 卡 B1–B4；`design.md` §6–§8
**路线图定位：** `docs/design/actor-migration-roadmap.md` §6.2；必答项 §6.4 **RQ-01 / RQ-03**
**平台：** Windows 11 / PowerShell

> **本计划对着已实施的 5a 代码写，不是对着 5a 计划的纸面附录写。** 全部事实来自 `9727ef1d4` 的工作树。

**v2 修订索引（leader 审查）：**

| 项  | 内容                                                 | 落点                        |
| --- | ---------------------------------------------------- | --------------------------- |
| L1  | `CandidateFile` 一并迁入 `core/actor/runtime.rs`     | S1、A.1、A.2、§4 D1         |
| L2  | `RuntimeRevisionAllocator` **留在 client 侧**        | S1、S2、A.1、A.3、§4 D1 后  |
| L3  | `restart()` 残余审计 → T-B4-03 改钉真实语义、F8 重述 | F26/F27、S6、§4 D5、T-B4-03 |
| L4  | 后台 rebuild 的 degradation 去向（I-B 缺口）         | F28/F29、§2.4、S2、T-PC-09  |
| L5  | 删除 `LifecycleSnapshot` 消息                        | F30、A.2、S2                |
| L6  | S4 重试判据预先定死；specta 已启用（新事实 F25）     | F25、S4、§8 风险表          |
| L7  | T-PC-06 用 `TestBackend` 脚本化传输错误              | §6.2 T-PC-06                |

**v2.1 盖章（leader 裁定 D5=A + 两条 rider）：**

| 项  | 内容                                                                        | 落点                           |
| --- | --------------------------------------------------------------------------- | ------------------------------ |
| D5  | 停止态 `change_core` 走 `RunningIdentity` 分支 + 仓内 `RuntimeApplyOutcome` | §4 D5、S6、A.4                 |
| R1  | §3 表补 `Started` 行（Applied **推进**）；restart 失败走 post-commit 路径   | §3.1、S6 流程、T-B4-03/05、A.6 |
| R2  | 仓内枚举是**语义**选择而非编译必要；F25 保留原样                            | §4 D5 的 R2 注                 |

**v3 修订索引（codex 对抗审 REJECT → 逐条处理）：**

| 项  | 结论                                                                                    | 落点                                |
| --- | --------------------------------------------------------------------------------------- | ----------------------------------- |
| C1  | `RuntimeApplyOutcome` 加第八变体 `NotApplied`；T-PC 系列补 report 值断言                | F31、§2.2、§3.1、A.4、§6.2          |
| C2  | `CheckAndPromote` 改为 **`PublishPromoted { operation, snapshot }`**；文件工作留 client | F32、A.2、A.3、S2、§4 D1 的 L1 后记 |
| C3  | 推进决策与 wire 表示拆分：`advances_applied` 纯谓词 + `PublishApplied` 守卫消息         | F33b、A.2、A.5、S2、S4、S7          |
| H4  | F12 是错的——gate→begin 例外**三处**不是一处                                             | F12 改写、S3、T-B2-02               |
| H5  | `RunningIdentity` 的 `Ok(None)` ≠ 核已停止（attach 场景）                               | F33、S6 分支规则、T-B4-06           |
| H6  | rebuild 管线有**四类**调用上下文，不是两类                                              | F34、§2.1、§2.4、T-PC-10/11         |
| H7  | `change_clash_core` **去掉** `run_legacy_verge_mutation` 包裹                           | F35、S6、§10                        |
| H8  | `expected` 改为「观察到 revision 则 `Some`，缺失则 `None`」                             | F36、S4                             |
| M9  | `RuntimeLifecycleStore` 的删除从 S1 挪到 S2                                             | S1、S2                              |
| M10 | T-AP-13 空转——`TestBackend::apply` 是 `unreachable!`                                    | F37、§6.1 T-AP-13                   |
| M11 | 必删测试清单补 5 条                                                                     | §6.4                                |
| M12 | S3 顺序自相矛盾——统一为**先分配 revision**                                              | S3                                  |
| L13 | F10 措辞收窄                                                                            | F10                                 |

**v4 修订索引（复审 REJECT：3 High + 2 残留 + 6 处陈旧文本）：**

| 项       | 结论                                                                                            | 落点                              |
| -------- | ----------------------------------------------------------------------------------------------- | --------------------------------- |
| NH1      | `RunningIdentity` reply 扩为 **(身份, `FaithfulLifecycle`) 原子守卫联合读**；真值表改读忠实六态 | F40、S6 真值表、A.2、A.3、T-B4-06 |
| NH2      | lease seam 声明**类型化** `CheckAndPromoteError { phase, source }`；相位由构造位置打标签        | F41、A.1b、S2 分工表              |
| NH3      | I-A 加两条豁免；§2.2 第 4 行拆 4a/4b；分裂窗口诚实记录 + 自愈论证，**不加二层恢复**             | F42、§2.2、§2.3、A.6 注           |
| C1 残留  | T-B4-05 补 `value` 三字段断言                                                                   | §6.3 T-B4-05                      |
| M10 残留 | parity 改测**真实转换层**；`TestBackend` 不得冒充 parity                                        | §3.2                              |
| 陈旧 ×6  | D1 两处 / D5 一处 / S4 实参 / S6 箭头 / S7「两条」/「其余 8 处」                                | 各处就地                          |

**v5 修订索引（三轮复审 REJECT：NH4–NH9 + 四项机械修）：**

| 项      | 结论                                                                                               | 落点                    |
| ------- | -------------------------------------------------------------------------------------------------- | ----------------------- |
| NH4     | `CoreActorError` 加 `LifecycleInvariant(_)`（双 kind）；豁免 (b) 写成可 `matches!` 的规则          | F43、§2.3、A.1          |
| NH5     | §2.2 拆出 E-a / E-b 发布中止行块；四处 categorical 表述改**引用** §2.3                             | §2.2、§2.1、I-B、S6     |
| NH6     | T-PC-04 拆 04a / 04b；新增 T-PC-12 豁免边界分类测试                                                | §6.2                    |
| NH7     | 行 7 拆 7a / 7b / 7c；report 段落排除 7a（`RolledBack` **有** `CoreApplyData`）                    | §2.2                    |
| NH8     | `NoBackend` 独立成行 6b（`core_backend_unavailable`）                                              | §2.2、S6、§2.3          |
| NH9     | 新函数改名 `runtime_outcome_from_apply_data`；既有同名函数**不动**、引用处写全限定名               | F44、A.5、§3.2、T-AP-13 |
| 机械 ×4 | A.6 表格归位（两 code 复位）／陈旧 `Ok(None)` 形消除／T-AP-13 删 TestBackend 前置／D1 删已裁条件句 | 各处就地                |

**v6 修订索引（四轮复审 REJECT：H1–H3 + 三项 Medium）：**

| 项   | 结论                                                                                               | 落点                                     |
| ---- | -------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| H1   | 同款 categorical 措辞另三处改引用；**用 grep 收口该措辞类**（第四次同形残留）                      | §2.1 边界定义、入口表、S6 流程、I-A 首句 |
| H2   | `CheckAndPromoteError` **只覆盖 check + 文件工作**；发布类错误以**裸 `CoreActorError`** 逃回编排层 | F45、D3 措辞、A.1b doc                   |
| H3   | 新增 **A.7 有序分类器**——`Backend(_)` 必须过它才能定行；每条判据锚到真实类型化谓词                 | F46/F47/F48、A.7、§2.3                   |
| M ×3 | T-PC-12 进出口与提交切分／T-AP-11/12 补 report 值断言／S4 的「第 7 行」改 7b                       | §7、§9、§6.1、S4                         |
| 索引 | 补齐 v5 与 v6 两张索引表（v5 那张原押后，现并入本版）                                              | 本表与上表                               |

---

## 0. 本阶段的边界

**做（= task.md B1–B4）：**

1. **B1**：Promoted / Applied 所有权迁入 `CoreActor`；删除 client 侧 `RuntimeLifecycleStore` 与 `publish_promoted` / `publish_applied` / `restore_promoted`；lifecycle 经第二条 watch 暴露；
2. **B2**：`CoreOperationGuard` 取代 `rebuild_gate`；全部事务**先取 guard 再读 snapshot**；保留 rebuild worker 的 capacity-1 coalesce；
3. **B3**：删除 API-first patch 与补偿层；`apply_promoted` 改走 `CoreBackend::apply`，按 `CoreApplyData` 映射结果；
4. **B4**：`change_core` 降级为普通 commit-first mutation，返回通用 `MutationOutcome<RuntimeApplyReport>`。

**不做（越界即返工）：**

- 不动 `CoreBackend` 封闭 enum 的形状，不加 trait / factory（已裁定）；
- 不加 actor 层二次恢复（design §5）；
- 不做 watch snapshot 的**状态投影**扩展、log ring、`set_mode` / `reconcile_mode`、macOS DNS（全部 C1–C3）；
- 不删 5 s 健康轮询线程与 `IPC_STATE` static（C2）；
- 不迁移 `feat::patch_clash_with_rebuild` 里的 sysproxy / systray / locale 副作用编排（PR-6e）；
- 不动 `on_profile_change` 的连接中断服务（PR-6）；
- 不碰 Updater 的 `UpdaterManager::global()`（PR-6d）。

---

## 1. 已核验事实（2026-08-02，全部读自 `9727ef1d4` 工作树）

### 1.1 5a 交付的 actor 面（B1/B3 的落点）

| ID  | 事实                                                                                                                                                                                                                   | 锚点                                 |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| F1  | `CoreActorState` 已有 `observed: BackendObservation`、`running: Option<CoreRequest>`、`status_tx: watch::Sender<CoreStatusView>`、`backend: Option<BackendSlot>`；**没有** promoted/applied 字段                       | `core/actor/mod.rs:47-62`            |
| F2  | `commit()` 与 `commit_backend()` **分流**：前者只更新缓存 + 发布 watch + 跑 latch + 终止态清 `running`；后者额外把 `view.run_type` 归一化为 `state.mode`。**合成观察走 `commit()`，真实后端观察走 `commit_backend()`** | `mod.rs:168-180`（`9727ef1d4` 引入） |
| F3  | `CoreBackend::apply(&request, expected: Option<RevisionIdInfo>) -> Result<CoreApplyData, CoreBackendError>` **已实现**（D4=A：5a 实现但生产未接线）                                                                    | `core/actor/backend.rs:344-348`      |
| F4  | actor 消息集已含 `Check` / `Run` / `Stop` / `Recover` / `SetBackend` / `RefreshStatus` / `RefreshHint` / `RunningIdentity` / `Shutdown`；**没有** apply 类消息                                                         | `mod.rs:70-111`                      |
| F5  | `CoreActorError` 四变体：`StaleOperation` / `NoBackend{last_error}` / `Backend(Arc<..>)` / `ShuttingDown`                                                                                                              | `core/actor/types.rs:59-69`          |
| F6  | `CoreOperationGuard::acquire` 超时常量 `CORE_ACQUIRE_TIMEOUT = 120s`                                                                                                                                                   | `client/core.rs:30`、`:278-293`      |

### 1.2 lease seam 现状（B2/B3 的落点）

| ID  | 事实                                                                                                                                                                                                                                                                           | 锚点                     |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------ |
| F7  | `CoreLeaseAdapter` 字段：`guard` / `core` / `application` / `requests` / `runtime_paths` / `target_core: Option<ClashCore>`                                                                                                                                                    | `client/core.rs:331-338` |
| F8  | **`restart()` 用 `self.target_core.take()`**（一次性消费）——`9727ef1d4` 的语义之一：回滚深路径因此会回退到 typed 快照里**已提交的旧核**，这是有意的                                                                                                                            | `client/core.rs:463-471` |
| F9  | 五个 lease 方法的当前实现：`check_and_promote` 经 actor `check` + 客户端文件工作；`apply_candidate` **混合**（actor `check` + 裸 `put_configs`）；`apply_promoted` **纯裸 HTTP**；`restart` 经 actor `run`；`stop` 经 actor `stop`                                             | `client/core.rs:390-477` |
| F10 | **PR-5b 迁移范围内唯一的全量 runtime apply 通道**是 `apply_config_from()`：5 次重试包 `crate::core::clash::api::put_configs`，每次间隔 250 ms。`feat.rs:79` 的 `change_clash_mode` 是另一条裸 `put_configs`，但它只切单个 mode 字段、属 PR-6，不在本阶段管线内（L13 收窄措辞） | `client/core.rs:479-489` |

### 1.3 client 侧待删除面（B1/B2/B3 的落点）

| ID  | 事实                                                                                                                                                                                                                                                                                                                                                                                                                                                                   | 锚点                                                                |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| F11 | `NyanpasuClientInner` 含 `clash_patch` / `clash_patch_gate` / `rebuild_gate` / `rebuild` / `runtime_revisions` / `runtime`                                                                                                                                                                                                                                                                                                                                             | `client/mod.rs:232-261`                                             |
| F12 | `rebuild_gate` **共 10 处获取**；**「gate → `core.begin()` 紧邻」的只有 7 处，例外有 3 处**（v2 写「唯独 promote_default 一处」是错的，H4 纠正）：`promote_default_runtime_config`（gate `rebuild.rs:406` → begin `:443`，其间建 snapshot + candidate）、`promote_existing_runtime_product`（gate `mod.rs:1391` → begin `:1423`，其间分配 revision、读产物、建 snapshot + candidate）、`start_promoted_runtime`（gate `mod.rs:1436` → begin `:1440`，其间读 Promoted） | `mod.rs:1391,1436,1512,1574,1590`；`rebuild.rs:232,257,268,282,406` |
| F13 | `clash_patch_gate` **只有 1 处获取**（`patch_running_config`）                                                                                                                                                                                                                                                                                                                                                                                                         | `mod.rs:1511`                                                       |
| F14 | `publish_applied` **8 个调用点**、`publish_promoted` 3 个、`restore_promoted` **仅 1 个**（change_core 深回滚）；另有一处**绕过 publisher 的裸写** `runtime.write().applied = ...`                                                                                                                                                                                                                                                                                     | `mod.rs:1343/1360/1380`；裸写 `mod.rs:1501`                         |
| F15 | `regenerate_runtime_inner` **先分配 revision、再读 typed snapshots**，doc 明写「必须在 `rebuild_gate` 下运行」                                                                                                                                                                                                                                                                                                                                                         | `mod.rs:1595-1611`                                                  |
| F16 | `regenerate_runtime_with` 是 typed 与 legacy 两条路径**共用的** candidate→check→promote 核心                                                                                                                                                                                                                                                                                                                                                                           | `mod.rs:1613-1687`                                                  |
| F17 | `patch_running_config` 的 API-first 顺序：捕获 lifecycle → `compensation_for` → **先打 `clash_patch.patch()` 到运行核** → 再 `patch_clash_with_rebuild` → 失败走 `restore_applied_after_patch_failure`                                                                                                                                                                                                                                                                 | `mod.rs:1507-1571`                                                  |
| F18 | `restore_applied_after_patch_failure` 的 `Ok` 分支**不可达**——它总是以 `bail!` 收尾                                                                                                                                                                                                                                                                                                                                                                                    | `mod.rs:1445-1505`（`:1502-1504`）                                  |
| F19 | `change_core` 有**三条回滚分支**：A 构建失败（discard，产物未动）／B 回滚重建成功后重启旧核／C 回滚重建也失败 → `restore_product` + `restore_promoted` + 重启旧核                                                                                                                                                                                                                                                                                                      | `rebuild.rs:275-400`                                                |
| F20 | **`ControllerBinding` 与 `config_patch_from_mapping` 在代码库中不存在**——仅出现在 spec/roadmap 文本里。B3 卡上这两项对当前代码是 **no-op**                                                                                                                                                                                                                                                                                                                             | 全仓 grep 仅命中 docs                                               |
| F21 | **`RuntimeApplyReport` 与 `ChangeCoreReport` 都不存在**；`change_clash_core` 与 `patch_clash_config` 两个命令目前都返回 unit `Result`                                                                                                                                                                                                                                                                                                                                  | `ipc.rs:479-496`、`:435-458`                                        |
| F22 | `MutationOutcome::from_parts` 是 **`CommittedDegraded` 的唯一产出点**（degradations 为空即 `Applied`）；`DegradationPhase` 已含 `CoreRollback` 与 `RuntimeApply`                                                                                                                                                                                                                                                                                                       | `client/runtime.rs:395-404`、`:456-471`                             |
| F23 | `RebuildCoordinator` 的 capacity-1 coalesce：`mpsc::channel(1)` + `try_send` 丢弃 + 500 ms 接收端去抖 + `try_recv` 排空；worker 经 `Weak<NyanpasuClientInner>` 调 `rebuild_running_config()`                                                                                                                                                                                                                                                                           | `rebuild.rs:21,24-44,58-187`；`mod.rs:435-449`                      |
| F24 | service harness 测试须套 `transport_available()` 守卫（`9727ef1d4` 的语义之三）；Unix 下它探测 `/var/run` 可写性                                                                                                                                                                                                                                                                                                                                                       | `core/actor/backend.rs:864-880`                                     |

### 1.4 v2 新增事实（L3/L4/L5/L6 的依据）

| ID  | 事实                                                                                                                                                                                                                                                                                  | 锚点                                                                       |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| F25 | **workspace 已启用 `specta`**：`nyanpasu-utils` 与 `nyanpasu-ipc` 的 features 含 `"specta"`，`apply.rs` 的 `ApplyOutcomeKind` / `CoreApplyData` 都带 `#[cfg_attr(feature = "specta", derive(specta::Type))]`。**A.4 可直接内嵌 upstream 类型，无需镜像 DTO**                          | `backend/Cargo.toml:36-41`；`nyanpasu_ipc/src/api/core/apply.rs:34-38,66`  |
| F26 | `lease.restart()` 今日**生产**调用点共 4 类 6 处：`start_promoted_runtime`（启动）、`patch_running_config`（B3 删）、`regenerate_and_restart_for_legacy`（legacy replay）、`change_core` 三处（B4 删）。**B3+B4 之后仍剩两类**——启动路径与 legacy replay，二者都不在 5b 范围内        | `mod.rs:1441,1540`；`rebuild.rs:271,315,333,377`                           |
| F27 | **`CoreBackend::apply` 要求核已在运行**——upstream doc 原文「apply never starts one」。今日 `change_core` 用 `restart()`，**核处于停止态时会把新核启起来**；而 `rebuild_running_config` / `patch_running_config` 的 apply 走裸 HTTP，核停止时本来就失败。故只有 `change_core` 有语义差 | `apply.rs:10-18`；`rebuild.rs:315`                                         |
| F28 | 后台 rebuild worker 对 `rebuild_running_config()` 的 `Err` **只写 `tracing::warn!`**，无人接收；唯一会把它转成 degradation 的是**同步** caller `collect_post_commit_degradations`，经 `map_runtime_rebuild_degradation` 塌成单一 `runtime_rebuild_failed` / `RuntimeBuild`            | `rebuild.rs:172-174`；`mod.rs:1013-1016`、`:979-988`                       |
| F29 | `CoreDegradationSink` 已存在并已注入 actor（5a 的 D5 latch 用它发 `core_recovery_exhausted`）；生产实现 `TauriCoreDegradationSink` 落到 `UiEventSink::notice_message`。`ClientSetupArgs` 有 `degradation` 字段，但 **`NyanpasuClientInner` 没有保存它**                               | `backend.rs:581-583`；`mod.rs:36,193`；`event_sink.rs:85-105`；`mod.rs:86` |
| F30 | `commit()` 在**消息处理函数内**就 `status_tx.send_replace`，早于 reply 发出。因此「await 守卫消息的 reply」happens-before「读到新的 watch 值」——测试不需要额外的快照消息                                                                                                              | `mod.rs:168-175`                                                           |

### 1.5 v3 新增事实（codex 对抗审的依据，全部已复核）

| ID   | 事实                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | 锚点                                                                                                                                                             |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| F31  | `MutationOutcome` 的**两个变体都要求 `value: T`**——`Applied { value }` 与 `CommittedDegraded { value, degradations }`。因此 post-commit 失败也必须给出一个诚实的 report 取值（C1 的根因）                                                                                                                                                                                                                                                                                                                                                  | `client/runtime.rs:382-392`                                                                                                                                      |
| F32  | `RuntimeSnapshot` **完全在 client 侧构建**：revision、`RuntimeSnapshotData`、解析后的 config、target core、产物字节全部在 `lease.check_and_promote()` **调用之前**备齐；candidate 哈希比对也在 client 侧（`:1668-1673`）。actor 拿不到构建这份快照所需的任何输入（C2 的根因）                                                                                                                                                                                                                                                              | `mod.rs:1656-1686`                                                                                                                                               |
| F33  | `pre_start` **会观察后端**：注入或新建 backend 后调 `observe_status()` 并把结果写进 `observed`——因此进程启动时 attach 到一个**已在运行**的 Service 核是可能的；但 `state.running` 恒初始化为 `None`，只有 GUI 发过 `Run` 才填充。**`RunningIdentity` 的 `Ok(None)` 因此不等于「核已停止」**（H5 的根因）                                                                                                                                                                                                                                   | `core/actor/mod.rs:320-347`、`:355`                                                                                                                              |
| F33b | `Run` 处理器只做「调 backend.run → `commit_backend`」，**不碰 lifecycle**——5a 的任何消息都没有推进 Applied 的途径（C3 的根因之一）                                                                                                                                                                                                                                                                                                                                                                                                         | `core/actor/mod.rs:393-401`                                                                                                                                      |
| F34  | rebuild 管线的调用上下文**有四类**，不是两类：①同步 post-commit（`collect_post_commit_degradations`，`mod.rs:1012`）；②后台 worker（`mod.rs:444`）；③**`enhance_profiles` 命令**——doc 明写「there is no prior state commit, so a failure is a plain error」（`ipc.rs:135-140`）；④legacy no-commit 入口 `regenerate_and_apply_for_legacy` / `regenerate_and_restart_for_legacy`（`rebuild.rs:254,267`）（H6 的根因）                                                                                                                       | `mod.rs:1012,444`；`ipc.rs:135-140`；`rebuild.rs:254,267`                                                                                                        |
| F35  | `run_legacy_verge_mutation<F, Fut>` 的签名是 `Fut: Future<Output = anyhow::Result<()>>` → `ClientResult<()>`，**装不下 `MutationOutcome<RuntimeApplyReport>`**；且它在 `mutate()` 成功后还要做 legacy restore / 投影刷新 / typed patch plan / finalize，**这些失败都在 typed commit 之后却返回普通 `Err`**。它在 `change_clash_core` 的存在理由由代码注释写死：「核心切换动了 legacy verge,须回灌 typed actors」——**而 B4 恰恰删掉那次 legacy verge 写入**（H7 的根因）                                                                    | `bridge/verge.rs:257-300`；`ipc.rs:479-496`（注释 `:489`）                                                                                                       |
| F36  | upstream `apply_config` **先取 `ctrl.current` 并在其为 `None` 时直接返回 `Error::NotStarted`，早于 `expected_revision` 的 CAS 比较**。停止核天然没有 revision，因此「缺失 expected 即不变量破坏」的规则与「停止核给 degraded」的要求互斥（H8 的根因）                                                                                                                                                                                                                                                                                      | `nyanpasu-core-manager/src/manager/apply.rs:19-37`（`:26` 取 current，`:29-37` 才比 CAS）                                                                        |
| F37  | `CoreBackend::apply` 的 `Test` 分支是 **`unreachable!("test backend does not implement apply")`**——`TestBackend` 今天根本无法脚本化 apply（M10 的根因）                                                                                                                                                                                                                                                                                                                                                                                    | `core/actor/backend.rs:369`                                                                                                                                      |
| F38  | **typed → legacy 的镜像由 state actor 的提交路径自动维护，与任何包裹器无关**：`ApplicationActor::commit` 每次 typed 提交都走 `prepare_replace`（内含 `bridge.prepare(&next)`）→ `manager.upsert` 持久化 → **`mirror.apply()`**；条件替换路径的 `Replaced` 分支同样调 `mirror.apply()`。投影函数 `apply_app_config_to_legacy_verge` **含 `clash_core`**（`draft.clash_core = Some(yaml_convert(snap.core)?)`）。`Patch` 消息也汇入同一个 `commit`。**`fn apply(self: Box<Self>)` 返回 unit——镜像不可失败**，因此不引入新的 degradation 路径 | `state/application.rs:75-87`（commit）、`:101-103`（条件替换）、`:140-150`（Patch→commit）；`bridge/verge.rs:678`（clash_core 投影）、`:149-152`（`apply` 签名） |
| F39  | change_core 之后仍在读 legacy `clash_core` 的残余读者共 4 处：托盘两处、`feat.rs` 的核心选择、clash core 模块一处。它们读的是 F38 那条镜像，**B4 之后由 typed 提交自动刷新**                                                                                                                                                                                                                                                                                                                                                               | `core/tray/mod.rs:167,336`；`feat.rs:379`；`core/clash/core.rs:98`                                                                                               |
| F40  | **`CoreStatusView.state` 是二值投影，不能用来判「核在不在跑」**：`map_local_status` 的 `lifecycle` 忠实保留六态，但 `state` 把 `Running`/`Switching`/**`Stopping`** 压成 `CoreState::Running`，把 `Stopped` 之外的其余（**`Starting`**/**`Restarting`**）落进兜底 `_ => Stopped(None)`。即 **Starting→Stopped、Restarting→Stopped、Stopping→Running 三个分支会被反转**（NH1 的根因）                                                                                                                                                       | `core/actor/backend.rs:438-457`（`:445-447` 兜底、`:450-452` 三态归 Running）；`types.rs:20`                                                                     |
| F41  | lease seam 的 `check_and_promote` 返回 **`anyhow::Result<[u8; 32]>`**——一个**未分化**的错误类型，**不携带任何相位信息**。v3 说「anyhow 天然区分 check 与 promote」是**错的**：知道相位的是 adapter 里的**构造位置**，不是错误值本身；照 v3 写下去，`runtime_check_failed` / `runtime_promote_failed` 的区分只能靠嗅探错误字符串（NH2 的根因）                                                                                                                                                                                              | `client/core_bridge.rs:48-58`                                                                                                                                    |
| F42  | promote 是**先写后验**：`restore_product` 写入产物后才读回比对哈希（`core.rs:421` 写、`:422-427` 验）。因此「产物已是新值、Promoted 仍是旧值」的分裂窗口**今天的代码里就存在**，不是 5b 引入的（NH3 的根因之一）                                                                                                                                                                                                                                                                                                                           | `client/core.rs:405-428`                                                                                                                                         |
| F43  | **`CoreActorError` 今天只有四个变体** `StaleOperation` / `NoBackend` / `Backend` / `ShuttingDown`——**没有任何不变量类变体**。因此 v4 的「豁免 (b) 靠类型区分」在代码里**没有落点**：实施者要么挪用 `StaleOperation`（语义是守卫身份不符，不是单调性），要么裹进 `Backend`（把 bug 伪装成后端故障）——两条路都退回 NH2 刚消灭的字符串嗅探（NH4 的根因）                                                                                                                                                                                      | `core/actor/types.rs:59-70`                                                                                                                                      |
| F44  | **`map_apply_outcome` 这个名字已被占用**：`backend.rs:536` 的既有函数做的是 **manager `ApplyOutcome` → `CoreApplyData`**（Local 侧 wire DTO 转换），有 3 处引用（`:358` 生产、`:975`/`:986` 测试）。C3 要新增的函数方向**相反**（`CoreApplyData` → `RuntimeApplyOutcome`）却同名、且在相邻模块（NH9 的根因）                                                                                                                                                                                                                               | `core/actor/backend.rs:536`、`:358`、`:975`、`:986`                                                                                                              |
| F45  | 今天的实现里 **check/晋升 与 发布本来就是分离的两段**：`check_and_promote` 在 `mod.rs:1678-1680` 完成并 `?` 出错误，`publish_promoted` 在 `:1685` **独立调用**。v5 的 D3 那句「`check_and_promote` **内部**发 `PublishPromoted`」是全文唯一与此矛盾的措辞（H2 的根因）                                                                                                                                                                                                                                                                     | `client/mod.rs:1678-1685`                                                                                                                                        |
| F46  | **`Backend(_)` 是一个未分化的口袋**：`CoreBackendError` 有四支 `Local` / `Service` / `Binary` / `Construct`，而 `backend_error()` 把**全部**裹进 `CoreActorError::Backend`。revision 冲突、not-started、传输丢失**全藏在这一支里面**——没有有序判据，§2.2 的第 5 行与 7b 会掉进 7c 兜底，6a 与 7c 互相歧义（H3 的根因）                                                                                                                                                                                                                     | `core/actor/backend.rs:593-603`；`core/actor/mod.rs:297-299`                                                                                                     |
| F47  | **Local 侧有类型化谓词**：`nyanpasu_core_manager::Error::RevisionConflict { .. }`（`error.rs:44`）与 `Error::NotStarted`（`error.rs:11`）。注意 `NotStarted` 是 **`Err` 而非 outcome**——`manager/apply.rs:26` 的 `.ok_or(Error::NotStarted)?` 与 `:28` 的 `return Err(Error::NotStarted)` 都走错误通道                                                                                                                                                                                                                                     | `crates/nyanpasu-core-manager/src/error.rs:11,44`；`manager/apply.rs:26,28`                                                                                      |
| F48  | **Service 侧的分类键是 `error_kind: Option<String>`，不是 `code`**：`ClientError::Server` 同时带 `code: ResponseCode` 与 `error_kind: Option<String>`，而**后者才是 R0 收敛出来的那个字段**（对照 `api::error_kind::{NOT_STARTED, REVISION_CONFLICT, …}` 字符串常量）。另：`ClientError` **共七支**（`BuildClient` / `Request` / `HttpStatus` / `Decode` / `Server` / `EmptyData` / `WebSocket`）**且标了 `#[non_exhaustive]`**——分类器必须有兜底                                                                                          | `nyanpasu_ipc/src/client/mod.rs:23-62`；`api/mod.rs:40,45`                                                                                                       |
| F49  | **停止态 `restart()` 的失败也是 `Backend(_)`**：`Run` 处理器是 `backend.run(&request).await.map_err(backend_error)`，而 `backend_error()` 把它包成 `CoreActorError::Backend`——与 apply 路径的失败**在类型上无法区分**（A.7 作用域漏洞的根因）                                                                                                                                                                                                                                                                                              | `core/actor/mod.rs:400`、`:297-299`                                                                                                                              |
| F50  | **`start` / `stop` / `restart` 不带 `error_kind`**：submodule 明写它们「predate `error_kind` 并继续返回 `anyhow::Error`」，只有 S8 新增的那批操作才填充该字段；而 `error_kind` 缺失的语义是「**not classified**，never no error」。因此 Service 侧启核失败的 `error_kind` **恒为 `None`**，A.7 第 4 行会**确定性地**把它判成 `Other` → 行 7c——这不是概率问题                                                                                                                                                                               | `nyanpasu-runtime/.../manager_bridge.rs:47-52`；`nyanpasu_ipc/src/api/mod.rs:35-37`                                                                              |
| F51  | **check 阶段本身就是一次 actor 调用**：`CoreClient::check` 返回 `Result<(), CoreActorError>`，而 `check_and_promote` 内部正是 `self.core.check(&self.guard, &request).await?`。所以 seam 的错误面**必然**会遇到 `CoreActorError`——v6 那句「`CheckAndPromoteError` 永不承载 `CoreActorError`」在 check 阶段不可能成立（H2 残留的根因）                                                                                                                                                                                                      | `client/core.rs:172-176`、`:408`                                                                                                                                 |
| F52  | **`HttpStatus` 只在「回包不是格式良好的服务信封」时才产生**：客户端拿到非 2xx 后**先试着把 body 解成服务信封**，解得出且 `code != Ok` 走 `Server`；解不出才落 `HttpStatus`。即它意味着「对端根本没按本协议应答」                                                                                                                                                                                                                                                                                                                           | `nyanpasu_ipc/src/client/mod.rs:135-151`                                                                                                                         |

---

## 2. RQ-01 — post-commit 失败矩阵（必答）

### 2.1 分界线的定义

**分界线是「typed desired state 是否已经提交」**，不是「操作是否已经开始」。

- `ApplicationClient::patch` / `ClashConfigClient::patch` 返回 `Ok` 的那一刻起，用户意图已经持久化 → 此后的失败**除 §2.3 的 E-a / E-b 两条豁免外**都不得表现为普通 `Err`（否则 UI 会显示「失败」而磁盘上已经变了）；
- 在此之前的任何失败，desired 未动，返回 `Err` 是诚实的。

因此 5b 的三类入口有不同的分界位置：

| 入口                                                                                                                         | 有无 desired commit           | 分界                                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------- | ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `patch_running_config`（B3）、`change_core`（B4）                                                                            | **有**，且在最前              | commit 之后**除 §2.3 两条豁免外**全部 degraded                                                                                                          |
| `rebuild_running_config`（**后台脏重建**，`RebuildCoordinator` worker）                                                      | 无（响应更早的 commit）       | **除 §2.3 两条豁免外全部 degraded**——commit 早已发生，此处只是迟到的副作用；投递走 sink，见 §2.4                                                        |
| `rebuild_running_config`（**`enhance_profiles` 命令**，F34 ③）                                                               | 无（doc 明写无前置 commit）   | **全部 `Err`**——命令 doc 原文「there is no prior state commit, so a failure is a plain error」                                                          |
| `regenerate_and_apply_for_legacy` / `regenerate_and_restart_for_legacy`（**legacy 重播**，F34 ④）                            | 无（legacy draft 尚未 apply） | **全部 `Err`**——调用方 `feat::*` 靠 `Err` 触发 `Config::verge().discard()`；`RolledBack` 也必须映射成 `Err`，否则 legacy draft 会被误当成功而 `apply()` |
| `promote_existing_runtime_product` / `start_promoted_runtime` / `promote_default_runtime_config`（启动路径）、`restart_core` | 无                            | 全部 `Err`（没有已提交的用户意图需要保护）                                                                                                              |

### 2.2 七项逐条作答

`P` 列 = 该失败发生在分界线之前（`pre`）还是之后（`post`）。**除 §2.3 的两条豁免（E-a / E-b）外**，`post` 一律映射为 `Degradation { phase, code, retryable }` 并经 `MutationOutcome::from_parts` 变成 `CommittedDegraded`。

| #   | 失败                       | 触发点                                                                   | P       | commit-first 入口的结果                                                                                                                                                               | 无-commit 入口的结果 |
| --- | -------------------------- | ------------------------------------------------------------------------ | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| 1   | **operation acquire 超时** | `begin_operation()` 等待 `CORE_ACQUIRE_TIMEOUT`（F6）                    | **pre** | `Err(OperationError::AcquireTimeout)`——**guard 在 desired commit 之前取得**（见 S3 的顺序约束），因此永远是 pre                                                                       | `Err`                |
| 2   | **build 失败**             | `regenerate_runtime_with` 的 `spawn_blocking` 构建段                     | post    | `phase = RuntimeBuild`、`code = "runtime_build_failed"`、`retryable = true`                                                                                                           | `Err`                |
| 3   | **check 失败**             | `CoreBackend::check`（dry-run）                                          | post    | `phase = RuntimeCheck`、`code = "runtime_check_failed"`、`retryable = true`                                                                                                           | `Err`                |
| 4a  | **promote 前置失败**       | candidate 哈希不符 / `check` 后文件被改 / `restore_product` 写入本身失败 | post    | `phase = RuntimePromote`、`code = "runtime_promote_failed"`、`retryable = true`；**产物保持旧值**——原保证仍然成立                                                                     | `Err`                |
| 4b  | **写后校验失败**           | `restore_product` 已写入**之后**：读回哈希不符                           | post    | `phase = RuntimePromote`、`code = "runtime_promote_failed"`、`retryable = true`、report = `NotApplied`（**message 与 4a 区分**）；**产物已是新值、Promoted 仍是旧值**——见下方分裂窗口 | `Err`                |
| 5   | **revision 冲突**          | `CoreBackend::apply` 的 CAS（`Error::RevisionConflict`）                 | post    | `phase = RuntimeApply`、`code = "revision_conflict"`、`retryable = true`；**Applied 不变**，下一次 rebuild 会带新 revision 重试                                                       | `Err`                |
| 6a  | **IPC 连接丢失**           | Service backend 的传输错误（`ClientError`）                              | post    | `phase = RuntimeApply`、`code = "core_transport_lost"`、`retryable = true`、report = `NotApplied`；**Applied 不变**                                                                   | `Err`                |
| 6b  | **后端不可用**             | `CoreActorError::NoBackend`（后端构造失败，槽位为 `Failed`）             | post    | `phase = RuntimeApply`、`code = "core_backend_unavailable"`、`retryable = true`、report = `NotApplied`（NH8：它与 6a **不是同一回事**，A.6 早有独立 code）                            | `Err`                |
| 7a  | **apply 回滚**             | `CoreApplyData.outcome == RolledBack`（**有** `CoreApplyData`）          | post    | `phase = CoreRollback`、`code = "core_rollback"`、`retryable = true`；**report = `RolledBack`，不是 `NotApplied`**——旧配置确实在跑，这是一个真实终态动作                              | `Err`                |
| 7b  | **核未运行**               | backend 返回 `NotStarted`（F27 / F36）                                   | post    | `phase = RuntimeApply`、`code = "core_not_running"`、`retryable = true`、report = `NotApplied`                                                                                        | `Err`                |
| 7c  | **其它 apply 失败**        | backend 返回其它 `Err`                                                   | post    | `phase = RuntimeApply`、`code = "runtime_apply_failed"`、`retryable = true`、report = `NotApplied`                                                                                    | `Err`                |

**发布中止（两条豁免类；穷尽性要求它们必须上桌，NH5 ①）**

下面两类**不产 degradation、返回 `Err`**——把它们从矩阵里省略，正是 v4 让 4b 变得自相矛盾的原因。判别依据是 §2.3 的 `matches!` 规则，**不是**看失败发生在哪一步：

| #   | 情形                                                          | 元组                                                  | 为什么不降级                                              |
| --- | ------------------------------------------------------------- | ----------------------------------------------------- | --------------------------------------------------------- |
| E-a | 发布时进程正在关停（`ShuttingDown`）                          | `Err`、**无 degradation**、无 report                  | teardown 期没有任何读者会去渲染 degraded 结果（豁免 (a)） |
| E-b | 发布被不变量拒绝（`LifecycleInvariant(_)`、`StaleOperation`） | `Err` + **错误级日志**、**无 degradation**、无 report | 这只可能是 bug，必须响亮失败（豁免 (b)）                  |

> **E-a / E-b 与第 4b 行的区别**：三者都可能发生在「产物已写入」之后，终态也都是产物新 / Promoted 旧。区别**只在错误类型**——4b 是真实的运行时失败（读回哈希不符），E-a / E-b 匹配豁免规则。实施者按 `matches!` 分流，不靠推断失败位置。

> **第八项（D5=A 引入，不在 RQ-01 原列表内）**：停止态 `change_core` 的 `restart()` 失败。desired 早已提交，故同样是 `post`——`phase = CoreLifecycle`、`code = "core_start_failed"`、`retryable = true`，Applied 不推进（§3.1 的 `Started` 行、T-B4-05）。**它不过 A.7 的分类器**——A.7 只管 apply 路径，见其作用域声明。用 `CoreLifecycle` 而非 `RuntimeApply`：失败发生在核**生命周期**上，不是在配置应用上，与 5a 的 `core_recovery_exhausted` 同相。

> **上表各行的 `report` 取值（C1，按 NH7 修正）**：`MutationOutcome` 的两个变体都要求 `value: T`（F31），所以 post-commit 失败也得给出一个诚实的 report。**第 2、3、4a、4b、5、6a、6b、7b、7c 行**都**没有发生任何终态动作**——既没有 `CoreApplyData`，也没有启核——因此一律取 **`RuntimeApplyOutcome::NotApplied`**、`applied_revision = None`，「为什么」由 degradations 承载。**唯一的例外是第 7a 行**：`RolledBack` **是有 `CoreApplyData` 的**，旧配置确实在跑、这是 apply 主动做出的真实终态动作，所以它的 report 就是 `RolledBack`（与 §3.1 的映射一致）——v4 把它一并归进 `NotApplied` 是错的。**E-a / E-b 两类没有 report**（返回 `Err`，不构造 `MutationOutcome`）。`desired_revision` 始终填本次分配的 revision。

> **4b 的分裂窗口：诚实记录，不加第二套回滚（NH3 ②）**
>
> 产物先落盘、`PublishPromoted` 后到，中间任何失败都会留下「产物 = 新、Promoted = 旧」的分裂。**这个窗口今天的代码里就有**——`restore_product` 写完才读回比对（F42：`core.rs:421` 写、`:422-427` 验），**不是 5b 引入的**。
>
> **它是自愈的**：Promoted 只是读模型，产物才是核实际加载的文件；下一次 rebuild（coalesce worker 的脏重建，或任何后续事务）会重新走 build→check→promote 并重新发布，两者随即收敛。窗口期内的可观测后果是「runtime 读 IPC 返回的快照比磁盘产物旧一拍」，不会让核加载到错误配置。
>
> **明确不做**：不加产物回滚、不加二段提交、不加补偿事务——B 范围明令禁止二层恢复（§0），而为一个自愈窗口引入一套恢复机制，恰恰是 5a 已经裁掉的那类复杂度。4a 与 4b 共用 `runtime_promote_failed`，靠 `message` 区分是哪一侧。

### 2.3 三条不变量

- **I-A（不撒谎）**：desired 已提交时，**除下列两条豁免外绝不**返回 `Err`——只返回 `CommittedDegraded`。**两条显式豁免（NH3 ① / NH4）**：
  - **(a) 进程正在关停**（`CoreActorError::ShuttingDown`）：teardown 期没有任何读者会去渲染一个 degraded 结果，返回 `Err` 是诚实的；把它包成 `CommittedDegraded` 只是制造一个没人看的假成功；
  - **(b) 不变量破坏**：**这些只可能是 bug**——守卫串行化 + 单事务单次分配之下，陈旧 operation 与非递增 revision 都不该发生。**bug 必须响亮失败**（`Err` + 错误级日志）；包装成 degradation 等于把实现缺陷伪装成一次「运行时小故障」，让它长期潜伏。

  **豁免的判据是一条可 `matches!` 的类型规则，全文在此声明一次（NH4）：**

  ```rust
  // 豁免当且仅当匹配这两支；其余一律 degraded。
  matches!(err, CoreActorError::ShuttingDown)                  // 豁免 (a)
      || matches!(err, CoreActorError::StaleOperation
                     | CoreActorError::LifecycleInvariant(_))  // 豁免 (b)
  ```

  **`Backend`、`NoBackend`、以及任何 `CheckAndPromoteError` 永不符合豁免**——它们是真实的运行时失败，一律 degraded。这条规则由 **T-PC-12** 钉住（§6.2），其中「`Backend` / `NoBackend` → `CommittedDegraded`」那两例是证明**豁免吞不掉真实失败**的关键断言。

  > v4 说「豁免 (b) 靠类型区分、边界够硬」——**那是错的**：`CoreActorError` 当时只有四个变体，没有任何不变量变体（F43），所谓硬边界并不存在。NH4 补上 `LifecycleInvariant`，规则才真正可判别。

- **I-B（不静默）**：**除本节两条豁免外**，任何 `post` 失败**必须**产出至少一条 `Degradation`，不允许只写日志（豁免类改为 `Err` + 错误级日志，同样不静默）；
- **I-C（状态单调）**：`post` 失败不得回退已经推进的 Promoted；Applied 只在 backend 确认采纳新 revision 时才推进（§3）。

> 与 `CoreActorError` 的关系：`StaleOperation` / `LifecycleInvariant(_)` / `ShuttingDown` 走上面的 `matches!` 豁免规则，**不映射 degradation**；`NoBackend` 按 §2.2 第 **6b** 行处理（`core_backend_unavailable`）；**apply 路径上的 `Backend(_)` 不是单一去向**——它是个未分化口袋（F46），必须过 **A.7 的有序分类器**才能定到第 5 / 6a / 7b / 7c 行；**生命周期路径（停止态 `restart()`）的 `Backend(_)` 不过 A.7**，直接落 restart-failure 行（见 §2.2 第八项）。**后两者永不豁免。**

### 2.4 degradation 投递到哪里（I-B 对**无 caller 的入口**如何满足）

I-B 说「不允许只写日志」，但 rebuild 管线有**四类**调用上下文（F34），其中只有三类有 degradation 的容身处。四条路径必须分清——v2 只写了三条，漏掉 `enhance_profiles` 与 legacy 重播两类，而那两类**根本不该 degrade**：

| 调用上下文                                                                                                                        | 有无前置 commit | 投递路径                                                                                             | 现状                                                 |
| --------------------------------------------------------------------------------------------------------------------------------- | --------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| **① 同步 post-commit**：`collect_post_commit_degradations`（profile mutation 的 `after_commit` 等 5 处，`mod.rs:1012`）           | 有              | 返回值——追加进调用方的 `MutationOutcome<..>` degradations                                            | 已存在，但塌成单一 `runtime_rebuild_failed`（F28）   |
| **② 后台 worker**：`RebuildCoordinator` 的 dirty 重建（`mod.rs:444`）                                                             | 有（更早）      | **`CoreDegradationSink::publish`**（F29 已有的注入面，与 5a 的 `core_recovery_exhausted` 同一 sink） | **今天只有 `tracing::warn!`——I-B 的缺口，5b 必须补** |
| **③ 命令入口**：`patch_running_config` / `change_core`                                                                            | 有              | 返回值——`MutationOutcome<RuntimeApplyReport>`                                                        | S5 / S6 新建                                         |
| **④ 无 commit 入口**：`enhance_profiles`（`ipc.rs:135-140`）、`regenerate_and_{apply,restart}_for_legacy`（`rebuild.rs:254,267`） | **无**          | **普通 `Err`，不产 degradation**                                                                     | 已是现状，5b **不得**改动                            |

> ④ 为什么不 degrade：`enhance_profiles` 的命令 doc 原文就是「there is no prior state commit, so a failure is a plain error」；legacy 重播的调用方（`feat::patch_verge` / `patch_clash` 等）**靠 `Err` 触发 `Config::verge().discard()`**——把失败包装成 `CommittedDegraded` 会让 legacy draft 被误当成功而 `apply()`，在磁盘上留下一份从未生效的配置。**`RolledBack` 在 ④ 里也必须映射成 `Err`**（它意味着新配置没生效），这是 ④ 与 ①②③ 的关键差异。

**因此管线要拆成「内部」与「公共包装」两层：**

```text
内部：rebuild_pipeline(..) -> (Result<RuntimeApplyReport>, Vec<Degradation>)   ← 只报事实，不决定表现形式
  ├─ ① 同步 post-commit → 调用方并入自己的 MutationOutcome
  ├─ ② 后台 worker      → 逐条 degradation.publish()；Err 另记 warn
  ├─ ③ 命令入口         → MutationOutcome::from_parts(report, degradations)
  └─ ④ 无 commit 入口   → 有 degradation 或 RolledBack 即折叠成 Err
                          （公共 rebuild_running_config / 两个 legacy 重播方法
                            的签名与 Err 语义一字不改）
```

**5b 的三项对应动作：**

1. **同步路径精度提升**：`map_runtime_rebuild_degradation` 的 doc 现在写着「不要臆造 `RuntimeCheck` / `Promote` / `Apply` 精度，错误面撑不住」（`mod.rs:979-980`）——5b 恰恰把错误面做出来了。把它改为直接透传管线产出的 `Vec<Degradation>`（§2.2 的 phase/code 分级），删掉那条 doc 与单一 `runtime_rebuild_failed` 常量。
2. **后台路径补 sink**：`NyanpasuClientInner` 增 `degradation: Arc<dyn CoreDegradationSink>` 字段（`ClientSetupArgs` 早有此值，F29，只是没留存）；worker 闭包把 degradations 逐条 `publish`。`tracing::warn!` 保留（可观测性），但**不再是唯一出口**。
3. **④ 保持原样**：公共 `rebuild_running_config()` 与两个 legacy 重播方法的**签名和 `Err` 语义一字不改**，内部改调新管线并在出口折叠。T-PC-10 / T-PC-11 钉住这条。

> 为什么用 `CoreDegradationSink` 而不是 lifecycle watch 携带 `last_error`：watch 是**状态**投影（当前值、可被下一次覆盖），degradation 是**事件**（每条都要送达一次）。用状态通道送事件会在两次重建之间静默丢失中间那条。5a 已经为「事件」选定了 sink，5b 沿用，不新造第二套。

---

## 3. RQ-03 — apply parity 矩阵（必答）

### 3.1 六个 apply outcome + 一个非 apply 终态 + 一个正交标志

`CoreApplyData { outcome: ApplyOutcomeKind, revision: ConfigRevisionInfo, warning: Option<String>, failed_apply: Option<String> }`。

**`Warning` 不是第七个分支**——它是与 outcome 正交的标志位，可以与**任何**一个 outcome 组合出现（来源是 runtime 的 `ApplyOutcome::DurabilityUncertain` 包装，可嵌套两层并以 `"; "` 拼接）。

| outcome                               | Applied 是否推进 | 返回                                         | 说明                                                                                                                                                                                                                        |
| ------------------------------------- | ---------------- | -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Noop`                                | **推进**         | `Applied`                                    | 配置已在生效——运行的就是该 revision，Applied 必须等于 Promoted，否则读模型会永远滞后                                                                                                                                        |
| `Patched`                             | **推进**         | `Applied`                                    | 就地 `PATCH /configs`                                                                                                                                                                                                       |
| `Reloaded`                            | **推进**         | `Applied`                                    | 就地 `PUT /configs`                                                                                                                                                                                                         |
| `Restarted`                           | **推进**         | `Applied`                                    | 同 epoch 内换进程                                                                                                                                                                                                           |
| `Switched`                            | **推进**         | `Applied`                                    | 换核（B4 的正常成功路径）                                                                                                                                                                                                   |
| `RolledBack`                          | **不推进**       | `CommittedDegraded { phase: CoreRollback }`  | **旧配置在跑**；desired 与 Promoted 保留新值，Applied 保留旧值                                                                                                                                                              |
| **`Started`**（非 apply 产出，D5=A）  | **推进**         | `Applied`                                    | 核原本停止，本路径把它启起来了：restart 成功后**运行的就是 promoted revision**，因此与 `Noop` 同理必须推进，否则读模型永远滞后。`report.outcome = Started`                                                                  |
| **`NotApplied`**（非 apply 产出，C1） | **不推进**       | `CommittedDegraded`（degradations 说明原因） | **什么终态动作都没发生**：build / check / promote / 传输 / `NoBackend` 失败时用它。`applied_revision = None`。它存在的唯一理由是 `MutationOutcome` 两变体都要求 `value`（F31），而拿 `RolledBack` 或 `Started` 顶替就是撒谎 |

前六格逐一映射到 `RuntimeApplyOutcome` 的同名变体（A.4）。第七行 `Started` **不由 `apply` 产出**——它是 D5=A 的停止态启核路径直接构造的，**不进 `runtime_outcome_from_apply_data`**，因此不进 §3.2 的 12 格 parity 矩阵；但它是一个真实终态，Applied 推进规则必须在这张表上说清。

**`Started` 路径的失败侧（R1）**：停止态分支的 `restart()` 失败时，desired 早已提交（§2.1），因此**走 §2.2 的 post-commit 路径**——返回 `CommittedDegraded`，`phase = CoreLifecycle`、`code = "core_start_failed"`、`retryable = true`，Applied **不推进**。成功侧由 **T-B4-03** 钉住，失败侧由 **T-B4-05** 钉住（§6.3）。

**Warning 的处理（与上表正交）：**

- `warning.is_some()` 时**追加**一条 `Degradation { phase: RuntimeApply, code: "core_apply_durability_uncertain", retryable: false }`；
- **不改变**上表的 Applied 推进决策；
- 因此 `Applied + warning` 会变成 `CommittedDegraded`（`from_parts` 见 F22），而 `RolledBack + warning` 会有**两条** degradation。

### 3.2 parity 测试要求

矩阵是 **6 × 2 = 12 个组合**（六个 outcome × warning 有/无），每个组合都要断言三件事：Applied 是否推进、返回的 `MutationOutcome` 变体、degradation 列表内容。测试编号 T-AP-01…12（§6）。

**双后端 parity（M10 残留已修）**：Local 与 Service 对同一 outcome 必须产出同一结果，而 parity 的价值全在**后端各自的转换层**，不在共用的 mapper。因此：**Local 侧直接单测真实的 manager-outcome → `CoreApplyData` 转换函数**（喂 manager 的 `ApplyOutcome` fixtures），**Service 侧走 IPC harness 解码**（套 `transport_available()` 守卫，F24），两侧得到的 `CoreApplyData` 逐字段相等后再各自过 `runtime_outcome_from_apply_data`。**`TestBackend` 的脚本化 apply 只服务 actor 层测试，不得冒充 parity**——用它喂 `CoreApplyData` 等于把两个后端的转换层双双旁路掉（F37 说明它今天还是 `unreachable!`，须先补）。

---

## 4. 决策点（D1–D5 leader 已全部裁定，2026-08-02；**无待决项**）

### D1 — `RuntimeSnapshot` 等类型放哪 —— **裁定 A**

B1 要把 Promoted/Applied 搬进 actor，但 `RuntimeSnapshot` / `RuntimeRevision` / `RuntimeLifecycleState` 现在住在 `client/runtime.rs`（F11 邻域）。

- **裁定 A**：把 `RuntimeRevision` / `RuntimeSnapshot` / `RuntimeSnapshotData` / `RuntimeLifecycleState` 移到 **`core/actor/runtime.rs`**（新文件）。理由：所有权跟着数据走；actor 反向依赖 `client::` 是层次倒置。（**`CandidateFile` 原也在此列，v3 起不再迁移**——见下方 v3 后记。）
- **选项 B（未采纳）**：类型不动，actor 直接 `use crate::client::runtime::*`。改动小，但让 `core::actor` 依赖 `client`，与 5a 建立的方向相反。

> **L1 修正（已被 v3 后记取代，保留以存档决策脉络）**：v1 曾把 `CandidateFile` 留在 `client/runtime.rs`，同时让 A.2 的 `CheckAndPromote` 消息携带它——那确实是 D1=A 要消灭的反向依赖。**但 C2 换掉了那条消息，前提随之消失**，最终结论见下方 v3 后记：不迁。
>
> **L2 修正**：`RuntimeRevisionAllocator` **不迁**，留在 `client/runtime.rs`。它是 **ID 源泉**（一个 `AtomicU64`），不是 lifecycle 状态；迁入 actor 会逼出一条 `AllocateRevision` 守卫消息和一次多余的 round-trip，而单调性本来就由 actor 在 promote 时校验（T-LC-01 已钉）。它 `use` 迁走的 `RuntimeRevision`，方向仍是 client → core。**A.1 / A.2 / A.3 / S1 / S2 / S3 五处已按此对齐。**
>
> **v3 后记（C2 使 L1 的前提消失）**：L1 的理由是「`CheckAndPromote` 消息按值携带 `CandidateFile`」。C2 把该消息换成了 `PublishPromoted { operation, snapshot }`——**actor 不再碰任何文件类型**，`CandidateFile` 也就不必迁移。因此实施时 **`CandidateFile` 留在 `client/runtime.rs`**：L1 要保护的不变量（`core::actor` 不反向依赖 `client::`）由 C2 更彻底地实现了，而搬一个 actor 用不到的类型属于无谓改动。**这是对 L1 落地方式的修正，不是对 L1 裁定的推翻**；leader 已于 2026-08-02 确认。

### D2 — lifecycle 用第二条 watch 还是塞进 `CoreStatusView` —— **裁定 A**

B1 要求「CoreClient 通过 watch 暴露 lifecycle」。

- **裁定 A：第二条 watch 通道** `watch::Sender<RuntimeLifecycleState>`，与 `status_tx` 并列。理由：`CoreStatusView` 是 UI 投影（5 个小字段，每次 `commit()` 都克隆），而 `RuntimeSnapshot` 持有产物字节与整个 config Mapping——塞进同一条通道会让每次状态变化都克隆一份重量级快照。
- **选项 B（未采纳）**：扩 `CoreStatusView`。省一条通道，但把重负载塞进高频路径，且会改 5a 已稳定的 `commit()` 语义（F2）。

### D3 — `apply_candidate` 的去留 —— **裁定 A**

`apply_candidate`（F9）今天只被 `restore_applied_after_patch_failure` 使用，而后者随 B3 删除。

- **裁定 A：一并删除** `apply_candidate`，`CoreLifecycleLease` 收敛到 4 个方法（`check_and_promote` / `apply_promoted` / `restart` / `stop`）。C2 之后 lease 的 `check_and_promote` **名字与文件工作都留在 client 侧**，且它**不发 `PublishPromoted`**，且其错误面是 A.1b 的**外层和** `CheckAndPromoteFailure`（actor 错误原样透出，H2 残留）——发布是编排层紧随其后的**独立一段**（H2；今天的实现本就如此，F45）。`restart` 因 F26 的两类残余调用者而**保留**（见 D5）。理由：删掉唯一调用者后 `apply_candidate` 就是死代码。
- **选项 B（未采纳）**：保留备用。违反「不留无调用者的抽象」。

### D4 — `change_core` 的 wire 变化幅度 —— **裁定 A**

B4 让 `change_clash_core` 返回 `MutationOutcome<RuntimeApplyReport>`（F21：今天返回 unit）。

- **裁定 A**：新增 `RuntimeApplyReport { outcome, desired_revision: u64, applied_revision: Option<u64> }`（design §8 的形状），命令返回 `MutationOutcome<RuntimeApplyReport>`。bindings 因此新增两个 TS 类型 + 命令返回类型变化——**这是本阶段唯一的 wire 变化**，S9 按「恰好这些」核对。`specta` 已在 workspace 启用（F25），upstream 类型可直接内嵌，**不做镜像 DTO**。
- **选项 B（未采纳）**：仍返回 unit，degraded 只进日志。B4 卡明写「前端复用通用 `RuntimeApplyReport`/MutationOutcome 展示 degraded」。

### D5 — 核处于停止态时 `change_core` 怎么办（L3 审计的产物）—— **裁定 A**

L3 让我审计 `restart()` 残余，审出一条 v1 漏掉的语义差（F26/F27）：

- `CoreBackend::apply` **不启核**（upstream doc「apply never starts one」）；
- 今天的 `change_core` 用 `lease.restart()`，因此**核停止时切核会把新核启起来**；
- 另两条 apply 入口（`rebuild_running_config` / `patch_running_config`）今天走裸 HTTP，核停止时本来就失败——所以**只有 `change_core` 有行为差**。

若 B4 无条件改走 `ApplyPromoted`，「停止态下切核」会从「切完并启动」变成「切完但失败」，这是 B4 卡没有授权的行为回归。

- **裁定 A：按 `RunningIdentity` 分支**。**判据是「身份 + 忠实六态」的原子守卫联合读**（NH1；`Ok(None)` 单独看**不等于**核已停止，二值 `CoreStatusView.state` 更会把三个分支判反——真值表见 S6）。在跑 → `ApplyPromoted` 承载切换（`Switched`）；已停 → `restart()` 启新核，**与今天逐字同行为**；后端不可用 → 按 §2.2 第 6b 行 degraded。`RuntimeApplyReport.outcome` 用**本仓自有**枚举 `RuntimeApplyOutcome`（镜像 upstream 六变体 + `Started` + `NotApplied`，共八个）。
- **选项 B（未采纳）**：无条件 apply，停止态切核返回 degraded。更简单，但改用户可见行为，且 `Started` 这个真实状态在 wire 上无处表达。

**leader 裁定 A（2026-08-02），四条理由：** ①前提经独立核实——`apply.rs` doc 原文「The core must already be running: apply never starts one」；②选项 B 未经 B4 卡授权就改用户可见行为，违反迁移政策；③`Ok(None)` → `restart()` 与今天逐字同行为，且与 5a updater 的 `Ok(None)` 先例不冲突——两者各自保留各自的 legacy 行为（updater 停止态换二进制**不**启动，change_core 停止态切核**启动**）；④`Started` 第七变体是诚实建模，把 `Ok(None)` 分支映射成 `Switched` 是撒谎（apply 根本没承载它）。

> **R2 —— 为什么用仓内枚举而不是直接复用 `ApplyOutcomeKind`：这是语义选择，不是编译必要。** F25（specta 已在 workspace 启用）说明的是「直接内嵌 upstream 类型**能编译**、无需镜像 DTO」；本条决定的是「**不该**直接内嵌」，理由有两条且都与编译无关：`Started` 在 upstream 枚举里没有对应变体（承载不了 D5=A 的真实终态），以及把我们的 TS wire 与 submodule 的 API 版本解耦。**两条事实并立不矛盾**——F25 保留原样不动。

---

## 5. 实施步骤

> 每步给出编辑内容 → 验证 → 通过判据。**不要**跑 `cargo clippy -- -D warnings`（仓库本就红）。已知坑：共享 target 的 kache 污染会造成本地 clippy 假红，用独立 `--target-dir` 复验再判定。

### S1 — 迁移 runtime lifecycle 类型（按 D1=A）

新建 `backend/tauri/src/core/actor/runtime.rs`，从 `client/runtime.rs` 整体搬入（**逻辑一字不改**，只改可见性与 `use` 路径）：

| 搬入 `core/actor/runtime.rs`                                                                                                          | 留在 `client/runtime.rs`                                                                                                                                                                                                                                                                             |
| ------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `RuntimeRevision`（`:27-33`）、`RuntimeSnapshotData`（`:52-57`）、`RuntimeSnapshot`（`:59-96`）、`RuntimeLifecycleState`（`:98-102`） | **`RuntimeRevisionAllocator`（`:35-50`）**、**`CandidateFile`（`:297-334`）**、`MutationOutcome` / `Degradation` / `DegradationPhase`、candidate 构建函数与 `prepare_private_dir` 等文件工作、`compensation_*`（S5 删）、`RuntimeTransactionSnapshot`（S6 删）、`RuntimeLifecycleStore`（**S2 删**） |

- **`CandidateFile` 不迁（C2 使 L1 的前提消失）**：改用 `PublishPromoted { operation, snapshot }` 后 actor 不再碰任何文件类型，搬迁没有收益。L1 要保护的不变量由 C2 更彻底地满足——详见 §4 D1 的 v3 后记。
- **`RuntimeRevisionAllocator` 不迁（L2）**：留在 client 侧，`use crate::core::actor::runtime::RuntimeRevision`。F15 的「先分配 revision、再读 typed snapshot」顺序因此**原样保留**，也不需要新增 `AllocateRevision` 消息。
- **`RuntimeLifecycleStore` / `new_runtime_lifecycle_store` 的删除挪到 S2（M9）**：S1 若把它删了，替代品（actor 的 `lifecycle` 字段）还不存在，「纯搬迁、零行为」的第一个 commit 就不成立、也无法独立回滚。S1 只搬类型。

**验证：** `cargo check`；`rg 'client::runtime::(RuntimeSnapshot|RuntimeLifecycleState)' backend/tauri/src` 为 0。**此步 `RuntimeLifecycleStore` 仍应存在**（M9），其归零判据在 S2。

### S2 — B1：Promoted / Applied 入 actor

**附录 A.1 声明全部新增字段与消息，此处只述行为。**

- `CoreActorState` 增 `lifecycle: RuntimeLifecycleState`、`lifecycle_tx: watch::Sender<RuntimeLifecycleState>`。**不增 `revisions`**（L2：allocator 留 client 侧）；
- 新增**三条守卫消息** `PublishPromoted` / `ApplyPromoted` / `PublishApplied`（见 A.2，全部校验 active `OperationId`）。**actor 只做 lifecycle 簿记，不碰文件**——这是 C2 的裁定：

  | 谁做什么                                          | 在哪                                                                                                        | 理由                                                                                                                                                                                                            |
  | ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
  | 建 candidate、比对哈希、`check`、原子晋升产物文件 | **client 侧 lease adapter**（沿用今天 `regenerate_runtime_with` 的顺序，`check` 走 5a 已有的 `Check` 消息） | 快照的全部输入都在 client 侧（F32）。**相位区分靠新增的类型化错误 `CheckAndPromoteError`（NH2）**，不是靠 `anyhow`——v3 说「anyhow 天然区分」是错的，今天 seam 只有一个未分化的 `anyhow::Result<[u8;32]>`（F41） |
  | 单调性校验 + 状态提交 + watch 发布                | **actor**（`PublishPromoted`）                                                                              | actor 是 lifecycle 的**单一写者与读模型**，这正是 B1 的实质                                                                                                                                                     |

  > v2 的 `CheckAndPromote { operation, request, candidate, reply -> Arc<RuntimeSnapshot> }` **在类型上就不可实现**：actor 拿不到 revision、`RuntimeSnapshotData`、解析后的 config、target core、产物字节中的任何一个（F32），却承诺返回由它们构成的快照。

  **lease seam 的类型化错误（NH2 + H2 残留）**：`check_and_promote` 的返回类型由 `anyhow::Result<[u8; 32]>` 改为 `Result<[u8; 32], CheckAndPromoteFailure>`（A.1b）。**外层和把两类不同源的失败分开**——`Actor(_)` 原样透出（check 本身就是一次 actor 调用，F51），`Operation(_)` 才是本 seam 自己的两相位失败。相位标签由 **adapter 内的构造位置**打，编排层按臂分流、按 `phase` 查表，**不做字符串嗅探、不 downcast**。两相位分类学不扩大。

- **推进决策与 wire 表示拆分（C3）**：
  - **推进决策**：`core/actor` 内一个**纯谓词** `advances_applied(outcome: ApplyOutcomeKind) -> bool`；`ApplyPromoted` 处理器在提交前调它——这是 **apply 路径唯一的推进点**；
  - **wire 表示**：`runtime_outcome_from_apply_data` 只产 `RuntimeApplyReport` + `Vec<Degradation>`，**不再决定推进**（A.5 的契约相应改述）。两者读同一个 `outcome`，但一个决定 actor 状态、一个决定给前端看什么，不构成双决策；
  - **restart 类路径**：5a 的 `Run` 只更新 backend 观察、不碰 lifecycle（F33b），所以 `start_promoted_runtime` / legacy 重播 / D5 的 `Started` 分支都需要一条显式的 `PublishApplied { operation, snapshot }`（与 `PublishPromoted` 同族），由 client 在 `run` 成功后发送。
- promote 成功推进 `lifecycle.promoted` 并发布 `lifecycle_tx`；apply 成功按 §3 决定是否推进 `lifecycle.applied`；
- **`publish_promoted` 的「拒绝非递增 revision」与 `publish_applied` 的「必须存在 Promoted 且 `identity_eq`」两条校验原样迁入 actor**（F14 的语义不能丢）。**单调性由 actor 兜底**，因此 allocator 留在 client 侧不削弱任何不变量（T-LC-01 直接打 actor）；
- `CoreClient` 增 `lifecycle()`（同步 watch 克隆）。**不加 `LifecycleSnapshot` 消息、也不加 `subscribe_lifecycle()`**（L5）：`lifecycle_tx` 的发布发生在消息处理函数内、早于 reply（F30 已在 `commit()` 上验证同一时序），所以「await 守卫调用的 reply → 读 `lifecycle()`」已经是确定性的读后写；再加一条诊断消息只会多一个需要 `cfg(test)` 门控的面。

client 侧删除：`runtime` 字段、`RuntimeLifecycleStore` / `new_runtime_lifecycle_store` 类型本身（M9：替代品在本步才存在）、`publish_promoted` / `publish_applied` / `restore_promoted` / `runtime_lifecycle_state`，以及 6 处测试构造点（`mod.rs:2256/2327/2612/2928/3983` 等）。**`runtime_revisions` 字段保留**（L2）。`promoted_runtime()` **保留同名同签名**，内部改读 `core_client.lifecycle().promoted`。

**同时接上 §2.4 的 sink（L4）**：`NyanpasuClientInner` 增 `degradation: Arc<dyn CoreDegradationSink>`（值早已在 `ClientSetupArgs` 里，F29），供后台 rebuild worker 投递 degradation 使用。

> **`restore_promoted` 直接删除、不迁入**：它唯一的调用者是 `change_core` 的深回滚（F14），而 B4 删掉整条深回滚路径。

**四条 runtime 读 IPC**（`ipc.rs:346/362/377/390`）改读 `client.promoted_runtime()` 的新实现——facade 方法保留同名同签名，内部改为 `core_client.lifecycle().promoted`，**wire 不变**。

**验证：** `rg 'RuntimeLifecycleStore|publish_promoted|publish_applied|restore_promoted|LifecycleSnapshot' backend/tauri/src` 为 0；四条读 IPC 的 bindings 不变。

### S3 — B2：`CoreOperationGuard` 取代 `rebuild_gate`

删除 `rebuild_gate` 字段与全部 10 处获取（F12）。**顺序约束（关键，M12 已统一为「先分配 revision」——基线 `mod.rs:1601` 就是先分配）**：

```text
begin_operation()  →  分配 revision  →  读 typed snapshots  →  build → check → promote → apply
```

- **7 处「gate → begin」紧邻的站点**：删掉 gate 那一行即可，`begin()` 已经在最前；
- **三处例外必须调整**（H4 纠正了 v2「唯独一处」的错误陈述）：

  | 站点                               | 今天的 gate→begin 间隙                                                                          | 改法                         |
  | ---------------------------------- | ----------------------------------------------------------------------------------------------- | ---------------------------- |
  | `promote_default_runtime_config`   | gate `rebuild.rs:406` → begin `:443`；其间建 snapshot 与 candidate                              | `begin()` 提到原 gate 的位置 |
  | `promote_existing_runtime_product` | gate `mod.rs:1391` → begin `:1423`；其间**分配 revision、读产物文件、建 snapshot 与 candidate** | 同上                         |
  | `start_promoted_runtime`           | gate `mod.rs:1436` → begin `:1440`；其间读 Promoted                                             | 同上                         |

  > `start_promoted_runtime` 的间隙**最危险**：它可以读到旧 Promoted、在 `begin()` 上排队等另一个 rebuild 跑完、然后启动**新**产物却把 Applied 关联到**旧**快照——read-then-act 竞态，产物与读模型直接对不上。三处都必须在 guard 内读快照。

- `restart_core` facade（`mod.rs:491-493`）本就自取 guard，删 gate 后自然一致。

**保留 coalesce（F23）**：`RebuildCoordinator` 一行不改。它的串行化来自 worker 单线程 + capacity-1 通道，与 gate 无关。「构建期间到达的新 commit 触发下一次 rebuild」由 `try_send` + `try_recv` 排空保证。

**验证：** `rg 'rebuild_gate' backend/tauri/src` 为 0；`rebuild.rs` 的 5 个 coordinator 测试（`:469/505/537/573/623`）零改动通过。

### S4 — 统一 apply：`apply_promoted` 改走 backend

`ApplyPromoted`（A.2）内部调 `CoreBackend::apply`（F3）；**推进决策在 actor 内**由纯谓词 `advances_applied(outcome)` 做出（C3），成功推进后发布 `lifecycle_tx`。

`CoreLeaseAdapter::apply_promoted` 改为调 `core.apply_promoted(&guard, &request, expected, snapshot)`（四个实参，与 A.3 的声明一致）；**删除 `apply_config_from`**（F10）。

**`expected` 的取值规则（H8 改写了 v2 的规则）：**

| actor 观察到的 revision | `expected`                 | 理由                                                                                                                                                                                                                                         |
| ----------------------- | -------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 存在                    | `Some(..)`                 | 正常 CAS，防刷新竞态——这是 5a RQ-02 讨论的情形                                                                                                                                                                                               |
| **缺失**                | **`None`**（无条件 apply） | **核处于停止态时天然没有 revision**。upstream 在 CAS 比较**之前**就因 `ctrl.current == None` 返回 `Error::NotStarted`（F36），所以传什么 `expected` 都不影响结果；此时返回 `Err` 反而与 §2.2「停止核给 `core_not_running` degraded」直接矛盾 |

> v2 写「`expected` 为 `None` 即不变量破坏，返回 `Err`」是错的。5a 的 RQ-02 注解讨论的是**刷新竞态**下要不要带 CAS，不是停止核；本条是对它的**细化**，不是推翻——有 revision 时依旧一律 `Some`。backend 返回的 `NotStarted` 按 §2.2 第 **7b** 行映射（A.7 的分类器第 2 顺位）为 `core_not_running` degradation。

按 D3=A **删除 `apply_candidate`**。

#### S4 的重试判据（L6：预先定死，不留「实测再说」）

旧路径 `apply_config_from` 是 5 次 × 250 ms 重试（F10）；新路径由 runtime 的 `apply_config` 承担 reconcile。**决策规则如下，实施时按规则走，不再开放讨论：**

| 新路径把该类失败暴露为                                             | 动作                                                                               |
| ------------------------------------------------------------------ | ---------------------------------------------------------------------------------- |
| **传输类**硬失败（连接被拒 / 连接重置 / socket 未就绪 / 读写超时） | 在 **`CoreLeaseAdapter` 层**对**仅传输类错误**补有界重试，沿用 **5 × 250 ms** 形状 |
| `check` 失败、语义失败（revision 冲突、配置被拒）、`RolledBack`    | **一律不重试**——重试对它们没有意义，只会把一次失败放大成 5 次副作用                |
| runtime 内部已自带重试 / reconcile 且传输类失败不外泄              | **不补**，保持单次调用                                                             |

判定方法：实施时读 `CoreBackendError` 的传输类变体（Service 侧 `ClientError`、Local 侧 manager 的 IO 错误）在新路径下是否会原样冒出到 `apply_promoted` 的返回值。**实测数据（哪类错误、是否外泄、重试是否生效）写进实施报告**，但**加不加重试由上表决定，不由实测口味决定**。

**验证：** `rg 'put_configs' backend/tauri/src` 只剩 `feat.rs:79`（`change_clash_mode`，PR-6 范围）；`rg 'apply_config_from|apply_candidate' backend/tauri/src` 为 0。

### S5 — B3：删除 API-first patch 与补偿层

`patch_running_config`（F17）重写为：

```text
guard → typed desired commit（ClashConfigClient::patch）→ 统一 rebuild → apply → 按 §3 映射
```

删除：`clash_patch` 字段与 `ClientSetupArgs.clash_patch`、`RunningConfigPatchPort`、`LegacyRunningConfigPatchBridge`、`clash_patch_gate`、`restore_applied_after_patch_failure`（F18）、`client/runtime.rs:123-190` 的 `PatchCompensationPlan` / `PatchCompensationOp` / `compensation_for`、以及 `mod.rs:1501` 的裸写。

**保留** `feat::patch_clash_with_rebuild`（`feat.rs:254-336`）：它的 mixed-port / external-controller 检查与 sysproxy / systray 后效属于 PR-6e，本阶段只把它的 rebuild 闭包接到新管线上。

> **B3 卡的两项对当前代码是 no-op**：`ControllerBinding` 与 `ConfigPatch mapper`（`config_patch_from_mapping`）**在代码库中不存在**（F20）。计划照实记录，**不要**为了「删掉它们」而先造出来。

`patch_clash_config` 命令返回类型改为 `MutationOutcome<RuntimeApplyReport>`（与 B4 统一）。

**验证：** `rg 'RunningConfigPatchPort|LegacyRunningConfigPatchBridge|clash_patch_gate|compensation_for|PatchCompensationPlan' backend/tauri/src` 为 0。

### S6 — B4：`change_core` 降级为普通 commit-first mutation

新形态（对比 F19 的三分支回滚）：

```text
guard → ApplicationClient::patch(core = new)   ← desired 提交；此后除 §2.3 的 E-a/E-b 外一律 degraded
      → 统一 rebuild（新核）→ check → 晋升产物 → PublishPromoted   （C2：文件工作在 client）
      → RunningIdentity → (身份, FaithfulLifecycle) 原子守卫联合读        （NH1）
          在跑        → ApplyPromoted          ← apply 内部承载切核（Switched）
                         actor 内 advances_applied() 决定推进（C3）
          已停        → restart()              ← 核本来就停着：启新核，与今天同行为
                         Ok  → PublishApplied，outcome=Started      （C3）
                         Err → CommittedDegraded(CoreLifecycle /
                                 core_start_failed)，Applied 不推进  （R1）
          无后端      → §2.2 第 6b 行 degraded（core_backend_unavailable，NH8）
        注：只有「在跑」这条走 runtime_outcome_from_apply_data()（C3：wire 表示的唯一决策点）；
            「已停」两侧与各类失败由 S7 直接构造 report（Started / NotApplied）。
```

**删除**：legacy `Config::verge().draft()/apply()/discard()`、回滚重建、`restore_product` 调用、回滚分支里的两次 old-core restart、`RuntimeTransactionSnapshot`（连类型一起删——change_core 是它唯一的使用者）。

**`RolledBack` 时的终态**（B4 卡与 Exit 判据）：desired = 新核，Promoted = 新配置，Applied = 旧值。**不做**第二套应用层回滚。

#### 「核在跑吗」怎么判（H5：`Ok(None)` ≠ 已停止）

`state.running` 是**身份缓存**，恒初始化为 `None`，只有 GUI 发过 `Run` 才填充；而 `pre_start` 会 `observe_status()`，所以进程启动后 attach 到一个**已在运行**的 Service 核完全可能（F33）。此时联合读的身份分量是 `None` 但生命周期分量是 `Running`——**核正在跑**——照 v2 的写法会对一个运行中的核走 `restart()`，把用户的连接全部打断。

**判据必须读忠实六态，不能读 `CoreStatusView.state`（NH1）**：后者是二值投影，`Starting`/`Restarting` 被压成 `Stopped`、`Stopping` 被压成 `Running`（F40）——直接用它会把下表**三个分支判反**。这与 5a 规划期裁过的 wire 塌缩是同一类问题，只是换了场景。

**裁定：`RunningIdentity` 的 reply 扩为一次原子守卫读**，返回 `(Option<CoreRequest>, FaithfulLifecycle)`——身份与忠实生命周期在**同一个 mailbox 轮次内**取齐，杜绝两次查询之间的撕裂读（A.2 已改声明）。真值表从这个联合值取：

| 身份      | `FaithfulLifecycle`                                 | 分支        | 理由                                                                                   |
| --------- | --------------------------------------------------- | ----------- | -------------------------------------------------------------------------------------- |
| `Some(_)` | 任意                                                | **apply**   | 本进程启的核，身份已知                                                                 |
| `None`    | `Running` / `Starting` / `Restarting` / `Switching` | **apply**   | 核在跑或正在起来——apply 能承载切换，不该重启                                           |
| `None`    | `Stopped { .. }`                                    | **restart** | 真的停着，D5=A 的启核分支                                                              |
| `None`    | `Stopping`                                          | **restart** | 正在停——等它停稳后启新核；**注意二值投影会把它读成 `Running`**，这正是必须读六态的原因 |
| —         | 后端不可用（`Err(NoBackend)`）                      | degraded    | §2.2 第 **6b** 行（`core_backend_unavailable`，NH8）                                   |

> **先取 guard 再读**，所以联合读到的值不会被并发操作改写。attach 场景与三个过渡态由 **T-B4-06** 钉住（`Starting` / `Restarting` / `Stopping` 各至少一例断言分支方向——这三例正是二值投影会判反的那三个）。

#### `restart()` 残余审计（L3）与 F8 的重新表述

v1 说「事务内 `restart()` 恰好一次」，**那是错的**：新流程的切核由 `CoreBackend::apply` 内部承载（F27 的 upstream doc 明写「`core_type` 与运行核不同即为切换」，产出 `Switched`），`restart()` 在核**在跑**的那条主路径上根本不出现。审计结果（F26）：

| `restart()` 调用点                                          | B3/B4 之后         | 归属                                                 |
| ----------------------------------------------------------- | ------------------ | ---------------------------------------------------- |
| `change_core` 主路径 + 两处回滚（`rebuild.rs:315/333/377`） | **删除**           | B4                                                   |
| `patch_running_config` 的 restart 分支（`mod.rs:1540`）     | **删除**           | B3——restart-vs-apply 改由 `apply` 自己分类           |
| `start_promoted_runtime`（`mod.rs:1441`）                   | **保留**           | 启动路径；apply 不启核（F27）                        |
| `regenerate_and_restart_for_legacy`（`rebuild.rs:271`）     | **保留**           | legacy replay（`feat.rs:244/369/392`），PR-6/7a 范围 |
| `change_core` 的**停止态**分支（D5=A 新增）                 | **保留，条件触发** | B4                                                   |

**结论：`restart()` 归零不了**，方法与 `target_core` 机制都必须留着（D3 只删 `apply_candidate`）。

**F8 的重新表述**：`restart()` 的 `target_core.take()` 是一次性消费，其目的是「回滚深路径不得重启失败的新核」。B4 删掉整条深回滚路径后，**这个防护对象消失了**——`take()` 从「防回滚误重启」退化为「防同一 lease 内第二次 restart 误用陈旧目标」。因此 T-B4-03 改钉真实语义（见 §6.3）：**同一 lease 内 `check_and_promote` 设置的 `target_core` 只能被消费一次；若停止态分支触发，`restart()` 使用的正是本次 promote 的新核**。

**验证（一）：** `rg 'lease\.restart\(\)|\.restart\(\)\.await' backend/tauri/src/client/rebuild.rs` 只剩 `regenerate_and_restart_for_legacy` 与 change_core 的停止态分支两处。

#### `change_clash_core` 去掉 `run_legacy_verge_mutation` 包裹（H7）

v2 写「外层 `run_legacy_verge_mutation` 的 typed 重播保留（PR-7a 才删）」，**那是在追溯它的存在理由之前写的，现予纠正**。

包裹器在这个调用点的理由由代码注释写死：「核心切换动了 legacy verge,须回灌 typed actors」（`ipc.rs:489`）。**B4 恰恰删掉那次 legacy verge 写入**（`Config::verge().draft().clash_core = ...`），前提随之消失：

- 前提没了：`legacy_patch_between(previous, desired)` 得到空 patch，整趟重播是 no-op；
- 类型装不下：包裹器签名是 `Fut: Future<Output = anyhow::Result<()>>` → `ClientResult<()>`（F35），承不住 `MutationOutcome<RuntimeApplyReport>`；
- **还会违 I-A**：它在 `mutate()` 成功后仍要做 legacy restore / 投影刷新 / typed patch plan / finalize，这些**发生在 typed commit 之后**却一律返回普通 `Err`——用户会看到「切核失败」而磁盘上核已经换了。

**做法**：`change_clash_core` 命令直接调 `client.change_core(core)`，返回 `MutationOutcome<RuntimeApplyReport>`；`LegacyVergeBridge` 参数从该命令签名移除。**其余 7 处 `run_legacy_verge_mutation` 调用点一律不动**（今天共 8 处，含 `change_clash_core` 自身）（它们确实还在写 legacy verge），包裹器本身也不改签名——不为一个即将退场的调用点去泛化一个即将删除的 compat 包裹器。

**「摘掉之后，残余的 legacy 读者谁来喂？」——F38 / F39 预先钉死这条**（leader 独立核验，v3.1 记入）：

- **喂食者是 state actor 的提交路径，不是包裹器**：`ApplicationActor::commit` 在每次 typed 提交内部 `prepare` → 持久化 → `mirror.apply()`（`state/application.rs:75-87`；条件替换 `:101-103` 同样），而 `Patch` 消息正汇入这个 `commit`（`:140-150`）。B4 的 `ApplicationClient::patch(core = new)` 因此**自动刷新镜像**；
- **投影确实含 `clash_core`**：`apply_app_config_to_legacy_verge` 的 `draft.clash_core = Some(yaml_convert(snap.core)?)`（`bridge/verge.rs:678`）；
- **四个残余读者读到的都是新鲜值**（F39）：`core/tray/mod.rs:167,336`、`feat.rs:379`、`core/clash/core.rs:98`；
- **镜像不可失败**：`fn apply(self: Box<Self>)` 返回 unit（`bridge/verge.rs:149-152`），所以摘掉包裹器**不新增任何 degradation 路径**；
- **`verge_update_lock` 的互斥域退出是正确的**：B4 后 change_core 不再触碰 legacy draft，留在锁内反而是无谓的跨域串行。

> 换句话说：包裹器负责的是 **legacy → typed 的回灌**，而残余读者依赖的是 **typed → legacy 的镜像**——**两个方向**。B4 删掉的是前者的输入源，后者一直由 state actor 独立维护，不受影响。

> 这是 CLAUDE.md §11「优先可迁移的破坏性变更，而不是加兼容层」的直接应用：前提消失的兼容层就该在该阶段摘掉，而不是留到 PR-7a 陪葬。**leader 已于 2026-08-02 裁定摘除**（独立核验过上述镜像链后确认），不改为泛化包裹器——本条不再是待决问题。

命令 `patch_clash_config` **本来就没有**走包裹器（`ipc.rs:435-458`），不受影响。

**验证（二）：** `rg 'restore_promoted|RuntimeTransactionSnapshot' backend/tauri/src` 为 0。

### S7 — degradation 映射与 `RuntimeApplyReport`

新增 `RuntimeApplyReport`（D4=A 的三字段，`serde` + `specta`；F25 确认 workspace 已启用 specta，upstream 类型可直接内嵌）与 `RuntimeApplyOutcome`（D5=A），放在 `client/runtime.rs` 与 `MutationOutcome` 同层。

新增纯函数 `runtime_outcome_from_apply_data(data: &CoreApplyData, promoted_revision: u64) -> (RuntimeApplyReport, Vec<Degradation>)`。**它的契约按 C3 改述：它是「wire 表示」的唯一决策点，不再是「Applied 是否推进」的决策点**——后者由 actor 内的纯谓词 `advances_applied(outcome)` 独占（S2 / S4）。两者读同一个 `outcome`：一个决定 actor 状态、一个决定给前端看什么，职责不重叠。§3 的 12 格 parity 矩阵仍全部由 `runtime_outcome_from_apply_data` 决定，测试直接打它。

**三条不经过它的路径**（都没有 `CoreApplyData`，由 S5/S6 直接构造报告）：

| 路径                                                    | report                                                   | degradations                          |
| ------------------------------------------------------- | -------------------------------------------------------- | ------------------------------------- |
| 停止态启核成功（D5=A / R1）                             | `outcome = Started`、`applied_revision = Some(promoted)` | 空                                    |
| 停止态启核失败（R1）                                    | `outcome = NotApplied`、`applied_revision = None`        | `CoreLifecycle` / `core_start_failed` |
| build / check / promote / 传输 / `NoBackend` 失败（C1） | `outcome = NotApplied`、`applied_revision = None`        | §2.2 对应行                           |

> 三行的 `desired_revision` 都填本次分配的 revision——它确实分配了，这不是猜测。

**同时收敛 §2.4 的两条投递路径：**

- 删掉 `map_runtime_rebuild_degradation`（`mod.rs:981`）与它那条「错误面撑不住精度」的 doc——5b 之后 rebuild 失败带着 §2.2 的 phase/code 出来，`collect_post_commit_degradations` 直接并入即可；
- 后台 worker 闭包（`mod.rs:435-449` 处注册的那条）改为：拿到 degradations 后逐条 `inner.degradation.publish(..)`，`tracing::warn!` 保留。

**验证：** T-AP-01…12 全绿；T-PC-09 证明后台失败到达 sink；T-PC-10/11 证明无-commit 入口仍是普通 `Err`。

### S8 — 测试适配

按 §6 的清单逐个改。原则：**只改构造与断言目标，不改测试语义**；coordinator 的 5 个测试与 5a 的 16 个 `client/core.rs` 测试**期望零改动**（若被迫改动，说明 B2/B3 溢出了范围，停下核查）。

### S9 — 门禁

```powershell
pnpm fmt:backend
pnpm lint:rustfmt
pnpm lint:clippy
cargo build --manifest-path .\backend\Cargo.toml -p fake-core --bin manager-probe-core
pnpm test:backend
git diff frontend/interface/src/ipc/bindings.ts
pnpm lint:ts
pnpm architecture-ledger
pnpm lint:architecture-ledger
```

**bindings 预期差异（恰好这些）**：新增 `RuntimeApplyReport` 与 `RuntimeApplyOutcome` 两个 TS 类型（D5=A：第二个是**本仓自有**枚举，不是 upstream 的 `ApplyOutcomeKind`）；`changeClashCore` 与 `patchClashConfig` 的返回类型由 `null` 变为 `MutationOutcome<RuntimeApplyReport>`。**其余零变化**——四条 runtime 读 IPC、`getCoreStatus` 均不得变。 `changeClashCore` 命令**去掉了 `LegacyVergeBridge` 参数**（H7），但 tauri `State<'_, _>` 参数本就不进 bindings，**因此该改动的 bindings 差异为零**——若 diff 里出现它，说明改错了地方。

**ledger 预期**：`config_calls` 应**下降**（B4 删掉 `change_core` 的 legacy draft/apply/discard 三处 `Config::verge()`）；`migration_markers` 应下降（`rebuild.rs` 的 core-selection 与 log-sink 两条 TODO 随 B4 删除）；`test_real_dirs` **必须仍为 0**。逐项核对后再 `--write-snapshot`。

---

## 6. 测试矩阵

### 6.1 apply parity（RQ-03；§3 的 12 格）

| ID         | 组合                                        | 断言                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ---------- | ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-AP-01    | `Noop` 无 warning                           | Applied **推进**至 Promoted；`Applied`；degradations 空                                                                                                                                                                                                                                                                                                                                                                                                                   |
| T-AP-02    | `Noop` + warning                            | Applied 推进；`CommittedDegraded`；恰 1 条 `RuntimeApply` durability                                                                                                                                                                                                                                                                                                                                                                                                      |
| T-AP-03/04 | `Patched` 无/有 warning                     | 同上形态                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| T-AP-05/06 | `Reloaded` 无/有 warning                    | 同上形态                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| T-AP-07/08 | `Restarted` 无/有 warning                   | 同上形态                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| T-AP-09/10 | `Switched` 无/有 warning                    | 同上形态                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| T-AP-11    | `RolledBack` 无 warning                     | Applied **不推进**；`CommittedDegraded`；恰 1 条 `CoreRollback`；**并断言 report 值**：`value.outcome == RolledBack`（**不是 `NotApplied`**——§2.2 行 7a 的例外）、`value.desired_revision` = 本次分配的 revision、`value.applied_revision` = 旧值                                                                                                                                                                                                                         |
| T-AP-12    | `RolledBack` + warning                      | Applied 不推进；**2 条** degradation（`CoreRollback` + durability）；report 值断言同 T-AP-11                                                                                                                                                                                                                                                                                                                                                                              |
| T-AP-13    | Local / Service 双后端对同一 outcome 同映射 | **不比共用的 mapper**（M10）——parity 的价值全在各后端的转换层。Local 侧**直接在模块内单测既有的 `core::actor::backend::map_apply_outcome`**（喂 manager `ApplyOutcome` fixtures → `CoreApplyData`；`:975`/`:986` 已有两个测试可扩），Service 侧走 IPC harness 解码（套 `transport_available()`，F24）；两侧 `CoreApplyData` 逐字段相等。**不需要 `TestBackend` 前置**——M10 由此彻底关闭。注意它与新增的 `runtime_outcome_from_apply_data` 是**两个不同函数**（NH9 / F44） |

### 6.2 post-commit 失败矩阵（RQ-01；§2.2 的七项）

> **每一行都必须同时断言 report 值（C1）**：v2 的 T-PC-02…07 与 T-B4-05 只断言外层变体与 degradation、从不看 `value`——正是这个空转掩盖了「report 没有诚实取值」的洞。第 2–7 行一律断言 `outcome == NotApplied` 且 `applied_revision == None`，`desired_revision` 等于本次分配的 revision。

| ID       | 失败注入                                           | 入口                                           | 断言                                                                                                                                                                                                                                                                                                                                                                   |
| -------- | -------------------------------------------------- | ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-PC-01  | acquire 超时                                       | `change_core`                                  | `Err`；**desired 未提交**（typed 快照未变）                                                                                                                                                                                                                                                                                                                            |
| T-PC-02  | build 失败                                         | `change_core`                                  | `CommittedDegraded`；`RuntimeBuild`；desired = 新核                                                                                                                                                                                                                                                                                                                    |
| T-PC-03  | check 失败                                         | `change_core`                                  | `CommittedDegraded`；`RuntimeCheck`；Promoted 未推进                                                                                                                                                                                                                                                                                                                   |
| T-PC-04a | promote **前置**失败（candidate 哈希不符）         | `patch_running_config`                         | `CommittedDegraded`；`RuntimePromote` / `runtime_promote_failed`；**产物保持旧值**；report = `NotApplied`                                                                                                                                                                                                                                                              |
| T-PC-04b | promote **写后校验**失败（产物已写、读回哈希不符） | `patch_running_config`                         | `CommittedDegraded`；同上 phase/code（message 区分）；**产物 = 新、Promoted = 旧**（分裂窗口）；report = `NotApplied`；**并断言下一次 rebuild 使两者重新收敛**——这是「自愈」论证的实证，没有它 §2.2 的 4b 就只是一句主张                                                                                                                                               |
| T-PC-05  | revision 冲突                                      | `patch_running_config`                         | `CommittedDegraded`；`RuntimeApply` / `revision_conflict`；Applied 不变                                                                                                                                                                                                                                                                                                |
| T-PC-06  | IPC 传输丢失                                       | `change_core`                                  | `CommittedDegraded`；`core_transport_lost`；Applied 不变。**实现方式（L7）：优先用 `TestBackend` 脚本化传输错误**——不需要真 harness、也就不需要 `transport_available()` 守卫；只有在必须验证真实 `ClientError` 映射时才起真 IPC harness，那时套守卫（F24）                                                                                                             |
| T-PC-07  | apply error（非 RolledBack）                       | `patch_running_config`                         | `CommittedDegraded`；`runtime_apply_failed`；Applied 不变                                                                                                                                                                                                                                                                                                              |
| T-PC-08  | 启动路径同类失败                                   | `promote_default_runtime_config`               | **`Err`**（无 desired commit）——证明分界线按入口区分                                                                                                                                                                                                                                                                                                                   |
| T-PC-09  | 后台 rebuild 失败                                  | `RebuildCoordinator` worker                    | **degradation 到达 `CoreDegradationSink`**（用 `RecordingSink`，`client/core.rs:529` 已有），而不是只进日志（§2.4 的 I-B 缺口）；同一失败经**同步** caller 时则出现在 `MutationOutcome` 里                                                                                                                                                                             |
| T-PC-10  | build / check / apply 失败                         | **`enhance_profiles`**（F34 ③）                | **普通 `Err`**，**零 degradation**、**不进 sink**——命令 doc 明写无前置 commit（§2.4 ④）                                                                                                                                                                                                                                                                                |
| T-PC-11  | apply 返回 `RolledBack`                            | **`regenerate_and_apply_for_legacy`**（F34 ④） | **普通 `Err`**——否则 legacy draft 会被误当成功而 `apply()`，在磁盘留下一份从未生效的配置（§2.4 ④ 的关键差异）                                                                                                                                                                                                                                                          |
| T-PC-12  | **豁免边界分类**（NH4 / NH6）                      | 任意 commit-first 入口                         | 四例分类断言：①`ShuttingDown` → `Err`、**零 degradation**；②`LifecycleInvariant(PromotedRegression)` → `Err` + 错误级日志、**零 degradation**；③**`Backend(_)` → `CommittedDegraded`**；④**`NoBackend` → `CommittedDegraded`（`core_backend_unavailable`）**。**③④ 是关键**——它们证明豁免**吞不掉真实运行时失败**；没有 T-PC-12，§2.3 的 `matches!` 规则就只是纸面上的 |

### 6.3 B1/B2/B4 结构测试

| ID      | 断言                                                                                                                                                                                                                                                                                                                                                                          |
| ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T-LC-01 | Promoted 推进拒绝非递增 revision（迁自 F14 的 `publish_promoted` 校验）                                                                                                                                                                                                                                                                                                       |
| T-LC-02 | Applied 推进要求存在 Promoted 且 `identity_eq`（迁自 `publish_applied` 校验）                                                                                                                                                                                                                                                                                                 |
| T-LC-03 | `lifecycle()` 是同步 watch 克隆，慢 `Run` 阻塞期间**立即返回**（沿用 5a 的活性性质）                                                                                                                                                                                                                                                                                          |
| T-B2-01 | **两个并发 rebuild 不重叠**，后一个在 `OperationGate` FIFO 之后**读取最新 snapshot**（Exit 判据原文）                                                                                                                                                                                                                                                                         |
| T-B2-02 | **三处** gate→begin 例外都在 guard 内构建快照 / candidate（H4 纠正了 F12）：`promote_default_runtime_config`、`promote_existing_runtime_product`、`start_promoted_runtime`。三例**分别断言不合并**——`start_promoted_runtime` 那例必须构造「读到旧 Promoted → 在 begin 上排队 → 启动新产物」的竞态窗口并证明它已关闭                                                           |
| T-B4-01 | change-core `RolledBack`：desired = **新核**、Promoted = **新配置**、Applied = **旧值**（Exit 判据原文）                                                                                                                                                                                                                                                                      |
| T-B4-02 | change-core 成功（**核在跑**）：三者一致推进；`RuntimeApplyReport.outcome == Switched`；**全程零次 `restart()`**——切核由 `apply` 承载（L3 的真实语义）                                                                                                                                                                                                                        |
| T-B4-03 | change-core（**核已停**，D5=A 分支）：`RunningIdentity` 返回 `Ok((None, FaithfulLifecycle::Stopped { .. }))` → 恰好一次 `restart()`，且该次 Run 请求用的是**本次 promote 的新核**（`target_core.take()` 消费的正是它，F8 重述）；`outcome == Started`                                                                                                                         |
| T-B4-04 | `restart()` 的 `target_core` 一次性消费：同一 lease 内第二次 `restart()` 落回 typed 快照而非重用陈旧目标（把 F8 的机制本身钉住，与 change_core 流程解耦）                                                                                                                                                                                                                     |
| T-B4-05 | change-core 停止态分支的**失败侧**（R1）：`restart()` 失败 → `CommittedDegraded`，`phase = CoreLifecycle` / `code = "core_start_failed"`；desired = 新核（已提交）、Promoted = 新配置、**Applied 不推进**。**并断言 report 值**（C1 残留）：`value.outcome == NotApplied`、`value.applied_revision == None`、`value.desired_revision == ` 本次分配的 revision                 |
| T-B4-06 | **attach 场景 + 三个过渡态**（H5 / NH1）：(a) `pre_start` 观察到 `Running` 但 `state.running == None` → change_core **必须走 apply**（`outcome == Switched`）、**零次 `restart()`**；(b) `Starting`、(c) `Restarting`、(d) `Stopping` 各一例，断言分支方向。**(b)(c)(d) 正是二值 `CoreStatusView.state` 会判反的三个**（F40），所以它们同时也是「判据确实读了忠实六态」的证明 |

### 6.4 回归（期望零改动通过）

- `rebuild.rs` coordinator 五连：`:469 / 505 / 537 / 573 / 623`（B2 的 coalesce 不变式）；
- `client/core.rs` 的 16 个 5a 测试（`:671`…`:1182`）——**若被迫修改，说明范围溢出**；唯一预期的例外见下一条；
- **`rollback_build_failure_restarts_the_committed_old_core`（`rebuild.rs:1276`，`9727ef1d4` 为 PR-5a Finding 1 新增）**：它脚本化的正是 change_core 的**回滚重建失败**分支，而 B4 把整条深回滚路径删掉。该测试**必须删除**，其保护的不变量由 **T-B4-04** 接手（直接钉 `target_core` 的一次性消费机制，不再依赖已消失的回滚流程）。实施时在 commit body 里写明这次接管，否则会被读成「删了个回归测试」；
- **B4 删除深回滚后必然失效的另外 6 条**（M11 补齐，逐条写明理由）：

  | 测试                                                                   | 位置                          | 为什么失效 / 谁接管                                                                                      |
  | ---------------------------------------------------------------------- | ----------------------------- | -------------------------------------------------------------------------------------------------------- |
  | `change_core_rolls_back_via_second_regenerate_and_restart`             | `rebuild.rs:710`              | 断言的「二次重建 + 重启」在 B4 后不存在。**删除**                                                        |
  | `change_core_rollback_rebuild_failure_restores_product_and_errors`     | `rebuild.rs:756`              | 断言 `restore_product` + 返回 `Err`；B4 改为 `CommittedDegraded` 且不再回滚产物。语义由 **T-B4-01** 接管 |
  | `s01_contract_product_restore_leaves_runtime_read_model_on_new_core`   | `rebuild.rs:1014`             | 产物恢复路径整体删除。读模型终态由 **T-B4-01** 接管                                                      |
  | `change_core_product_restore_advances_applied_when_promoted_ahead`     | `rebuild.rs:1103`             | 同上；「Promoted 领先于 Applied」的不变量由 **T-LC-02** 接管                                             |
  | `change_core_successful_rollback_publishes_applied_from_old_core`      | `rebuild.rs:1227`             | 回滚后 publish_applied 的路径不存在。**删除**                                                            |
  | `s09_process_change_core_new_start_exit_rollback_old_restart_succeeds` | `process_core_bridge.rs:1205` | 进程级回滚重启场景消失。**删除**；进程级切核成功路径由 **T-B4-02** 覆盖                                  |

  > 连同上一条的 `rollback_build_failure_restarts_the_committed_old_core`，B4 共删 **7** 条回滚测试。**每一条都要在 commit body 里写明「删除原因 + 接管者」**——一次性删掉 7 条测试而不交代，任何审查者都会（合理地）怀疑是在掩盖回归。

- `s04_concurrent_restart_waits_until_change_core_rollback_completes`（`rebuild.rs:931`）：B4 删掉回滚分支后该测试**必须重写**为「并发 restart 在 change_core 事务后串行执行」，语义（互斥）保持。

---

## 7. Exit 判据映射

| task.md B-Exit                                                                                     | 交付步骤 | 验证                              |
| -------------------------------------------------------------------------------------------------- | -------- | --------------------------------- |
| `rg 'rebuild_gate\|clash_patch_gate\|RunningConfigPatchPort\|LegacyRunningConfigPatchBridge'` 为 0 | S3、S5   | 该 `rg` 命令输出为空              |
| apply parity：Noop/Patched/Reloaded/Restarted/Switched/RolledBack；Warning 正交                    | S4、S7   | T-AP-01…13 全绿（12 格 + 双后端） |
| change-core rollback 断言 desired=new、Promoted=new、Applied=old                                   | S6       | T-B4-01                           |
| 两个并发 rebuild 不重叠，后一个读最新 snapshot                                                     | S3       | T-B2-01                           |
| RQ-01 已作答（含 §2.4 的**四类**投递上下文与 §2.3 的两条豁免）                                     | §2       | T-PC-01…12                        |
| RQ-03 已作答（含 R1 的 `Started` 与 C1 的 `NotApplied` 两个非 apply 终态）                         | §3       | T-AP-01…13 + T-B4-03 / T-B4-05    |

---

## 8. 风险与回滚

| 风险                                                      | 概率 | 影响                    | 缓解                                                                                                                                                                                          |
| --------------------------------------------------------- | ---- | ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Promoted/Applied 迁入 actor 后四条读 IPC 行为漂移         | 中   | 前端读到空/陈旧 runtime | facade 方法保持同名同签名，内部改实现；四条 IPC 的 bindings 必须零变化（S9 判据）                                                                                                             |
| 删 `rebuild_gate` 后 coalesce 语义被牵连                  | 中   | 重复/丢失 rebuild       | `RebuildCoordinator` 一行不改；五个 coordinator 测试零改动通过（T-6.4）                                                                                                                       |
| `apply_promoted` 改道后重试语义丢失                       | 中   | 瞬时失败变成硬失败      | 旧路径是 5 次 250 ms 重试（F10）。**判据已在 S4 预先定死**：仅传输类错误在 `CoreLeaseAdapter` 层补 5 × 250 ms，check / 语义失败 / `RolledBack` 一律不重试；实测数据入实施报告，但**不改判据** |
| `change_core` 在停止态下的行为回归                        | 中   | 切核后核不再自动启动    | D5=A 的 `RunningIdentity` 分支保留今天的启核行为；成功侧 T-B4-03、失败侧 T-B4-05 钉住                                                                                                         |
| `RolledBack` 被误判为成功                                 | 中   | Applied 错误推进        | §3 的映射集中在 `runtime_outcome_from_apply_data` 一个纯函数里；T-AP-11/12 直接打它                                                                                                           |
| `target_core.take()` 在多次 restart 下失效                | 低   | 启核用错目标            | T-B4-04 直接钉机制本身（不再依赖 B4 已删除的回滚流程）；T-B4-03 钉停止态分支只消费一次                                                                                                        |
| B3 删除面过大牵连 profile mutation 的 `CommittedDegraded` | 中   | 既有 degraded 路径回归  | `MutationOutcome::from_parts` 是唯一产出点（F22），不动它；profile 侧测试零改动                                                                                                               |
| Service parity 测试在 CI 某平台不可用                     | 中   | parity 静默消失         | 沿用 5a 规则：`transport_available()` 守卫 + 支持平台上必须常规运行（F24）                                                                                                                    |

**回滚：** 改动集中在 `core/actor/{mod,backend,runtime,types}.rs`、`client/{mod,core,rebuild,runtime}.rs`、`ipc.rs`、`setup.rs`、`bridge/verge.rs`。S1–S2 可独立成一个 commit（类型迁移 + actor 增字段，生产路径未变），单独回滚不影响行为。

---

## 9. 提交切分建议

1. `refactor(core): move runtime lifecycle types into the actor module` —— S1（纯搬迁，零行为）；
2. `feat(core): own promoted and applied state in CoreActor` —— S2 + T-LC；
3. `refactor(client): replace rebuild_gate with the core operation guard` —— S3 + T-B2；
4. `feat(core): route promoted apply through the core backend` —— S4 + S7 + T-AP；
5. `refactor(client): delete the api-first patch and compensation layer` —— S5 + T-PC-01…08、T-PC-10/11、**T-PC-12**（豁免边界分类测试随 §2.3 的规则一起落地）；
6. `feat(client): publish background rebuild degradations to the core sink` —— §2.4 的两项动作 + T-PC-09；
7. `refactor(client): make change_core a commit-first mutation` —— S6 + T-B4 + S8 + S9。

第 2 步与第 3 步**必须分开**：前者改所有权，后者改并发原语；混在一起的 diff 无法判断回归来自哪一侧。第 6 步单独成 commit：它是 §2.4 认定的既有 I-B 缺口修复，与 B1–B4 的删除面无关，混进去会让「本来就漏、还是这次改漏」分不清。

---

## 10. 明确 out-of-scope（登记去向）

| 项                                                                                                                        | 去向                                                 |
| ------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| watch snapshot 的状态投影扩展、100 条 `LogFrame` ring                                                                     | **PR-5c / C1**                                       |
| `set_mode` / `reconcile_mode`、删 5 s 轮询与 statics                                                                      | **PR-5c / C2**                                       |
| macOS DNS 归入 actor                                                                                                      | **PR-5c / C3**                                       |
| `feat::patch_clash_with_rebuild` 的 sysproxy/systray/locale 后效                                                          | **PR-6e**                                            |
| `on_profile_change` 的连接中断服务                                                                                        | **PR-6**                                             |
| `UpdaterManager::global()`                                                                                                | **PR-6d**                                            |
| `run_legacy_verge_mutation` 的 typed 重播（**其余 7 处调用点**；今天共 8 处，`change_clash_core` 那处 H7 已在本阶段摘除） | **PR-7a**                                            |
| `feat.rs:79` 的 `change_clash_mode` 裸 `put_configs`                                                                      | **PR-6**（不在本阶段的 apply 管线内）                |
| `ControllerBinding` / `config_patch_from_mapping`                                                                         | **不存在**（F20）；B3 卡该两项记为 no-op，不新造再删 |

---

## 11. 附录 A — 接线单点声明（normative；其它小节只引用，不复述）

> 沿用 5a 的反漂移机制：本附录是 5b 全部新增/变更类型与消息的**唯一声明处**。

### A.1 actor 侧新增

```rust
// core/actor/runtime.rs（S1 从 client/runtime.rs 迁入，逻辑不改）
pub(crate) struct RuntimeRevision(u64);
pub(crate) struct RuntimeSnapshotData { /* 字段与迁移前逐字相同 */ }
pub(crate) struct RuntimeSnapshot { /* 字段与迁移前逐字相同 */ }
pub(crate) struct RuntimeLifecycleState {
    pub(crate) promoted: Option<Arc<RuntimeSnapshot>>,
    pub(crate) applied: Option<Arc<RuntimeSnapshot>>,
}
// CandidateFile **不迁**（C2 使 L1 前提消失）：actor 不再碰文件类型，它留在 client/runtime.rs。

// 注意（L2）：RuntimeRevisionAllocator **留在 client/runtime.rs**，
// 它 `use crate::core::actor::runtime::RuntimeRevision`。actor 不持有 allocator。

// core/actor/types.rs —— 扩 5a 的 CoreActorError（NH4；加法式，5a 四变体不动）
pub(crate) enum LifecycleInvariantKind {
    /// PublishPromoted 的 revision 未严格递增
    PromotedRegression,
    /// PublishApplied 找不到匹配的 Promoted，或身份不符
    AppliedWithoutPromoted,
}
// CoreActorError 新增第五变体：
//   #[error("core lifecycle invariant violated: {0:?}")]
//   LifecycleInvariant(LifecycleInvariantKind),
// I-A 豁免 (b) 的判据即 matches!(err, StaleOperation | LifecycleInvariant(_))——§2.3 单点声明。

// core/actor/mod.rs —— CoreActorState 新增字段（其余字段见 5a 现状）
pub(crate) lifecycle: RuntimeLifecycleState,
pub(crate) lifecycle_tx: watch::Sender<RuntimeLifecycleState>,

// CoreActorArgs 新增
pub(crate) lifecycle_tx: watch::Sender<RuntimeLifecycleState>,
```

### A.1b lease seam 的类型化错误（NH2 + H2 残留）

```rust
// client/core_bridge.rs —— 取代今天未分化的 anyhow::Result<[u8; 32]>（F41）

/// 外层和：把**两类不同源**的失败分开（H2 残留）。
/// check 阶段本身是一次 actor 调用（F51），所以 actor 错误必然会出现在这个 seam 上；
/// 把它压进下面的两相位类型，会让 ShuttingDown / StaleOperation 失去豁免资格、
/// 让 NoBackend 被导向行 3 而不是行 6b。
pub(crate) enum CheckAndPromoteFailure {
    /// 原样透出，§2.3 的 matches! 与 A.7 直接可判。
    Actor(CoreActorError),
    /// 本 seam 自己的失败，仍然**恰好两相位**（NH2 的分类学不扩大）。
    Operation(CheckAndPromoteError),
}

pub(crate) enum CheckAndPromotePhase {
    Check,      // → §2.2 行 3
    Promote,    // → §2.2 行 4a / 4b（写前 / 写后，按构造位置区分）
}

#[derive(Debug, thiserror::Error)]
#[error("{phase:?} failed: {source:#}")]
pub(crate) struct CheckAndPromoteError {
    pub(crate) phase: CheckAndPromotePhase,
    pub(crate) source: anyhow::Error,
}

// trait 签名：
//   async fn check_and_promote(..) -> Result<[u8; 32], CheckAndPromoteFailure>;
//
// 编排层分流：
//   Actor(e)     → 先过 §2.3 的 matches!（豁免则 Err），否则过 A.7 定行
//   Operation(e) → 按 e.phase 映射行 3 / 4a / 4b，查 A.6 取 code
//
// 相位仍由 adapter 内的构造位置打标签——它确切知道自己停在哪一步；
// 编排层不做字符串嗅探，也不 downcast anyhow。
//
// **边界（H2）**：`CheckAndPromoteError`（内层）只承载本 seam 自己的失败——
// candidate 文件工作与 check 的**非 actor** 部分。actor 侧的错误走 `Actor(_)` 臂，
// 不被改写、不被降级、不丢豁免资格。`PublishPromoted` 同样在 seam 之外独立一段（F45）。
```

### A.2 新增消息（三条，全部守卫消息）

```rust
/// C2：actor 只做 lifecycle 簿记。candidate 建立、哈希比对、check、产物原子晋升
/// 全部留在 client 侧 lease adapter —— 快照的输入本来就只在那里（F32），
/// 相位区分靠 A.1b 的类型化 CheckAndPromoteError（NH2），不是靠 anyhow。
PublishPromoted {
    operation: OperationId,
    snapshot: Arc<RuntimeSnapshot>,          // client 已建好并已晋升产物
    reply: RpcReplyPort<Result<(), CoreActorError>>,   // 拒绝非递增 revision
},
ApplyPromoted {
    operation: OperationId,
    request: CoreRequest,
    expected: Option<RevisionIdInfo>,        // H8：观察到 revision 则 Some，缺失则 None
    snapshot: Arc<RuntimeSnapshot>,          // 供 advances_applied 为真时提交
    reply: RpcReplyPort<Result<CoreApplyData, CoreActorError>>,
},
/// C3：Run 不碰 lifecycle（F33b），restart 类路径靠这条推进 Applied——
/// start_promoted_runtime、legacy 重播、D5 的 Started 分支。
PublishApplied {
    operation: OperationId,
    snapshot: Arc<RuntimeSnapshot>,
    reply: RpcReplyPort<Result<(), CoreActorError>>,   // 要求存在 Promoted 且 identity_eq
},
/// NH1：5a 的 RunningIdentity 的 reply 由 Option<CoreRequest> 扩为联合值——
/// 身份与忠实六态必须在同一个 mailbox 轮次内取齐，否则两次查询之间会撕裂读；
/// 且 CoreStatusView.state 是二值投影，单独用它会把 Starting/Restarting/Stopping 判反（F40）。
RunningIdentity {
    operation: OperationId,
    reply: RpcReplyPort<Result<(Option<CoreRequest>, FaithfulLifecycle), CoreActorError>>,
},
// L5：不加 LifecycleSnapshot 诊断消息。lifecycle_tx 在处理函数内、reply 之前发布
// （F30 的时序），所以 await reply 后读 lifecycle() 已经确定性可见。
```

**actor 内的纯谓词（C3，非消息）：**

```rust
// core/actor/runtime.rs —— Applied 是否推进的唯一决策点
pub(crate) fn advances_applied(outcome: ApplyOutcomeKind) -> bool;
```

### A.3 client 侧新增与删除

```rust
// CoreClient 新增
pub(crate) fn lifecycle(&self) -> RuntimeLifecycleState;          // 同步 watch 克隆
// NH1：5a 的 running(&guard) 返回类型随 A.2 一起加宽
pub(crate) async fn running(&self, op: &CoreOperationGuard)
    -> Result<(Option<CoreRequest>, FaithfulLifecycle), CoreActorError>;
pub(crate) async fn publish_promoted(&self, op: &CoreOperationGuard, snapshot: Arc<RuntimeSnapshot>)
    -> Result<(), CoreActorError>;
pub(crate) async fn publish_applied(&self, op: &CoreOperationGuard, snapshot: Arc<RuntimeSnapshot>)
    -> Result<(), CoreActorError>;
pub(crate) async fn apply_promoted(&self, op: &CoreOperationGuard, req: &CoreRequest,
    expected: Option<RevisionIdInfo>, snapshot: Arc<RuntimeSnapshot>) -> Result<CoreApplyData, CoreActorError>;

// NyanpasuClientInner 删除字段
- clash_patch, clash_patch_gate, rebuild_gate, runtime
// NyanpasuClientInner 保留字段（L2）
  runtime_revisions: runtime::RuntimeRevisionAllocator
// NyanpasuClientInner 新增字段（L4）
+ degradation: Arc<dyn crate::core::actor::backend::CoreDegradationSink>

// ClientSetupArgs 删除字段
- clash_patch
```

### A.4 wire 类型（D4=A + D5=A）

```rust
// client/runtime.rs，与 MutationOutcome 同层。
// F25：workspace 已启用 specta，upstream apply 类型自带 specta::Type，无需镜像 DTO。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeApplyOutcome {
    Noop,
    Patched,
    Reloaded,
    Restarted,
    Switched,
    RolledBack,
    /// 核原本停止，本次操作把它启起来了（D5=A）。upstream 的 apply 不启核，
    /// 因此这个状态在 `ApplyOutcomeKind` 里没有对应变体。
    Started,
    /// 什么终态动作都没发生（C1）：build / check / promote / 传输 / NoBackend 失败。
    /// MutationOutcome 的两变体都要求 value（F31），而 RolledBack / Started 在这些情形下都是假话。
    NotApplied,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RuntimeApplyReport {
    pub outcome: RuntimeApplyOutcome,
    pub desired_revision: u64,
    pub applied_revision: Option<u64>,
}
```

命令返回：`change_clash_core` 与 `patch_clash_config` 均改为 `Result<MutationOutcome<RuntimeApplyReport>>`。

### A.5 wire 表示的唯一决策点（C3 改述）

```rust
/// §3 的 12 格矩阵**全部**由它决定：outcome→report 取值、warning→追加 degradation。
/// C3：它**不**决定 Applied 是否推进——那由 actor 内的 advances_applied() 独占。
/// 其它地方不得再判 outcome 来产 wire 值。
/// NH9：**不要叫 map_apply_outcome**——那个名字已属于
/// core::actor::backend::map_apply_outcome（manager ApplyOutcome → CoreApplyData，
/// 方向相反、模块相邻，F44）。既有函数不改名，新函数两端都写进名字。
/// 都在这里，其它地方不得再判 outcome。
pub(crate) fn runtime_outcome_from_apply_data(
    data: &CoreApplyData,
    promoted_revision: u64,
) -> (RuntimeApplyReport, Vec<Degradation>);
```

### A.7 `Backend(_)` 的有序分类器（H3；**唯一实现处**）

**作用域（先读这条）：A.7 只分类 apply 路径上的 `Backend(_)`。** 生命周期路径（D5 停止态分支的 `restart()`）的失败**不过 A.7**，直接落 §2.2 的 restart-failure 行（`CoreLifecycle` / `core_start_failed`，R1 裁定、T-B4-05 钉住）。

> **为什么必须显式圈作用域**：两条路径的失败**在类型上完全同形**——`Run` 与 apply 一样经 `backend_error()` 包成 `Backend(_)`（F49）。而 Service 侧的启核失败 `error_kind` **恒为 `None`**（`start`/`stop`/`restart` 早于 `error_kind` 引入，F50），A.7 第 4 行会确定性地把它判成 `Other` → 行 7c → `runtime_apply_failed`，**与 R1 裁定的 `core_start_failed` 直接冲突**。靠判据本身分不开，只能靠调用点的路径归属。

`CoreActorError::Backend(_)`（apply 路径）是个口袋：revision 冲突、not-started、传输丢失全在里面（F46）。

```rust
// core/actor/backend.rs —— 与 CoreBackendError 同模块（它最清楚自己的内部结构）
pub(crate) enum BackendFailureClass {
    RevisionConflict,   // → §2.2 行 5
    NotRunning,         // → §2.2 行 7b
    TransportLost,      // → §2.2 行 6a
    Other,              // → §2.2 行 7c
}

pub(crate) fn classify_backend_failure(error: &CoreBackendError) -> BackendFailureClass;
```

| 顺序 | 类                        | Local 判据（`CoreBackendError::Local`）                                             | Service 判据（`CoreBackendError::Service`）                                                                                               |
| ---- | ------------------------- | ----------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| 1    | `RevisionConflict` → 行 5 | `matches!(e, Error::RevisionConflict { .. })`（`error.rs:44`）                      | `Server { error_kind: Some(k), .. }` 且 `k == api::error_kind::REVISION_CONFLICT`（`api/mod.rs:45`）                                      |
| 2    | `NotRunning` → 行 7b      | `matches!(e, Error::NotStarted)`（`error.rs:11`；**它是 `Err` 不是 outcome**，F47） | 同上，`k == api::error_kind::NOT_STARTED`（`api/mod.rs:40`）                                                                              |
| 3    | `TransportLost` → 行 6a   | **无类型化谓词——照实说明，见下**                                                    | **没拿到格式良好的服务信封**的三支：`BuildClient` / `Request` / `WebSocket`，**外加 `HttpStatus`**（F52：它恰恰意味着对端没按本协议应答） |
| 4    | `Other` → 行 7c           | 其余 `Error` 变体                                                                   | **拿到了信封但载荷不可用**：`Decode` / `EmptyData`；`Server` 带其它或缺失 `error_kind`；**以及 `#[non_exhaustive]` 的未来新变体**         |

`CoreBackendError::Binary` / `Construct` 一律归 `Other`（行 7c）。

> **Service 侧的判别原则（一句话）：我们有没有拿到一个格式良好的服务信封？**
>
> - **没拿到** → `TransportLost`：`BuildClient`（连都没建起来）、`Request`（发不出去 / 收不回来）、`WebSocket`、**`HttpStatus`**；
> - **拿到了，但这次操作的载荷不可用** → `Other`：`Decode`（信封 OK、typed 载荷解不了）、`EmptyData`（信封 OK、没带数据）；
> - **拿到了且是服务端分类过的错误** → 按 `error_kind` 走第 1 / 2 顺位，否则 `Other`。
>
> **`Decode` / `EmptyData` 归 `Other` 是对 v6 的修正**：连接成功了、服务也应答了，只是回包内容用不了——把它叫「传输丢失」是错的。这一错源自最初把它们列进传输类的那份清单。
>
> **`HttpStatus` 我归 `TransportLost`，理由**：它的产生条件不是「收到一个回应」而是「**收到的回应不是本协议的信封**」——客户端会先试解信封，解得出的走 `Server`，解不出才落 `HttpStatus`（F52）。所以它实际指向「请求没到达一个能按协议作答的服务」——反向代理挡了、端口错了、服务还没起监听——这更接近链路问题而非 apply 失败。**两行行为完全相同**（retryable=true、report=`NotApplied`），差别只在 degradation 的 code 字符串，即只影响诊断精度：调试者看到 `core_transport_lost` 会去查链路，看到 `runtime_apply_failed` 会去查配置，而前者才是对的方向。

> **第 4 行的 `error_kind: None` 读作「未分类」而不是「无错误」**（`api/mod.rs:35-37` 原文「Absent means "not classified", never "no error"」）。所以在 **apply 路径内**把未分类归 7c 是对的；但正因为「未分类」这个集合同时装着生命周期那批失败（F50），**作用域声明是必需的**——否则 7c 会把启核失败一并吞掉。

> **两处对裁定输入的修正（我逐一核过源码）：**
>
> 1. **Service 侧的分类键是 `error_kind: Option<String>`，不是 `code`。** `ClientError::Server` 两个字段都有：`code: ResponseCode` 是协议层响应码，`error_kind` 才是 **R0 收敛出来的**那个（doc 原文「The envelope's `error_kind`, when the service classified the failure」）。按 `code` 分类会分错。
> 2. **`ClientError` 是七支且 `#[non_exhaustive]`**，不是四支。除 `Server` 外还有 `EmptyData` 与 `WebSocket`；`#[non_exhaustive]` 意味着 submodule 升级可能加新支，**分类器必须有兜底**（第 4 行已覆盖）。

> **Local 侧的传输类没有类型化谓词——照实记录，不造。** `nyanpasu_core_manager` 是**进程内**管理器，没有 IPC 传输层，所以「传输丢失」在 Local 侧**根本不存在**；第 3 行的 Local 格因此为空，不是漏写。Local 的其余失败一律落 `Other`（行 7c）。

> **这条分类链是 R0 的实证价值落点**：Service 侧之所以能把 revision 冲突与 not-started 从传输错误里**分**出来，靠的正是 R0 把服务端错误收敛成 `error_kind` 字符串常量。没有 R0，Service 侧只能看 HTTP 状态码与错误文本——那就退回 NH2 刚消灭的字符串嗅探。**R0 先行的必要性在这里得到实证。**

### A.6 degradation code 常量表（§2.2 / §3 引用）

| code                              | phase            | retryable |
| --------------------------------- | ---------------- | --------- |
| `runtime_build_failed`            | `RuntimeBuild`   | true      |
| `runtime_check_failed`            | `RuntimeCheck`   | true      |
| `runtime_promote_failed`          | `RuntimePromote` | true      |
| `revision_conflict`               | `RuntimeApply`   | true      |
| `core_transport_lost`             | `RuntimeApply`   | true      |
| `core_backend_unavailable`        | `RuntimeApply`   | true      |
| `runtime_apply_failed`            | `RuntimeApply`   | true      |
| `core_not_running`                | `RuntimeApply`   | true      |
| `core_start_failed`               | `CoreLifecycle`  | true      |
| `core_rollback`                   | `CoreRollback`   | true      |
| `core_apply_durability_uncertain` | `RuntimeApply`   | false     |

> **actor 发布失败的归类（NH3 ③）——本表不新增 code**：
>
> - `PublishPromoted` 因**单调性**被拒（`LifecycleInvariant(PromotedRegression)`）→ 不是 degradation，按 I-A 豁免 (b) 处理为 `Err` + 错误级日志。守卫串行 + 单事务单次分配之下，非递增 revision 只可能是 bug；
> - `PublishApplied` 找不到匹配 Promoted 或身份不符（`LifecycleInvariant(AppliedWithoutPromoted)`）→ 同上；
> - `PublishPromoted` / `PublishApplied` 遇 `ShuttingDown` → 按 I-A 豁免 (a) 处理为 `Err`；
> - 写后校验失败（§2.2 第 4b 行）→ 复用 `runtime_promote_failed`，`message` 注明是「产物已写、发布未成」侧。**它不是豁免类**——见 §2.3 的 `matches!` 规则。
