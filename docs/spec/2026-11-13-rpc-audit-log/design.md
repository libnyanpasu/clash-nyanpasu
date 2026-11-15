# RPC 审计日志持久化与查看页

**日期：** 2026-11-13

**状态：** 设计稿 r4，未实施。

- r1–r3 经 codex（gpt-6-astra）审核三轮：58 → 78 → PASS 94。
- r4 按 Fable 的架构简化审查删去审查轮次累积的机制，三项取舍经用户裁定，见 §9。

**基线：** app `main@0f067133f`。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec。

## 1. 背景与目标

统一 RPC 已有完整的审计采集。`backend/nyanpasu-rpc/src/audit.rs` 的 `RpcAudit` 持有注入的 `Arc<dyn RpcAuditSink>`，每次调用按控制流记录各阶段，不含参数与返回值。HTTP/Local 的 owner 是实例密钥哈希。唯一的生产 sink `TracingRpcAuditSink` 把记录写入 tracing：HTTP/Local 为 INFO，Tauri 为 DEBUG。这些记录随应用日志一起轮转，没有独立的保留策略，也不能结构化查询。

目标：

1. 所有审计 tracing 事件降为 TRACE。
2. HTTP 与 Local 的终态审计记录写入 `nyanpasu-tsdb`，按"时长 + 字节"混合保留；两个上限有默认值，可在设置页修改。
3. 新增审计查看页。
4. sink 侧使用有界缓冲，满即丢弃，丢弃数可见。

非目标：

- Tauri IPC / TauriProtocol 不持久化，仍有 TRACE 事件。
- 中间阶段（admitted、delivery、observation_ended）不持久化，只在 TRACE 中。
- 不改变采集点、阶段语义或 owner 哈希方案。
- 不提供删除、清空、导出命令。
- 不保证持久性：这是尽力而为的诊断记录。

## 2. 现状

| 部件               | 位置                                                                                                                 | 现状                                                                                                    |
| ------------------ | -------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| 采集核与 sink 端口 | `backend/nyanpasu-rpc/src/audit.rs`                                                                                  | `RpcAuditSink::record(&self, &RpcAuditRecord)` 同步；注释要求"不得 panic 或排队"                        |
| 生产 sink          | `audit.rs:279-376`                                                                                                   | `TracingRpcAuditSink`；按传输分 INFO/DEBUG                                                              |
| 组装点             | `backend/tauri/src/setup.rs:642`、`backend/nyanpasu-server/src/compose.rs:355`                                       | 直接 `RpcAudit::new(Arc::new(TracingRpcAuditSink))`。`compose.rs:141` 是测试构造                        |
| 规则文档           | `docs/development/rpc.md:157-215`                                                                                    | 写死 INFO/DEBUG，并写明"Audit durability is diagnostic tracing only"                                    |
| 级别断言           | `nyanpasu-rpc/src/local/tests.rs:691`、`registry/audit_tests.rs:938-942`、`tauri/src/unified_rpc/audit_tests.rs:168` | 断言 INFO/DEBUG                                                                                         |
| 有界通道先例       | `nyanpasu-rpc/src/local/server.rs:709-733`                                                                           | `mpsc::channel(n)` + `try_send`                                                                         |
| 存储 actor 先例    | `nyanpasu-observers/src/logs/{actor,ports}.rs`                                                                       | 内核日志 actor 持有 `Box<dyn …Store>`，在 `spawn_blocking` 中移入移出；只有一个丢弃计数；故障后停止写入 |
| tsdb 先例          | `nyanpasu-core/src/traffic/storage.rs`                                                                               | redb 打开与可丢弃错误分类 `disposable()`（`:121`）                                                      |
| 页面 / 设置        | `frontend/nyanpasu/src/pages/(main)/main/logs/`、`settings/nyanpasu/logs/`                                           | 日志页有三个来源 tab；"日志与历史"设置页                                                                |

## 3. 决策

