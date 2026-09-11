# PR-6 实施计划：effects / system proxy / hotkey / UI 副作用

**日期：** 2026-09-12
**设计：** [`docs/superpowers/specs/2026-09-12-pr6-application-effects/design.md`](../specs/2026-09-12-pr6-application-effects/design.md)
**基线：** `main @ 31967446d`
**交付形态：** 五条堆叠分支，后一条基于前一条

```text
main
 └─ feat/pr6-effect-plan          任务 6e-1
     └─ feat/pr6-config-reconcile     任务 6e-2
         └─ feat/pr6-system-proxy-actor   任务 6a
             └─ feat/pr6-hotkey-actor         任务 6b
                 └─ feat/pr6-ui-effects           任务 6e-3
```

**不在范围：** 6c、6d、6e-4、6e-5、6-final、PR-7。不得修改 `client/core_lifecycle/**`、`core/updater/**`、`core/proxies.rs`、`core/tray/proxies.rs`、`client/clash_api.rs`、`frontend/interface/src/ipc/use-clash-proxies.ts`、`core-manager-card.tsx`，以及设计文档 §13 列出的 `client/mod.rs` 保留行区。

---

## 0. 每条分支通用的验证命令

```sh
# 定向单测（把 <filter> 换成本任务的测试名前缀）
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features <filter>
# 全量库测试
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib
# 静态检查
cargo clippy --manifest-path backend/Cargo.toml --all-targets --all-features
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
# 架构台账（gate 是逐 key 精确比对）
pnpm lint:architecture-ledger
# 台账有变化时重生成快照，禁止手改 JSON 数字
deno run -A scripts/architecture-ledger.ts --write-snapshot --snapshot scripts/architecture-ledger.snapshot.json
```

wire 有变化时追加：

```sh
cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features export_typescript_bindings
pnpm -F interface build && pnpm typecheck && pnpm lint:oxlint
pnpm exec prettier --write <改到的 ts/tsx/json/md>
```

**通用约束：** 每个任务一个原子 commit（个别任务允许两个，已在下文标注）。actor 测试用 RPC 应答、`Status` 查询或 mock 调用计数同步，禁止 `sleep`。所有测试路径走 `tempfile::TempDir` + `PathResolver::with_base_dirs`。

---

## 1. 任务 6e-1 — 副作用计划与结果协议

**分支：** `feat/pr6-effect-plan`（基于 `main`）

**目标：** 交付纯服务 `ApplicationEffectPlan` 与结果协议 `EffectStatus` / `EffectHealth` / `EffectRevision`，以及单一分发缝 `ApplicationEffectsPort` 的 trait 定义。零 Tauri、零 OS、零 IO。本任务**不改动任何现有代码路径的行为**。

### 步骤（TDD）

1. **编译前提（已核实，无需改动上游）。** `ApplicationEffectFields` / `ClashEffectFields` 用到的每个类型都已 derive `PartialEq`：`LoggingLevel`（`logging.rs:5`，注意它 **不是 `Copy`**，`LoggerDesired` 按 `Clone` 写）、`I18nLanguage`、`ProxiesSelectorMode`、`TrayMenuMode`、`NetworkStatisticWidgetConfig`、`TunStack`、`ClashControlChannel`、`PortStrategy`、`ExternalControllerStrategy`。`NyanpasuAppConfig` 与 `ClashConfig` 本身**没有** `PartialEq`，所以必须走投影而不是整体比较。
2. **新建 `backend/tauri/src/client/effects/plan.rs`，先写测试。** 在文件底部 `#[cfg(test)] mod tests` 中写：
   - `empty_diff_produces_no_effects`（before == after）
   - `system_proxy_toggle_produces_system_proxy_effect`
   - `bypass_change_produces_system_proxy_effect`
   - `port_change_alone_produces_system_proxy_effect`
   - `pac_url_change_produces_system_proxy_effect`
   - `guard_interval_change_produces_only_proxy_guard`
   - `language_change_produces_locale_and_full_tray`
   - `system_proxy_change_alone_produces_part_tray`
   - `language_and_system_proxy_change_produces_full_tray_only`
   - `log_level_change_produces_logger_effect`
   - `widget_change_produces_widget_effect`
   - `hotkeys_change_produces_hotkeys_effect`
   - `effects_are_sorted_by_kind`（`Locale < Logger < AutoLaunch < SystemProxy < ProxyGuard < Hotkeys < Widget < Tray`）
   - `full_plan_contains_every_kind_with_full_tray`
   - `runtime_apply_kind_prefers_control_channel_over_rebuild`
   - `runtime_apply_kind_reports_rebuild_for_port_and_tun_changes`
   - `runtime_apply_kind_ignores_core_and_service_mode`
   - verify：`cargo test ... --all-features client::effects::plan`（应全红）
