# AppContainer 回环豁免工具 `enable-loopback`

**日期：** 2026-10-19

**状态：** 已实现（2026-10-20）。实施中因 reactor 限制做的调整记录在 §4.2、§4.5、§6.0–§6.2，fork 补丁见 issue #2；真实指针、键盘与外观的手动冒烟清单在 `docs/manual-smoke.md`。

**基线：** 主仓 `main@d72ad2af0`；被替换的工具为 `backend/tauri/resources/enableLoopback.exe`（84 040 字节，.NET Framework 4 WinForms，来自 `Kuingsmile/uwp-tool` 的 `latest` release，无源码）；UI 框架 `windows-reactor`（microsoft/windows-rs，WinUI 3），crates.io 最新 0.100.0，master `a29e064`（2026-10-19）。

**需求：** 在 `https://github.com/libnyanpasu/enable-loopback.git` 新建 Rust 实现，用 `windows-reactor` 搭 WinUI 3 界面；以 `runas` 提权；支持 i18n；功能完整复刻原工具。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec。

**行为来源：** 以 `ilspycmd` 反编译原 exe 得到的源码为准（`frmMain.cs`、`Program.cs`、`IsolationAPI/*.cs`），下文 §3 每条都可在反编译源码里找到对应。

## 1. 决策与范围

1. **仓库与布局（已定）**：独立仓库 `libnyanpasu/enable-loopback`，以 git submodule 挂在主仓 `backend/enable-loopback`（与 `backend/nyanpasu-runtime` 同级）。子仓是自己的 cargo workspace，唯一成员 `crates/enable-loopback`；主仓 `backend/Cargo.toml` 把它加入 `exclude`（与 `nyanpasu-runtime` 同理）。产物落地文件名保持 `enableLoopback.exe`，主仓调用方只需追加 `--lang`（见 §8）。
2. **crate 内分层**：
   - `netiso`：FirewallAPI 封装，纯数据进出，不依赖 UI；
   - `model`：列表状态、脏标记、排序、复制格式化、命令行过滤规则等纯逻辑，可用普通值测试；
   - `ui`：`windows-reactor` 组件；
   - `cli`：命令行模式；
   - `i18n`：文案。
3. **提权**：沿用主仓现有方式——`backend/tauri/src/core/win_uwp.rs` 用 `runas` 启动，进程本身已提权。exe 清单保留原工具的 `requestedExecutionLevel level="highestAvailable"`，让用户直接双击运行时也能得到管理员令牌。不做自提权、不做 helper 进程。
4. **不做的**：新增功能（批量清理孤儿等；工具栏计数见 §4.2，已批准的搜索扩展见 §10）、Windows 8 版本检查（WinAppSDK 2.x 最低就是 Windows 10 1809，检查已无意义）、x86 构建。只发布 x64 与 arm64。
5. **原工具的 Fiddler 品牌**（`Learn more` 指向 `api.getfiddler.com`、悬停显示 Progress 版权）要替换，见 §3.9。
6. **修正原工具的保存缺陷**（已定）：见 §3.5。除此以外的行为照原样复刻。

## 2. 系统接口

全部为用户态 API，**不需要内核驱动**。

| 用途              | API                                                                                                    | 备注                                                                                                          |
| ----------------- | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| 枚举 AppContainer | `NetworkIsolationEnumAppContainers(flags, &count, &list)`                                              | 首次加载 `flags = 0`；手动刷新 `flags = NETISO_FLAG_FORCE_COMPUTE_BINARIES (1)`（与原工具一致）               |
| 释放枚举结果      | `NetworkIsolationFreeAppContainers`                                                                    | 用 RAII guard 保证释放                                                                                        |
| 读取豁免名单      | `NetworkIsolationGetAppContainerConfig(&count, &sids)`                                                 | 返回 `SID_AND_ATTRIBUTES[]`，用 `LocalFree`/`HeapFree` 释放（需核对文档，原工具用 `FreeHGlobal`=`LocalFree`） |
| 写入豁免名单      | `NetworkIsolationSetAppContainerConfig(count, sids)`                                                   | **整表覆盖**；返回 5 (`ERROR_ACCESS_DENIED`) 表示未提权                                                       |
| 解析资源字符串    | `SHLoadIndirectString`                                                                                 | 仅当字符串以 `@` 开头；失败则保留原文（截图里大量 `@{...}` 就是解析失败的原文）                               |
| SID → 账户名      | `LookupAccountSidW`                                                                                    | 结果为 `本机名\用户` 时去掉本机名前缀；失败则显示 SID 字符串                                                  |
| SID ↔ 字符串      | `ConvertSidToStringSidW` / `ConvertStringSidToSidW`                                                    | 注册表回退模式用                                                                                              |
| 注册表回退        | `HKCU\Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppContainer\Mappings` | 子键名为 AC SID，值 `DisplayName`、`Moniker`                                                                  |

绑定来源：`windows` crate 的 `Win32_NetworkManagement_WindowsFirewall`、`Win32_Security`、`Win32_UI_Shell`、`Win32_System_Registry`。具体 feature 名实施时以 docs.rs 为准。

### 2.1 数据模型

```rust
struct AppContainer {
    sid: Sid,                     // 主键；按 SID 合并多用户
    users: Vec<String>,           // 排序后显示为 "a; b"
    name: Option<String>,         // appContainerName
    display_name: Option<String>, // 已解析
    description: Option<String>,  // 已解析
    package_full_name: Option<String>,
    binaries: Vec<String>,
    exempt_at_last_check: bool,
}
```

- 同一 SID 对每个用户各返回一条记录，按 SID 合并，`users` 追加后排序（原工具行为）。
- 缺失字段的占位文案（需 i18n）：`(No DisplayName)`、`(No Description)`、`(No Package Name)`、`(No AC Name)`、`(No AC SID)`、`(No User SID)`、Binaries 为空时 `(None)`。注意：原工具只对 `null` 用占位，空串照原样显示为空（截图第一行 DisplayName 为空即此情况），复刻此差别。
- 注册表回退模式下：`description`、`package_full_name` 为 `?`，`users` 为当前用户 `域\用户名`，`binaries` 为空。

## 3. 功能细节（复刻清单）

### 3.1 启动

