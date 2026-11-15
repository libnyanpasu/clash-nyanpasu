# RPC 审计日志实施计划

**依据：** [`docs/spec/2026-11-13-rpc-audit-log/design.md`](../spec/2026-11-13-rpc-audit-log/design.md)（r4）。spec 与本计划冲突时以 spec 为准；spec 与 AGENTS.md / development guides 冲突时以后者为准。

**分支：** `feat/rpc-audit-log`，worktree `G:/Programs/Rust/.ccg/clash-nyanpasu/rpc-audit-log`，基线 `main@729e05f80`。

**提交规则：** 每张卡一个提交，提交后能独立构建；只 stage 明确列出的路径；主题用祈使句，不超过 72 字符，不带句号。

## 涉及的指南规则

- **rust.md**
  - §7.2：每个模块自有 snafu 错误类型，变体按失败步骤命名并带上下文。
  - §8：阻塞 I/O 不得在 async worker 上执行，用 `spawn_blocking`。
  - §5.4：共享所有权只限 `AuditDropped`，即 `Arc<AtomicU64>`，需带注释说明理由。
- **architecture.md**
  - 依赖由组合根显式注入，不加全局状态。
  - actor 不另设队列：tick 驱动取数，邮箱仍是唯一的串行化手段。
- **backend-packages.md**：observers 只放 owner 与端口，tsdb/redb 实现放 core，依赖方向为 rpc → core → observers。
- **rpc.md**：新命令声明 `rpc(http)`，归入 queries，重新生成 bindings，不手改。
- **typescript.md**
  - 页面用嵌套文件路由，模块放 `_modules/`。
  - i18n 按功能命名空间组织。
  - 组件用 `data-slot`。

## 卡片

### A0 文档

- **内容：** 提交 spec r4 与本计划。
- **验证：** 用 prettier 检查这两个文件。

### A1 审计 tracing 一律 TRACE

- **代码：** `nyanpasu-rpc/src/audit.rs` 的 `TracingRpcAuditSink` 末尾不再按传输分级，统一为 `tracing::Level::TRACE`。
- **测试：** 更新三处级别断言，改为全部是 TRACE。
  - `nyanpasu-rpc/src/local/tests.rs:691`
  - `nyanpasu-rpc/src/registry/audit_tests.rs:938-942`
  - `tauri/src/unified_rpc/audit_tests.rs:168`
- **文档：** `rpc.md` 中 "HTTP and local use INFO and Tauri IPC uses DEBUG…" 一句改为一律 TRACE。
- **验证：**
  - `cargo test -p nyanpasu-rpc`
  - `cargo test -p clash-nyanpasu unified_rpc::audit_tests`

### A2 配置 `rpc_audit`

- **代码：**
  - `nyanpasu-config/src/application/` 新增 `rpc_audit.rs`。`RpcAuditSettings { max_age_days: u16, max_size_mib: u16 }` 的写法照 `core_logs.rs`：`Default` 为 7 / 16，`validate()` 校验范围 1..=90 与 4..=256。
  - `NyanpasuAppConfig` 加字段 `#[serde(default)] rpc_audit`。
  - 新增 `ConfigError::InvalidRpcAudit`。
  - 启动校验与修改校验两处，照 `core_logs` 的同位置接入。
  - `nyanpasu-core/src/state/mutation/impact.rs` 登记 `AppCase`，`impact: None`。
- **bindings：** 重新生成。
- **验证：**
  - `cargo test -p nyanpasu-config`
  - impact 覆盖测试
  - bindings 新鲜度检查

### A3 observers：审计 owner 与端口

- **新增 `nyanpasu-observers/src/audit/`：**
  - `model.rs`：`AuditRow`（spec §4.2 的 11 个字段，用拥有所有权的值）、`AuditFilter { transport, outcome }`、不透明的 `AuditCursor`、`AuditPage { rows, next, dropped }`、`AuditDropped`。wire 类型照 traffic model 派生 serde + specta。
  - `ports.rs`：
    - `AuditStore: Send + 'static`，方法为 `append(&mut self, rows: &[AuditRow]) -> Result<Appended>`、`maintain(&mut self, now, Retention)`、`query(&mut self, filter, before, limit)`。`Appended { late }`。
    - 错误必须能区分 `Backend`（致命）与普通错误。
    - `AuditRetention { fn retention(&self) -> (Duration, u64) }`。
  - `actor.rs` / `client.rs`：照 spec §4.4。
    - 用 `send_interval(10 s)` 发 `Tick`：一次取至多 `AUDIT_BATCH_ROWS = 1024` 行，在一个 `spawn_blocking` 里移入 store，依次执行 append 和 maintain，再随结果移回。
    - `late` 行与普通错误下的批次行数，计入 `dropped`。
    - `Backend` 错误：drop store、`receiver.close()`，之后查询返回 unavailable。
    - `post_stop`：先 `close()`，取完剩余行，写最后一批。
    - 关机走 owner-support 的现有约定，参考 `logs/actor.rs` 与 traffic。
