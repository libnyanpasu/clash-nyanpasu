# 前端 warning / error 上报到应用日志

**日期：** 2026-10-05

**状态：** 已选定方案 A 并实施（分支 `feat/frontend-error-reporting`）；实施偏差见 §11。

**调查基线：** `main@3e22a3cba`（`feat(logs): configure Core log rotation and compression live (#5599)`）。

**需求：** 把前端的 `console.warn` / `console.error`、未捕获异常、未处理的 Promise rejection、React 错误边界捕获的错误转发到后端，写入应用日志文件，供后续离线分析（按指纹聚合、按版本/页面统计）。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec > 后续实施计划。

## 1. 决策与范围

1. 目的地是**现有应用日志**：`tracing` → JSON 行 → `<app logs>/clash-nyanpasu_*.log`（[logging.rs](../../../../backend/tauri/src/utils/init/logging.rs)）。不新增存储、不新增 redb、不新增独立文件。前端事件因此自动获得现有的轮转/保留策略（`max_files`、`max_file_size`）和应用日志查看器（`LogSource::App`）。
2. 事件**只落本地**。不向 Sentry SaaS 或任何外部服务发送，不引入 DSN 配置、不引入遥测开关。
3. 新增一个 Unified RPC mutation `report_frontend_events`，桌面与 HTTP（浏览器 UI）两种传输都可用；调用方身份取自注入的 `RpcOwner`，不信任客户端自报。
4. 后端是**无状态**的：纯服务做校验与截断，经一个窄的 `FrontendLogSink` 端口写日志。不新增 actor、不新增后端限流状态。限流、去重、批量在前端完成；后端只做单请求上限，磁盘总量由现有轮转兜底。
5. 前端捕获代码属于应用组合根（`@nyanpasu/nyanpasu`），在 `main.tsx` 安装；共享包不读写 `window` 全局钩子。
6. 用户设置的应用日志级别照常生效：级别为 `error` 时前端 warning 被过滤，`silent` 时全部丢弃。不为前端事件开旁路。

**包含：** 上述四类来源的捕获、规范化、指纹、去重/限流/批量、RPC、后端校验与写日志、两种传输的测试、开发文档。

**不包含：** 日志分析工具本身、source map 反解（见 §9）、query/mutation 错误的单独捕获（它们若已 `console.error` 则自然被捕获）、面包屑（breadcrumbs）、性能追踪、会话回放、日志上传/导出给开发者。

## 2. 已确认事实与现有契约

