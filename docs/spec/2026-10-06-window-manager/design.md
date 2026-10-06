# 窗口管理层：由后端掌管窗口生命周期与显示时机

**日期：** 2026-10-06

**状态：** 已实现（2026-10-06），四个 draft PR：#5646、#5647、#5648、#5649，见 §7。本文按最终实现更新；起初的边沿触发设计经多轮评审被电平触发设计取代，见 §9。

**基线：** 起草于 main `e2a5e45a9`，实现基于 main `e53f874e7`；Tauri 2.12.1，tauri-runtime-wry 2.12.1。

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
- tauri-runtime-wry 2.12.1 `src/lib.rs`：`send_user_message` 在主线程上内联执行（:263）；窗口的 `close` 与 `destroy` 一律经 `proxy.send_event` 排队（:2120–2134）；窗口监听器在持有该窗口监听器列表的 `Mutex` 时被调用（:4228、:4364）。
- 本 spec §2 引用的仓库代码与提交（行号为起草时的 main，实现后已变）。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec。

## 1. 决策与范围

1. 新增 `WindowManager`，取代 `WindowRegistry`，同时负责窗口登记、创建、显示、隐藏、ready 和销毁。窗口的 Tauri 操作只由它发起。它属于 GUI crate 的 adapter，用一把锁保护窗口表，不是 actor，也不进入 `NyanpasuClient`。窗口表只记事实（已建好、已 ready、应显示），原生窗口的状态由一个幂等的 `apply` 按事实收敛，结果与事件到达的顺序无关（§3.2–§3.4）。
2. 所有窗口都以隐藏状态创建。前端准备好后调用新的 RPC mutation `report_window_ready`，后端据此显示窗口。删除 `WindowReadyEvent` 和 `WindowConfig.visible_on_create`；前端不再自己调用 `show`、`unminimize` 和 `setFocus`。
3. 窗口未 ready 时，打开请求只把窗口记为“应显示”，不提前显示白屏或空窗口；ready 到达后由 `apply` 显示。窗口已 ready 时，打开请求触发的 `apply` 直接显示并聚焦。
4. 兜底计时器从“只在启动时、只管主窗口”改为每个新建窗口一份：10 秒内没有 ready 就按已 ready 处理并记录警告。ready 或销毁时取消计时器。见 §3.5、§8-1。
5. 去掉 Windows 的 `.transparent(true)`，同时删除只有 Linux 读取、却没有任何窗口设置过的 `WindowConfig.transparent`。macOS 本来就不透明，不变。
6. macOS 也走这套流程。Dock 图标的显示与隐藏，从 `resolve_setup` 的 ready 监听和 `lib.rs` 的 `CloseRequested` 分支移到 `apply` 的显示路径和窗口种类的 `on_dismissed` 钩子里。
7. 顺带收拢几处与窗口生命周期直接相关的临时做法：
   - 三个创建窗口的命令里“开线程、睡 10 ms、再 `run_on_main_thread`”的写法；
   - Windows 上创建窗口后“开线程、睡 100 ms，再关闭滑动导航”的写法；
   - 托盘菜单的 750/250 ms 焦点宽限期，以及前端两处“等服务端应用”的 300 ms 和导入对话框的 150 ms（§2.5、§3.10）。
8. 新增关闭模式 `WindowCloseBehavior { Destroy, Hide }`：
   - 全局设定一个值，主窗口、编辑器、托盘菜单各自可以选择“继承全局”、“销毁”或“隐藏”；
   - 所有关闭路径（标题栏、前端按钮、热键、托盘菜单失焦）都只做一件事：把窗口记为“不再应显示”。`apply` 对不再应显示的窗口，按有效模式销毁或隐藏；
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

Tauri 的 `Window::close` 与用户点击关闭按钮一样，会先发出 `CloseRequested`，可以被拦截（`tauri-2.12.1/src/window/mod.rs:1890`）。`destroy` 则不发事件。起草时的设想是全部关闭路径都在 `CloseRequested` 一处统一处理；实现时发现管理层自己调用原生 `close()` 会带来排序问题，改为“关闭只改事实”，见 §3.3。