| 条件                                                              | 行为                                                                     |
| ----------------------------------------------------------------- | ------------------------------------------------------------------------ |
| 首个参数（去掉前导 `/`、`-`，小写）为 `all`/`none`/`allow`/`deny` | 进入命令行模式（§3.8），不显示窗口                                       |
| 首个参数含 `?`                                                    | 弹出语法说明，退出码 `-10`                                               |
| 已有实例在运行（命名互斥量 `EnableLoopbackTool`，等 250 ms）      | 找到其窗口则置前并退出（`-3`）；找不到则提示"已在运行但无法切换"（`-5`） |
| 命令行含 `manual`                                                 | 打开窗口但不自动刷新                                                     |
| 命令行含 `-registry`                                              | 刷新时跳过 API，直接读注册表                                             |
| 其他                                                              | 打开窗口并自动刷新（不计算 binaries）                                    |

互斥量名沿用 `EnableLoopbackTool`，这样新旧工具互斥。置前方式见 §4.1。

### 3.2 刷新

1. 若有未保存修改：确认框"尚未保存修改，仍要刷新吗？"，默认按钮为"否"。
2. 清空列表，禁用全部按钮与列表，状态栏"正在刷新 AppContainer 信息…"，显示进度指示。
3. 后台线程（`spawn_background`）执行：
   - 非 `-registry`：调用枚举 API。失败时提示"无法用 NetIsoEnumAppContainers 获取列表，错误码 0x…，将改为读取注册表，部分信息不可用"，然后走注册表回退；
   - 回退仍失败：报错（见 §3.7）。
   - 读取豁免名单，标记 `exempt_at_last_check`。
4. 豁免名单里有、枚举结果里找不到的 SID 为**孤儿记录**，逐条弹窗（§4.4）。原工具在后台线程弹 MessageBox；新实现在后台收集孤儿列表，回到 UI 线程后依次弹 `ContentDialog`。
5. 完成：填充列表，勾选状态 = `exempt_at_last_check`；状态栏"已于 HH:mm:ss 刷新 AppContainer 信息。"（时间按当前语言的长时间格式）；列表非空时启用列表和"全部豁免/全部取消"；刷新按钮总是重新启用；焦点移到列表。

### 3.3 勾选与脏状态

- 勾选框即"是否豁免"。
- 任一行勾选状态 ≠ `exempt_at_last_check` 即为脏：启用"保存修改"，状态栏"点击『保存修改』以提交。"；否则禁用保存、清空状态栏。
- 刷新填充期间不触发脏检查（原工具会临时解绑事件）。

### 3.4 全部豁免 / 全部取消

- 按钮与右键菜单均可触发。搜索期间只操作**可见行**（被搜索隐藏的行保持原状），按钮与右键菜单的标签随之改为“豁免全部结果 / 取消全部结果”，状态栏提示改为“所有搜索结果待豁免 / 待移除豁免”；无可见行时两个按钮禁用。计数“已豁免 n / m”始终为全局值。（用户 2026-10-20 决定）
- 全部勾选后若为脏，状态栏"所有 AppContainer 待豁免。点击『保存修改』以提交。"；全部取消对应"所有 AppContainer 待移除豁免。…"。

### 3.5 保存

1. 先禁用保存按钮，防止重复提交。
2. 用所有**已勾选**行的 SID 调用 `SetAppContainerConfig`。
3. 成功：才把各行 `exempt_at_last_check` 更新为当前勾选状态；状态栏"回环豁免配置已更新。"
4. 返回 `ERROR_ACCESS_DENIED`：提示"必须以管理员身份运行才能修改"，状态栏"未保存。需要管理员权限。"
5. 其他错误：提示错误信息（含 `0x…`），状态栏"未保存。"
6. 失败（4、5）时不改 `exempt_at_last_check`，勾选保持用户的修改，列表仍为脏，保存按钮重新启用。

**与原工具的差异（已定修正）**：原工具在调用 API **之前**就改写 `exempt_at_last_check`，失败后列表被误判为干净、保存按钮一直禁用，用户的修改既没写入也无法重试。

**原样保留并在界面说明**：该 API 整表覆盖，保存时孤儿记录会被删除。存在孤儿记录时，保存前的状态栏追加提示"保存将移除 n 条孤立记录"。

### 3.6 列表交互

| 交互                        | 行为                                                                                                                                                                                                                                |
| --------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 点击列头                    | 同一列依次为升序 → 降序 → 不排序（有搜索词时按相关度，否则恢复枚举顺序）；点另一列从升序开始；初始按显示名称升序（同原工具）；比较为**序数**比较（`Ordinal`）；排序后滚动到首个选中行。原工具只在升降之间切换，三态为用户要求的改动 |
| 多选                        | 原工具支持（`FullRowSelect` + 多选）；reactor 无选中集合回调，自建选中模型，见 §6.2                                                                                                                                                 |
| `Ctrl+A`（焦点在列表）      | 全选当前可见（筛选后）的行                                                                                                                                                                                                          |
| `Ctrl+C` / 右键"复制选中行" | 每行：`LoopbackExempt` 或 `LoopbackBlocked` + 7 列，用 `\t` 分隔；选中多于一行时每行以 `\r\n` 结尾，只有一行时不加换行                                                                                                              |
| 右键"复制选中列"            | 取右键所在列，复制每个选中行该列的值，每行以换行结尾                                                                                                                                                                                |
| 右键菜单                    | 复制选中行、复制选中列、分隔线、全部取消、全部豁免                                                                                                                                                                                  |
| 当前用户的行高亮            | `users` 含当前 `域\用户名` 的行用高亮底色；高对比度模式下换用系统色                                                                                                                                                                 |
| 关闭窗口                    | 有未保存修改时确认"尚未保存，仍要退出吗？"，默认"否"                                                                                                                                                                                |

`复制选中行`的状态前缀 `LoopbackExempt`/`LoopbackBlocked` 是机器可读格式，**不翻译**。

### 3.7 错误提示

| 情况                                     | 状态栏                               | 弹窗                                                                     |
| ---------------------------------------- | ------------------------------------ | ------------------------------------------------------------------------ |
| 枚举错误码 1753 (`EPT_S_NOT_REGISTERED`) | "获取 AppContainer 信息失败 [0x6d9]" | 标题"Windows 防火墙已停用"，说明第三方安全软件可能停用或替换了防火墙服务 |
| 其他枚举错误                             | 同上格式                             | "无法获取本机的 AppContainer 列表" + 详情                                |
| 其他异常                                 | "失败：{message}"                    | "操作失败" + 详情                                                        |