### D1 分发方式：依赖注入

组合根换成一个同时写 tracing 和有界通道的 sink。不采用 tracing Layer 截取，原因有三：

- 字段要先变成字符串，再解析回来；
- 依赖进程全局 subscriber；
- 是否落库会和日志级别过滤耦合在一起。

### D2 级别

`TracingRpcAuditSink` 对所有传输一律输出 `TRACE`。

### D3 有界通道

- 组合根创建 `mpsc::channel::<AuditRow>(AUDIT_BUFFER_ROWS)`，以及一个共享计数 `Arc<AtomicU64>`（`AuditDropped`）。
- sink 调用 `try_send`：
  - 返回 `Full` 时，`dropped` 加一。
  - 返回 `Closed` 时，表示存储未启动或已停用，直接返回，不计数。
- 不需要额外的开关标志：接收端被 drop，通道自然关闭。
- `RpcAuditSink` 的注释与 rpc.md 改为：允许非阻塞地 `try_send` 到有界、满即丢弃的诊断通道；仍禁止排队业务工作、等待外部服务或 panic。

### D4 持久化范围

- 只处理 `RpcTransport::Http | RpcTransport::Local`。
- 只处理 `Rejected`、`Finished`、`Subscription` 三种阶段。
- 每条这样的记录一行，不做跨行关联。

### D5 存储与保留

- **文件**：独立的 `<app_data_dir>/audit/rpc-audit.tsdb`，目录权限与 traffic 相同。
- **Profile**：使用 `Profile::low_end()`。它的 `durable_interval` 为 `None`，因此每个非空 tick 都是一次持久提交，最多每 10 s 一次。
- **事件流**：`EventKey::Timestamp`（毫秒），`ExtentSpec::Window(AUDIT_WINDOW = 10 min)`。
- **保留**：`Retention { max_age, max_bytes }` 两项同时生效，按整窗过期。
- **打开失败**：复用 traffic 的 `disposable()` 分类，只有可丢弃错误才在启动时删除并重建；其他错误保留文件，本次运行不记录。

### D6 配置

```rust
#[serde(default)]
pub rpc_audit: RpcAuditSettings,

pub struct RpcAuditSettings {
    /// 1..=90，默认 7。
    pub max_age_days: u16,
    /// 4..=256，默认 16。
    pub max_size_mib: u16,
}
```

- `Default` 是默认值的唯一来源，`validate()` 校验范围；错误类型 `ConfigError::InvalidRpcAudit`，与 `CoreLogSettings` 的写法相同。
- 采用拉取模式：actor 经端口 `AuditRetention` 在每个 tick 读取配置快照。
- `impact.rs` 登记 `AppCase { field: "rpc_audit", impact: RuntimeImpact::None, owners: &[] }`。

## 4. 设计

### 4.1 模块归属

依照内核日志的形状：

- **actor**：放在 observers，非泛型，持有一个 `Box<dyn AuditStore>` 端口。
- **tsdb 实现**：放在 core。
- **sink**：放在 rpc。

依赖方向是 rpc → core → observers，均为已允许的方向。

| 内容                                                                                   | 位置                                                   |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| `AuditRow`、`AuditPage`、`AuditCursor`、`AuditFilter`、`AuditDropped`                  | `nyanpasu-observers/src/audit/model.rs`                |
| 端口 `AuditStore`（append / maintain / query）、`AuditRetention`                       | `nyanpasu-observers/src/audit/ports.rs`                |
| `AuditActor`、`AuditClient`                                                            | `nyanpasu-observers/src/audit/{actor,client}.rs`       |
| tsdb schema、`TsdbAuditStore`、打开器                                                  | `nyanpasu-core/src/audit/`                             |
| `SettingsAuditRetention`、`NyanpasuClient::query_rpc_audit`                            | `nyanpasu-core/src/client/`                            |
| `RecordingRpcAuditSink`、自由函数 `audit_row(&RpcAuditRecord, at) -> Option<AuditRow>` | `nyanpasu-rpc/src/audit.rs`                            |
| RPC 命令                                                                               | `nyanpasu-rpc/src/commands/audit.rs`                   |
| 组装                                                                                   | `tauri/src/setup.rs`、`nyanpasu-server/src/compose.rs` |