3. **实现 `plan.rs`。** 类型签名：
   ```rust
   pub struct ApplicationEffectInputs { pub app: ApplicationEffectFields, pub clash: ClashEffectFields, pub ports: Option<ResolvedPortBindings> }
   impl ApplicationEffectInputs {
       pub fn project(app: &NyanpasuAppConfig, clash: &ClashConfig, ports: Option<ResolvedPortBindings>) -> Self;
   }
   pub enum EffectKind { Locale, Logger, AutoLaunch, SystemProxy, ProxyGuard, Hotkeys, Widget, Tray }
   pub enum TrayRefresh { Full, Part }
   pub struct SystemProxyDesired { pub enabled: bool, pub bypass: String, pub port: Option<u16>, pub pac_url: Option<url::Url> }
   pub struct ProxyGuardDesired { pub enabled: bool, pub interval: std::time::Duration }
   pub struct LoggerDesired { pub level: LoggingLevel, pub max_files: usize }
   pub enum ApplicationEffect { Locale(I18nLanguage), Logger(LoggerDesired), AutoLaunch(bool),
                                SystemProxy(SystemProxyDesired), ProxyGuard(ProxyGuardDesired),
                                Hotkeys(Vec<String>), Widget(NetworkStatisticWidgetConfig), Tray(TrayRefresh) }
   pub struct ApplicationEffectPlan { effects: Vec<ApplicationEffect> }
   impl ApplicationEffectPlan {
       pub fn diff(before: &ApplicationEffectInputs, after: &ApplicationEffectInputs) -> Self;
       pub fn full(after: &ApplicationEffectInputs) -> Self;
       pub fn is_empty(&self) -> bool;
       pub fn effects(&self) -> &[ApplicationEffect];
   }
   pub enum RuntimeApplyKind { None, Rebuild, ControlChannel }
   pub fn runtime_apply_kind(before: &ApplicationEffectInputs, after: &ApplicationEffectInputs) -> RuntimeApplyKind;
   ```
   - verify：同上，应全绿
4. **新建 `client/effects/status.rs` + 测试。**
   ```rust
   pub struct EffectRevision(u64);
   pub enum EffectHealth { Healthy, Degraded { code: &'static str, message: String, retryable: bool },
                           Superseded, Unsupported { code: &'static str } }
   pub struct EffectStatus { pub kind: EffectKind, pub desired_revision: EffectRevision,
                             pub applied_revision: EffectRevision, pub health: EffectHealth }
   pub fn degradation_of(status: &EffectStatus) -> Option<runtime::Degradation>;
   ```
   测试：`degradation_maps_system_kinds_to_system_effect_phase`、`degradation_maps_ui_kinds_to_ui_effect_phase`、`healthy_superseded_and_unsupported_produce_no_degradation`、`revision_is_monotonically_comparable`。
5. **新建 `client/effects/ports.rs`。** `ApplicationEffectsPort`（`#[async_trait]` + `#[cfg_attr(test, mockall::automock)]`）与 `NoopApplicationEffects`。测试 `noop_port_reports_healthy_for_every_effect`。
6. **新建 `client/effects/mod.rs`**，只做 `mod`/`pub use` 转发；在 `client/mod.rs` 的模块列表（`:1-15` 之后、不进入保留行区）加 `pub(crate) mod effects;`。
7. **全量验证。**
   - `cargo test --manifest-path backend/Cargo.toml -p clash-nyanpasu --all-features --lib`
   - `cargo clippy ... --all-targets --all-features`、`cargo fmt ... -- --check`
   - `pnpm lint:architecture-ledger`（**应当直接通过，快照不变**；若失败说明误引入了 `Config::` / `::global()` / marker）

### 核心验收 → 检查项 1:1

| 任务卡验收                       | 检查                                                                                                                                                                              |
| -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 无变化不产生操作                 | `empty_diff_produces_no_effects`                                                                                                                                                  |
| 字段变化产生正确计划             | 步骤 2 中 11 条单字段用例 + `effects_are_sorted_by_kind`                                                                                                                          |
| 明确提交前错误、提交后降级的区别 | `degradation_of` 只对 `Degraded` 返回 `Some`；`ApplicationEffectPlan::diff` 与 `runtime_apply_kind` 均为 infallible（无 `Result`），提交前错误只能来自调用方的校验（6e-2 起接入） |

### 文件

新建：`client/effects/{plan,status,ports,mod}.rs`。
修改：`client/mod.rs`（仅新增一行 `mod` 声明）；可能修改 `nyanpasu-config/src/application/logging.rs`（补 `PartialEq`）。

