# PR-6 Application Effects / SystemProxy / Hotkey / UI 副作用架构设计

**日期：** 2026-09-12
**基线：** `main @ 31967446d`（worktree `feat/pr6-effect-plan`）
**范围：** 任务 6e-1、6e-2、6a、6b、6e-3
**不在范围：** 6c（ProxiesActor）、6d（UpdaterActor）、6e-4（连接中断）、6e-5（启动/退出全量接线）、6-final、PR-7 清算
**权威顺序：** `AGENTS.md` > `docs/design/actor-migration-roadmap.md` §1/§7/§9/§10/§11 > 本设计 > task card > implementation plan

---

## 0. 结论摘要

1. 新增 **纯服务** `ApplicationEffectPlan`：从两份 `ApplicationEffectInputs` 投影快照计算出需要执行的副作用列表。零 Tauri、零 OS、零 IO，全部可用普通值单测。
2. 新增 **单一分发缝** `ApplicationEffectsPort`，由 `ApplicationEffectExecutor` 实现；executor 内部扇出到 **两个 actor typed client**（`SystemProxyClient`、`HotkeyClient`）和 **四个窄 adapter trait**（`LocaleSink`、`TrayRefresher`、`LoggerRefresher`、`WidgetController`）。不新建 god-actor。
3. facade 建立统一 mutation 管线：`读 before → typed commit → runtime apply → 读 after → diff → 执行 effects → MutationOutcome`。提交前失败返回 `Err`，提交后失败返回 `CommittedDegraded`。
4. 陈旧保护由两层组成：facade 侧 `ApplicationEffectGate`（串行化 before/after 采样与 revision 分配），执行侧每个 effect owner 自持 `applied_revision` 并丢弃更旧的 revision。
5. `feat::patch_verge` 中的副作用编排 **逐项迁出并在同一 commit 内删除原实现**，因此永远不会出现"两处都执行"的窗口。6e-3 结束后 `feat::patch_verge` 只剩 `enable_service_mode`、TUN 权限预检、`theme_color` 校验三件事，作为显式标注的 legacy shim 留给 PR-7 删除。
6. `Sysopt::global()`（8 处）、`Hotkey::global()`（3 处）、`core/sysopt.rs`、`core/pac.rs`、`core/hotkey.rs` 在 6a/6b 内整体删除。

---

## 1. 现状关键事实（已核对源码，不是推测）

| 事实                                                                                                            | 位置                                                                                  |
| --------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| `MutationOutcome<T>` / `Degradation` / `DegradationPhase` 已定型并已上 wire                                     | `backend/tauri/src/client/runtime.rs:375-460`                                         |
| `DegradationPhase` 已含 `SystemEffect` / `UiEffect`                                                             | `client/runtime.rs:451-462`                                                           |
| `NyanpasuClient::patch_app_config` 当前**无生产调用方**（只有测试）                                             | `client/mod.rs:544`；生产路径走 legacy saga                                           |
| 生产 verge patch 路径：IPC → `LegacyVergeBridge::patch_verge_config` → 两条路由                                 | `ipc.rs:492`、`bridge/verge.rs:219-259`                                               |
| `route_verge_patch` 用 15 个字段决定走 `LegacySideEffects`                                                      | `bridge/verge.rs:491-513`                                                             |
| `LegacySideEffects` 路由 = 先跑 `feat::patch_verge`（副作用在提交前），再回滚 legacy 文件、重新经三域 saga 提交 | `bridge/verge.rs:272-355`                                                             |
| 提交后唯一副作用是 TUN/控制通道 reconcile                                                                       | `bridge/verge.rs:328-347`                                                             |
| 三域 saga 的实际提交点                                                                                          | `client/mod.rs:612-700`（`apply_legacy_verge_states_saga`）                           |
| `feat::patch_verge` 的副作用块                                                                                  | `feat.rs:368-452`                                                                     |
| `Sysopt` 全局单例 + guard 循环无取消句柄、间隔滞后一拍                                                          | `core/sysopt.rs:60,346-408`                                                           |
| PAC 无本地 HTTP 服务，只有远端 URL 下载 + `sysproxy::Autoproxy`                                                 | `core/pac.rs:104-250`                                                                 |
| `Hotkey::init` 的 KV/verge 双读                                                                                 | `core/hotkey.rs:186-204`                                                              |
| hotkeys 双写源：`set_hotkeys` 写 KV，`patch_verge_config` 写 typed config，互不同步                             | `ipc.rs:1068-1077`、`bridge/mod.rs:87`                                                |
| `WidgetManager` 经进程级 `consts::app_handle()` 获取                                                            | `feat.rs:436-437`                                                                     |
| `refresh_logger` 是 mpsc 单向信号                                                                               | `utils/init/logging.rs:33`                                                            |
| `UiEventSink` 已有 `update_systray` / `update_systray_part`，后者被 `core_lifecycle/workflow.rs:192` 使用       | `client/event_sink.rs:9-35`                                                           |
| 组合根                                                                                                          | `setup.rs:22-93`；`ClientSetupArgs` 在 `client/mod.rs:71-81`                          |
| 退出清理                                                                                                        | `utils/help.rs:243-269`，其中 `resolve_reset()` → `Sysopt::global().reset_sysproxy()` |
| `SessionPortResolver` 缓存实际端口，按 `PortsFingerprint` 失效                                                  | `client/ports.rs:39-150`                                                              |
| `ClashGuardOverrides` 七个字段全部私有、无 getter                                                               | `nyanpasu-config/src/clash/config/overrides/mod.rs:59-67`                             |

**由此得出的两个硬约束：**

- 本设计**不读 `mode` / `allow_lan`**。mode 变化的 tray part 刷新已经由 `core_lifecycle/workflow.rs:192` 在 reconcile 后完成，重复实现只会产生双刷新。
- `feat::patch_clash_with_rebuild`（`feat.rs:263`）唯一调用方 `feat::patch_clash` 带 `#[allow(dead_code)]`，即**当前 mixed-port 变化既不 rebuild 也不重设系统代理**。这是既存缺陷，6e-2 的 `runtime_apply_kind` 顺带修复它，属于 6a 验收项"端口变化"的前置条件。

---

## 2. 目标结构

```text
Tauri command (ipc.rs)  /  tray menu  /  HotkeyActionPump
        ↓
NyanpasuClient（facade）
   ├─ ApplicationEffectGate            串行化 before/after 采样 + EffectRevision 分配
   ├─ ApplicationEffectPlan::diff/full 纯服务，无依赖
   └─ Arc<dyn ApplicationEffectsPort>  单一分发缝
            ↓ ApplicationEffectExecutor
            ├─ SystemProxyClient  → SystemProxyActor  → OsProxyPort / AutoLaunchPort / PacPort
            ├─ HotkeyClient       → HotkeyActor       → ShortcutRegistrar / HotkeyActionSink
            ├─ Arc<dyn LocaleSink>
            ├─ Arc<dyn TrayRefresher>
            ├─ Arc<dyn LoggerRefresher>
            └─ Arc<dyn WidgetController>
```

新增模块目录（均为新文件，避开另外两条 PR-6 分支已占用的行区）：

```text
backend/tauri/src/client/effects/{mod,plan,status,ports,executor,tests}.rs     6e-1 / 6e-2
backend/tauri/src/client/system_proxy/{mod,actor,ports,adapters,tests}.rs       6a
backend/tauri/src/client/hotkey/{mod,actor,ports,adapters,tests}.rs             6b
backend/tauri/src/client/ui_effects/{mod,ports,adapters,tests}.rs               6e-3
```

文件名不含 `bridge`、不位于 `bridge/` 目录，因此不会污染 ledger 的 `bridgeFiles` 列表。

---

## 3. 任务 6e-1 — 副作用计划与结果协议

### 3.1 输入投影 `ApplicationEffectInputs`

`NyanpasuAppConfig` 与 `ClashConfig` 都**没有** `PartialEq`（`nyanpasu-config/src/application/mod.rs:64`、`clash/config/mod.rs:30`），且带 `cfg(target_os)` / `cfg(feature)` 字段。因此 diff 不直接比较配置结构体，而是先投影成一个小的、全 `PartialEq` 的输入类型。这同时把"无关字段变动不触发副作用"变成类型层面的保证。

```rust
// client/effects/plan.rs
#[derive(Debug, Clone, PartialEq)]
pub struct ApplicationEffectInputs {
    pub app: ApplicationEffectFields,
    pub clash: ClashEffectFields,
    /// 由 SessionPortResolver 解析出的实际端口；启动早期可能为 None。
    pub ports: Option<ResolvedPortBindings>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApplicationEffectFields {
    pub enable_system_proxy: bool,
    pub system_proxy_bypass: String,
    pub enable_proxy_guard: bool,
    pub proxy_guard_interval: u64,
    pub pac_url: Option<url::Url>,
    pub enable_auto_launch: bool,
    pub hotkeys: Vec<String>,
    pub language: I18nLanguage,
    pub app_log_level: LoggingLevel,
    pub max_log_files: usize,
    pub tray_selector_mode: ProxiesSelectorMode,
    pub tray_menu_mode: TrayMenuMode,
    pub enable_tray_text: bool,
    pub enable_tray_traffic: bool,
    pub network_statistic_widget: NetworkStatisticWidgetConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClashEffectFields {
    pub enable_tun_mode: bool,
    pub tun_stack: TunStack,
    pub mixed_port: PortStrategy,
    pub socks_port: Option<PortStrategy>,
    pub http_port: Option<PortStrategy>,
    pub external_controller: ExternalControllerStrategy,
    pub enable_clash_fields: bool,
    pub clash_control_channel: ClashControlChannel,
    pub clash_ipc_disable_http_controller: bool,
}

impl ApplicationEffectInputs {
    pub fn project(
        app: &NyanpasuAppConfig,
        clash: &ClashConfig,
        ports: Option<ResolvedPortBindings>,
    ) -> Self;
}
```