热键切换（`hotkey/adapters.rs:194-197`）用“窗口是否存在”判断开关，窗口改为隐藏后，这个判据不再成立。

### 2.4 其他相关的临时做法（基于延时）

- `ipc.rs:1902`、`:1918`、`:1952` 是同步命令，它们开线程、睡 10 ms，再 `run_on_main_thread` 创建窗口。Tauri 文档说明，在 Windows 上从同步命令或事件处理器里创建窗口会死锁，应改用 `async` 命令（`webview_window.rs:56-59`）。
- `window.rs:695-718` 开线程睡 100 ms，“等 webview 就绪”后再关闭 WebView2 的滑动导航，与 webview 何时就绪并无因果关系。

### 2.5 其他基于延时的临时做法

全仓库扫描 `sleep`、`setTimeout` 后，筛出的“用延时代替等待真实事件”的做法：

| 位置                                                                             | 延时            | 等的是什么                               | 处理                          |
| -------------------------------------------------------------------------------- | --------------- | ---------------------------------------- | ----------------------------- |
| `ipc.rs:1906`、`:1922`、`:1960`                                                  | 10 ms           | 让同步命令先返回，避免在命令里建窗死锁   | 改为 async 命令（§3.6）       |
| `window.rs:705`                                                                  | 100 ms          | WebView2 controller 创建完成             | 改为首次 ready 时设置（§3.3） |
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
| `table.rs`   | 纯数据的窗口表 `WindowTable`：label 分配与窗口的事实，不依赖 Tauri                  |
| `manager.rs` | `WindowManager`：持有 `Mutex<WindowTable>` 与 `AppHandle`，排队并执行 `apply`       |
| `kinds.rs`   | `MainWindow`、`EditorWindow`、`TrayMenuWindow` 及托盘菜单定位，从 `resolve.rs` 迁入 |
| `macos.rs`   | 红绿灯定位（现有 `window::macos` 迁入）                                             |
| `engine.rs`  | 与 webview 引擎绑定的代码：浏览器参数、滑动导航（§3.9）                             |

`resolve.rs` 中 `create_window`、`close_window`、`is_window_open`、`create_editor_window` 等窗口入口删除，调用方改为直接调用 `WindowManager`，不留转发函数。`TrayMenuWindowController` 原样随 `kinds.rs` 迁移。

### 3.2 窗口表（`table.rs`，纯逻辑）

窗口表只记事实，不记“发生过什么”。每个条目：

```rust
struct Entry<H> {
    base_label: String,
    built: bool,          // 原生窗口已建好（隐藏）
    ready: bool,          // webview 报告了 ready，或兜底计时器到期
    wanted: bool,         // 应当显示：每次打开置真，每次关闭置假
    setup_pending: bool,  // 首次 ready 还没处理，需要已建好的窗口
    watchdog: Option<AbortHandle>,
    hooks: H,             // 窗口种类的钩子，表保存并原样交还
}
```

不变量：原生窗口展示 ⇔ `built && ready && wanted`；不再 `wanted` 时，窗口被销毁，或在已展示时被隐藏。

事件只改事实，然后排一次 `apply`（§3.3）。`facts(label)` 只对已建好的条目返回事实，并取走 `setup_pending`；`Facts::actions(presented)` 是纯函数，给出 `first_ready`、`show`（`ready && wanted`）、`retire`（`!wanted`）和 `dismiss`（`!wanted && presented`）。`presented` 由 `apply` 读原生窗口得到：可见，或已最小化（macOS 的 `isVisible` 对最小化窗口为假，其他平台把最小化窗口算作可见）。

主要接口：`open(base, singleton, hooks) -> Opened { Build(label) | Existing(label) }`、`built`、`ready`、`unwant`、`wanted`、`facts`、`set_watchdog`、`remove`、`instances`。表带泛型 `H`，所以不出现 Tauri 类型。

- label 规则与旧 `WindowInstances::generate_label` 一致：单例用基础 label，非单例取 `base-N` 中最小的空闲编号。
- 新窗口在 `build` **之前**登记（`wanted` 为真），所以 ready 一定晚于登记；即使 ready 早于 `build` 返回，也只是先记下 `ready`，`apply` 等窗口建好再处理。
- 关闭模式为隐藏时，`open` 先选同一 base 下“已建好且不再应显示”的实例，再考虑新建（§3.8）。
- 窗口收到 `Destroyed`，或 `build` 失败时 `remove`；不再在创建前清理“已不存在窗口的记录”。

