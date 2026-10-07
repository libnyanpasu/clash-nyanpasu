# Tauri → Core 全目录反选审计

## 1. 目的、基线与范围

本审计按“**默认移出 `backend/tauri/`，只有 UI／前端宿主必需的职责才留下**”重新检查整个目录，不以现有 `client/`、`core/`、`service/` 或 `utils/` 的命名决定归属，也不把“被 UI 调用”“当前只有桌面调用者”“没有 `tauri::` 类型”作为判断依据。

目标是一个可以独立构造、运行、观察和关闭的跨平台应用核心，由 Tauri、未来 CLI 和移动端共同使用；不是把桌面后端原样包装成一个需要假窗口、假托盘和假 widget 的库。

扫描基线：

- 日期：2026-10-06。
- 分支：`refactor/extract-core-02-boundaries`。
- HEAD：`8f344a358404f43fe118cc9145f1e9eeba24a5f5`。
- 扫描开始时工作树干净，尚无独立 `nyanpasu-paths` 提取；这是历史扫描状态，不代表后续 PR 或当前 checkout 的状态。
- `git ls-files backend/tauri` 共 **275 个文件**：**226 个 Rust 文件**（含 `build.rs`，`src/` 下 225 个）及 **49 个非 Rust 文件**。
- 清单覆盖全部跟踪文件，包括模块入口、已有测试、fixture、未挂载源码、配置、图标、locale 和安装模板；另外检查 gitignored 的构建／分发目录职责。

调查方式是文件清单、模块／声明／依赖扫描、调用关系检查，以及混合边界的源码阅读。文件覆盖不等于对每一行完成行为证明；迁移具体能力时仍须复查调用链和原有测试。初次审计只编写文档，未修改实现或执行 Rust 测试；下方清单保留该 **275 文件的历史基线**，不是后续 checkout 的实时文件清单。

