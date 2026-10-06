# Windows 内存剖析报告

**日期：** 2026-10-06

**基线：** `main@36c667d97`（含 #5622 的 `profiling` profile 与 #5628 的剖析 feature）。

**依据：** [内存与启动开销剖析 spec](../spec/2026-10-06-memory-profiling/design.md) §6.1 的 Windows 流程。

**结论摘要：**

- 窗口打开时多出的约 30 MB 是 tauri 为**透明窗口**重绘背景用的 softbuffer 位图，大小等于窗口物理像素 × 4 字节。spec §2.2 把它归给 WeType 输入法，这个结论是错的。
- 窗口关闭后，私有内存几乎全部在进程默认堆里；其中九成是应用自己的 Rust 分配。WebView2、COM 与系统库合计不到 2 MiB。
- 堆中最大的几项是：运行时配置的多份全量拷贝（4.8 MiB，随订阅规模增长）、日志 appender 预分配的通道（3.9 MiB）、代理缓存中为比较变化而保留的整份 JSON（2 MiB）。

## 1. 测量条件

| 项目     | 取值                                                                                                                                                                                                        |
| -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 系统     | Windows 11 Pro 10.0.26300，32 个逻辑处理器                                                                                                                                                                  |
| 构建     | `pnpm build:profiling --features verge-dev`，不开剖析 feature；用 dev 身份与已安装实例隔离（独立数据目录与单实例锁）                                                                                        |
| 配置     | 本机 dev 配置：service mode 关、系统代理关、mixed-port 10808、本地 mihomo-alpha                                                                                                                             |
| 流程     | 启动 → 窗口打开 90 s 取样 → 向主窗口发 `WM_CLOSE`（进托盘）→ 90 s 后取样 → 向关机钩子窗口发 `WM_QUERYENDSESSION` 正常退出                                                                                   |
| 对照     | 已安装的 2.0.x 实例（正式配置，窗口关闭，运行 4.2 h），只读取样                                                                                                                                             |
| 取样工具 | `VirtualQueryEx` + `QueryWorkingSetEx` 区域分类；读取 `_HEAP_SEGMENT` 头（`+0x10` 签名 `0xFFEEFFEE`，`+0x28` 所属 heap）把私有区域归到具体 heap；Toolhelp heap 列表                                         |
| 归因工具 | WPR `Heap` + `VirtualAllocation`（`wpr -heaptracingconfig` 只对剖析期间新启动的 `Clash Nyanpasu.exe` 生效，结束后关闭）；`xperf -a heap -stacks/-images`；`xperf -a dumper` 加 `llvm-symbolizer` 解析调用栈 |

各工具脚本只在本次调查的 scratchpad 中，没有入库；按 spec §7 做 bench 时再按 scripts 规范重写。

## 2. 进程内存组成

| 状态                      | 私有工作集 | 进程默认堆 | softbuffer 位图 | 日志通道（独立 VirtualAlloc 块） | 线程栈 | Image 私有页 |
| ------------------------- | ---------- | ---------- | --------------- | -------------------------------- | ------ | ------------ |
| dev 构建，窗口打开，91 s  | 67.9 MB    | 26.0 MB    | 30.2 MB         | 3.9 MB                           | 3.5 MB | 1.7 MB       |
| dev 构建，仅托盘，183 s   | 41.1 MB    | 29.4 MB    | —               | 3.9 MB                           | 3.5 MB | 1.8 MB       |
| 已安装实例，仅托盘，4.2 h | 48.1 MB    | 45.3 MB    | —               | （计入堆外私有 2.8 MB）          | 1.2 MB | 1.0 MB       |

- 进程共有 5 个 heap，除默认堆外合计不到 0.2 MB。默认堆同时承载 Rust 的 `System` allocator、COM 与 WebView2 宿主等系统组件，只能靠调用栈区分（§3）。
- WPR heap tracing 下复现了同样的组成：窗口打开时 Private Bytes 69.7 MB，30.23 MB 的块再次出现，进托盘后消失。

## 3. 默认堆的内容

在 WPR 停止时刻（仅托盘），默认堆中存活 **99,355 块，20.3 MiB**，驻留 23.7 MB，即堆本身开销约 3 MB。剔除 ntdll / KERNELBASE / ucrtbase 的分配帧后，按调用栈顶端所在模块聚合：

| 模块                          | 存活     |
| ----------------------------- | -------- |
| `Clash Nyanpasu.exe`          | 18.2 MB  |
| `opengl32.dll`                | 0.9 MB   |
| `ntdll.dll`（加载器、TLS 等） | 0.7 MB   |
| `crypt32.dll`（系统证书库）   | 0.6 MB   |
| 其他（COM、UIA、WebView2 等） | < 0.5 MB |

再把每个调用栈归到最靠近栈顶的应用 crate 帧（`clash_nyanpasu_lib`、`nyanpasu_*`、`clash_api`），模块路径取到第三层：