- **测试：** 用假 `AuditStore`，经 `AuditClient` 覆盖 spec §6 的 actor 行。
- **验证：** `cargo test -p nyanpasu-observers audit`。clippy 无新告警。

### A4 core：tsdb 存储与 facade

- **新增 `nyanpasu-core/src/audit/`：**
  - schema：单一事件流，`Timestamp` 键，`Window(10 min)`，列见 spec §4.2。
  - `TsdbAuditStore<S>`：实现 `AuditStore`。
    - `batch_id` 从 `view.last_batch()` 恢复后递增，seq 由 `next_event_seq` 分配。
    - query 用 Backward 方向，单页上限 500，`AuditCursor` 包装 `EventCursor`。
  - 打开器：`<app_data_dir>/audit/rpc-audit.tsdb`，用 `Profile::low_end()`。
    - 复用 `traffic/storage.rs` 的 `disposable()`。若它是私有函数，提升为 crate 内共享，不复制。
    - 目录权限照 traffic。
  - 常量测试：`AUDIT_BATCH_ROWS ×` 单行最大 `Input` 字节 `≤ low_end().batch_bytes`。
- **client 接入：**
  - `client/audit.rs`：`SettingsAuditRetention(StateSnapshot<NyanpasuAppConfig>)`。
  - `ClientSetupArgs` 新增 `audit: Option<AuditSetup>`，内容为 store、receiver、dropped。
  - `with_parts` 启动 actor；启动失败时 drop receiver。
  - `NyanpasuClient::query_rpc_audit`：没有 actor 时返回与 traffic 相同形式的 unavailable。
- **测试：** 内存 tsdb 后端，覆盖 spec §6 的 `TsdbAuditStore` 与打开器两行。
- **验证：** `cargo test -p nyanpasu-core audit`；clippy。

### A5 rpc：sink、命令与组装

- **sink**（`nyanpasu-rpc/src/audit.rs`）：
  - 自由函数 `audit_row(&RpcAuditRecord, SystemTime) -> Option<AuditRow>`：只处理 Http/Local 的 `Rejected`、`Finished`、`Subscription`；`code` 截断到 128 B（按字符边界）。
  - 阶段与结果的字符串映射抽成共享函数，供两个 sink 共用。
  - `RecordingRpcAuditSink` 照 spec §4.3 实现。
  - `RpcAuditSink` 的注释按 spec D3 修改。
- **命令：** `commands/audit.rs` 新增 `query_rpc_audit`，声明 `rpc(http)`；在 `specta_export.rs` 注册为 query。
- **组装**（`tauri/src/setup.rs`、`nyanpasu-server/src/compose.rs`）：
  1. 创建 `mpsc::channel(AUDIT_CHANNEL_ROWS = 4096)` 与 `AuditDropped`。
  2. 打开 audit store，成功则交给 `ClientSetupArgs.audit`，失败则 drop receiver。
  3. `RpcAudit::new(Arc::new(RecordingRpcAuditSink { .. }))`。
- **bindings：** 重新生成。
- **文档：** `rpc.md` 的审计段按 spec §5 更新。
- **测试：**
  - sink 部分覆盖 spec §6 的 sink 行。
  - 双传输一致性：用现有 Tauri 与 HTTP 测试夹具，验证结构与错误映射一致。
- **验证：**
  - `cargo test -p nyanpasu-rpc`
  - `cargo test -p clash-nyanpasu unified_rpc`
  - `pnpm run lint:backend-boundaries`
  - `pnpm run lint:architecture-ledger`

### A6 前端：页面与设置

- **query：** `frontend/query/src/ipc/use-rpc-audit-log.ts`，用 `useInfiniteQuery`，不自动轮询。
- **页面：**
  - 新建 `pages/(main)/main/logs/audit/index.tsx` 与 `_modules/`。
  - `logs/route.tsx` 改用 `AnimatedOutletPreset`。
  - `LogsSourceTabs` 增加"审计"。
  - 页面内容照 spec §4.6：筛选栏、虚拟表格、加载更多 / 刷新、丢弃警示条。
- **设置：** `settings/nyanpasu/logs/_modules/rpc-audit-retention-config.tsx`，在 `index.tsx` 增加第四组。
- **i18n：** 六种语言；执行 paraglide compile，带 `--strategy custom-extension baseLocale`。
- **测试：** vitest 覆盖 spec §6 的前端行。
- **验证：**
  - 相关包逐个执行 `pnpm --filter <pkg> typecheck`
  - vitest
  - oxlint
  - prettier
- **评审要求：** UI 效果先给用户确认，再推送。

## 执行与评审

- **执行者：** A1–A5 交给 codex（gpt-6.1-sol，high），按卡依次执行。A6 交给 Opus 子代理，或由我实施。
- **每张卡完成后：** 由我审 diff，确认每行改动都能追溯到 spec，然后执行验证命令，再带钩子提交。
- **全部完成后：**
  - 跑一轮 `/ccg:review`，硬上限 3 轮。
  - 推送为 draft PR。
  - 真机冒烟：用 dev identity 并行跑 WebUI 与 local CLI 调用。
