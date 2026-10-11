# Tauri/core 分离：阶段状态与剩余修正

> 整体目标与阶段定义见[迁移计划](tauri-core-extraction-plan.md)。本文区分已交付的 P09/P10、待建立的 P11，以及已确定的兼容策略和暂缓项。
> P09 实现已发布，状态文档随后以 [`8d0dcd6eb`](https://github.com/libnyanpasu/clash-nyanpasu/commit/8d0dcd6ebaf1b3cc09ba8c27a4ef6748d467e951) 更新；[PR #5717](https://github.com/libnyanpasu/clash-nyanpasu/pull/5717) 以 P08 #5707 为 base。P10 的 C1/C2 已分别人工审核并推送至 [`8755ffe61`](https://github.com/libnyanpasu/clash-nyanpasu/commit/8755ffe615b07e45e60c7f6bd36b50472aef3304)；[PR #5718](https://github.com/libnyanpasu/clash-nyanpasu/pull/5718) 以 P09 为 base，接入同一 stack #5681。分支交付不等于已合并到默认分支或全部平台验收完成。
> 后续改动仍按原子范围人工审核后提交；本文不自动授权新的构建、清理、历史改写或合并。

下文源码路径以 `backend/` 为根。

## 1. 先给结论

1. **P01–P08 的共享领域能力主体保留，不整体回退，也不再搬一遍。** CLI 直接使用 `NyanpasuClient`，不需要新增独立 HTTP/RPC 服务。
2. **P09 的 R0–R5 已完成实现、组合定向验收、独立审查、人工审核和推送。** HTTP、telemetry、icon、GUI session owner 与前端投影已按实际职责闭合；不再作为未实施候选重复施工。
3. **P10 / R6 的共享准备实现与本地定向验证已完成并推送。** 两个真实入口已迁入 core，宿主直接复用；不扩大成 process/bootstrap 大搬迁。
4. **下一技术阶段是 P11 的独立 Rust consumer 验收。** 验证公开构造、调用、观察与正常关闭，并保留 GUI/browser 证据；不是实现 CLI 产品或重新提取 HTTP 服务。
5. **D1/D3 保持迁移前行为，D2 暂缓。** 兼容方向已确定，不新增目的地或生命周期策略；这不是历史风险已修复或所有平台行为已验收的声明。

## 2. 当前状态与下一步的区别

| 编号 | 项目                             | 分支交付状态                                   | 验证边界或下一步                                                                  |
| ---- | -------------------------------- | ---------------------------------------------- | --------------------------------------------------------------------------------- |
| R0   | `IpcError` 候选处置与 `syn full` | 已完成；wire error 留 host，宏直接声明 feature | 独立 macro package 编译和回归通过，不重启中立 RPC 设计                            |
| R1   | HTTP hosting 回宿主              | 已完成并整合                                   | 原 HTTP/RPC 回归、host 消费者、真实 exporter 通过；浏览器用例仍 ignored           |
| R2   | Frontend telemetry 回宿主        | 已完成并整合                                   | 七个原测试和 State 回归通过；真实 browser fixture 保留但未运行                    |
| R3   | Icon cache 回 presentation       | 已完成并整合                                   | 源等价、消费者编译与 exporter 通过；没有 TTL/外网下载运行覆盖                     |
| R4   | GUI session state 迁出 core      | 已完成版本、query/event 与保存/drain 闭包      | 真实 owner、持久化、status 和退出保存定向测试通过；不等于 native 生命周期全部闭合 |
| R5   | Proxy raw API 与前端投影分离     | 已完成；领域缓存 owner 不变                    | 19 个 owner、12 个 assembly、6 个 host projection 测试通过                        |
| R6   | 共享启动准备                     | P10 已按 C1/C2 分别审核、提交并推送            | 临时路径下迁移前后验证通过；独立 consumer 复用证据归 P11，Windows ACL 等未运行    |
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

尚未运行的真实 GUI/browser/native、icon TTL/外网下载、musl 和独立外部 Rust consumer 验收分别保留限制。全套 CI 结果与本地定向结果分开；P09 的结果不代替 P10/P11 的证据，也不证明 D1/D3 的全部原行为或 native 情形，D2 仍暂缓。

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

### R6 — P10 已交付两个实际共享准备入口

| 原子提交                                                                                                     | 当前 core 入口                                             | 宿主调用与保留行为                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| [`fc3db0ef9`](https://github.com/libnyanpasu/clash-nyanpasu/commit/fc3db0ef91cd66bf8e4bd237a232664d487ea05d) | `nyanpasu-core/src/profiles/mod.rs::init_config`           | `tauri/src/lib.rs` 原位置直接调用，保留 log-and-continue、profiles stamp/默认字节、任何已有路径跳过、不创建父目录及写入错误上下文           |
| [`8755ffe61`](https://github.com/libnyanpasu/clash-nyanpasu/commit/8755ffe615b07e45e60c7f6bd36b50472aef3304) | `nyanpasu-core/src/traffic/storage.rs::open_traffic_store` | `tauri/src/setup.rs` 原位置直接调用，保留目录创建→Unix0700/原 Windows ACL→原 redb opener；失败 ERROR/None、选择性重建及锁定数据库不删除历史 |

两个原子范围各四个文件，分别为 +101/−44、+208/−27，均先经人工审核再提交、普通推送。core API 使用显式 `PathResolver`，宿主不留转发包装；没有新增 actor/cache/bootstrap 框架或依赖。C1 有效函数体与迁移前一致，旧 helper 中不执行的 home-dir 迁移注释随之移除；C2 的完整函数仅增加公开可见性。原测试/helpers/assertions/cfg/ignored、manifest/lock/bindings 未改。

**已验证：**

| 范围               | 原宿主 helper 基线 | 迁移后或保留的 Rust 定向测试                                         |
| ------------------ | ------------------ | -------------------------------------------------------------------- |
| C1 profiles helper | 4 通过             | 同 4 通过，另原 format 4、schema 2 通过，共 10                       |
| C2 traffic helper  | 5 通过             | 同 5 通过，另原 redb 7、facade 3、actor 1、日志 filter 1 通过，共 17 |

共 **27 个迁移后/保留的不同 package/test**，原位置的 **9 次基线执行单列**，不增加语义覆盖计数。C1 在独立前缀完成验证后才叠加 C2；其四个改动文件在最终版本字节不变，不声称 27 项全部又在同一个最终 tree 重跑。Deno 边界用例每个前缀各 3 项通过，仍是 **3 个独立用例**。

C1 与最终组合状态均通过 host `cargo check`、core/host all-target/all-feature Clippy、改动文件 Rustfmt、architecture/backend-boundary gates。代表性已执行入口：

```sh
cargo test --manifest-path backend/Cargo.toml -p nyanpasu-core --lib profiles::init_config_tests --locked --offline --jobs 2
cargo test --manifest-path backend/Cargo.toml -p nyanpasu-core --lib traffic::storage::tests --locked --offline --jobs 2
cargo test --manifest-path backend/Cargo.toml -p nyanpasu-core --lib client::traffic::tests --locked --offline --jobs 2
cargo test --manifest-path backend/Cargo.toml -p nyanpasu-traffic --lib redb::tests::a_database_that_is_already_open_is_not_deleted --locked --offline --jobs 2 -- --exact
```

发布时两个完整串行前缀分别通过正常 hooks 的 workspace `cargo clippy --all-targets --all-features`、`cargo fmt --all` 和 commitlint，提交后 tree 精确等于审核 tree。Hooks 是额外编译/lint 证据，不是重跑上述行为测试；既有 warnings 未清扫。

**失败记录与限制：** 首次新增基线测试因 `Profiles` 无 `PartialEq` 而 E0369，限定改为逐字段断言并保留精确字节检查后通过；错误 filter 的零测试结果、`--exact` 参数位置错误和证据格式比较错误均留存并有界修正，不计作首次全绿或有效覆盖。没有通过改动原生产行为让测试通过。

以上为 Linux 定向结果；Windows/macOS 构建、Windows ACL/权限拒绝、真实 GUI/native 与全套 CI 未在此运行。日志 target/source 元数据随模块从宿主迁到 core；级别、字段、消息及默认前缀过滤策略保留，但定制旧 target 的过滤器可以观察到该变化。

**剩余验收：** 独立外部 Rust consumer 的真实复用证据由 P11 建立；P10 的公开函数和同 crate 测试不能代替它。所有临时路径测试均未触碰真实用户配置。

**不自动包含：** bundle 发现、当前 GUI executable 的 migration helper 选择、GUI 单实例、产品 home-dir 迁移与提权、全局 subscriber、OS session/exit hooks。资源复制只有真实共享 consumer 需要时才单独评估，输入为显式 source/target。

### R7 — P11 必须增加真实外部 Rust 使用证据

`ClientSetupArgs`、async `try_new_with_args` 与 typed facade 已存在；不再创建另一套“无 GUI client”。需要验证的是公开可用性和真实 graph，不只是 grep 没有 `AppHandle`。

**还要补：** 独立 Rust consumer/公开 API 测试，从路径与支持能力输入到构造、领域 mutation、观察、正常关闭，不能访问 core 私有测试 helper，也不能借 Tauri dev-dependencies/feature unification 才能通过。

同时保留原 GUI/browser transport 验收，使用真实 Specta exporter。平台测试、外部系统 fake 的覆盖边界、ignored browser case 与未运行项分别报告。CI 只接所需门禁，不建设第二套 harness。

**完成条件：** core-only consumer 不需要 GUI 构建前提或入站 HTTP；实际复用领域实现；原 GUI/browser wire 与行为仍通过相应验证。若测试暴露 R4/R5/R6 的漏实现，回到对应修正，不在 P11 隐藏补业务。

## 5. 已确定的兼容策略与暂缓项

### D1 — 保持迁移前的内核更新目的地行为

**已确定：** 本轮沿用各能力迁移前的目标选择、路径推导及覆盖优先级，不为未来 CLI 新定更新目的地政策。兼容基线是迁移前真实实现，不是把当前实现未经比较就当作等价证明。

`PathResolver.install_dir` 已存在，binary lookup 已有 data → install → development 顺序；默认 installer 中从 `std::env::current_exe()` 推导目录的既有行为不因本次收尾改写。不要新增同义字段，或把它替换成另一个猜测目录。若后续发现迁移造成具体目标变化，应给出迁移前后证据并做最小原子修正，恢复原行为，而非顺带改变产品政策。

这不是把 Tauri 应用 updater 迁入 core，也不证明新 CLI 产品的全部安装/更新场景已验收。

### D2 — 无人值守 service control 暂缓

**当前处置：暂不处理。** 现有 Windows `gui(true)` / `show(true)` 和 macOS osascript 等宿主交互保持，不新增无人值守适配或将其作为本轮 P11 的必做项。“typed Rust 可调用”不等于“无人值守可运行”。

未来确有无交互产品需求时，再单独决定提权/失败策略及窄 adapter；不为抽象纯度重写整个 service/control 层。core/daemon 协议不是本轮取消的前端 RPC。

### D3 — 保持迁移前的生命周期契约

**已确定：** 本轮保持各能力迁移前的启动、工作完成、取消、退出及 native 所有权契约，不新增生命周期策略或通用强化框架，不主动扩大成存量问题治理。此决定不授权回退已审核的归属纠正或改写已发布历史。

若发现迁移引入具体偏差，应依据迁移前真实实现与原测试，单列最小原子修正以恢复原行为。下面是仍需如实保留的历史风险/未验证情形，不是新的本轮实施清单，也不能因为 HTTP 回宿主或文件搬迁就宣称已经修复：

- partial-start 失败路径与 constructor future 被 drop 后的 owner 清理；
- shutdown/panic、caller budgets、已开始工作与结果等待之间的契约；
- native thread/process 的实际 join，而非仅 timeout、取消 token、abort 或 Drop；
- `MainThreadHandoff` / `ExitGate` 的接受、完成和主线程边界；
- logger reload/parked thread、Windows callback/thread ownership；
- shutdown driver 不得等待包含自身的 tracker。

相关切口包括 core `client/app_lifecycle.rs`、`tasks/mod.rs`，宿主 `desktop/event_sink.rs`、`desktop/main_thread.rs`、`utils/exit.rs` 与 logger/Windows hooks。本次文档收尾没有改动这些实现或新增其运行验收；不引通用 admission/terminal/watch 框架。

**不作虚假完成声明：** D3 的兼容方向已确定，不等于上述情形已修复或完整验证；R7 正常构造/关闭也不构成 native join 或异常清理证明。以后若要超出迁移等价范围修复历史问题，须另行定义契约、范围和审批。

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
2. **P10 / R6 收尾：** 两个准备入口已按独立原子范围发布，#5718 已接在 #5717 后；保持实际验证、文档、PR 和 CI 结果的区别，不重复迁移或将未验平台写成通过。
3. **进入 P11 / R7：** 验证独立公开 Rust consumer 与原 GUI/browser，按实际缺口补 CI；发现遗漏或迁移回归归回对应能力，不在验收阶段隐藏业务补实现。
4. **持续保留审批边界：** 每个原子改动分别展示 diff 和实际验证，人工批准后才提交；共享文件整合有独立复核，不覆盖其他 dirty worktree。
5. **执行已定边界：** D1/D3 保持迁移前行为，D2 暂缓；不加入新产品/生命周期策略，不因正常构造/关闭通过而把历史风险标为已解决。

验证/依赖安装/产物清理另列可执行清单并获批。真实 exporter 需要其实际前端工具和构建前提，不能手写 bindings 或用假的 exporter 绕过；透明缓存由环境处理，不调查或修改其配置，不共享 Cargo target。

**CI 状态分开记录：** P06 原 Windows `test_http_module_loader` 的失败是历史记录；P09 收尾时查询 [#5690](https://github.com/libnyanpasu/clash-nyanpasu/pull/5690) 已有九项成功，包括[后续 Windows Unit Test](https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/37889911661/job/113730455970)。这只说明后续检查成功，不推断最初失败原因或声称修复了存量 bug。P08 #5707 当时九项检查也成功；它们不能替代 P09/P10 的结果。

P09 本次 [CI run 38105764635 的 Linux job](https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/38105764635/job/114370852139) 在 `Prepare sidecar and resources` 阶段因 `meow-alpha`（`alpha-3c27aca`）下载返回 404 而失败；macOS/Windows 被取消，Build/Unit Test 跳过。失败尚未进入实际 lint/编译/测试检查，不能据此宣称这些检查已通过。

后续 2026-10-11 04:13 UTC 的只读核查确认：仓库静态版本拼接的[旧 Linux 文件 URL](https://github.com/meow-rs/meow-rs/releases/download/Prerelease-Alpha/meow-alpha-3c27aca-x86_64-unknown-linux-musl.tar.gz) 返回 HEAD 404；上游 [Prerelease-Alpha](https://github.com/meow-rs/meow-rs/releases/tag/Prerelease-Alpha) 仍存在且声明每次 main push 替换资产，当时列出 `alpha-73f509c` 的六平台文件，同模板 Linux URL 返回 HEAD 302。证据支持旧文件名失效，不是观察到的架构模板错误；未下载/验证 payload，也未证明历史删除的精确时间。**已决定暂不修改版本或下载策略，不手动重跑 CI。**

P10 PR 创建后自动触发的 [run 38113010101 / macOS job](https://github.com/libnyanpasu/clash-nyanpasu/actions/runs/38113010101/job/114392288080) 对应 head `8755ffe61`，同样在 `Prepare sidecar and resources` 因 `meow-alpha alpha-3c27aca` 下载 404 而失败。Linux/Windows jobs 最终为取消，Build/Unit Test 跳过；这些都不是 lint/build/test 通过证据。这里只读核查结果，没有手动重跑或解除合并门槛。

各 head 的结果分别以 [P09 #5717 checks](https://github.com/libnyanpasu/clash-nyanpasu/pull/5717/checks) 与 [P10 #5718 checks](https://github.com/libnyanpasu/clash-nyanpasu/pull/5718/checks) 为准；历史失败、空 checks、取消或跳过都不是当前 head 通过的证据。本地定向结果不等于全套平台结果，任何当前红灯仍禁止合并；PR 接栈也不授权合并整个 stack。