### commit

```text
feat(effects): add pure application effect plan and status protocol
```

---

## 2. 任务 6e-2 — typed 配置提交与 reconcile 入口

**分支：** `feat/pr6-config-reconcile`（基于 `feat/pr6-effect-plan`）

**目标：** 在 facade 建立"提交 → runtime apply → 外围 effects → `MutationOutcome`"的统一管线，并把它同时挂到 typed client patch 与三域 legacy saga 两个提交点上。executor 此时是空壳（`NoopApplicationEffects`），因此**运行时行为不变**，唯一可见变化是 `patch_verge_config` 的 wire 增加了 `MutationOutcome` 包装，以及 TUN/控制通道 reconcile 的判定改由 `runtime_apply_kind` 统一负责。

本任务允许拆成两个 commit（后端管线 / wire+前端），见文末。

### 步骤（TDD）

1. **先写 facade 测试（红）。** 在 `client/effects/tests.rs` 新建，使用 `super::super::tests::test_client_args_with_endpoint`：
   - `commit_failure_runs_no_effects` —— 注入一个提交必失败的 typed 域（复用 `client/mod.rs` 测试里的 `FailingVergeMirror` 手法），断言 `MockApplicationEffectsPort::apply` 零调用且返回 `Err`
   - `effect_failure_keeps_committed_config` —— mock 返回 `Degraded`，断言结果是 `CommittedDegraded` 且 `get_app_config()` 已是新值
   - `unchanged_patch_dispatches_no_effects` —— 用当前值再 patch 一次，`apply` 收到空 plan（或不调用）
   - `revisions_are_allocated_in_commit_order` —— 连续两次 patch，mock 记录到的 revision 严格递增
   - `stale_reconcile_does_not_overwrite_newer_state` —— mock 端口记录最后一次收到的期望值，断言等于最后一次提交的值
   - `mixed_port_change_requests_rebuild` —— 断言 `runtime_apply_kind` 走到 `Rebuild`（通过记录 `rebuild_running_config` 是否被触发）
   - `control_channel_change_prefers_apply_control_channel`
   - `session_state_patch_produces_empty_plan`
2. **实现 `client/effects/mod.rs` 的 `ApplicationEffects` 与 gate。**
   ```rust
   pub(crate) struct ApplicationEffects {
       gate: tokio::sync::Mutex<GateState>,      // 注释见设计 §4.3
       port: Arc<dyn ApplicationEffectsPort>,
   }
   impl ApplicationEffects {
       pub(crate) fn new(port: Arc<dyn ApplicationEffectsPort>) -> Self;
       pub(crate) async fn gate(&self) -> GateGuard<'_>;
       pub(crate) fn port(&self) -> &Arc<dyn ApplicationEffectsPort>;
   }
   impl GateGuard<'_> { pub(crate) fn allocate(&mut self) -> EffectRevision; }
   ```
3. **在 `client/effects/mod.rs` 内 `impl NyanpasuClient`**（不进 `client/mod.rs` 的保留行区）：
   ```rust
   async fn effect_inputs(&self) -> Result<ApplicationEffectInputs>;
   async fn commit_and_reconcile<F, Fut>(&self, commit: F) -> Result<runtime::MutationOutcome<()>>
       where F: FnOnce() -> Fut, Fut: Future<Output = Result<()>>;
   pub async fn reconcile_application_effects(&self) -> Result<runtime::MutationOutcome<()>>;
   pub async fn shutdown_application_effects(&self) -> Vec<runtime::Degradation>;
   ```
   顺序严格按设计 §4.2：采样 before → commit → 分配 revision → runtime apply → 采样 after → `diff` → 释放 gate → `port.apply`。
4. **改 `client/mod.rs` 的六个方法签名**（`:544`、`:550`、`:561`、`:567`、`:592`、`:598`），把 `Result<()>` 换成 `Result<runtime::MutationOutcome<()>>`，内部改为 `self.commit_and_reconcile(|| async { ... }).await`。给 `NyanpasuClientInner` 末尾追加 `effects: ApplicationEffects` 字段，给 `ClientSetupArgs` 末尾追加 `pub effects: Arc<dyn ApplicationEffectsPort>`，在 `try_new_with_args` 的结构体字面量末尾追加 `effects`。
   - **注意**：字段一律加在结构体末尾，避开 `client/mod.rs:254-271` 与 `:380-406` 的既有 hunk 归属。