### 3.3 `WindowManager`（`manager.rs`）

组合根在 `setup.rs` 处 `app.manage(WindowManager::new(app_handle))`，替代 `WindowRegistry`。公开操作：

```rust
impl WindowManager {
    pub fn open<K: AppWindow>(&self, kind: &K, params: Option<WindowParams>) -> Result<String>;
    pub fn report_ready(&self, label: &str);
    pub fn close(&self, label: &str);   // 只把窗口记为不再应显示
    pub fn is_wanted(&self, label: &str) -> bool;
    pub fn instances(&self, base_label: &str) -> Vec<String>;
}
```

事件与它们设置的事实：

| 事件                                                                   | 效果                                                                                                                                                                                                      |
| ---------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `open`，窗口已存在                                                     | `wanted = true`，并总是排 `apply`，所以已显示的窗口会被聚焦                                                                                                                                               |
| `open`，窗口不存在                                                     | 登记（`wanted = true`）并在调用线程上构建。命令、托盘与热键都在异步运行时上构建，因为 Windows 上从主线程的事件处理里建窗会死锁；启动时的首次 `open` 在 `setup` 回调里进行（事件循环尚未运行），与以前相同 |
| 构建完成（几何、居中、阴影、devtools、红绿灯、`kind.on_created` 之后） | `built = true`；窗口尚未 ready 时启动兜底计时器                                                                                                                                                           |
| 前端 ready 报告                                                        | 首次：`ready = true`，`setup_pending = true`，停止计时器                                                                                                                                                  |
| 兜底计时器到期                                                         | 与 ready 报告相同，并记录警告                                                                                                                                                                             |
| `manager.close`、用户的 `CloseRequested`、托盘菜单失焦                 | `wanted = false`                                                                                                                                                                                          |
| `Destroyed`                                                            | 移除条目，停止计时器                                                                                                                                                                                      |

只有事实确实变了才排 `apply`（`open` 除外）。

**`apply(label)`**（`manager.rs`）总是在主线程运行，读取当时的事实，所以排队它的是哪些事件、顺序如何都无关紧要；为一个已消失或已被同名窗口取代的 label 排的 `apply` 只会看到现有条目自己的事实。步骤：

1. 条目缺失或窗口未建好：返回，待处理的事实留着。
2. `first_ready`：调用 `engine::on_first_ready`（§3.9）。
3. `ready && wanted`：`unminimize`、`show`、`set_focus`；macOS 且为主窗口时先显示 Dock 图标。
4. 否则若不再 `wanted`：窗口已展示（`presented`）时先调用窗口种类的 `on_dismissed` 钩子（主窗口保存几何，macOS 下隐藏 Dock 图标），然后按有效模式销毁，或在已展示时隐藏（§3.8）。

两条规则来自 tauri-runtime-wry 2.12.1 的源码：

- **管理层永远不调用原生 `close()`。** `close()` 与 `destroy()` 都经 `proxy.send_event` 排队，即使在主线程上也是（`src/lib.rs:2120–2134`）。排队的 `close()` 之后会作为一个 `CloseRequested` 回来，与用户的关闭无法区分，会覆盖更晚的 `open`（打开、关闭、再打开）。所以管理层的输出只有 `show`、`hide`、`destroy`，`destroy` 不发 `CloseRequested`。
- **`apply` 总是排队，不内联运行。** `run_on_main_thread`、`show`、`hide` 在主线程上内联执行（`:263`），而窗口监听器在持有该窗口监听器列表的 `Mutex` 时被调用（`:4228`、`:4364`）。从监听器里内联运行 `apply` 会在持锁时再派发同一窗口的事件。所以排队用 `tauri::async_runtime::spawn` 再 `run_on_main_thread`。`apply` 与 `CloseRequested` 处理都在主线程，彼此串行；`apply` 在读事实时才决定做什么，所以“先显示后隐藏”和“先隐藏后跳过显示”都以隐藏收场。持有表的锁只用于读写事实，不会在持锁时调用 Tauri 或排 `apply`。

