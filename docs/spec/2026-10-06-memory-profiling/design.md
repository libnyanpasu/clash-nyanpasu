# 内存与启动开销剖析基础设施

**日期：** 2026-10-06

**状态：** 草案，待确认 §8 的决策项；未实施。

**调查基线：** `main@db8965898`（`docs: route agent rules to guides and set the core/GUI split goal (#5618)`）；实测对象为本机已安装的 Clash Nyanpasu 2.0.1（Windows 11，双 1920×1080 屏）。

**需求：** 应用稳定后主进程常驻约 50 MB，偏大。先建立能定位"是哪些组件占用"的剖析手段，按以下顺序推进：

1. 添加 `profiling` 构建 profile；
2. 加入 `tracing-chrome` feature，追踪 bootstrap 开销；
3. 后期如有必要，加入 `tracing-opentelemetry` 追踪 async 调用开销；
4. 加入 allocator 分析，覆盖 Windows / macOS / Linux 三个平台的分析手段。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec > 后续实施计划。

## 1. 决策与范围

1. 所有剖析能力都是**编译期开关**（cargo profile / feature），默认构建与发布产物不变；不新增运行时设置项，不进入 UI。
2. 剖析产物只写本地文件，不向任何外部服务发送。阶段 3 的 OTLP exporter 只允许指向 loopback。
3. 归因优先用**操作系统原生工具**（WPR/WPA、`malloc_history`、heaptrack）。它们无需改代码，且能看到非 Rust 分配（WebView2 宿主、输入法注入 DLL 等），而 §2.2 的实测表明这部分不可忽略。进程内 allocator 包装只负责回答"Rust 代码自己持有多少、在哪里分配"。
4. `#[global_allocator]` 是 Rust 语言强制的进程级 static，按 architecture-ledger 的 `external` 类别登记并写明理由；它只在剖析 feature 下编译。

**包含：** profiling profile、tracing-chrome feature 与 bootstrap span、三平台测量口径与归因流程、可选的进程内 allocator feature、开发文档。

**不包含：** 完整的自动化内存 bench（固定配置启动 GUI、按时间采样、CI 门禁，见 §7）、前端 WebView 进程内存、任何内存优化本身。

## 2. 已确认事实

### 2.1 仓库现状

| 事实                                                                                                                                                                                                                    | 位置                                                                         |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| release profile：`panic='unwind'`、`codegen-units=1`、`lto=true`、`opt-level='s'`，未设置 `debug`（即无调试信息）                                                                                                       | `backend/Cargo.toml` `[profile.release]`                                     |
| tauri crate 现有 features：`custom-protocol`、`default-meta`、`nesting`、`nightly`、`devtools`、`deadlock-detection`、`verge-dev`；无任何剖析 feature                                                                   | `backend/tauri/Cargo.toml` `[features]`                                      |
| 全局 subscriber 只在 `init::logging::init()` 里组装一次：`registry().with(file_layer.with_filter(filter)).with(jobs.layer())`，之后 `set_global_default`                                                                | `backend/tauri/src/utils/init/logging.rs:162-168`                            |
| `logging::init()` 在 `run()` 中位于命令行解析、单例检测、`run_pending_migrations()`、i18n 设定**之后**；这些早期阶段不会进入任何 tracing layer                                                                          | `backend/tauri/src/lib.rs:128-190`                                           |
| 应用自有 crate（tauri + nyanpasu-core）中 `#[instrument]` 只有约 22 处；bootstrap 阶段基本没有 span                                                                                                                     | `git grep -c instrument`                                                     |
| 退出走 `app.run(                                                                                                                                                                                                        | _, RunEvent::ExitRequested …                                                 | )`，经 `utils::exit` 收尾 | `backend/tauri/src/lib.rs:345`、`backend/tauri/src/utils/exit.rs` |
| 已有进程内存读取实现：geodata bench 的 `process_memory()` 覆盖 macOS（`proc_pid_rusage` 的 `phys_footprint`/`resident_size`）、Windows（private bytes / shared commit / working set / private WS）、Linux（anon / rss） | `backend/nyanpasu-geodata/examples/bench.rs`                                 |
| 已有计数 allocator 先例（仅限 example / 集成测试）：geodata bench 与 `tests/build_heap.rs` 各自定义 `Counting` 并设为 `#[global_allocator]`                                                                             | 同上，`backend/nyanpasu-geodata/tests/build_heap.rs`                         |
| 日志模块的内存 bench 只在 Linux 读 `/proc/self/status` 的 `VmRSS`/`VmHWM`                                                                                                                                               | `backend/tauri/src/core/logs/tests.rs:95`                                    |
| `backend/fake-core` 是确定性的假内核（TCP ready/release 屏障），可用于构造可复现的稳态                                                                                                                                  | `backend/fake-core/src/main.rs`                                              |
| architecture-ledger 对每个 `static` 计数，允许类别为 `immutable` / `external` / `test`                                                                                                                                  | `scripts/src/architecture-ledger/policy.ts:80-97`                            |
| 应用 manifest 未声明 `heapType`，Windows 上使用 NT heap 而非 segment heap                                                                                                                                               | `backend/tauri/build.rs:107-128`（自嵌 manifest）及对安装包 exe 的字符串检查 |
| 前端已有 vitest perf bench，测的是渲染耗时，不测 WebView 内存                                                                                                                                                           | `frontend/nyanpasu/perf/*.bench.test.tsx`                                    |