**后续状态更正：** 旧 02-A 本地实现及其执行计划已丢弃，不再作为完成依据。路径能力改由独立、base 为 `main` 的 [PR #5645](https://github.com/libnyanpasu/clash-nyanpasu/pull/5645) 承接，落在 `backend/nyanpasu-paths`，不是 `nyanpasu-core/src/paths/`。本次按该 PR 的 head `65dd5c8be7094630d925ef93318a3e59b51ec3b9` 核对实际 diff；查询时 PR 为 **open、未合并**，不能据此声明 main 或本 core stack 已完成接入。第 4.2 节和第 9.2 节区分 PR 内实现、保留的 GUI 职责与后续工作；原 275 文件清单保留历史路径并注记，不改写为新分支实时库存。

开发规范仍以 [development guides](../development/README.md)、[架构规范](../development/architecture.md) 和 [RPC 规范](../development/rpc.md) 为准；本文件记录迁移归属与拆分点，不引入新的全局状态、兼容层或独立测试框架。

## 2. 反选规则

### 2.1 留在 Tauri 的必要条件

保留必须说明一项具体的前端职责：

1. 窗口、webview、托盘、widget、剪贴板、通知、对话框、主题／显示器／DPI 等展示与交互。
2. Tauri 的插件注册、事件循环、窗口身份、主线程调度及退出／重启接入。
3. RPC／HTTP／SSE／Channel 的传输、鉴权、调用者身份和前端资源交付。
4. 当前 GUI 分发包的探测、WebView2、打包配置，以及 Tauri 应用自身更新的 URL／策略／状态机／插件／安装流程。
5. 将 core 的状态和操作接入上述职责的薄适配与 GUI composition root。

保留的是职责，不是整个混合文件。HTTP 传输虽然不是原生 UI，也属于前端边界；可以以后独立交付，但不能成为应用 core 的依赖。

### 2.2 移出的两种归属

- **C — 应用 core**：`NyanpasuClient`、领域操作、状态所有者、typed clients、配置事务、运行时构建／应用／恢复、收敛、健康状态、进度和前端无关的观察契约。
- **I — 共享基础设施**：core 能力需要的磁盘、数据库、网络、下载、脚本执行、进程、daemon IPC、系统代理／PAC／DNS／自启动、权限与 OS 能力实现。必须移出 Tauri；通过消费方拥有的窄端口接入，复用现有独立 crate。

**I 不是“可留在 GUI 的 adapter”。** 默认落在对应 core 能力的 `adapters/` 等具体边界模块，或独立基础设施 crate；不为本审计预先创建一组新 crate。路径能力的独立落点已由 PR #5645 确定为 `nyanpasu-paths`，后续 core 直接消费它，不再重复规划 core 内的 paths 实现；其他 platform 能力仍按实际消费者划界。

OS portability 与 frontend independence 是两道不同的验收：不支持的平台要有显式能力／错误，桌面 OS 实现不应泄漏到移动端编译或 CLI 构造要求中。OS 权限提示即使由系统显示，也不意味着该能力只能属于 Tauri。

### 2.3 清单标记

| 标记 | 含义                                                                 |
| ---- | -------------------------------------------------------------------- |
| C    | 整体移入对应应用 core 能力；普通模块声明随迁移调整，不保留旧路径转发 |
| I    | 整体移出 Tauri，作为共享基础设施／能力 adapter                       |
| G    | GUI／前端宿主白名单；可以留在 Tauri，但不能要求 core 依赖它          |
| S    | 混合文件；只留下明确的 G 部分，C／I 部分必须抽走                     |
| T→…  | 已有测试／fixture 随相应职责迁移或拆分，不新增迁移专用测试           |

表中的能力路径是归属建议，不是承诺新增所有目录或 crate。`nyanpasu-config` 继续拥有 schema／patch；`nyanpasu-core-manager`、`nyanpasu-traffic`、`nyanpasu-geodata`、`nyanpasu-jobs`、`nyanpasu-logging` 等现有独立库无需为了“全在 core”而物理合并。

### 2.4 能力落点与路径约束

下面是逐文件清单中 C／I 的建议落点；除明确列出的独立 crate 外，路径相对于 `backend/nyanpasu-core/src/`。实施时按最小完整调用链落地，而不是一次性创建空目录：

| 当前能力                                                            | 建议共享落点                                                                              |
| ------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `client/mod.rs` 的 facade、共享 startup/shutdown/assembly           | `client/` 与 `bootstrap/`；facade 不收纳 GUI adapters 或任意 service lookup               |
| application/session/Clash state owners 与 mutations                 | `state/` 下各 domain，复用现有 transaction machinery；schema仍在nyanpasu-config           |
| application workflow／TCC／recovery／status                         | `application_workflow/`；runtime preparation errors由消费层拥有                           |
| runtime snapshot/inspection/ports/lifecycle/preparation/script      | `runtime/` 下按职责组织；复用已经迁入的builder，不恢复enhance旧路径                       |
| profiles owner/jobs/sources/file materialization/fetch              | `profiles/`，具体FS/HTTP放能力内adapters；state transaction基础仍复用state                |
| core endpoint router／API leases／local host                        | `core_control/`，底层process control继续复用nyanpasu-core-manager                         |
| daemon lifecycle／compat／OS service commands                       | `service/`，复用已经迁入的compat与fixtures                                                |
| connection policy/rates/streams 与 proxy cache/view                 | `connections/`、`proxies/`；Tauri/SSE/Channel delivery不进入这些模块                      |
| application/platform effects 与 logger convergence                  | `effects/`、`system_proxy/`、`system_dns/`、`logs/`；presentation-only plans不迁入        |
| logs／traffic／country index                                        | `logs/`、`traffic/`、`geo/`；复用logging/traffic/geodata独立库                            |
| KV persistence／backup／migration                                   | `storage/`、`backup/`、`migration/`；legacy schema仅供migration消费                       |
| Clash/Mihomo等代理内核更新与download                                | `updater/`、`download/`；Tauri应用自身更新整体留GUI，不创建core/app_update能力            |
| paths／binary location／service路径输入                             | `backend/nyanpasu-paths`，由独立 PR #5645 承接；core 消费实例／固定根目录快照，不复制实现 |
| resources初始化／OS permissions／shutdown检测                       | 消费能力内的初始化／platform adapters；PR #5645 不迁移资源初始化、权限执行或关机检测      |
| icon download/cache、network/environment diagnostics、proxy env文本 | `icons/`、`diagnostics/`、`proxy_env/`，不迁进通用utils桶                                 |

已有tests和fixtures跟随上表的能力目录；纯GUI tests留其frontend能力目录。G部分可以继续位于Tauri现路径，或在必要的拆分中落到明确的GUI/transport模块；不为目录美化额外改动无关调用链。

## 3. Tauri 最终保留白名单

最终可留下这些能力，**其余默认移出**：

| 保留能力                       | 当前主要位置                                                                                       | 保留上限                                                                                                             |
| ------------------------------ | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| GUI 入口与装配                 | `main.rs`、`lib.rs`、`setup.rs`                                                                    | Tauri/plugin 注册、读取 GUI bundle 输入、构造 GUI adapters、调用共享 bootstrap；不是共享 actor graph 的唯一实现      |
| 窗口与桌面呈现                 | `window.rs`、`utils/resolve.rs` 的窗口部分、`core/tray/`、`widget.rs`、`event_handler/`            | 窗口规则、托盘菜单／绘制／队列、widget 进程与 egui IPC；业务动作调用 core                                            |
| 原生交互与样式                 | `utils/dialog.rs`、`utils/open.rs`、`utils/color.rs`、`utils/dock.rs`、`utils/help.rs` 的展示部分  | 对话框、外部编辑器／文件管理器／浏览器、DPI、tray image、dock、accent color                                          |
| 桌面快捷键接入                 | `client/hotkey/` 的 OS 注册所有者及桌面动作部分                                                    | accelerator 注册、释放、主线程交互、dashboard toggle；配置验证／业务动作不能因此被困在 GUI                           |
| 前端观察与事件映射             | `client/event_sink.rs` 的 GUI 部分、`setup.rs` 事件转发、`core/clash/mod.rs` 的 bridge             | core observations → Tauri／前端 cache invalidation；没有 main window 时也不能阻断 core 观察                          |
| 传输与前端托管                 | `ipc.rs`、`unified_rpc.rs`、`specta_export.rs`、`server/`、`core/clash/connection_details.rs`      | 薄 RPC、错误映射、owner、Channel／SSE、鉴权、路由、assets／dev proxy；不承载业务工作流                               |
| 前端错误日志接入               | `client/frontend_events.rs`                                                                        | JS console／uncaught error／unhandled rejection 请求约束、净化与日志投递；从 core 构造要求中移除                     |
| GUI 发行包、自身更新与退出接入 | `bundle.rs`、`client/app_update/`、`utils/exit.rs`                                                 | Tauri应用更新的URL、策略、状态机、下载／校验／安装与events整体留GUI；共享身份输入、进程机制和core shutdown按职责拆开 |
| GUI 资源与构建                 | Tauri 配置、capability、icons、UI locales、installer templates、manifest、`build.rs` 的 Tauri 部分 | CLI／core 不应需要它们或 `tmp/dist`                                                                                  |

注意：没有 Tauri 类型的 `TrayQueue`、窗口 URL builder、widget 生命周期规则仍可以是 G，因为其目的就是 GUI。反过来，有 Tauri 调用的目录解析、版本查询、自启动配置并不因此成为 G，应拆掉前端耦合后移出。

## 4. 必须拆开的关键边界

### 4.1 Facade 与 bootstrap：不能只改 import

`client/mod.rs` 当前同时持有业务所有者和 `debug_http`、`BundleMetadata`、`frontend_log`、`WindowControl`、`UiEventSink`，`ClientSetupArgs` 还要求 `http_frontend`、`http_routes`、GUI window、accelerator 等输入。

- `NyanpasuClient`、typed state clients、工作流、资源／任务跟踪及业务查询迁入 core。
- 把应用版本、channel、二进制／资源根目录等建模为明确输入，而不是要求 `BundleMetadata` 或读取 Tauri 构建环境。
- HTTP server、routes、assets、前端错误 ingestion、dashboard/window dispatch、Tauri 应用自身 updater 放在 frontend composition；不能仅把它们换成“不含 Tauri 类型”的必填 trait。
- 共享 bootstrap 能构造业务 actor graph。GUI `setup` 只提供实际 GUI／平台 adapters、订阅观察并注册 transports。
- `try_new_with_args` 中的 `tauri::async_runtime::block_on` 与共享任务 spawn 改为显式 async／Tokio；GUI 可在边界适配同步入口。
- shutdown 保持一个 root token、owner cleanup 和真实等待；不新增全局 shutdown 阶段／预算，也不借迁移重写 actor 生命周期。

### 4.2 Paths 由独立 PR 承接；resources、版本、环境输入仍按职责拆分

历史耦合是 `utils/path.rs` 委托 GUI `utils/dirs.rs` 发现目录、后者混合 portable／registry／exe／发行输入，以及 `core/clash/mod.rs` 混合 binary 优先级与 Tauri target-triple／源码树 sidecar fallback。PR #5645 针对这条调用链重新设计实例，而非机械搬到 core。

**PR #5645 内已实现、尚未合入 main 的范围：**

- `nyanpasu_paths::PathResolver` 持有 `HostInputs`、可选显式 config/data roots 和 installation 输入，平台 policy 在 crate 内；构造不发现或创建目录。`ResolvedPaths` 是共享 application graph 消费的固定根目录快照，不是第二个发现服务。
- config/data 默认值、目录 namespace casing、Windows portable／registry／SID、单实例标识、目录创建及动态 binary 查找由 crate 承接。保留 config 的 portable → registry → OS default 优先级、独立根目录错误与 best-effort 边界；managed search 为 data → install → GUI 提供的 dev candidate，diagnostic locator 的既有差异不在本 PR 修复。
- GUI 新 `host_paths.rs` 只装配应用身份、Tauri executable 探测结果、portable 标记和 dev sidecar 候选；resource dir 在 GUI setup 发现后供应。portable 标记每进程一次探测、单实例后打开窗口、tray artwork 路径等仍留 GUI。共享 crate 不反查 Tauri 源码树或要求其 assets。
- service 的用户／目录输入准备归 crate，保留用户 → data → config → install 的错误及 IO 顺序；service 协议、OsString 参数模板、提权执行与 actor 生命周期仍在 Tauri，后续随 service 能力迁移。
- 旧 `utils/path.rs`、`utils/winreg.rs`、`utils/winreg_test.rs` 及 core binary lookup/export 在 PR 内删除；`utils/dirs.rs` 只剩 GUI version metadata 和原常量。原路径／Windows tests 随能力移动，含 tray 路径断言的原 test 留 GUI；deprecated legacy filesystem API 及独占输入／依赖移除，没有新迁移测试。
- runtime、profiles/scripts、backup、migration、日志／cache、diagnostics 与 GUI 操作更新为消费新实例／快照。`FsRuntimeBuildAdapter` 显式注入 core-spec 查找行为，生产与原 fake 测试使用相同字段和方法；这不表示这些 owners／业务能力已迁入 core。

**后续迁移约束与未覆盖范围：**

- PR 合入 main 后，core stack 分别衔接该基线并复用 `nyanpasu-paths`；不再创建重复的 `nyanpasu-core::paths`、兼容 re-export 或另一套 platform discovery。当前 PR 尚未合并，本 checkout 不能被写成已接入。
- 显式路径输入不要求每个 GUI adapter 保存字段或逐层传参。GUI `host_paths` 是宿主装配入口；未来 core 的服务、actor 和 migration 不回调它。固定启动 roots 与 GUI 按需发现有不同职责；不能仅为读取路径新增 `State<NyanpasuClient>`／`State<UnifiedRpc>` 或 facade 路径供应 API。
- resource 路径值可由宿主供应，不等于 resource 初始化／复制已迁出。geo／wintun 等初始化、权限授予／执行、版本进程查询、BuildInfo／channel 输入、service 控制、日志 archive、icon cache、migration／backup 工作流仍需各能力完成迁移。
- `BuildInfo`／版本／channel 是输入和共享诊断数据；窗口 label、编辑器标题、WebView2、安装包探测留 GUI。`utils/hwid.rs` 的订阅请求设备 headers／净化／fallback 和 static 审查不在 PR #5645 范围内，后续独立处理。

### 4.3 Effects：应用／平台收敛与呈现分开

`client/effects/` 当前把系统代理、PAC、自启动、日志、hotkeys、locale、tray、widget 放进同一计划和执行路径。

- 系统／应用 desired state、diff、revision、重试、coalescing、收敛状态及事务后的 handoff 属于 core。
- `LoggerRefresher`／`TracingLoggerRefresher` 是应用日志基础设施，不因位于 `ui_effects/` 就留在 GUI。
- `TrayView`、tray repaint/menu、widget controller、UI locale application 属于呈现；即使是纯 projection 也不必进入共享业务 facade。
- 桌面注册能力的运行所有者可以留在 frontend adapter 内；core 只保留它确实消费的配置／能力契约和显式结果。
- 如果共享状态仍汇总可选 frontend effect，必须表达 absent／unsupported／available／degraded 等真实结果；不能由 CLI 注入“应用成功”的 no-op。
- `ui_effects/ports.rs` 直接使用 `nyanpasu_egui::ipc::WidgetIpcError`。它是 GUI widget 契约／错误，不应机械迁入 core；需要跨边界汇总时映射为消费方定义的数据／错误。

Phase 2 只处理输入／契约／通知 seam，耦合的 effects 调度和职责切分仍应在 Phase 9 实施。本文不授权立即重写 effects。

### 4.4 Hotkeys 与业务动作

`HotkeyAction::OpenOrCloseDashboard` 和 Clash mode／system proxy／TUN 动作目前混在 `dispatch_hotkey_action` 中。

- mode／proxy／TUN 操作及其配置事务归 core，任何前端都可调用，不能以“快捷键”作为唯一 API 身份。
- dashboard toggle、window errors、`WindowControl`、Tauri/global-shortcut 的注册与回调、桌面注册 actor 的状态归 GUI capability。
- 热键配置的存储、提交前结构验证和应用 schema 属于共享能力；平台 accelerator 验证通过明确的可选平台规则处理，CLI 无需启动一个 shortcut registrar。
- 保留现有“提交前验证”、已注册集合的所有权、串行 reconcile、错误／降级和 shutdown unregister 行为；GUI action pump 调用普通业务操作，不能挪一份业务逻辑到 GUI。
- 现有 `MainThreadExecutor` 的生产消费者是 shortcut registration 和 window control，因此此端口及 handoff 可跟它们留在 GUI；core 不必为了抽象 UI 主线程而强制持有它。

### 4.5 Notifications 与事件：观察不是 UI invalidation

当前 `UiEventSink` 的生产 trait 调用位于 `effects/actor.rs`：group 2 执行前调用 `ui.refresh_clash()`，随后才调用 `port.apply(...)`。这表示 best-effort frontend refresh，不等于配置 commit、effects 成功或新的 runtime 已应用；lifecycle／reconcile／retry 也可能触发它。

- `StateChanged`、`nyanpasu://mutation`、前端 query invalidation 的映射留在 frontend。
- 从真实 state owner 迁出前端无关观察：committed source versions、runtime／effects health、profiles sources、proxy cache、streams、logs、代理内核 updater progress；Tauri 应用更新的观察留在 GUI。
- `CommitNotifications` 是已提交 desired state 向 effects owner 的 handoff，**不是 UI event bus**；保持来源与触发意图，不以一个统一“commit event”覆盖一切。
- 优先复用 watch／broadcast／typed subscriptions，不要求新增一个万能 event bus。
- 当前 `TauriUiEventSink` 只有 main webview 存在才 emit；HTTP events 又通过 `bridge_tauri_events` 从 Tauri 事件转发。这不能作为独立 core 的观察出口。Tauri 和 HTTP 应在 frontend composition 直接消费 core 观察，再映射自己的协议。
- 注意旧时序（apply 前）、重复通知、重试与合并、关闭、lag/resync 和无主窗口场景。现有 notification-count tests 不证明 frontend 接收或 cache invalidation 正确。
- `StateChanged` 序列化为 `clash_config`／`nyanpasu_config`，而 frontend mutation provider 读取 `clashConfig`／`nyanpasuConfig`；当前 transport 没有 casing 转换。此已有缺陷单独修复，不在迁移中静默改变 wire。
- `ConfigurationStatus` 消费方只按 `event_seq` 更新 health cache，不通过 `source_versions` 自动刷新业务 queries，不能直接替代原 mutation notification。
- `core/tray/proxies.rs` 既刷新 tray 又 emit proxy mutation；应把 frontend event forwarding 从 tray 生命周期拆开，保留 `core/proxies.rs` actor-owned cache 和独立 watch。

### 4.6 RPC／HTTP 与 capability 操作

`ipc.rs`、`unified_rpc.rs` 和 `server/debug_http.rs` 留作 transport／frontend boundary，但不得包住尚未迁出的业务执行：

| 当前调用链                                                               | 必须移出                                                     | GUI／transport 保留                                             |
| ------------------------------------------------------------------------ | ------------------------------------------------------------ | --------------------------------------------------------------- |
| `get_core_version` → `resolve_core_version` → shell plugin               | binary lookup 由 #5645 承接；版本执行／typed error 仍待迁移  | RPC DTO／error mapping                                          |
| `collect_logs` → save dialog → `candy::collect_logs`；HTTP archive route | 日志选择／ZIP export／tempfile                               | 保存位置对话框、HTTP response/body                              |
| `get_cached_icon` → `service/icon`                                       | HTTP 下载、URL key、7 天 cache 策略、postcard 磁盘实现       | data URL／base64 DTO 呈现                                       |
| `set_custom_app_dir`／`migrate_home_dir_handler`                         | 目录／registry 由 #5645 承接；迁移／备份／elevation 仍待迁移 | 确认对话框、GUI exit/relaunch 接入                              |
| `invoke_uwp_tool` → `core/win_uwp`                                       | Windows loopback tool 执行与权限策略                         | IPC adapter；其他平台显式能力结果                               |
| `copy_clash_env`                                                         | Shell／Cmd／Pwsh 的 proxy env 文本生成                       | 剪贴板写入                                                      |
| `get_service_install_prompt`                                             | 路径输入准备由 #5645 承接；参数模板／service 控制仍待迁移    | CLI／shell 展示文本的 adapter                                   |
| `open_*`、`view_profile`                                                 | 路径／内容查询                                               | 打开文件管理器／编辑器／浏览器                                  |
| log sessions／connection details                                         | 共享查询／流与 session 所有权语义                            | 从 transport 身份派生 owner、webview Channel／HTTP SSE 生命周期 |

RPC owner 是调用者身份，不是 window identity。共享能力不得直接使用 window label 做唯一 domain owner；HTTP cookie、desktop webview 的映射留在 transport。保留显式 `rpc(http)` opt-in、鉴权／Host／Origin／loopback、structured errors、query/mutation 分类和 bindings 导出流程。没有新建 CLI 产品的任务。

### 4.7 按更新对象划界：代理内核 vs. Tauri 应用

- Clash/Mihomo 等代理内核更新的 manifest／artifact URL 计算、platform selection、download/retry/progress、binary installation、与运行实例协同属于 `nyanpasu-core::updater`；CLI 和 Tauri 可以共同消费。
- Tauri 应用自身更新由 GUI 负责：manifest endpoints、下载候选／mirror URL、SourceForge metadata 校验、版本比较、状态机、进度／取消、下载／校验／安装、Tauri updater 插件和事件都留在 Tauri。没有 Tauri 类型或属于纯计算，不足以将这些职责归入 core。
- `bundle.rs` 的 portable／WebView2／发行包规则与应用 updater 策略留 GUI；只拆出共享业务实际需要的应用身份／版本／已安装 channel 输入，不把 GUI 自更新包装成共享应用能力。
- `client/app_update/` 的 backend／ports／PreparedAppUpdate／VerifiedAppUpdate／events 与原 tests 都随 GUI 自更新能力保留。后续 facade 分离时，将该 owner 及入口从 `NyanpasuClient` 的共享构造／状态／API 中剥离；core 不需要 optional 或 no-op Tauri updater。
- 未来 CLI 的自身更新由 CLI 自己确定，不预设与 Tauri 共用 URL 计算或自更新框架。通用下载库可被各能力复用，但这不改变自更新的职责归属。

此边界按用户在审计后的明确纠正记录，取代先前将 Tauri app update 状态机／policy 归入 core 的规划。

### 4.8 启动／关机／迁移 helper

- 初始化共享配置、拷贝 core geo resources／wintun、迁移子进程的双流 drain、backup 与失败信息、relaunch process 执行机制移出 Tauri。
- GUI panic dialog、桌面 helper 子命令的 dispatch、Tauri exit gate 和 UI-thread cleanup 接入留在 frontend。
- `shutdown_hook.rs` 虽用隐藏 Win32 window 接收 OS session shutdown，但不是主窗口／webview 展示。OS 关机检测机制可以是共享 Windows infrastructure；触发 Tauri exit 和主线程接管的 wiring 留在 GUI。不能因为“使用 HWND”就整文件保留，也不能把其现有 global callback state 直接当成 core service state。
- `event_handler/mod.rs` 当前 mount 函数为空、`utils/winhelp.rs` 的 module 声明被注释。记录其职责／未挂载状态，不在本轮删除已有死代码。
- geo 的 “index image” 是 `IpIndex` 二进制缓存／mmap，不是 UI 图片，整条国家索引能力移出。

## 5. 全部源码文件去向

以下路径均相对于 **`backend/tauri/` 的历史扫描基线**。每一个当时跟踪的 `.rs` 文件都在清单中。S 的拆分规则见第 4 节；测试行包含原测试／test helpers 的最终归属。PR #5645 删除的旧文件仍保留在清单并注记，新增的 `host_paths.rs` 与独立 crate 见第 4.2 节；接入路径能力不等于整个调用者已迁移。

### 5.1 根入口、commands、transports 与展示

| 当前文件                      | 归属  | 去向／依据及拆分点                                                                                                                                         |
| ----------------------------- | ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `build.rs`                    | S     | G：tauri-build、GUI manifest／包资源；共享 build/version DTO 由输入提供，不复制 Tauri build prerequisites 到 core                                          |
| `src/main.rs`                 | G     | GUI executable 入口；未来 CLI 自己调用共享 bootstrap                                                                                                       |
| `src/lib.rs`                  | S     | G：Tauri builder/plugins/events/deep links/panic UI；C/I：共享初始化、diagnostics 和进程操作抽出，GUI 只装配／调用                                         |
| `src/setup.rs`                | S     | G：GUI adapters、managed state、event/transport bridge；C/I：应用 bootstrap、actor graph、store 打开／降级策略、migrations、core/service/jobs construction |
| `src/consts.rs`               | S     | G：window labels／GUI titles／AppImage与portable探测；C：共享 BuildInfo／应用版本输入，crate env 填充由各宿主负责                                          |
| `src/bundle.rs`               | S     | G：BundleMetadata、WebView2、Tauri应用更新URL／mirror校验／版本比较／updater builder；C：仅共享业务需要的应用身份／compiled channel输入                    |
| `src/bundle/tests.rs`         | T→C/G | 共享身份／compiled channel原测试随消费层；Tauri更新URL／版本比较／bundle tests留GUI                                                                        |
| `src/shutdown_hook.rs`        | I     | Windows OS session shutdown adapter；GUI callback wiring 和既有 global callback 约束另审，不要求 core 主窗口                                               |
| `src/ipc.rs`                  | S     | G：薄命令、DTO、Tauri events/native interactions；C/I：版本查询、archive、迁移、icon cache、UWP／service参数等业务操作抽出                                 |
| `src/unified_rpc.rs`          | S     | G：RPC registry/dispatch/owner/errors、HTTP/SSE鉴权与routing、Tauri bridge；I：archive制作等调用共享能力；内联transport tests 留 G                         |
| `src/specta_export.rs`        | G     | commands/events metadata、query/mutation分类与 bindings generation，引用新的core类型，不手改输出                                                           |
| `src/window.rs`               | G     | window registry、ready/message protocol、URL/size/vibrancy/traffic lights规则均为展示；业务读取调用core                                                    |
| `src/widget.rs`               | G     | statistic widget process/link/egui IPC与展示生命周期；流由core供应，不让core依赖egui                                                                       |
| `src/cmds/mod.rs`             | S     | G：GUI executable子命令解析、panic dialog/widget/bridge/relaunch入口；C/I：diagnostics／迁移操作和进程机制                                                 |
| `src/cmds/migrate.rs`         | S     | G：参数、退出码、GUI executable relaunch接入；C/I：fresh-install判断、迁移/备份/home-dir/registry/elevation工作流                                          |
| `src/event_handler/mod.rs`    | G     | Tauri event挂载入口，当前mount为空；不因此删除                                                                                                             |
| `src/event_handler/widget.rs` | G     | widget UI配置事件与egui实例                                                                                                                                |
| `src/server/mod.rs`           | G     | frontend HTTP server模块入口；从core facade剥离                                                                                                            |
| `src/server/debug_http.rs`    | G     | HTTP server actor、credentials/session/Host/Origin、assets、dev proxy、transport drain；frontend subsystem，不属于应用core                                 |
| `src/service/mod.rs`          | C/I   | 移出所有业务实现后移除旧聚合入口；不在core复制generic services桶                                                                                           |
| `src/service/icon.rs`         | C/I   | `icons`能力：下载／SHA256 URL key／postcard cache／expiry；注入cache路径和HTTP输入                                                                         |
| `src/service/profile_file.rs` | C/I   | `profiles` materialization/cleanup/recovery/normalization/subscription fetch；端口消费与FS/HTTP分层，保留内联原测试及journal／权限语义                     |

### 5.2 Facade 与 capability clients

| 当前文件                             | 归属 | 去向／依据及拆分点                                                                                                                    |
| ------------------------------------ | ---- | ------------------------------------------------------------------------------------------------------------------------------------- |
| `src/client/mod.rs`                  | S    | C：core facade/bootstrap；G依赖移除：HTTP/assets、bundle、window、frontend log、UiEventSink；内联原测试按能力归属调整，不增加迁移测试 |
| `src/client/app_lifecycle.rs`        | C    | core startup/background sources/shutdown/tracked tasks；移除Tauri spawn，GUI producer注册仍由边界负责                                 |
| `src/client/application.rs`          | C    | application config typed client，build channel显式输入                                                                                |
| `src/client/clash_config.rs`         | C    | Clash config typed client                                                                                                             |
| `src/client/session_state.rs`        | C    | session state typed client；UI字段可持久化，不要求运行UI                                                                              |
| `src/client/profiles.rs`             | C    | profiles typed client、committed reads／transactional writes                                                                          |
| `src/client/clash_api.rs`            | C    | instance-bound API facade／proxy mutations及观察                                                                                      |
| `src/client/clash_info.rs`           | C    | 从typed applied state派生endpoint信息，非GUI地址helper                                                                                |
| `src/client/clash_streams.rs`        | C    | stream snapshot／recording／history facade                                                                                            |
| `src/client/configuration_status.rs` | C    | read-only source/runtime/effects/operation health，Tauri event wrapper不属于此能力                                                    |
| `src/client/convergence.rs`          | C    | health/retry budget/outcome vocabulary                                                                                                |
| `src/client/direct_egress.rs`        | C/I  | network diagnostics契约与HTTP DIRECT probe，按平台能力接入                                                                            |
| `src/client/error.rs`                | S    | C：应用错误；window-only等前端错误移到GUI，structured protocol保持                                                                    |
| `src/client/event_sink.rs`           | S    | G：StateChanged/URI/UiEventSink/Tauri emitter/main-thread handoff；C：由真实owner提供独立观察，不搬旧UI invalidation trait            |
| `src/client/frontend_events.rs`      | G    | JS/frontend request sanitization和日志投递；去掉core facade必填sink与method归属                                                       |
| `src/client/jobs.rs`                 | C/I  | shared job owner composition／ProfileSyncStatus／business registration；依赖现有nyanpasu-jobs                                         |
| `src/client/logs.rs`                 | S    | C：日志查询facade与service-log端口；I：daemon IPC查询；G：LoggingSetup.frontend sink从共享setup剥离                                   |
| `src/client/main_thread.rs`          | G    | 当前消费者仅desktop shortcut/window，UI-thread端口及原测试随GUI；core不必填                                                           |
| `src/client/ports.rs`                | C/I  | candidate/confirmed port binding、session resolution及probe，保留receipt语义                                                          |
| `src/client/runtime.rs`              | C/I  | runtime snapshots/store/receipts/outcomes/degradation；candidate FS与路径adapter分层                                                  |
| `src/client/runtime_error.rs`        | S    | C：runtime/preparation/recovery structured errors；Tauri shell／GUI-only causes移出并在adapter映射，不扁平字符串化                    |
| `src/client/runtime_inspection.rs`   | C    | immutable promoted build inspection                                                                                                   |
| `src/client/runtime_recovery.rs`     | C    | 根据observed receipt/binding证明恢复，不按enum名或旧pid判断                                                                           |
| `src/client/system_dns.rs`           | C/I  | DNS cache能力端口／OS实现，unsupported真实表达，test fake不进入production assembly                                                    |
| `src/client/traffic.rs`              | C/I  | selection/retention/local source cache/clock与traffic pipeline composition；去除Tauri runtime                                         |

### 5.3 Application workflow 与 core lifecycle

| 当前文件                                                     | 归属  | 去向／依据及拆分点                                                                          |
| ------------------------------------------------------------ | ----- | ------------------------------------------------------------------------------------------- |
| `src/client/application_workflow/mod.rs`                     | C     | workflow admission actor/typed client及lifecycle status                                     |
| `src/client/application_workflow/adapters.rs`                | I     | FS/script preparation、core check 仍待迁移；#5645 已显式注入 core-spec 查找，不改变能力归属 |
| `src/client/application_workflow/attempt.rs`                 | C     | write-ahead live attempt／recovery view                                                     |
| `src/client/application_workflow/error.rs`                   | C     | consuming preparation layer的RuntimePreparationError；不要并回RuntimeBuilder错误            |
| `src/client/application_workflow/impact.rs`                  | C     | runtime impact/activation/fields/content classification                                     |
| `src/client/application_workflow/inputs.rs`                  | C     | frozen runtime inputs/content                                                               |
| `src/client/application_workflow/mutation.rs`                | C     | mutation identities、journal／settlement协议                                                |
| `src/client/application_workflow/participant.rs`             | C     | 每次transaction的Required participant／DecisionHandle                                       |
| `src/client/application_workflow/policy.rs`                  | C     | pure command/disposition/defer policy                                                       |
| `src/client/application_workflow/ports.rs`                   | C     | workflow-owned build/check消费契约                                                          |
| `src/client/application_workflow/preparation.rs`             | C     | build/check准备编排                                                                         |
| `src/client/application_workflow/profiles.rs`                | C     | profiles mutation工作流                                                                     |
| `src/client/application_workflow/startup.rs`                 | C     | evidence-based owner plan／reestablish，保留local fallback与stop intent                     |
| `src/client/application_workflow/tcc.rs`                     | C     | prepare/try/confirm/cancel/recovery串行域；不改settlement或caller-drop语义                  |
| `src/client/application_workflow/workflow.rs`                | C     | workflow actor-owned运行状态与能力编排                                                      |
| `src/client/application_workflow/tests/mod.rs`               | T→C/G | 业务workflow原测试随C；CountingUi相关断言按通知／effects职责拆分，不当作delivery证明        |
| `src/client/application_workflow/tests/closing.rs`           | T→C   | 原closing/shutdown测试随workflow                                                            |
| `src/client/application_workflow/tests/connection_policy.rs` | T→C   | 原connection policy测试随workflow                                                           |
| `src/client/application_workflow/tests/mutations.rs`         | T→C   | 原mutation测试随workflow                                                                    |
| `src/client/application_workflow/tests/recovery.rs`          | T→C   | 原recovery测试随workflow                                                                    |
| `src/client/application_workflow/tests/service_recovery.rs`  | T→C   | 原service recovery测试随workflow                                                            |
| `src/client/application_workflow/tests/startup.rs`           | T→C   | 原startup evidence测试随workflow                                                            |
| `src/client/application_workflow/tests/validation.rs`        | T→C   | 原validation测试随workflow                                                                  |
| `src/client/core_lifecycle/mod.rs`                           | C     | lifecycle command/output protocol                                                           |
| `src/client/core_lifecycle/adapters.rs`                      | I     | FsBinaryInstaller：共享binary安装，不是GUI updater安装                                      |
| `src/client/core_lifecycle/apply.rs`                         | C     | runtime apply options/context                                                               |
| `src/client/core_lifecycle/ports.rs`                         | C     | binary install/progress/preparation端口                                                     |
| `src/client/core_lifecycle/workflow.rs`                      | C     | lifecycle/ownership/service recovery/submission规则                                         |

### 5.4 Effects、system proxy、hotkeys、app update

| 当前文件                              | 归属    | 去向／依据及拆分点                                                                                                   |
| ------------------------------------- | ------- | -------------------------------------------------------------------------------------------------------------------- |
| `src/client/effects/mod.rs`           | S       | C：应用effects API；G：tray_view等展示projection                                                                     |
| `src/client/effects/actor.rs`         | S       | C：desired state/revision/retry/coalescing/health；G：presentation group和UI refresh触发；切分时审查顺序与重试       |
| `src/client/effects/error.rs`         | C       | effects owner请求错误，前端特定variant按消费者拆分                                                                   |
| `src/client/effects/executor.rs`      | S       | C：平台／logger／core log capture dispatch；G：shortcut registration/locale/widget/tray fan-out                      |
| `src/client/effects/plan.rs`          | S       | C：app/platform projection/diff；G：TrayView/menu/repaint/widget/UI locale；不因pure就全迁                           |
| `src/client/effects/ports.rs`         | S       | C：application effects消费契约、CommitNotifications handoff；G：presentation-only契约；production no-op不得伪装支持  |
| `src/client/effects/status.rs`        | S       | C：revision/health/failure/result语义；G：frontend-only effect具体项；可选capability聚合用明确可用性                 |
| `src/client/effects/tests.rs`         | T→C/G   | 原计划、retry/group、handoff测试按拆分后的职责保留                                                                   |
| `src/client/system_proxy/mod.rs`      | C       | typed system proxy/PAC/autostart client                                                                              |
| `src/client/system_proxy/actor.rs`    | C       | 原OS proxy快照、guard timer、PAC接管及shutdown restore的serial owner                                                 |
| `src/client/system_proxy/error.rs`    | C       | platform convergence failures/fallback classification                                                                |
| `src/client/system_proxy/ports.rs`    | C       | consumer-owned OS proxy/PAC/autostart端口                                                                            |
| `src/client/system_proxy/adapters.rs` | S       | I：sysproxy/auto-launch/PAC HTTP+cache实现；G：Tauri AppImage／current-exe解析由GUI注入，不整文件保留                |
| `src/client/system_proxy/tests.rs`    | T→C/I   | 原actor/adapters测试随platform capability                                                                            |
| `src/client/hotkey/mod.rs`            | S       | C：bindings validation／业务动作；G：desktop registration typed client和dashboard dispatch；不能让core持有必填window |
| `src/client/hotkey/actor.rs`          | G       | desktop accelerator注册集合的serial owner；可在GUI adapter内部保持actor，不是headless启动要求                        |
| `src/client/hotkey/adapters.rs`       | S       | G：TauriShortcutRegistrar/WindowControl/channel pump；C/I：提取提交前所需的值验证契约，plugin parser实现留平台边界   |
| `src/client/hotkey/error.rs`          | G       | desktop registration convergence错误；需要共享汇总时映射consumer-owned结果                                           |
| `src/client/hotkey/ports.rs`          | S       | C：业务action/配置验证契约；G：dashboard/window/registration/main-thread错误；存储schema仍在config                   |
| `src/client/hotkey/tests.rs`          | T→C/G   | 业务config action／验证与desktop register/window tests分开保留                                                       |
| `src/client/ui_effects/mod.rs`        | S       | G：locale/widget/tray；C/I：logger能力移出，不能原样创建core/ui_effects桶                                            |
| `src/client/ui_effects/ports.rs`      | S       | C：logger端口／rotation/error；G：tray/widget/locale与egui原因；core不得依赖WidgetIpcError                           |
| `src/client/ui_effects/adapters.rs`   | S       | I：TracingLoggerRefresher；G：RustI18nLocaleSink/TauriTrayRefresher/TauriWidgetController                            |
| `src/client/ui_effects/tests.rs`      | T→C/I/G | 原logger与presentation effects测试随职责拆分                                                                         |
| `src/client/app_update/mod.rs`        | G       | Tauri应用自身更新owner/state machine/progress/cancel/verify/install与契约整体留GUI，从共享facade剥离                 |
| `src/client/app_update/adapters.rs`   | G       | Tauri应用更新backend/download/verify/installer/event sink/package context；可复用既有下载库，不迁入core              |
| `src/client/app_update/tests.rs`      | T→G     | 全部原应用更新state machine/backend/event tests留GUI；fake adapter测试不改变能力归属                                 |

### 5.5 Control plane、Clash streams、proxies、service、updater

| 当前文件                                    | 归属  | 去向／依据及拆分点                                                                                                    |
| ------------------------------------------- | ----- | --------------------------------------------------------------------------------------------------------------------- |
| `src/core/mod.rs`                           | S     | 移出业务模块后仅保留GUI模块声明（tray等），不保留原路径re-export                                                      |
| `src/core/actor_v2/mod.rs`                  | S     | C：endpoint router/status projection/typed client；G：CoreStatusChangedEvent/ServiceStatusChangedEvent的Tauri wrapper |
| `src/core/actor_v2/api.rs`                  | C/I   | instance-bound Clash API/revocable lease／transport，协议client不外泄                                                 |
| `src/core/actor_v2/endpoint.rs`             | C/I   | execution host/submission/status/ControlEndpoint；Local和Service concrete adapters分层                                |
| `src/core/actor_v2/facade.rs`               | C     | control protocol workflow receipts/recovery/handoff/helpers                                                           |
| `src/core/actor_v2/intent.rs`               | C     | pure RuntimeIntentBuilder及digest                                                                                     |
| `src/core/actor_v2/local_host.rs`           | C/I   | binary lookup 由 #5645 接入 nyanpasu-paths；core spec／CoreControl assembly 仍待迁移，复用 core-manager               |
| `src/core/actor_v2/service_actor.rs`        | C     | daemon资源serial owner、compat gate/restart/exhaustion/typed ServiceClient                                            |
| `src/core/actor_v2/service_host_adapter.rs` | I     | #5645 注入路径 resolver；OS service commands／IPC adapter 整体仍待迁移，CLI 可复用                                    |
| `src/core/actor_v2/tests.rs`                | T→C/I | 原router/control lifecycle tests随所属能力                                                                            |
| `src/core/clash/mod.rs`                     | S     | #5645 删除 binary lookup，dev candidates 移至 GUI host_paths；剩余 stream/log bridge／Channel 接入仍为 G              |
| `src/core/clash/api.rs`                     | C     | protocol/read DTO及log/check-output解析，不因UI显示而保留                                                             |
| `src/core/clash/proxies.rs`                 | C     | shared proxy view/capabilities，不是tray menu model                                                                   |
| `src/core/clash/ws.rs`                      | S     | C/I：StreamsActor、subscriptions/history/frame/recording/connectors；G：tauri_specta Event包装；保留原算法／tests     |
| `src/core/clash/connection_details.rs`      | G     | per-webview subscriptions/Channel/forwarding；需求生命周期归transport，frame producer在core                           |
| `src/core/connections.rs`                   | C     | connection scope/interruption规则                                                                                     |
| `src/core/proxies.rs`                       | C     | actor-owned cache/fingerprint/change watch；UI invalidation由订阅者映射                                               |
| `src/core/manager.rs`                       | I     | #5645 接入共享 binary path；权限授予、命令／escaping 机制仍待迁移，不留 GUI 权限 wrapper                              |
| `src/core/service/mod.rs`                   | C/I   | service能力入口迁移后删除旧声明                                                                                       |
| `src/core/service/control.rs`               | C/I   | #5645 仅拆出 user／路径准备；command/errors、参数模板及 elevated install/start/stop/status 仍待迁移                   |
| `src/core/win_uwp.rs`                       | I     | Windows loopback executable/elevation/deelevate，资源路径由宿主输入；不是UI展示                                       |
| `src/core/download/mod.rs`                  | C/I   | shared download session/status/cancel/retry engine，复用bolt-load                                                     |
| `src/core/download/adapter.rs`              | I     | reqwest adapter／request失败分类／proxy+UA输入                                                                        |
| `src/core/updater/mod.rs`                   | C     | core update actor/typed client/manifest orchestration                                                                 |
| `src/core/updater/instance.rs`              | C/I   | artifact download/preparation/progress与HTTP backend                                                                  |
| `src/core/updater/ports.rs`                 | C     | updater backend/installer/progress消费端口                                                                            |
| `src/core/updater/shared.rs`                | C     | platform/artifact/release path policy；unsupported平台明确返回，不扩大桌面target假设                                  |
| `src/core/updater/tests.rs`                 | T→C/I | 原updater tests随能力                                                                                                 |

### 5.6 Persistence、backup、migration、state

| 当前文件                                            | 归属 | 去向／依据及拆分点                                                                                                                             |
| --------------------------------------------------- | ---- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/core/storage.rs`                               | S    | C/I：KV/store/export/typed errors/change subscription；G：StorageValueChangedEvent、register_web_storage_listener；UI storage命名不代表DB属GUI |
| `src/core/backup.rs`                                | C/I  | backup policy/request/info、consistent storage export、FS archive/prune，migration与facade共用                                                 |
| `src/core/migration/mod.rs`                         | C    | migration state/advice/context/step/module协议                                                                                                 |
| `src/core/migration/fs.rs`                          | I    | crash-safe document writes／atomicwrites                                                                                                       |
| `src/core/migration/registry.rs`                    | C    | migration module registry；不变查找表不是service locator                                                                                       |
| `src/core/migration/runner.rs`                      | C/I  | #5645 改为显式 roots，不隐式发现宿主路径；migration／backup／settlement／FS 及原 tests 仍待同迁                                                |
| `src/core/migration/store.rs`                       | C/I  | migration/task journal persistence，明确paths                                                                                                  |
| `src/core/migration/modules/mod.rs`                 | C    | capability migration模块入口                                                                                                                   |
| `src/core/migration/modules/app_config.rs`          | C    | language/theme/widget等stored schema migrations；数据迁移不需要UI                                                                              |
| `src/core/migration/modules/profiles.rs`            | C    | profile document/schema cleanup/repair规则                                                                                                     |
| `src/core/migration/modules/storage.rs`             | C/I  | legacy KV/hotkeys→typed config迁移                                                                                                             |
| `src/core/migration/modules/typed_config.rs`        | C/I  | legacy split/path repair migrations                                                                                                            |
| `src/core/migration/legacy_schema/mod.rs`           | C    | migration-only legacy schema与conversion                                                                                                       |
| `src/core/migration/legacy_schema/application.rs`   | C    | legacy application→typed转换                                                                                                                   |
| `src/core/migration/legacy_schema/clash.rs`         | C    | legacy Clash template数据                                                                                                                      |
| `src/core/migration/legacy_schema/clash_config.rs`  | C    | legacy Clash config转换                                                                                                                        |
| `src/core/migration/legacy_schema/session_state.rs` | C    | legacy session/window state转换，非window server                                                                                               |
| `src/core/migration/legacy_schema/verge.rs`         | C    | 历史wire/schema镜像；GUI字段作为数据保留，不引入egui依赖                                                                                       |
| `src/state/mod.rs`                                  | C    | application state模块入口，复用已有nyanpasu-core/state事务基础                                                                                 |
| `src/state/application.rs`                          | C    | application config serial owner/snapshot/ports/commits                                                                                         |
| `src/state/clash_config.rs`                         | C    | Clash config serial owner及source事务                                                                                                          |
| `src/state/session_state.rs`                        | C    | session state owner，不需要实际window                                                                                                          |
| `src/state/config_error.rs`                         | C    | config/source domain structured errors；desktop-only causes按职责解耦                                                                          |
| `src/state/mutation.rs`                             | C    | MutationCoordinator、source settlement/RuntimeAftermath/handoff；不是UI event sink                                                             |
| `src/state/profiles/mod.rs`                         | C    | profiles state能力入口                                                                                                                         |
| `src/state/profiles/actor.rs`                       | C    | profiles document serial owner、refresh/import/reorder/transactions                                                                            |
| `src/state/profiles/error.rs`                       | C    | profiles/file/subscription/materialization typed errors                                                                                        |
| `src/state/profiles/jobs.rs`                        | C    | committed profile slice派生job registrations，依赖shared jobs owner                                                                            |
| `src/state/profiles/ports.rs`                       | C    | consumer-owned FS/fetch/materialization/cleanup/reconcile端口                                                                                  |
| `src/state/profiles/scheduler.rs`                   | C/I  | actor-owned per-uid refresh/watch producer lifecycle，concrete watch平台adapter分层                                                            |
| `src/state/profiles/sources.rs`                     | C    | source receipts/status ledger及watch snapshot                                                                                                  |

### 5.7 Observability：logs、traffic、geo

| 当前文件                     | 归属  | 去向／依据及拆分点                                                                             |
| ---------------------------- | ----- | ---------------------------------------------------------------------------------------------- |
| `src/core/logs/mod.rs`       | C     | logs capability入口                                                                            |
| `src/core/logs/actor.rs`     | C     | core log serial owner、query/capture/clear lifecycle                                           |
| `src/core/logs/model.rs`     | S     | C：records/cursor/query/status/errors/level normalization；G：CoreLogsChanged的Tauri event包装 |
| `src/core/logs/ports.rs`     | C     | consumer-owned CoreLogStore，Unavailable是真实降级不是成功no-op                                |
| `src/core/logs/redb.rs`      | I     | Redb core log storage backend                                                                  |
| `src/core/logs/codec.rs`     | I     | zstd log codec与preset字典资源                                                                 |
| `src/core/logs/tests.rs`     | T→C/I | 原logs tests随能力                                                                             |
| `src/core/traffic/mod.rs`    | C     | traffic recording application integration；算法／store复用nyanpasu-traffic                     |
| `src/core/traffic/actor.rs`  | C     | recording session owner、batch/retry/storage失败降级                                           |
| `src/core/traffic/client.rs` | C     | typed TrafficClient                                                                            |
| `src/core/traffic/geo.rs`    | C     | endpoints region lookup／CountryLookup                                                         |
| `src/core/traffic/ports.rs`  | C     | local source/profile selection/retention/clock端口                                             |
| `src/core/traffic/source.rs` | C     | raw Clash feed→accounting frames                                                               |
| `src/core/traffic/tests.rs`  | T→C   | 原traffic integration tests随能力                                                              |
| `src/core/geo/mod.rs`        | C     | running-core country index能力，不是UI map绘制                                                 |
| `src/core/geo/actor.rs`      | C     | index serial owner与core geodata-mode同步                                                      |
| `src/core/geo/client.rs`     | C     | typed GeoIndexClient                                                                           |
| `src/core/geo/ports.rs`      | C     | CountryIndexSource／loaded/key/error/watch契约                                                 |
| `src/core/geo/adapters.rs`   | I     | FS DB discovery/hash/mmap cache/watch；`.idx image`为二进制索引                                |
| `src/core/geo/fixtures.rs`   | T→C/I | 原geo test data builder随能力，不置于crate-root tests                                          |
| `src/core/geo/tests.rs`      | T→C/I | 原geo tests随能力                                                                              |

### 5.8 Runtime preparation 与脚本执行

| 当前文件                                | 归属  | 去向／依据及拆分点                                                                         |
| --------------------------------------- | ----- | ------------------------------------------------------------------------------------------ |
| `src/enhance/mod.rs`                    | C/I   | 剩余runtime preparation/script能力入口；RuntimeBuilder已迁，不重建旧入口shim               |
| `src/enhance/artifact_snapshot.rs`      | C     | RuntimeArtifact→runtime snapshot/log layout纯映射                                          |
| `src/enhance/chain.rs`                  | C     | postprocessing/script/log DTO                                                              |
| `src/enhance/content_source.rs`         | I     | FsProfileContentSource，复用profiles路径／文件adapter                                      |
| `src/enhance/utils.rs`                  | C     | LogSpan等runtime log语义，落具体能力而非core/utils                                         |
| `src/enhance/script/mod.rs`             | C/I   | script capability入口，按runner契约／执行adapter分层                                       |
| `src/enhance/script/adapter.rs`         | I     | EnhanceScriptRunner／private runtime／blocking execution adapter，CLI可复用                |
| `src/enhance/script/runner.rs`          | C/I   | runner/console契约与RunnerManager执行机制                                                  |
| `src/enhance/script/js.rs`              | I     | #5645 改用共享路径快照，Tauri paths import 已替换；Boa runner／ScriptDirs 能力整体仍待迁移 |
| `src/enhance/script/lua/mod.rs`         | I     | Lua context/runner实现                                                                     |
| `src/enhance/script/lua/console.rs`     | I     | Lua console与print映射，保持JS/Lua log语义                                                 |
| `src/enhance/script/lua/ordered_map.rs` | I     | ordered YAML↔Lua conversion，保留mapping顺序语义和原tests                                  |
| `src/enhance/golden.rs`                 | T→C/I | 原real FS/script golden coverage随runtime preparation，更新manifest-relative fixture路径   |
| `src/enhance/golden_support.rs`         | T→C/I | 原golden support随runtime/script能力                                                       |

### 5.9 所有 utils 文件：逐项拆，不搬一个 utils 桶

| 当前文件                    | 归属    | 去向／依据及拆分点                                                                                                                               |
| --------------------------- | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| `src/utils/mod.rs`          | S       | 迁移后仅声明GUI helpers；shared按能力落位，不复制generic utils模块                                                                               |
| `src/utils/blocking.rs`     | I       | blocking JoinError/panic传播helper；供真实shared消费者复用，不改变panic语义                                                                      |
| `src/utils/candy.rs`        | C/I     | 分为log archive、HTTP client/proxy+UA、mirror URL/speed probe；路径／版本输入显式化，原tests随各能力                                             |
| `src/utils/collect.rs`      | C/I     | diagnostics DTO／OS/device/core版本收集，BuildInfo/binary resolver显式输入                                                                       |
| `src/utils/color.rs`        | G       | system accent color与hex展示                                                                                                                     |
| `src/utils/config.rs`       | I       | system/self proxy与reqwest builder extension，供fetch/download共用                                                                               |
| `src/utils/dialog.rs`       | G       | panic/migration/error/warning/ask native dialogs，接收core错误数据                                                                               |
| `src/utils/dirs.rs`         | S       | #5645 将目录／binary／instance 机制移入 nyanpasu-paths，GUI 输入移至 host_paths；此文件仅剩 version metadata／原常量                             |
| `src/utils/dock.rs`         | G       | macOS Dock/NSApplication展示                                                                                                                     |
| `src/utils/exit.rs`         | S       | G：Tauri ExitGate/ExitBoundary/restart code/UI handoff；C：core shutdown调用；I：relaunch process机制；不是共享core exit gate                    |
| `src/utils/help.rs`         | S       | C/I：YAML读取/merge、UID、subscription字段解析、shared日志helper；G：open_file/DPI/tray image/quit/restart/dialog宏/UI locale helper             |
| `src/utils/hwid.rs`         | I       | subscription身份headers所需OS/device信息与sanitize/fallback；现有static另审，不复制隐式service状态                                               |
| `src/utils/init/mod.rs`     | S       | C/I：config/resources初始化、migration process/output、home迁移、instance lock机制；G：GUI exe/helper选择与UI failure接入                        |
| `src/utils/init/logging.rs` | S       | I：file appender/rotation/reload/log capture；G：frontend决定如何安装process tracing subscriber，core库不在import时安装全局subscriber            |
| `src/utils/init/tests.rs`   | T→C/I/G | 原migration stdout/stderr drain／reader panic／relaunch测试随机制；更新fixture test全名及child入口，不新增测试                                   |
| `src/utils/main_thread.rs`  | G       | 识别UI/event-loop主线程，用于GUI executor                                                                                                        |
| `src/utils/net.rs`          | C/I     | URL delay与IP/ASN诊断，明确HTTP/proxy输入及deadline                                                                                              |
| `src/utils/open.rs`         | G       | 外部app/file/url launcher；不是业务文件I/O                                                                                                       |
| `src/utils/path.rs`         | C/I     | #5645 删除此模块，改为 nyanpasu-paths::PathResolver／ResolvedPaths；不再规划迁入 core 的重复实现                                                 |
| `src/utils/proxy_env.rs`    | S       | C：proxy env文本及CopyEnvOption；G：clipboard manager写入；原文本测试随C                                                                         |
| `src/utils/resolve.rs`      | S       | G：window/tray-menu focus/editor/ready timeout；C：startup/background-source API；I：resolve_core_version改用shared process adapter与typed error |
| `src/utils/sudo.rs`         | I       | macOS elevation/process-output机制，GUI授权展示不等于Tauri专属                                                                                   |
| `src/utils/winhelp.rs`      | I       | Windows版本查询机制；当前未挂载，记录去向不自动清理                                                                                              |
| `src/utils/winreg.rs`       | I       | #5645 删除此模块及 SOFTWARE_KEY Lazy；registry／SID／instance 机制改由 nyanpasu-paths 内部 Windows 实现承接                                      |
| `src/utils/winreg_test.rs`  | T→I     | #5645 删除此模块，原 tests 移至 nyanpasu-paths 的 Windows 模块；仍有真实 OS 用户环境依赖，不是纯 fake 测试                                       |

### 5.10 Tray：纯算法也可以确实属于 GUI

| 当前文件                    | 归属 | 去向／依据及拆分点                                                                                                |
| --------------------------- | ---- | ----------------------------------------------------------------------------------------------------------------- |
| `src/core/tray/mod.rs`      | G    | native tray construction/attached menu/view/queue/menu events；业务actions调用core，迁到GUI语义路径而非应用core   |
| `src/core/tray/display.rs`  | G    | complete tray publication/paint/unknown display状态机，纯值但仅服务呈现                                           |
| `src/core/tray/executor.rs` | G    | UI main-thread coalesced tray queue/drain，原tests留GUI                                                           |
| `src/core/tray/icon.rs`     | G    | tray image选择/cache/resize/scale/custom-icon持久化；与通用URL icon下载cache区分                                  |
| `src/core/tray/proxies.rs`  | S    | G：proxy menu与tray refresh；独立frontend mutation forwarding从tray生命周期剥离，cache/view truth仍在core/proxies |

## 6. 全部非 Rust 跟踪文件去向

以下包含全部 **49 个非 Rust 文件**。路径仍相对于 `backend/tauri/`。

### 6.1 Manifest、配置、installer 与 UI locale

| 当前文件                             | 归属 | 去向／依据                                                                                           |
| ------------------------------------ | ---- | ---------------------------------------------------------------------------------------------------- |
| `.gitignore`                         | G    | GUI生成物／资源忽略规则；shared新增crate各自负责ignore                                               |
| `Cargo.toml`                         | S    | GUI dependencies保留，业务／shared infrastructure dependencies随消费者迁出；不能将全部依赖复制到core |
| `Info.plist`                         | G    | macOS GUI bundle metadata                                                                            |
| `capabilities/main.json`             | G    | Tauri capability/permissions                                                                         |
| `tauri.conf.json`                    | G    | GUI bundle/build/assets/plugins配置                                                                  |
| `tauri.windows.conf.json`            | G    | Windows GUI配置                                                                                      |
| `overrides/fixed-webview2.conf.json` | G    | GUI fixed WebView2发行配置                                                                           |
| `overrides/nightly.conf.json`        | G    | GUI nightly发行覆盖；shared channel使用值输入                                                        |
| `templates/cleanup.wxs`              | G    | Windows GUI installer cleanup                                                                        |
| `templates/installer.nsi`            | G    | GUI NSIS installer                                                                                   |
| `templates/installer.wxs`            | G    | GUI WiX installer                                                                                    |
| `windows-app-manifest.xml`           | G    | GUI Windows manifest与dialog/Common-Controls依赖；core测试不应需要GUI manifest                       |
| `locales/en.json`                    | G    | tray/dialog等UI文案                                                                                  |
| `locales/ko.json`                    | G    | UI文案                                                                                               |
| `locales/ru.json`                    | G    | UI文案                                                                                               |
| `locales/zh-cn.json`                 | G    | UI文案                                                                                               |
| `locales/zh-tw.json`                 | G    | UI文案；core返回结构化错误/状态，不搬展示locale bundle                                               |

### 6.2 全部图标资产

| 当前文件                       | 归属 | 去向／依据               |
| ------------------------------ | ---- | ------------------------ |
| `icons/128x128.png`            | G    | GUI package icon         |
| `icons/128x128@2x.png`         | G    | GUI package icon         |
| `icons/32x32.png`              | G    | GUI package icon         |
| `icons/Square107x107Logo.png`  | G    | Windows GUI package logo |
| `icons/Square142x142Logo.png`  | G    | Windows GUI package logo |
| `icons/Square150x150Logo.png`  | G    | Windows GUI package logo |
| `icons/Square284x284Logo.png`  | G    | Windows GUI package logo |
| `icons/Square30x30Logo.png`    | G    | Windows GUI package logo |
| `icons/Square310x310Logo.png`  | G    | Windows GUI package logo |
| `icons/Square44x44Logo.png`    | G    | Windows GUI package logo |
| `icons/Square71x71Logo.png`    | G    | Windows GUI package logo |
| `icons/Square89x89Logo.png`    | G    | Windows GUI package logo |
| `icons/StoreLogo.png`          | G    | Windows GUI store logo   |
| `icons/icon-shrink.png`        | G    | GUI icon                 |
| `icons/icon.icns`              | G    | macOS GUI icon           |
| `icons/icon.ico`               | G    | Windows GUI icon         |
| `icons/icon.png`               | G    | GUI icon                 |
| `icons/tray-icon.ico`          | G    | tray icon                |
| `icons/tray-icon.png`          | G    | tray icon                |
| `icons/win-tray-icon-blue.png` | G    | Windows tray icon        |
| `icons/win-tray-icon-pink.png` | G    | Windows tray icon        |
| `icons/win-tray-icon.png`      | G    | Windows tray icon        |

### 6.3 能力资源与已有 fixtures

| 当前文件                                                         | 归属  | 去向／依据                                                                                        |
| ---------------------------------------------------------------- | ----- | ------------------------------------------------------------------------------------------------- |
| `src/core/logs/preset.zdict`                                     | I     | codec资源随logs能力迁移，保持bytes与decode兼容                                                    |
| `src/core/migration/fixtures/v1_6_1/nyanpasu-config.yaml`        | T→C   | migration capability内fixtures                                                                    |
| `src/core/migration/fixtures/v1_6_1/profiles.yaml`               | T→C   | migration capability内fixtures                                                                    |
| `src/core/migration/fixtures/v2_0_expected/nyanpasu-config.yaml` | T→C   | migration expected fixture                                                                        |
| `src/core/migration/fixtures/v2_0_expected/profiles.yaml`        | T→C   | migration expected fixture                                                                        |
| `src/enhance/fixtures/golden/builtin_clash_rs.yaml`              | T→C/I | runtime preparation/script golden fixture                                                         |
| `src/enhance/fixtures/golden/builtin_mihomo.yaml`                | T→C/I | runtime preparation/script golden fixture                                                         |
| `src/enhance/fixtures/golden/composition_global_chain.yaml`      | T→C/I | runtime preparation/script golden fixture                                                         |
| `src/enhance/fixtures/golden/whitelist_on.yaml`                  | T→C/I | runtime preparation/script golden fixture                                                         |
| `tests/sample_clash_config.yaml`                                 | T→C   | runtime/Clash config能力fixture；当前源码搜索未见引用，实施时核实消费者，不在审计中删除或新造测试 |

## 7. Gitignored 内容同样不能变成 core 的 GUI 前提

| 目录                | 归属与迁移约束                                                                                                                                                          |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `sidecar/`          | core/service executables是运行资源，不是UI逻辑。GUI externalBin下载／target-triple命名与打包留GUI tooling；共享binary lookup接收安装／开发资源位置，不从Tauri源码树反查 |
| `resources/`        | `Country.mmdb`、geoip/geosite、wintun、service/UWP executables等运行能力资源由宿主或独立分发供应；共享初始化／执行机制迁出，GUI bundling配置留下                        |
| `tmp/dist/`         | 前端assets；core编译／构造／测试无需placeholder，也不能要求CLI打包它                                                                                                    |
| `tmp/git-info.json` | GUI build metadata输入；shared BuildInfo从宿主输入，不强制依赖此文件                                                                                                    |
| `gen/schemas/`      | Tauri capability/config生成物，留GUI，不作为core API schema                                                                                                             |

这里没有要求现在搬动下载缓存或另建共享分发目录；只要求核心的运行输入和分发适配分离。worktree资源复用仍遵循现有workflow规范，不能共享 `backend/target/` 或 `tmp/dist/`。

## 8. 依赖迁移审查

依赖跟随职责迁移，按正常／transitive依赖和features检查，不能只看直接 `use tauri`：

| 依赖类别                                                                                                                               | 最终归属                                                                                                                        |
| -------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Tauri、tauri-build、tauri-specta／query、Tauri plugins、webview2-com、window-vibrancy、rfd、nyanpasu-egui、display-info、UI rust-i18n  | GUI／transport；core正常依赖不引入这些                                                                                          |
| nyanpasu-config、core-manager、clash-api、ipc、jobs、logging、traffic、geodata，ractor/Tokio/TaskTracker，Snafu/Serde/Specta           | shared能力按真实消费者依赖；不因Specta DTO就依赖tauri-specta Event                                                              |
| reqwest、bolt-load、sysproxy、auto-launch、runas、notify、redb/zstd、atomicwrites、FS/archive、Boa/Lua/OXC、device/registry/system API | shared infrastructure按能力／平台分层；GUI如仍有真实消费者可继续依赖，不一刀切删除                                              |
| Axum、HTTP/SSE/Channel、inventory command registration、transport auth/session                                                         | frontend／transport，而非core facade；普通HTTP出站client与HTTP server不能混为一类                                               |
| OS-specific crates（Windows/macOS/Unix）                                                                                               | 逐feature／API分拆：window/color/WebView2为GUI，proxy/process/permissions/shutdown检测为platform；不整包复制target dependencies |
| mockall、fake-core、fixtures、test runtime                                                                                             | 现有测试随owner；mock-only接口维持cfg(test)，不进入production capability要求                                                    |

PR #5645 的 `nyanpasu-paths` 是独立共享基础设施依赖，GUI 与未来 core 按消费者接入；不将其源码再搬进 core。GUI 的 `convert_case` 依赖随目录 casing 归属移至该 crate，不把 registry／FS 等 platform dependencies 整包复制到 core。

Schema/config字段可以描述UI偏好而保持Tauri-free；这是数据，不是强制构造UI service。已完成的 `nyanpasu-egui → nyanpasu-config` 依赖反转必须保持。

## 9. 已完成工作与后续迁移顺序

### 9.1 历史 Phase 01 基线已实现，不再列为待搬

| 能力                                    | 现位置／状态                                                                                              |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------- |
| RuntimeBuilder与4个builtin脚本          | `backend/nyanpasu-core/src/runtime/`；原Tauri实现已删除                                                   |
| Runtime build/preparation error分层     | core `RuntimeBuildError` vs consuming workflow `RuntimePreparationError` 已完成；后者仍随workflow后续迁移 |
| connection rate derivation              | `backend/nyanpasu-core/src/connections/rates.rs`；原10个算法tests保留，StreamsActor尚待迁移               |
| ServiceCompat及2个fixtures              | `backend/nyanpasu-core/src/service/`；原16个tests保留，ServiceActor/OS控制尚待迁移                        |
| widget enum与依赖方向                   | `backend/nyanpasu-config/src/application/widget.rs`；config不依赖egui，egui消费config                     |
| 通用state transaction machinery／format | 已在nyanpasu-core，后续state owners复用，避免第二套事务机制                                               |

对应实现提交为 `f24c8cb5c`、`7b06c5d39`、`8f344a358`。PR [#5629](https://github.com/libnyanpasu/clash-nyanpasu/pull/5629) 是 Phase 1／PR 01；它的实现与历史focused local验证记录不应重置为pending。本文不重新声明merged或跨平台CI通过。

这些是 Phase 1 历史基线中的实现状态，不代表已合入最新 main。旧 Phase 2／02-A 本地路径实现已丢弃，不能继续作为完成记录；当前分支名也不代表后续边界工作已落地。

**最新 main 接入覆盖：** 本次获授权将 01 的自身提交接到新 00（`2e03a81a4`），再将 02 任务 1 接到新 01（`9656d6021`），共同 main 基线为 `8ac8ba84c`。main 已通过 #5652 将 RuntimeBuilder／builtin 迁入 `nyanpasu-application`，script／FS adapters 迁入 `nyanpasu-platform`，widget enum 归中立 `nyanpasu-helper`；本次保留这些唯一实现与 #5662／#5663 的快照图、单次序列化行为，不恢复旧 core runtime 模块或 config 内重复 enum。01 首项因此调整为 build／preparation 错误分层（`66860b466`）；原 widget／CLI／golden／wire tests 保留并消费新 owner。连接速率（`69650a859`）与 service compatibility（`9656d6021`）仍归 core，后者保留 main 的最低服务版本 `2.0.0-rc.10` 与当前原测试。

### 9.2 独立路径 PR #5645：历史核对与最新集成覆盖

| 项目                    | PR 内状态与后续处理                                                                                                                     |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| 路径／平台／binary 机制 | 独立 `backend/nyanpasu-paths` 已在 PR 内实现；以第 4.2 节的实际范围为准，不再作为 Phase 02 待编写的 core paths 模块                     |
| GUI 装配与调用者接入    | host_paths／setup 供应真实宿主输入，现有调用者使用 resolver／快照；service、runtime、migration 等业务 owners 仍在 Tauri                 |
| 删除与测试归属          | PR 内删除旧 path／winreg／binary 实现和 deprecated legacy API，移动原 tests；不留 compatibility shim，不新增迁移 tests                  |
| 集成与未完成范围        | 历史核对时 open；现已作为 main `8ac8ba84c` 合并并接入本 stack。版本／device／资源初始化／permissions／service 执行／facade 继续后续阶段 |

PR 内的 API／边界说明见其 [README](https://github.com/libnyanpasu/clash-nyanpasu/blob/65dd5c8be7094630d925ef93318a3e59b51ec3b9/backend/nyanpasu-paths/README.md) 和 [执行计划](https://github.com/libnyanpasu/clash-nyanpasu/blob/65dd5c8be7094630d925ef93318a3e59b51ec3b9/docs/plan/2026-10-06-extract-paths.md)。这些链接固定于本次核对的 head，避免当前 checkout 缺少独立 PR 文件时产生错误的本地链接。PR 的本地测试、平台限制、CI 与合并状态独立记录；本文这次只做文档核对，不重跑或推断平台通过。

### 9.3 对齐 stacked roadmap，但每次迁移都完整更新调用者

沿用 PR [#5621](https://github.com/libnyanpasu/clash-nyanpasu/pull/5621) 的阶段顺序；独立 paths PR 是 main 上的前置能力，不加入或改写 00→01 stack。此审计仍是完整范围表，不另建迁移路线：

| Phase | 本次全目录审计补足的范围                                                                                                                                             |
| ----- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 00    | 已有architecture gate/baselines，不扩展成新的迁移测试项目                                                                                                            |
| 01    | 已实现pure services/config依赖反转；不追加无消费者的helper来扩充第二个PR                                                                                             |
| 02    | 复用 #5645 的路径能力；处理剩余 resources/version/channel/build/device 输入、errors/contracts、bundle 分界与 notification seams，不重写 effects 调度                 |
| 03    | persistence/KV/backup/migration及shared init/FS机制，剥离web storage emitter                                                                                         |
| 04    | core/service control plane、core spec/permissions、IPC/elevation、UWP 等 platform 机制；binary lookup 复用 nyanpasu-paths，不重复提取                                |
| 05    | system proxy/PAC/autostart/DNS等应用／平台effects与logger基础设施；保留GUI registration／presentation adapter边界                                                    |
| 06    | streams/proxy cache、logs/traffic/geo与typed observations；独立前端delivery，不借tray保活                                                                            |
| 07    | Clash/Mihomo代理内核updater状态机、URL/policy/progress与download机制；Tauri应用自身更新整体留GUI，不迁core                                                           |
| 08    | profiles FS/materialization/fetch/hwid headers、script runners/preparation/golden及runtime foundations                                                               |
| 09    | source state owners／mutation／TCC／recovery／effects调度职责分离，保持settlement、retry和notification intent                                                        |
| 10    | independent NyanpasuClient/bootstrap/lifecycle，消除必填HTTP/assets/window/tray/widget/frontend-log依赖，完成最小headless assembly；Tauri应用updater从共享facade剥离 |
| 11    | frontend接入和终审：transport/native UI/thin setup、bindings、无主窗口观察与交付、平台矩阵；不是旧模块删除兜底阶段                                                   |

Phase 02 的 resources／version／channel／build／device 输入细化见[独立子计划](../plan/2026-10-06-extract-core-02-host-inputs.md)。该文档已拆为七个原子任务：任务 1 原在 `70ff8350f` 交付，本次 rebase 并推送为 `2eaa58b85`，见第 9.4 节；任务 2 已从 stash 恢复、适配并通过新基线本地验证，用户已审阅并授权本次提交／推送，见第 9.5 节；任务 3—7 尚未实施。resources 位置复用 #5645，资源初始化与 UWP 执行仍归后续能力阶段；不把子任务完成视作整个 Phase 02 完成。

各阶段允许按最小完整调用链调整依赖顺序，但必须在同一个PR更新所有调用者、删除旧实现／module声明；没有re-export shim，没有“Phase 11再清理”。frontend-only端口和原测试可以跟拆分能力一起调整，不意味着一次性重做全套UI。

### 9.4 Phase 02 任务 1：本地 build／diagnostics 契约提取

任务 1 在 `8f344a358404f43fe118cc9145f1e9eeba24a5f5` 基线上实现、验证并通过用户审阅，源码及配套文档随本次任务 1 提交交付，未集成 #5645。第 5／6 节继续是原 275 文件历史 inventory；这里记录任务 1 增量，不重写历史文件集合或宣称 main／PR 已交付。

- `BuildInfo`、`EnvInfo`、diagnostics `DeviceInfo`、`CoreInfo` 已归 `backend/nyanpasu-core/src/diagnostics/mod.rs`，原 GUI 定义删除，没有 re-export shim。
- 同一模块声明窄 `EnvironmentCollector` 契约；GUI `OsEnvironmentCollector` 显式持有真实 BuildInfo，保留原 OS／进程采集。setup 注入 adapter，RPC 通过 facade 调用；CLI 直接构造相同 adapter。
- GUI 编译环境装配、完整 diagnostics／core-version 执行能力、channel、订阅 device／UA、backup／migration 和资源初始化不因本任务而记作已迁移。
- 本地原 core 140、client 576、RPC 5／1 ignored、Specta 1、macro 7 通过；生成 bindings 逐字节不变。Clippy／Rustfmt／architecture gate／49 ledger tests／Deno checks 通过。
- 完整 GUI suite 为 1162 passed／6 ignored／1 原硬件 model 失败；同一断言在原 main 基线复现，未改动 HWID 源码。仅明确过滤该项后的串行 suite 为 1162 passed／6 ignored／1 filtered；不宣称未过滤 suite 全通过。
- 未新增迁移 tests。用户已另行授权任务 1 提交／推送，不创建 PR 或修改其他分支；未跑真实 CLI collect 端到端、Windows／macOS runtime 或 CI／merge 验证。执行与限制详见子计划第 8 节。

**本次 rebase 适配：** diagnostics collector 同时持有真实 BuildInfo 与 `nyanpasu-paths::PathResolver`，setup／CLI 注入同一 resolver；保留 main 的 binary lookup、目录准备与错误／进程顺序。第 5／6 节完整历史 inventory 与上述最初 PR 核对记录不改写为当前文件清单；最新集成状态以第 9.1／9.2 节覆盖说明为准。本次验证记录见子计划第 10 节，不由历史测试结果推断新基线通过。

### 9.5 Phase 02 任务 2：本地 channel／bundle 输入分离

本任务最初以 `70ff8350f2a92dbc7168bf80e08d8f2c595bcc42` 为基线，在用户选择的当前 checkout 实施；下面测试数字是最初执行记录。本次已从原 stash 恢复到接入最新 main 的 `2eaa58b85` 基线，保留 main 新增的 ApplicationFormat 类型参数；用户已在恢复与验证后审阅通过并另行授权本次任务 2 提交／推送。第 5／6 节仍是原历史 inventory，不改写为当前文件集合。

- `ClientSetupArgs`、facade inner 与私有 assembly 不再接收／持有 `BundleMetadata`，只使用真实需要的 installed ReleaseChannel 与 portable bool；setup 从既有 GUI metadata 供应相同值，没有复制 fixed WebView 字段或再次探测。
- application client／facade／RPC 直接使用 config ReleaseChannel，旧 GUI public alias 与全部消费者同步删除／更新。GUI bundle 只保留普通私有 import。
- 原 application actor／channel 状态规则未改；app updater 仍留 GUI，`unwrap_or`／`resolve` 区别、portable 支持判定、WebView／feeds／发行包策略及原 tests 保留。
- 原 bundle 16、client 576、RPC 5／1 ignored、Specta 1、macro 7、core 140 通过；bindings 字节不变。Clippy／Rustfmt／architecture gate／49 ledger tests／Deno checks 通过。
- 串行完整 suite 为 1162 passed／6 ignored／1 明确 filtered，仅过滤任务 1 已复现的原硬件 model test。本轮未另跑未过滤完整 suite，未宣称该问题修复。
- 首次 client suite 曾因原 Socks port `48234` 不可用失败；端口源码未改，单项与完整重跑通过，首次失败独立记录。未新增迁移 tests，未做 Windows／macOS runtime、自更新安装或 CI／merge 验证；详情见子计划第 9 节。
- 新基线恢复后 bundle 16、client 584、Specta 1、macro 8、core 134 项通过；串行完整 suite 为 1163 passed／5 ignored／1 明确 filtered。Clippy／Rustfmt／architecture gate／49 ledger tests／backend boundaries／Deno checks 通过；bindings 字节不变，原测试函数清单、bundle body、actor 规则及 app-update 模块未变。用户已另行授权将任务 2 源码与配套文档纳入本次原子提交／推送，不创建 PR 或推进后续任务；详见子计划第 11 节，不以历史验证代替本次结果。

## 10. 验收与执行约束

### 每个能力迁移

- 每个变更行都对应本次能力的归属，未请求的行为修复另开范围。
- 领域schema、structured error、Serde/Specta和wire语义不意外改变；generated bindings通过现有export workflow生成。
- 原actor owner/typed client、transactions/settlement/degradation、coalescing/retry、caller-drop与shutdown语义保留。
- 移走原实现和所有调用者；删除由本次变更形成的孤儿，不顺手删除已有未挂载文件／死代码。
- 不新增迁移专用tests；保留并迁移现有tests/fixtures到能力目录，按现有检查验证。
- capability确实不可用时返回真实状态／错误，不能将不支持包装为成功应用。
- 核对core正常及传递依赖、platform cfg/features、CARGO_MANIFEST_DIR/include paths和test child入口。

### 整体分离完成

1. 不链接Tauri/egui、不读取Tauri configuration、不提供frontend assets或假GUI service，可以构造、操作、观察、关闭应用核心。
2. CLI与GUI可复用真实FS/network/database/script/process/IPC/platform实现，CLI不会为这些能力依赖Tauri crate。
3. 不支持的平台显式表达能力；desktop-only依赖不会迫使移动端编译或运行GUI基础设施。
4. core state/progress/health订阅不要求main window、tray或Tauri event listener；每个frontend独立负责delivery/cache mapping。
5. GUI只保留白名单职责与薄适配；剩余S文件逐项检查，不以“已有trait”“现在没tauri import”作为通过依据。
6. 原有测试、bindings、format/Clippy/architecture gate按影响范围验证；本地通过、跨平台CI、merged状态分别记录，不能互相替代。

### 本文档的完成条件

- 全部275个跟踪文件都已列出且有去向；226个Rust文件与49个非Rust文件分开核对。
- 明确GUI白名单，而非只列迁移候选。
- 混合文件记录保留上限与移出职责；区分历史基线实现、独立 PR 内实现、main 合并和仍待迁移，不把已丢弃实现算作完成或重复规划 #5645 范围。
- 本轮只有文档变更；没有实现修改、PR远端编辑、提交或新增测试。