**`CloseRequested`**（标题栏、Alt+F4、前端的 `close()`）在两种模式下都被 `prevent_close`，并等同于 `manager.close`；`WindowManager` 状态缺失时不拦截。`on_window_event` 仍用于窗口种类的其他事件（托盘菜单的焦点）。

**窗口种类的钩子**用一个对象安全的 `WindowHooks` 在条目里保存（对 `AppWindow` 的 blanket impl）：`on_dismissed`，以及按窗口种类决定关闭后销毁还是隐藏的钩子。第一阶段（#5647）里它是 `hides_on_close(&NyanpasuAppConfig)`，托盘菜单按旧的 `tray_menu_close_behavior` 回答，其他种类为否；#5649 把它换成 `close_override(&WindowCloseSettings)`（§3.8）。`apply` 在运行时才读配置，所以设定改动不需要 effect。

托盘菜单定位需要在显示前完成。`TrayMenuWindow` 带着光标位置，在 `on_created` 钩子里为新窗口定位，早于“已建好”这一事实；已存在的窗口则在 `open` 之前先 `set_position`，以免在旧位置闪现。

### 3.4 热键切换

热键切换只读 `is_wanted(main)`：为真则 `close`，否则 `open`。它在异步一侧运行并被等待，所以连按两次不会交错；加载中的窗口也算 `wanted`，所以连按两次以关闭收场。`exit.rs` 保存几何的条件是“主窗口存在”，隐藏的主窗口仍保存它的几何。

### 3.5 兜底计时器

窗口建好且尚未 ready 时，用 `tauri::async_runtime::spawn` 起一个任务：睡 10 秒，然后按 ready 报告处理并记录 `warn`。任务的 `AbortHandle` 存进条目，`ready` 或 `remove` 时 `abort`；条目已 ready 或已消失时 `set_watchdog` 拒绝存放，由调用方终止任务。起初设计里给计时器配的任务号检查已去掉，接受下面的残留风险（§9）。

`resolve.rs` 里的 `spawn_window_ready_timeout` 已删除。

### 3.6 RPC 与前端

后端在 `ipc.rs` 新增以下命令，并在 `specta_export.rs` 注册为 mutation。它只在桌面端可用，没有 `http`。

```rust
#[nyanpasu_macro::rpc]
#[tauri::command]
#[specta::specta]
pub async fn report_window_ready(
    windows: State<'_, WindowManager>,
    webview: Webview,
) -> Result<()> {
    windows.report_ready(webview.label());
    Ok(())
}
```

label 取自调用方的 webview，不接受前端传入，前端无法替其他窗口报告 ready。参数类型是 `Webview` 而不是 `WebviewWindow`：宏识别两者，但生成的处理函数传入的是 `::tauri::Webview`（`nyanpasu-macro/src/unified_command.rs:198-201`），声明为 `WebviewWindow` 会类型不符。

`create_main_window`、`create_debug_tray_menu_window`、`create_editor_window` 改为 `async` 命令，直接调用 `WindowManager`，删除开线程睡 10 ms 的写法。托盘的点击处理与热键切换同样不在主线程上建窗：托盘处理 `spawn` 到异步运行时，热键切换在异步一侧运行并被等待。

前端 `__root.tsx` 的 `WindowReveal` 改为 `WindowReadyReporter`：在 Tauri 环境中，设置查询结束（成功或失败）后，每次页面加载调用一次 `rpc.reportWindowReady()`，失败只记 `console.error`。错误边界 `Catch` 渲染时也调用一次，让出错的窗口立即可见，而不是等 10 秒兜底；重复的 ready 对后端是空操作。

删除的部分：

- `window.rs:291-297` 的 `WindowReadyEvent`；
- `specta_export.rs:21`、`:188` 的注册；
- `unified_rpc.rs:37` 的事件名；
- `resolve.rs:158-167` 的监听；
- 重新生成的绑定中对应的类型。

`frontend/rpc/tests/event-transport.test.ts:134-135` 只是借用这个事件名来测“HTTP 拒绝 emit”，改用仍然存在的 `window-message-event` 即可。