5. **把 reconcile 挂到三域 saga。** `client/mod.rs:612` `apply_legacy_verge_patch_saga` 与 `:640` `apply_legacy_verge_replacement_saga` 改为返回 `Result<runtime::MutationOutcome<()>>`，在 `apply_legacy_verge_states_saga` 成功返回后按同一顺序跑 runtime apply + effects。在挂载点写入设计 §11 的第二条 `TODO(actor-migration)`。
6. **删除 `bridge/verge.rs:328-347` 的 `reconcile_tun` 块**，并把 `:295-297` 的 `channel_changed` / `reconcile_tun` 计算一并删除；改由 saga 内的 `runtime_apply_kind` 判定。`bridge/verge.rs:219-259` 的 `patch_verge_config` 与 `:261` 的 `replace_verge_config` 返回类型改为 `ClientResult<runtime::MutationOutcome<()>>`，`:240-247` 的 `channel_changed` + `apply_control_channel` 分支同样删除。
   - verify：`cargo test ... --all-features bridge::verge`（既有 bridge 测试必须全绿；签名变化处更新断言，不得放宽语义）
7. **改 IPC wire。** `ipc.rs:492` `patch_verge_config` 返回 `Result<crate::client::runtime::MutationOutcome<()>>`。
   - verify：`cargo test ... export_typescript_bindings`，确认 `bindings.ts` 中 `patchVergeConfig` 变成 `typedError<MutationOutcome<null>, string>`
8. **改前端。** `frontend/interface/src/ipc/use-settings.ts:70-85`：`mutationFn` 返回类型跟随生成的类型；committed-degraded 视为成功，degradations 走 `console.warn` 或既有的 profiles 侧处理方式（保持与 `use-clash-config` 一致，不新造 UI）。
   - verify：`pnpm -F interface build && pnpm typecheck && pnpm lint:oxlint`
9. **全量验证 + 台账。**

### 核心验收 → 检查项 1:1

| 任务卡验收                  | 检查                                                                                                                                          |
| --------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| 提交失败不执行副作用        | `commit_failure_runs_no_effects`                                                                                                              |
| 副作用失败保留已提交配置    | `effect_failure_keeps_committed_config`                                                                                                       |
| 过期 reconcile 不覆盖新状态 | `revisions_are_allocated_in_commit_order` + `stale_reconcile_does_not_overwrite_newer_state`                                                  |
| 不重复提交                  | `bridge::verge` 既有测试保持绿（saga 是唯一提交点）；新增 `legacy_route_commits_once_and_reconciles_once`：mock port 的 `apply` 调用次数 == 1 |

### 文件

新建：`client/effects/{executor.rs（空壳）,tests.rs}`。
修改：`client/effects/{mod,ports}.rs`、`client/mod.rs`、`bridge/verge.rs`、`ipc.rs`、`setup.rs`、`frontend/interface/src/ipc/{bindings.ts,use-settings.ts}`、`scripts/architecture-ledger.snapshot.json`（如有变化）。

### commit

```text
feat(effects): reconcile application effects after typed config commits
```

如需拆分：

```text
feat(effects): reconcile application effects after typed config commits
feat(ipc): surface verge patch degradations through MutationOutcome
```

---

## 3. 任务 6a — SystemProxyActor 完整迁移

**分支：** `feat/pr6-system-proxy-actor`（基于 `feat/pr6-config-reconcile`）

**目标：** actor 独占系统代理、原始代理快照、自启动、guard 定时器与 PAC；注入 OS / HTTP / 文件与路径 adapter；消费 typed 实际端口；迁移启动、修改、退出三个调用点；删除 `Sysopt::global()`、`core/sysopt.rs`、`core/pac.rs`。

### 步骤（TDD）

1. **新建 `client/system_proxy/ports.rs`。** `OsProxyConfig`、`OsProxyPort`、`AutoLaunchPort`、`PacPort`（签名见设计 §5.2），全部带 `#[cfg_attr(test, mockall::automock)]`。
2. **新建 `client/system_proxy/tests.rs`，写设计 §5.6 的 13 条用例（红）。** 测试用 `SystemProxyClient::spawn(Args { schedule_guard_ticks: false, .. })` + mock 端口 + `tick_guard()`。
   - verify：`cargo test ... --all-features client::system_proxy`
