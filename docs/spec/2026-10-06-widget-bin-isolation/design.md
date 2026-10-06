# 统计浮窗独立可执行文件与构建打包

**日期：** 2026-10-06

**状态：** Draft；本 PR 仅提交设计，实施前确认构建与打包决策。

**调查基线：** `main@dc2a1e630`。内存证据来自 [Windows 内存剖析报告](../../review/2026-10-06-windows-memory-profile.md)（测量基线 `36c667d97`），不把该历史测量视为当前构建的复测结果。

**需求：** 将统计浮窗编译为独立 bin，并随应用分发；切断主程序到渲染库的依赖，消除由 widget 引入的 `opengl32!GLInitializeThread` 线程分配。

**权威顺序：** 当前 [架构规范](../../development/architecture.md)、[工作流](../../development/workflow.md)与[脚本规范](../../development/scripts.md) > 本 spec。

## 1. 决策摘要与范围

1. 将 `nyanpasu-egui` 的统计浮窗统一为一个独立可执行文件 `nyanpasu-statistic-widget`，通过 `small` / `large` 参数选择外形。主程序不再用自身 EXE 的 `statistic-widget` 子命令运行浮窗。
2. 把配置共用类型、IPC 消息和现有 IPC helper 移到无渲染依赖的 `backend/nyanpasu-widget-protocol`。主程序、配置 crate 与 widget 都直接依赖它；删除主程序和配置 crate 对 `nyanpasu-egui` 的依赖。
3. **推荐在每次应用 release / nightly 流程内，从同一个提交、同一份 Cargo.lock 编译 widget，再打包。** 实际顺序是先编译并准备 widget，再编译 Tauri 主程序，最后一起打包；不假设 Tauri 自动构建其他 workspace bin。
4. widget 作为 Tauri `externalBin` 随安装包、portable 包及 updater 一起分发，不另设下载或升级渠道。分支相关产物放在本 checkout 的 `backend/tauri/tmp/widget-bin/`，不放入允许跨 worktree 共享的 `sidecar/`。
5. 保留现有 IPC 消息语义、浮窗尺寸和外观、流量更新、启动握手、进程退出与 shutdown 的资源所有权。进程路径解析与启动留在 GUI/OS adapter，不向 client、actor 或协议 crate 引入 Tauri 类型。
6. 本次不调整 Tokio worker 数、合并 runtime、修改主窗口透明性、切换渲染后端，或把应用核心整体迁入 `nyanpasu-core`。

最终验收是主程序的依赖图和 Windows 导入/加载链不再因 widget 包含 OpenGL，并且浮窗仍可在所有既有桌面发布目标上启动与退出。内存数值是辅助证据，不能仅以总内存下降作为成功标准。

## 2. 已确认现状与问题

### 2.1 浮窗已经是子进程，但共享主程序 EXE

`backend/tauri/src/widget.rs` 的 `ProcessWidgetHost::spawn` 启动 `current_exe()`，传入 `statistic-widget <variant>`、`NYANPASU_EGUI_IPC_SERVER` 和 `NYANPASU_EGUI_WINDOW_STATE_PATH`。`cmds/mod.rs` 在 Tauri 启动前解析子命令，直接调用 `nyanpasu_egui::widget::start_statistic_widget`。

`nyanpasu-egui/Cargo.toml` 已有 large / small 两个 bin，但生产启动路径未使用它们。这两个入口重复配置窗口，和 `widget/*::run()` 的配置并不完全一致；实施应统一调用现有 `start_statistic_widget`，而不是另复制一套启动逻辑。

### 2.2 只更换进程启动路径不足以隔离渲染库

当前至少存在两条主程序依赖路径：

```text
clash-nyanpasu -> nyanpasu-egui -> eframe / 渲染依赖
clash-nyanpasu -> nyanpasu-config -> nyanpasu-egui
```

`nyanpasu-config/src/application/widget.rs` 从 egui crate 引用 `StatisticWidgetVariant`。`nyanpasu-egui/src/ipc.rs` 的 `Message::UpdateLogo` 又引用渲染模块中的 `LogoPreset`。共享类型和 IPC 必须一起脱离渲染代码。

### 2.3 内存证据与适用边界

报告 §4.5 记录：主进程即使不渲染 OpenGL，也因统计浮窗依赖静态导入 `opengl32.dll`，在线程 attach 时分配内部数据；测量中每线程约 10 KB、合计约 0.9 MB。这是 DLL 的线程初始化开销，不是每线程创建了完整的 OpenGL 渲染上下文。