### 3.8 命令行模式

| 参数                     | 行为                                                                      |
| ------------------------ | ------------------------------------------------------------------------- |
| `-all`                   | 豁免所有 AppContainer                                                     |
| `-none`                  | 清空豁免名单（直接写空表，不枚举）                                        |
| `-allow NAME`            | 新名单 = 已豁免 ∪ AC 名为空 ∪ AC 名包含 NAME（忽略大小写）                |
| `-deny NAME`             | 新名单 = 已豁免 且（AC 名为空 或 AC 名不含 NAME）。`-deny *` 等同 `-none` |
| `-allow`/`-deny` 缺 NAME | 提示"必须指定部分名称，用 * 匹配全部"，退出码 `-1`                        |
| 无法枚举                 | 写入调试输出（`OutputDebugString`），退出码 `-10`                         |
| 其他失败                 | 调试输出，退出码 `-1`                                                     |
| 成功                     | `0`                                                                       |

原工具的解析是"在拼接后的整条命令行里找子串"，因此 `-allow` 会优先于 `-all` 命中。新实现改为按参数逐个解析，但对上述合法输入结果一致；`-allow` 中 NAME 允许包含空格（剩余参数用空格拼接）。

命令行模式不创建窗口，也不初始化 WinAppSDK——即使机器上没有 Windows App Runtime，命令行模式也能用。

### 3.9 帮助与版本

- `F1` 或 `Alt+L`：打开"了解更多"链接。
- 链接目标（已定）：Microsoft Learn《How to enable loopback and troubleshoot network isolation (Windows Runtime apps)》，`https://learn.microsoft.com/previous-versions/windows/apps/hh780593(v=win.10)`。URL 不带语言段，Learn 按浏览器语言自动跳转本地化版本（2026-10-19 验证 en-us、zh-cn 均返回 200）。该页属于"以前的版本"归档，但仍是微软对回环豁免与 `CheckNetIsolation` 的正式说明。
- 悬停链接时状态栏显示 `enable-loopback v{版本} {64bit|arm64} {系统版本}`，替代原 Progress 版权。

## 4. UI 设计

### 4.1 窗口

- 标题：本地化（已定）"AppContainer 回环豁免工具"。
- 图标（已定新绘）：按 Fluent Design 的 Windows 11 应用图标风格（无底板、几何隐喻、形状内柔和渐变、轻微层叠阴影）手绘 SVG，先由 codex 出概念图再定稿，源文件 `crates/enable-loopback/assets/icon.svg` 入库。另备一份 16px 简化版 SVG，保证小尺寸可辨识。构建时由 `build.rs` 用 `resvg` 栅格化为 16/20/24/32/48/256 多尺寸 `.ico`，通过资源脚本嵌入 exe（资源管理器与任务栏图标）。窗口图标不用 reactor 的 `icon(path)`（它调用 `AppWindow::SetIcon`，需要磁盘上的 `.ico` 路径），而是在 `run_window` 回调里用 `LoadImageW` 从自身资源加载并 `WM_SETICON`，免得运行时释放文件。具体图形见 §9-1。单实例置前不依赖标题：启动时把本实例的 HWND 写入命名共享内存（与互斥量同名前缀），第二个实例从中读取后置前。这样各语言之间也能互相置前；与旧工具（英文标题）并存时无法互相置前，可接受。
- 初始客户区 960×600 DIP，最小 640×400（原工具 761×352 / 516×270，按 WinUI 控件尺寸放大）。
- 背景 `Mica`，跟随系统亮/暗色；高对比度下使用系统资源。
- DPI 感知由 WinUI 负责。

### 4.2 布局

```text
┌ AppContainer 回环豁免工具 ─────────────────────────────── ─ □ ✕ ┐
│  ┌ ⓘ 说明…                                    了解更多 ┐      │
│  └─────────────────────────────────────────────────────┘      │
│  [🔍 搜索应用       ]                 [⟳ 刷新] [☑ 全部豁免] [☐ 全部取消] [💾 保存修改] │
│  ╭ ☐ │显示名称 ▲│描述│包│AC 名称│AC SID│用户│文件 ───────╮      │
│  │ ☑  row …                                            │      │
│  ╰─────────────────────────────────────────────────────╯      │
│  已于 9:19:18 刷新 AppContainer 信息。                         │
└─────────────────────────────────────────────────────────────┘
```

页面四边统一留白 24 DIP；说明、工具栏、表格卡片和状态文字左右对齐。各区间距统一
`SECTION_SPACING = 12` DIP，工具栏按钮间距 8 DIP。搜索框宽 280 DIP、靠左；窗口宽度小于 900 DIP 时操作按钮移到下一行并靠右（reactor 无法测量文字，900 按最宽的英文按钮约 544 DIP + 搜索框 + 间距 + 页边距估算）。640 DIP 最小窗口下四个按钮仍可排在一行。计数移到状态栏右侧，避免工具栏在默认 960 DIP 宽度下换行。