| 组件                                                                                         | MiB  | 根因                                                                              |
| -------------------------------------------------------------------------------------------- | ---- | --------------------------------------------------------------------------------- |
| 运行时配置快照（`nyanpasu_config::runtime::snapshot`、`client::runtime`、core-manager 三处） | 4.83 | §4.2                                                                              |
| `utils::init`（日志）                                                                        | 3.94 | §4.3                                                                              |
| 代理缓存（`clash_api::api::proxies`、`core::proxies`）                                       | 3.56 | §4.4                                                                              |
| `core::logs`                                                                                 | 0.89 | zstd 压缩上下文（`ZSTD_createCDict`，0.39 MiB）与 redb 页缓存                     |
| `opengl32.dll`                                                                               | 0.88 | `GLInitializeThread`：opengl32 被静态导入，每个线程 attach 时分配约 10 KB（§4.5） |
| `core::clash`                                                                                | 0.72 | websocket 流的握手与读缓冲                                                        |
| `run::inner`（tauri 内部，已内联）                                                           | 0.71 | 其中 0.18 MiB 是 `glob::Pattern`（能力 scope）                                    |
| `core::tray`                                                                                 | 0.68 | muda 菜单项（代理组的 check item）                                                |
| `client::app_update`                                                                         | 0.63 | 主要是 crypt32 打开系统证书库（`CertOpenStore`），归因到首次发起 TLS 的调用方     |
| 其余                                                                                         | ~3.5 | 单项均不超过 0.5 MiB                                                              |

dhat（#5628 的 `dhat-heap`，同一 dev 配置，窗口打开约 70 s 后退出）给出的 Rust 堆 t-end 为 16.3 MiB、峰值 26.3 MiB，与上表的 Rust 部分一致。

## 4. 根因

### 4.1 透明窗口的 softbuffer 位图（窗口打开时 30.2 MB）

VirtualAllocation 跟踪中，这个块（`RESERVE COMMIT`，`0x1E3C000` 字节）的分配调用栈是：

```text
win32k / gdi32full!…                      ← CreateDIBSection
softbuffer::backends::win32::Buffer::new  (softbuffer-0.4.8/src/backends/win32.rs:100)
tauri_runtime_wry::handle_event_loop      (tauri-runtime-wry-2.12.1/src/lib.rs:4144)
tao …::public_window_callback_inner       ← WM_PAINT → RedrawRequested
wry::webview2::InnerWebView::parent_subclass_proc
tauri_runtime_wry::undecorated_resizing::windows::subclass_parent
```

tauri-runtime-wry 在 Windows 上对 `is_window_transparent` 的窗口持有一个 softbuffer surface，每次 `RedrawRequested` 都按 `inner_size()` 调整 surface 并填充背景色（`src/window/windows.rs:46` 的 `draw_surface`）。softbuffer 的 Win32 后端为此创建一个窗口大小的 32 位 DIB section。本机窗口的物理尺寸是 3840×2064，3840 × 2064 × 4 = 31,703,040 字节，与块的大小一致。窗口销毁后，surface 随之释放。

我们在 `backend/tauri/src/window.rs:542` 对 Windows 无条件设置 `.transparent(true)`。这个设置至少在 2023 年的代码里就已存在（`git log -S` 最早追到 11ae5ef4a，那次只是移动文件），经 v2 迁移（#1510）和窗口重构（#3982）一直保留，git 历史没有记录原因。

**更正 spec §2.2：** 当时把这个块归给 WeType 输入法，依据只是指向该块的对象附近有 `wetype_tip_core.dll` 的 vtable，没有分配栈。本次有几轮运行 WeType 根本没有加载（模块列表里没有 `wetype_tip*.dll`），这个块照样出现，分配栈也确认了它来自 softbuffer。WeType 加载后会带来约 10 个线程，内存影响很小。

### 4.2 运行时配置的多份全量拷贝（4.83 MiB）

- `ConfigSnapshotsGraph` 的每个节点都持有一份完整配置的 `serde_json::Value`，而不是相对父节点的差异。pipeline 的每一步各一个节点，`with_effective_config` 还会追加内核回读的 effective 配置作为一个节点（`backend/tauri/src/client/runtime_inspection.rs:100`）。
- `append_transition` 在校验前先 `self.clone()` 整个图（`backend/nyanpasu-config/src/runtime/snapshot.rs:293`）；`with_effective_config` 和 `without_effective_config` 也会先深拷贝整个 inspection 再修改（`runtime_inspection.rs:76`、`:137`）。
- `RuntimeSnapshot` 本身还持有产物字节 `product_bytes`、解析后的 `config: Mapping` 与 effective 配置（`backend/tauri/src/client/runtime.rs:73-77`）。core-manager 在 `prepare_full`、`switching::prepare_launch`、`publish_instance` 中各保留一份（各约 0.45 MiB）。

本次的 dev 配置约 0.45 MiB 一份，合计约十份。总量与订阅规模成正比，规则多的订阅会成倍放大。已安装实例的 45 MB 堆有可能就来自这里，但它的配置目录没有找到，未能确认（§6）。