### 4.2 行模型

| 列              | tsdb 类型             | 来源                                     |
| --------------- | --------------------- | ---------------------------------------- |
| `at`            | Timestamp（键，毫秒） | sink 转换时的 `SystemTime::now()`        |
| `transport`     | Dict                  | `http` / `local`                         |
| `method`        | Dict                  | `header.method`（`&'static str`）        |
| `owner`         | Dict                  | 哈希后的 owner（定长格式）               |
| `invocation_id` | U64                   | header                                   |
| `request_id`    | U64，可空             | `NonZeroU64`                             |
| `phase`         | Dict                  | `rejected` / `finished` / `subscription` |
| `outcome`       | Dict，可空            | 沿用 `TracingRpcAuditSink` 现有拼写      |
| `kind`          | Dict，可空            | `&'static str`                           |
| `code`          | Dict，可空            | 唯一无界的字段，截断到 128 B             |
| `duration_ns`   | U64                   | 单调耗时                                 |

- 阶段与结果的字符串映射抽成一个函数，供两个 sink 共用。
- owner 的哈希密钥每次运行都不同，跨重启无法关联同一调用方。页面文案需要说明这一点。

### 4.3 sink

```rust
pub struct RecordingRpcAuditSink {
    rows: mpsc::Sender<AuditRow>,
    dropped: AuditDropped,
}

impl RpcAuditSink for RecordingRpcAuditSink {
    fn record(&self, record: &RpcAuditRecord) {
        TracingRpcAuditSink.record(record);
        if let Some(row) = audit_row(record, SystemTime::now())
            && let Err(TrySendError::Full(_)) = self.rows.try_send(row)
        {
            self.dropped.increment();
        }
    }
}
```

- `audit_row` 负责按传输与阶段过滤：不符合 D4 的记录返回 `None`。
- 组合根的流程：先创建通道，再打开存储。
  - 打开成功：把 `(store, receiver, dropped)` 经 `ClientSetupArgs` 交给 core，由 core 启动 actor。
  - 打开失败，或 actor 启动失败：drop 接收端。此后 sink 的 `try_send` 一律返回 `Closed`，退化为只写 tracing。

### 4.4 actor

- **tick**：`pre_start` 用 ractor 的 `send_interval(AUDIT_TICK = 10 s)` 发送 `Tick`。时钟使用 `SystemTime::now()`，与行键同源。
- **`Tick`**：
  1. 用 `try_recv` 取出至多 `AUDIT_BATCH_ROWS` 行。
  2. 在同一个 `spawn_blocking` 中写一批，并执行 `maintain(now, Retention 来自 AuditRetention)`。写批时 `batch_id = last_batch + 1`，seq 由 `next_event_seq` 按取出顺序分配。store 移入闭包、处理后随结果移回。
  3. 写入结果中的 `late` 行数，以及失败批次的行数，都加到 `dropped`。
- **一次取出就是一批**：`AUDIT_BATCH_ROWS = 1024`，并用常量测试断言 `AUDIT_BATCH_ROWS × 单行最大编码字节 ≤ low_end().batch_bytes`。因此不需要按字节拆批。没取完的行留在通道里，下个 tick 再处理。
- **`Query`**：在同一个 `spawn_blocking` 边界内扫描，`Direction::Backward`，单页上限 500，返回 `AuditPage { rows, next, dropped }`。
- **失败处理**：
  - 普通写入错误：丢弃该批，计入 `dropped`，然后继续。
  - `BackendFailed`，或写入结果未知：drop store，关闭接收端，停止记录直到重启；之后的查询返回 `unavailable`。
  - 两种情况都不重放、不做恢复。
