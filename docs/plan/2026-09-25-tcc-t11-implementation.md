# T11 删除重复机制、全回归与文档交付

基线：L4 `refactor/inject-app-infrastructure` 的 head。实施分支 `refactor/tcc-t11-cleanup`，本地 stacked，未推送、未合并。整栈起点为 `main@4f59ca781`，自下而上五层：

| 层  | 分支                                 |
| --- | ------------------------------------ |
| L1  | `refactor/typed-config-ipc`          |
| L2  | `refactor/remove-legacy-config`      |
| L3  | `feat/tcc-startup-shutdown`          |
| L4  | `refactor/inject-app-infrastructure` |
| L5  | `refactor/tcc-t11-cleanup`（本分支） |

## 范围与假设

- 代码部分只删除重复机制：T10 之后已无调用者的路由和信号、`effects/plan.rs` 中的旧分类，以及 `feat.rs`。按 R28，`feat.rs` 的边界 helper 迁到调用方所在的边界后删除；roadmap §8.2 与 §12 要求 `feat.rs` 为零，而没有其他任务负责它。
- 保留仍服务于 owner 并发或外部事件的 fencing，不改变生产路径的语义；例外见“行为差异”。
- 文档部分：
  - roadmap 状态表、指标说明、§12 证据与 residual 台账；
  - 本计划状态行；
  - T10 与本记录；
  - 旧调用网络文档加历史横幅；
  - T10 设计 §3 补记错误归类。

## 提交

| 提交                                                                     | 内容                                                                |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------- |
| `refactor(workflow): remove the dead dirty-rebuild signal`               | 删除从未触发的 dirty-rebuild 信号                                   |
| `refactor(effects): drop the legacy runtime-apply classification`        | 删除 `runtime_apply_kind` 一族与 retry map 残留 `with_retries`      |
| `refactor(tray): move the feat helpers to the boundaries that call them` | `feat.rs` 的 helper 迁到托盘和 `utils/proxy_env.rs`，删除 `feat.rs` |
| `refactor(client): remove core lifecycle routes nothing calls`           | 删除没有调用者的 core lifecycle 路由                                |
| `refactor(client): remove the facade methods a blanket allow hid`        | 删除 `NyanpasuClient` 上 blanket allow 掩盖的 11 个 facade 方法     |
| `chore(ledger): exempt the legacy schema's only reader from DTO refs`    | ledger 把 legacy schema 的唯一读者列入 allowlist                    |

文档提交在其后，见本分支 `git log`。

## 删除与保留

- **dirty 信号**：`DirtyNotifier`、`DIRTY_WINDOW`、`DirtyTick`、`Command::RuntimeDirty`、`RebuildNotifier`。它从未触发：`client/mod.rs` 构造时就丢弃了 sender，`has_changed()` 返回 `Err`，又被 `unwrap_or(false)` 吞掉。
- **旧的 runtime 分类**：`runtime_apply_kind`、`RuntimeApplyKind`、`ClashRuntimeDesired`。它们只剩测试调用，实际分类已由 `application_workflow/impact.rs` 承担。同时删除 facade retry map 的残留 `ApplicationEffectPlan::with_retries`。
- **死路由**：`SetExecutionHost`、`ApplyControlChannel`、`RecoverCore`、`regenerate_runtime`、`rebuild_running_config`、`promote_existing_runtime_product`、`start_promoted_runtime`。托盘“重启 core”原先经 `rebuild_running_config`，现在直接调用 `reconcile_core`，与 `restart_sidecar` 同路。
- **R31**：删除 `impl NyanpasuClient` 上的 `#[allow(dead_code)]`。它掩盖的 11 个方法没有生产调用者，已删除；相关测试改走生产路径，没有删除测试。
- **`feat.rs`**：
  - `restart_core` 成为托盘私有函数；
  - `CopyEnvOption` 与 `copy_clash_env` 迁到 `utils/proxy_env.rs`，文本生成是纯函数，有逐字节测试；
  - bindings 字节不变。