### 3.7 macOS

- `resolve_setup` 中 `set_activation_policy(Accessory)` 保持不变。
- Dock 图标随主窗口显示而出现（`apply` 的显示步骤），随主窗口被销毁或隐藏而消失（`on_dismissed`）。这与现有行为一致，只是改由 `WindowManager` 负责。最小化的窗口在 macOS 上 `isVisible` 为假，所以 `apply` 以“可见或已最小化”判定是否已展示，否则热键关闭最小化的主窗口会漏掉 `on_dismissed` 和隐藏。隐藏（`orderOut`）是否同时移除 Dock 里的最小化缩略图未能验证，列入 macOS 冒烟清单。
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
- `WindowCloseSettings` 派生 `Patch`，`window_close` 字段用 `#[patch(nesting)]`，与 `ClashConfig.mixed_port` 相同。补丁只写出要改的子字段，合并发生在串行的配置所有者里，所以两次改不同字段的编辑（来自同一页面或两个页面）不会互相覆盖。没有为此新增 RPC。生成的序列化形式的补丁类型要求嵌套补丁，所以 `useSettings` 的补丁类型写作 `Partial<NyanpasuAppConfigPatch_Serialize>`。
- `window_close` 带 `#[serde(default)]`：旧版本写出的文件没有这个键，缺少默认值会让它们加载失败。
- 改动不影响运行时，`impact.rs` 的用例按 `RuntimeImpact::None` 改写；补丁只涉及已具名的字段，`window_close` 不触发任何 effect。`apply` 在运行时读取 `app_config_snapshot()`，所以设定改动不需要 effect。
- “继承全局”用显式的 `Inherit` 变体，而不是 `Option`。这样 YAML 和前端下拉框里都能看到这个选项，也避开 struct-patch 对 `Option` 字段的双层包装。

窗口种类通过 `AppWindow` 的方法（并经 `WindowHooks` 暴露给 `apply`）取用自己的设定：

```rust
fn close_override(&self, settings: &WindowCloseSettings) -> WindowCloseOverride;
```

主窗口返回 `settings.main`，两种编辑器返回 `settings.editor`，托盘菜单（包括调试用的常驻托盘菜单）返回 `settings.tray_menu`。

#### 关闭处理

`apply` 对不再应显示的窗口计算有效模式 `kind.close_override(&settings).resolve(settings.global)`：

- `Destroy`：调用 `destroy()`，随后收到 `Destroyed`，从表中移除。
- `Hide`：窗口已展示时调用 `hide()`。窗口仍以已建好、已 ready、不再 `wanted` 的状态留在表里，下次 `open` 时被优先选中并重新显示（§3.2）。

`on_dismissed`（保存几何、macOS 隐藏 Dock 图标）在两种模式下都在窗口已展示时执行。

所有关闭入口都收敛为“窗口不再应显示”：

- 托盘菜单失焦处理不再读设定，直接调用 `close`。
- 前端托盘菜单的点击处理（`hooks.ts`）不再读设定：先执行动作，最后 `close()`。动作的 IPC 在窗口可能被销毁之前发出；不先 `hide()`，因为隐藏会让菜单失焦，后端会随即关闭它。菜单在动作执行期间保持可见，这一取舍已接受。
- 其他前端入口原本就调用 `close()`，它与标题栏、Alt+F4 一样是用户的关闭，经 `CloseRequested` 变成同一件事。

热键切换见 §3.4。

退出应用时，Tauri 直接销毁窗口，不经过 `CloseRequested`，所以隐藏模式不会阻止退出。

#### 配置迁移

应用真正读写的配置文件是 `application.yaml`（`ApplicationClient`），不是 `nyanpasu-config.yaml`：后者是 2.0 前的遗留文件，只作为 `typed_config` 迁移的输入，`app_config` 模块只改它。`app_config` 是冻结的启发式模块（`migration/mod.rs` 的 `ModuleKind` 说明，`registry.rs` 的冻结测试），不能再加步骤。所以：