- **关机**：`post_stop` 关闭接收端，取出剩余的行，写最后一批。关闭之后才到达的记录会丢失，不计数。

### 4.5 RPC

```rust
#[nyanpasu_macro::rpc(http)]
async fn query_rpc_audit(
    client: RpcState<NyanpasuClient>,
    filter: AuditFilter,          // transport? / outcome?
    before: Option<AuditCursor>,
    limit: usize,
) -> Result<AuditPage>;
```

- 归入 queries，并重新生成 bindings。
- 开放 HTTP，供 WebUI 使用。返回内容只有方法名、哈希 owner、耗时和错误分类。
- 存储不可用时，返回与 traffic 相同形式的不可用错误。
- `AuditCursor` 包装 tsdb 的 `EventCursor`，对外不透明。
- 前端 hook 是 `useRpcAuditLog`（`frontend/query/src/ipc/`），基于 `useInfiniteQuery`。挂载时拉取第一页，"加载更多"取更早的页，"刷新"重置到第一页。不自动轮询。

### 4.6 页面

- **路由**：`/main/logs/audit`，文件在 `pages/(main)/main/logs/audit/index.tsx`，模块放在 `_modules/`，遵循 typescript.md "Organize routed pages and their modules"。
- **父路由**：`logs/route.tsx` 的 `<Outlet />` 改为 `AnimatedOutletPreset`。
- **来源 tab**：`LogsSourceTabs` 增加"审计"项，链接到 `/main/logs/audit`；其余三项不变。
- **筛选**：左侧栏放传输筛选（全部 / HTTP / Local）和结果筛选（全部 / 成功 / 失败 / 拒绝），不使用 `LogLevelsLayout`。
- **表格**：`DataPage` + `useVirtualizer`，列为时间、传输、方法、调用方、阶段/结果、耗时、错误码。
- **丢弃提示**：`dropped > 0` 时，页面顶部显示警示条"本次运行中有 N 条审计记录未保存"。
- 不新增顶部导航项。

### 4.7 设置

在"设置 › Nyanpasu › 日志与历史"页新增第四组"RPC 审计"：

- **控件**：`_modules/rpc-audit-retention-config.tsx`，两个 `SettingsSliderRow`，分别是天数 1–90 和容量 4–256 MiB。用 `useSetting('rpc_audit')` 整对象替换写回。
- **说明文案**，告诉用户三件事：
  - 缩短保留期会不可恢复地删除记录；
  - 只记录 HTTP 与本地通道的调用；
  - 调用方标识只能在单次运行内关联。
- **i18n**：更新 `settings_label_logs_description`，六种语言都补上新 key，然后执行 paraglide compile，带 `--strategy custom-extension baseLocale`。

## 5. 文档同步

- `docs/development/rpc.md`：
  - 审计级别改为一律 TRACE；
  - 增加一段 HTTP/Local 终态持久化的说明，包括存储、保留、丢弃语义，以及故障后停用到重启；
  - sink 契约按 D3 修改。
- `audit.rs` 中 `RpcAuditSink` 的注释同步修改。

## 6. 测试

| 层                                         | 用例                                                                                                                                                                  |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| sink                                       | Tauri 两种传输不发送；HTTP/Local 只发送三种终态阶段；`Full` 时计数，`Closed` 时不计数也不阻塞；tracing 事件全部为 TRACE（替换 §2 中三处级别断言）；超长 `code` 被截断 |
| actor（假 `AuditStore`，经 `AuditClient`） | tick 取出的行写成一批；`late` 和失败批次计入 `dropped`；`BackendFailed` 后停止记录、查询返回 unavailable、sink 侧看到 `Closed`；`post_stop` 写最后一批                |
| `TsdbAuditStore`（内存 tsdb）              | 写入后能分页回读；同一毫秒内按 seq 排序；`max_age` 过期；`max_bytes` 按整窗过期，过期后仍能继续写入；配置变更在下一个 tick 生效；`AUDIT_BATCH_ROWS` 常量断言          |
| 打开器                                     | 可丢弃错误时删除并重建；锁或权限错误时保留文件，本次不记录，应用照常启动                                                                                              |
| 配置                                       | 默认值；缺字段时取 Default；越界被拒；`impact.rs` 覆盖测试                                                                                                            |
| 双传输                                     | `query_rpc_audit` 经 Tauri 和 HTTP 返回相同的结构与错误映射                                                                                                           |
| 前端                                       | 筛选参数正确映射到请求；"加载更多"与"刷新"；丢弃警示条                                                                                                                |