| 事实                                                                                                                                                    | 位置                                                                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 应用日志用 `fmt::layer().json()` 写文件，`app_filter` 把 `nyanpasu`、`clash_nyanpasu` 两个 target 设为配置级别，其余 target 固定 `warn`                 | `backend/tauri/src/utils/init/logging.rs`                                                                                                                                                 |
| 日志查看器按行解析 `timestamp/level/target/message`，`raw` 保留整行 JSON                                                                                | `nyanpasu-logging/src/protocol.rs` `LogRow`                                                                                                                                               |
| 现有前端仅有 `window.addEventListener('error', e => console.error(e))`，无 `unhandledrejection` 处理                                                    | `frontend/nyanpasu/src/main.tsx:23`                                                                                                                                                       |
| 根路由 `errorComponent: Catch` 只渲染错误，不上报                                                                                                       | `frontend/nyanpasu/src/pages/__root.tsx`                                                                                                                                                  |
| React 19.3.0，`createRoot` 支持 `onUncaughtError` / `onCaughtError` / `onRecoverableError`                                                              | `frontend/nyanpasu/package.json`                                                                                                                                                          |
| 前端共享包内已有 43 处 `console.error/warn`（如连接详情订阅失败、kv 读取失败）                                                                          | `git grep "console\.\(error\|warn\)" -- frontend/*/src`                                                                                                                                   |
| 稳定版构建不产出 source map（`sourcemap: isDev \|\| IS_NIGHTLY ? 'inline' : false`），因此稳定版堆栈是压缩后的位置                                      | `frontend/nyanpasu/vite.config.ts:141`                                                                                                                                                    |
| `rpc(owner)` 在桌面取窗口 label，在 HTTP 取会话 owner；已有 `open_log_session` 等先例                                                                   | `backend/tauri/src/ipc.rs:1516` 起                                                                                                                                                        |
| 仓库内无任何 Sentry 依赖                                                                                                                                | `git grep -i sentry`                                                                                                                                                                      |
| Sentry JS SDK 支持自定义 `Transport { send(envelope), flush(timeout) }`；可直接构造 `BrowserClient` 并只列出需要的 integrations 以树摇默认 integrations | [Sentry 文档：Transports](https://docs.sentry.io/platforms/javascript/configuration/transports/)、[Tree Shaking](https://docs.sentry.io/platforms/javascript/configuration/tree-shaking/) |

**未核实（需 spike）：** Sentry `BrowserClient` 在未提供 DSN 时是否仍创建 transport（据记忆不会，需要占位 DSN）；`@sentry/browser` 仅含所需 integrations 时的实际 gzip 体积。搜索引擎给出的体积数字互相矛盾且含杜撰选项，不予采用。

## 3. 方案选择（待确认）

两种方案共用 §4 的 DTO 和 §5 的后端，区别只在前端如何得到事件。

### 方案 A（推荐）：自建轻量捕获

约 200 行 TS：安装四类钩子 → 规范化为 `FrontendEvent` → 去重/限流/批量 → `rpc.reportFrontendEvents`。

- 优点：零新依赖；行为完全可控（尤其是 console 包装与重入保护）；不对 `fetch`/`XHR`/`history`/DOM 打补丁。
- 缺点：堆栈只保留原始 `error.stack` 字符串，不做跨引擎（WebView2 / WKWebView / WebKitGTK）帧解析；指纹用规范化后的首个堆栈帧，精度低于 Sentry 的帧级分组。

### 方案 B：Sentry SDK + 自定义 transport

`new BrowserClient({ dsn: <占位>, transport: makeRpcTransport, stackParser: defaultStackParser, integrations: [globalHandlersIntegration(), linkedErrorsIntegration(), dedupeIntegration(), captureConsoleIntegration({ levels: ['warn', 'error'] })] })`，React 19 根节点钩子接 `Sentry.reactErrorHandler()`。transport 从 envelope 中取 `event` 项，映射为同一 `FrontendEvent` DTO 后调用 RPC，后端不解析 Sentry 协议。

- 优点：成熟的跨引擎堆栈解析（结构化 frames）、`cause` 链、去重；未来若要接真实 Sentry 只需换 transport。
- 缺点：新依赖与体积（待实测）；需占位 DSN（待核实）；`captureConsoleIntegration` 与 SDK 自身日志的重入行为需验证；Sentry 事件模型比需求大，映射层要维护。

**推荐 A 的理由：** 需求是"本地日志 + 后续分析"，原始堆栈对 jq/脚本分析已足够；稳定版没有 source map，Sentry 的结构化帧在稳定版也只是压缩坐标，帧级分组的收益有限。DTO 与后端对两种方案相同，A 落地后若需要可无破坏地切到 B。

## 4. 事件模型（Rust 定义，specta 导出）

```rust
pub struct FrontendEventBatch {
    pub events: Vec<FrontendEvent>,
    /// 自上一批以来因前端限流被丢弃的事件数。
    pub dropped: u32,
}

pub struct FrontendEvent {
    pub kind: FrontendEventKind,   // console | uncaught_error | unhandled_rejection
                                   // | react_uncaught | react_caught | react_recoverable
    pub level: FrontendEventLevel, // warning | error
    pub message: String,
    pub error_name: Option<String>,
    pub stack: Option<String>,
    /// error.cause 链，最多 3 层，每层 name/message/stack。
    pub causes: Vec<FrontendErrorCause>,
    /// React componentStack（仅 react_* 种类）。
    pub component_stack: Option<String>,
    pub fingerprint: String,
    /// 去重窗口内同指纹出现的次数（≥1）。
    pub count: u32,
    /// 客户端时钟（Unix ms），仅作参考；日志行时间戳以后端写入时间为准。
    pub first_seen_ms: f64,
    pub last_seen_ms: f64,
    /// location.pathname + hash 路由路径，不含 query string。
    pub route: String,
}
```

窗口/会话标识、应用版本、平台由后端补充（owner 来自 `RpcOwner`，版本与平台后端已知），不由前端上报。

## 5. 后端设计

### 5.1 分层

```text
ipc::report_frontend_events  (#[nyanpasu_macro::rpc(http, owner)], mutation)
    -> NyanpasuClient::report_frontend_events(owner, batch)
        -> FrontendEventSanitizer::sanitize(batch, &limits)   // 纯服务
        -> dyn FrontendLogSink::write(owner, &[SanitizedEvent], dropped)  // 端口
            -> TracingFrontendLogSink  // 适配器：tracing::event!(target: "clash_nyanpasu::frontend", ...)
```

- `FrontendLogSink` 是 `#[cfg_attr(test, mockall::automock)]` 的窄 trait，由组合根构造并注入 `NyanpasuClient` 内部；不新增全局。
- target 固定为 `clash_nyanpasu::frontend`，因而受 `app_filter` 的用户级别控制，并可在查看器/离线分析中按 target 过滤。
- 每个事件写一条日志：`message` 字段为事件 message，其余字段作为结构化字段（`kind`、`fingerprint`、`count`、`route`、`owner`、`error_name`、`stack`、`causes`（JSON 字符串）、`component_stack`）。`dropped > 0` 时额外写一条 `warn`。JSON formatter 对控制字符转义，换行不会伪造日志行。

### 5.2 单请求上限（纯服务，常量集中定义）

| 项                          | 上限                   | 超限处理                                 |
| --------------------------- | ---------------------- | ---------------------------------------- |
| 每批事件数                  | 32                     | 保留前 32 个，其余计入 `dropped`         |
| `message`                   | 4 KiB                  | 在 UTF-8 边界截断并标记 `truncated=true` |
| `stack` / `component_stack` | 16 KiB                 | 同上                                     |
| `causes`                    | 3 层                   | 丢弃多余层                               |
| `route` / `error_name`      | 512 B                  | 同上                                     |
| `fingerprint`               | 128 B，限 `[0-9a-z_-]` | 非法则由后端按 message 重新计算          |

整批为空返回 `Ok`。DTO 反序列化失败按现有 `RpcError` 结构返回，不写日志。截断后单批最大约 32 × (4 + 16 + 16 + 3 × 20) KiB 量级，由每批上限与前端限流共同约束；后端不保存跨请求状态。

### 5.3 RPC 与绑定

- 在 `specta_export.rs` 注册为 **mutation**，经现有导出流程重新生成 `rpc-bindings.ts`，不手改绑定。
- 启用 `http`：浏览器 UI 同样需要上报，写入的是本地日志，访问仍受现有每次启动凭据与 Host/Origin 校验保护。
- 返回 `Result<()>`；错误保持结构化，两种传输一致。

## 6. 前端设计（方案 A）

### 6.1 位置

`frontend/nyanpasu/src/services/error-reporting/`：

- `normalize.ts`（纯）：`unknown` → `FrontendEvent` 草稿；处理 `Error`、`ErrorEvent`、`PromiseRejectionEvent.reason`、非 Error 值；console 参数序列化（Error 取 stack，对象安全 JSON 化：深度 4、总长 4 KiB、循环引用占位）；遍历 `cause`。
- `fingerprint.ts`（纯）：`kind + error_name + 规范化 message（数字/UUID/十六进制串替换为占位）+ 首个堆栈帧（去掉 query）` 的短哈希。
- `reporter.ts`：队列、去重、限流、批量、发送、重入保护；依赖以参数注入（`send: (batch) => Promise<void>`、`now()`、定时器），便于测试。
- `install.ts`：在组合根安装/卸载钩子；HMR `dispose` 时卸载。

### 6.2 捕获来源

| 来源                      | 钩子                                                                                                                           | kind / level                                |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------- |
| `console.warn` / `.error` | 包装原函数：先调用原函数（devtools 输出不变），再入队                                                                          | `console` / warning、error                  |
| 全局异常                  | `window` `error` 事件（替换 `main.tsx` 现有监听；资源加载错误按 `event.target` 区分并记录 URL 路径）                           | `uncaught_error` / error                    |
| Promise rejection         | `window` `unhandledrejection`                                                                                                  | `unhandled_rejection` / error               |
| React                     | `createRoot(container, { onUncaughtError, onCaughtError, onRecoverableError })`；`onCaughtError` 覆盖 TanStack Router 错误边界 | `react_*` / error（recoverable 为 warning） |

React 19 的根钩子替换了默认的 `console.error` 输出，安装时保留默认行为（调用原 `console.error`，但不再二次入队）。

### 6.3 去重、限流、批量

- 同指纹 60 秒窗口内合并为一个事件并累加 `count`、更新 `last_seen_ms`。
- 每窗口（webview）每分钟最多 60 个**不同**事件，超出计入 `dropped`。
- 达到 16 个事件或 1 秒后发送一批；`visibilitychange → hidden` 与 `pagehide` 时尽力发送。
- 发送失败不重试、不排队增长：本批计入 `dropped` 后丢弃。

### 6.4 重入与噪声

- 上报路径内部（含 RPC 传输层失败）只使用保存下来的原始 `console` 方法，并设置重入标记；标记期间的 console 调用不入队，避免 "上报失败 → console.error → 再上报" 的循环。
- 开发模式下照常上报（便于验证），但 HMR 卸载后不得留下重复包装：安装函数幂等，卸载恢复原函数。
- 不过滤具体消息内容；噪声治理留给分析阶段（按 fingerprint 聚合）。如确有已知无害噪声，后续在 `normalize.ts` 以显式列表处理，不在本次范围。

## 7. 隐私与安全

- 只写本地日志，与现有应用日志同等保护和保留期。
- `route` 只取路径，不含 query/hash 参数。错误消息本身可能包含订阅 URL 等敏感串，本次不做内容擦除；若未来新增"导出日志给开发者"，需在导出侧另行脱敏（记录为后续事项）。
- HTTP 传输复用现有凭据与 Host/Origin 检查；owner 由服务端注入，前端无法伪造他人身份。
- 单请求上限 + 前端限流 + 日志轮转共同约束磁盘占用；恶意的已认证客户端最多把日志轮转打满，不影响其它存储。

## 8. 实施顺序与文件边界

```text
T0（仅方案 B）spike：核实占位 DSN、实测体积、captureConsole 重入 -> 产出数据后再定是否继续 B
T1 后端：DTO + FrontendEventSanitizer + FrontendLogSink/TracingFrontendLogSink + NyanpasuClient 方法
        + ipc 命令 + specta 注册 + 重新生成绑定            -> verify: cargo test（纯服务/mock sink）、两传输 RPC 测试
T2 前端：normalize / fingerprint / reporter 单测 + install 接入 main.tsx 与 createRoot
                                                        -> verify: vitest unit；pnpm --filter 逐包 typecheck
T3 端到端：浏览器 HTTP fixture 触发四类来源，断言日志文件出现对应 target 与字段
                                                        -> verify: 现有 browser_debug_page 类测试
T4 文档：docs/development/ 增补"前端错误上报"小节（target、字段、级别过滤、上限）
```

每个 T 一个原子提交；T1 与 T2 可分 PR 叠加（T2 依赖 T1 的绑定）。

## 9. 验收标准

1. 在桌面与浏览器 UI 分别触发：`console.warn('x')`、`console.error(new Error('y'))`、`throw` 于事件回调、未处理 rejection、渲染期抛错（被路由错误边界捕获）——应用日志中各出现一条 `target = clash_nyanpasu::frontend` 的记录，`kind`、`level`、`owner`、`route`、`stack` 正确。
2. 同一错误 1 秒内触发 100 次，只产生 1 条记录且 `count = 100`。
3. 制造 200 个不同错误：每分钟最多 60 条，另有一条 `dropped` 记录与差值一致。
4. 让 RPC 失败（如断开 HTTP 会话）：不出现无限循环，devtools 只多出有界的原始输出。
5. 应用日志级别设为 `error` 时 warning 不落盘；`silent` 时全部不落盘。
6. 超长 message/stack 在 UTF-8 边界截断并标记；超出 32 个的批次计入 `dropped`。
7. 两种传输的错误映射一致；mutation 分类正确；绑定为生成产物。
8. HMR 多次热更新后 `console.error` 只被包装一层。

## 10. 未决事项

1. **方案 A / B 选择**（§3）。
2. **稳定版 source map**：稳定版堆栈是压缩坐标。可选的后续工作是构建时生成 `hidden` source map 作为 CI 产物（不随安装包分发），供离线反解；本 spec 不包含。
3. **query/mutation 错误**：是否在 `QueryCache`/`MutationCache` 的 `onError` 单独上报。当前倾向不做——大量是预期内的内核离线错误，会淹没真实问题。
4. 前端事件是否需要在日志查看器中提供"仅前端"快捷过滤（按 target）。属 UI 增强，可后续单独处理。

## 11. 实施偏差

1. `first_seen_ms` / `last_seen_ms` 为 `Option<f64>`：specta 把 `f64` 导出为 `number | null`（NaN 序列化为 null），非有限值归一为 `None`。
2. 指纹保留堆栈帧的行列号，只去 query：稳定版是压缩产物，几乎所有帧都在第 1 行，去掉列号会让不同问题撞指纹。
3. 去重语义明确为：窗口内首次出现即时入批（≤1 秒发送）并累加同批重复；首批发送后的重复被暂存，到 60 秒窗口结束时连同计数发送。
4. `installErrorReporting` 的目标收窄为 `ReportingTarget`，console 单独注入：DOM 类型中 `console` 不是 `Window` 成员，测试也需要不触发测试运行器全局错误处理的替身窗口。
5. 调试页 Advance Tools 增加 Error Reporting Test 卡片（console.warn / console.error / 未捕获异常 / 未处理 rejection / 渲染错误）供手动测试。端到端（真实浏览器 + HTTP）点击前四个按钮并断言四类事件到达 sink；React 根回调在浏览器测试中用替身窗口验证。桌面传输未冒烟。
6. 开发文档为 `docs/development/frontend-error-reporting.md`。