- #5646 新增 Document 类型的迁移模块 `application`，管理 `application.yaml`，并像 `profiles.yaml`（#5364）一样盖章：应用经 `StampedYamlFormat` 读写；`unstamped_ceiling` 为 0；`typed_config` 创建文件时直接盖在模块当前的 revision，所以新装和 1.x 转换的结果落在正确的 revision，没有“头部 revision 却没有戳”的窗口。#5646 本身没有步骤。
- #5649 加入步骤 `application/window_close`（revision 1）：如果存在 `tray_menu_close_behavior`，就写入 `window_close.tray_menu`，按 `hide → hide`、`close → destroy` 转换，其余字段取默认值，删除旧键，与盖戳在同一次原子写入里完成。未知的旧值使步骤失败。
- legacy 1.x 迁移（`legacy_schema/application.rs`）改为写入 `next.window_close.tray_menu`。

#### 设置界面

设置页 `settings/nyanpasu` 中的 `TrayMenuCloseBehaviorSelector` 换成独立的“窗口关闭方式”区块（`WindowCloseBehaviorCard`），标题用 `settings_nyanpasu_window_close`：

- 一项全局选择：销毁 / 隐藏；
- 主窗口、编辑器、托盘菜单各一项：跟随全局（括号中显示解析后的值）/ 销毁 / 隐藏。托盘菜单一项仍然只在 WebView 托盘模式下显示。
- 说明文案写明取舍：隐藏保留 webview，再次打开更快，但占用内存；设定在窗口下次关闭时生效。
- 每个选项调用 `useSetting('window_close').upsert({ main: 'hide' })`，只带改动的子字段（嵌套补丁，见上）。卡片里没有锁、重新拉取或禁用状态。
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

（行号为起草时的 `window.rs` 与 `resolve.rs`，实现后这些代码已迁到 `window/` 目录。）

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

| 调用方                                         | 改为                                                                                                                    |
| ---------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `core/tray/mod.rs:525`、`:554` 打开窗口        | `WindowManager::open(&MainWindow, None)`                                                                                |
| `core/tray/mod.rs:564` 托盘菜单                | `kinds::show_tray_menu_window`（§3.3）                                                                                  |
| `client/hotkey/adapters.rs:194-197` 切换主窗口 | `is_wanted` / `close` / `open`（§3.4）                                                                                  |
| `lib.rs:337`、`:344` 深链                      | `open(&MainWindow, None)`                                                                                               |
| `lib.rs:363-385` 主窗口事件                    | 关闭钩子移入窗口种类的 `on_dismissed`；`ScaleFactorChanged`（托盘）与 `connection_details::cancel_for_webview` 留在原处 |
| `utils/exit.rs:235-237`                        | 主窗口存在即保存几何（§3.4）                                                                                            |
| `resolve.rs:218-221` 启动创建                  | `open(&MainWindow, None)`                                                                                               |

## 4. 去掉透明后的预期与风险

预期：Windows 主窗口打开时，私有工作集减少约 30 MB（报告 §2 的 softbuffer 行，与窗口尺寸成正比）。窗口关闭后不受影响。

风险与对策：

- **显示前的背景色：** 窗口在前端渲染完成后才显示，首帧就是前端画面，所以不需要 `background_color`。
- **拖拽缩放时边缘露底：** 深色主题下，WebView2 尚未重绘的区域可能闪过白色。如果冒烟测试复现，再按主题调用 `set_background_color`（Tauri 2.12.1 提供）。本 spec 不预先加入。
- **无边框窗口的外观：** Windows 11 上 `set_shadow(true)` 提供圆角与阴影，与是否透明无关；Windows 10 一直是直角。冒烟测试时核对两者。

## 5. 测试

`table.rs` 的纯单元测试（不依赖 Tauri，不用 sleep）：

