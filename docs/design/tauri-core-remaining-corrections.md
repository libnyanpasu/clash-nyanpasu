# Tauri/core 分离：阶段状态与剩余修正

> 整体目标与阶段定义见[迁移计划](tauri-core-extraction-plan.md)。本文区分已交付的 P09、待实施的 P10/P11，以及独立 OPEN 决策。
> P09 状态基于已审核并推送的 [`2442b66a1`](https://github.com/libnyanpasu/clash-nyanpasu/commit/2442b66a18db91096bcd61773bcca70fd5cffac9)。[PR #5717](https://github.com/libnyanpasu/clash-nyanpasu/pull/5717) 以 P08 #5707 为 base，已加入现有 stack #5681；分支交付不等于已合并到默认分支或全部平台验收完成。
> 后续改动仍按原子范围人工审核后提交；本文不自动授权新的构建、清理、历史改写或合并。

下文源码路径以 `backend/` 为根。

## 1. 先给结论

1. **P01–P08 的共享领域能力主体保留，不整体回退，也不再搬一遍。** CLI 直接使用 `NyanpasuClient`，不需要新增独立 HTTP/RPC 服务。
2. **P09 的 R0–R5 已完成实现、组合定向验收、独立审查、人工审核和推送。** HTTP、telemetry、icon、GUI session owner 与前端投影已按实际职责闭合；不再作为未实施候选重复施工。
3. **下一阶段是 P10 / R6 的共享启动准备。** 仅处理真实 `init_config` 与受保护 traffic opener 两个入口，不扩大成 process/bootstrap 大搬迁。
4. **还有独立 Rust consumer 验收待建立。** 这是 P11 的工作，不是实现 CLI 产品，更不是重新提取 HTTP 服务。
5. **内核更新目的地、无人值守宿主要求、生命周期问题须单列。** 不把尚未决定的产品政策写成已经确定的迁移，也不把已发现的关闭风险隐藏到 P10/P11。

## 2. 当前状态与下一步的区别

| 编号 | 项目                             | 分支交付状态                                   | 验证边界或下一步                                                                  |
| ---- | -------------------------------- | ---------------------------------------------- | --------------------------------------------------------------------------------- |
| R0   | `IpcError` 候选处置与 `syn full` | 已完成；wire error 留 host，宏直接声明 feature | 独立 macro package 编译和回归通过，不重启中立 RPC 设计                            |
| R1   | HTTP hosting 回宿主              | 已完成并整合                                   | 原 HTTP/RPC 回归、host 消费者、真实 exporter 通过；浏览器用例仍 ignored           |
| R2   | Frontend telemetry 回宿主        | 已完成并整合                                   | 七个原测试和 State 回归通过；真实 browser fixture 保留但未运行                    |
| R3   | Icon cache 回 presentation       | 已完成并整合                                   | 源等价、消费者编译与 exporter 通过；没有 TTL/外网下载运行覆盖                     |
| R4   | GUI session state 迁出 core      | 已完成版本、query/event 与保存/drain 闭包      | 真实 owner、持久化、status 和退出保存定向测试通过；不等于 native 生命周期全部闭合 |
| R5   | Proxy raw API 与前端投影分离     | 已完成；领域缓存 owner 不变                    | 19 个 owner、12 个 assembly、6 个 host projection 测试通过                        |
| R6   | 共享启动准备                     | P10，待实施                                    | 两个窄入口按实际 consumer 提取，保留原失败/保护行为                               |
| R7   | 独立 Rust consumer 验收          | P11，待建立                                    | 外部公开 API 使用证据及必要持续验证接线                                           |

### 2.1 P09 的实际验证与未运行项

组合候选的 Linux 定向结果为 **105 次 Rust 测试执行通过、100 个不同 package/test 通过**：macro 11、core 44、host 50 次；五个 HTTP 测试在两种 feature 配置各运行一次，重复执行不重复计入 unique。原 browser/Playwright 用例 **1 项 ignored**，Deno backend-boundary fixtures **3 项通过**。此前独立 R1 的结果不累计进这些数字。

macro/core 编译、host default/all-features 编译、相关 Clippy、31 个改动 Rust 文件格式检查，以及 architecture/backend-boundary gates 通过。实际 Specta exporter 执行一次（已计入上面的 host 数量），三份 bindings 字节不变。组合锁协调保留全部 **1,264 个完整 source-qualified 外部 package records** 和非 package 字段；相对 R1 锁，仅 bytes/postcard 的 core/host 直接 owner 改变，R1 的 HTTP 依赖归属另计。

代表性已执行入口（省略环境专属 target-dir 和日志参数；不是完整 workspace 测试套件）：

```sh
cargo test --manifest-path backend/Cargo.toml --locked --offline --jobs 2 -p nyanpasu-macro --lib unified_command::tests::
cargo test --manifest-path backend/Cargo.toml --locked --offline --jobs 2 -p nyanpasu-core --lib client::proxies::tests::
cargo test --manifest-path backend/Cargo.toml --locked --offline --jobs 2 -p clash-nyanpasu --lib --all-features desktop::session_state::tests::
cargo test --manifest-path backend/Cargo.toml --locked --offline --jobs 2 -p clash-nyanpasu --lib unified_rpc::tests::
cargo test --manifest-path backend/Cargo.toml --locked --offline --jobs 2 -p clash-nyanpasu --lib specta_export::tests::export_typescript_bindings -- --exact
deno task lint:architecture-ledger
deno task lint:backend-boundaries
deno task test:backend-boundaries
```

发布时 R1–R5 又分别在完整串行前缀上通过了正常 hooks 的 `cargo clippy --manifest-path=./backend/Cargo.toml --all-targets --all-features`、`cargo fmt --manifest-path ./backend/Cargo.toml --all` 及适用的 Prettier/commitlint。这是额外的前缀编译/格式证据，不是每个前缀重跑了上述行为测试；首个 R0 hook 在组合工作树上运行，不能据此声称独立 R0 前缀已运行验证。

初次组合测试编译曾因两处旧 status-test tuple 仍引用 `session` 而 E0609/exit101，测试尚未开始；完成两行调用者适配后通过。原失败、工具/路径检查失败与后续限定恢复不改写成首次全绿。Clippy 通过也不代表没有既有 warnings。

尚未运行的真实 GUI/browser/native、icon TTL/外网下载、musl 和独立外部 Rust consumer 验收分别保留限制。全套 CI 结果与本地定向结果分开；P09 分支交付不关闭 P10/P11 或 D1–D3。

## 3. P09 已交付的归属纠正

### R0 — 不继续交付 core `IpcError`；独立处理宏 feature

**为什么：** `IpcError` 的 `kind/message/detail` 属于前端协议，CLI 直接调用 Rust API 不需要它。`ClientError`、`RuntimeError`、`ProfilesError` 等领域错误则继续留 core。

**已完成：** 原未提交 core wire-error 候选已按明确授权撤回，`IpcError` 和转换仍在宿主 `ipc.rs` / `unified_rpc.rs`。HTTP 移出后不重建 core transport 兼容层。

宏真实使用 `ItemFn` / `FnArg` / `Pat` 与 `File` / `parse_file`；`nyanpasu-macro/Cargo.toml` 已直接声明 `syn full`，独立 package 编译、宏回归及宿主 error 回归通过。没有借此统一 Display/Debug、source 或领域错误分类。

### R1 — HTTP hosting 回到现有宿主

**为什么：** 这是 GUI/browser/debug 入口，不是 GUI 与 CLI 共用的业务用例。普通 Rust/Axum 实现也不因此成为 core 职责。

**已交付：** `nyanpasu-core/src/transport/http/mod.rs` 回到 `tauri/src/server/debug_http.rs`；宿主 callers、模块和直接依赖一起更新。924 行实现逐字搬迁，原五个测试随真实 SUT 保留，没有换成模拟 Router。

**已核对的契约：**

- host 默认及 dev-proxy 相关 feature 下的实际消费者能编译；core 不再负责该 listener。
- embedded assets/SPA/404、start/stop 与 foreign Origin、打开的事件流关闭、缺失前端保持禁用、dev proxy query/status/WebSocket 协议等原测试保持。
- 原 host RPC 权限、owner、错误映射和 HTTP opt-in 回归；默认关闭、loopback、Host/Origin、token/cookie、weak routes、generation fencing 和 drain 不变。
- 真实 bindings exporter 已执行，命令清单/顺序与三份 bindings 保持；原 browser 用例的 ignored 状态不变。

**不做：** 中立 Router 工厂、新的 HTTP registry、CLI HTTP 模式或新的 HTTP event-bus。已发布的旧 P09-C1 不自动改写历史。

### R2 — Frontend telemetry 回到宿主并保留实际 sink 注入

**为什么：** 前端错误批量上报、字段净化及前端 owner 标记是宿主协议行为；不是 core 的 app/core 日志能力。

**已交付：** `nyanpasu-core/src/logs/frontend.rs` 回到 `tauri/src/desktop/frontend_events.rs`；core 的 frontend sink/forwarder 和 `LoggingSetup.frontend` 已移除，宿主用最小 `FrontendTelemetry(Arc<dyn FrontendLogSink>)` 接线。

同一个实例用于 setup/manage 与 `RpcDependencies`，原宏只增加所需 State mapping/allowlist 及回归。四个真实 RPC fixtures 保留，browser fixture 仍注入 `RecordingSink`，不是固定 tracing 替身。frontend-error-reporting 文档的测试 owner 路径已同步更新。

**已验证与覆盖边界：** 七个原净化/批处理/structured tracing 测试、宏 State 回归、消费者编译及真实 exporter 通过；原断言、helpers 和条件保留。core 初始化不再要求前端 sink，host owner 来源、错误返回和 batch wire 保持。browser fixture 仍 ignored，不能把接线/编译等同于真实浏览器上报已运行。

### R3 — Icon cache 回到 presentation

**为什么：** 该缓存服务于前端 icon 显示，不是代理控制或领域缓存。

**已交付：** `nyanpasu-core/src/icons.rs` 回到 `tauri/src/desktop/icon_cache.rs`，IPC callee 和 bytes/postcard 直接依赖随 owner 迁移。core 的共享 network 能力及仍有消费者的 sha2/hex 保留。

**已验证：** 83 行实现仅适配 network import 路径；URL/hash、七日 TTL、MIME/data URL、postcard 与下载逻辑保持。host/IPC 消费者编译及真实 exporter 通过，公开 wire 不变。

**覆盖限制：** 原模块没有单元测试，不能宣称“原 icon 单测全部通过”；本次没有运行 TTL 或外网下载场景，编译/导出不替代这些行为覆盖。

## 4. 已完成的接口闭包与后续阶段

### R4 — GUI session state：不能只搬 actor 或删除一个字段

**已解决的耦合：** 窗口 session 的 serial owner 和 geometry facade 已迁到 `tauri/src/desktop/session_state/`；`nyanpasu-config::state::PersistentState` 的 schema，以及共享持久化、migration、backup 仍保留原 owner。core 构造不再打开窗口 session 文件。

宿主 `desktop/configuration_status.rs` 在原 query/event 路径投影真实 session 版本，同一次捕获用于 `source_versions.session` 和 `event_seq`，没有填零、删 wire 字段或新增 session watch。原 mutation/effect/source 三触发源保留。

**构造与关闭：** session 现在于 migrations/storage 后、core 构造前初始化，而非原 core 内部位置；该顺序调整已审。它与窗口消费者使用同一 owner、root token tree 和 tracker。`utils/exit.rs` 仍先排入最后 geometry，再取消并等待既有 tracked drain。

**已验证：** 六个 session 测试和四个 host lifecycle/status/startup 测试通过，覆盖真实版本、query/event payload、持久化失败、最后保存与磁盘重载；core 构造/工作流及生成 wire 的相关回归通过。旧 status 测试的两个 tuple 改为 core 的三个领域后，原 import/fetch-failure/receipt/prune 场景仍保留。GUI session 由 host 测试分别承接，不声称重现了同一个 fetch-failure 场景，也不是 live webview 或 native join 验收。

### R5 — Proxy：保留领域 owner，只迁前端裁剪

**已交付：** `nyanpasu-core/src/client/clash_api.rs` 返回同一领域 owner 的未裁剪数据；`trim_for_frontend` 与原测试移到宿主 `desktop/proxy_projection.rs`。IPC 仍按 query → default latency URL → trim/error 的顺序处理，GUI 不会因此收到完整 raw payload。

**查询契约不变：** `get_proxies()` 使用 `get(false)`，遵守原缓存新鲜度策略；`refresh_proxies()` 使用 `get(true)` 强制刷新；`proxies_snapshot()` 仍是 raw **cached** snapshot，不能冒充 async refresh。default URL getter 仅开放所需可见性。

原 actor/cache 文件逐字保留；endpoint lease、generation、选择/失效、订阅以及 tray/stream 的 refresh-before-subscribe、detail demand、lag/coalescing 语义不变。没有第二份 cache 或额外 clock 框架。

**已验证：** 原 19 个 owner、12 个 assembly、5 个 projection 测试及新增 1 个 owned-copy 回归通过；真实 exporter 证明 GUI wire 保持。

### R6 — P10 的实际共享准备只有窄范围，不是 process 大搬迁

**已确认入口：**

| 入口                                       | 应提取的内容                      | 必须保留                                               |
| ------------------------------------------ | --------------------------------- | ------------------------------------------------------ |
| `tauri/src/utils/init/mod.rs::init_config` | 与 GUI 无关的初始配置文件准备     | profiles stamp、默认内容、已存在文件不覆盖、错误上下文 |
| `tauri/src/setup.rs::open_traffic_store`   | 实际受保护的 store 打开与降级规则 | 现有数据库保护/失败策略，而不是改成盲目直接 open       |

路径和能力输入显式传入；两类宿主使用同一个实现。正式模块位置按现有消费能力确定，不先创建一个包揽所有入口的 bootstrap/process 框架。

**不自动包含：** bundle 发现、当前 GUI executable 的 migration helper 选择、GUI 单实例、产品 home-dir 迁移与提权、全局 subscriber、OS session/exit hooks。资源复制只有真实共享 consumer 需要时才单独评估，输入为显式 source/target。

**完成条件：** 原 GUI 行为不变；外部 Rust consumer 能复用真实准备逻辑；临时路径下验证已有文件、错误和数据库保护，不以真实用户目录做迁移试验。

### R7 — P11 必须增加真实外部 Rust 使用证据

`ClientSetupArgs`、async `try_new_with_args` 与 typed facade 已存在；不再创建另一套“无 GUI client”。需要验证的是公开可用性和真实 graph，不只是 grep 没有 `AppHandle`。

**还要补：** 独立 Rust consumer/公开 API 测试，从路径与支持能力输入到构造、领域 mutation、观察、正常关闭，不能访问 core 私有测试 helper，也不能借 Tauri dev-dependencies/feature unification 才能通过。

同时保留原 GUI/browser transport 验收，使用真实 Specta exporter。平台测试、外部系统 fake 的覆盖边界、ignored browser case 与未运行项分别报告。CI 只接所需门禁，不建设第二套 harness。

**完成条件：** core-only consumer 不需要 GUI 构建前提或入站 HTTP；实际复用领域实现；原 GUI/browser wire 与行为仍通过相应验证。若测试暴露 R4/R5/R6 的漏实现，回到对应修正，不在 P11 隐藏补业务。

## 5. 待决策/独立 OPEN：不先假定解决方案

### D1 — 代理内核更新的目的地政策

`PathResolver.install_dir` 已存在，binary lookup 已有 data → install → development 顺序，不需要再加同义字段。`nyanpasu-core/src/client/mod.rs` 的默认更新 installer 仍有从 `std::env::current_exe()` 推导目录的路径。

**要决定：** CLI 应更新当前选中的 binary、安装目录 binary，还是明确指定目标；当 data override 与 install 同时存在时如何保持查找/更新一致。确定后再复用现有 resolver/installer port 更新真实 callers，不能先把 `current_exe()` 全部替换成另一个猜测目录。

这不等于把 Tauri 应用 updater 迁入 core，也不阻塞先验收 R1–R3。

### D2 — 未来 CLI 是否需要无人值守的 service control

现有 `ServiceHostAdapter` 路径包含 Windows `gui(true)` / `show(true)` 和 macOS osascript 交互。当前“typed Rust 可调用”不证明“无人值守可运行”。

若未来 CLI 要求无交互运行，需先决定提权/失败策略与注入位置，再补窄 adapter 行为及平台测试；没有该需求时不为了抽象纯度重写整个 service/control 层。core/daemon 协议不是本轮取消的前端 RPC。

### D3 — 生命周期与 native 所有权问题仍独立 OPEN

这些不因 HTTP 移回宿主而闭合，也不是把文件搬完就能证明：

- partial-start 失败路径与 constructor future 被 drop 后的 owner 清理；
- shutdown/panic、caller budgets、已开始工作与结果等待之间的契约；
- native thread/process 的实际 join，而非仅 timeout、取消 token、abort 或 Drop；
- `MainThreadHandoff` / `ExitGate` 的接受、完成和主线程边界；
- logger reload/parked thread、Windows callback/thread ownership；
- shutdown driver 不得等待包含自身的 tracker。

相关切口包括 core `client/app_lifecycle.rs`、`tasks/mod.rs`，宿主 `desktop/event_sink.rs`、`desktop/main_thread.rs`、`utils/exit.rs` 与 logger/Windows hooks。先针对真实问题定义可复现契约和所有权，再决定最小修复；不引通用 admission/terminal/watch 框架掩盖问题。

**不作虚假完成声明：** R7 的正常构造/关闭验证不能关闭上述异常与 native 情形。它们的实施优先级和是否阻塞具体产品发布应单独评审。

## 6. 不再列为剩余工作的项目

- 不重做 P01–P08 domain/actor/DI/facade；不把 core 中所有普通 Rust helpers 都迁回宿主。
- 不重启中立 RPC registry、catalog/schema DSL、独立 HTTP host 或 HTTP event-bus 提取。
- 不继续把前端 `IpcError` 当 CLI 前置；也不把领域错误一并迁回宿主。
- 不为同产品 CLI 新建 strict Storage opener；现有宿主 `web:` namespace 不重复建设。
- 不为未提出的 quiet/machine-readable 模式重写 migration Runner 的 stdout/stderr。
- 不重复增加 `install_dir`；不声称现有 PAC 流程是本地 PAC server。
- 不把 Tauri application updater、release/mirror 规则当共享内核 updater。
- 不以 feature flags 包装错误归属，不为 P10/P11 新建通用启动/关闭框架。

## 7. 后续顺序与发布门槛

1. **P09 收尾：** #5717 已接在 #5707 后加入原 stack；状态文档、PR 审查、CI 和合并仍分别按各自门槛收尾，不重复实现 R0–R5，也不改写已发布 C1 历史。
2. **进入 P10 / R6：** 先核对两个真实准备入口及调用者，再按能力实施；保持已有文件、profiles stamp、错误及数据库保护，测试仅使用临时路径。
3. **进入 P11 / R7：** 验证独立公开 Rust consumer 与原 GUI/browser，按实际缺口补 CI；发现遗漏归回对应能力，不在验收阶段隐藏业务补实现。
4. **持续保留审批边界：** 每个原子改动分别展示 diff 和实际验证，人工批准后才提交；共享文件整合有独立复核，不覆盖其他 dirty worktree。
5. **D1–D3 另行决策：** 不将 kernel 目的地、无人值守或异常/native 生命周期工作混入 P10/P11，也不因正常构造/关闭测试通过而关闭它们。

验证/依赖安装/产物清理另列可执行清单并获批。真实 exporter 需要其实际前端工具和构建前提，不能手写 bindings 或用假的 exporter 绕过；透明缓存由环境处理，不调查或修改其配置，不共享 Cargo target。

**CI 状态分开记录：** P06 原 Windows `test_http_module_loader` 的失败是历史记录；本次收尾查询 [#5690](https://github.com/libnyanpasu/clash-nyanpasu/pull/5690) 已有九项成功，包括[后续 Windows Unit Test](https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/37889911661/job/113730455970)。这只说明后续检查成功，不推断最初失败原因或声称修复了存量 bug。P08 #5707 当前九项检查也成功；它们不能替代 P09 的结果。

P09 本次 [CI run 38105764635 的 Linux job](https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/38105764635/job/114370852139) 在 `Prepare sidecar and resources` 阶段因 `meow-alpha`（`alpha-3c27aca`）下载返回 404 而失败；macOS/Windows 被取消，Build/Unit Test 跳过。失败尚未进入实际 lint/编译/测试检查，不能据此宣称这些检查已通过；本次未改下载策略或推断 404 的上游原因。

P09 最新 CI 和评审状态以 [#5717 的 checks](https://github.com/libnyanpasu/clash-nyanpasu/pull/5717/checks) 为准。本地定向结果不等于全套平台结果，任何当前红灯仍禁止合并；PR 接栈也不授权合并整个 stack。