`project` 是投影函数，不是 `From`，因为要显式接收第三个来源（session 解析出的端口）。session state（`PersistentState`，只含 `window_state`）**不参与**任何副作用，因此不进入输入。

### 3.2 计划与效果种类

每个效果项携带**完整的期望值**，不是增量。这是陈旧保护能够"后到的高 revision 直接覆盖"的前提；如果携带的是增量，跳过任何一项都会永久丢失该效果。

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EffectKind {
    Locale,          // 1 — 必须最先：tray 菜单文案由 rust_i18n 全局 locale 决定
    Logger,          // 2
    AutoLaunch,      // 3
    SystemProxy,     // 4 — 含 PAC 分支
    ProxyGuard,      // 5 — 必须在 SystemProxy 之后：guard 复用已应用的期望值
    Hotkeys,         // 6
    Widget,          // 7
    Tray,            // 8 — 必须最后：读取 locale / system proxy / tun 的最终期望
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApplicationEffect {
    Locale(I18nLanguage),
    Logger(LoggerDesired),          // { level: LoggingLevel, max_files: usize }
    AutoLaunch(bool),
    SystemProxy(SystemProxyDesired),
    ProxyGuard(ProxyGuardDesired),  // { enabled: bool, interval: Duration }
    Hotkeys(Vec<String>),
    Widget(NetworkStatisticWidgetConfig),
    Tray(TrayRefresh),              // Full | Part
}

impl ApplicationEffect { pub fn kind(&self) -> EffectKind; }

#[derive(Debug, Clone, PartialEq)]
pub struct SystemProxyDesired {
    pub enabled: bool,
    pub bypass: String,
    pub port: Option<u16>,        // ports.mixed_port；None 表示端口尚未解析
    pub pac_url: Option<url::Url>,
}
```

`TrayRefresh` 的判定规则直接对应 `feat.rs:424-428`，但改为**可以同时成立时取 Full**（原实现是 `else if`，导致 language+system_proxy 同时变化时只做 Full 而丢掉 part 语义；`update_systray` 内部末尾本就会调用 `update_part`，所以 Full 覆盖 Part 是安全的）：

- Full：`language` / `tray_menu_mode` / `tray_selector_mode` 任一变化。
- Part：否则 `enable_system_proxy` / `enable_tun_mode` / `enable_tray_text` / `enable_tray_traffic` 任一变化。

```rust
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ApplicationEffectPlan {
    effects: Vec<ApplicationEffect>,   // 已按 EffectKind 排序、去重
}

impl ApplicationEffectPlan {
    /// 增量：before == after 的字段不产生任何效果项。
    pub fn diff(before: &ApplicationEffectInputs, after: &ApplicationEffectInputs) -> Self;
    /// 全量：启动 reconcile 用，产生每一种效果的期望值（Tray 取 Full）。
    pub fn full(after: &ApplicationEffectInputs) -> Self;
    pub fn is_empty(&self) -> bool;
    pub fn effects(&self) -> &[ApplicationEffect];
}
```

**SystemProxy 的触发条件**是 `enable_system_proxy` / `system_proxy_bypass` / `pac_url` / `ports.mixed_port` 任一变化。端口来自 `ports`，这是"端口变化重新应用系统代理"这一验收项的实现点。

**ProxyGuard 与 SystemProxy 拆成两项**，因为 guard 的启停条件（`enable_proxy_guard`、`proxy_guard_interval`）与代理本身的期望值是两组独立字段：只改间隔不应该重设系统代理，只改 bypass 不应该重启 guard 定时器。

**连接中断（6e-4）不设效果项。** 该能力已在 `feat/pr6-proxies-updater-interruption` 上以事件驱动方式实现（`core/proxies.rs` 的 `State::select` 按 `BreakConnectionStrategy` 分支、`core_lifecycle/workflow.rs` 的 profile 切换分支），它由"选择代理 / 切换 profile"这类**动作**触发，不是由配置 diff 触发。在此加一个 `ConnectionInterruption` 变体只会与之重复。交接点见 §11。

### 3.3 效果状态与 revision

```rust
// client/effects/status.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct EffectRevision(u64);
impl EffectRevision { pub fn get(self) -> u64; }

#[derive(Debug, Clone, PartialEq)]
pub enum EffectHealth {
    Healthy,
    Degraded { code: &'static str, message: String, retryable: bool },
    /// 期望 revision 不高于已应用 revision，被更新的 reconcile 取代。
    Superseded,
    /// 平台或依赖不支持（例如 PAC 在无 Autoproxy 支持的平台）。
    Unsupported { code: &'static str },
}

#[derive(Debug, Clone, PartialEq)]
pub struct EffectStatus {
    pub kind: EffectKind,
    pub desired_revision: EffectRevision,
    pub applied_revision: EffectRevision,
    pub health: EffectHealth,
}
```

这与 roadmap §7.1 的 `EffectStatus { desired_revision, applied_revision, health }` 一致，只多了 `kind`（executor 要把多条状态合并上报，必须能区分来源）。

`EffectRevision` 与 typed actor 的 `version: u64` **不是同一个量**：后者是单域持久化版本，一次 patch 可能跨 app + clash 两个域产生两个不相关的版本号，无法构成全序。`EffectRevision` 是 facade 在提交完成后按全序分配的单调计数器（见 §4.3），语义是"第 N 次已提交的期望配置"。它不持久化，进程重启从 0 开始，启动时由 `ApplicationEffectPlan::full` 做一次全量 reconcile 兜底。

### 3.4 端口选择：一个分发缝 + 若干窄能力

roadmap §7.5 写的是"窄 `ApplicationEffectsPort`"，任务 6a/6b 又要求 SystemProxy 与 Hotkey 是**actor**。两者并不冲突，取法如下：

- **facade 只依赖一个 trait** `ApplicationEffectsPort`。这样 `NyanpasuClient` 不必为每种效果多持有一个字段，facade 层测试只 mock 一个对象；6a/6b/6e-3 每次只往 executor 里加装配，不改 facade 的依赖形状。
- **executor 内部按能力扇出**，每个下游都是窄的：actor 用 typed client（有生命周期与自有可变状态），无状态的 UI/日志用单方法或双方法 adapter trait。
- **不存在 `get_effect::<T>()` 之类的查找 API**，因此不构成 service locator。

```rust
// client/effects/ports.rs
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait ApplicationEffectsPort: Send + Sync + 'static {
    /// 按 plan 内既定顺序串行执行，逐项返回状态。执行方负责 revision 过期判定。
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus>;

    /// 退出路径：恢复进入应用前的系统状态并注销 OS 注册。永不返回 Err。
    async fn shutdown(&self) -> Vec<EffectStatus>;
}
```

`apply` 不返回 `Result`：提交已经发生，任何失败都必须以 `EffectStatus::health` 呈现而不是错误。

6e-1 只交付 trait 定义 + `MockApplicationEffectsPort` + 一个 `NoopApplicationEffects`；具体 executor 在 6e-2 落地空壳，6a/6b/6e-3 逐项填充。

### 3.5 提交前错误 vs 提交后降级

| 类别                                       | 判定时机                                             | 结果                                                                     |
| ------------------------------------------ | ---------------------------------------------------- | ------------------------------------------------------------------------ |
| `theme_color` 非法 hex                     | typed commit 之前                                    | `Err(ClientError)`，不提交、不执行任何副作用                             |
| 快捷键语法非法 / 缺少 super key / 组内重复 | typed commit 之前，由纯 `HotkeyBindings::parse` 完成 | `Err(ClientError)`                                                       |
| 固定端口被占用                             | typed commit 之前（沿用 `port_scanner` 预检）        | `Err(ClientError)`                                                       |
| OS 设置系统代理失败                        | commit 之后                                          | `Degradation { phase: SystemEffect, code: "system_proxy_apply_failed" }` |
| PAC 下载/校验失败                          | commit 之后                                          | `SystemEffect` / `pac_apply_failed`，并回落到直连代理                    |
| 自启动写入失败                             | commit 之后                                          | `SystemEffect` / `auto_launch_failed`                                    |
| 快捷键部分注册失败                         | commit 之后                                          | `SystemEffect` / `hotkey_partial_registration`                           |
| locale / tray / logger / widget 失败       | commit 之后                                          | `UiEffect` / 各自 code                                                   |

phase 映射固定为：`Locale`/`Tray`/`Logger`/`Widget` → `UiEffect`；`SystemProxy`/`ProxyGuard`/`AutoLaunch`/`Hotkeys` → `SystemEffect`。`Superseded` 与 `Unsupported` **不产生** `Degradation`（前者是正常的并发结果，后者是平台事实），只写 `tracing::debug!`。

映射函数放在 `client/effects/status.rs`：

```rust
pub fn degradation_of(status: &EffectStatus) -> Option<Degradation>;
```

`DegradationPhase` 不新增变体 —— `specta_export.rs:373-389` 固定了十个 tag，新增会改 wire。

### 3.6 runtime apply 分类

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeApplyKind { None, Rebuild, ControlChannel }

pub fn runtime_apply_kind(
    before: &ApplicationEffectInputs,
    after: &ApplicationEffectInputs,
) -> RuntimeApplyKind;
```

- `ControlChannel`：`clash_control_channel` 或 `clash_ipc_disable_http_controller` 变化（优先级最高，与 `bridge/verge.rs:295-297,336-339` 现行判定一致）。
- `Rebuild`：否则 `enable_tun_mode` / `tun_stack` / `mixed_port` / `socks_port` / `http_port` / `external_controller` / `enable_clash_fields` 任一变化。
- `None`：其余。

`core`（换核心）与 `enable_service_mode`（换执行宿主）**不在此列**，它们有专用 facade 操作（`update_core` / `set_execution_host`），重复触发会与 `CoreLifecycleActor` 的操作队列打架。

---

## 4. 任务 6e-2 — typed 配置提交与 reconcile 入口

### 4.1 字段归属

`bridge/mapping.rs:21` 的 `IVERGE_FIELD_MAPPING` 已有 owner 元数据，但文件头明确写着"Phase 0 ownership metadata only, must not be used as production conversion logic"，真正的转换在 `bridge/mod.rs:56/119/135`。本设计不复用那张表，而是以 typed 结构体的实际归属为准：

| 域          | typed 类型            | 本设计关心的字段                                                                                                            | 触发                                        |
| ----------- | --------------------- | --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- |
| Application | `NyanpasuAppConfig`   | `enable_system_proxy`、`system_proxy_bypass`、`enable_proxy_guard`、`proxy_guard_interval`、`pac_url`、`enable_auto_launch` | 外围 effects                                |
| Application | 同上                  | `hotkeys`                                                                                                                   | 外围 effects（6b 起成为唯一权威，见 §6.1）  |
| Application | 同上                  | `language`、`tray_menu_mode`、`tray_selector_mode`、`enable_tray_text`、`enable_tray_traffic`                               | 外围 effects                                |
| Application | 同上                  | `app_log_level`、`max_log_files`                                                                                            | 外围 effects                                |
| Application | 同上                  | `network_statistic_widget`                                                                                                  | 外围 effects                                |
| Application | 同上                  | `core`、`enable_service_mode`                                                                                               | 专用 facade 操作，**不进 plan**             |
| Session     | `PersistentState`     | `window_state`                                                                                                              | 无副作用                                    |
| Clash       | `ClashConfig`         | `enable_tun_mode`、`tun_stack`、`mixed_port`、`socks_port`、`http_port`、`external_controller`、`enable_clash_fields`       | runtime apply（`Rebuild`）                  |
| Clash       | 同上                  | `clash_control_channel`、`clash_ipc_disable_http_controller`                                                                | runtime apply（`ControlChannel`）           |
| Clash       | `ClashGuardOverrides` | `mode` / `allow_lan` / `log_level` / `ipv6` / `secret`                                                                      | 走 `patch_runtime_overrides`，**不进 plan** |

### 4.2 统一 mutation 管线

```rust
// client/effects/mod.rs（impl NyanpasuClient）
async fn commit_and_reconcile<F, Fut>(&self, commit: F)
    -> Result<runtime::MutationOutcome<()>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let gate = self.inner.effects.gate().await;        // ① 取门
    let before = self.effect_inputs().await?;          // ② 采样 before
    commit().await?;                                   // ③ typed commit（失败直接 Err，不执行任何副作用）
    let revision = gate.allocate();                    // ④ 分配单调 EffectRevision
    let mut degradations = Vec::new();
    match plan::runtime_apply_kind(&before, &self.effect_config_only().await?) {
        RuntimeApplyKind::None => {}
        RuntimeApplyKind::Rebuild => { /* rebuild_running_config，失败 → RuntimeBuild 降级 */ }
        RuntimeApplyKind::ControlChannel => { /* apply_control_channel，同上 */ }
    }
    let after = self.effect_inputs().await?;           // ⑤ rebuild 之后再采样，端口才是新的
    let plan = ApplicationEffectPlan::diff(&before, &after);
    drop(gate);                                        // ⑥ 释放门后再跑外围 effects
    let statuses = self.inner.effects.port().apply(revision, plan).await;
    degradations.extend(statuses.iter().filter_map(status::degradation_of));
    Ok(runtime::MutationOutcome::from_parts((), degradations))
}
```

顺序理由：

- **runtime apply 在外围 effects 之前**。系统代理的端口来自 `SessionPortResolver`，而端口重新解析发生在 rebuild/reconcile 过程中（`client/ports.rs` 按 `PortsFingerprint` 失效）。若先跑外围 effects，系统代理会被设成旧端口，下一次才纠正。
- **`after` 在 runtime apply 之后采样**，因此 `SystemProxyDesired.port` 一定是核心实际监听的那个。
- **门在 dispatch 之前释放**。`apply` 里可能有 PAC 的远端下载（`core/pac.rs:11-13`，30s 超时 ×3 次重试），把它压在门内会让窗口拖拽保存（`ipc.rs:1288` 的 `save_window_size_state` 每次移动都要跑一遍 mutation）排在后面。

`rebuild_running_config` 失败映射为已有的 `Self::map_runtime_rebuild_degradation`（`client/mod.rs:951`，`phase = RuntimeBuild`，`retryable = true`），不新造 code。

### 4.3 陈旧保护与"不重复提交"

**第一层：`ApplicationEffectGate`。**

```rust
// client/effects/mod.rs
pub(crate) struct ApplicationEffects {
    // 窄作用域实现细节：串行化「采样 before → 提交 → runtime apply → 采样 after →
    // 分配 revision」，保证 (before, after) 成对一致且 revision 的分配顺序等于提交顺序。
    // 这是 LegacyVergeBridgeInner::verge_update_lock（bridge/verge.rs:34）的迁移落点，
    // 不是新增的共享可变状态：actor 自有状态仍然只在各自 actor 内。
    gate: tokio::sync::Mutex<GateState>,
    port: Arc<dyn ApplicationEffectsPort>,
}
struct GateState { next_revision: u64 }
```

因为门覆盖了提交与 revision 分配，两个并发 mutation 的 `(commit_order, revision_order)` 必然一致，不会出现"后提交的拿到更小 revision"。

**第二层：每个 effect owner 自持 `applied_revision`。**

`SystemProxyActor` / `HotkeyActor` 把 `applied_revision: EffectRevision` 放在 actor state 里；executor 为四个无状态 adapter 各持一个 `applied: parking_lot::Mutex<BTreeMap<EffectKind, EffectRevision>>`。收到 `revision <= applied_revision` 的请求时返回 `EffectHealth::Superseded` 且不执行。

这一层保护的是**门以外的 reconcile 入口**：启动全量 reconcile（6a/6e-5）、退出恢复、以及将来 6e-5 可能加入的定时 reconcile。它们不经过门，只有比较 revision 才能保证不把旧期望写回去。

**不重复提交。** 三条提交入口最终都只落在一个提交点：

| 入口                                                              | 提交点                                                                       | reconcile 挂载点                        |
| ----------------------------------------------------------------- | ---------------------------------------------------------------------------- | --------------------------------------- |
| `patch_app_config` / `patch_clash_config` / `patch_session_state` | 对应 typed actor 的 `Patch` 消息                                             | 各自方法内包一层 `commit_and_reconcile` |
| `patch_verge_config` 的 `PureConfig` 路由                         | `apply_legacy_verge_patch_saga`                                              | saga 内部包一层                         |
| `patch_verge_config` 的 `LegacySideEffects` 路由                  | 同上（`run_legacy_verge_mutation` 先回滚 legacy 文件，再经同一个 saga 提交） | 同上                                    |

因此 reconcile 只挂在 **typed client patch** 与 **三域 saga** 两处，而这两处不会互相嵌套：`apply_legacy_verge_patch_saga` 走 `prepare_replace` + `replace_prepared_if_version`，不经过 `ApplicationClient::patch`。

**不重复 reconcile。** 6e-2 落地时 executor 的效果集合是**空的**（所有 `ApplicationEffect` 变体都由 `NoopApplicationEffects` 消化），`feat::patch_verge` 里的副作用一行未动，因此行为完全不变。此后 6a / 6b / 6e-3 每迁移一项，必须在**同一个 commit 内**完成三件事：

1. 在 executor 里装配该效果的真实执行方；
2. 从 `feat::patch_verge`（`feat.rs:368-452`）删除对应代码块；
3. 从 `route_verge_patch`（`bridge/verge.rs:491-513`）的字段清单里删除对应字段，让该字段走 `PureConfig` 路由。

这三件事绑定成原子提交，就不存在"两处都执行"或"两处都不执行"的中间窗口。

同时，`run_legacy_verge_mutation` 中的 `reconcile_tun` 块（`bridge/verge.rs:328-347`）在 6e-2 内删除，改由 saga 挂载的 `runtime_apply_kind` 统一判定 —— 否则 TUN 变化会 rebuild 两次。

### 4.4 facade API 变更

```rust
impl NyanpasuClient {
    pub async fn patch_app_config(&self, patch: NyanpasuAppConfigPatch)
        -> Result<runtime::MutationOutcome<()>>;            // 原 Result<()>
    pub async fn patch_clash_config(&self, patch: ClashConfigPatch)
        -> Result<runtime::MutationOutcome<()>>;            // 原 Result<()>
    pub async fn patch_session_state(&self, patch: PersistentStatePatch)
        -> Result<runtime::MutationOutcome<()>>;            // 原 Result<()>；plan 恒为空
    /// 启动/兜底全量 reconcile，不依赖 before。
    pub async fn reconcile_application_effects(&self)
        -> Result<runtime::MutationOutcome<()>>;
    /// 退出路径；恢复原系统代理、注销快捷键、停 widget。
    pub async fn shutdown_application_effects(&self) -> Vec<runtime::Degradation>;
}
```

`replace_*` 三个方法同样改成返回 `MutationOutcome<()>`（`bridge` 的 `replace_verge_config` 会用到）。

### 4.5 IPC wire 变更（决定：**现在就改**）

`ipc.rs:492` 的 `patch_verge_config` 返回类型从 `Result<()>` 改为 `Result<MutationOutcome<()>>`。

理由：6a 的"失败降级"、6b 的"部分注册失败"、6e-3 的"各项失败能分别报告"这三条验收项，如果 wire 不带 degradations 就只能靠日志断言，无法端到端验证。`MutationOutcome<null>` 的 TS 类型已经存在于 `bindings.ts:1486`（`patchClashConfig` 等 13 个命令在用），`specta_export.rs:392-400` 也已经固定了 `MutationOutcome<null>` 必须出现，因此这是零新增 wire 概念的改动。

前端改动范围（最小）：

- `frontend/interface/src/ipc/bindings.ts` — 由 `cargo test export_typescript_bindings` 重新生成。
- `frontend/interface/src/ipc/use-settings.ts:70-85` — `mutationFn` 的返回类型从 `null` 变为 `MutationOutcome<null>`；沿用 profiles 侧既有的 degradation 处理方式（committed-degraded 仍算 mutation 成功）。
- 其余 47 处 `useSetting(...)` 调用点不需要改（它们只 `await upsert(value)`）。

`ipc.rs:1068` 的 `set_hotkeys` 在 6b 中一并改为返回 `MutationOutcome<()>`。

---

## 5. 任务 6a — SystemProxyActor

### 5.1 消息、状态、启动参数

```rust
// client/system_proxy/actor.rs
pub(super) enum Message {
    Reconcile {
        revision: EffectRevision,
        proxy: Option<SystemProxyDesired>,   // None = 本次不动系统代理
        guard: Option<ProxyGuardDesired>,
        auto_launch: Option<bool>,
        reply: RpcReplyPort<Vec<EffectStatus>>,
    },
    Status(RpcReplyPort<SystemProxyStatus>),
    /// 退出恢复：把系统代理恢复成本进程启动前的值。
    Restore(RpcReplyPort<EffectStatus>),
    /// guard 定时器投递；也可由测试直接投递以避免 sleep。
    GuardTick,
}

pub(super) struct Args {
    pub os: Arc<dyn OsProxyPort>,
    pub auto_launch: Arc<dyn AutoLaunchPort>,
    pub pac: Arc<dyn PacPort>,
    /// true 时用 myself.send_interval 建真实定时器；测试传 false 并手工投递 GuardTick。
    pub schedule_guard_ticks: bool,
}

pub(super) struct State {
    os: Arc<dyn OsProxyPort>,
    auto_launch: Arc<dyn AutoLaunchPort>,
    pac: Arc<dyn PacPort>,
    schedule_guard_ticks: bool,
    applied_revision: EffectRevision,
    /// 首次启用系统代理前抓到的 OS 状态，退出时恢复它。只写一次。
    original: Option<OsProxyConfig>,
    /// 本进程最后一次实际写入 OS 的值，guard tick 复用它。
    current: Option<OsProxyConfig>,
    pac_active: bool,
    guard: Option<ProxyGuardDesired>,
    guard_job: Option<ractor::concurrency::JobHandle<()>>,
}
```

一次 `Reconcile` 可能同时携带三类期望（一个 plan 里 `AutoLaunch` / `SystemProxy` / `ProxyGuard` 三项都命中同一个 actor），executor 把它们合并成一条消息发送，避免三次往返。

### 5.2 端口

```rust
// client/system_proxy/ports.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsProxyConfig { pub enable: bool, pub host: String, pub port: u16, pub bypass: String }

#[cfg_attr(test, mockall::automock)]
pub trait OsProxyPort: Send + Sync + 'static {
    fn get(&self) -> anyhow::Result<OsProxyConfig>;
    fn set(&self, config: &OsProxyConfig) -> anyhow::Result<()>;
    fn default_bypass(&self) -> &'static str;
}

#[cfg_attr(test, mockall::automock)]
pub trait AutoLaunchPort: Send + Sync + 'static {
    fn is_enabled(&self) -> anyhow::Result<bool>;
    fn set_enabled(&self, enabled: bool) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait PacPort: Send + Sync + 'static {
    fn is_supported(&self) -> bool;
    /// 下载 → 校验 → 落盘缓存 → 设置 OS autoproxy。失败由调用方决定回落。
    async fn apply(&self, url: &url::Url) -> anyhow::Result<()>;
    fn disable(&self) -> anyhow::Result<()>;
}
```

具体实现放 `client/system_proxy/adapters.rs`：

- `SysproxyOsProxy` 包 `sysproxy::Sysproxy`，`default_bypass` 保留 `core/sysopt.rs:33-39` 的三份平台常量（`core/pac.rs:170-175` 里那份重复的副本随 `pac.rs` 一起删除）。
- `AutoLaunchBackend` 包 `auto_launch::AutoLaunchBuilder`。构造参数 `AutoLaunchConfig { app_name: String, app_path: String, appimage: Option<String> }` **在组合根解析**：`setup.rs` 有 `app_handle`，可以直接取 `app_handle.env().appimage`，从而删掉 `core/sysopt.rs:256-267` 里唯一一处 `Handle::global()`。macOS 的"先 disable 再 enable 防重复登录项"逻辑原样搬进 adapter。
- `HttpPacBackend { client: reqwest::Client, cache_path: Utf8PathBuf }`，`cache_path` 由 `PathResolver` 提供（`utils/dirs.rs:223` 的 `cache_dir()/pac.js`），从而满足 AGENTS §1.6"测试不碰真实用户目录"。

所有 `fn` 都是同步阻塞（`sysproxy` / `auto_launch` 是阻塞调用），actor 内部用 `tokio::task::spawn_blocking` 包一层再 await，不阻塞 actor 邮箱。

### 5.3 typed client

```rust
// client/system_proxy/mod.rs
#[derive(Clone)]
pub(crate) struct SystemProxyClient(Arc<ClientInner>);

impl SystemProxyClient {
    pub(crate) async fn spawn(args: Args) -> anyhow::Result<Self>;
    pub(crate) async fn reconcile(
        &self,
        revision: EffectRevision,
        proxy: Option<SystemProxyDesired>,
        guard: Option<ProxyGuardDesired>,
        auto_launch: Option<bool>,
    ) -> Vec<EffectStatus>;
    pub(crate) async fn status(&self) -> SystemProxyStatus;
    pub(crate) async fn restore(&self) -> EffectStatus;
    #[cfg(test)]
    pub(crate) async fn tick_guard(&self);
}
```

`reconcile` / `status` 用有限超时（`SYSTEM_PROXY_RPC_TIMEOUT = Duration::from_secs(15)`，覆盖 PAC 下载 30s 里的第一次尝试之外的场景；超时返回 `Degraded { code: "system_proxy_timeout", retryable: true }`，actor 继续跑完并更新自己的 `applied_revision`）。`restore` 用更短的 5s，退出路径不能吊死。

### 5.4 guard 语义（修掉现有三个缺陷）

| 现状                                                                    | 位置                                 | 新语义                                                                 |
| ----------------------------------------------------------------------- | ------------------------------------ | ---------------------------------------------------------------------- |
| 首次固定 sleep 10s，间隔改动滞后一拍                                    | `core/sysopt.rs:361,383`             | `Reconcile` 收到新 `interval` 时立即 `abort` 旧 job 并按新间隔重建     |
| 无取消句柄，关掉 guard 后最多还要跑满一个周期                           | `core/sysopt.rs:378-380`             | `guard_job: Option<JobHandle>`，关闭即 `abort()`；`post_stop` 也 abort |
| PAC 模式下 guard 仍每 tick 写 `set_system_proxy`，与 autoproxy 互相打架 | `core/sysopt.rs:74-87` vs `:394-401` | `pac_active == true` 时 `GuardTick` 直接 return                        |

`GuardTick` 的动作：若 `current` 为 `Some(cfg)` 且 `cfg.enable`，则 `os.set(&cfg)`。**不重新读配置**（期望值已经在 state 里），也**不因为用户手动关闭系统代理就停止 guard** —— 这正是 guard 的语义；用户要停 guard 必须关 `enable_proxy_guard`，这与现状一致。

### 5.5 退出恢复

`Restore` 的判定沿用 `core/sysopt.rs:183-203`：

- `pac_active` → `pac.disable()`；
- 有 `original` 且 `original.port != current.port` → 写回 `original`；
- 否则若 `current.enable` → 写 `current` 的 disable 版本；
- 无 `current` → 无操作。

调用点：`utils/help.rs:243-269` 的 `cleanup_processes` 中，把 `super::resolve::resolve_reset()` 替换为在已有的 `block_on` 块里 `client.shutdown_application_effects().await`（与 `shutdown_logs` / `shutdown_core` 并列）。`utils/resolve.rs:276-279` 的 `resolve_reset` 随之删除。

6e-5 拥有完整的启动/退出接线（面板恢复、窗口状态、panic 路径），6a 只做"删掉 `Sysopt::global()` 所必需的最小迁移"：`resolve.rs:232-233` 的两行 `init_launch`/`init_sysproxy` 换成一次 `client.reconcile_application_effects()`，`help.rs` 的退出恢复换成上面那行。

### 5.6 6a 测试矩阵（全部 mockall fake，无 sleep，无真实 OS 调用）

| 用例                                            | 断言                                                                                       |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `enabling_sets_os_proxy_with_resolved_port`     | `MockOsProxyPort::set` 收到 `{enable:true, port:<mixed>, bypass}`                          |
| `disabling_restores_and_stops_guard`            | `set` 收到 `enable:false`；`status().guard_active == false`                                |
| `port_change_reapplies_system_proxy`            | 仅端口变化时 `set` 被再次调用且端口是新的                                                  |
| `pac_enabled_takes_over_and_skips_plain_proxy`  | `PacPort::apply` 被调用，`OsProxyPort::set` 未被调用                                       |
| `pac_failure_falls_back_to_direct_and_degrades` | `set` 被调用；返回 `Degraded{code:"pac_apply_failed"}`                                     |
| `pac_unsupported_platform_reports_unsupported`  | `is_supported()==false` → `Unsupported`，不产生 `Degradation`                              |
| `guard_interval_change_rebuilds_timer`          | 两次 `Status` 的 `guard_interval` 不同且 `guard_active` 保持                               |
| `guard_tick_reapplies_last_desired`             | 手工 `tick_guard()` 后 `set` 调用次数 +1，参数等于最后期望                                 |
| `guard_tick_is_suppressed_while_pac_active`     | `tick_guard()` 后 `set` 调用次数不变                                                       |
| `exit_restores_captured_original`               | `Restore` 后 `set` 收到启动前抓到的值                                                      |
| `set_failure_degrades_and_keeps_desired`        | `set` 返回 Err → `Degraded{code:"system_proxy_apply_failed"}`；`status().desired` 仍是新值 |
| `stale_revision_is_superseded`                  | 先发 rev=5 再发 rev=3 → 第二次 `health == Superseded` 且 `set` 未被调用                    |
| `auto_launch_failure_degrades_independently`    | 只有 AutoLaunch 项降级，SystemProxy 项仍 `Healthy`                                         |

---

## 6. 任务 6b — HotkeyActor

### 6.1 权威来源：typed config（需要一次反向迁移）

现状是**双源**：`ipc.rs:1068` 的 `set_hotkeys` 写 KV `Storage["hotkeys"]`，`patch_verge_config({hotkeys})` 写 `NyanpasuAppConfig.hotkeys`，`core/hotkey.rs:186-204` 启动时先读 KV、读不到再回落 `Config::verge().latest().hotkeys`。而 2.0.0 的迁移 `storage/hotkeys_to_kv`（`core/migration/modules/storage.rs:51`）把 legacy yaml 里的 hotkeys 搬去了 KV 并从 yaml 删除。

**决策：以 typed `NyanpasuAppConfig.hotkeys` 为唯一权威。** 依据：任务卡"初始值来自 typed config"；hotkeys 必须成为 6e-1 plan 的输入才能享受统一的 revision / degradation 协议；KV `Storage` 的 `"hotkeys"` 键没有命名空间前缀、没有版本号、没有 CAS，无法参与三域 saga。

代价是需要新增一个迁移步骤：

```text
core/migration/modules/storage.rs
  static HOTKEYS_TO_TYPED_CONFIG: MigrateHotkeysToTypedConfig;   // revision 2
```

它读 KV `"hotkeys"`，写入 `application.yaml` 的 `hotkeys`，然后删除 KV 键；`detect_baseline` 在 KV 仍存在该键时返回 1。同时把 `core/migration/runner.rs:444-448` 那条 `application.hotkeys.is_empty()` 断言改成断言迁移后的值。

**备选方案（若维护者否决反向迁移）：** HotkeyActor 改为注入一个 `HotkeyStore` 端口（KV 实现），hotkeys 退出 `ApplicationEffectInputs`，`set_hotkeys` 直接打到 `HotkeyClient`。其余设计不变。这个分歧点必须在 6b 开工前确认。

### 6.2 消息、状态、端口

```rust
// client/hotkey/actor.rs
pub(super) enum Message {
    Reconcile { revision: EffectRevision, desired: HotkeyBindings, reply: RpcReplyPort<EffectStatus> },
    Status(RpcReplyPort<HotkeyStatus>),
    UnregisterAll(RpcReplyPort<EffectStatus>),
}

pub(super) struct State {
    registrar: Arc<dyn ShortcutRegistrar>,
    sink: Arc<dyn HotkeyActionSink>,
    applied_revision: EffectRevision,
    /// accelerator -> action，只记录「OS 确认注册成功」的条目。
    registered: BTreeMap<String, HotkeyAction>,
}
```

```rust
// client/hotkey/ports.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyAction {
    OpenOrCloseDashboard,
    ClashModeRule, ClashModeGlobal, ClashModeDirect, ClashModeScript,
    ToggleSystemProxy, EnableSystemProxy, DisableSystemProxy,
    ToggleTunMode, EnableTunMode, DisableTunMode,
}
impl HotkeyAction {
    pub fn all() -> &'static [HotkeyAction];
    pub fn as_str(&self) -> &'static str;      // 与 core/hotkey.rs:86-100 的 11 个字符串一字不差
}
impl std::str::FromStr for HotkeyAction { type Err = HotkeyParseError; }

/// 纯服务：解析 + 校验 `"{func},{key}"` 列表。无 OS 依赖，单测覆盖全部失败分支。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotkeyBindings(BTreeMap<String, HotkeyAction>);   // accelerator -> action
impl HotkeyBindings {
    pub fn parse(raw: &[String]) -> Result<Self, HotkeyParseError>;
    pub fn diff(&self, next: &Self) -> Vec<HotkeyOp>;         // Bind / Unbind / Rebind
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HotkeyParseError {
    MalformedEntry(String),          // 不是 "func,key"
    UnknownFunction(String),
    InvalidAccelerator(String),      // Shortcut::from_str 失败
    MissingSuperKey(String),
    DuplicateAccelerator(String),    // 同一组合绑定到两个 func
}

#[cfg_attr(test, mockall::automock)]
pub trait ShortcutRegistrar: Send + Sync + 'static {
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError>;
    fn register(&self, accelerator: &str, action: HotkeyAction, sink: Arc<dyn HotkeyActionSink>)
        -> anyhow::Result<()>;
    fn unregister(&self, accelerator: &str) -> anyhow::Result<()>;
    fn unregister_all(&self) -> anyhow::Result<()>;
}

#[cfg_attr(test, mockall::automock)]
pub trait HotkeyActionSink: Send + Sync + 'static {
    fn dispatch(&self, action: HotkeyAction);
}
```

`TauriShortcutRegistrar<R>`（`adapters.rs`）包 `tauri_plugin_global_shortcut::GlobalShortcutExt`：`validate` 复刻 `core/hotkey.rs:230-249` 的两步校验（`parse::<Shortcut>()` + `SUPER_KEYS` 检查，注意现行实现是**小写子串匹配**，保留原行为并在单测里钉住），`register` 在注册前对已注册的同一组合先 `unregister`（保持 `core/hotkey.rs:258-260` 的 last-writer-wins）。

`HotkeyParseError` 复用 `core/hotkey.rs:31` 已有的 i18n 键（`hotkey_error.invalid_hotkey` / `hotkey_error.missing_super_key`），并补两条新键。原 `HotkeyError` 类型从未被构造过，随 `core/hotkey.rs` 一起删除。

### 6.3 回调如何调用 facade 而不产生依赖环

`HotkeyActor` 被 `NyanpasuClient` 持有，回调又要调 `NyanpasuClient` —— 直接持有会成环。解法是**在组合根开一条 mpsc 单向通道**：

```rust
// client/hotkey/adapters.rs
pub struct ChannelActionSink(tokio::sync::mpsc::UnboundedSender<HotkeyAction>);
impl HotkeyActionSink for ChannelActionSink {
    fn dispatch(&self, action: HotkeyAction) { let _ = self.0.send(action); }
}
```

`setup.rs` 先建 channel、把 `ChannelActionSink` 放进 `ClientSetupArgs`，client 构造完成后 spawn 一个 pump：

```rust
tauri::async_runtime::spawn(async move {
    while let Some(action) = rx.recv().await {
        if let Err(error) = client.dispatch_hotkey_action(action).await {
            tracing::warn!(%error, ?action, "hotkey action failed");
        }
    }
});
```

pump 是串行的，所以连按快捷键不会并发打进 facade。

facade 侧的映射（`client/hotkey/mod.rs` 的 `impl NyanpasuClient`）：

| `HotkeyAction`              | facade 调用                                                                            |
| --------------------------- | -------------------------------------------------------------------------------------- |
| `OpenOrCloseDashboard`      | `self.inner.window.toggle_dashboard()`（新 `WindowControl` 端口）                      |
| `ClashMode*`                | `self.patch_runtime_overrides(ClashGuardOverridesPatch { mode: Some(..), ..Default })` |
| `ToggleSystemProxy`         | 读 `get_app_config().enable_system_proxy`，`patch_app_config` 取反                     |
| `Enable/DisableSystemProxy` | `patch_app_config({ enable_system_proxy: Some(v) })`                                   |
| `ToggleTunMode`             | 读 `get_clash_config().enable_tun_mode`，`patch_clash_config` 取反                     |
| `Enable/DisableTunMode`     | `patch_clash_config({ enable_tun_mode: Some(v) })`                                     |

```rust
// client/hotkey/ports.rs
#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait WindowControl: Send + Sync + 'static {
    async fn toggle_dashboard(&self) -> anyhow::Result<()>;
}
```

`TauriWindowControl<R>` 包 `resolve::{is_window_open, create_window, close_window}`（`utils/resolve.rs:492-505`），并在 adapter 内部负责主线程调度。这样 `feat::{toggle_dashboard, open_dashboard, close_dashboard, toggle_system_proxy, enable_system_proxy, disable_system_proxy, toggle_tun_mode, enable_tun_mode, disable_tun_mode}` 九个函数在 6b 内全部删除，tray 菜单（`core/tray/mod.rs:432-461`）与 hotkey 回调改调 facade。

> `TauriWindowControl` 内部仍可能触及 `WindowManager::global()`（`window.rs`）。那是 adapter 内的既有实现细节，不属于本阶段范围，由 PR-7 / 6e-5 清算，不新增全局。

### 6.4 更新语义与失败模型

`Reconcile` 的执行顺序：

1. `HotkeyBindings::diff(&state.registered, &desired)` 得到 `Unbind` / `Rebind` / `Bind` 三类操作；
2. 先执行全部 `Unbind` 与 `Rebind` 的注销半边，再执行注册半边（避免"先注册新的再注销旧的"导致同组合冲突）；
3. 每条注册成功才写进 `state.registered`，失败只记一条 `failures`；
4. 全成功 → `Healthy`；部分失败 → `Degraded { code: "hotkey_partial_registration", message: "<n> of <m> shortcuts failed: ..." , retryable: true }`；
5. 无论结果如何，`applied_revision = revision`（期望已被消费）。

语法层错误（`HotkeyParseError`）在 **commit 之前**由 facade 调用 `HotkeyBindings::parse` 拦下，返回 `Err`，配置不落盘。这修掉了现状里"`init` 用 `log_err!` 吞掉每条失败但仍把整份列表写进 `current`"（`core/hotkey.rs:214-223`）造成的状态失真。

`UnregisterAll` 在退出路径由 `shutdown_application_effects` 调用，替代现有的 `impl Drop for Hotkey`（`core/hotkey.rs:363`）。

### 6.5 IPC 迁移

- `ipc.rs:1060` `get_hotkeys` → `client.get_app_config().await?.hotkeys`（不再读 KV）。
- `ipc.rs:1068` `set_hotkeys` → `client.patch_app_config({ hotkeys: Some(hotkeys) })`，返回 `MutationOutcome<()>`。`HOTKEYS_KEY` 常量与 `Storage` 读写删除。
- `ipc.rs:486` `get_hotkey_functions` → `HotkeyAction::all().iter().map(HotkeyAction::as_str).collect()`，签名与返回值不变，前端 `use-hotkey-functions.ts` 不动。

### 6.6 6b 测试矩阵

| 用例                                                         | 断言                                                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------- |
| `parse_rejects_malformed_unknown_invalid_and_missing_super`  | 四类 `HotkeyParseError` 各一条，纯函数断言                                            |
| `parse_rejects_duplicate_accelerator`                        | 同组合绑两个 func → `DuplicateAccelerator`                                            |
| `invalid_hotkey_is_rejected_before_commit`                   | facade 返回 `Err`，`get_app_config().hotkeys` 未变                                    |
| `update_unregisters_before_registering`                      | `MockShortcutRegistrar` 的调用序列断言（`unregister` 全部先于 `register`）            |
| `partial_registration_failure_degrades_and_keeps_successes`  | 一条失败 → `Degraded`，`status().registered` 含成功的那条、不含失败的                 |
| `unchanged_bindings_produce_no_os_calls`                     | 同一份列表重复 reconcile → registrar 零调用                                           |
| `stale_revision_is_superseded`                               | 低 revision 不触碰 registrar                                                          |
| `exit_unregisters_all`                                       | `unregister_all` 被调用一次                                                           |
| `callback_dispatches_action_to_sink`                         | 触发注册回调 → `MockHotkeyActionSink::dispatch` 收到对应 action                       |
| `dispatch_toggle_system_proxy_flips_app_config`（facade 测） | 用 `test_client_args_with_endpoint` 建 client，dispatch 后 `enable_system_proxy` 取反 |

---

## 7. 任务 6e-3 — UI 与日志副作用

### 7.1 四个窄 adapter

```rust
// client/ui_effects/ports.rs
#[cfg_attr(test, mockall::automock)]
pub trait LocaleSink: Send + Sync + 'static {
    fn set_locale(&self, language: I18nLanguage) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait TrayRefresher: Send + Sync + 'static {
    async fn refresh_full(&self) -> anyhow::Result<()>;
    async fn refresh_part(&self) -> anyhow::Result<()>;
}

#[cfg_attr(test, mockall::automock)]
pub trait LoggerRefresher: Send + Sync + 'static {
    fn refresh(&self, level: Option<LoggingLevel>, max_files: Option<usize>) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
#[cfg_attr(test, mockall::automock)]
pub trait WidgetController: Send + Sync + 'static {
    async fn apply(&self, config: NetworkStatisticWidgetConfig) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
}
```

具体实现（`client/ui_effects/adapters.rs`）：

- **`RustI18nLocaleSink`** — 直接调 `rust_i18n::set_locale(language.as_str())`。这是 crate 级进程全局设置，无法注入；把它关在一个单方法 adapter 后面是唯一诚实的做法（AGENTS §8"adapter 负责基础设施"）。`set_locale` 返回 `()`，adapter 恒返回 `Ok(())`，但保留 `Result` 以免将来换 i18n 实现时改签名。
- **`TauriTrayRefresher<R>`** — `refresh_full` 走 `app_handle.emit("update_systray", ())`（由 `utils/resolve.rs:213-223` 的监听器在主线程上跑 `Tray::update_systray`，这是 GTK 主线程约束的既有解法）；`refresh_part` 走 `Tray::update_part(&app_handle)`。**主线程调度留在 adapter 内，业务层不 import Tauri。**
- **`TracingLoggerRefresher`** — 包 `crate::utils::init::refresh_logger((level, max_files))`（`utils/init/logging.rs:33`）。
- **`TauriWidgetController`** — 见 §7.2。

> `UiEventSink`（`client/event_sink.rs`）**不动**。它的 `update_systray_part` 仍被 `core_lifecycle/workflow.rs:192` 用于 reconcile 之后的 tray 刷新，属于 core 生命周期自己的 UI 通知路径。与 `TrayRefresher` 的六行重复是有意保留的，统一放在 PR-7 —— 现在合并会改到另外两条 PR-6 分支正在动的 `core_lifecycle/workflow.rs`。

### 7.2 Widget 的晚绑定

`WidgetManager` 需要 `client.subscribe_clash_connections()` 才能建（`utils/resolve.rs:196-202`），而 client 又要在构造时拿到 `WidgetController` —— 顺序相反。解法是注入一个**由组合根拥有的一次性填充句柄**，不是全局：

```rust
// client/ui_effects/adapters.rs
#[derive(Default)]
pub struct TauriWidgetController {
    manager: tokio::sync::OnceCell<crate::widget::WidgetManager>,
}
impl TauriWidgetController {
    /// 组合根在 widget::setup 完成后调用一次。
    pub fn install(&self, manager: crate::widget::WidgetManager) -> anyhow::Result<()>;
}
```

未 `install` 之前收到效果 → `Degraded { code: "widget_unavailable", retryable: true }`。`crate::widget::setup` 从 `utils/resolve.rs:196` 移到 `setup.rs`（`app.manage(client)` 之后），并在其中调用 `install`。`feat.rs:436-437` 那处 `crate::consts::app_handle().state::<WidgetManager>()` 随之删除。

`apply(config)` 的语义修掉现状缺陷（`feat.rs:441-448` 对相同 variant 也会 `start()` 一次，等于重启）：

- `Disabled` 且在跑 → `stop()`；`Disabled` 且没跑 → 无操作；
- `Enabled(v)` 且当前 variant 相同且在跑 → 无操作；
- 其余 → `start(v)`。

### 7.3 `feat::patch_verge` 的逐项迁出

| 任务 | 从 `feat.rs` 移除的行                                                                                       | 从 `route_verge_patch` 移除的字段                                                                                                                       |
| ---- | ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 6a   | `:403-405`（`update_launch`）、`:406-409`（`update_sysproxy` + `guard_proxy`）、`:411-413`（`guard_proxy`） | `enable_auto_launch`、`enable_system_proxy`、`system_proxy_bypass`、`enable_proxy_guard`                                                                |
| 6b   | `:415-417`（`Hotkey::update`）、`:24-52`（三个 dashboard 函数）、`:134-229`（六个 toggle/enable/disable）   | `hotkeys`                                                                                                                                               |
| 6e-3 | `:419-422`（`set_locale`）、`:424-428`（tray）、`:430-432`（logger）、`:434-449`（widget）                  | `language`、`clash_tray_selector`、`enable_tray_text`、`tray_menu_mode`、`network_statistic_widget`、`app_log_level`、`max_log_files`、`auto_log_clean` |

6e-3 结束后 `route_verge_patch` 只剩 `enable_service_mode` 与 `enable_tun_mode` 两个字段，`feat::patch_verge` 只剩：

1. `theme_color` 的 hex 校验（`:348-353`）——与 `bridge/verge.rs:515` 的 `validate_verge_patch` 重复，保留其一即可，但删除哪一个属于 PR-7 的 legacy 清算；
2. `enable_service_mode` → `client.set_execution_host()`（`:368-380`）；
3. macOS/Linux 的 TUN 核心权限预检（`:382-401`）。

**决策：6e-3 不删除 `feat.rs`。** `feat.rs` 的删除是 roadmap §8.2 PR-7b 的明文交付物，且剩下这三件事分别属于 6e-5（启动/退出与宿主）和 PR-7。6e-3 的最后一步是把 `patch_verge` 缩成一个带 `FIXME(actor-migration)` 的薄 shim 并在文件头写清残留清单：

```rust
// FIXME(actor-migration): legacy verge side-effect shim。
// 外围 effects（system proxy / PAC / auto-launch / hotkeys / locale / tray /
// logger / widget）已全部迁至 ApplicationEffectPlan + ApplicationEffectsPort。
// 此处只剩 enable_service_mode 宿主切换与 TUN 核心权限预检。
// New code must use NyanpasuClient::{patch_app_config, patch_clash_config}。
// Remove after: PR-7b 删除 feat.rs 编排中心。
```

### 7.4 6e-3 测试矩阵

| 用例                                             | 断言                                                                                                           |
| ------------------------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| `locale_is_applied_before_tray_refresh`          | `MockLocaleSink` 与 `MockTrayRefresher` 共享一个调用序列 `Vec<&str>`，断言 `set_locale` 在 `refresh_full` 之前 |
| `language_change_requests_full_refresh`          | plan 产出 `Tray(Full)`                                                                                         |
| `system_proxy_change_only_requests_part_refresh` | plan 产出 `Tray(Part)`                                                                                         |
| `logger_effect_forwards_level_and_max_files`     | mock 收到 `(Some(level), Some(files))`                                                                         |
| `widget_same_variant_is_a_noop`                  | `apply` 两次同 variant → 底层 `start` 只调一次                                                                 |
| `widget_before_install_degrades`                 | `widget_unavailable`                                                                                           |
| `each_failure_reports_its_own_degradation`       | 四个 adapter 同时失败 → 四条 `Degradation`，`code` 互不相同，`phase` 均为 `UiEffect`                           |
| `ui_effect_failure_keeps_committed_config`       | facade 层：`patch_app_config` 返回 `CommittedDegraded`，`get_app_config()` 是新值                              |

---

## 8. 组合根变更

`ClientSetupArgs`（`client/mod.rs:71-81`）**逐任务追加字段**，每次只加在结构体末尾，避免与另外两条 PR-6 分支的 hunk 冲突（它们都不改这个结构体）：

| 任务 | 新增字段                                                                                                                                                          |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 6e-2 | `pub effects: Arc<dyn ApplicationEffectsPort>`                                                                                                                    |
| 6a   | `pub os_proxy: Arc<dyn OsProxyPort>`、`pub auto_launch: Arc<dyn AutoLaunchPort>`、`pub pac: Arc<dyn PacPort>`                                                     |
| 6b   | `pub shortcuts: Arc<dyn ShortcutRegistrar>`、`pub hotkey_sink: Arc<dyn HotkeyActionSink>`、`pub window: Arc<dyn WindowControl>`                                   |
| 6e-3 | `pub locale: Arc<dyn LocaleSink>`、`pub tray: Arc<dyn TrayRefresher>`、`pub logger_refresher: Arc<dyn LoggerRefresher>`、`pub widget: Arc<TauriWidgetController>` |

6a/6b 的 actor 在 `NyanpasuClient::try_new_with_args` 内部 spawn（与既有的 `ProxiesClient::spawn` / `StreamsClient::spawn` 同一段），executor 在三个 client 都就绪后组装。spawn 顺序：`SystemProxyClient` → `HotkeyClient` → `ApplicationEffectExecutor`。它们彼此无依赖，也不依赖 `CoreLifecycleClient`，所以不引入 actor 环。

`setup.rs` 的增量：

```rust
// 6a
let auto_launch_cfg = AutoLaunchConfig::resolve(app_handle)?;   // 取 env().appimage，删掉 Handle::global()
// 6b
let (hotkey_tx, hotkey_rx) = tokio::sync::mpsc::unbounded_channel();
// ...构造 client...
app.manage(client.clone());
// 6b：动作泵
tauri::async_runtime::spawn(hotkey_action_pump(hotkey_rx, client.clone()));
// 6e-3：widget 晚绑定
crate::widget::setup(app, client.subscribe_clash_connections()).await?;   // 内部 widget_controller.install(..)
```

`setup.rs` 未被另外两条 PR-6 分支修改，是安全区。

---

## 9. 测试策略

| 层       | 方式                                                                                                                                                                          | 位置                                                     |
| -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------- |
| 纯计划   | 普通值断言，零 IO                                                                                                                                                             | `client/effects/tests.rs`                                |
| 纯解析   | `HotkeyBindings::parse` 全失败分支                                                                                                                                            | `client/hotkey/tests.rs`                                 |
| actor    | `Actor::spawn` + mockall fake 端口，经 typed client 发消息；无 sleep，用 `Status` RPC 或 mock 调用计数做同步点；guard 定时器用 `schedule_guard_ticks: false` + `tick_guard()` | `client/system_proxy/tests.rs`、`client/hotkey/tests.rs` |
| facade   | `test_client_args_with_endpoint(&dir, endpoint)` + `NyanpasuClient::try_new_with_args`（`client/mod.rs:2347`），替换 `args.effects` 为 mock                                   | `client/effects/tests.rs`                                |
| 顺序断言 | 共享 `Arc<StdMutex<Vec<&'static str>>>` 记录调用序列，仿 `client/mod.rs:2371` 的 `host_transition_client`                                                                     | 同上                                                     |

所有路径都在 `TempDir` 内（`PathResolver::with_base_dirs`），满足 AGENTS §1.6 与 ledger 的 `test_real_dirs == 0` 硬门。

---

## 10. ledger 与前端

`scripts/architecture-ledger.ts` 的五项指标里，本栈只会动三项。**每个任务都必须用
`deno run -A scripts/architecture-ledger.ts --write-snapshot` 重生成快照并把 diff 放进同一个 commit**（gate 是逐 key 精确比对，`byKey` 少一个键或多一个键都会 fail）。

预期方向（实际数字以重生成为准）：

| 任务 | `config_calls`                                                                             | `service_globals`                                                                         | `migration_markers`                                                   | `legacy_dto_refs` | `bridgeFiles` |
| ---- | ------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------- | --------------------------------------------------------------------- | ----------------- | ------------- |
| 6e-1 | 不变                                                                                       | 不变                                                                                      | 不变                                                                  | 不变              | 不变          |
| 6e-2 | 不变                                                                                       | 不变                                                                                      | 可能 +1（新增 gate 的说明不是 marker，但 saga 挂载点若留 TODO 则 +1） | 不变              | 不变          |
| 6a   | `Config::verge()` −13（`core/sysopt.rs` 9 + `core/pac.rs` 4），`utils/resolve.rs` 另减若干 | `Sysopt::global()` 键**整条删除**（−8），`Handle::global()` 10→9                          | 不变                                                                  | 不变              | 不变          |
| 6b   | `Config::verge()` −2（`core/hotkey.rs`）+ `feat.rs` 减量                                   | `Hotkey::global()` 键**整条删除**（−3），`Handle::global()` 随 `feat.rs` 九个函数删除再减 | 不变                                                                  | 不变              | 不变          |
| 6e-3 | `feat.rs` 与 `core/tray/mod.rs` 的 `Config::verge()` 减量                                  | `Handle::global()` 继续减                                                                 | +1（`feat::patch_verge` 的 `FIXME(actor-migration)` shim 注释）       | 不变              | 不变          |

**注意 `service_globals` 与 `migration_markers` 两个块正被 `feat/pr6-updater-actor` 与 `feat/pr6-proxies-updater-interruption` 修改**（它们删掉 `UpdaterManager::global()` 键、把 `FIXME` 从 3 减到 2）。快照 JSON 在这两个块上会与本栈产生文本冲突，解决方式是**重跑 `--write-snapshot`**，不要手工合并数字。

前端影响仅两处，且都在另外两条分支未触及的区域：

- 6e-2：`use-settings.ts:70-85` 的 `mutationFn` 返回类型。
- 6b：`use-hotkeys.ts:22` 的 `unwrapResult(await invokeMutation(setHotkeys, ...))` 返回类型。

`bindings.ts` 每次任务后由 `cargo test -p clash-nyanpasu export_typescript_bindings --all-features` 重新生成（该测试会就地写文件并调 `pnpm exec prettier --write`）。

---

## 11. 交接点与残留

### 交接给 6e-4（连接中断）

连接中断已在 `feat/pr6-proxies-updater-interruption` 上实现为**动作驱动**（`core/proxies.rs` 的 `State::select`、`core_lifecycle/workflow.rs` 的 profile 切换分支）。若将来要把"`BreakConnectionStrategy` 配置本身变化"也纳入统一协议，挂载点是 `ApplicationEffectPlan`：新增 `ClashEffectFields::break_connection` 与 `ApplicationEffect::ConnectionPolicy(..)`，由 executor 转给 `ProxiesClient`。**本栈不预留空变体**，因为空变体等同于未使用的抽象。

### 交接给 6e-5（启动/退出全量接线）

- 启动入口：`NyanpasuClient::reconcile_application_effects()`（6e-2 交付，6a/6b/6e-3 逐步填充）。6e-5 负责把它接进正确的启动阶段（silent start、面板恢复、panic 路径），并决定失败时的 UI 提示。
- 退出入口：`NyanpasuClient::shutdown_application_effects()`。6e-5 负责 panic hook（`lib.rs:131-189` 当前**不**跑 `cleanup_processes`，退出后系统代理仍留在系统里）与 Windows shutdown hook 的时序。
- 6a 只做删除 `Sysopt::global()` 所需的最小接线，`utils/resolve.rs:232-236` 与 `utils/help.rs:243-269` 的完整重排留给 6e-5。

### 交接给 PR-7 的残留 bridge（本栈有意保留）

1. `feat::patch_verge` 薄 shim（`FIXME(actor-migration)`，措辞见 §7.3）。
2. `route_verge_patch` 仅剩 `enable_service_mode` / `enable_tun_mode` 两字段的分类器。
3. `client/event_sink.rs` 的 `UiEventSink::update_systray{,_part}` 与 `TrayRefresher` 的重复。
4. `client/rebuild.rs` 的 `legacy_regen_inputs` FIXME —— 本栈不触碰。
5. `bridge/verge.rs` 整体（三域 saga、legacy 投影）——PR-7a 删除。

新增的 `TODO(actor-migration)` 只允许出现在这两处：

```rust
// TODO(actor-migration): compatibility bridge for feat::patch_verge side-effect routing.
// Reason: enable_service_mode / enable_tun_mode 仍由 legacy IVerge 路由驱动，宿主切换属于 6e-5。
// Remove when: 6e-5 完成启动/退出接线且 PR-7b 删除 feat.rs。
```

```rust
// TODO(actor-migration): the three-domain legacy saga is the commit point for
// patch_verge_config, so the effect reconcile is mounted here as well as on the
// typed client patches.
// Reason: legacy IVerge wire 仍是前端唯一的 app-config patch 入口。
// Remove when: PR-7a 删除 run_legacy_verge_mutation / route_verge_patch。
```

---

## 12. 风险

| 风险                                                                                                                      | 影响                                                          | 缓解                                                                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **hotkeys 权威源反转需要一次新迁移**（§6.1）                                                                              | 6b 体量比任务卡预期大；迁移写错会丢用户快捷键                 | 迁移只做"KV → typed config + 删 KV 键"，幂等；`detect_baseline` 以 KV 键存在与否判定；加"KV 为空则无操作"和"typed 已有非空则不覆盖"两条单测；开工前向维护者确认方案，否则走 §6.1 的备选 |
| **`patch_verge_config` wire 变更**                                                                                        | 前端 `use-settings.ts` 是 47 处设置项的唯一漏斗，改错会全面崩 | 只改 `mutationFn` 的返回类型，`useSetting` 的 `upsert` 签名不变；`export_typescript_bindings` 测试已钉住 `MutationOutcome<null>` 必须存在                                               |
| **`runtime_apply_kind` 新增了当前不存在的 rebuild**（mixed-port 变化现在不 rebuild，见 §1）                               | 改端口会真的重启核心，行为可见变化                            | 这是修复而非回归（否则系统代理永远指向旧端口）；在 6e-2 的 plan 里显式列为行为变更并加测试 `mixed_port_change_requests_rebuild`                                                         |
| **快照 JSON 与另外两条 PR-6 分支冲突**                                                                                    | rebase 时手工改数字会让 gate 挂                               | 一律 `--write-snapshot` 重生成，禁止手改                                                                                                                                                |
| **PAC 下载把 reconcile RPC 拖长**                                                                                         | 门已释放，但调用方 IPC 会等                                   | `SYSTEM_PROXY_RPC_TIMEOUT = 15s`，超时返回 `retryable` 降级，actor 继续跑完                                                                                                             |
| **`bridge/verge.rs:328-347` 是共享 hunk**                                                                                 | 与未来 PR-7a 的删除冲突                                       | 本栈是该区域唯一在动的分支（两条 PR-6 分支都不碰 `bridge/`），风险仅存在于与 PR-7 的先后顺序                                                                                            |
| **`LoggingLevel` 非 `Copy`**（`nyanpasu-config/src/application/logging.rs:5` 已 derive `PartialEq`/`Clone`，但无 `Copy`） | `LoggerDesired` 与 `ApplicationEffectFields` 需要 `clone()`   | 已核实；`LoggerDesired` 按 `Clone` 设计，不要写成 `Copy`                                                                                                                                |

---

## 13. 文件归属矩阵

实现者不得改动未列出的文件；确有必要时必须在 PR 描述里写明原因。

| 文件                                                                      | 6e-1 | 6e-2 |  6a  |  6b  | 6e-3 |
| ------------------------------------------------------------------------- | :--: | :--: | :--: | :--: | :--: |
| `client/effects/{plan,status,ports}.rs`                                   | 新建 |  改  |  改  |  改  |  改  |
| `client/effects/{mod,executor,tests}.rs`                                  |  —   | 新建 |  改  |  改  |  改  |
| `client/mod.rs`（`ClientSetupArgs` / `NyanpasuClientInner` / patch 方法） |  —   |  改  |  改  |  改  |  改  |
| `client/system_proxy/**`                                                  |  —   |  —   | 新建 |  —   |  —   |
| `client/hotkey/**`                                                        |  —   |  —   |  —   | 新建 |  —   |
| `client/ui_effects/**`                                                    |  —   |  —   |  —   |  —   | 新建 |
| `bridge/verge.rs`（saga 挂载 / `route_verge_patch`）                      |  —   |  改  |  改  |  改  |  改  |
| `feat.rs`                                                                 |  —   |  —   |  改  |  改  |  改  |
| `ipc.rs`                                                                  |  —   |  改  |  —   |  改  |  —   |
| `setup.rs`                                                                |  —   |  改  |  改  |  改  |  改  |
| `utils/resolve.rs`                                                        |  —   |  —   |  改  |  改  |  改  |
| `utils/help.rs`                                                           |  —   |  —   |  改  |  —   |  改  |
| `core/sysopt.rs`、`core/pac.rs`                                           |  —   |  —   | 删除 |  —   |  —   |
| `core/hotkey.rs`                                                          |  —   |  —   |  —   | 删除 |  —   |
| `core/tray/mod.rs`（菜单事件改调 facade）                                 |  —   |  —   |  —   |  改  |  改  |
| `core/migration/modules/storage.rs`、`core/migration/runner.rs`           |  —   |  —   |  —   |  改  |  —   |
| `widget.rs`                                                               |  —   |  —   |  —   |  —   |  改  |
| `frontend/interface/src/ipc/bindings.ts`                                  |  —   | 生成 |  —   | 生成 |  —   |
| `frontend/interface/src/ipc/use-settings.ts`                              |  —   |  改  |  —   |  —   |  —   |
| `frontend/interface/src/hooks/use-hotkeys.ts`                             |  —   |  —   |  —   |  改  |  —   |
| `scripts/architecture-ledger.snapshot.json`                               |  —   | 生成 | 生成 | 生成 | 生成 |

**明确禁止触碰**（另外两条 PR-6 分支已占用）：`client/core_lifecycle/**`、`core/updater/**`、`core/proxies.rs`、`core/tray/proxies.rs`、`client/clash_api.rs`、`frontend/interface/src/ipc/use-clash-proxies.ts`、`frontend/nyanpasu/src/pages/settings/clash/_modules/core-manager-card.tsx`，以及 `client/mod.rs` 的 `:254-271` / `:380-406` / `:519-536` / `:1000` / `:1019-1025` / `:1200-1206` / `:1360-1366` / `:2505` 行区。新字段一律追加到结构体末尾。