- 排序无关性：用一个带“已排队但未运行的 `apply`”和假原生窗口（不存在、隐藏、展示、最小化）的模型，枚举 built、ready（两次，即兜底计时器）、open、close（两次，即用户的关闭与托盘失焦）、minimize 的所有不同顺序，并在每个位置选择是否运行一次排队的 `apply`，最后清空队列。断言：原生窗口展示当且仅当 `built && ready && wanted`；`on_dismissed` 恰好在被展示的窗口退场时执行；首次 ready 的处理恰好一次。
- 为被取代的 label 排的 `apply` 不会显示尚未建好或尚未 ready 的新条目；先关闭再打开，在 `apply` 运行前以 `wanted` 收场且不退场。
- 最小化的窗口像展示的窗口一样被通知 `on_dismissed`，从未展示的不会。
- 建好之前的关闭，在建好后得到处理；销毁再重建同一 label 从空白事实开始；热键切换跟随 `wanted`；单例只打开一次，非单例取最小空闲编号。
- 隐藏模式：`open` 优先选不再应显示的已建好实例，都在应显示时才新建。
- 兜底计时器：`ready`、`remove` 停止已存放的计时器；已 ready 或已消失的窗口不接受计时器。

`window_close.rs`：`WindowCloseOverride::resolve` 的三种取值，`WindowCloseSettings::default()` 全部解析为销毁，嵌套补丁只改它列出的字段。`ApplicationClient` 的测试：两次改不同字段的 `window_close` 补丁合并，且不开启运行时事务（回执没有操作号）。

迁移：

- `application` 模块：新装得到盖了戳的文件且不跑步骤；2.0.x 未盖戳的文件被转换并盖戳；盖在 revision 0 的文件从戳继续迁移；状态文件丢失时用戳代替探测；丢失戳的文件被拒绝；1.x 转换的旧托盘选择带入新配置。
- `application/window_close`：`close → destroy`、`hide → hide`，旧键被删除，没有旧键的配置不变，未知值被拒绝。

`WindowManager` 直接操作 Tauri 窗口，不写依赖 Tauri 运行时的测试，由 §6 的冒烟测试覆盖。

前端：现有测试不覆盖 `__root`，没有为 `reportWindowReady` 新增测试。

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
    - 用 2.0.x 的配置启动，`tray_menu_close_behavior` 被正确迁移，`application.yaml` 被盖戳。
    - 热键连按两次（含加载期间）以窗口关闭或隐藏收场；打开后立即关闭再打开，窗口保持打开。

macOS 需要你在本机测试，我无法在 macOS 上运行：

- 冷启动后 Dock 图标出现；关闭主窗口后 Dock 图标消失；点 Dock 图标重新打开主窗口。销毁和隐藏两种模式各测一次。
- 最小化的主窗口被热键关闭：几何被保存、Dock 图标消失；隐藏模式下 Dock 里的最小化缩略图是否随 `hide()` 消失（未能验证）。
- 红绿灯位置，以及全屏的进入和退出。红绿灯现在经 `run_on_main_thread` 在建窗之后设置，因为命令可能在任意线程建窗。

Linux：启动、托盘打开、编辑器窗口各测一次，看有无回归。

## 7. 提交拆分与 PR

实现最终分成四个 draft PR（起初计划的两个 PR 与 9 个提交因下列原因调整）：

| PR    | 分支                         | 内容                                                                                                                                                                              |
| ----- | ---------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| #5646 | `feat/application-document`  | 1 个提交：`application.yaml` 作为盖戳的 Document 迁移模块（§3.8）                                                                                                                 |
| #5647 | `refactor/window-manager`    | 5 个提交：窗口管理层与 `report_window_ready`；async 创建窗口命令；首次 ready 时关闭滑动导航；去掉透明；托盘只在松开时响应（§3.10）                                                |
| #5648 | `refactor/delay-workarounds` | 1 个提交：删除两处 `sleep(300)`（已确认 mutation 在运行时应用并发布产物之后才返回）。导入按钮的 150 ms 未改：等的是 `AnimatedOutlet` 里约 0.35 s 的路由滑动，没有可等待的完成信号 |
| #5649 | `feat/window-close-modes`    | 叠在 #5647 上并带着 #5646 的提交：关闭模式（配置类型与嵌套补丁、`application/window_close` 迁移、`close_override`、设置区块与 i18n、隐藏实例复用）合为一个提交                    |

调整的原因：`app_config` 迁移模块已冻结，需要先把 `application.yaml` 变成盖戳的 Document 模块（#5646）；起初的“配置提交”与“关闭模式提交”单独看，前者让设置界面提供了后端尚不理会的选项，所以合并为一个提交；#5647 单独合入也必须自洽，所以托盘菜单的旧设定在第一个提交里就经 `hides_on_close` 走管理层。