调查基线的 eframe 已是 0.36.1，widget 使用默认渲染选项。不得仅凭默认渲染器推断最终 EXE 的导入链；实施前后都须检查实际构建产物。若新基线已无该导入，需要记录结果，不承诺仍能获得报告中的 0.9 MB 收益。主程序其他组件或第三方注入也可能加载 OpenGL，归因时需区分来源。

## 3. crate 与入口设计

目标依赖方向：

```text
clash-nyanpasu -----------+
nyanpasu-config ---------+--> nyanpasu-widget-protocol
nyanpasu-egui -----------+
  └─ bin: nyanpasu-statistic-widget
      └─ eframe / 渲染依赖
```

### 3.1 无渲染依赖的共享协议

迁入 `nyanpasu-widget-protocol`：

- `StatisticWidgetVariant`、`LogoPreset`；
- `Message`、`StatisticMessage`、`WidgetIpcError`；
- 当前 IPC server、sender、receiver、握手和释放 helper。

只保留序列化、类型导出、参数解析、IPC 和错误处理所需依赖，不依赖 `eframe`、`egui`、`glow`、`glutin`、`wgpu`、Tauri 或配置 crate。IPC helper 属于进程通信 adapter；不在这里放窗口、配置读取或业务调度。

保留 `StatisticWidgetVariant` 的 serde 名称 `small` / `large`、`NetworkStatisticWidgetConfig` 的 `kind` / `value` 格式及现有消息枚举顺序和数据布局。主程序与 widget 成对更新，本次不增加版本协商协议。

直接更新所有消费者，包括 legacy config migration、ui-effects ports/adapters/tests、effects plan 与 workflow impact tests。不在 egui crate 留共享类型或 IPC 的旧路径 re-export；统一使用新 crate。

`event_handler/widget.rs` 中保留具体 egui widget 类型的旧 `WidgetInstance` 也必须处理，否则仍有渲染依赖。实施先核对调用点；若确认无使用，只删除阻挡依赖切断的该类型及其引用，不借此清理其他历史代码。

### 3.2 一个正式 bin

在 `nyanpasu-egui` 中声明唯一正式 bin `nyanpasu-statistic-widget`，解析必需的 `small` / `large` 参数后，调用 `start_statistic_widget`。

- 复用 large / small 的现有 `run()`，保留 macOS activation policy、窗口配置和 IPC 接收线程。
- Windows release 构建保持隐藏控制台。
- 删除被统一入口替代的两个旧 bin 声明和入口文件；主程序移除 `Commands::StatisticWidget` 及其启动分支。
- widget 是桌面 GUI 产物，不作为 mobile 的编译或打包前置条件。
- 不保留启动自身 EXE 的 fallback；安装遗漏时应报告定位/启动失败。

### 3.3 主程序启动 adapter

`ProcessWidgetHost` 接收解析好的 widget 可执行文件绝对路径，并用现有 `tokio::process::Command` 启动它，参数改为 `<variant>`。保留 IPC server 环境变量、状态文件环境变量、stdio、`kill_on_drop` 与现有进程/握手 cleanup。

组合根按当前 bundle 路径规则解析已安装的 external bin；开发和 profiling 从对应产物目录解析。不得依赖 cwd、PATH 搜索或硬编码 Windows 路径。路径解析仍在 GUI adapter；不为此让 `NyanpasuClient` 成为资源定位器。

缺失可执行文件只使浮窗启动失败并走现有错误路径；禁用浮窗时不影响主程序正常启动。不绕过生命周期管理直接 fire-and-forget 启动进程。

## 4. 构建方式比较

这里区分“本次应用构建先编译 widget”和“独立流水线预发布 widget 后再下载”：前者也会产生打包前置产物，但两者的版本与发布责任不同。

| 项目     | A：本次应用流水线从源码编译（推荐）                       | B：独立预编译并发布后下载                                       |
| -------- | --------------------------------------------------------- | --------------------------------------------------------------- |
| 输入     | 同一应用提交、Cargo.lock、目标平台                        | 独立发布的 widget artifact                                      |
| IPC 配对 | 主程序与 widget 天然同源                                  | 需固定 widget 版本并管理协议兼容                                |
| 构建时间 | 首次多编译一个 bin；可复用本 checkout 与 CI 的 Cargo 缓存 | 应用构建可省 widget 编译，但另需六目标构建和发布流水线          |
| 本地开发 | 直接构建当前源码，便于联调                                | 修改 IPC 或 widget 后仍需本地编译，存在双路径                   |
| 分发维护 | 随同应用签名、打包、更新                                  | 另需 artifact 清单、校验、下载缓存与供应链管理                  |
| worktree | 源码相关产物各自生成                                      | 必须按精确版本/目标校验，不能当 branch-independent sidecar 共享 |
| 当前判断 | 最小完整改动，无新发布系统                                | 目前没有构建耗时证据证明额外维护值得                            |