### 2.2 本机实测（安装版 2.0.1，窗口打开，PID 21412）

测量方法：`VirtualQueryEx` 遍历全部已提交区域，再用 `QueryWorkingSetEx` 逐页判定是否常驻、是否共享（脚本见附录 A）。

| 指标                                                      | 数值                 |
| --------------------------------------------------------- | -------------------- |
| Private Bytes / 私有工作集（任务管理器"内存"列） / 工作集 | 99.7 / 79.2 / 254 MB |
| Image（exe 与 DLL）：已提交 / 常驻 / 其中私有             | 199 / 170 / 2.3 MB   |
| Mapped（geodata mmap 等）：已提交 / 常驻 / 其中私有       | 29 / 6.9 / 0 MB      |
| Private：已提交 / 私有常驻                                | 95 / 76.9 MB         |
| 其中**单块 30.2 MB 分配**（全零、全部常驻）               | 30.2 MB              |
| 各 heap 段合计                                            | 约 40 MB             |
| 96 个线程栈                                               | 4.7 MB               |
| GDI / USER 对象（峰值 GDI）                               | 24 / 175（27）       |

结论（置信度见括号）：

- **30.2 MB 单块很可能属于 WeType 输入法的 TSF 注入组件，不是应用代码（中高）。** 依据：该块不是 NT heap 分配（起始处没有 `HEAP_VIRTUAL_ALLOC_ENTRY` 链表头），是直接 `VirtualAlloc` 得到的；整个进程只有一个指针指向它，指针所在对象的邻近内存里是 `wetype_tip_core.dll`、`d2d1.dll`、`input.dll` 的 vtable；该对象紧邻字段 `0x0F00`、`0x0810`，`3840×2064×4 = 31,703,040` 字节，与按两块 1920×1032 工作区换算的位图尺寸一致。尚未用分配调用栈确认，确认方法见 §6.1。扣除它后私有工作集约 49 MB，与"约 50 MB"一致。
- **托盘图标与代理组图标缓存可以排除（高）。** 托盘图标是 3 张约 8 KB 的内置 PNG，缩放结果缓存在磁盘，`set_icon` 时才解码；GDI 对象数稳定在 24，没有 HICON 泄漏；代理组图标缓存是磁盘上 98 个文件，共 1.1 MB，按请求读取，Rust 侧没有内存缓存。
- **geodata 不占私有内存（高）。** 索引通过 `memmap2::Mmap` 映射，属于 Mapped 类型。
- **剩余约 40 MB heap 尚未归属（未知）。** 同一进程里有多个 heap（Rust 的 `System` 使用进程默认 heap，COM / WebView2 / 输入法另有 heap），按 AllocationBase 分组无法区分。这正是本 spec 要补的能力。

### 2.3 外部工具与 crate（一手来源核实）

