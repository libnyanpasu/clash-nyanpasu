# 启动计时与 benchmark：issue #5338

[Issue #5338](https://github.com/libnyanpasu/clash-nyanpasu/issues/5338)
报告 Windows 11 25H2 x64 上运行程序后约 5 秒才出现托盘和窗口。
目前源码能确认同步等待的位置，不能确认这台机器上哪个阶段贡献最大。
本次改动只增加测量，保留原有初始化顺序。

## 已加入的测量

计时从 Rust `run()` 的入口开始，使用 `Instant`。日志初始化前完成的阶段先保留在内存，
随后写入现有 JSON 应用日志；因此早期记录的日志 `timestamp` 是补写时间，不能用它计算阶段耗时。
日志 target 为 `clash_nyanpasu::startup`，沿用应用日志级别；采样时设置 Info 或 Debug。

阶段记录包含 `fields.stage`、`fields.elapsed_ms`、`fields.pid`；入口和 setup 阶段还包含
相对同一入口的 `fields.start_ms` 和 `fields.end_ms`。内部构造和 reconcile 的明细只报告
自己的持续时间，不伪造进程入口偏移。阶段退出即记录，包括错误提前返回；
`startup stage finished` 不代表操作成功，要同时检查原有错误和 startup reconcile outcome。
日志初始化前进程退出的失败启动不会有完整的应用计时日志。

| 阶段 / 前缀                                               | 测量的工作                                                                                                                                                                                  |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `entry.commands_and_deep_link`                            | 命令解析、deep-link prepare，以及启用时的死锁检测线程启动                                                                                                                                   |
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
父阶段包含子阶段，并发后台任务也可能重叠，不能把所有 `elapsed_ms` 相加。

里程碑只记录每个进程的第一次事件，`fields.elapsed_ms` 均从 Rust 入口累计：

| `fields.milestone`                                  | 含义                                                                                                                          |
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

每次采样给一个外部 `run_id`，保存启动条件和该次所有日志片段到独立目录。
先确认 `debug_assertions=false`，并保存 startup outcome。
日志文件可能跨运行追加或因大小/日期轮转，不把“一个文件”默认等同于“一次启动”；
结合 PID、采样起止时间和最早的 `entry.commands_and_deep_link` 记录切分。
PID 也可能复用，不能单靠 PID 识别整个实验中的运行。

## 提取与统计

以下 PowerShell 片段把**已切分为一次运行**的 JSON 日志提取为 CSV。
用实际采样目录替换路径；保留 JSON 原件及条件清单。

```powershell
$runId = 'local-minimal-warm-001'
$files = Get-ChildItem '.\samples\local-minimal-warm-001\*.log'
$events = foreach ($file in $files) {
    foreach ($line in Get-Content $file.FullName) {
        $event = $line | ConvertFrom-Json
        if ($event.target -eq 'clash_nyanpasu::startup') { $event }
    }
}
$events | Where-Object { $_.fields.stage -or $_.fields.milestone } |
    ForEach-Object {
        [pscustomobject]@{
            run_id = $runId
            pid = $_.fields.pid
            kind = if ($_.fields.stage) { 'stage' } else { 'milestone' }
            name = if ($_.fields.stage) { $_.fields.stage } else { $_.fields.milestone }
            elapsed_ms = $_.fields.elapsed_ms
            start_ms = $_.fields.start_ms
            end_ms = $_.fields.end_ms
        }
    } | Export-Csv ".\samples\$runId.csv" -NoTypeInformation
```

以场景、cache 组和版本分别统计每个阶段的 count / median / p95 / min / max。
p95 使用排序后的 nearest-rank：第 `ceil(0.95 * n)` 个样本。
缺失的阶段和里程碑保留为缺失，注明条件分支、失败或日志过滤原因，不填零。
有 fallback 或非 Ready outcome 的样本单列，仍报告其数量和原始延迟，不静默丢弃。

首先比较三个累计量：入口到 `tray_icon_created`、`main_window_created`、`main_window_ready`。
然后逐次计算 `main_window_ready - main_window_created` 和
`main_page_load_finished - main_page_load_started`；先算每次差值，再汇总，
不能用两个阶段的 p95 相减。
绘制带 `start_ms/end_ms` 的单次时间线，再对最大父阶段向下展开明细；不累计重叠阶段。
没有入口偏移的 client/reconcile 明细按名称比较持续时间，通过所属父阶段定位。

## 第二轮：解释慢阶段

源码当前可见的候选如下；只有测量支持后才作为优化目标：

- `entry.migration_subprocess` 大：每次正常 GUI 启动都创建并等待 `migrate` 子进程，
  之后 setup 又执行进程内迁移。比较已迁移与待迁移组，追查子进程加载、迁移检查和磁盘访问。
  后续方案需保留迁移在配置/数据库读取之前执行的约束。
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

Rust 入口计时不包含 Windows 进程创建、加载器和入口前初始化。
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
另用父提交与计时构建做少量外部 launcher/ETW 对照，检查计时日志本身的影响。

结果交付包括逐次原始 JSON/CSV、环境和配置清单、分场景统计、代表性的时间线、
慢样本 trace，以及正常/降级/回退/失败数量。提前约定目标延迟或最小改善幅度；
目前不凭源码承诺一个秒数。成功标准是同机目标组的托盘及正常 ready 延迟改善，
同时 profile 应用、服务模式、静默启动、deep link、托盘操作、失败处理和退出行为保持正确。
当前 Linux 的编译或单测通过不等于 issue 所述 Windows release 冷启动已优化或已复现。