| 区域           | reactor 控件                                                                                                                                                                                                                | 说明                                                                                                                                                                         |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 说明           | 仿 Informational `InfoBar` 的 `Border`（CardBackground/CardStroke、圆角 4）内一行 `Grid`：状态圆图标（`FontIcon` F136+F13F）+ 自动换行 `TextBlock` + 右侧 `HyperlinkButton`                                                 | 替代原工具的 Label + LinkLabel；a29e064 的 `InfoBar` 没有 Content/ActionButton，链接放不进同一行，故自绘                                                                     |
| 工具栏         | 左侧 `AutoSuggestBox`（查询图标、无建议项），右侧四个 `Button`，内容为 `SymbolIcon` + 文字（Refresh / SelectAll / ClearSelection / Save）                                                                                   | 前三个 `ButtonStyle::Default`，保存 `ButtonStyle::Accent`；独立 Grid 行，不遮盖表头；禁用规则见 §4.3                                                                         |
| 计数           | 状态栏右侧 `TextBlock`，`toolbar.counter` 六语言文案；搜索时追加“· 找到 n 项”                                                                                                                                               | “已豁免 n / m”；n 按当前勾选计算，脏时同步变化                                                                                                                               |
| 表格卡片/表头  | `Border`（CardBackground / CardStroke、1 DIP 边线、圆角 8）包含表头和列表；表头 `Grid` 的 `Button` 右侧显示 10 DIP 的 Segoe Fluent 排序字形（升序 F0AD、降序 F0AE，同 WinUI.TableView，`TextSecondary`），右侧 1 DIP 分隔线 | 表头固定不参与纵向滚动；与行共用水平 ScrollViewer。表头贴住卡片顶边并做 7 DIP 上圆角（Border 不裁剪子元素）；底部内缩 4 DIP 避免末行背景越过圆角；行间为 1 DIP DividerStroke |
| 列表           | `ListView` + `DataItem`（WinUI 原生容器虚拟化），行内容为与表头共用组件状态列宽的 `Grid`：`CheckBox` + 7 个 `TextBlock`（单行、尾部省略、`tooltip` 显示全文）                                                               | 保留默认 ItemsStackPanel，有限高度的 Grid/仅水平滚动的 ScrollViewer 不破坏纵向虚拟化；DataItem 内容由 reactor 预建，容器仍由 WinUI 按需实现                                  |
| 状态栏         | 底部 `TextBlock`；刷新中显示 `ProgressRing`（小号）                                                                                                                                                                         | 使用页面统一边距，无额外内缩                                                                                                                                                 |
| 右键菜单       | `AppContext::show_menu_at(ScreenPoint, Menu)`，WinUI MenuFlyout transient XAML host                                                                                                                                         | `App::run_with` 创建 AppContext，经 Startup 传入组件（仅 UI 线程）；菜单回调发送组件消息；指针窗口 DIP 经窗口 DPI 和 ClientToScreen 转为物理屏幕像素；顺序见 §3.6            |
| 确认/错误/孤儿 | `ContentDialog`                                                                                                                                                                                                             | §4.4                                                                                                                                                                         |