3. **实现 `client/system_proxy/actor.rs`**（`Message` / `Args` / `State` 见设计 §5.1）与 `client/system_proxy/mod.rs` 的 `SystemProxyClient`：
   ```rust
   pub(crate) struct SystemProxyStatus {
       pub desired_revision: EffectRevision, pub applied_revision: EffectRevision,
       pub health: EffectHealth, pub guard_active: bool,
       pub guard_interval: Option<std::time::Duration>, pub pac_active: bool,
   }
   impl SystemProxyClient {
       pub(crate) async fn spawn(args: Args) -> anyhow::Result<Self>;
       pub(crate) async fn reconcile(&self, revision: EffectRevision,
           proxy: Option<SystemProxyDesired>, guard: Option<ProxyGuardDesired>,
           auto_launch: Option<bool>) -> Vec<EffectStatus>;
       pub(crate) async fn status(&self) -> SystemProxyStatus;
       pub(crate) async fn restore(&self) -> EffectStatus;
       #[cfg(test)] pub(crate) async fn tick_guard(&self);
   }
   const SYSTEM_PROXY_RPC_TIMEOUT: Duration = Duration::from_secs(15);
   const SYSTEM_PROXY_RESTORE_TIMEOUT: Duration = Duration::from_secs(5);
   ```
   guard 定时器用 `myself.send_interval(interval, || Message::GuardTick)`，句柄存 `State::guard_job`，间隔变化或关闭时 `abort()`，`post_stop` 也 `abort()`。
4. **实现 `client/system_proxy/adapters.rs`：** `SysproxyOsProxy`（搬 `core/sysopt.rs:33-39` 的三份平台 bypass 常量）、`AutoLaunchBackend` + `AutoLaunchConfig { app_name, app_path, appimage }`（搬 `core/sysopt.rs:209-322`，Windows 引号路径、macOS `.app` 回溯与 disable-then-enable、Linux appimage 全部保留；appimage 改为**构造参数**）、`HttpPacBackend { client, cache_path }`（搬 `core/pac.rs:27-250`，`cache_path` 由 `PathResolver` 注入）。
5. **接入 executor。** `client/effects/executor.rs` 里 `ApplicationEffectExecutor` 新增 `system_proxy: SystemProxyClient`；`apply` 把同一 plan 中的 `AutoLaunch` / `SystemProxy` / `ProxyGuard` 三项**合并成一条 `reconcile` 消息**发送，把返回的 `Vec<EffectStatus>` 并入结果。
6. **组合根接线。** `ClientSetupArgs` 末尾追加 `os_proxy` / `auto_launch` / `pac` 三个字段；`try_new_with_args` 内 spawn `SystemProxyClient`；`setup.rs` 构造三个具体 adapter，`AutoLaunchConfig::resolve(app_handle)` 取 `app_handle.env().appimage`。
7. **迁移三个调用点并删除旧实现（同一 commit）。**
   - `utils/resolve.rs:232-233` 两行 → 一次 `client.reconcile_application_effects()`（`block_on`，`log_err!` 包裹）
   - `utils/help.rs:243-269`：`super::resolve::resolve_reset()` 删除，改在已有的 `block_on` 块里 `client.shutdown_application_effects().await`；`utils/resolve.rs:276-279` 的 `resolve_reset` 删除
   - `feat.rs:403-413` 三个块删除
   - `bridge/verge.rs:491-513` 的 `route_verge_patch` 删除 `enable_auto_launch` / `enable_system_proxy` / `system_proxy_bypass` / `enable_proxy_guard` 四个字段
   - `rm backend/tauri/src/core/sysopt.rs backend/tauri/src/core/pac.rs`，`core/mod.rs` 删除两条 `pub mod`
   - 清理由此产生的孤儿 import（`feat.rs` 的 `sysopt`、`utils/resolve.rs` 的 `sysopt`）
8. **全量验证 + 重生成快照。**
   - `deno run -A scripts/architecture-ledger.ts --write-snapshot --snapshot scripts/architecture-ledger.snapshot.json`
   - 确认 `service_globals.byKey` 中 `Sysopt::global()` 键**整条消失**，`Handle::global()` 至少 10→9

### 核心验收 → 检查项 1:1

| 任务卡验收              | 检查                                                                                                                                            |
| ----------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| 代理切换                | `enabling_sets_os_proxy_with_resolved_port`、`disabling_restores_and_stops_guard`                                                               |
| PAC 切换                | `pac_enabled_takes_over_and_skips_plain_proxy`、`pac_failure_falls_back_to_direct_and_degrades`、`pac_unsupported_platform_reports_unsupported` |
| 端口变化                | `port_change_reapplies_system_proxy`                                                                                                            |
| guard 启停              | `guard_interval_change_rebuilds_timer`、`guard_tick_reapplies_last_desired`、`guard_tick_is_suppressed_while_pac_active`                        |
| 退出恢复                | `exit_restores_captured_original`                                                                                                               |
| 失败降级                | `set_failure_degrades_and_keeps_desired`、`auto_launch_failure_degrades_independently`                                                          |
| 删除 `Sysopt::global()` | `rg -n 'Sysopt::global'` 零命中；台账 `service_globals` 中该键消失                                                                              |