- **T11 点名、但在前序层已消失的机制**：`commit_and_reconcile` mutex、源提交后的重复 rebuild、旧启动 effects 管线、`reconcile_application_effects`（T10 删除）、`update_systray` 事件及其监听（L4 删除，托盘工作改经主线程执行器请求）。
- **保留的 fencing**：
  - workflow 准入 FIFO、`MAX_PENDING`、closing 与隔离门；
  - mutation context 与 decision waiter、准入预算；
  - `RecoveryTick` / `ConvergenceTick`、service 恢复、`start_permitted` 与所有权；
  - `execute_lifecycle` 的逐命令 `notify_committed(true)`：lifecycle 命令仍可能改变 confirmed 端口绑定和 core/service 状态，托盘重启依赖它刷新；
  - `reconcile_core` / `Command::Reconcile`：`restart_sidecar`、`enhance_profiles` 和托盘仍在使用。

## 改变行为或接口的 leader 裁定

- **R29**：保留 `NyanpasuClient::stop_core` 与 `Command::StopCore`。它们是 `CoreIntent::Stopped` 的唯一构造者，也是 V11 与 T10 S18 显式 stop 意图的 facade 入口。方法带一个限定到该项、写明理由的 `#[allow(dead_code)]`。目前没有 UI 入口调用它。
- **R30（修订）**：删除 `change_execution_host` facade 方法。生产环境切换宿主只经 `enable_service_mode` TCC mutation。`ApplicationWorkflowClient::change_host`、`Command::ChangeHost`、`Output::Handoff` 改为 `#[cfg(test)]` seam，供 7 个测试直接驱动 `move_execution_host`，其中 5 个组合 service 恢复策略。
- **ledger**：`legacy_dto_refs` 的 allowlist 条目在 `core/migration/legacy_schema/` 之外，增加 `core/migration/modules/typed_config.rs`，按单个文件精确匹配，理由相同：升级输入 schema，不是应用 DTO。新增测试覆盖同目录其他模块和同前缀路径；两项变异（删掉条目、改回前缀匹配）都使测试失败。

## 行为差异

- 托盘“重启 core”成功后，少发一次重复的 `StateChanged::ClashConfig`。刷新仍由 lifecycle 命令末尾的 `notify_committed(true)` 触发，成功和失败都会刷新，与 `restart_sidecar` 一致；`a_core_reconcile_notifies_the_ui`（所有者已证实）与 `an_explicit_start_notifies_the_ui`（所有者未证实时的显式启动）覆盖这条刷新。
- workflow 等待超时的错误文字由 “inspect core_lifecycle_status” 改为 “inspect Configuration status”，因为前者已不存在。没有测试依赖这段文字。
- 其余删除项在生产中不可达。

## 文档交付

- [`actor-migration-roadmap.md`](../design/actor-migration-roadmap.md) 的改动：
  - §1.3 换成本计划 §15 的选择性 TCC 文本（R33）；
  - §2 更新 PR-6、PR-7a、PR-7b 三行；
  - §11 如实描述 `mutable_statics`（沿用旧 id，实际统计 allowlist 之外的全部非 const static）和 DTO allowlist；
  - §12.1 列出逐条证据，§12.2 列出 residual 的 owner 与移除条件。
- 本计划状态行指向 T6–T11 记录。
- [`legacy-iverge-call-network.md`](../architecture/legacy-iverge-call-network.md) 顶部加历史横幅，正文不改。
- T10 设计 §3 补记：`commit_file_first` 的全部错误，包括 persist IO 和版本冲突，都归入 `*_rejected`。
- R33：本计划 §15 要求设计文档不再与失败策略矛盾。[PR-6 effect 设计](../superpowers/specs/2026-09-12-pr6-application-effects/design.md) 与 [runtime apply options 计划](2026-09-13-runtime-apply-options.md) 顶部加了带日期的状态横幅：冲突处以本计划 §2 与 roadmap §1.3 的失败矩阵为准（关键 runtime 失败可以拒绝；外围效果在提交后执行，绝不回滚源配置），并指向本计划与 T10 设计。正文未改写。
- `AGENTS.md` §9 保持不变。那份清单用于识别 legacy 写法，不代表这些符号仍然存在；旧分支和历史 diff 里仍会遇到它们。`CLAUDE.md` 只 `@AGENTS.md`，无需同步。

