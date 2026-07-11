# PR-4 — Runtime 派生化收尾(设计 spec)

**日期:** 2026-07-12
**状态:** 已批准(brainstorming 会话逐节确认)
**范围基线:** main @ `40183093b`(PR-3 全部合入:#4889 / #4890 / #4920)
**上游依据:** `docs/design/actor-migration-roadmap.md` §4.6;PR-3 复审挂账(`docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md` §6)

---

## 1. 背景与动机

PR-3 之后,runtime 的「执行半」(pipeline executor)与「输入装配」(RuntimeBuilder,取数走 typed client)均已落地,但产物仍写入 legacy 可写状态 `Config::runtime()`(`Draft<IRuntime>`),形成 B8 残余桥(`client/mod.rs` 的 draft 写入,TODO 标记)。本 PR 兑现 roadmap §4.6:**消灭「可写的 runtime 状态」**,runtime 成为纯派生快照,由 facade 持有;同时解决被显式延至 PR-4 的「committed/degraded」IPC 结果模型决策(C-M2 后半)与 C-M5 挂账。

现状实测(2026-07-12,main @ 40183093b):

- `Config::runtime()` 共 **12 处 / 6 文件**:`ipc.rs:311/327/341/347`(四读命令)、`config/core.rs:50`(`generate_file`)、`client/mod.rs:719`(B8 draft 写入)+ `:1130`(测试)、`feat.rs:283`(patch_clash 内存 patch)、`core/clash/core.rs:566/577/584`(change_core 的 discard/apply)。
- `rebuild::regenerate()` 进程桥的 legacy 调用点 **5 处**:`feat.rs:268/328/352`、`core/clash/core.rs:559/601`。
- 核心启动消费面:`run_core_inner` → `Instance::try_new` 按 `runtime_config_path()` 的产物文件启动;热更新走 `apply_config`(check → `generate_file(Run)` → `api::put_configs` ×5 重试)。
- `patch_clash_config` IPC 先 `api::patch_configs` 直推运行核,再 `feat::patch_clash` 落 clash draft + 内存 patch runtime draft(不触发 rebuild)。

## 2. 目标

1. **runtime 成为纯派生快照**:facade(`NyanpasuClientInner`)持有 `SimpleStateManager<Option<RuntimeState>>`;`rebuild_running_config()` 保持唯一重建入口,重建在既有 `rebuild_gate` 内串行。
2. **删除**:`Config::runtime()` accessor 与 `runtime_config` 字段、`IRuntime` 类型、`Config::generate_file()`/`ConfigType`;`Config` god-object 收缩为 clash + verge 两域(`CHECK_CONFIG`/`RUNTIME_CONFIG*` 常量与 `runtime_config_path()` 不删,随候选/产物逻辑迁至 client 侧)。
3. **四条 runtime 读 IPC**(`get_runtime_config` / `get_runtime_yaml` / `get_runtime_exists` / `get_postprocessing_output`)改为经 `tauri::State<NyanpasuClient>` 读 manager;**wire 形状不变**。
4. **`clash-config.yaml` 降级为产物**:候选文件 check 通过后由 atomicwrites 晋升(check-before-write 不变式保留,见 §5.2);`CoreManager` 不再自己 generate。
5. **四字段并入 rebuild**:删除 `feat.rs:283` 的 runtime 内存 patch;明确触发谓词——**`patch_clash` 提交后总是 regenerate**(整个 clash mapping 都是 rebuild 输入,经 ClashConfig 转换进入 guard overrides),而**重启核心仍限 mixed-port / external-controller / secret 变更**(现行为不变)。allow-lan / ipv6 / log-level / mode 的即时性由既有 `api::patch_configs` 直推保证,派生一致性由 regenerate 保证。
6. **CommitOutcome 结构化降级**(C-M2 后半决策落地):facade 的 post-commit 内联 rebuild 路径返回 `Ok(RebuildOutcome)` 而非失败即 `Err`;前端同 PR 适配(降级 toast)。
7. **C-M5 消账**:删除 `run_core_inner` 的 `Config::clash().reload()`(`core.rs:465`),按 2026-07-07「重启 = 应用当前 draft」决策。
8. **文档勘误**:roadmap B8 行收窄登记、C-M4 改挂 PR-5、ack-based rollback 过渡方向标注(见 §9)。

## 3. 非目标

- `CoreManager` actor 化(PR-5)。
- C-M4 端口生命周期编排 stop→resolve→mirror→start(改挂 PR-5:编排需要控制核心启停时序,属 CoreActor 职责;在 legacy CoreManager 上做一遍 PR-5 还要重做)。
- 连接中断服务 typed 读(`ConnectionInterruptionService` 仍读 `Config::verge()`,改挂 PR-6 副作用批次)。
- legacy `patch_clash` / `patch_verge` 流程迁移(PR-5/6);二者维持 draft-discard 先验回滚语义,不套降级模型。
- snapshot-persistence 归档;artifact 图谱类新 UI(graph / step_logs 不进读模型,YAGNI)。
- ack-based rollback 机制本体(仅标注方向,post-PR-7)。

## 4. 用户决策记录(2026-07-11/12 brainstorming)

| #   | 决策点                                     | 结论                                                                                                                                                                 |
| --- | ------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | 策划目标                                   | PR-4(roadmap 依赖图唯一解锁项)                                                                                                                                       |
| D2  | committed/degraded IPC 结果模型(C-M2 后半) | **采纳结构化降级结果**;同时标注 TODO:终态方向是利用 state 层异步 ack 在配置应用失败时 rollback,取代降级模型——不在本期 roadmap 内,但须在 roadmap 标注以便后期过渡实施 |
| D3  | PR-4 额外范围                              | 仅并入 C-M5(删 reload 行);C-M4 缓至 PR-5,连接中断 typed 读缓至 PR-6                                                                                                  |
| D4  | 实现方案                                   | 方案 A:facade 持有重建时一次派生的读模型(非存 artifact 本体、非 IRuntime 换名保留)                                                                                   |
| D5  | check 顺序                                 | **保守处理**:先写候选文件 check,通过后再 atomicwrites 晋升产物;不接受「写产物→check」的坏产物窗口                                                                    |

## 5. 架构设计

### 5.1 RuntimeState 与存放点

新模块 `client/runtime.rs`:

```rust
pub struct RuntimeState {
    pub config: serde_yaml::Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
}
```

即今日 `IRuntime` 的只读形态。`NyanpasuClientInner` 增字段 `runtime: SimpleStateManager<Option<RuntimeState>>`(nyanpasu-core;初始 `None`,对应今日 `IRuntime::new()` 的 `config: None` 语义)。

`enhance/artifact_bridge.rs` 的 `runtime_from_artifact` 改名重塑为 `runtime_state_from_artifact`(输入不变:artifact + Profiles 快照 + core + builtin_enabled;输出 `RuntimeState`),postprocessing 映射逻辑与测试原样迁移。**派生在重建点一次完成**——`map_postprocessing` 需要 Profiles 快照(取 profile 名),重建时快照现成;读路径因此是纯内存 O(1),不跨 actor。

`runtime_config_path()` 与 `RUNTIME_CONFIG*` 常量从 `config/core.rs` 迁至 client 侧;`resolve.rs` / `core.rs` 消费点随迁。

### 5.2 重建管线(check 前置晋升,D5)

`regenerate_runtime_with`(仍在 `rebuild_gate` 内,与 facade rebuild / legacy regenerate 串行):

1. 取数:profiles / clash / app 快照 + resolved_ports(不变);
2. `spawn_blocking`:executor 构建 artifact → 派生 `RuntimeState` + 序列化产物 yaml 字符串(一次完成);
3. manager 原子替换 `RuntimeState`(取代 `*Config::runtime().draft() = runtime`,B8 桥注释摘除);
4. 写**候选文件**(temp 目录,沿用 `clash-config-check.yaml` 常量);
5. 经桥 `check_and_promote(candidate)`:核心二进制对候选做只读 parse 检查 → 通过后 atomicwrites 晋升为产物 `runtime/clash-config.yaml`;
6. (调用方需要时)`apply_config()`:`api::put_configs(产物路径)` 推送热更新(5 次重试逻辑不变)。

不变式:**产物文件任何时刻只含通过检查的配置**(check-before-write 保留)。代价:每次重建多一次核心二进制 parse 检查;后台重建已有 500ms 防抖,可接受。

### 5.3 RunningCoreBridge 拆两操作

```rust
#[async_trait]
pub trait RunningCoreBridge: Send + Sync + 'static {
    /// check 候选文件并原子晋升为产物(boot 路径可用——核心未运行时不能 put)。
    async fn check_and_promote(&self, candidate: &Utf8Path) -> anyhow::Result<()>;
    /// 晋升后推送给运行中的核心(put_configs,重试逻辑不变)。
    async fn apply_config(&self) -> anyhow::Result<()>;
    async fn on_profile_change(&self);
}
```

- `regenerate_runtime`(boot 路径,`resolve.rs`)止于晋升;`rebuild_running_config` 继续推送。
- `CoreManager::check_config` 保留但收窄为「对给定路径做 parse 检查」(binary 选择仍读 `Config::verge().latest().clash_core`,draft-inclusive——`change_core` 天然以新核校验);`generate_file` / `ConfigType` 删除。
- `LegacyCoreBridge` 实现继续经 `CoreManager::global()`,TODO(actor-migration)标记不变(PR-5 清偿)。

### 5.4 change_core 回滚语义:回滚 = 重建

`Config::runtime().discard()/apply()` 三处(`core.rs:566/577/584`)删除。失败路径改为:

```text
draft verge(新核)→ regenerate(新核 check + 晋升)→ 启动新核
  失败 → discard verge draft → 再 regenerate 一次(旧核 check + 晋升,state + 产物随之回旧)→ 重启旧核
```

runtime 是纯派生物,回滚即从已提交状态重建,draft 机制整体消失。

### 5.5 regenerate 进程桥的去留

`rebuild::regenerate()` 与 `REGEN_BRIDGE` **保留**(feat.rs ×3、CoreManager ×2 的 legacy 调用点是 PR-5/6 迁移对象),仅内部写入目标随 §5.2 管线改变;FIXME 注释的清偿期改写为 PR-5/6。

**错误映射的双层边界**:同一个 `regenerate_runtime_inner` 的 `Result`——

- legacy 调用方(经 `regenerate()`)拿到 `Err` → 各自 discard draft,先验回滚语义不变;
- facade post-commit 路径映射为 `RebuildOutcome::Degraded`(§6.2),状态不回滚。

## 6. IPC 面

### 6.1 四读命令保形

命令名、返回 wire 形状全部不变;实现改为注入 `tauri::State<'_, NyanpasuClient>` 读 manager(State 注入对前端透明)。`None` 语义逐条对齐今日 `IRuntime::new()`:

| 命令                        | manager 为 `None` 时                  |
| --------------------------- | ------------------------------------- |
| `get_runtime_config`        | `Ok(None)`                            |
| `get_runtime_yaml`          | `Err`(同今日空 config 报错)           |
| `get_runtime_exists`        | `Ok(vec![])`                          |
| `get_postprocessing_output` | `Ok(PostProcessingOutput::default())` |

### 6.2 RebuildOutcome 降级模型(D2)

新 specta 类型(tagged enum):

```rust
#[derive(Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RebuildOutcome {
    Ok,
    Degraded { error: String },
}
```

**覆盖规则**:凡 facade 路径在 post-commit 内联 await rebuild 的 IPC——返回 `Result<()>` 者改返 `Result<RebuildOutcome>`;携带数据者(如 `add_profile → ProfileId`、import)改返 `{ value, rebuild }` 包装。具体命令清单在 plan 阶段按 `rebuild_running_config` 调用点逐一枚举(已知含 `enhance_profiles`、activate / save / patch / delete 触发重建的路径)。后台防抖重建保持 fire-and-forget + 降级日志,不套此模型。

`RebuildOutcome` 定义处标注(普通 TODO,不入 actor-migration 台账——post-PR-7 方向而非桥):

```rust
// TODO(post-PR-7): degraded outcome is transitional. State managers already
// expose async commit acks; the end-state is ack-driven rollback when config
// application fails, replacing this degraded-report model. Tracked in
// actor-migration-roadmap §6.
```

### 6.3 前端适配

- interface 层统一拦截 `degraded` → 降级 toast(新 i18n key ×4 语言:en / zh-cn / zh-tw / ru);命令本身按成功处理(列表照常刷新)。
- bindings 重导出 + `pnpm typecheck` + `web:build` 同 PR(铁律 3);`specta_export` 冻结测试更新(`IRuntime` 退役、`RebuildOutcome` 等新型入册)。

## 7. 删除清单(逐处)

| 落点                                                                                         | 处置                                                                                            |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `config/runtime.rs`(`IRuntime`)                                                              | 整型删除                                                                                        |
| `config/core.rs`:`runtime_config` 字段、`Config::runtime()`、`generate_file()`、`ConfigType` | 删除;`runtime_config_path()` / `RUNTIME_CONFIG*` / `CHECK_CONFIG` 迁至 client 侧                |
| `client/mod.rs:716-719`(B8 draft 写入 + TODO)                                                | 替换为 manager 原子替换,TODO 摘除                                                               |
| `client/mod.rs:1130` 测试断言                                                                | 改断言 manager 状态                                                                             |
| `enhance/artifact_bridge.rs`                                                                 | `runtime_from_artifact` → `runtime_state_from_artifact`                                         |
| `feat.rs:283`(runtime 内存 patch)                                                            | 删除;`patch_clash` 提交后总是 regenerate,重启核心仍限 mixed-port / external-controller / secret |
| `core/clash/core.rs:465`(C-M5 reload)                                                        | 删除                                                                                            |
| `core/clash/core.rs:566/577/584`(runtime discard/apply)                                      | 删除,失败路径改「discard verge → 再 regenerate」                                                |
| `ipc.rs:310-348` 四读命令                                                                    | 改读 facade manager,wire 保形                                                                   |
| `client/rebuild.rs` FIXME ×2                                                                 | 清偿期改写为 PR-5/6                                                                             |
| `core_bridge.rs` 连接中断 TODO                                                               | 改挂 PR-6                                                                                       |

**台账影响**:`TODO/FIXME(actor-migration)` 17 → 预计 16(−B8 draft 写入;零新增)。B8 行残余收窄为仅 `CoreManager` apply 桥(PR-5)。

## 8. 测试策略

- **单元**:`runtime_state_from_artifact` 映射(现测试适配);候选→check→晋升→推送顺序断言(mock `RunningCoreBridge` 两操作);`change_core` 失败回滚 = 二次 regenerate 断言;facade post-commit rebuild 失败 → `Ok(Degraded)` 且状态已提交(现有 failing-content-source 测试改造);legacy `regenerate()` 桥仍返 `Err`(draft-discard 语义回归钉)。
- **IPC / 绑定**:四读命令对 seeded manager 的 wire 形状测试;`specta_export` 冻结测试更新。
- **行为不变钉**:`enhance_profiles` 端到端(现有);rebuild gate 串行化(现有);patch_clash 四字段触发 regenerate 的谓词单测。
- **全量**:`cargo test --workspace --all-features` 绿;`pnpm typecheck` + `web:build` 绿;bindings 除预期新类型外零漂移。

## 9. 文档勘误(随本 PR 提交)

- **roadmap**(`docs/design/actor-migration-roadmap.md`):§4.6 状态行(→ 完成);§5 B8 行收窄登记;C-M4 端口编排改挂 PR-5;§6 新增「ack-based rollback 过渡方向」行(D2 标注);§2.4 遗留计数刷新。
- **task.md(PR-3)**:§6 的 C-M2 后半、C-M5 决策处置回填(指向本 spec)。

## 10. 执行约定

- 单 PR;分支 `refactor/pr4-runtime-derivation`;worktree 隔离(CLAUDE.md §17)。
- 无原子切换组:全程编译绿,可分 commit 推进(与 PR-3 的 T07–T10 不同)。
- 作者模式 = 主会话串行(沿 PR-3 先例)。
- plan 用 `superpowers:writing-plans` 展开(TDD、bite-sized)。

## 11. 验收判据

1. `grep -rn "Config::runtime" backend/tauri/src` 零命中;`grep -rn "IRuntime" backend/tauri/src` 零命中。
2. `grep -rn "generate_file\|ConfigType" backend/tauri/src` 仅允许无关同名(预期零)。
3. `enhance_profiles` IPC 行为不变(现有端到端测试绿)。
4. 产物文件不变式:check 失败时 `runtime/clash-config.yaml` 保持旧内容(测试断言)。
5. facade post-commit rebuild 失败返回 `Ok(Degraded)` 且 profiles 状态已提交;legacy 桥路径仍 `Err`。
6. `cargo test --workspace --all-features` 绿;bindings 重导出后 `pnpm typecheck` + `web:build` 绿。
7. `TODO/FIXME(actor-migration)` 台账 = 16,逐处与 §7 对账。