## 8. 决策（2026-10-06 已定）

1. **兜底计时器：** 每个新建窗口一份，10 秒（§3.5）。
2. **macOS Dock 策略：** 保持现状，Dock 图标只跟随主窗口。
3. **窗口种类迁出 `resolve.rs`：** 迁到 `window/kinds.rs`（§3.1）。
4. **ready 的判定条件：** 沿用“设置查询结束”。
5. **关闭模式的默认值：** 全部为销毁（§3.8）。
6. **设定改动对已隐藏窗口的影响：** 下次关闭时才生效。
7. **隐藏模式下复用编辑器的状态：** 不处理。
8. **CEF 预留的范围：** 只落实 §3.9 的约束 1–4，入口分流只作记录。

实施期间追加的决定（用户裁定）：

9. **迁移放进新的 `application` Document 模块**（§3.8），而不是 `app_config` 的 revision 5；先单独做 #5646。
10. **生命周期改为电平触发的事实表加幂等 `apply`**（§3.2–§3.4），取代起初的边沿触发设计（§9）。
11. **设置区块用嵌套补丁，不新增 RPC**（§3.8）。

## 9. 实现状态、偏差与已接受的风险

**状态：** 已实现，见 §7。冒烟测试（§6）由人在开发版上执行；macOS 与 Linux 未在本地编译或运行。

**相对本 spec 初稿的偏差：**

- `report_window_ready` 的参数是 `Webview`，不是 `WebviewWindow`（§3.6）。
- `WindowManager::open` 对窗口种类是泛型 `open<K: AppWindow>`，不是 `&dyn AppWindow`：`AppWindow` 要求 `Clone + Send + Sync`，管理层把一个克隆放进窗口事件监听器，并把对象安全的 `WindowHooks` 存在条目里。
- `AppWindow` 新增 `on_created`（构建后、可显示之前完成设置，例如托盘菜单定位）和 `on_dismissed`（窗口退场前的通知）；`WindowTable` 带泛型 `H`。
- 管理层永远不调用原生 `close()`；`CloseRequested` 在两种模式下都被拦截并转成“不再应显示”（§3.3）。
- 迁移走 `application` Document 模块，不是 `app_config` revision 5（§3.8）。
- 设置区块是嵌套补丁，没有新增 RPC；卡片是独立区块（§3.8）。
- 托盘点击与热键切换不再在主线程上建窗（§3.6）。
- 提交与 PR 的拆分（§7）。

**已接受的残留风险：**

- **F2**：窗口建好时按 label 处理；一个仍隐藏的窗口，在“事件到达与处理”的间隙里被销毁，且同名窗口被重建，可能被处理两次。除管理层之外没有谁会关闭一个隐藏的窗口，所以不为它引入代次令牌。
- **过期的兜底计时器**：没能及时停止的计时器任务如果在同名窗口重建之后触发，最多让新窗口提前显示，仅是外观问题。
- **过期的 ready 报告**：被销毁的 webview 在其 label 被复用后发来的 ready 会被接受，因为 `report_window_ready` 标识的是窗口而不是它的化身。
- **销毁模式下，已排队的 `destroy()` 与 `Destroyed` 之间的打开会丢失**：窗口只有约一次事件循环的长度，再按一次即可。不在 `Destroyed` 时重建，因为应用退出也会销毁窗口。

**评审历史（简记）：** 起初的版本是边沿触发的窗口表（`Phase`、`Admission`、`ReadyOutcome`、`close_pending`、任务号、`keep_hidden` 等），评审多轮都在其上发现事件顺序问题，每次修补都多一个标志或结果。2026-10-06 改为现在的电平触发模型：只记事实，`apply` 幂等并读取当时的事实，再按 tauri-runtime-wry 的源码约束去掉原生 `close()` 并让 `apply` 总是排队。之后又收紧了三点：托盘菜单的失焦在第一个提交里就经管理层、最小化窗口也算已展示、以及用带排队的模型测试代替“每个事件后立刻应用”的排序测试。