## 验证记录

环境：

- 本机 macOS（Darwin 27.0.0，aarch64）。
- 工具链：`rust-toolchain.toml` 的 nightly，rustc 1.100.0-nightly (4aa1fbcf4 2026-09-08)。
- pnpm 12.4.2、deno 2.9.7、node v26.9.0。

全部命令在 L5 head 上运行，此时各轮 CCG 评审的修正都已折叠进各自的提交；文档提交只改 `docs/`，不影响结果。

- `cargo test --manifest-path backend/Cargo.toml --workspace --all-features`：退出 0。20 个测试目标合计 **1253 passed、0 failed、2 ignored**，其中：
  - clash-nyanpasu lib：939 passed、1 ignored；
  - nyanpasu-config：152 passed，doc-test 1 ignored；
  - nyanpasu-core：122；boa_utils：16；
  - fake-core：单元 7、protocol 13；
  - nyanpasu-helper：3；nyanpasu-egui：1。
- 同一次运行中 `an_expired_ack_keeps_the_domain_until_the_handoff_is_compensated` 通过，没有挂起。
- `cargo clippy --manifest-path backend/Cargo.toml --workspace --all-features --all-targets`：退出 0，无 error。
  - clash-nyanpasu 去重后 83 个 warning 位置，与 Task 10 结束时相同，修正批次没有新增位置。
  - 其余 warning 均为既有：submodule（nyanpasu-ipc 3、nyanpasu-core-manager 3）、tauri-plugin-deep-link 8、若干 manifest lint。
- `cargo fmt --manifest-path backend/Cargo.toml --all -- --check`：退出 0。
- `export_typescript_bindings`：通过，`bindings.ts` 无 diff。
- `pnpm -F interface build`：退出 0。
- `pnpm typecheck`：四个 tsc 项目均退出 0。
- `pnpm test:frontend`：Test Files 12 passed、Tests 60 passed。
- `pnpm lint:architecture-ledger`：扫描 305 个 Rust 文件，`config_calls`、`service_globals`、`migration_markers`、`legacy_dto_refs`、`test_real_dirs`、`mutable_statics`、`bridge_files` 全部为 0。allowlist 内的 static 共 37 个：immutable 32、external 4、test 1。gate passed，快照与报告一致。
- `pnpm test:architecture-ledger`：44 passed、0 failed。
- `pnpm lint:deno`：退出 0，检查 31 个文件。
- Task 10 各提交单独执行 `cargo check --all-targets --all-features`，均退出 0。每个提交都运行了 pre-commit hook，按改动类型执行 clippy、rustfmt、deno fmt/check 或 prettier。

## 尚未验证与边界

- T11 的完成条件包括 GUI smoke（语言、托盘、热键、代理、启动退出），本记录**未执行**，待维护者按 roadmap §1.8 留存证据。在此之前不宣称 T11 或整条 actor migration 关闭。
- 整栈尚未推送，没有三平台 CI 结果。L4 中 Windows/Linux 专属改动只经评审人阅读和本机 cfg 翻转检查（R25），编译与运行待 CI。
- 其余 residual 及各自的 owner、移除条件见 roadmap §12.2：ChangeHost seam、`stop_core` 无 UI 入口、`dirs::*` 自由函数、shutdown hook static、已停止的日志 actor 被记为 `Incomplete`、托盘调用 Tauri 菜单 API 的步骤只靠维护者 smoke、Native 模式重建托盘图标时重复注册菜单处理器、深链接去重只能按正在处理的链接近似、迁移覆盖测试仍逐字段构造输入、最小化后退出不保存几何等。