### 文件

新建：`client/system_proxy/{mod,actor,ports,adapters,tests}.rs`。
修改：`client/effects/executor.rs`、`client/mod.rs`、`setup.rs`、`feat.rs`、`bridge/verge.rs`、`utils/resolve.rs`、`utils/help.rs`、`core/mod.rs`、`scripts/architecture-ledger.snapshot.json`。
删除：`core/sysopt.rs`、`core/pac.rs`。

### commit

```text
feat(system-proxy): own proxy, PAC and auto-launch in a typed actor
```

---

## 4. 任务 6b — HotkeyActor 完整迁移

**分支：** `feat/pr6-hotkey-actor`（基于 `feat/pr6-system-proxy-actor`）

**目标：** actor 独占注册表；注入 shortcut adapter；初始值来自 typed config；callback 经 facade 执行动作；迁移快捷键 IPC；删除 KV/verge 双读与 `Hotkey::global()`；callback 不再调用 `feat::*`。

> **开工前确认项：** 设计 §6.1 的权威源反转（KV → typed config）需要一次新迁移步骤。若维护者否决，改走 §6.1 的备选方案（`HotkeyStore` 端口 + hotkeys 退出 plan），本节步骤 2、3、9 相应替换。

本任务允许拆成两个 commit（迁移 / actor），见文末。

### 步骤（TDD）

1. **新建 `client/hotkey/ports.rs`。** `HotkeyAction`（11 个变体，`as_str()` 与 `core/hotkey.rs:86-100` 一字不差）、`HotkeyParseError`（5 个变体）、`HotkeyBindings`、`HotkeyOp`、`ShortcutRegistrar`、`HotkeyActionSink`、`WindowControl`。
2. **新建 `client/hotkey/tests.rs`，先写纯解析用例（红）：** `parse_rejects_malformed_unknown_invalid_and_missing_super`、`parse_rejects_duplicate_accelerator`、`parse_accepts_the_legacy_func_comma_key_format`、`diff_emits_unbind_rebind_and_bind`。实现 `HotkeyBindings::parse` / `diff`。
3. **写 actor 用例（红）**（设计 §6.6 的其余 7 条），实现 `client/hotkey/actor.rs` + `client/hotkey/mod.rs` 的 `HotkeyClient`：
   ```rust
   pub(crate) struct HotkeyStatus {
       pub desired_revision: EffectRevision, pub applied_revision: EffectRevision,
       pub health: EffectHealth, pub registered: BTreeMap<String, HotkeyAction>,
   }
   impl HotkeyClient {
       pub(crate) async fn spawn(args: Args) -> anyhow::Result<Self>;
       pub(crate) async fn reconcile(&self, revision: EffectRevision, desired: HotkeyBindings) -> EffectStatus;
       pub(crate) async fn status(&self) -> HotkeyStatus;
       pub(crate) async fn unregister_all(&self) -> EffectStatus;
   }
   const HOTKEY_RPC_TIMEOUT: Duration = Duration::from_secs(5);
   ```
   注册顺序：先全部注销（`Unbind` + `Rebind` 的注销半边），再全部注册。
4. **实现 `client/hotkey/adapters.rs`：** `TauriShortcutRegistrar<R>`（包 `tauri_plugin_global_shortcut`，`validate` 复刻 `core/hotkey.rs:230-249` 的两步校验含小写子串 super-key 判定）、`ChannelActionSink`、`TauriWindowControl<R>`（包 `resolve::{is_window_open, create_window, close_window}`，主线程调度在 adapter 内）。
5. **facade 动作映射。** 在 `client/hotkey/mod.rs` 的 `impl NyanpasuClient` 中实现 `pub async fn dispatch_hotkey_action(&self, action: HotkeyAction) -> Result<()>`，映射表见设计 §6.3。给 `NyanpasuClientInner` 末尾追加 `window: Arc<dyn WindowControl>`、`hotkeys: HotkeyClient`。
   - 新增 facade 测试 `dispatch_toggle_system_proxy_flips_app_config`、`dispatch_toggle_tun_mode_flips_clash_config`、`dispatch_clash_mode_patches_runtime_overrides`