**建议采用 A。** 只在后续测得 widget 编译显著拖慢发布，且明确 artifact 版本、六目标产出、校验、签名和 IPC 兼容策略后，再单独设计 B。本次不实现独立预编译发布渠道，也不把新 helper 嵌入主 EXE 后运行时解压。

## 5. 构建与打包流程

### 5.1 显式前置构建

在 `scripts/src/` 增加负责 widget 编译和 staging 的 Deno TypeScript 入口，并在根 `deno.jsonc` 登记具名 task，例如 `build:widget`。入口接收 target、Cargo profile 与实际 runner（Cargo / Linux cross），不隐式把 host 当 cross target。

内部步骤：

1. 用当前 workspace 与 lockfile 编译 `-p nyanpasu-egui --bin nyanpasu-statistic-widget`。
2. 从该 target/profile 的真实输出目录取得产物；区分未显式传 target 的 host 目录和显式 target 目录。
3. 复制到本 checkout 的 `backend/tauri/tmp/widget-bin/nyanpasu-statistic-widget-<target-triple>[.exe]`。编译失败不发布旧 staging 文件。
4. 检查目标架构与产物存在后，才允许 Tauri 主程序编译/打包。

使用 `externalBin: ["tmp/widget-bin/nyanpasu-statistic-widget"]`。Tauri 要求输入文件带 target triple；安装后的名称与位置按实际 bundler 结果验证，不把 staging 名称直接当安装名称。[Tauri sidecar 文档](https://v2.tauri.app/develop/sidecar/)

该目录是 branch-dependent 的构建输出，加入忽略规则且不可跨 worktree symlink。不能放到 `backend/tauri/sidecar/` 后按仓库现有 branch-independent 下载资源政策共享。

不要从 Rust `build.rs` 再启动 Cargo：widget 必须在 Tauri 的 externalBin 编译前置校验前准备好，避免递归构建与 Cargo 锁冲突。`cargo build -p clash-nyanpasu`、测试与 Clippy 也需在开发文档中列明这一前置条件。

### 5.2 接入所有入口，避免缺包与重复编译

| 场景                      | widget 构建与分发要求                                                                                               |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Windows release / nightly | 在现有 `tauri build --no-bundle` 前构建一次；standard / fixed-webview 两次 bundle 复用同一产物                      |
| Windows portable          | `scripts/src/release/portable.ts` 显式加入 widget；它目前逐项列举 EXE，不会自动收录新 externalBin                   |
| macOS                     | 与主程序匹配 x86_64 / aarch64 target，随 .app / DMG 与 updater 分发；检查 helper 签名和最终 notarization            |
| Linux x86_64              | 随 AppImage / deb / rpm 分发；检查打包路径与可执行权限                                                              |
| Linux aarch64             | 使用当前 cross runner 和匹配的图形开发库，不用 host widget 替代；随当前 deb / rpm 流程分发                          |
| 本地 dev                  | 在 `tauri dev` 启动前编译 debug widget；变更 widget 或共享协议后可重新运行具名 task，不以发布版 helper 代替当前源码 |
| profiling                 | `pnpm build:profiling` 前准备相同 target 的 profiling widget，运行目录内有可启动的 helper                           |
| backend CI                | Tauri crate 的 build/test/clippy 前准备 widget；纯协议 crate 测试不要求 GUI runtime                                 |

发布目标按现有六组合覆盖：Windows、macOS、Linux 各 x86_64 与 aarch64；本次不扩充目标矩阵。主程序的 `nightly`、`verge-dev`、剖析 feature 不盲目转发到 widget。保留父进程传入的配置目录相关环境变量与明确的状态文件路径，验证正式、dev、portable 身份。

构建依赖必须在工作流里可见。仅依赖 `beforeBuildCommand` 不覆盖 `tauri dev`、直接 Cargo 检查和独立 `tauri bundle`；需统一具名 task 与各入口的前置关系。独立 bundle 不重编译，缺 staging 文件时明确失败。更新 nightly/release 配置生成逻辑，保证新增 externalBin 不被覆盖。

### 5.3 安装与更新的一致性

- Windows 检查自定义 NSIS 与 WiX 模板是否通过 externalBin 收录 helper，升级替换与卸载也包含它。
- portable standard / fixed-webview 都包含 helper，不能依赖机器上另装的应用副本。
- macOS helper 位于 app bundle，Linux 按打包布局定位；用现有应用路径规则实现绝对路径解析。
- updater artifact 含主程序和同源 helper；更新前现有 shutdown 先结束运行中的 widget，避免 Windows 文件占用。
- 本次不增加启动时下载 helper 的逻辑。

## 6. 验证与验收

### 6.1 依赖和二进制

1. 按主程序实际 target/features 检查 `cargo tree -p clash-nyanpasu` 与 `nyanpasu-config` 的 normal 依赖，不再通过 widget 拉入 `nyanpasu-egui`、eframe 或渲染库。workspace 全量构建仍包含 widget，不能误当成主程序依赖。
2. 对 Windows 主 EXE 用 `dumpbin /imports` 或 LLVM 等价工具检查普通和 delay import，并沿应用自身 DLL 依赖检查；widget 不再给主程序引入 OpenGL。
3. 在干净环境中分别检查浮窗关闭/开启后的主 PID 与子 PID 模块列表，确认主进程无 widget 导致的 OpenGL 加载。若其他依赖仍引入它，先定位并补齐最小调用路径迁移，不能以“新增 bin”宣布完成。
4. 对 widget 记录真实渲染后端与加载模块；它可按后端需要加载图形库，不要求 widget 本身消除 OpenGL。

### 6.2 行为与资源所有权

- 协议 round-trip 测试覆盖 small/large、Disabled/Enabled、Stop、UpdateStatistic、UpdateLogo；检查配置 JSON 与导出类型，路径移动不改序列化契约。按现有流程重新生成绑定，不手改生成文件。
- 保留并运行 WidgetManager 的 fake-host 测试：正常退出、握手失败/子进程提前退出、阻塞 IPC 清理、退出未确认时继续持有进程。
- adapter 测试核对可执行文件路径、variant、环境变量和不同 profile/target 的 staging 选择；无 helper 或构建失败不得静默使用自身 EXE 或旧目标产物。
- 打包测试断言 portable 两种布局包含 helper、缺 helper 时失败；其他格式至少解包检查 helper 路径、架构与权限。
- 使用显式屏障或确认信号进行 IPC 集成测试，不靠固定 sleep。三平台 GUI smoke 覆盖 small/large、流量更新、切换/禁用、主程序退出时无残留进程及 macOS 浮窗 activation policy。

实施检查包括相关 Rust 单元/集成测试、`pnpm lint:rustfmt`、`pnpm lint:clippy`、`deno task lint:architecture-ledger`、受影响脚本测试与 `deno task lint:deno`、`deno task test:windows-bundle`，以及六目标构建与实际安装包检查。无法运行的平台验证需明确记录，不能宣称全平台通过。

### 6.3 内存复测

沿用报告的 WPR Heap 方法和固定测量条件，对同一 target/profile、同一配置与线程数做前后对照：

- 主进程由 widget 引起的 `opengl32!GLInitializeThread` 存活分配消失；
- 分别记录主进程私有工作集、线程数，以及开启 widget 时的子进程数据和两者总量；
- 不把约 0.9 MB 固定为跨机器门槛，也不把主进程节省误写成消除了 widget 自身需要的图形资源；
- 若当前基线已经没有 OpenGL 线程分配，以导入链、依赖隔离与功能验证作为结果，注明历史收益不再可复现。

## 7. 实施顺序与待确认项

```text
1. 复核当前导入链与依赖来源
   -> verify: 主 EXE / DLL 导入、模块列表、WPR 基线
2. 抽出协议与类型，迁移所有调用方并统一独立 bin
   -> verify: 序列化契约、主程序 normal 依赖图、既有 manager 测试
3. 接入具名构建 task、启动路径与六目标打包
   -> verify: 清洁构建、dev/profiling、portable、安装/升级/卸载
4. 行为与内存复测
   -> verify: small/large smoke、无残留子进程、主 PID 的线程分配归因
```

本 draft 需要确认的产品/构建决策只有：是否采用 A（同一次应用发布从源码构建）；本 spec 推荐 A。实施时还需实测当前导入来源、各 bundle 的 helper 位置/签名、Linux cross 的图形依赖，不以推断代替产物验证。

本次 docs PR 只新增 spec，以 draft 提交并在提交消息中加入 `[skip ci]`。上述编译、打包、GUI 与内存验证属于后续实现 PR，不能因本次文档格式通过而视为已完成。