### 4.3 日志 appender 的预分配通道（3.94 MiB）

`tracing_appender::non_blocking(writer)`（`backend/tauri/src/utils/init/logging.rs:61`）使用默认的 `DEFAULT_BUFFERED_LINES_LIMIT = 128_000`，底层的 crossbeam 有界通道在创建时就分配全部槽位：128,000 × 32 字节 ≈ 3.9 MiB。这块内存常驻，与实际写入量无关。dhat 与 WPR 都把它归到 `get_file_appender`。

### 4.4 代理缓存的 fingerprint（2.05 MiB）

`core/proxies.rs` 的 `Snapshot.fingerprint`（`backend/tauri/src/core/proxies.rs:24`、`:114`）是 `serde_json::to_vec(&(&proxies, &providers))` 的完整结果，只用来和上一份比较是否变化（`:133`）。缓冲区按倍增扩容，实测为一块 2.00 MiB 的分配，随 Snapshot 常驻。`Proxies` 与 providers 本身另占约 1.5 MiB。

### 4.5 线程数（dev 构建 101 个线程）

- 按 Win32 起始地址分组：79 个是 Rust `std::thread`，其余来自 WeType（10 个）、ntdll 线程池、WebView2 宿主等。
- 进程里有两个独立的 tokio 多线程 runtime：tauri 的 `async_runtime::default_runtime`，以及 `nyanpasu_utils::runtime::get_runtime_handle` 在非 tokio 线程上调用时创建的 `Runtime::new()`（`backend/nyanpasu-runtime/crates/nyanpasu-utils/src/runtime/mod.rs:11`、`:18`）。两者在 WPR 的堆栈中都有存活分配。tokio 默认每个 runtime 的 worker 数等于逻辑 CPU 数，本机为 32。"两个 runtime 各 32 个 worker"是由此推断的，没有逐个线程核对。
- 每个线程的代价：栈驻留，合计 3.5 MB；opengl32 的 per-thread 数据，合计 0.9 MB；tokio worker 本身的结构。opengl32 由 `nyanpasu-egui` 的统计小组件静态导入，主进程并不渲染 OpenGL，却在每个线程上都付出这份开销。

## 5. 优化方向

按收益与改动量排序。以下都只是建议，本报告没有改任何代码。

| #   | 方向                                                                                | 预计收益                                    | 改动量 | 备注                                                      |
| --- | ----------------------------------------------------------------------------------- | ------------------------------------------- | ------ | --------------------------------------------------------- |
| 1   | Windows 主窗口不再设置 `transparent(true)`                                          | 窗口打开时 −30 MB（随窗口尺寸变化）         | 一行   | 需要确认圆角、阴影与背景的视觉效果不变；属于 UI 决策      |
| 2   | `NonBlockingBuilder::default().buffered_lines_limit(..)`，例如 4096                 | −3.8 MiB                                    | 几行   | 需要确定日志高峰时丢行（`lossy`）的取舍                   |
| 3   | 代理 fingerprint 改为哈希（或流式写入 hasher）                                      | −2 MiB，每次刷新也少一次整份序列化          | 小     |                                                           |
| 4   | 合并两个 tokio runtime，并限制 worker 数                                            | 线程减少数十个，约 −3 MB（栈 + per-thread） | 中     | 涉及 runtime submodule 的 `nyanpasu-utils`                |
| 5   | 配置快照图改存差异或用 `Arc` 共享节点，避免整图深拷贝，减少 core-manager 的冗余副本 | 本配置约 −3 MiB；大订阅收益成倍             | 大     | 结构性改动，需要单独 spec；inspection UI 依赖现有数据形状 |
| 6   | opengl32 改为延迟加载，或只在小组件进程中加载                                       | −0.9 MB                                     | 中     | 需要确认 egui/glow 的链接方式                             |

## 6. 未解决的问题

- **已安装实例的堆比 dev 构建大约 16 MB**（45.3 MB，运行 4.2 h；dev 构建为 24–29 MB，运行 3 min）。两者配置不同，运行时长也不同。已安装实例的配置目录不在默认位置（`%APPDATA%\Clash Nyanpasu` 不存在，推测使用了自定义 home 目录），无法比较订阅规模。下一步是让 dev 构建长时间运行（例如 1 h），用 `wpr -snapshotconfig heap -pid <pid> enable` 配合 `-enableperiodicsnapshot` 取多个堆快照，对比增长最多的调用栈，区分"随配置规模"与"随时间增长"两种情况。
- spec §6.1 的 WPA 图形界面流程没有走：本次用 `xperf -a heap` 与 `xperf -a dumper`（不带 `-symbols`，再用 `llvm-symbolizer` 解析）替代。`xperf -symbols -a dumper` 会为每个栈帧解析符号，在 0.85 GB 的跟踪上数小时都无法完成；`wpaexporter` 搭配内置 profile 时，因为部分表没有数据，导出直接失败。
- macOS、Linux 未测。