| 工具 / crate                              | 已核实要点                                                                                                                                                                                                                                                                                                                                                                     | 来源                                                                                                                                                                                                                                                                                |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Cargo 自定义 profile                      | `[profile.<name>] inherits = "release"`，通过 `--profile <name>` 选择，产物位于 `target/<name>/`；`debug = "line-tables-only"` 只生成回溯所需的文件/行号信息；profile 只能在 workspace 根 `Cargo.toml` 中定义                                                                                                                                                                  | [Cargo Book: Profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)                                                                                                                                                                                                     |
| `tracing-chrome` 0.7.2                    | `ChromeLayerBuilder::{file, writer, include_args, include_locations, trace_style, name_fn, category_fn, build}`；`build()` 返回 `(ChromeLayer, FlushGuard)`，guard drop 时停止写线程并 join；`TraceStyle::Threaded` 要求 span 在同一线程进出，`TraceStyle::Async` 按异步操作记录；输出可在 ui.perfetto.dev 打开                                                                | [docs.rs/tracing-chrome](https://docs.rs/tracing-chrome/latest/tracing_chrome/)、[ChromeLayerBuilder](https://docs.rs/tracing-chrome/latest/tracing_chrome/struct.ChromeLayerBuilder.html)、[TraceStyle](https://docs.rs/tracing-chrome/latest/tracing_chrome/enum.TraceStyle.html) |
| `tracing-opentelemetry`                   | 提供 `OpenTelemetryLayer`，与 `opentelemetry` 0.33 配套；`otel.*` 字段有保留含义；底层 opentelemetry crate 仍会有破坏性变更                                                                                                                                                                                                                                                    | [docs.rs/tracing-opentelemetry](https://docs.rs/tracing-opentelemetry/latest/tracing_opentelemetry/)                                                                                                                                                                                |
| `dhat` 0.3.3                              | 以包装系统 allocator 的 `dhat::Alloc` 作为 global allocator；`Profiler` drop 时写出 `dhat-heap.json`，用 DHAT viewer 查看；调用 `std::process::exit` 会跳过 drop，必须手动 drop；作者声明为实验性、维护优先级低；Windows 上回溯采集"可能慢得多"；建议 `debug = 1`                                                                                                              | [docs.rs/dhat](https://docs.rs/dhat/latest/dhat/)                                                                                                                                                                                                                                   |
| `tracy-client` 0.19.0 `ProfiledAllocator` | `ProfiledAllocator::new(System, callstack_depth)` 作为 global allocator；非零深度会采集调用栈，开销不小；首次分配即启动 client                                                                                                                                                                                                                                                 | [docs.rs/tracy-client ProfiledAllocator](https://docs.rs/tracy-client/latest/tracy_client/struct.ProfiledAllocator.html)                                                                                                                                                            |
| `tracing-tracy` 0.12.0                    | Tracy 的 tracing layer；不支持跨线程进出的 span；默认可能在局域网广播并暴露数据，应通过 `enable` feature 条件启用，并有 `only-localhost`、`ondemand` 等 feature                                                                                                                                                                                                                | [docs.rs/tracing-tracy](https://docs.rs/tracing-tracy/latest/tracing_tracy/)                                                                                                                                                                                                        |
| WPR（本机 `C:\WINDOWS\system32\wpr.exe`） | 内置 profile 有 `Heap`、`VirtualAllocation`、`HeapSnapshot`、`ResidentSet`、`ReferenceSet`、`Pool`；`wpr -heaptracingconfig <exe> enable` 为指定进程名启用 heap tracing（对之后启动的实例生效）；`wpr -snapshotconfig heap -pid <pid> enable` 可在**进程运行期间**启用快照，再用 `wpr -singlesnapshot heap <pid>` 或 `-enableperiodicsnapshot heap <秒> <pid>`（最小 5 s）采集 | 本机 `wpr -help heap`、`wpr -profiles`；[WPR Command-Line Options](https://learn.microsoft.com/en-us/windows-hardware/test/wpt/wpr-command-line-options)                                                                                                                            |
| macOS `footprint`                         | 默认输出与内核 `phys_footprint` 账本一致（Activity Monitor 的"内存"列也取自该账本）；`--sample <秒>` 定时采样，`-j` 输出 JSON；MALLOC_* 类别即 heap                                                                                                                                                                                                                            | [footprint(1)](https://keith.github.io/xcode-man-pages/footprint.1.html)                                                                                                                                                                                                            |
| macOS `vmmap`                             | `-summary` 汇总各区域与 malloc zone，%FRAG = 100 − 100 × Allocated / (Dirty + Swapped)；可读 `leaks` 生成的 memgraph 文件                                                                                                                                                                                                                                                      | [vmmap(1)](https://keith.github.io/xcode-man-pages/vmmap.1.html)                                                                                                                                                                                                                    |
| macOS `heap`                              | 列出当前 malloc 块，`-s` 按大小排序；进程以 MallocStackLogging 运行时会用分配调用者为"非对象"分配命名                                                                                                                                                                                                                                                                          | [heap(1)](https://keith.github.io/xcode-man-pages/heap.1.html)                                                                                                                                                                                                                      |
| macOS `malloc_history`                    | `-allBySize` / `-allByCount` / `-callTree` 按调用栈聚合当前存活的 malloc 与匿名 VM 分配；要求目标进程以 `MallocStackLogging=1`（或 `MallocStackLoggingNoCompact=1`）启动；`-highWaterMark` 需要 full 模式                                                                                                                                                                      | [malloc_history(1)](https://keith.github.io/xcode-man-pages/malloc_history.1.html)                                                                                                                                                                                                  |
| Linux heaptrack                           | 可启动时注入或 `--pid` 附加（附加需要 gdb）；`heaptrack_print` / `heaptrack_gui` 分析；Rust 需要调试符号，demangle 依赖可选的 `rustc_demangle` 库                                                                                                                                                                                                                              | [KDE/heaptrack README](https://github.com/KDE/heaptrack)                                                                                                                                                                                                                            |
| Linux bytehound                           | 通过 `LD_PRELOAD=libbytehound.so` 采集，自带 Web UI                                                                                                                                                                                                                                                                                                                            | [koute/bytehound README](https://github.com/koute/bytehound)                                                                                                                                                                                                                        |
| `jemalloc_pprof`                          | 仅支持 Linux；要求把 allocator 换成启用 `profiling` feature 的 `tikv-jemallocator`，输出 pprof 格式                                                                                                                                                                                                                                                                            | [docs.rs/jemalloc_pprof](https://docs.rs/jemalloc_pprof/latest/jemalloc_pprof/)                                                                                                                                                                                                     |

**未核实（实施前需验证）：**

- MSVC 下 `debug = "line-tables-only"` 能否生成可被 WPA 解析出 Rust 函数与行号的 PDB（预期可以，未实测）。
- tauri `app.run` 退出时是否经 `std::process::exit` 结束进程，导致 `run()` 内局部变量（包括 `FlushGuard`）不被 drop。§4 的设计按"不会 drop"处理，结论无论哪种都安全。
- 以 `--profile profiling` 构建时，tauri-build 是否像 `target/debug/` 那样把 sidecar 复制到 `target/profiling/`。
- `xctrace record --template Allocations` 的确切参数（检索结果互相矛盾、无来源，未采用）。
- 启用 hardened runtime 的已签名 macOS 应用能否被 `malloc_history` / `heap` 附加。本地 `cargo build` 产物未签名，预计不受影响。
- bytehound 当前的维护状态；glibc 多 arena 对 RSS 的具体影响（`MALLOC_ARENA_MAX`）。检索结果无来源，未采用。

## 3. 阶段 1：`profiling` profile

在 `backend/Cargo.toml` 中添加：

```toml
[profile.profiling]
inherits = 'release'
debug = 'line-tables-only'
```

- 继承 release 的 `lto`、`codegen-units = 1`、`opt-level = 's'`，测到的就是发布产物的代码与布局；只额外生成行号表，供回溯符号化。
- 不改 `strip`：release 默认 `"none"`。
- 构建入口：先确认 `backend/tauri/tmp/dist` 存在（`pnpm web:build`），再执行 `cargo build --profile profiling -p clash-nyanpasu`。在 `deno.jsonc` 中登记一个具名 task（如 `build:profiling`），外部调用一律走 task。
- 产物不打包、不签名、不发布。

**验证：** 产物位于 `target/profiling/`；Windows 上同目录有 PDB，WPA 能把 heap 调用栈解析到 `clash_nyanpasu::…`；与同一 commit 的 release 构建相比，稳态私有工作集差异在测量噪声内（只多了调试信息，不应改变运行时内存）。

## 4. 阶段 2：`tracing-chrome` feature

### 4.1 接入

- `backend/tauri/Cargo.toml` 新增可选依赖 `tracing-chrome = { version = '0.7', optional = true }` 和 feature `trace-chrome = ['dep:tracing-chrome']`。
- 在 `logging::init()` 组装 registry 时，`cfg(feature = "trace-chrome")` 下额外 `.with(chrome_layer.with_filter(..))`。layer 使用独立的 per-layer filter，**不受**用户日志级别 reload 影响：bootstrap 期间用户级别尚未加载，且剖析需要 trace 级 span。
- 输出路径：feature 开启时写入 `<app logs>/trace-<启动时间>.json`。不新增环境变量开关：编译了 feature 就意味着要剖析。
- `TraceStyle::Async`：actor 启动与 `.instrument()` 的 future 会在 tokio worker 之间迁移，`Threaded` 会产生无效轨迹（文档要求同线程进出）。
- `include_args(true)`：span 字段（profile 名、actor 名）对归因有用；体积在 bootstrap 范围内可以接受。

### 4.2 `FlushGuard` 的生命周期

`FlushGuard` drop 时才 join 写线程。`logging::init()` 把它与 reload sender 一起返回；由组合根持有，在 `utils::exit` 的收尾路径（现有关闭流程完成之后、进程退出之前）显式 drop。不能交给日志 reload 线程持有：那个线程最终 `park()` 永不返回，guard 永远不会 drop。不新增 static，也不新增全局 shutdown 阶段。

### 4.3 bootstrap span

现有 span 不足以覆盖启动过程（§2.1）。只在以下阶段添加 `info_span!`，每个阶段一个，不做细粒度插桩：

1. `logging::init` 之后到 `tauri::Builder` 构建完成（配置加载 `init_config`、specta transport 构建）；
2. `setup::setup` 整体，以及其中组合根构建 `NyanpasuClient`、各 actor spawn 的阶段边界；
3. `resolve::resolve_setup`、首个窗口创建 `resolve::create_window`；
4. 内核首次启动到 ready（已有的 core lifecycle 路径上，若已有 span 则复用）。

`logging::init()` 之前的阶段（命令行解析、单例检测、迁移）不进入 tracing。`run()` 入口记一个 `Instant`，logging 初始化后发一条 `pre_logging_ms` 字段的 event 作为补偿；不为此提前初始化 logging（会改变迁移阶段的日志行为，超出范围）。

**验证：** 用 `cargo build --profile profiling --features trace-chrome` 构建并启动，到 dashboard 可交互后正常退出，生成的 JSON 能在 ui.perfetto.dev 打开，§4.3 各阶段可见且时序正确；不开 feature 时 `cargo tree -e features` 不含 tracing-chrome。

## 5. 阶段 3（可选）：`tracing-opentelemetry`

只在阶段 2 无法解释 async 调用开销时启动。届时：

- feature `trace-otel`，引入 `tracing-opentelemetry` 与 `opentelemetry` / OTLP exporter（版本按届时 docs.rs 固定）；exporter 只允许 loopback endpoint，本地运行 Jaeger 或 Tempo 等 collector。
- 与 `trace-chrome` 互不依赖，可同时开启。
- 本阶段的设计（span 粒度、采样、跨 RPC 的 context 传播）届时另写 spec。这里只记录：opentelemetry crate 仍有破坏性变更（见 §2.3），引入前需评估升级成本；若只关心 tokio 任务调度，tokio-console 可能是更轻的替代（未核实其当前要求，届时再查）。

## 6. 阶段 4：allocator 分析

分两层，**先用第一层**：

- **第一层（OS 原生，零代码）：** 只依赖阶段 1 的 profiling 构建，三平台各有一套固定流程（§6.1–§6.3）。能看到全部分配者，包括非 Rust 代码。
- **第二层（进程内 feature）：** 只回答"Rust 代码持有多少、分配点在哪里"，跨平台输出同一格式（§6.4）。

### 6.0 统一测量口径

| 平台    | 主指标（与系统 UI 一致）                     | 辅助指标                                                                 | 现有实现                                |
| ------- | -------------------------------------------- | ------------------------------------------------------------------------ | --------------------------------------- |
| Windows | 私有工作集（任务管理器"内存"列）             | Private Bytes、按 Image / Mapped / Private 分类的常驻量、GDI/USER 对象数 | geodata `process_memory()`；附录 A 脚本 |
| macOS   | `phys_footprint`（Activity Monitor"内存"列） | `resident_size`、`footprint` 的分类结果                                  | geodata `process_memory()`              |
| Linux   | `RssAnon`（或 `smaps_rollup` 的 `Pss_Anon`） | `VmRSS`、`VmHWM`、`Private_Dirty`                                        | logs bench 读 `/proc/self/status`       |

测量条件必须固定并记录：窗口打开或关闭、运行时长、内核是否运行（真实内核或 fake-core）、**第三方输入法 / 注入 DLL**（Windows 上以加载模块列表为证据）。§2.2 的 30 MB 就来自这一类变量。

### 6.1 Windows

1. **非 heap 大块与第三方注入（先做）**：`wpr -start VirtualAllocation -filemode`，启动应用，操作到稳态，`wpr -stop va.etl`，在 WPA 的 VirtualAlloc 表中按调用栈与模块查看。这一步用于确认 §2.2 的 30 MB 块属于 WeType；对照实验是切换到微软拼音后重测。
2. **启动全程 heap 归因**：`wpr -heaptracingconfig clash-nyanpasu.exe enable`（之后启动的实例生效），`wpr -start Heap -filemode`，启动并到稳态，`wpr -stop heap.etl`，在 WPA 的 Heap 表中按"Outstanding"（存活）分配的调用栈聚合；用完执行 `wpr -heaptracingconfig clash-nyanpasu.exe disable`。heap tracing 开销大，只用于剖析构建。
3. **稳态快照（不重启）**：`wpr -snapshotconfig heap -pid <pid> enable` 后用 `wpr -singlesnapshot heap <pid>`，或用 `-enableperiodicsnapshot heap 60 <pid>` 按间隔取多个快照对比增长（Profile `HeapSnapshot`）。适合观察长期运行的增长；只覆盖启用之后的分配。
4. **区域分类**：附录 A 的 `VirtualQueryEx` + `QueryWorkingSetEx` 脚本，或 Sysinternals VMMap（本机未安装）。
5. **符号**：WPA 指向 `target/profiling/` 下的 PDB；系统 DLL 用微软符号服务器。

已知限制：Rust 的 `System` 与 COM、WebView2 共用进程默认 heap，必须靠调用栈区分，不能按 heap 区分；直接 `VirtualAlloc` 的分配只出现在 VirtualAllocation 表，不在 Heap 表里。

### 6.2 macOS

Rust 的 `System` allocator 在 macOS 上就是 libmalloc，以下工具都能直接看到 Rust 分配：

1. **总量与分类**：`footprint <pid>`（默认按 Dirty 排序；`--sample 5 -j out.json` 定时采样），`vmmap -summary <pid>` 查看 malloc zone 与 %FRAG。
2. **存活分配按调用栈聚合**：以 `MallocStackLogging=1` 启动 profiling 产物，到稳态后执行 `malloc_history <pid> -allBySize`（或 `-callTree`）；`heap <pid> -s` 按块大小排序。需要启动全程的峰值时用 full 模式加 `-highWaterMark`。
3. **图形化**：Instruments 的 Allocations + VM Tracker 模板。
4. **符号**：`split-debuginfo` 在 macOS 上默认为 `unpacked`，调试信息留在 `target/` 下的目标文件中，本机分析可以直接符号化；若要把产物拷到别处分析，需要 `dsymutil` 生成 dSYM（实施时验证）。

### 6.3 Linux

1. **总量**：`/proc/<pid>/smaps_rollup`、`/proc/<pid>/status`。
2. **归因**：heaptrack（启动注入 `heaptrack ./target/profiling/clash-nyanpasu`，或用 `heaptrack --pid` 附加），`heaptrack_print` 输出峰值消费者与存活分配。bytehound（`LD_PRELOAD`）作为备选。
3. **不采用**：`jemalloc_pprof` 要求替换 allocator，测到的是另一个 allocator 的行为，与发布产物不可比；Valgrind 太慢，不适合 GUI 加内核的完整启动。

### 6.4 进程内 allocator feature（第二层）

用途：三平台统一产出"Rust 分配点 → 存活字节"，并为 §7 的 bench 提供进程内计数。候选：

| 方案                                                        | 产出                                                          | 优点                                                                             | 缺点                                                                                                                         |
| ----------------------------------------------------------- | ------------------------------------------------------------- | -------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| **A. `dhat-heap` feature（推荐）**                          | 退出时写 `dhat-heap.json`，含 t-gmax / t-end 存活分配及调用栈 | 单一依赖，产物是文件，不开网络端口；可同时用于 heap 用量测试（`HeapStats::get`） | 作者声明实验性；Windows 上回溯采集明显更慢；只在 `Profiler` drop 时写出，需要像 §4.2 的 `FlushGuard` 一样在退出路径显式 drop |
| B. `tracy` feature（`tracing-tracy` + `ProfiledAllocator`） | Tracy GUI 实时显示内存曲线与分配栈，并同时承载 span           | 实时；一套工具同时覆盖阶段 2 和阶段 4                                            | 与阶段 2 的 tracing-chrome 职责重叠；需要 Tracy server；默认网络暴露，必须开 `only-localhost`；不支持跨线程 span             |
| C. 自写计数 allocator                                       | 只有总字节数与峰值                                            | 零依赖，开销最低；仓库已有两处先例                                               | 没有调用栈，无法归因到组件                                                                                                   |

`#[global_allocator]` 放在 `backend/tauri/src/main.rs`（binary crate），`cfg(feature = ...)` 守卫，并在 architecture-ledger 的 static allowlist 中以 `external` 类别登记（理由："Rust 语言要求 global allocator 是 static；只在剖析 feature 下编译"）。

**验证：** 开启 feature 后启动到稳态再正常退出，产物能被对应 viewer 打开，且包含 `clash_nyanpasu` 帧；不开 feature 时 ledger 计数不变。

## 7. 后续：内存 bench（不在本 spec 范围）

阶段 1 至 4 完成后，再另起 spec 设计可复现的 bench：

- 以 `fake-core` 构造确定性稳态（固定 profile、无真实网络）；
- 把 geodata 的 `process_memory()` 提升为可复用的实现（工具 crate 或 bin）；外部采样由 `deno task` 驱动，遵守 scripts 规范；
- 固定 §6.0 的测量条件，在 Windows Sandbox 或干净 VM 中排除第三方注入；
- 产出与基线对比的报告；是否接入 CI 届时再定。

## 8. 待确认的决策

1. 阶段 4 第二层选 A（dhat，推荐）、B（tracy），还是暂不做、只用 OS 原生工具？
2. 阶段 2 的 trace 文件放 `<app logs>/`（推荐：与应用日志一起被现有清理策略覆盖），还是工作目录？
3. 阶段 1 的 profiling 构建是否需要 CI 产物（推荐：不需要，本地构建即可）？

## 9. 实施顺序与检查

```text
1. profiling profile + deno task      -> verify: target/profiling/ 产物 + WPA 符号化 Rust 帧
2. trace-chrome feature + bootstrap span -> verify: Perfetto 中 §4.3 各阶段可见；默认构建依赖树不变
3. Windows §6.1 第 1、2 步实测          -> verify: 30 MB 块归属确认；40 MB heap 按调用栈归属到组件
4. 第二层 allocator feature（按 §8.1）   -> verify: viewer 可读；ledger 已登记
5. docs/development/testing.md 增补剖析章节 -> verify: 与本 spec 一致
```

macOS / Linux 的 §6.2、§6.3 流程至少各实测一次再写入开发文档；没有对应平台时在文档中标注"未实测"。

## 附录 A：Windows 区域分类脚本要点

PowerShell 中通过 `Add-Type` 调用：

1. `OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ)`；
2. `VirtualQueryEx` 遍历，筛选 `State == MEM_COMMIT`，按 `Type`（`MEM_IMAGE` / `MEM_MAPPED` / `MEM_PRIVATE`）和 `AllocationBase` 分组；
3. 对每页调用 `QueryWorkingSetEx`：`VirtualAttributes` 的 bit 0 表示是否常驻（Valid），bit 15 表示是否共享（Shared）；
4. 带 `PAGE_GUARD` 的私有分配近似为线程栈；
5. 对可疑大块用 `ReadProcessMemory` 扫描内容，并在全部可读区域中搜索指向该块的指针，再把邻近值与模块地址范围比对，定位所属模块（§2.2 即用此法）。

脚本在本次调查的 scratchpad 中，实施 §7 时按 scripts 规范重写，不直接入库。