需要跑的检查：相关 crate 的 `cargo clippy` / `cargo test`、bindings 新鲜度、`pnpm run lint:backend-boundaries`、`pnpm run lint:architecture-ledger`、前端逐包 typecheck。

## 7. 成功判据

1. 应用日志级别为 debug 时，没有 `nyanpasu::rpc_audit` 事件；级别为 trace 时，四种传输都有。
2. 经 WebUI 与 local CLI 发起的调用，在 10 s 内落库，刷新审计页即可看到；桌面窗口内发起的调用不会出现。
3. 把容量设为 4 MiB 后压测写入，记账字节回落到上限以内；保留期改为 1 天后，下一个 tick 起超过 1 天的窗口被删除。
4. 通道满时，调用不会等待；审计页出现丢弃警示。

## 8. 已知限制

- 中间阶段不落库；Local 的 delivery 失败只能在 TRACE 中看到。
- WebUI 的各类轮询（流量、系统代理等）本身也会产生审计记录，开着这些页面时，容量上限会先于时长上限生效。
- owner 和丢弃计数都只在单次运行内有效。
- 崩溃和关机尾部的丢失不计数。
- 存储故障后停止记录，直到重启。
- 最新窗口自身超过容量上限时，最长 10 分钟不记录，这部分计入 `dropped`。
- 时钟回拨可能让行出现在已加载页之外，或被当作 late 丢弃。

## 9. 审查记录

**r1–r3**：codex 三轮，58 → 78 → PASS 94。逐条处理见本文件的 git 历史（r3 版本的 §9）。

**r4**：Fable 架构简化审查。

| 项                                                                                                                                                           | 处理                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------- |
| 只存终态                                                                                                                                                     | 用户裁定采纳。删除 run 标识、详情侧栏、`all_phases` 开关                                 |
| 共享队列 + `active` 标志 → 有界 mpsc + `try_send`                                                                                                            | 采纳。通道关闭即表示停用                                                                 |
| 四类丢失计数 + 失败表 → 单一 `dropped`                                                                                                                       | 采纳                                                                                     |
| 运行期恢复 → 故障后停用到重启                                                                                                                                | 用户裁定采纳                                                                             |
| 排除审计查询自身                                                                                                                                             | 删除。WebUI 本来就有大量轮询，单独排除这一项没有效果；而且会让 `audit.rs` 依赖具体命令名 |
| 最新视图轮询规则 → 不自动轮询，提供刷新按钮                                                                                                                  | 用户裁定采纳                                                                             |
| 按字节拆批 → 一次取出即一批，加常量断言                                                                                                                      | 采纳                                                                                     |
| 行精简：16 列 → 11 列；只截断 `code`；`request_id` 改为 U64                                                                                                  | 采纳                                                                                     |
| 三个端口 → `AuditStore` + `AuditRetention`，tsdb 实现移入 core                                                                                               | 采纳                                                                                     |
| 删除查询下界截断；单独的 rpc-audit.md 并入 rpc.md                                                                                                            | 采纳                                                                                     |
| 必须保留：`spawn_blocking`、嵌套路由、依赖注入 sink、tsdb 的 batch_id/seq 约定、拉取式保留、不透明游标、`rpc(http)`、`code` 截断、最终 drain、可丢弃错误判定 | 保留                                                                                     |
