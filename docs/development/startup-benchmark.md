# 启动计时与 benchmark：issue #5338

[Issue #5338](https://github.com/libnyanpasu/clash-nyanpasu/issues/5338)
报告 Windows 11 25H2 x64 上运行程序后约 5 秒才出现托盘和窗口。
源码中存在一个可直接解释固定等待的路径：GUI 启动同步等待 `migrate` 子进程，
而该子命令正常返回后，共用的 `DelayedExitGuard` 在退出前固定 sleep 5 秒。
这是当前源码的静态证据，仍需在 issue 所述 Windows release 环境中测量确认其实际占比。

## 采集启动 trace

启动阶段使用原生 `tracing` span，`tracing-chrome` 导出 Chrome Trace Event JSON。
在 [Perfetto UI](https://ui.perfetto.dev/) 中查看阶段重叠、等待和里程碑，
用 Trace Processor 提取样本。手写计时、早期记录缓冲和日志补写已移除。

设置 `NYANPASU_TRACE_DIR=<绝对目录>` 开启导出，默认关闭，不创建 trace 文件。
每次 GUI 运行创建 `startup-<实际 PID>-<Unix 纳秒标识>.json`，不覆盖已有文件。
CLI 参数解析完成后、deep-link prepare 和迁移子进程等待前安装一个 registry；
文件日志仍在原来的阶段初始化，再接入该 registry。Chrome layer 使用独立的启动 target filter，
不受普通日志 Silent 设置、reload 或日志轮转影响，jobs capture 仍接入同一 registry。

`startup_entry` 是采集起点，参数包括实际 `pid`、`debug_assertions`、OS 和架构。
测量不包含 CLI 解析、入口前初始化和启用时的死锁检测线程启动。
CLI 子命令不安装 exporter，因此迁移子进程继承环境变量也不产生额外 trace；
父进程的 `entry.migration_subprocess` 包含完整子进程等待，子进程内部成本用 ETW 进一步分析。
当前 exporter 的顶层 `pid` 固定为 1；识别实际进程应使用入口参数和文件名，不能据此合并多个文件。

Chrome layer 使用 `TraceStyle::Async`，记录 span 创建到最后一个引用关闭的 wall time，包含 `.await` 等待。
异步 future 通过 `Instrument` 传播上下文，不跨 `.await` 持有 entered guard。
测量阶段使用独立 root span：该 exporter 将同一 root 的后代分组到同一个 async track，
并发子阶段可能形成相互交叠的嵌套 begin/end。独立 track 避免这种歧义，
阶段关系通过名称前缀和时间范围定位；UI 轨道层级不等于业务调用树。
span 关闭只表示作用域结束，不表示操作成功，也不能当作 CPU 采样火焰图。

首次里程碑以 `startup.milestone` instant event 的 `milestone` 参数记录，
首次收敛结果以 `startup_outcome` 的 `outcome` 参数记录
（`ready`、`ready_degraded`、`recovery_required` 或 `unsettled`）。
trace 不采集 profile 内容、密钥或完整配置。

bootstrap 拥有 `FlushGuard`。最终 `RunEvent::Exit`、单实例冲突、迁移失败，
以及 panic hook 中直接退出进程的路径先发出 `capture_end`，再排空队列、结束 JSON 并 join writer。
不能仅依赖入口栈变量析构：Tauri `App::run` 和 `process::exit` 不保证该析构发生。
`flush()` 也不代表 JSON 文件结束。强制终止、abort 或写入失败可能留下不完整文件，应单列失败样本。

参考 [TraceStyle](https://docs.rs/tracing-chrome/0.7.2/tracing_chrome/enum.TraceStyle.html)、
[实现源码](https://docs.rs/tracing-chrome/latest/src/tracing_chrome/lib.rs.html)和
[FlushGuard](https://docs.rs/tracing-chrome/0.7.2/tracing_chrome/struct.FlushGuard.html)。

### 尚未闭合的 span

`TraceStyle::Async` 在 `on_new_span` 写 begin，在 `on_close` 写 end。
离开 entered scope 不等于关闭 span；仍存活的 clone 或子 span 引用可能延长其生命周期。
`FlushGuard` 的 Drop 只排空已入队记录并结束 JSON，不会给仍存活的 span 自动补 end。
因此文件可以是完整 JSON，而其中仍有未完成阶段；文件完整性和测量完整性必须分别检查。

采样结束时发出 `capture_end` instant event，再完成文件。已完成的启动阶段应先释放所有 span 引用；
长期 actor、stream 或后台任务使用独立的生命周期 span，不持续持有短期启动阶段的 span。
测量实际异步操作的 span 由操作所有者持有，不能因等待者返回就提前闭合。
若操作在采样结束时仍未完成，保留 begin，不伪造完成时间，也不为收尾强行取消业务工作。

Perfetto 将没有 end 的 slice 保留为 incomplete（`dur = -1`）。
正常 duration 统计只使用完整阶段，同时单独导出未完成阶段的名称、起点和数量。
若要显示截至采样结束已观察到的时间，计算 `capture_end.ts - slice.ts`，
明确标为观测时长/耗时下界，不纳入完整阶段的 median/p95。
参考 [Perfetto 的未完成 slice 处理](https://github.com/google/perfetto/blob/main/src/trace_processor/importers/common/slice_tracker.cc)。

自动化测试覆盖：持有 span clone 时完成 trace，JSON 仍可解析且没有伪造 end；
跨线程释放最后一个引用后再完成 trace，得到配对 begin/end；异步错误返回仍关闭阶段。
不在存活 span 跨越文件边界时使用 `start_new()` 切割 benchmark 样本，它不会在新文件重放已有 begin。

## 保留的测量边界

| 阶段 / 前缀                                               | 测量的工作                                                                                                                                                                                  |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `entry.deep_link`                                         | deep-link prepare                                                                                                                                                                           |
| `entry.single_instance`                                   | 获取单例锁，包括发生冲突时的重试等待                                                                                                                                                        |
| `entry.system_locale`                                     | 系统语言检测与默认 locale 设置                                                                                                                                                              |
| `entry.migration_subprocess`                              | 默认 locale 设置后的迁移子进程等待；无锁时跳过迁移也会记录短作用域                                                                                                                          |
| `entry.logging`、`entry.config_files`                     | 日志创建、配置文件准备                                                                                                                                                                      |
| `entry.transport_schema`、`entry.bundle_context`          | IPC transport schema、Tauri context、安装目录和 bundle/updater 配置                                                                                                                         |
| `entry.debug_bindings_export`                             | 仅 debug：schema 和 bindings 导出、Prettier 子进程                                                                                                                                          |
| `tauri.plugin_construction`、`tauri.build`、`tauri.setup` | 插件对象构造、Tauri build、setup 回调；build 包含 setup                                                                                                                                     |
| `setup.*`                                                 | 路径与边界、进程内迁移、资源复制、runtime 路径与 IPC、核心管理器、core/service actor、jobs、effect adapters、traffic/web/core-log 数据库、client、事件转发、widget 和内部 HTTP server spawn |
| `client.*`                                                | 配置与 session state 加载、陈旧候选清理、profiles、HTTP/log/effect/workflow/updater/proxy/stream/traffic actor 初始化                                                                       |
| `resolve.*`                                               | 托盘图标缓存、startup reconcile、后台源 spawn、Clash connectors/streams、主窗口创建、listeners                                                                                              |
| `reconcile.*`                                             | 首次收敛中的服务命令状态、服务探测、runtime 状态、ownership 建立、profiles 内容、runtime build、config check、host handoff（若发生）、核心 apply                                            |

`spawn` 阶段只计创建/接线或 actor startup 等待，不代表后台工作全部完成。
`reconcile.*` 仅在首次 startup 报告产生前记录，后续普通操作与恢复不作为启动样本。
父阶段包含子阶段，并发后台任务也可能重叠，不能把所有 span duration 相加。

里程碑只记录每个进程的第一次事件，累计延迟从 `startup_entry` 事件计算：

| 里程碑                                              | 含义                                                                                                                          |
| --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `tray_icon_created`                                 | 托盘 builder 成功返回；操作系统实际显示图标可能稍晚                                                                           |
| `main_window_created`                               | 原生窗口/WebView 创建及初始配置完成，此时主窗口仍隐藏                                                                         |
| `main_page_load_started`、`main_page_load_finished` | Tauri 主页面 load 事件，不代表 React 或数据已就绪                                                                             |
| `main_window_ready`                                 | 收到现有前端 ready 事件；前端在 settings 查询成功或失败、show/unminimize/focus 调用结算后发出，不是首帧呈现或全部数据就绪证明 |
| `main_window_fallback_shown`                        | 隐藏窗口超时回退的 show 成功；单独统计，不能当作正常 ready                                                                    |
| `tauri_build_finished`、`event_loop_ready`          | Tauri build 返回、收到 event-loop Ready                                                                                       |
| `silent_start`                                      | 本次配置为静默启动，不应期待立即出现主窗口                                                                                    |

静默启动下，第一次手动打开窗口的里程碑仍从进程入口累计，不能用于非静默启动的窗口延迟比较。
第二次启动撞到已有实例、CLI 子命令、失败和恢复启动也不混入正常启动样本。

## 第一轮：同机建立基线

使用 Windows 11 25H2 x64 的 **release 安装包**，固定 Rust features、资源和 sidecar 版本。
不要用 `tauri dev`、debug 程序或占位 frontendDist 代替真实打包程序。
记录 commit、二进制 SHA-256、Windows build、CPU/内存/磁盘、电源模式、WebView2 版本、
安装路径、服务版本和状态、Defender 状态；保持这些条件一致，不为测试更改安全设置。

准备固定配置快照，包括当前 profile、增强脚本、核心类型、service mode、silent start、
数据库规模和日志级别。主比较组关闭 silent start；另开静默启动组测托盘和核心。
已有配置先完成迁移，再保留这一状态用于正常启动比较；
首次安装和有待迁移的启动各用独立副本，单列结果。

建议先测最小 profile + 本地核心，再测实际 profile + 本地核心，
最后按实际使用方式测已运行的服务模式。固定每组核心/daemon 的初始状态；
未安装、已停止、已运行、残留核心和需要恢复属于不同场景。

每个场景分成以下两组：

1. **重复启动 / warm cache**：同一登录会话中正常退出程序，确认 GUI 进程与它拥有的核心完成退出，
   再启动。先做 3 次预热，再记录至少 30 次；服务组的 daemon 保持指定状态。
   关闭窗口往往只是隐藏，必须通过退出操作结束程序。每次都等待系统恢复到相同的空闲条件。
2. **重启后的首次启动 / cold cache proxy**：每次 Windows Restart 后，在固定的空闲等待条件下
   只记录一次启动，至少 10 次。标明它是“重启后首次启动”，并非证明所有相关文件都没有缓存；
   不把反复结束进程称为清除了系统文件缓存。小样本报告中位数、范围和原始值；
   要比较 cold 组的 p95，增加到至少 30 次，并保留不确定性。

每次采样给一个外部 `run_id`，保存启动条件、该次 trace 和普通应用日志到独立目录。
先确认入口参数 `debug_assertions` 为字符串 `false`，并保存 startup outcome。正常退出后检查 JSON 完整，
再导入 Perfetto 检查未闭合阶段；未完成阶段单列，不填入完整 duration 统计。不要用任务管理器强制结束来收尾。PID 可能复用，使用 run_id 和文件中的进程信息共同识别样本。

采样入口使用以下 PowerShell 命令：

```powershell
$runId = 'local-minimal-warm-001'
$sampleDir = New-Item -ItemType Directory -Force ".\samples\$runId"
$env:NYANPASU_TRACE_DIR = $sampleDir.FullName
& 'C:\path\to\clash-nyanpasu.exe'
# 等待 ready，然后从应用正常退出，最后打开该目录中的 trace JSON。
Remove-Item Env:NYANPASU_TRACE_DIR
```

## Perfetto 分析与统计

先在 Perfetto 中打开单次 trace，定位入口事件，查看到托盘、窗口创建、ready 的时间线。
按阶段名称前缀和时间范围检查大阶段的明细及并行工作。特别比较 `entry.migration_subprocess` 是否持续约 5 秒，
再用 ETW 检查迁移子进程活动和退出等待。

以下查询以一个文件为一个样本。Trace Processor 的 `ts` / `dur` 单位是 ns；
原始 JSON 的 `ts` 单位是微秒，不要直接套用相同换算。
先检查入口、里程碑、outcome 和收尾，再查询完整阶段：

```sql
WITH origin AS (SELECT ts FROM slice WHERE name = 'startup_entry')
SELECT name,
  EXTRACT_ARG(arg_set_id, 'args.milestone') AS milestone,
  EXTRACT_ARG(arg_set_id, 'args.outcome') AS outcome,
  EXTRACT_ARG(arg_set_id, 'args.pid') AS real_pid,
  EXTRACT_ARG(arg_set_id, 'args.debug_assertions') AS debug_assertions,
  (slice.ts - origin.ts) / 1e6 AS elapsed_ms
FROM slice CROSS JOIN origin
WHERE name IN ('startup_entry', 'startup.milestone', 'startup_outcome', 'capture_end')
ORDER BY slice.ts;
```

```sql
WITH origin AS (SELECT ts FROM slice WHERE name = 'startup_entry')
SELECT name, (slice.ts - origin.ts) / 1e6 AS start_ms,
  dur / 1e6 AS duration_ms
FROM slice CROSS JOIN origin
WHERE dur >= 0 AND (
  name GLOB 'entry.*' OR name GLOB 'tauri.*' OR name GLOB 'setup.*'
  OR name GLOB 'client.*' OR name GLOB 'resolve.*' OR name GLOB 'reconcile.*'
)
ORDER BY slice.ts;
```

未完成阶段单列；`observed_ms` 是截至收尾已观察到的耗时下界，不是完整 duration。
若缺少 `capture_end`，这个查询返回的下界为 NULL，样本收尾也不能视为已验证。

```sql
WITH origin AS (SELECT ts FROM slice WHERE name = 'startup_entry'),
  cutoff AS (SELECT MAX(ts) AS ts FROM slice WHERE name = 'capture_end')
SELECT name, (slice.ts - origin.ts) / 1e6 AS start_ms,
  (cutoff.ts - slice.ts) / 1e6 AS observed_ms
FROM slice CROSS JOIN origin CROSS JOIN cutoff
WHERE dur = -1 AND (
  name GLOB 'entry.*' OR name GLOB 'tauri.*' OR name GLOB 'setup.*'
  OR name GLOB 'client.*' OR name GLOB 'resolve.*' OR name GLOB 'reconcile.*'
)
ORDER BY slice.ts;
```

参数由 exporter 的 Debug visitor 写为字符串，包括 PID 和布尔值。
这些查询已用本仓库采集模块实际导出的验证文件在 Perfetto Trace Processor v58.2 执行：
完整阶段有非负 duration，保留引用的阶段为 `dur = -1`，里程碑参数可读取，
未出现 error / data_loss 导入统计。该文件仅验证导出协议，不是应用启动 benchmark。
参考 [Perfetto Trace Analysis](https://perfetto.dev/docs/analysis/getting-started)。

以场景、cache 组和版本分别统计每个阶段的 count / median / p95 / min / max。
p95 使用排序后的 nearest-rank：第 `ceil(0.95 * n)` 个样本。
缺失的阶段和里程碑保留为缺失，注明条件分支、失败或 trace 过滤原因，不填零。
有 fallback 或非 Ready outcome 的样本单列，仍报告其数量和原始延迟，不静默丢弃。

首先比较三个累计量：入口到 `tray_icon_created`、`main_window_created`、`main_window_ready`。
然后逐次计算 `main_window_ready - main_window_created` 和
`main_page_load_finished - main_page_load_started`；先算每次差值，再汇总，
不能用两个阶段的 p95 相减。
用 Perfetto 的单次时间线按前缀和时间范围检查最大阶段的明细；不累计重叠阶段。
同进程的 client/reconcile span 使用同一 trace clock，独立 root track 不表示串行关系。

## 第二轮：解释慢阶段

源码当前可见的候选如下；只有测量支持后才作为优化目标：

- **优先验证固定 5 秒等待**：`cmds::parse` 为所有子命令创建 `DelayedExitGuard`，
  `migrate::parse` 正常完成后返回，guard 的 Drop 固定 sleep 5 秒；GUI 的 `child.wait()` 等待它结束。
  先采集未优化 trace，再考虑只让正常迁移子命令跳过延迟退出，保留其他命令的行为。
  对照已迁移、首次安装、待迁移、迁移失败/备份失败场景，验证退出码、备份和数据库状态。
  不为绕过等待移除迁移或改变配置/数据库读取之前的迁移约束。
- `entry.migration_subprocess` 仍大：继续追查子进程加载、实际迁移检查和磁盘访问。
  之后 setup 还执行进程内迁移，应分别测量，不能只看父进程等待推断实际迁移成本。
- `setup.*_store`、`setup.web_storage` 或 `client.*` 大：检查数据库打开/恢复、配置反序列化、
  filesystem 和 actor pre_start；不要把持久化初始化盲目移到 UI 之后。
- `resolve.startup_reconcile` 大：先区分 service probe、runtime build、config check、core apply。
  当前 setup 同步等待这一过程后才请求主窗口；评估提前创建 UI 和后台 reconcile 时，
  还需要验证初始配置、托盘、错误展示、操作排队及 shutdown 的一致性。
- `resolve.tray_image_cache` 或 `resolve.main_window` 大：检查图标处理、native window/WebView2 创建。
- 创建到 ready 大：检查 WebView load、前端模块执行/路由、settings RPC 与 reveal。
  下一步在测试构建中使用前端 `performance.mark/measure` 拆分这些步骤；
  前端 clock 与 Rust clock 原点不同，只比较各自持续时间，避免直接相减。
- 正常首次启动出现很大的 `entry.single_instance`：检查已有 GUI 进程和锁状态。
  代码中的 1 秒重试仅在锁冲突时执行，不能直接解释正常冷启动的 5 秒。

`startup_entry` 不包含 Windows 进程创建、加载器、CLI 解析和采集初始化之前的工作。
另外用 Windows Performance Recorder / Analyzer 测进程启动及 CPU/磁盘活动，
覆盖 GUI、迁移子进程、核心和 WebView2 子进程。ETW 采样作为独立诊断组，
不要把带 recorder 的样本直接混入普通延迟基线。
在管理员终端、确认没有要保留的现有 recording 后：

```powershell
wpr -start GeneralProfile -filemode
# 启动目标 release 程序；收到 ready 或记录失败后结束这次 trace。
wpr -stop .\samples\startup.etl
```

WPA 中按 PID/进程树检查 Process Lifetimes、CPU Usage、Disk Usage / File I/O 和等待。
如果需要更细的 WebView/磁盘事件，再调整 profile，而不是假定 GeneralProfile 含所有事件。
命令与图表见 Microsoft 的
[WPR 命令参考](https://learn.microsoft.com/en-us/windows-hardware/test/wpt/wpr-command-line-options)和
[WPA 图表列表](https://learn.microsoft.com/en-us/windows-hardware/test/wpt/list-of-wpa-graphs)。
外部 launcher 的 Stopwatch 也可记录用户操作到就绪的延迟，但必须说明启动调用和信号传递开销；
不能把它与 Rust 入口时间混为同一指标。

## 优化验证与交付

选择基线中主要耗时阶段，每次只改变一个有证据的阻塞点。
用相同计时点、同机、同配置和同场景做 A/B；warm 组交替启动两个构建，
cold 组轮换构建顺序，每次从同样的配置/daemon 状态开始。
另用同一 release 构建的 trace 开启/关闭组做外部 launcher/ETW 对照，检查 exporter 本身的影响。

结果交付包括逐次原始 Chrome trace JSON、查询导出的 CSV、环境和配置清单、分场景统计、代表性的时间线、
慢样本 trace，以及正常/降级/回退/失败数量。提前约定目标延迟或最小改善幅度；
目前不凭源码承诺一个秒数。成功标准是同机目标组的托盘及正常 ready 延迟改善，
同时 profile 应用、服务模式、静默启动、deep link、托盘操作、失败处理和退出行为保持正确。
当前 Linux 的编译或单测通过不等于 issue 所述 Windows release 冷启动已优化或已复现。

实现验证覆盖：日志初始化前的阶段在 trace 中可见；异步等待及跨线程 span 有完整开始/结束；
错误提前返回仍结束 span；正常退出后 JSON 可导入 Perfetto；普通日志 reload、jobs capture 和轮转保持工作；
默认关闭时不创建 trace 文件。用不依赖 sleep 的确认信号测试跨线程与异步边界。
