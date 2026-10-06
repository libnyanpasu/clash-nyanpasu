# 窗口管理层：由后端掌管窗口生命周期与显示时机

**日期：** 2026-10-06

**状态：** 设计已确认（2026-10-06，§8 全部决策已定），按本 spec 实施。

**基线：** main `e2a5e45a9`；Tauri 2.12.1，tauri-runtime-wry 2.12.1。

**需求：**

- 去掉 Windows 窗口的 `transparent(true)`。它让每个打开的主窗口多占约 30 MB，见[内存剖析报告](../../review/2026-10-06-windows-memory-profile.md) §4.1。
- 不再用 ready event。仿照 Tauri splashscreen 示例，前端通过 invoke 通知后端，由后端显示窗口。
- 每个窗口的生命周期都由窗口管理层负责，包括前端 ready 后才显示窗口。
- macOS 同样使用这套机制。
- 支持窗口关闭模式：销毁或隐藏。每个窗口可以单独设定，也可以继承全局设定。
- 架构上为将来切换到 Tauri 3 的 CEF runtime 做准备，但本次不切换。

**依据：**

- [Tauri splashscreen 示例](https://v2.tauri.app/learn/splashscreen/)（2026-10-06 抓取）。
- Tauri 2.12.1 `src/webview/webview_window.rs`：`build` 的已知问题注释（:56-59、:113-116）、`transparent`（:1146）、`background_color`（:1248）。
- tauri-runtime-wry 2.12.1 `src/lib.rs` 的 `is_window_transparent` 分支（:4308、:4470）。
- [tauri v3.0.0-alpha.0 发布说明](https://github.com/tauri-apps/tauri/releases/tag/tauri-v3.0.0-alpha.0)（#15985，2026-10-06 抓取）。
- [docs.rs `tauri-runtime-cef` 3.0.0-alpha.5](https://docs.rs/tauri-runtime-cef/latest/tauri_runtime_cef/)，以及其中的 `WebviewWindowBuilderCefExt`（2026-10-06 抓取）。
- 本 spec §2 引用的仓库代码与提交。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec。

## 1. 决策与范围

1. 新增 `WindowManager`，取代 `WindowRegistry`，同时负责窗口登记、创建、显示、ready 和销毁。窗口的 Tauri 操作只由它发起。它属于 GUI crate 的 adapter，用一把锁保护窗口表，不是 actor，也不进入 `NyanpasuClient`。
2. 所有窗口都以隐藏状态创建。前端准备好后调用新的 RPC mutation `report_window_ready`，后端据此显示窗口。删除 `WindowReadyEvent` 和 `WindowConfig.visible_on_create`；前端不再自己调用 `show`、`unminimize` 和 `setFocus`。
3. 窗口未 ready 时，打开请求只记下“ready 后显示”，不提前显示白屏或空窗口。窗口已 ready 时直接显示。
4. 兜底计时器从“只在启动时、只管主窗口”改为每个新建窗口一份：10 秒内没有 ready 就强制显示并记录警告。ready 或销毁时取消计时器。这一条需要审核确认，见 §8-1。
5. 去掉 Windows 的 `.transparent(true)`，同时删除只有 Linux 读取、却没有任何窗口设置过的 `WindowConfig.transparent`。macOS 本来就不透明，不变。
6. macOS 也走这套流程。Dock 图标的显示与隐藏，从 `resolve_setup` 的 ready 监听和 `lib.rs` 的 `CloseRequested` 分支移到 `WindowManager` 的显示与关闭路径里。
7. 顺带收拢几处与窗口生命周期直接相关的临时做法：
   - 三个创建窗口的命令里“开线程、睡 10 ms、再 `run_on_main_thread`”的写法；
   - Windows 上创建窗口后“开线程、睡 100 ms，再关闭滑动导航”的写法；
   - 托盘菜单的 750/250 ms 焦点宽限期，以及前端两处“等服务端应用”的 300 ms 和导入对话框的 150 ms（§2.5、§3.10）。
8. 新增关闭模式 `WindowCloseBehavior { Destroy, Hide }`：
   - 全局设定一个值，主窗口、编辑器、托盘菜单各自可以选择“继承全局”、“销毁”或“隐藏”；
   - 所有关闭路径统一在 `CloseRequested` 处按该窗口的有效模式处理；
   - 现有的 `tray_menu_close_behavior` 并入新设定，通过配置迁移转换。
   - 默认值：全部为销毁（托盘菜单也继承全局）。已有配置里显式保存的托盘菜单设定由迁移保留。见 §3.8。
9. 不变：
   - 窗口种类（主窗口、编辑器、托盘菜单）的外观参数、URL 参数与 label 规则；
   - 窗口几何的保存与恢复；
   - 托盘菜单的失焦判定 `TrayMenuFocus`（失焦后改为调用 `close`，由关闭模式决定结果）；
   - `WindowMessageEvent`；
   - 统计小组件（独立的 egui 进程，不是 Tauri 窗口）；
   - capabilities 文件。

## 2. 现状与问题

### 2.1 transparent 的来历

`transparent(true)` 由 `98b8bd90e`（2022-05-25，Clash Verge 时期）加入。同一提交在 Windows 10 上调用 `window_vibrancy::apply_blur`，模糊效果要求窗口透明。2022-11 的 `5a35c5b92` 删除了 `apply_blur`，`transparent(true)` 却留了下来，一直保留到现在的 `window.rs:542`。

隐藏创建、前端 ready 后再显示，是 2026-05 的 `3abf38a1e` 才引入的。所以现有提交记录不支持“透明是为了掩盖前端初始化慢”这一说法。透明更像是模糊效果被删掉后的遗留。

现在前端 `body` 的背景不透明（`index.css`：浅色 `#f7f7f7`，深色 `#0b0b0b`），窗口透明不会产生任何可见效果。代价是 tauri-runtime-wry 为透明窗口在每次 `RedrawRequested` 时按窗口大小创建 softbuffer 位图（见报告 §4.1）。

`window-vibrancy` 依赖仍留在 `backend/tauri/Cargo.toml:233`，源码中已无调用。本 spec 不处理它，只记录在此。

### 2.2 显示时机分散在前后端

| 位置                                         | 做什么                                                                                                        |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `window.rs:539-545`                          | Windows：无边框，透明，`visible(config.visible_on_create)`                                                    |
| `resolve.rs:269-276`、`:377-385`、`:466-477` | 主窗口和托盘菜单隐藏创建，编辑器**可见创建**                                                                  |
| `__root.tsx:95-127` `WindowReveal`           | 每个窗口在设置查询结束（成功或失败）后自己调用 `show`、`unminimize`、`setFocus`，然后 emit `WindowReadyEvent` |
| `resolve.rs:158-167`                         | 监听 `WindowReadyEvent`；macOS 下主窗口 ready 时显示 Dock 图标                                                |
| `resolve.rs:218-251`                         | 只在非静默启动时为主窗口挂 10 秒兜底，超时后强制显示                                                          |
| `window.rs:467-476`                          | 单例窗口已存在时直接 `show` + `set_focus`，不管前端是否已 ready                                               |
| `resolve.rs:537-647`                         | 托盘菜单窗口新建后立即定位并 `show`，不等前端                                                                 |
| `lib.rs:363-385`                             | 主窗口 `CloseRequested`：保存几何；macOS 隐藏 Dock 图标                                                       |
| `lib.rs:388-389`                             | macOS `Reopen`：创建主窗口                                                                                    |

这种分散带来的问题：

- 编辑器窗口可见创建，前端加载期间先露出空白窗口。托盘菜单首次右键时也一样。
- 启动阶段主窗口还在加载时，从托盘或热键再请求打开，会走 `window.rs:467-476` 提前显示空窗口。
- 兜底计时器只覆盖启动时创建的主窗口。从托盘、热键或深链创建的窗口如果前端崩溃，会一直不可见。单例窗口之后的打开请求又会提前 `show`，只能碰巧救回来。
- 显示由前端调用 Tauri 窗口 API 完成，Dock 图标却由后端监听事件处理，同一件事有两个负责方。`WindowReadyEvent` 还占着共享事件总线的一个名字（`unified_rpc.rs:37`），它在 HTTP 传输上发送会被拒绝，实际只在桌面端有意义。

### 2.3 关闭行为

现在只有托盘菜单的关闭行为可以配置，即 `NyanpasuAppConfig.tray_menu_close_behavior: TrayMenuCloseBehavior { Hide, Close }`，默认为 `Hide`（`nyanpasu-config/src/application/mod.rs:51-58`、`:198-199`）。这个设定在两处分别分支处理：

- 后端失焦处理按设定调用 `close` 或 `hide`（`resolve.rs:493-505`）；
- 前端托盘菜单的点击处理也按设定决定先执行动作还是先隐藏（`tray-menu/_modules/hooks.ts`）。

主窗口和编辑器的关闭一律销毁，不可配置。所有前端关闭入口都调用 `appWindow.close()`：

- `window-control.tsx:111`；
- 编辑器 `css/index.tsx:57`、`:70`；
- 编辑器 `profile/index.tsx:74`、`:102`。

Tauri 的 `Window::close` 与用户点击关闭按钮一样，会先发出 `CloseRequested`，可以被拦截（`tauri-2.12.1/src/window/mod.rs:1890`）。`destroy` 则不发事件。因此全部关闭路径都可以在 `CloseRequested` 一处统一处理。

热键切换（`hotkey/adapters.rs:194-197`）用“窗口是否存在”判断开关，窗口改为隐藏后，这个判据不再成立。

### 2.4 其他相关的临时做法（基于延时）

- `ipc.rs:1902`、`:1918`、`:1952` 是同步命令，它们开线程、睡 10 ms，再 `run_on_main_thread` 创建窗口。Tauri 文档说明，在 Windows 上从同步命令或事件处理器里创建窗口会死锁，应改用 `async` 命令（`webview_window.rs:56-59`）。
- `window.rs:695-718` 开线程睡 100 ms，“等 webview 就绪”后再关闭 WebView2 的滑动导航，与 webview 何时就绪并无因果关系。

### 2.5 其他基于延时的临时做法

全仓库扫描 `sleep`、`setTimeout` 后，筛出的“用延时代替等待真实事件”的做法：

| 位置                                                                             | 延时            | 等的是什么                               | 处理                          |
| -------------------------------------------------------------------------------- | --------------- | ---------------------------------------- | ----------------------------- |
| `ipc.rs:1906`、`:1922`、`:1960`                                                  | 10 ms           | 让同步命令先返回，避免在命令里建窗死锁   | 改为 async 命令（§3.6）       |
| `window.rs:705`                                                                  | 100 ms          | WebView2 controller 创建完成             | 改为首次 ready 时设置（§3.4） |
| `resolve.rs:23-24`，`TrayMenuFocus`                                              | 750 ms / 250 ms | 托盘点击后，Windows shell 的焦点抖动结束 | 见 §3.10                      |
| `web-ui/_modules/core-secret-config.tsx:73`、`external-controller-config.tsx:62` | 300 ms          | “等服务端应用”                           | 见 §3.10                      |
| `profiles/$type/_modules/import-button.tsx:29`                                   | 150 ms          | 路由切换动画结束后再打开对话框           | 见 §3.10                      |

以下不属于这类，不处理：

- 周期任务：代理刷新 10 s、更新检查 30 s、日志落盘、geo 重试、ws 退避、deadlock 检测；
- 测试代码中的轮询；
- 带重试上限的单例等待（`utils/init/mod.rs:251`、`cmds/migrate.rs:151`），等待的是另一个进程退出；
- 窗口过程中必须同步阻塞的关机钩子轮询（`shutdown_hook.rs:84`、`:101`）；
- boa 的 promise 轮询（`enhance/script/js.rs:200`）；
- 前端的输入防抖、提示自动消失、resize 合并（`use-window-maximized.ts:37`），以及核心下载的进度轮询（`core-manager-card.tsx:72`）。

## 3. 设计

### 3.1 模块布局

`backend/tauri/src/window.rs` 改为目录 `backend/tauri/src/window/`：

| 文件         | 内容                                                                                |
| ------------ | ----------------------------------------------------------------------------------- |
| `mod.rs`     | `AppWindow`、`WindowConfig`、URL 参数工具、`WindowMessageEvent`（现有内容迁入）     |
| `table.rs`   | 纯数据的窗口表 `WindowTable`：label 分配与生命周期状态，不依赖 Tauri                |
| `manager.rs` | `WindowManager`：持有 `Mutex<WindowTable>` 与 `AppHandle`，执行 Tauri 窗口操作      |
| `kinds.rs`   | `MainWindow`、`EditorWindow`、`TrayMenuWindow` 及托盘菜单定位，从 `resolve.rs` 迁入 |
| `macos.rs`   | 红绿灯定位（现有 `window::macos` 迁入）                                             |
| `engine.rs`  | 与 webview 引擎绑定的代码：浏览器参数、滑动导航（§3.9）                             |

`resolve.rs` 中 `create_window`、`close_window`、`is_window_open`、`create_editor_window` 等窗口入口删除，调用方改为直接调用 `WindowManager`，不留转发函数。`TrayMenuWindowController` 原样随 `kinds.rs` 迁移。

### 3.2 窗口表（`table.rs`，纯逻辑）

```rust
/// Where a window is in its life. Visibility after ready is the OS's,
/// read from the window; the table does not mirror it.
enum Phase {
    /// Built hidden; the webview has not reported ready. Every open asks
    /// for the window, so it shows once ready.
    Loading,
    Ready,
}

struct Entry {
    base_label: String,
    phase: Phase,
}

#[derive(Default)]
pub struct WindowTable {
    entries: HashMap<String, Entry>,
}

pub enum Admission {
    /// Build a new window under this label.
    Create(String),
    /// A singleton instance exists and has reported ready: show it now.
    ShowExisting(String),
    /// A singleton instance is still loading; it shows when ready.
    Pending(String),
}

pub enum ReadyOutcome {
    /// First ready of a loading window: reveal it.
    Reveal,
    /// Ready again, e.g. after a page reload: nothing to do.
    AlreadyReady,
    /// No window under this label was created through the table.
    Unknown,
}

impl WindowTable {
    pub fn admit(&mut self, base_label: &str, singleton: bool) -> Admission;
    pub fn ready(&mut self, label: &str) -> ReadyOutcome;
    pub fn remove(&mut self, label: &str);
    pub fn is_ready(&self, label: &str) -> bool;
    pub fn instances(&self, base_label: &str) -> Vec<String>;
}
```

- `admit` 分配 label 的规则与现有 `WindowInstances::generate_label` 一致：单例用基础 label，非单例取 `base-N` 中最小的空闲编号。单例窗口未 ready 时再次请求打开，返回 `Pending`，不会提前显示：它 ready 后反正会显示。
- 新窗口在调用 `build` **之前**以 `Loading` 状态登记，所以 ready 一定晚于登记。即使 ready 早于 `build` 返回到达，由锁保证顺序。
- `build` 失败或收到 `WindowEvent::Destroyed` 时调用 `remove`。现有“创建前清理已不存在窗口的记录”（`window.rs:452-462`）改为依赖 `Destroyed`，并保留创建失败时的 `remove`。

### 3.3 `WindowManager`（`manager.rs`）

组合根在 `setup.rs:227` 处 `app.manage(WindowManager::new(app_handle))`，替代 `WindowRegistry`。公开操作：

```rust
impl WindowManager {
    /// Opens a window of this kind: builds it hidden, or shows the existing
    /// singleton once it is ready.
    pub fn open(&self, kind: &dyn AppWindow, params: Option<WindowParams>) -> Result<String>;
    /// The frontend of `label` has rendered; reveals it on the first call.
    pub fn report_ready(&self, label: &str);
    /// Asks the window to close; its close behavior decides what that means.
    pub fn close(&self, label: &str);
    /// Whether the window exists and is shown. A hidden window is not open.
    pub fn is_visible(&self, label: &str) -> bool;
}
```

`open` 的步骤：

1. 先在 `table.instances(base_label)` 中找已 ready、但被隐藏的实例。找到就复用它，走 §3.4 的显示流程；只有关闭模式为隐藏时才会出现这种实例（§3.8）。
2. 没有可复用的实例时调用 `table.admit`。`ShowExisting` 走 §3.4 的显示流程；`Pending` 直接返回。
3. `Create`：沿用现有 `create_with_params` 的构建代码，但固定 `.visible(false)`，并删除 `.transparent(true)`。几何恢复、居中、阴影、开发版 devtools、macOS 红绿灯都放在 `build` 之后执行，与现在相同。
4. 为该窗口注册一个 `on_window_event`，处理以下事件：
   - `Destroyed`：调用 `table.remove`，并取消兜底计时器；
   - `CloseRequested`：先执行窗口种类的关闭钩子，即主窗口保存几何（现 `lib.rs:371`）、macOS 下主窗口隐藏 Dock 图标（现 `lib.rs:373`）；然后按 §3.8 的有效关闭模式处理。
5. 启动兜底计时器，见 §3.5。

托盘菜单定位需要在显示前设置位置。`show_tray_menu_window` 改为：先 `open`，再对窗口 `set_position`。窗口已 ready 时立即显示；仍在加载时只更新位置，ready 后由 §3.4 显示。

### 3.4 显示流程（ready 与再次打开共用）

`report_ready` 收到 `Reveal`，或 `open` 收到 `ShowExisting` 时，依次执行：

1. 仅限首次 ready：调用 `engine::on_first_ready`。在 Windows 上，它关闭 WebView2 的滑动导航，代码从 `window.rs:707-716` 迁来。此时 webview 一定已创建，所以删除开线程睡 100 ms 的写法。
2. 仅限 macOS 且仅限主窗口：在主线程调用 `dock::macos::show_dock_icon()`，它会把激活策略设为 Regular 并激活应用。
3. 依次调用 `unminimize`、`show`、`set_focus`。

### 3.5 兜底计时器

`open` 新建窗口时，用 `tauri::async_runtime::spawn` 起一个任务：睡 10 秒，然后对该 label 调用 `report_ready`，并记录 `warn`。任务的 `JoinHandle` 存进 `Entry`，`ready` 或 `remove` 时调用 `abort`。这样窗口销毁后同名窗口重建，旧计时器也不会误触发，不需要另加代次编号。

`resolve.rs:228-251` 的 `spawn_window_ready_timeout` 删除。

### 3.6 RPC 与前端

后端在 `ipc.rs` 新增以下命令，并在 `specta_export.rs` 注册为 mutation。它只在桌面端可用，没有 `http`。

```rust
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn report_window_ready(
    windows: State<'_, WindowManager>,
    webview: WebviewWindow,
) -> Result<()> {
    windows.report_ready(webview.label());
    Ok(())
}
```

label 取自调用方的 webview，不接受前端传入，前端无法替其他窗口报告 ready。宏已支持注入 `WebviewWindow`（`nyanpasu-macro/src/unified_command.rs:109-110`、`:198-201`）。

`create_main_window`、`create_debug_tray_menu_window`、`create_editor_window` 改为 `async` 命令，直接调用 `WindowManager`，删除开线程睡 10 ms 的写法。

前端 `__root.tsx` 的 `WindowReveal` 改为 `WindowReadyReporter`：在 Tauri 环境中，设置查询结束（成功或失败）后，每次页面加载只调用一次 `rpc.reportWindowReady()`。错误边界 `Catch` 渲染时也调用一次，让出错的窗口立即可见，而不是等 10 秒兜底。

删除的部分：

- `window.rs:291-297` 的 `WindowReadyEvent`；
- `specta_export.rs:21`、`:188` 的注册；
- `unified_rpc.rs:37` 的事件名；
- `resolve.rs:158-167` 的监听；
- 重新生成的绑定中对应的类型。

`frontend/rpc/tests/event-transport.test.ts:134-135` 只是借用这个事件名来测“HTTP 拒绝 emit”，改用仍然存在的 `window-message-event` 即可。

### 3.7 macOS

- `resolve_setup` 中 `set_activation_policy(Accessory)` 保持不变。
- Dock 图标随主窗口显示而出现（§3.4 第 2 步），随主窗口 `CloseRequested` 而隐藏（§3.3 第 3 步）。这与现有行为一致，只是改由 `WindowManager` 负责。
- `RunEvent::Reopen` 改为调用 `WindowManager::open(&MainWindow, None)`。
- 隐藏的 `WKWebView` 仍会加载和执行脚本，现有隐藏创建主窗口的流程已经依赖这一点，所以 ready 可以在窗口显示之前到达。

### 3.8 关闭模式

#### 配置

在 `nyanpasu-config/src/application/` 新增 `window_close.rs`：

```rust
/// What closing a window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum WindowCloseBehavior {
    /// Destroy the window and its webview, freeing their memory.
    #[default]
    Destroy,
    /// Keep the window and its webview, hidden, so reopening is instant.
    Hide,
}

/// One window kind's choice: follow the global behavior or set its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum WindowCloseOverride {
    #[default]
    Inherit,
    Destroy,
    Hide,
}

impl WindowCloseOverride {
    pub fn resolve(self, global: WindowCloseBehavior) -> WindowCloseBehavior {
        match self {
            Self::Inherit => global,
            Self::Destroy => WindowCloseBehavior::Destroy,
            Self::Hide => WindowCloseBehavior::Hide,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(default)]
pub struct WindowCloseSettings {
    pub global: WindowCloseBehavior,
    pub main: WindowCloseOverride,
    pub editor: WindowCloseOverride,
    pub tray_menu: WindowCloseOverride,
}
```

- 用 `#[derive(Default)]`：`global: Destroy`，三个窗口均为 `Inherit`。也就是说，新安装的托盘菜单默认销毁，与之前默认隐藏不同；已有配置文件会显式写有 `tray_menu_close_behavior`，迁移后保留原值。
- `NyanpasuAppConfig` 新增字段 `window_close: WindowCloseSettings`，删除 `tray_menu_close_behavior` 与 `TrayMenuCloseBehavior`。
- struct-patch 对嵌套字段整值替换，前端每次提交整个 `window_close` 对象。
- 改动不影响运行时，`impact.rs:872-876` 的用例按 `RuntimeImpact::None` 改写。`WindowManager` 在每次关闭时读取 `app_config_snapshot()`，所以设定改动不需要 effect。
- “继承全局”用显式的 `Inherit` 变体，而不是 `Option`。这样 YAML 和前端下拉框里都能看到这个选项，也避开 struct-patch 对 `Option` 字段的双层包装。

窗口种类通过 `AppWindow` 新增的方法取用自己的设定：

```rust
fn close_override(&self, settings: &WindowCloseSettings) -> WindowCloseOverride;
```

主窗口返回 `settings.main`，两种编辑器返回 `settings.editor`，托盘菜单（包括调试用的常驻托盘菜单）返回 `settings.tray_menu`。

#### 关闭处理

`WindowManager` 在 `CloseRequested` 中计算有效模式 `kind.close_override(&settings).resolve(settings.global)`：

- `Destroy`：不拦截，窗口照常销毁，随后收到 `Destroyed`，从表中移除。
- `Hide`：调用 `api.prevent_close()`，再调用 `hide()`。窗口仍以 `Ready` 状态留在表中，下次 `open` 时按 §3.3 第 1 步复用。

关闭钩子（保存几何、macOS 隐藏 Dock 图标）在两种模式下都执行。

关闭入口统一为 `close`：

- 后端托盘菜单失焦处理（`resolve.rs:493-505`）不再读设定，直接调用 `close`。
- 前端托盘菜单的点击处理（`hooks.ts`）不再读设定，统一为：先 `hide()` 以立即消失，再执行动作，最后 `close()`。动作的 IPC 在窗口销毁之前已经发出，原注释要防止的问题依然被避免；在隐藏模式下，最后的 `close()` 等于对已隐藏的窗口再隐藏一次。
- 其他前端入口原本就调用 `close()`，不需要改。

热键切换改为：`is_visible(main)` 为真时 `close`，否则 `open`。`exit.rs:235-237` 保存几何的条件改为“主窗口存在”，隐藏的主窗口仍保存它的几何。

退出应用时，Tauri 直接销毁窗口，不经过 `CloseRequested`（需在冒烟测试中确认），所以隐藏模式不会阻止退出。

#### 配置迁移

在 `core/migration/modules/app_config.rs` 新增步骤 `app_config/window_close`，revision 为 5：

- 如果存在 `tray_menu_close_behavior`，就写入 `window_close`，其中 `tray_menu` 按 `hide → hide`、`close → destroy` 转换，其余字段取默认值，然后删除旧键。
- `detect_baseline` 增加对应的 `needs_window_close_migration`。
- legacy 1.x 迁移（`legacy_schema/application.rs:111-115`）改为写入 `next.window_close.tray_menu`。

#### 设置界面

设置页 `settings/nyanpasu` 中的 `TrayMenuCloseBehaviorSelector` 换成“窗口关闭方式”卡片：

- 一项全局选择：销毁 / 隐藏；
- 主窗口、编辑器、托盘菜单各一项：跟随全局（括号中显示解析后的值）/ 销毁 / 隐藏。托盘菜单一项仍然只在 WebView 托盘模式下显示。
- 说明文案写明取舍：隐藏保留 webview，再次打开更快，但占用内存。
- 五种语言都新增 i18n key，删除 `settings_nyanpasu_tray_menu_close_behavior*`，并运行 paraglide compile。

### 3.9 为 CEF runtime 预留

#### Tauri 3 alpha 的现状

以下事实来自一手来源：

- 从 v3.0.0-alpha.0 起，webview runtime 不再由 `tauri` crate 的 Cargo feature 选择。应用直接依赖 `tauri-runtime-wry` 或 `tauri-runtime-cef`，在 `tauri::Builder::runtime` 处选定。`AppHandle`、`Window`、`Webview` 的默认类型参数改为 `tauri::DynRuntime`。
- runtime 专属 API 从 `tauri` 移到各 runtime crate 的扩展 trait 中：
  - wry：`AppHandleWryExt`、`WebviewWryExt`、`WebviewWindowBuilderWryExt` 等；
  - CEF：`WebviewCefExt`、`WebviewWindowBuilderCefExt` 等。
  - `with_webview` 拿到的平台 webview 改为通过 `PlatformWebview::downcast_ref` 取得具体类型。
- `tauri-runtime-cef` 3.0.0-alpha.5 的 `WebviewWindowBuilderCefExt` 提供 `browser_runtime_style`、`on_frame_event`、`on_console_message`、`allow_chrome_commands`、`with_browser_settings`。CEF 的非浏览器进程（renderer、GPU 等）会重新执行同一个可执行文件，由 `#[cef_entry_point]` 在入口处分流。

以下内容只有单一的二手来源，置信度低：CEF runtime 目前主要在 Linux X11 上验证过，Windows 与 macOS 仍在打磨。

本次不升级到 Tauri 3，也不引入 CEF。下面的约束只是让以后的切换局限在少数几处。

#### 现有代码中与 runtime 绑定的点

| 位置                                                          | 绑定内容                                            | 切换到 CEF 时                                                                                  |
| ------------------------------------------------------------- | --------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `window.rs:544` `additional_browser_args`                     | WebView2 的命令行 feature 开关                      | 改由 CEF 的命令行开关或 `with_browser_settings` 提供；两者的开关含义不同，需逐项核对           |
| `window.rs:707-716` `with_webview` + `ICoreWebView2Settings6` | 直接调用 WebView2 COM 关闭滑动导航                  | CEF 没有 WebView2，需要换成 CEF 侧的等价做法或去掉                                             |
| `window.rs:842-1103` macOS 红绿灯                             | 用 `ns_window()` 取 `NSWindow`，并替换它的 delegate | CEF 在 macOS 上的窗口层级与 delegate 归属尚未确认，需要重新验证                                |
| 8 个文件共 35 处 `tauri::Wry` / `Wry>`                        | 显式写死 runtime 类型                               | v3 中改为默认的 `DynRuntime` 或泛型                                                            |
| `lib.rs:128-188` `run()` 开头                                 | Tauri 之前运行命令行解析、单例检测、配置迁移        | CEF 子进程会重新执行这里；必须在所有这些之前分流，否则子进程会撞上单例锁并退出，或者重复跑迁移 |

#### 本次落实的约束

1. **ready 协议不依赖 webview 引擎：** ready 由前端主动调用 `report_window_ready`（§3.6），不使用 `on_page_load` 等由引擎上报的加载事件。这类事件在 WebView2、WKWebView、CEF 之间的触发时机并不一致。
2. **引擎专属代码集中到一处：** 新增 `window/engine.rs`，只放与 webview 引擎绑定的代码：
   - `configure_builder(builder)`：附加浏览器参数；
   - `on_first_ready(&window)`：关闭滑动导航。

   `WindowManager` 和各窗口种类只调用这两个函数。将来切换 runtime 时，只需按 runtime 改写这个文件。macOS 红绿灯代码绑定的是窗口层而不是 webview，留在 `window/macos.rs`，在上表中登记即可。

3. **新代码不写 `tauri::Wry`：** `window/` 下的新代码一律使用默认类型参数的 `AppHandle`、`WebviewWindow`。迁入 `window/` 的代码顺带去掉显式的 `Wry`，即 `window.rs:924`、`:929` 与 `resolve.rs:486` 三处。其余文件中的 `Wry` 不在本次范围内。
4. **透明去掉之后**，就不必再依赖各 runtime 对透明窗口的不同实现（§4）。
5. **入口分流点只记录，不实施：** 切换 CEF 的前置条件是，在 `run()` 的最开头，即 `Profilers` 和命令行解析之前，加入 CEF helper 分流。本次不改 `run()`，只把这一点写进本 spec，作为将来迁移的检查项。

### 3.10 基于延时的临时做法

**托盘菜单的焦点宽限期。** `core/tray/mod.rs:550-566` 匹配 `TrayIconEvent::Click` 时没有限定 `button_state`。在 Windows 上，按下和松开各发一次 `Click`，所以一次右键会调用两次 `show_tray_menu_window`，一次左键也会调用两次 `create_window`。第一次在按下时显示并获取焦点，随后 shell 在松开时把焦点拿回托盘区，于是窗口失焦。这很可能就是那段“焦点抖动”的来源。

处理：

1. 只在 `button_state: MouseButtonState::Up` 时响应左右键。
2. 删除 `TRAY_MENU_SHOW_BLUR_GRACE` 与 `TRAY_MENU_FOCUS_BLUR_GRACE`，以及 `TrayMenuFocus` 中基于时间的字段。保留基于事件的判定：首次 `Focused(true)` 之前的失焦不关闭菜单，常驻调试菜单不因失焦关闭。`TrayMenuFocus` 不再需要传入时间，测试相应改写。
3. 冒烟测试（§6 第 3 项）如果仍有失焦后立即关闭的现象，就停下来报告，不要悄悄加回延时。

**“等服务端应用”的 300 ms。** 两处都在 `patchClashConfig` 返回后再等 300 ms，然后刷新运行时 profile。按架构规则，进程内的 mutation 要等到真实结果才返回。实施前沿 `patch_clash_config` 的后端调用链确认：它是否在运行时应用完成（Required participant 投票并提交）之后才返回。

- 若确认如此，删除 `sleep(300)` 及其注释；如果 `sleep` 因此不再被使用，也一并删除。
- 若并非如此，不改，在交付报告中说明原因。

**导入按钮的 150 ms。** 这段代码在路由切换后打开“导入本地 profile”对话框，150 ms 是在等切换动画。实施时查明动画的来源（路由 view transition 还是页面的进入动画），如果有可等待的完成信号（例如 `document.startViewTransition` 的 `finished`、路由的 `onResolved`，或动画结束事件），就改用它。找不到可靠信号时不改，在交付报告中说明。

### 3.11 调用方迁移

| 调用方                                         | 改为                                                                                                           |
| ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `core/tray/mod.rs:525`、`:554` 打开窗口        | `WindowManager::open(&MainWindow, None)`                                                                       |
| `core/tray/mod.rs:564` 托盘菜单                | `kinds::show_tray_menu_window`（§3.3）                                                                         |
| `client/hotkey/adapters.rs:194-197` 切换主窗口 | `is_visible` / `close` / `open`（§3.8）                                                                        |
| `lib.rs:337`、`:344` 深链                      | `open(&MainWindow, None)`                                                                                      |
| `lib.rs:363-385` 主窗口事件                    | 关闭钩子移入 `WindowManager`；`ScaleFactorChanged`（托盘）与 `connection_details::cancel_for_webview` 留在原处 |
| `utils/exit.rs:235-237`                        | 主窗口存在即保存几何（§3.8）                                                                                   |
| `resolve.rs:218-221` 启动创建                  | `open(&MainWindow, None)`                                                                                      |

## 4. 去掉透明后的预期与风险

预期：Windows 主窗口打开时，私有工作集减少约 30 MB（报告 §2 的 softbuffer 行，与窗口尺寸成正比）。窗口关闭后不受影响。

风险与对策：

- **显示前的背景色：** 窗口在前端渲染完成后才显示，首帧就是前端画面，所以不需要 `background_color`。
- **拖拽缩放时边缘露底：** 深色主题下，WebView2 尚未重绘的区域可能闪过白色。如果冒烟测试复现，再按主题调用 `set_background_color`（Tauri 2.12.1 提供）。本 spec 不预先加入。
- **无边框窗口的外观：** Windows 11 上 `set_shadow(true)` 提供圆角与阴影，与是否透明无关；Windows 10 一直是直角。冒烟测试时核对两者。

## 5. 测试

`table.rs` 的纯单元测试，现有两条 label 测试一并迁入：

- 新建窗口登记为 `Loading`；首次 `ready` 返回 `Reveal`，第二次返回 `AlreadyReady`。
- 未登记的 label 收到 `ready` 时返回 `Unknown`。
- 单例窗口：加载中再次 `admit` 返回 `Pending`，ready 后再 `admit` 返回 `ShowExisting`。
- 非单例窗口取最小的空闲编号；`remove` 之后同一 label 重新以 `Loading` 登记。

`window_close.rs`：`WindowCloseOverride::resolve` 的三种取值，以及 `WindowCloseSettings::default()` 全部解析为销毁。

迁移步骤 `app_config/window_close`：

- `tray_menu_close_behavior: close` 转为 `window_close.tray_menu: destroy`，`hide` 转为 `hide`；
- 旧键被删除；
- 没有旧键的配置不触发迁移；
- legacy 1.x fixture 的期望结果同步更新。

`WindowManager` 直接操作 Tauri 窗口，不写依赖 Tauri 运行时的测试，由 §6 的冒烟测试覆盖。

前端：如果现有测试覆盖了 `__root`，就在其 mock 下断言 `reportWindowReady` 每次加载只调用一次；否则不为此新增测试。

## 6. 冒烟测试

在 Windows 上用 verge-dev 开发版测试，与安装版并行运行，不替换安装版：

1. 冷启动：主窗口在前端渲染完成后出现，没有空白帧。
2. 静默启动后，从托盘打开主窗口。
3. 托盘菜单首次右键：在光标处出现，没有空窗口闪现。之后的右键在“隐藏”和“销毁”两种模式下均正常，点击“退出”等动作仍然生效。去掉焦点宽限期后，菜单不会一出现就因失焦而关闭；左键单击托盘只打开一次主窗口。
4. 打开 profile 编辑器和 CSS 编辑器：没有空白窗口。
5. 热键切换主窗口；启动加载期间连续点托盘“打开窗口”，加载完成前窗口不提前显示。
6. 在主窗口按 Ctrl+R 重载：不触发重复显示或抢焦点。
7. 让前端在 ready 之前抛错：错误页立即可见。让前端完全不加载（例如指向不存在的 dev server）：10 秒后窗口被强制显示，日志中有警告。
8. 内存：用剖析报告中的同一测量脚本，窗口打开时私有工作集应减少约 30 MB。
9. 外观：深色主题下拖拽缩放；Windows 11 的圆角与阴影。
10. 关闭模式：
    - 全局设为隐藏后，主窗口和编辑器关闭后再次打开，都是复用原窗口（页面状态保留）。
    - 单独把主窗口设为销毁，其他窗口继承隐藏，两者互不影响。
    - 隐藏模式下，热键能正确切换主窗口。
    - 主窗口隐藏后退出应用，能正常退出。
    - 用 2.0.x 的配置启动，`tray_menu_close_behavior` 被正确迁移。

macOS 需要你在本机测试，我无法在 macOS 上运行：

- 冷启动后 Dock 图标出现；关闭主窗口后 Dock 图标消失；点 Dock 图标重新打开主窗口。销毁和隐藏两种模式各测一次。
- 红绿灯位置，以及全屏的进入和退出。

Linux：启动、托盘打开、编辑器窗口各测一次，看有无回归。

## 7. 提交拆分

分两个 PR，均基于 main。PR A 是窗口管理（提交 1–7），PR B 是窗口以外的延时（提交 8–9）。每个提交单独可构建：

1. `refactor(window): let a window manager own the window lifecycle`
   - 建立 `window/` 目录、`WindowTable`、`WindowManager`；
   - 新增 `report_window_ready`，前端同步改用；
   - 删除 `WindowReadyEvent`、`visible_on_create` 和启动兜底；
   - 迁移全部调用方，重新生成绑定。
   - 前后端在同一提交中改，因为单独改任何一边，窗口都不会被显示。
2. `refactor(window): create windows from async commands`：删除三个创建窗口命令中开线程睡 10 ms 的写法。
3. `refactor(window): disable swipe navigation once the webview is ready`：删除开线程睡 100 ms 的写法。`engine.rs` 在第 1 个提交中建立，浏览器参数从那时起就放在这个文件里。
4. `perf(window): stop making windows transparent`：删除 Windows 的 `.transparent(true)` 和 `WindowConfig.transparent`。
5. `feat(config): replace the tray menu close behavior with window close settings`：新增配置类型和迁移步骤，同步修改 legacy 迁移；后端仍只对托盘菜单应用该设定，行为不变。
6. `feat(window): let each window close by destroying or hiding`：`WindowManager` 拦截 `CloseRequested`，复用隐藏的窗口；修改热键切换和托盘菜单关闭入口；替换设置界面与 i18n，重新生成绑定。
7. `fix(tray): respond to a tray click once, on release`：只在松开时响应，删除托盘菜单的焦点宽限期（§3.10）。
8. `refactor(settings): stop waiting for the core after patching the controller`：删除两处 `sleep(300)`，前提是已确认 mutation 会等待应用完成（§3.10）。
9. `refactor(profiles): open the import dialog when the transition ends`：仅在找到可靠的完成信号时提交（§3.10）。

## 8. 决策（2026-10-06 已定）

1. **兜底计时器：** 每个新建窗口一份，10 秒（§3.5）。
2. **macOS Dock 策略：** 保持现状，Dock 图标只跟随主窗口。
3. **窗口种类迁出 `resolve.rs`：** 迁到 `window/kinds.rs`（§3.1）。
4. **ready 的判定条件：** 沿用“设置查询结束”。
5. **关闭模式的默认值：** 全部为销毁（§3.8）。
6. **设定改动对已隐藏窗口的影响：** 下次关闭时才生效。
7. **隐藏模式下复用编辑器的状态：** 不处理。
8. **CEF 预留的范围：** 只落实 §3.9 的约束 1–4，入口分流只作记录。