实现核对（a29e064）：`src/generated_declarations.rs:3118–3201` 的 ListView 只有 DataItem items，VirtualSource 仅见于 ItemsRepeater（`:4478`）。`src/native/winui/objects.rs:107–175` 直接创建 WinUI ListView，设置 ItemTemplate 而不替换默认 ItemsPanel；`:627–639`（relations.rs）用 ContentPresenter 展示 DataItem 内容。据 [WinUI 官方性能说明](https://learn.microsoft.com/windows/apps/develop/performance/optimize-gridview-and-listview)，保留虚拟化容器面板即可保留 UI 虚拟化。因此选用 ListView；revision 2 使用 fork 的容器样式与滚动引用，原生选择模式改为 None（§6.2）。列宽总和超过窗口宽度时，表头和列表一起水平滚动。

列宽（DIP，初始值）：勾选 40（固定）、显示名称 220、描述 220、包 200、AC 名称 160、AC SID 140、用户 140、文件 *（剩余，最小 48）。
`ui/table/` 是独立的 reactor 嵌套组件：`component::<TableView>("table", Input)`。Input 为 Clone + PartialEq，含七列的本地化标题/初始宽度/最小宽度/sortable/resizable、`Rc<Vec<Rc<Row>>>`（SID 字符串 key、checkbox、七个 cell 文本、highlight）、Sort、选中 key 集合、焦点 key、键盘焦点来源、enabled，以及 on_sort/on_toggle/on_pointer/on_key/on_context_menu/on_focus 回调。viewport_width、generation、外部 focus_ref 负责布局、原生列表重建和对话框后的焦点恢复。Callback 使用 reactor 的回调身份比较。父组件仅连接业务状态；TableView 保留列宽和拖动，行组件保留 hover/focus 外观，引用与计时器跟随组件生命周期。缩放不重绘 MainWindow。

表头总高 32 DIP（含底部 1 DIP 分隔线），文字 SemiBold，背景 SubtleFillSecondary；行高 40 DIP（含分隔线），文本单元格左右 padding 12 DIP。表头每格右侧和行间均用 1 DIP DividerStroke。容器 padding=0、min_height=40、HorizontalContentAlignment=Stretch，经 fork 的 ItemContainerStyle 应用于当前及后续虚拟化容器；不再补偿默认 16 DIP inset。卡片使用 CardBackground/CardStroke、1 DIP 边框和 8 DIP 圆角，无不透明行带。

悬停使用 SubtleFillSecondary；选区使用 SubtleFillTertiary，左侧 3×16 DIP Accent 胶囊（radius 1.5）。当前用户使用 Users 单元格内 TextSecondary 小圆点，和选区同时显示，不改变复制文本；highlight 沿用既有 Row::highlights_user 规则。实际位于行内且来自键盘的焦点显示 2 DIP PrimaryText 边框，离开行后移除。资源由 WinUI 随亮/暗色及高对比度解析。

内部列分隔线两侧各 4 DIP 组成 8 DIP 透明 grip（最后一列在右缘内保留完整 8 DIP），使用 fork 的 SizeWestEast cursor；与排序按钮互为兄弟。Border 捕获指针，按起点计算宽度；释放/失去捕获提交，重复提交无副作用，不触发排序。几何仍在 model/columns.rs 并有单元测试：最小 48 DIP，可配置更大最小值，最后一列填充余宽。260 行、20 次 debug recording-adapter 样本：表头约 11.1 ms/次，全行宽度提交约 20.9 ms/次（均不含原生 XAML 布局），全行更新超过 16.7 ms 帧预算，因此保留表头实时、行在释放/失去捕获后更新。真实 pointer 流畅度由用户手测。

### 4.3 状态与按钮可用性

| 状态         | 刷新           | 全部豁免/取消  | 保存 | 列表                |
| ------------ | -------------- | -------------- | ---- | ------------------- |
| 刷新中       | 禁用           | 禁用           | 禁用 | 禁用 + ProgressRing |
| 已加载、干净 | 可用           | 列表非空时可用 | 禁用 | 可用                |
| 已加载、脏   | 可用（需确认） | 可用           | 可用 | 可用                |
| 加载失败     | 可用           | 禁用           | 禁用 | 禁用                |

### 4.4 对话框

| 对话框       | 标题                 | 正文                                                                                                                            | 按钮（默认）    |
| ------------ | -------------------- | ------------------------------------------------------------------------------------------------------------------------------- | --------------- |
| 刷新丢弃修改 | 放弃修改？           | 尚未保存修改，仍要刷新吗？                                                                                                      | 刷新 / **取消** |
| 退出丢弃修改 | 不保存就退出？       | 尚未保存修改，仍要退出吗？                                                                                                      | 退出 / **取消** |
| 孤儿记录     | 发现孤立的豁免记录   | 存在 SID 为 `{sid}`（可选择文本）的回环豁免，但找不到对应的 AppContainer。 + 复选框"本次会话中继续提示其他孤立记录"（默认勾选） | **确定**        |
| 需要管理员   | 未保存修改           | 必须以管理员身份运行本工具才能修改 AppContainer 配置。                                                                          | 确定            |
| 防火墙停用   | Windows 防火墙已停用 | 见 §3.7                                                                                                                         | 确定            |
| 通用错误     | 操作失败 / 保存失败  | 详情                                                                                                                            | 确定            |

孤儿对话框把原工具的"是/否"问句改成复选框，语义不变（取消勾选 = 原工具选"否"）。

### 4.5 键盘

| 键                | 作用                                                                                                                                                        |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `F5`              | 刷新（原工具为 `Alt+R` 助记符；a29e064 未暴露 AccessKey（所有生成声明及 visual_methods 中均无该方法），根 Border 显式处理 Alt+R/A/N/S/L，不显示访问键提示） |
| `Ctrl+S`          | 保存（新增，替代 `Alt+S` 助记符）                                                                                                                           |
| `Ctrl+F`          | 聚焦搜索框                                                                                                                                                  |
| `Esc`（搜索框内） | 清空搜索词，保留搜索框焦点                                                                                                                                  |
| `Ctrl+A`          | 列表内全选筛选后的行；搜索框内保留原生文本全选                                                                                                              |
| `Ctrl+C`          | 复制选中行                                                                                                                                                  |
| `Space`           | 切换焦点行勾选（列表 Border 处理；CheckBox 焦点保留原生 Space 行为）                                                                                        |
| `F1` / `Alt+L`    | 了解更多                                                                                                                                                    |

## 5. i18n

- 语言：与主应用一致的 `en`、`zh-cn`、`zh-tw`、`ja`、`ko`、`ru`。
- 方案：`rust-i18n`（主仓 `backend/tauri` 已用 v4），文案放 `crates/enable-loopback/locales/*.json`，键按 `window.*`、`toolbar.*`、`column.*`、`status.*`、`dialog.*`、`cli.*`、`placeholder.*` 分组。
- 语言选择顺序：`--lang <code>` 参数（主仓调用时传入当前应用语言）> 系统 UI 语言（`sys-locale`，主仓 `nyanpasu-helper` 已用）> `en`。
- 不翻译：`LoopbackExempt`/`LoopbackBlocked`、SID、包名、错误码。
- 时间格式：状态栏时间用系统长时间格式（`GetTimeFormatEx`），与语言设置一致。
- 主仓需同步：`win_uwp.rs` 调用时追加 `--lang`。

## 6. windows-reactor 能力核对与缺口

核对对象为 crates.io `windows-reactor 0.100.0` 与 windows-rs master `a29e064`（2026-10-19）源码。

| 需求              | 0.100.0                                                                                                                             | master `a29e064`                                                                                                                               | 处理                                                                                                                                                                                                             |
| ----------------- | ----------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 运行时依赖        | 框架依赖：要求已安装 **Windows App Runtime 2.4**（`Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe`），缺失时弹英文 MessageBox 引导下载 | 同左；另有 `windows-reactor-setup`（crates.io 0.100.0）支持自包含部署                                                                          | 见 §8.1 与 §9-1                                                                                                                                                                                                  |
| 表格              | 无 DataGrid                                                                                                                         | 同左（保留原生 ListView 容器虚拟化）                                                                                                           | 应用内可复用 TableView，业务选择在父组件（§4.2）                                                                                                                                                                 |
| 主题笔刷          | token 不足                                                                                                                          | 同左                                                                                                                                           | **fork 已解决**：[vendor/windows-rs](../vendor/windows-rs) 的 ThemeBrush 增加八个 Fluent token（§6.0）                                                                                                           |
| 分隔线 cursor     | 无                                                                                                                                  | 无                                                                                                                                             | **fork 已解决**：cursor(CursorShape) 经 IUIElementProtected QI 设置 ProtectedCursor；Border 原生运行测试通过。沿用 WinUI 控件/弹出层 cursor 优先规则                                                             |
| ListView 容器外观 | 无                                                                                                                                  | 无                                                                                                                                             | **fork 已解决**：item_container(ItemContainerOptions)，控制 padding/min-height/stretch 并抑制自带 chrome                                                                                                         |
| 滚动到焦点行      | 无公开 API                                                                                                                          | 无公开 API                                                                                                                                     | **fork 已解决**：ElementRef<ListView>::request_scroll_into_view[_result](index, alignment)，已删除自身 UIA 兼容代码                                                                                              |
| 多选              | `ListView` 有 `Multiple`/`Extended` 模式，但 `on_selection_changed` 只回传 `Option<usize>`                                          | 同左；但 `Border` 新增带修饰键的指针与键盘事件                                                                                                 | 拿不到完整选中集合，上游无相关 issue（2026-10-19 检索）。自建选中模型，见 §6.2                                                                                                                                   |
| 右键菜单          | `MenuExt::menu`，回调参数为菜单项文本                                                                                               | `menu`，回调参数为 `Key`                                                                                                                       | 可用；"复制选中列"需要知道右键所在列：行 Border 的局部 x 坐标按当前列宽定位列；Border.menu 仍不可用，改用公开 AppContext::show_menu_at（native/app/mod.rs:123）的 WinUI MenuFlyout；无需 Win32 popup menu        |
| 剪贴板            | 无                                                                                                                                  | 无                                                                                                                                             | 直接用 Win32 剪贴板 API（`OpenClipboard`/`SetClipboardData(CF_UNICODETEXT)`）                                                                                                                                    |
| 关闭拦截          | 只有 `WindowRef::request_close`，无 closing 回调                                                                                    | 仍无 closing 回调；但新增 `ComponentContext::run_window`，在 UI 线程把窗口 HWND（`IWindowNative::WindowHandle`）交给回调，属公开的原生集成入口 | 用 `run_window` 取 HWND 后 `SetWindowSubclass` 拦截 `WM_CLOSE`，见 §6.1                                                                                                                                          |
| 单实例置前        | 无                                                                                                                                  | 无                                                                                                                                             | 互斥量 + 共享内存中的 HWND + `SetForegroundWindow`，与 UI 框架无关                                                                                                                                               |
| 后台任务          | `spawn_background`                                                                                                                  | 同左                                                                                                                                           | 刷新用                                                                                                                                                                                                           |
| 对话框            | `ContentDialog`                                                                                                                     | 同左                                                                                                                                           | 可用                                                                                                                                                                                                             |
| 快捷键            | `KeyAccelerators`                                                                                                                   | 同左（D/O/S/M 键与组合修饰键缺失，见上游 #5087 第 14 项）                                                                                      | `Ctrl+S` 等使用根 Border 的 `on_preview_key_down`（`:2476`，generated_declarations.rs），KeyEventInfo 提供完整 VirtualKey 与修饰键（declaration.rs:1228–1315）；XAML 键盘消息不经过顶层 HWND，子类过程只拦截关闭 |

**版本选择**：使用子模块 [vendor/windows-rs](../vendor/windows-rs) 的 nyanpasu 分支（基于 a29e064）。windows-reactor/windows 均为 path 依赖，Cargo.lock 不再引用旧 git source。子 workspace exclude fork；cargo metadata 的 workspace_members 只有 enable-loopback。CI/release 使用 submodules: true；首次 clone --recursive，已有 checkout 用 git submodule update --init。fork 只有下列四个本地提交，未推送，待负责人 review/push 后随子模块指针发布。

### 6.0 Fork 补丁与验证（2026-10-20）

| 问题 / API                                                                                                                                                                                       | 本地提交 | 上游参考                                                                                                                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| ThemeBrush 缺少表格 token；增加 SubtleFillSecondary/Tertiary、ControlFillDefault/Secondary、DividerStroke、ControlStrokeDefault、TextSecondary、AccentFillSecondary，映射真实 WinUI resource key | 993d79e  | [#5087 第 6 项](https://github.com/microsoft/windows-rs/issues/5087)（现已 closed）                                                                                |
| 无元素 cursor；增加 CursorShape 和共享 cursor 属性，经 QI 设置/清除 ProtectedCursor                                                                                                              | 7aacd86  | [#4988](https://github.com/microsoft/windows-rs/issues/4988)（open；上游提醒部分控件自行管理 cursor，本补丁实测 Border）                                           |
| ListViewItem 默认 inset/高度/对齐/chrome 无法配置；增加 ItemContainerOptions + ListView::item_container，用 XamlReader 加载 ItemContainerStyle 与可选 content-only template                      | 6ac31d0  | [#5087 第 8 项](https://github.com/microsoft/windows-rs/issues/5087)                                                                                               |
| 无公开 ScrollIntoView；增加 ListView 引用与 index/alignment imperative 请求、异步 acceptance 结果，越界返回 Ok(false)，过期绑定返回 Unavailable                                                  | ab90737  | [#5087 第 11/17 项](https://github.com/microsoft/windows-rs/issues/5087)、[#5146](https://github.com/microsoft/windows-rs/issues/5146)（open，重排后原生崩溃报告） |

- **生成与检查**：修改 schema/generator/手写实现后在 fork 根执行 cargo run -p tool-reactor；没有手改 generated 文件。更新 public-api.txt。303 个 reactor unit tests 通过；3 个原生测试分别单独执行通过：Border cursor set/update/clear，真实容器 padding/min-height/stretch/style removal，滚动到第 81 行后新实现容器的样式与 SelectionMode::None 下行 focus。原生测试只启动自己的 WinUI 窗口并调用程序接口，无桌面鼠标/键盘注入。reactor all-targets/all-features clippy -D warnings 通过。
- **排序容器重建**：保留 revision 1 的 ListView 换 key 方案和 SID 选区；未改 fork 的 collection reorder，不保证修复 #5146 的任意原地重排。重建后在 effect 中请求滚动，再等待布局帧恢复行焦点；不再设置 selected_index。
- **滚动与焦点**：ScrollIntoView 接受请求不等于布局完成。应用校验 generation + focused key，再于后续帧发出 ElementRef focus，取消过期计时器。checkbox 焦点不被恢复逻辑抢走；指针选区变化不安排延迟 focus，以免抢走右键菜单。ui/native.rs 的 reveal、COM MTA/UIA 后台任务及仅该路径使用的 Cargo features 已删除。
- **上下文菜单**：继续使用 AppContext::show_menu_at 的 WinUI MenuFlyout。TableView 用 run_window + GetDpiForWindow/ClientToScreen 将事件坐标转为物理屏幕点，回传 key/column/ScreenPoint。父组件先提交选区帧，再弹菜单；列命中使用已提交行宽，不读取全局鼠标位置。MainWindow 销毁时仍通过 AppContext 显式退出。

### 6.1 退出前确认

1. 窗口创建后，组件在 `create` 中调用 `run_window`，对 HWND 安装子类过程，同时把 HWND 写入单实例共享内存。
2. 子类过程收到 `WM_CLOSE`：若组件状态为脏（共享的 `Rc<Cell<bool>>`，UI 线程内读写），吞掉消息并给组件发消息"请求关闭"；否则交给默认处理。
3. 组件弹"不保存就退出？"`ContentDialog`；保存写入期间暂缓关闭以保留写入快照，子类过程在 `WM_NCDESTROY` 移除自身并释放 UI 线程的 Rc 状态。用户选"退出"后置位"已确认"标志并调用 `close_window`，子类过程见到标志即放行。
4. 风险：需要实测 WinUI 3 的标题栏关闭按钮与 `Alt+F4` 都经过 `WM_CLOSE`（AppWindow 的 Closing 事件即由此触发，预期成立）。

### 6.2 自建多选模型

master 的 `Border` 提供 `on_pointer_pressed`（`PointerEventInfo` 含 `modifiers`：Shift/Control，及左右键）与 `on_preview_key_down`/`on_key_up`（`KeyEventInfo` 含按键与修饰键），足以在应用内实现多选，不必改 reactor：

1. `ListView` 设为 `None`，保留原生 ListViewItem、虚拟化和键盘焦点；原生测试确认该模式下行可聚焦。父组件保留 Selection.focused，滚动通过 fork 的 ElementRef<ListView>，不再使用原生单选索引。
2. 应用自己维护 `selected: BTreeSet<Sid>` 与 `anchor`。每行包一层 `Border`，`on_pointer_pressed`：
   - 无修饰键左键：选中集合 = {该行}，`anchor` = 该行；
   - `Ctrl`：切换该行；
   - `Shift`：选中 `anchor`..该行（按当前排序后的顺序）；
   - 右键：该行不在集合内时先改为单选该行（与 Windows 资源管理器一致），并记录右键所在列供"复制选中列"使用。
3. 列表外层 `Border` 的 `on_preview_key_down`：`Shift+↑/↓` 扩展选区；`Ctrl+A` 全选；`Ctrl+C` 复制；单独 `↑/↓` 也由纯 Selection 模型移动焦点，由 TableView 请求滚动并恢复实际行焦点；Space 在焦点行切换豁免，焦点在 CheckBox 时保留其原生 Space 行为。
4. TableRow 绘制选择/hover/当前用户/键盘焦点，见 §4.2。ItemContainerStyle 去掉默认 inset/chrome，无需 SolidBackground 遮罩。CheckBox 焦点只更新逻辑 focused，不清除多选，原生 Space 仍交给 CheckBox。
5. 选中集合按 SID 保存，刷新与排序后仍然有效；刷新后不存在的 SID、被搜索隐藏的 SID 自动移出（包括 anchor/focused）；清空搜索不会恢复已移出的选区。
6. 纯逻辑（点击/修饰键/区间计算）放在 `model`，用普通值测试。

验证记录（2026-10-20，revision 2）：recording adapter 覆盖嵌套组件的 Ctrl 多选、checkbox 不清除选区、键盘路由、hover/focus chrome、sort 后 scroll index 和 260 行缩放。桌面仅启动 app 截图及运行无注入的 fork 原生测试，未点击/调用 Save，未做 UIA Select/Toggle/Invoke。默认/最大化截图为 target/ui-rev2-default.png 和 target/ui-rev2-max.png，已检查列对齐、32/40 DIP 高度与最后一列填充。真实修饰键、右键位置、pointer capture、主题和键盘流列于 docs/manual-smoke.md，待用户手测。

## 7. 测试

- 格式检查：cargo fmt -p enable-loopback -- --check。cargo fmt --all 会沿 path dependency 进入 fork 全部 workspace targets，在 Windows 超过命令行长度（os error 206）；CI 因此限定子项目包，fork 的 reactor/tool-reactor 单独检查。
- `model`：脏检测、全选/全不选的状态文案、排序（序数、升序 → 降序 → 不排序三态）、复制行/列格式（单行无换行、多行 `\r\n`）、`-allow`/`-deny` 过滤规则、参数解析（含 `-allow` 优先于 `-all`、NAME 含空格）、按 SID 合并用户、占位文案与空串差别、孤儿检测。全部用普通值测试。
- `netiso`：真实 API 只做冒烟（读取枚举与豁免名单不需提权，可在 CI 的 Windows runner 跑只读测试）；写入不在 CI 做。
- i18n：所有语言键集合一致的测试。
- 手动冒烟：提权/非提权保存、孤儿记录（可用 `CheckNetIsolation LoopbackExempt -a -p=<不存在的 SID>` 构造）、防火墙服务停止时的 1753 提示、`-registry`、`manual`、单实例置前、命令行各模式、各语言显示。

## 8. 主仓接入

1. `.gitmodules` 新增 `backend/enable-loopback` → `https://github.com/libnyanpasu/enable-loopback.git`；`backend/Cargo.toml` 的 `exclude` 加入该路径。
2. 取得产物沿用 nyanpasu-service 的做法（`scripts/src/prepare/binaries.ts` 的 `getNyanpasuServiceInfo`）：从 submodule 的 `crates/enable-loopback/Cargo.toml` 读版本号，下载子仓**同版本 tag** 的 release 产物，并对照 release 附带的 SHA-256 校验。版本随 submodule 指针一起变化，消除当前 `latest` 无校验的供应链风险。不在主仓构建流程里编译子仓，避免主仓 CI 多编一份 windows-rs。
3. `scripts/src/prepare/check.ts` 删掉 `Kuingsmile/uwp-tool` 的下载项，改走上一条。
4. 架构：子仓发布 x64 与 arm64，按主仓打包目标选择。
5. `win_uwp.rs` 追加 `--lang <当前应用语言>`。
6. 子仓 CI：Windows runner 上 `cargo test`、clippy、fmt，tag 时构建两种架构并上传产物与校验和。

### 8.1 Windows App Runtime 的获取方式（调研结果）

| 方式       | 情况                                                                                                                                                                                                        |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 微软商店   | **没有面向用户的独立商店条目**（`winget search "Windows App Runtime" --source msstore` 无结果）。商店应用依赖它时由商店自动安装框架包，所以装过较新商店应用的机器上往往已有（本机已有 2.2–2.5），但不能假定 |
| 官方安装器 | `https://aka.ms/windowsappsdk/2.4/2.4.0/windowsappruntimeinstall-x64.exe`（及 arm64），支持离线分发，需要管理员权限；下载页 `https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads`            |
| winget     | `winget install Microsoft.WindowsAppRuntime.2`（2026-10-19 最高 2.4.0，落后于下载页的 2.5.1）                                                                                                               |
| 自包含     | `windows-reactor-setup` 在构建时从 NuGet 拉取固定版本的 runtime，把所需 DLL 和目录复制到 exe 旁并嵌入自包含清单；用户机器无需安装任何东西，代价是安装包变大（数十 MB 量级，需实测）                         |

### 8.2 部署决定：框架依赖，由工具自己在启动阶段补装

1. 子仓不用 `windows-reactor-setup`，exe 依赖已安装的 `Microsoft.WindowsAppRuntime.2` 框架包，版本下限等于所用 reactor 提交的 `WINDOWSAPPSDK_RUNTIME_VERSION`：0.100.0 为 2.4.0，锁定的 master `a29e064` 为 **2.5.1**（`crates/libs/reactor/src/native/bindings.rs`）。升级 reactor 提交时同步核对。
2. 检测与补装由工具自己负责，主仓只负责启动工具并传 `--lang`。版本下限只在子仓维护，主仓不再重复检测。GUI 模式在取得单实例之后、调用 reactor 之前，用 [`nyanpasu-windows-dependencies`](https://github.com/libnyanpasu/nyanpasu-windows-dependencies) 检测当前进程架构下已注册的最高版本；检测接口本身失败时只记调试日志，继续交给 reactor。命令行模式不检测（不需要 runtime）。
3. 缺失时弹本地化对话框（`TaskDialogIndirect`，不依赖 WinUI）：
   - "从 Microsoft Store 安装"：只在 Store 应用已注册、且策略未阻止时出现，并作为默认按钮。经 Store 安装服务按商品 `9NRZT3Q9R3DL` 安装，不打开商品页（商品页显示"当前不可用"）。安装在工作线程运行，对话框显示进度条，完成后重新检测，满足下限就继续启动。用户关闭进度对话框时工具退出，Store 在后台继续安装。安装失败，或装好的版本仍低于下限时，提示原因，再回到下载选项；
   - "下载安装程序"：打开与所需版本和架构匹配的官方安装器链接 `https://aka.ms/windowsappsdk/<major.minor>/<version>/windowsappruntimeinstall-<x64|arm64>.exe`（锁定 `a29e064` 时为 `2.5/2.5.1`）；
   - "打开下载页"：`https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads`；
   - "取消"。
     **不提供 winget 安装**：2026-10-19 winget 上 `Microsoft.WindowsAppRuntime.2` 最高只有 2.4.0，低于所需的 2.5.1，装完仍无法启动。winget 跟上后可再评估。
     没有 Store 的系统（如 Windows LTSC）只显示后三项。
4. C 运行时静态链接。否则在没有 VC++ Redistributable 的系统上，加载器在检测代码运行之前就会失败。

## 9. 待决问题

已定（2026-10-19）：submodule；修正保存缺陷；链接到微软文档；标题本地化；保留计数；不做 x86；runtime 由工具在启动阶段检测并经 Store 或官方下载补装（框架依赖，见 §8.2）；接受 git rev 依赖 windows-rs master；多选自建（§6.2）；图标新绘、Fluent、SVG。

1. 图标图形：待从 codex 概念图中选定。

## 10. 搜索

`model/search.rs` 是纯匹配器。加载/替换行时，从七列原始值一次性生成 Unicode
lowercase 缓存，不把本地化占位文案放进索引。查询按 Unicode 空白分词并转小写；
所有 token 都必须在至少一列命中。每个 token 取各列“权重 × 质量”的最大值，行分数
为 token 分数之和。权重是具名常量：显示名称 10、AC 名称 6、包 5、描述 3、用户 2、
文件 1、SID 1。质量：整字段相等 3，前缀 2，词首 1.5，其他子串 1；词首只认前一个
字符为空格、`.`、`_`、`-`、`\`、`/`、`{`。扫描所有命中位置以取最高质量。
内部用半分整数（质量 6/4/3/2）保持精确排序。null 不匹配，存在但为空的字段也不会
匹配非空 token；空白查询显示全部行。

有活动排序列时保持现有排序；三态清除排序后，按相关度降序，再按枚举顺序。
筛选只改变可见索引，不改变勾选与保存基线。选区会裁掉隐藏 SID，列表 Ctrl+A、范围
选择与复制按可见顺序；复制只遍历可见选中行。批量“全部豁免 / 全部取消”只作用于可见行，
搜索时标签改为“豁免全部结果 / 取消全部结果”（见 §3.4）；“已豁免 n / m”始终使用全部行；结果数只在查询有效时追加。刷新保留搜索词并重建
缓存。六语言均有搜索 placeholder、结果数和居中的空状态；TableView 的可选
`empty_text` 相当于 EmptyTemplate，表头仍保留。搜索框内 Esc 清空，Ctrl+F 聚焦；
搜索输入期间禁止列表重建后的延迟焦点恢复抢走搜索焦点。无需修改 reactor fork。

### Release 基准与 rayon 决定（2026-10-20）

运行前设置 `CARGO_INCREMENTAL=0`，再执行
`cargo bench -p enable-loopback --bench search --config 'build.rustc-wrapper=""'`。
使用独立 `harness=false` 基准，
`benches/search.rs` 直接调用生产匹配器。生成器模拟应用名、资源描述、包全名、AC
名称、长 SID、用户及 WindowsApps 文件路径的长度和缺失比例，含 Unicode 字段；
没有保存本机 AppContainer 数据。另测 3k，覆盖“几千”行的决策边界。

Intel Core i9-14900KF，24 核 / 32 逻辑处理器，rayon 默认 32 线程。预计算字段与
线程池初始化在计时外；每个变体预热 100 ms，随后 7 个至少 100 ms 的批次，报告
各批次每次耗时的中位数。包含 query 分词、小写、匹配评分、结果收集与相关度排序；
不含字段缓存构建及 XAML 布局/渲染。每个查询先断言顺序与并行结果完全相同。
两列耗时均为 **ms / 次**：

|    行数 | 查询                       |    命中 | sequential |    rayon |
| ------: | -------------------------- | ------: | ---------: | -------: |
|     260 | `store`                    |      44 |   0.033553 | 0.045023 |
|     260 | `microsoft alice`          |     260 |   0.058723 | 0.082389 |
|     260 | `windows store 3500123456` |      44 |   0.071836 | 0.049527 |
|   1,000 | `store`                    |     167 |   0.126442 | 0.082279 |
|   1,000 | `microsoft alice`          |   1,000 |   0.225575 | 0.207805 |
|   1,000 | `windows store 3500123456` |     167 |   0.276171 | 0.086546 |
|   3,000 | `store`                    |     500 |   0.378526 | 0.173700 |
|   3,000 | `microsoft alice`          |   3,000 |   0.672888 | 0.478560 |
|   3,000 | `windows store 3500123456` |     500 |   0.824748 | 0.218122 |
|  10,000 | `store`                    |   1,667 |   1.261901 | 0.392938 |
|  10,000 | `microsoft alice`          |  10,000 |   2.316727 | 0.901483 |
|  10,000 | `windows store 3500123456` |   1,667 |   2.776178 | 0.469858 |
| 100,000 | `store`                    |  16,667 |  13.188038 | 1.850985 |
| 100,000 | `microsoft alice`          | 100,000 |  24.202060 | 4.604723 |
| 100,000 | `windows store 3500123456` |  16,667 |  28.473225 | 2.908517 |

**决定：生产搜索保持 sequential；rayon 仅为 dev-dependency，用于持续比较。**
260 行最慢 0.071836 ms，1k 最慢 0.276171 ms，3k 最慢 0.824748 ms，均远低于
约 4 ms / keystroke 的阈值。rayon 在一些查询中明显胜出，但合理规模下顺序搜索
没有慢到影响输入，因此不满足加入生产依赖的两个共同条件。10k / 100k 的并行收益
不改变当前决策。若未来实际规模或端到端输入延迟显著变化，再用此基准复核。
