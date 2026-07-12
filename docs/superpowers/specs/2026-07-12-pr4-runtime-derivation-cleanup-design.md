# PR-4 — Runtime 派生化收尾(设计 spec)

**日期:** 2026-07-12
**状态:** 已批准(brainstorming 会话逐节确认);**2026-07-12 按外部审计(GPT-Pro)修订 r2**——处置记录见 §12
**范围基线:** main @ `fbb72905b`(r2 修订:原基线 `40183093b` 之后 main 合入 #4923 韩语 locale 与 #4928 i18n 同步;实施前先 rebase,见 plan Task 0)
**上游依据:** `docs/design/actor-migration-roadmap.md` §4.6;PR-3 复审挂账(`docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md` §6)

---

## 1. 背景与动机

PR-3 之后,runtime 的「执行半」(pipeline executor)与「输入装配」(RuntimeBuilder,取数走 typed client)均已落地,但产物仍写入 legacy 可写状态 `Config::runtime()`(`Draft<IRuntime>`),形成 B8 残余桥(`client/mod.rs` 的 draft 写入,TODO 标记)。本 PR 兑现 roadmap §4.6:**消灭「可写的 runtime 状态」**,runtime 成为纯派生快照,由 facade 持有;同时解决被显式延至 PR-4 的「committed/degraded」IPC 结果模型决策(C-M2 后半)与 C-M5 挂账。

现状实测(2026-07-12,main @ 40183093b;rebase 后逐条复核):

- `Config::runtime()` 共 **12 处 / 6 文件**:`ipc.rs:311/327/341/347`(四读命令)、`config/core.rs:50`(`generate_file`)、`client/mod.rs:719`(B8 draft 写入)+ `:1130`(测试)、`feat.rs:283`(patch_clash 内存 patch)、`core/clash/core.rs:566/577/584`(change_core 的 discard/apply)。
- `rebuild::regenerate()` 进程桥的 legacy 调用点 **5 处**:`feat.rs:268/328/352`、`core/clash/core.rs:559/601`。其中 `feat.rs:268-269/328-329/352-353` 三处为「regenerate + run_core」成对调用,`core.rs:601-602`(`update_config`)为「regenerate + apply_config」成对调用——**这四对的第二步都发生在 rebuild_gate 释放之后**(r2 审计确认,P0-2)。
- 核心启动消费面:`run_core_inner` → `Instance::try_new` 内调 `Config::generate_file(Run)` 生成并按 `runtime_config_path()` 的产物文件启动;热更新走 `apply_config`(check → `generate_file(Run)` → `api::put_configs` ×5 重试)。
- `patch_clash_config` IPC 先 `api::patch_configs` 直推运行核,再 `feat::patch_clash` 落 clash draft + 内存 patch runtime draft(不触发 rebuild)。**运行核先于持久化被修改**——rebuild 失败时 clash draft 被 discard 而运行核不回滚(r2 审计确认,P0-6)。
- 今日四读命令读 `Config::runtime().latest()`(draft-inclusive):check 失败后未 apply 的 draft 同样会暴露给 IPC。「IPC=新未检 / 产物=旧 / 核=旧」的三面分裂**今天就存在**,本 PR 的发布顺序(§5.2)将其修复而非引入。

## 2. 目标

1. **runtime 成为纯派生快照**:facade(`NyanpasuClientInner`)持有 `SimpleStateManager<Option<RuntimeState>>`;`rebuild_running_config()` 保持唯一重建入口,重建在既有 `rebuild_gate` 内串行。**发布顺序(r2,P0-1):候选 check 通过并晋升产物之后才发布到 manager**——四读 IPC 任何时刻只见「已检查晋升的目标配置」。
2. **删除**:`Config::runtime()` accessor 与 `runtime_config` 字段、`IRuntime` 类型、`Config::generate_file()`/`ConfigType`;`Config` god-object 收缩为 clash + verge 两域。`RUNTIME_CONFIG*` 常量与 `runtime_config_path()` 迁至 client 侧;`CHECK_CONFIG` 固定候选文件名**废弃**,候选文件改唯一命名(r2,防 TOCTOU/多实例互踩,见 §5.2)。
3. **四条 runtime 读 IPC**(`get_runtime_config` / `get_runtime_yaml` / `get_runtime_exists` / `get_postprocessing_output`)改为经 `tauri::State<NyanpasuClient>` 读 manager;**wire 形状不变**。语义定义(r2):返回「最新已检查并晋升的目标配置」,**不保证**运行核已成功 apply(后者由 `RebuildOutcome::Degraded` 表达)。
4. **`clash-config.yaml` 降级为产物**:唯一候选文件 check 通过后由 atomicwrites 晋升(check-before-write 不变式保留,见 §5.2);check 使用与 builder **同一输入快照的 target core 显式传参**(r2,P0-3);`CoreManager` 不再自己 generate。
5. **重建/换核/legacy 更新共用同一事务 gate**(r2,P0-2):`change_core` 编排迁入 facade 并全程持有 `rebuild_gate`;legacy 的「regenerate + apply / regenerate + 重启」成对调用改为 gate 内一体完成的组合桥操作(§5.5),消灭「regen 后、apply/启动前」的跨 gate 覆盖窗口。
6. **四字段并入 rebuild**:删除 `feat.rs:283` 的 runtime 内存 patch;**`patch_clash` 提交后总是 regenerate**,重启核心仍限 mixed-port / external-controller / secret 变更(现行为不变)。allow-lan / ipv6 / log-level / mode 的即时性由既有 `api::patch_configs` 直推保证;**直推成功而 rebuild 失败时,IPC 层尽力补偿回推旧值**(r2,P0-6,D6,见 §6.4)。
7. **CommitOutcome 结构化降级**(C-M2 后半决策落地):facade 的 post-commit 内联 rebuild 路径返回 `Ok(RebuildOutcome)` 而非失败即 `Err`;前端同 PR 适配(降级 toast)。
8. **C-M5 消账**:删除 `run_core_inner` 的 `Config::clash().reload()`(`core.rs:465`),按 2026-07-07「重启 = 应用当前 draft」决策。
9. **文档勘误**:roadmap §4.6 **整节重写**(RuntimeArtifact → RuntimeState 读模型,r2)、§4.0/§4.7 图与 PR-5 契约同步修正、B8 行收窄登记、C-M4 显式加入 PR-5 任务清单、ack-based rollback 过渡方向标注(见 §9)。

## 3. 非目标

- `CoreManager` actor 化(PR-5)。`change_core` **编排**迁入 facade 属本 PR(事务 gate 需要,§5.4);核心启停的**实现**仍经 `LegacyCoreBridge → CoreManager::global()`(TODO 标记,PR-5 清偿)。
- C-M4 端口生命周期编排 stop→resolve→mirror→start(改挂 PR-5,并在 roadmap §4.7 任务清单中显式登记,r2)。
- 连接中断服务 typed 读(`ConnectionInterruptionService` 仍读 `Config::verge()`,改挂 PR-6 副作用批次)。
- legacy `patch_clash` / `patch_verge` 流程迁移(PR-5/6);二者维持 draft-discard 先验回滚语义,不套降级模型。
- snapshot-persistence 归档;artifact 图谱类新 UI(**明确放弃** graph / step_logs 进读模型,YAGNI;此为 roadmap §4.6「facade 持有完整 RuntimeArtifact」的正式改判,随 §9 重写 roadmap 落账)。
- ack-based rollback 机制本体(仅标注方向,post-PR-7)。
- 前端测试设施引入(仓库现无 JS 单测 runner,仅 `cargo test`;`extractDegradedRebuild` 的单测记为后续项,见 §12)。

## 4. 用户决策记录(2026-07-11/12 brainstorming;D6–D8 为 2026-07-12 审计修订)

| #   | 决策点                                     | 结论                                                                                                                                                                   |
| --- | ------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | 策划目标                                   | PR-4(roadmap 依赖图唯一解锁项)                                                                                                                                         |
| D2  | committed/degraded IPC 结果模型(C-M2 后半) | **采纳结构化降级结果**;同时标注 TODO:终态方向是利用 state 层异步 ack 在配置应用失败时 rollback,取代降级模型——不在本期 roadmap 内,但须在 roadmap 标注以便后期过渡实施   |
| D3  | PR-4 额外范围                              | 仅并入 C-M5(删 reload 行);C-M4 缓至 PR-5,连接中断 typed 读缓至 PR-6                                                                                                    |
| D4  | 实现方案                                   | 方案 A:facade 持有重建时一次派生的读模型(非存 artifact 本体、非 IRuntime 换名保留)                                                                                     |
| D5  | check 顺序                                 | **保守处理**:先写候选文件 check,通过后再 atomicwrites 晋升产物;不接受「写产物→check」的坏产物窗口。**r2 补全:manager 发布同样后置于晋升成功之后**(P0-1)                |
| D6  | patch_clash 失败一致性(r2)                 | **保留 API-first**(即时性为既定属性,goal 6)+ rebuild 失败时 IPC 层尽力补偿回推旧值;不采纳「重排为 draft→build→patch→commit」(mode 等托盘切换的即时性回退不可接受)      |
| D7  | 事务 gate 统一(r2)                         | `change_core` 编排迁 facade、legacy 成对调用改组合桥操作,全部在 `rebuild_gate` 单次持有内完成;不采纳 generation/checksum 句柄方案(gate 统一后无残余窗口,见 §12)        |
| D8  | boot fallback(r2)                          | 默认配置兜底**同样走候选→check→晋升管线**,D5 不变式无例外;check 失败则不落产物、boot 继续(UI 可开,核启动失败可见)——不采纳「boot 硬失败」(违背应用「可开 UI 自救」惯例) |

## 5. 架构设计

### 5.1 RuntimeState 与存放点

新模块 `client/runtime.rs`:

```rust
/// 最新一次「已通过核心二进制检查并晋升为产物」的目标配置读模型
/// (former IRuntime, minus the draft machinery)。
/// 语义(r2):它是目标配置,不承诺运行中的核已成功 apply——apply 失败
/// 以 RebuildOutcome::Degraded 表达,不影响本状态。
pub struct RuntimeState {
    pub config: serde_yaml::Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
}
```

`NyanpasuClientInner` 增字段 `runtime: SimpleStateManager<Option<RuntimeState>>`(nyanpasu-core;初始 `None`,对应今日 `IRuntime::new()` 的 `config: None` 语义)。

`enhance/artifact_bridge.rs` 的 `runtime_from_artifact` 改名重塑为 `runtime_state_from_artifact`(输入不变:artifact + Profiles 快照 + core + builtin_enabled;输出 `RuntimeState`),postprocessing 映射逻辑与测试原样迁移。**派生在重建点一次完成**——`map_postprocessing` 需要 Profiles 快照,重建时快照现成;读路径因此是纯内存 O(1),不跨 actor。

`runtime_config_path()` 与 `RUNTIME_CONFIG*` 常量从 `config/core.rs` 迁至 client 侧;`resolve.rs` / `core.rs` 消费点随迁。

> 审计建议的改名(`RuntimeReadModel` / `PublishedRuntime`)不采纳:D4 已批准 `RuntimeState` 名称,语义歧义以上述文档注释消除(§12)。

### 5.2 重建管线(check→晋升→发布,D5+P0-1)

`regenerate_runtime_with`(仍在 `rebuild_gate` 内,与 facade rebuild / legacy regenerate 串行):

1. 取数:profiles / clash / app 快照 + resolved_ports(不变);**target core = 本次输入快照的 `app.core`**,贯穿 build 与 check(P0-3);
2. `spawn_blocking`:executor 构建 artifact → 派生 `RuntimeState` + 序列化产物 yaml 字符串(一次完成);
3. 写**唯一候选文件**(temp 目录,`clash-nyanpasu-candidate-{pid}-{seq}.yaml`;固定路径废弃——两实例/测试进程互踩与 TOCTOU 消除);
4. 经桥 `check_and_promote(candidate, target_core)`:核心二进制对候选做只读 parse 检查 → 通过后 atomicwrites 晋升为产物 `runtime/clash-config.yaml`;调用后尽力删除候选文件;
5. **晋升成功后**,manager 原子替换 `RuntimeState`(取代 `*Config::runtime().draft() = runtime`,B8 桥注释摘除)——check 失败时 manager 与产物**都**保持旧值,四读 IPC 永不见被拒配置;
6. (调用方需要时)`apply_config()`:`api::put_configs(产物路径)` 推送热更新(5 次重试逻辑不变)。

不变式:**产物文件与 manager 任何时刻只含通过检查的配置**。代价:每次重建多一次核心二进制 parse 检查;后台重建已有 500ms 防抖,可接受。

**发布失败语义(r2)**:晋升成功后 `upsert` 失败(当前 manager 无 subscriber,属理论路径;未来 ack subscriber 落地时才可能出现)——产物为权威,manager 保旧值,本次重建返回 Err(legacy 路径)/ `Degraded`(facade 路径),下一次成功重建自然重同步;不回滚产物。此语义在 `client/runtime.rs` 文档注释固化。

### 5.3 RunningCoreBridge 拆四操作

```rust
#[async_trait]
pub trait RunningCoreBridge: Send + Sync + 'static {
    /// check 候选文件(用显式传入的 target core 二进制)并原子晋升为产物
    /// (boot 路径可用——核心未运行时不能 put)。target_core 必须与 builder
    /// 使用同一输入快照,禁止实现内部再读全局选核(P0-3)。
    async fn check_and_promote(
        &self,
        candidate: &Utf8Path,
        target_core: ClashCore,
    ) -> anyhow::Result<()>;
    /// 晋升后推送给运行中的核心(put_configs,重试逻辑不变)。
    async fn apply_config(&self) -> anyhow::Result<()>;
    /// 重启核心(change_core / regenerate_and_restart 消费;P0-2)。
    async fn restart_core(&self) -> anyhow::Result<()>;
    async fn on_profile_change(&self);
}
```

- `regenerate_runtime`(boot 路径,`resolve.rs`)止于晋升+发布;`rebuild_running_config` 继续推送。
- `CoreManager::check_config` 收窄为「对给定路径、给定 core 做 parse 检查」——签名 `check_config(&self, config_path: &Utf8Path, core: ClashCore)`,**不再**读 `Config::verge().latest()` 隐式选核(原 spec「draft-inclusive 天然以新核校验」的跨层隐式耦合废弃);`generate_file` / `ConfigType` 删除。
- `LegacyCoreBridge` 实现继续经 `CoreManager::global()`(`restart_core` → `run_core()`),TODO(actor-migration)标记(PR-5 清偿)。
- 审计建议的 `PromotedRuntime { generation, checksum, path }` 句柄 + `apply_config(&PromotedRuntime)` 不采纳:D7 gate 统一后,所有 apply/restart 均与其 regenerate 在**同一次 gate 持有内**完成,不存在「apply 消费到别人 generation」的窗口;句柄属无对应故障面的 API 复杂度(§12)。

### 5.4 change_core:编排迁 facade,全事务 gate + 强回滚(P0-2/P0-4)

`CoreManager::change_core`(`core.rs:549-589`)**整体删除**,编排迁入 facade(实现放 `client/rebuild.rs` 的 legacy-compat impl 区,与其余 legacy 触点同区、带桥注释):

```text
client.change_core(core)                    // rebuild_gate 全程持有
  ↓ 记住旧产物字节(若存在;已检查产物,恢复无需重检)
  ↓ Config::verge().draft().clash_core = new    // legacy 桥,TODO(actor-migration)
  ↓ regenerate(legacy 输入→新核 build+check+晋升+发布)
  │    失败 → discard verge draft → Err(产物/manager 零变化)
  ↓ Logger 清日志(既有全局,TODO(actor-migration) 注释)
  ↓ bridge.restart_core()(新核)
  │    成功 → verge apply + save_file → Ok(IPC 层 reseed wrapper 不变)
  │    失败 → discard verge draft
  │        → 再 regenerate(旧核 check+晋升,state+产物回旧)
  │             成功 → bridge.restart_core()(旧核);启动失败 → 复合错误
  │             失败 → 原子恢复旧产物字节(无旧产物则删产物文件)
  │                  恢复成功 → bridge.restart_core()(旧核)
  │                  恢复/重启失败 → 复合错误,核保持停止
  │        → Err(新核错误为主,回滚各步失败以 context 链附带,不吞错)
```

关键改变(相对 r1):

- gate 覆盖「draft→重建→启动→提交/回滚」整个事务——并发的 profile 提交 / 后台防抖重建 / legacy update_config 不可能在 check 之后、启动之前覆盖产物(P0-2);
- 回滚重建失败**不再 `log_err!` 吞掉**(P0-4):逐级降级为「恢复旧产物字节」,仍失败则返回复合错误且**不用错误产物启动旧核**;
- 复合错误用 anyhow `context` 链表达(`ChangeCoreError` 结构体不采纳——IPC 边界终归字符串化,§12);
- IPC `change_clash_core` 改调 `client.change_core`(仍套 `run_legacy_verge_mutation` reseed wrapper,行为不变)。

runtime 是纯派生物,回滚即从已提交状态重建;draft 机制整体消失。

### 5.5 regenerate 进程桥:三种组合操作(P0-2)

`rebuild.rs` 的 `REGEN_BRIDGE` **保留**,request 升级为枚举三类,**每类都在 facade `rebuild_gate` 单次持有内完成整个组合**:

| 桥操作                     | 组合                           | 迁移的 legacy 调用点                                                                         |
| -------------------------- | ------------------------------ | -------------------------------------------------------------------------------------------- |
| `regenerate()`             | 重建(build→check→晋升→发布)    | `feat::patch_clash` 非重启路径                                                               |
| `regenerate_and_apply()`   | 重建 + `apply_config`(gate 内) | `CoreManager::update_config`(`core.rs:601-602` 的跨 gate 两步合一)                           |
| `regenerate_and_restart()` | 重建 + `restart_core`(gate 内) | `feat.rs:268-269`(patch_clash 重启分支)、`:328-329`/`:352-353`(patch_verge service/tun 分支) |

FIXME 注释的清偿期改写为 PR-5/6。

**错误映射的双层边界**(不变):同一个 `regenerate_runtime_inner` 的 `Result`——

- legacy 调用方(经桥)拿到 `Err` → 各自 discard draft,先验回滚语义不变;
- facade post-commit 路径映射为 `RebuildOutcome::Degraded`(§6.2),状态不回滚。

### 5.6 boot 兜底走管线(P0-5,D8)

`resolve.rs` 启动段:首铸失败且产物缺失时的默认配置兜底**不再**直接 `help::save_yaml` 落盘,改为:序列化默认 clash 配置 → 唯一候选 → `check_and_promote(candidate, core)`。check 失败(如核心二进制缺失)则**不落产物**、记日志、boot 继续——D5「产物文件任何时刻只含通过检查的配置」无例外;此场景下核心启动本就必然失败,行为面无回退。

## 6. IPC 面

### 6.1 四读命令保形

命令名、返回 wire 形状全部不变;实现改为注入 `tauri::State<'_, NyanpasuClient>` 读 manager(State 注入对前端透明)。语义 = 「最新已检查晋升的目标配置」(§5.2)。`None` 语义逐条对齐今日 `IRuntime::new()`:

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

**覆盖规则**:凡 facade 路径在 post-commit 内联 await rebuild 的 IPC——返回 `Result<()>` 者改返 `Result<RebuildOutcome>`;携带数据者(如 `add_profile → ProfileId`、import)改返 `{ value, rebuild }` 包装。具体命令清单在 plan 阶段按 `rebuild_running_config` 调用点逐一枚举(实施期枚举定稿:11 条 unit 命令 + `create_profile` 返回 `RebuildOutcome`,`import_profile` 返回 `CommitOutcome<ProfileId>`;`enhance_profiles` 无前置 commit、`save_profile_file` 不触发 rebuild,均不适用)。后台防抖重建保持 fire-and-forget + 降级日志,不套此模型。

**范围澄清(r2)**:`RebuildOutcome` 只覆盖 **rebuild(build/check/promote/publish/apply)**;`CommitReport.warnings`(commit 后其他副作用)维持 tracing 日志,不进 wire——名字与文档以此为准,避免「post-commit side-effect degradation」的过宽表述。审计建议的 `Degraded { phase, error }` 阶段枚举**暂缓**:anyhow 错误链已携带阶段上下文("failed to check config" / "failed to promote…"),前端只做展示;ack-rollback 落地时再结构化(§12)。

`RebuildOutcome` 定义处标注(普通 TODO,不入 actor-migration 台账——post-PR-7 方向而非桥):

```rust
// TODO(post-PR-7): degraded outcome is transitional. State managers already
// expose async commit acks; the end-state is ack-driven rollback when config
// application fails, replacing this degraded-report model. Tracked in
// actor-migration-roadmap §6.
```

**specta 冻结(r2)**:除命名导出断言外,增加对导出文本的实例化断言——`importProfile` 返回类型含 `CommitOutcome<ProfileId>` 实例化、`RebuildOutcome` 的 union 形状(`{ status: "ok" } | { status: "degraded"; error: string }`)逐字冻结,防 specta 泛型/嵌套 tagged enum 回归。

### 6.3 前端适配

- interface 层统一拦截 `degraded` → 降级 toast(新 i18n key ×**5** 语言:en / zh-cn / zh-tw / ru / **ko**——rebase 后 main 已含韩语 locale,r2);命令本身按成功处理(列表照常刷新)。
- `setDegradedRebuildHandler` 返回 disposer,`useEffect` 清理注册(HMR / StrictMode / 测试隔离,r2)。
- toast 明文展示 `error` 字符串:沿用本应用现有通知惯例(`message()` 已普遍直出错误);`code + i18n` 映射记为后续项(§12)。
- bindings 重导出 + `pnpm typecheck` + `web:build` 同 PR(铁律 3);`specta_export` 冻结测试更新(`IRuntime` 退役、`RebuildOutcome` 等新型入册)。

### 6.4 patch_clash_config 补偿(P0-6,D6)

IPC `patch_clash_config` 顺序改为:

1. 读 manager 捕获 patch 各键的**旧值快照**(纯函数 `compensation_for(patch, prev_config) -> Option<Mapping>`;manager 为 `None` → 无补偿——核心未跑,直推本也会失败);
2. `api::patch_configs(mapping)` 直推(即时性,现行为);
3. `feat::patch_clash(mapping)`(恒重建,§2 goal 6);
4. `feat::patch_clash` 失败(含 rebuild/check 失败,clash draft 已被 feat 内部 discard)→ **尽力** `api::patch_configs(补偿)` 把运行核回推到旧值(失败仅记日志)→ 返回 Err。

消除「API 成功、rebuild 失败」后运行核与持久状态/产物的永久分裂;回归测试见 §8。

## 7. 删除清单(逐处)

| 落点                                                                                         | 处置                                                                                                  |
| -------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `config/runtime.rs`(`IRuntime`)                                                              | 整型删除                                                                                              |
| `config/core.rs`:`runtime_config` 字段、`Config::runtime()`、`generate_file()`、`ConfigType` | 删除;`runtime_config_path()` / `RUNTIME_CONFIG*` 迁至 client 侧;`CHECK_CONFIG` 废弃(候选唯一命名取代) |
| `client/mod.rs:716-719`(B8 draft 写入 + TODO)                                                | 替换为「晋升成功后 manager 原子替换」,TODO 摘除                                                       |
| `client/mod.rs:1132` 测试断言(`facade_add_activate_rebuilds_via_core_bridge` 内)             | 改断言 manager 状态                                                                                   |
| `enhance/artifact_bridge.rs`                                                                 | `runtime_from_artifact` → `runtime_state_from_artifact`                                               |
| `feat.rs:283`(runtime 内存 patch)                                                            | 删除;`patch_clash` 提交后总是 regenerate,重启核心仍限 mixed-port / external-controller / secret       |
| `feat.rs:268-269 / 328-329 / 352-353`(regenerate+run_core 成对)                              | 改 `regenerate_and_restart()`(gate 内一体,P0-2)                                                       |
| `core/clash/core.rs:465`(C-M5 reload)                                                        | 删除                                                                                                  |
| `core/clash/core.rs:549-589`(`change_core` 全函数)                                           | **整体删除**,编排迁 facade `client.change_core`(§5.4);IPC 改调 client                                 |
| `core/clash/core.rs:601-602`(`update_config` 两步)                                           | 改 `regenerate_and_apply()`(gate 内一体,P0-2)                                                         |
| `utils/resolve.rs:178-193`(boot 兜底直写)                                                    | 改走候选→check→晋升管线(§5.6,P0-5)                                                                    |
| `ipc.rs:310-348` 四读命令                                                                    | 改读 facade manager,wire 保形                                                                         |
| `ipc.rs:379-396`(`patch_clash_config`)                                                       | 增补偿逻辑(§6.4)                                                                                      |
| `client/rebuild.rs` FIXME ×2                                                                 | 清偿期改写为 PR-5/6                                                                                   |
| `core_bridge.rs` 连接中断 TODO                                                               | 改挂 PR-6                                                                                             |

**台账影响(r2 修正)**:不再预硬编码「17 → 16」。净变化 = −1(B8 draft 写入)+ 新桥面若干(`check_and_promote` / `restart_core` 的 LegacyCoreBridge impl、facade `change_core` 的 verge draft 与 Logger 触点)——每处均按规则带 `TODO(actor-migration)` + 删除条件(PR-5/6),最终计数以 plan Task 10 的 `rg -n` 全量枚举定稿并回填 roadmap(审计 §四.8)。B8 行残余收窄为仅 `CoreManager` apply/restart/check 桥(PR-5)。

## 8. 测试策略

- **单元**:`runtime_state_from_artifact` 映射(现测试适配);候选→check→晋升→发布→推送顺序断言(mock `RunningCoreBridge`,mockall Sequence);**check 失败 → 产物内容与 manager 双双保持旧值**(D5+P0-1 不变式钉);`change_core` 失败回滚 = 二次 regenerate 断言;**change_core 回滚重建也失败 → 恢复旧产物字节、不以新核产物启动旧核、复合错误**(P0-4 钉);facade post-commit rebuild 失败 → `Ok(Degraded)` 且状态已提交;legacy `regenerate()` 桥仍返 `Err`(draft-discard 语义回归钉);`compensation_for` 纯函数单测 + patch_clash「API 成功、rebuild 失败 → 补偿回推」回归(P0-6)。
- **IPC / 绑定**:四读命令空态 + **seeded 态**(重建后 config/exists/postprocessing 三面准确)wire 测试;`specta_export` 冻结测试更新 + `CommitOutcome<ProfileId>` 实例化/union 形状文本断言(§6.2)。
- **行为不变钉**:`enhance_profiles` 端到端(现有);rebuild gate 串行化(现有);patch_clash 重启谓词单测;**gate 覆盖判据**——`regenerate_and_apply` / `regenerate_and_restart` / `change_core` 的组合步骤在单次 gate 持有内完成(顺序 mock 断言 + 代码审查判据;多线程竞态测试不引入——易 flaky,由结构性串行保证)。
- **boot 兜底**:check 失败时不产生未检产物(P0-5 钉)。
- **全量**:`cargo test --workspace --all-features` 绿;`pnpm typecheck` + `web:build` 绿;bindings 漂移以 `git diff --exit-code -- frontend/interface/src/ipc/bindings.ts` 判定(r2,不再用 `git checkout --` 掩盖)。

## 9. 文档勘误(随本 PR 提交;r2 全面加码)

- **roadmap**(`docs/design/actor-migration-roadmap.md`):
  - **§4.6 整节重写**(非追加状态行):任务 ① 由「`SimpleStateManager<RuntimeArtifact>`」改判为「重建时一次派生 `RuntimeState` 读模型」,显式记录「graph / step_logs 不保留」的 trade-off 与理由(YAGNI;需要图谱/诊断时再引入,post-PR-5);登记实施状态;
  - **§4.0 端态图**:`NC2`(:258)「持有 RuntimeArtifact(PR-4)」与 `ART` 节点(:326)同步改为 RuntimeState 读模型;
  - **§4.7 时序图**(:579-581):`RB-->>NC: RuntimeArtifact` 后增派生步;「SimpleStateManager 原子替换 artifact」改为「候选 check→晋升产物→发布 RuntimeState」;`CO: UpdateConfig(artifact/path)` 契约**定死为产物路径**(artifact 选项随 D4 废弃);
  - **§4.7 任务清单**:C-M4 端口生命周期编排(stop→resolve→mirror→start + fixed-port 占用测试)显式登记为 PR-5 任务与验收项(r2,防文档迁移丢失);
  - §5 B8 行收窄登记 + 收尾登记段(台账计数按 Task 10 实测枚举回填);
  - §6 新增「ack-based rollback 过渡方向」行(D2 标注);
  - §2.4 遗留计数刷新(`Config::*()` / `runtime()` 计数按 rebase 后实测)。
- **task.md(PR-3)**:§6 的 C-M2 后半、C-M5 决策处置回填(指向本 spec);C-M4 后半改挂 PR-5 勘误。

## 10. 执行约定

- 单 PR;分支 `refactor/pr4-runtime-derivation`;**实施前 rebase 到 main @ `fbb72905b`**(r2,Task 0);worktree 隔离(CLAUDE.md §17)。
- **toolchain 事实(r2 勘误)**:仓库 `rust-toolchain.toml` 为浮动 `channel = "nightly"`,**未钉日期**;本地 ICE 规避(nightly-2026-05-27 override、kache 处理)按项目记忆在环境侧处理,不随本 PR 入库。
- 无原子切换组:全程编译绿,可分 commit 推进(与 PR-3 的 T07–T10 不同)。
- 作者模式 = 主会话串行(沿 PR-3 先例)。
- plan 用 `superpowers:writing-plans` 展开(TDD、bite-sized);P0 修复相关的关键测试在 plan 中给出完整可执行代码,不留自然语言占位(审计 §四.7)。

## 11. 验收判据

1. `grep -rn "Config::runtime" backend/tauri/src` 零命中;`grep -rn "IRuntime" backend/tauri/src` 零命中。
2. `grep -rn "generate_file\|ConfigType" backend/tauri/src` 仅允许无关同名(预期零)。
3. `enhance_profiles` IPC 行为不变(现有端到端测试绿)。
4. 产物+发布不变式:check 失败时 `runtime/clash-config.yaml` 与 manager **双双**保持旧内容(测试断言,P0-1)。
5. facade post-commit rebuild 失败返回 `Ok(Degraded)` 且 profiles 状态已提交;legacy 桥路径仍 `Err`。
6. gate 判据:`change_core` / `regenerate_and_apply` / `regenerate_and_restart` 的全部组合步骤在单次 `rebuild_gate` 持有内完成(顺序测试 + 审查)。
7. `change_core` 回滚判据:新核启动失败 → 旧核以旧配置(重建或恢复的旧产物)启动;回滚链任何一步失败返回复合错误,不吞错、不用新核产物启动旧核(P0-4)。
8. `cargo test --workspace --all-features` 绿;bindings 重导出后 `pnpm typecheck` + `web:build` 绿;全量测试后 `git diff --exit-code -- frontend/interface/src/ipc/bindings.ts` 零漂移。
9. `TODO/FIXME(actor-migration)` 台账 = Task 10 `rg -n` 全量枚举定稿,逐处与 §7 对账,计数回填 roadmap。

## 12. 审计处置记录(2026-07-12,GPT-Pro 全面审计 r2)

外部审计以 remote main @ `fbb72905b` 为基线,对 r1 spec/plan 输出 6 项阻断(P0)+ 多项非阻断与计划可执行性缺陷。逐项核对本仓库代码后处置如下:

**采纳(代码核实成立):**

| 审计项                                                                                                                                     | 处置                                                                                                                                                                                                                                                                                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P0-1 发布先于 check                                                                                                                        | §5.2 顺序改为 check→晋升→发布;补充事实修正:今日 IPC 读 draft-inclusive `latest()`,分裂**已存在**,本 PR 修复之                                                                                                                                                                                                                                                                     |
| P0-2 change_core / update_config / patch 成对调用跨 gate                                                                                   | D7:change_core 迁 facade 全事务 gate(§5.4);三种组合桥操作(§5.5)                                                                                                                                                                                                                                                                                                                   |
| P0-4 回滚失败被吞                                                                                                                          | §5.4 强回滚链 + 复合错误,禁止以新核产物启动旧核                                                                                                                                                                                                                                                                                                                                   |
| P0-5 boot 兜底直写产物                                                                                                                     | D8:兜底走管线(§5.6);但不采纳「boot 硬失败」                                                                                                                                                                                                                                                                                                                                       |
| P0-6 API-first 失败分裂                                                                                                                    | D6:保留 API-first + IPC 层补偿回推(§6.4)+ 回归测试                                                                                                                                                                                                                                                                                                                                |
| P0-3 build/check 选核可能不一致                                                                                                            | 采纳显式 `target_core` 传参(§5.3)。**部分驳回**:审计所述「change_core 以 typed old-core 构建、legacy new-core 检查」不成立——legacy 路径经 `legacy_regen_inputs()` 读 draft-inclusive `latest()` 转 typed 输入,build 与 check 同用新核(有测试 `legacy_regen_inputs_conversion_reflects_drafted_fields` 钉住);真实窗口是 facade 路径与未提交 verge draft 交错,已由 D7 gate 统一关闭 |
| 固定候选路径 TOCTOU                                                                                                                        | 唯一候选命名(§5.2)                                                                                                                                                                                                                                                                                                                                                                |
| 晋升↔发布无共同原子边界                                                                                                                    | 发布失败语义文档化(§5.2):产物权威,下次重建自愈                                                                                                                                                                                                                                                                                                                                    |
| RebuildOutcome 范围与 warnings                                                                                                             | §6.2 范围澄清:仅 rebuild,warnings 不进 wire                                                                                                                                                                                                                                                                                                                                       |
| specta 泛型冻结不足                                                                                                                        | §6.2 实例化/union 文本断言                                                                                                                                                                                                                                                                                                                                                        |
| C-M4 只改标签会丢任务                                                                                                                      | §9:显式登记进 roadmap §4.7 任务与验收                                                                                                                                                                                                                                                                                                                                             |
| roadmap 两套权威并存                                                                                                                       | §9:路线 B(D4 既定)整节重写 §4.6 + §4.0/§4.7 图与契约,记录 graph/step_logs 放弃决策                                                                                                                                                                                                                                                                                                |
| 计划缺陷:T2 过滤器假绿、T4 双过滤器、T5 漏 stage、T10 `2>$null` 与 checkout 掩盖漂移、toolchain 声称失实、缺 Task 0 rebase、关键测试伪代码 | plan r2 全部修正                                                                                                                                                                                                                                                                                                                                                                  |
| 缺韩语 key;handler 无 disposer                                                                                                             | §6.3 采纳                                                                                                                                                                                                                                                                                                                                                                         |

**驳回 / 暂缓(附理由):**

| 审计项                                                                         | 处置与理由                                                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| `RuntimeReadModel` / `PublishedRuntime` 改名                                   | 驳回:D4 已批准 `RuntimeState`;语义歧义以文档注释消除,改名属无行为收益的 churn                                 |
| `PromotedRuntime{generation,checksum}` 句柄 + `apply_config(&PromotedRuntime)` | 驳回:D7 gate 统一后 apply/restart 与 regenerate 同一 gate 持有,无残余覆盖窗口;句柄是无故障面对应的 API 复杂度 |
| `Degraded { phase }` 阶段枚举                                                  | 暂缓:anyhow 错误链已带阶段上下文,前端只展示;ack-rollback(post-PR-7)时再结构化                                 |
| `ChangeCoreError` 结构体                                                       | 驳回:IPC 边界字符串化,anyhow context 链等价表达(§5.4)                                                         |
| boot check 失败应硬失败                                                        | 驳回:应用惯例为「UI 可开、核失败可见可自救」;D5 不变式已由 D8 保全                                            |
| patch_clash 重排为 draft→build→patch→commit                                    | 驳回:mode/allow-lan 托盘即时性是既定产品属性(goal 6);D6 补偿覆盖失败一致性                                    |
| 前端 `extractDegradedRebuild` / MutationCache 单测                             | 暂缓:仓库无 JS 测试设施(仅 `cargo test`),引入 vitest 超出 PR-4 范围;记为后续项                                |
| toast 错误脱敏(code+i18n)                                                      | 暂缓:与应用现有错误通知惯例一致;结构化错误码属独立改造                                                        |
| 并发竞态测试(change_core×rebuild 等)                                           | 部分采纳:以 gate 结构性串行 + 顺序 mock 断言钉住;多线程真竞态测试易 flaky,不引入                              |