6. **提交前校验。** 在 `commit_and_reconcile` 的调用方（`patch_app_config`）之前，对携带 `hotkeys` 的 patch 先跑 `HotkeyBindings::parse`，失败返回 `Err`。测试 `invalid_hotkey_is_rejected_before_commit`。
7. **接入 executor。** `ApplicationEffectExecutor` 新增 `hotkeys: HotkeyClient`，处理 `ApplicationEffect::Hotkeys`。
8. **组合根接线。** `ClientSetupArgs` 追加 `shortcuts` / `hotkey_sink` / `window`；`setup.rs` 建 mpsc 通道、spawn `hotkey_action_pump(rx, client.clone())`。
9. **新增迁移步骤。** `core/migration/modules/storage.rs` 增加 `MigrateHotkeysToTypedConfig`（revision 2）：读 KV `"hotkeys"` → 写 `application.yaml` 的 `hotkeys` → 删 KV 键；`detect_baseline` 在 KV 键存在时返回 1。测试：`migrates_kv_hotkeys_into_typed_config`、`is_idempotent_when_kv_key_absent`、`does_not_overwrite_non_empty_typed_hotkeys`。更新 `core/migration/runner.rs:444-448` 的断言。
10. **迁移 IPC 与删除旧实现（同一 commit）。**
    - `ipc.rs:486` `get_hotkey_functions` → `HotkeyAction::all()`
    - `ipc.rs:1058-1077` `HOTKEYS_KEY` / `get_hotkeys` / `set_hotkeys` → facade 调用，`set_hotkeys` 返回 `MutationOutcome<()>`
    - `feat.rs` 删除 `:24-52`（三个 dashboard）与 `:134-229`（六个 toggle/enable/disable）与 `:415-417`（hotkey update）
    - `core/tray/mod.rs:432-461` 的 `system_proxy` / `tun_mode` / `rule_mode|global_mode|direct_mode|script_mode` / `open_window` 分支改调 facade（`app_handle.state::<NyanpasuClient>()`）
    - `bridge/verge.rs` 的 `route_verge_patch` 删除 `hotkeys`
    - `rm backend/tauri/src/core/hotkey.rs`，`core/mod.rs` 删除 `pub mod hotkey;`；`utils/resolve.rs:236` 删除
11. **前端。** `frontend/interface/src/hooks/use-hotkeys.ts:22` 跟随生成的返回类型。
12. **全量验证 + 重生成快照**，确认 `Hotkey::global()` 键消失。

### 核心验收 → 检查项 1:1

| 任务卡验收                              | 检查                                                                                                                                           |
| --------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| 重复/非法快捷键                         | `parse_rejects_malformed_unknown_invalid_and_missing_super`、`parse_rejects_duplicate_accelerator`、`invalid_hotkey_is_rejected_before_commit` |
| 部分注册失败                            | `partial_registration_failure_degrades_and_keeps_successes`                                                                                    |
| 更新                                    | `update_unregisters_before_registering`、`unchanged_bindings_produce_no_os_calls`、`stale_revision_is_superseded`                              |
| 退出注销                                | `exit_unregisters_all`                                                                                                                         |
| 删除 KV/verge 双读和 `Hotkey::global()` | `rg -n 'Hotkey::global\|get_item::<Vec<String>>\("hotkeys"\)'` 零命中；迁移测试三条                                                            |
| callback 不再调用 `feat::*`             | `rg -n 'feat::' backend/tauri/src/client/hotkey` 零命中；`callback_dispatches_action_to_sink` + 三条 facade dispatch 测试                      |

### 文件

新建：`client/hotkey/{mod,actor,ports,adapters,tests}.rs`。
修改：`client/effects/executor.rs`、`client/mod.rs`、`setup.rs`、`feat.rs`、`ipc.rs`、`bridge/verge.rs`、`core/mod.rs`、`core/tray/mod.rs`、`core/migration/modules/storage.rs`、`core/migration/runner.rs`、`utils/resolve.rs`、`frontend/interface/src/ipc/{bindings.ts}`、`frontend/interface/src/hooks/use-hotkeys.ts`、`scripts/architecture-ledger.snapshot.json`。
删除：`core/hotkey.rs`。

### commit

```text
feat(hotkey): own shortcut registration in a typed actor
```

如需拆分（推荐，迁移与 actor 是两件事）：

```text
feat(migration): move hotkeys back into typed application config
feat(hotkey): own shortcut registration in a typed actor
```

---

## 5. 任务 6e-3 — UI 与日志副作用迁移

**分支：** `feat/pr6-ui-effects`（基于 `feat/pr6-hotkey-actor`）

**目标：** locale、tray、logger refresh、widget 改为窄 adapter；主线程调度留在 Tauri 边界；逐项迁出 `feat::patch_verge`；各项失败能分别报告；业务层无 Tauri 依赖。

### 步骤（TDD）

1. **新建 `client/ui_effects/ports.rs`。** `LocaleSink`、`TrayRefresher`、`LoggerRefresher`、`WidgetController`（签名见设计 §7.1），全部 `#[cfg_attr(test, mockall::automock)]`。
2. **新建 `client/ui_effects/tests.rs`，写设计 §7.4 的 8 条用例（红）。** 顺序断言用共享 `Arc<StdMutex<Vec<&'static str>>>`。
3. **实现 `client/ui_effects/adapters.rs`：** `RustI18nLocaleSink`、`TauriTrayRefresher<R>`（`refresh_full` 走 `emit("update_systray", ())`，`refresh_part` 走 `Tray::update_part`）、`TracingLoggerRefresher`、`TauriWidgetController`（`tokio::sync::OnceCell<WidgetManager>` + `install()`，`apply` 的三分支语义见设计 §7.2）。
4. **接入 executor。** `ApplicationEffectExecutor` 新增四个 `Arc<dyn ...>` 字段，处理 `Locale` / `Logger` / `Widget` / `Tray` 四类效果，每类失败各自产出一条 code 互异的 `EffectStatus`。
5. **组合根接线。** `ClientSetupArgs` 追加四个字段；`crate::widget::setup` 从 `utils/resolve.rs:196-202` 移到 `setup.rs`（`app.manage(client)` 之后），并在其中 `widget_controller.install(manager)`。
6. **迁出并删除（同一 commit）。**
   - `feat.rs:419-422`（locale）、`:424-428`（tray）、`:430-432`（logger）、`:434-449`（widget）删除
   - `bridge/verge.rs` 的 `route_verge_patch` 删除 `language` / `clash_tray_selector` / `enable_tray_text` / `tray_menu_mode` / `network_statistic_widget` / `app_log_level` / `max_log_files` / `auto_log_clean`
   - `feat::patch_verge` 缩为薄 shim，文件头加设计 §7.3 的 `FIXME(actor-migration)` 注释块
   - `feat.rs:436-437` 的 `consts::app_handle().state::<WidgetManager>()` 随之消失
7. **确认业务层无 Tauri 依赖。**
   - `rg -n 'tauri::' backend/tauri/src/client/{effects,ui_effects,system_proxy,hotkey}/{mod,plan,status,ports,actor,executor}.rs` 应零命中（只有 `adapters.rs` 允许出现）
8. **全量验证 + 重生成快照。**

### 核心验收 → 检查项 1:1

| 任务卡验收                  | 检查                                                                                                                                           |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| locale 更新后 tray 顺序正确 | `locale_is_applied_before_tray_refresh`、`language_change_requests_full_refresh`、`system_proxy_change_only_requests_part_refresh`             |
| widget 生命周期正确         | `widget_same_variant_is_a_noop`、`widget_before_install_degrades`、外加 `disabled_config_stops_a_running_widget`                               |
| 各项失败能分别报告          | `each_failure_reports_its_own_degradation`（四条 `Degradation`，`code` 互异，`phase == UiEffect`）、`ui_effect_failure_keeps_committed_config` |
| 业务层无 Tauri 依赖         | 步骤 7 的 `rg` 检查（写进 PR 描述作为证据）                                                                                                    |

### 文件

新建：`client/ui_effects/{mod,ports,adapters,tests}.rs`。
修改：`client/effects/executor.rs`、`client/mod.rs`、`setup.rs`、`feat.rs`、`bridge/verge.rs`、`utils/resolve.rs`、`utils/help.rs`、`widget.rs`、`core/tray/mod.rs`、`scripts/architecture-ledger.snapshot.json`。

### commit

```text
feat(ui-effects): move locale, tray, logger and widget behind narrow adapters
```

---

## 6. 收尾检查清单（每条分支合并前）

- [ ] 每一行改动都能追到任务卡；未列入文件归属矩阵（设计 §13）的文件若被修改，PR 描述里写明原因
- [ ] 无新增 `::global()` / `OnceCell<Service>` / `Lazy<Service>` / 可变 static
- [ ] 新增的 `TODO/FIXME(actor-migration)` 只有设计 §11 列出的两处，措辞一致
- [ ] actor 状态不经 `Arc<Mutex<_>>` 泄漏；唯一的 `tokio::sync::Mutex` 是 `ApplicationEffectGate`，带说明注释
- [ ] 跨 actor RPC 都有有限超时
- [ ] 测试零访问真实用户目录（台账 `test_real_dirs.total == 0` 是硬门）
- [ ] `pnpm lint:architecture-ledger` 通过，快照由 `--write-snapshot` 生成而非手改
- [ ] `bindings.ts` 由 `export_typescript_bindings` 生成，且 `git diff --exit-code` 干净
- [ ] PR 描述包含：design 决策、failure matrix、automated tests、manual smoke evidence、residual bridge ledger（roadmap §11.3）
