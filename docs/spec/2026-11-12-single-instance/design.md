# 自维护的单例检测与深链唤醒

**日期：** 2026-11-12

**状态：** 设计稿，未实施。

**基线：** app `main@0f067133f`；上游参考 `tauri-apps/plugins-workspace` v2 分支的 `tauri-plugin-single-instance 2.5.5` 与 `tauri-plugin-deep-link 2.6.3`。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec。

## 1. 背景与目标

桌面 shell 与 headless 的 `nyanpasu-server` 都需要"同一用户、同一配置目录只跑一个实例"。桌面还需要第二个实例把自己的启动参数（深链 URL）交给主实例并退出，由主实例拉起主窗口。现在这两件事由两个外部来源拼起来：crates.io 的 `single-instance 0.3.3` 负责独占，vendored 的 `backend/tauri-plugin-deep-link`（FabianLars 0.1.2 的 fork）负责转发 URL；server 两者都没有。

上游两个官方插件是 Tauri 插件，依赖 `AppHandle`、`tauri.conf.json` 的 identifier 与插件生命周期；Windows 实现依赖消息循环，Linux 实现依赖 session D-Bus。headless 进程两者都没有，所以不能直接换成官方插件。本 spec 定义一套自维护实现，两种 shell 共用一个机制，接口形状照搬上游。

目标：

- 一个中立 crate 提供"独占 + 唤醒"，桌面与 server 都在组装应用之前调用它。
- 第二个桌面实例无论有没有 URL 都唤醒主实例（现在只有带 URL 时才转发）。
- 端点只有当前用户可达（现在 Windows 管道对 Everyone 开放，Linux socket 在 `/tmp` 无权限保护）。
- 删除 vendored 插件与 `single-instance` 依赖；不再有 `OnceCell` 全局状态。
- 深链注册与 argv 过滤留在 Tauri shell，macOS 改用 Tauri 自己的 `RunEvent::Opened`。

非目标：改 NSIS 安装器的 scheme 注册；把本地 RPC 端点改为默认开启；移动端。

## 2. 现状

| 部件               | 位置                                                                        | 行为                                                                                                                                                                | 问题                                                                                                                                                                                                                                              |
| ------------------ | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 桌面独占           | `backend/tauri/src/utils/init/mod.rs:305-401`                               | Windows 命名互斥量 `Local\<APP_NAME>-<SID>`，SID 靠启动 PowerShell / wmic 子进程取得；unix 为 `<config_dir>/instance.lock`。失败后 5 次 × 1 s 重试，再静默退出      | 子进程取 SID 慢且脆弱；Windows 不按配置目录区分而 unix 按；第二实例不唤醒主实例                                                                                                                                                                   |
| 深链转发           | `backend/tauri-plugin-deep-link/src/{windows,linux,macos}.rs`               | `prepare()` 在独占检测之前运行：argv[1] 存在时连主实例的端点、写 URL、`process::exit(0)`；主实例 `register()` 起一条 `deep-link` 线程 accept                        | 全局 `static ID`/`HANDLER`；Windows DACL `D:(A;;GA;;;WD)`（Everyone 全权）；Linux `/tmp/<id>-deep-link.sock` 无权限；崩溃计数 `CRASH_COUNT` 自重启线程；macOS 自装 `NSAppleEventManager` 处理器，与 tao 的处理器互相覆盖；无 URL 的第二实例不转发 |
| 深链注册           | 同上                                                                        | 每次启动写 HKCU `Software\Classes\<scheme>`；Linux 每次重写 `~/.local/share/applications/<exe>-handler.desktop` 并跑 `update-desktop-database` + `xdg-mime default` | 可用，但 `update-desktop-database` 缺失即报错                                                                                                                                                                                                     |
| 冷启动 URL         | `backend/tauri/src/lib.rs:184-203, 482-491`                                 | argv[1] 能被 `Url::parse` 就当深链，跳过子命令解析                                                                                                                  | 没有 scheme 白名单，`C:\x` 也会被当成 scheme `c`                                                                                                                                                                                                  |
| 主实例交付         | `lib.rs:136-147`，`ipc.rs:247-274`                                          | `PendingDeepLinks` 队列 + `SchemeRequestReceived` 事件，前端 `take_pending_deep_links` 取走                                                                         | 保留不动                                                                                                                                                                                                                                          |
| `migrate-home-dir` | `backend/tauri/src/cmds/migrate.rs:205-215, 252`                            | 轮询 `SingleInstance::new` 等主程序退出                                                                                                                             | 随独占机制一起换                                                                                                                                                                                                                                  |
| server             | `backend/nyanpasu-server/src/{main,run}.rs`                                 | `Server::try_new` 先组装整个应用，之后 `serve` 才 bind 本地 RPC 端点；第二个实例要跑完整组装才失败                                                                  | 需要组装前的独占                                                                                                                                                                                                                                  |
| 可复用             | `backend/nyanpasu-rpc/src/local/endpoint/{windows,unix,directory_chain}.rs` | 每用户私有端点、首绑定者独占、陈旧回收、校验目录链，带测试；`nyanpasu-local-channel/src/name.rs` 另有一份 SID 与私有目录代码                                        | 两份重复；本 spec 给它们一个所有者                                                                                                                                                                                                                |

## 3. 上游参考：采用与放弃

| 来源                                                                                            | 采用     | 放弃                | 原因                                                           |
| ----------------------------------------------------------------------------------------------- | -------- | ------------------- | -------------------------------------------------------------- |
| single-instance：回调签名 `(args: Vec<String>, cwd)`，args 含 argv[0]                           | 采用     |                     | 形状通用，shell 自己过滤                                       |
| single-instance：第二实例"转发成功即退出 0，转发失败按启动错误"                                 | 采用     |                     | 2.5.4 改成这样，避免静默吞掉参数                               |
| single-instance Windows：互斥量 + 隐藏窗口 + `WM_COPYDATA`                                      |          | 放弃                | 要主实例有消息循环，server 没有                                |
| single-instance Windows：第二实例 `AllowSetForegroundWindow(主实例 pid)`                        | 采用     |                     | 让主实例能把窗口提到前台；pid 由 ack 携带                      |
| single-instance Linux：session D-Bus 名 `<id>.SingleInstance`                                   |          | 放弃                | headless、无桌面会话时无 session bus，上游此时直接退化为不检测 |
| single-instance macOS：`/tmp/<id>_si.sock`                                                      | 机制采用 | 路径放弃            | 改到用户私有目录                                               |
| single-instance：`semver` feature 按版本分名                                                    |          | 放弃                | 本仓库没有多版本并存需求                                       |
| deep-link：`handle_cli_arguments` 只在 argv 恰为 `[bin, url]` 时接受，且 scheme 必须在白名单    | 采用     |                     | 修掉现状的"任何 URL 都算"                                      |
| deep-link Windows：HKCU `Software\Classes\<scheme>` 四个值，命令 `"<exe>" "%1"`                 | 采用     |                     | 与 vendored 相同                                               |
| deep-link Linux：`.desktop` 用 ini 合并、`update-desktop-database` 缺失仅警告、xdg 非零退出即错 | 行为采用 | `rust-ini` 依赖放弃 | 文件是我们自己写的模板，内容比较即可                           |
| deep-link macOS：`RunEvent::Opened { urls }`                                                    | 采用     |                     | 删掉 objc2 处理器                                              |
| deep-link：`register/unregister/is_registered` 与 `get_current` 作为 Tauri 命令                 |          | 放弃                | 前端不需要                                                     |

## 4. 设计

### 4.1 crate：`nyanpasu-single-instance`

新的 leaf crate `backend/nyanpasu-single-instance`（名字为工作名）。它只依赖 `interprocess`、`tokio`（`net`、`sync`、`time`）、`snafu`、`tracing`、`serde`、`sha2`，以及平台 crate `windows-sys`、`widestring`、`libc`、`socket2`。它不依赖任何 nyanpasu 包，也不依赖 Tauri、egui/eframe、Slint。两个模块：

- `endpoint`：从 `nyanpasu-rpc::local::endpoint` **移入**的通用原语。用户身份（Windows SID 经 token API，unix euid）、私有目录与管道名、位置校验、首绑定者 bind（Windows `FILE_FLAG_FIRST_PIPE_INSTANCE` + 当前用户 DACL；unix 私有目录 + flock claim 文件 + 仅在 `ECONNREFUSED` 时回收陈旧 socket）、connect。`nyanpasu-rpc` 改为依赖它，不留 shim。现在写死的 `nyanpasu-rpc-` 前缀变成 `Namespace` 参数（编译期校验的 `&'static str`，1–32 个小写字母、数字或 `-`），`Endpoint::discover(namespace, directory, location)` 按它命名管道前缀与 unix socket 目录，Windows 位置校验按它的前缀进行。rpc 导出 `local::ENDPOINT_NAMESPACE = "nyanpasu-rpc"`，因此 rpc 端点路径与移动前逐字节相同，已有 CLI 无需改动。
- `instance`：本 spec 新增的独占与唤醒。

`nyanpasu-local-channel/src/name.rs` 里的重复 SID 代码不在本 spec 范围内；记为后续去重项。

### 4.2 身份与端点路径

实例身份是 `(当前用户, namespace, root)`：namespace 区分进程家族，`root` 是规范化后的配置目录。端点复用 `Endpoint::discover(namespace, root, None)`，`identity = hex(sha256(user ‖ 0 ‖ canonical(root))[..16])`，namespace 落在路径前缀上而不进哈希。

| 平台          | 路径                                                                                                                                                         |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Windows       | `\\.\pipe\<namespace>-<SID>-<identity>`，DACL `D:P(A;;GA;;;<SID>)`                                                                                           |
| Linux / macOS | `<private>/<namespace>-<uid>/<identity>.sock`，`<private>` 为 `/tmp` 的规范化路径（macOS 下即 `/private/tmp`），目录 0700，claim 文件 `<identity>.sock.lock` |

桌面的 namespace 是 `<APP_NAME>-instance`（`clash-nyanpasu-instance` 或 `clash-nyanpasu-dev-instance`），`root` 是 `paths.app_config_dir()`；server 的 namespace 固定为 `nyanpasu-server-instance`，`root` 是 `--config-dir`。最长 namespace 下 macOS socket 路径约 94 字节，低于 `sockaddr_un` 的 103 字节上限。桌面与 server 永不互斥；dev 与正式构建靠 `APP_NAME` 区分；同一构建、不同配置目录（便携版与安装版）可以并存。这是 Windows 的行为变化：现在 Windows 只按用户和应用名互斥。

配置目录在 claim 之前创建（现状如此，`check_singleton` 已这么做），规范化用 `canonicalize`，Windows 上去掉 `\\?\` 前缀后再哈希，避免同一目录两种写法产生两个身份。

### 4.3 claim 语义

```rust
pub enum Claim {
    /// 本进程是主实例；丢弃它即释放端点。
    Primary(Primary),
    /// 另一个实例持有端点。
    AlreadyRunning(Peer),
}

pub async fn claim(namespace: Namespace, root: &Path) -> Result<Claim, ClaimError>;
```

- `claim` 不等待、不重试。Windows 上首实例标志让"创建管道"本身就是独占；unix 上先 flock claim 文件，再按 `endpoint` 的规则处理陈旧 socket，再 bind。
- `AlreadyRunning` 只在"另一个活着的进程确实持有端点"时返回：Windows 的 `AddrInUse`/`PermissionDenied`；unix 的 flock `WouldBlock`，或 socket 可连接。
- 其余失败（目录不安全、权限、位置非法）是 `ClaimError`，调用方决定是继续还是退出（见 4.6）。
- `Primary` 持有监听器和 claim；`Drop` 关闭监听器并释放 claim。不提供 `destroy()`：shell 在进程退出路径上丢弃它即可。
- `claim` 是 async：`interprocess` 的 Tokio 监听器要在 reactor 上注册，端点 bind 本身也在 blocking 线程上做文件系统检查。桌面在 `tauri::async_runtime::set` 时已拿到 `nyanpasu_utils::runtime::get_runtime_handle()`，用它 `block_on(claim(..))`；server 在建好 runtime 之后、组装之前 claim。

### 4.4 唤醒协议

```rust
pub struct Launch {
    /// `std::env::args_os()` 的全部元素，非 UTF-8 的按 `to_string_lossy`。
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
}

impl Peer {
    /// 把本进程的启动信息交给主实例，等它确认。带网络侧超时。
    pub async fn notify(&self, launch: &Launch) -> Result<Acknowledged, NotifyError>;
}

pub struct Acknowledged {
    pub primary_pid: u32,
}
```

线上格式：一个字节版本号 `1`，随后 big-endian `u32` 长度与 JSON 体，上限 64 KiB；回复同样格式，内容 `{ "pid": u32 }`。版本不符、超限或 JSON 错误由 `FrameError` 区分，只断开那一条连接。超时固定 5 s，属于 IPC 适配层的网络侧期限，符合 architecture.md 对期限的归属。

主实例侧：

```rust
impl Primary {
    /// 在当前 runtime 上启动接收任务：每个连接读一条 Launch、入队、回 ack。
    pub fn into_launches(self) -> Launches;
}

impl Launches {
    pub async fn recv(&mut self) -> Option<Launch>;
}
// Drop for Launches：abort 接收任务，任务持有的监听器随之释放端点。
```

接收任务立刻 ack 而不等 shell 处理，所以第二实例不会被主实例的 setup 卡住；`AppHandle` 还不存在时 `Launch` 留在队列里，shell 准备好后再消费。接收任务用 `tokio::spawn` 并独占监听器，每个连接一个子任务（放在任务自有的 `JoinSet` 里，abort 时一并取消），单个连接的读写受同一 5 s 期限约束。不需要 `CancellationToken` 或 `TaskTracker`：丢弃 `Launches` 就是停止。`cwd` 用 lossy 的 `String`，与上游相同。

Windows 上 `notify` 收到 ack 后调用 `AllowSetForegroundWindow(primary_pid)`，失败只记 `debug` 日志。不保留 vendored 的 `SendInput` 假按键。

### 4.5 桌面 shell 接入

`lib.rs::run()` 的顺序改为：

1. 解析 `paths`，创建配置目录。
2. 从 argv 判定冷启动深链：`desktop::deep_link::from_args(std::env::args_os(), SCHEMES)`，只接受 `[bin, url]` 且 scheme 属于 `["clash-nyanpasu", "clash"]`。不是深链才走 `cmds::parse`。
3. `tauri::async_runtime::set(handle)` 后 `handle.block_on(claim(INSTANCE_NAMESPACE, config_dir))`：
   - `Primary`：继续启动，在同一个 `block_on` 里 `into_launches()`，从此开始 ack 第二实例；`Launches` 交给 setup，由 setup spawn 的消费任务持有到进程结束（替代现在的 `_singleton` 局部变量）。
   - `AlreadyRunning`：`notify(Launch::current())`；成功即 `return ExitCode::SUCCESS`；失败（连接被拒、超时）时在 5 s 预算内每 250 ms 重新 `claim`，预算耗尽后把错误写到 `stderr` 并 `return ExitCode::FAILURE`。这个预算覆盖自重启：relauncher 在旧进程退出后拉起新进程，旧进程的端点可能还有一瞬没释放。
   - `Err(ClaimError)`：记 `error` 日志后继续启动（维持现状的"尽力而为"策略），不再做单例保护。
4. 消费任务对每个 `Launch`：`WindowManager::open(MainWindow)`，再用同一 `from_args` 过滤；命中则 `queue_deep_link`。冷启动的 URL 仍在 setup 里直接 `queue_deep_link`。

删除 `tauri_plugin_deep_link::prepare/register`、`utils::init::check_singleton`、`single_instance_placeholder`、`current_user_sid`（PowerShell 版）、`config_hash` 与对应测试；`Error::CreateSingleton*` 变体随之删除。

### 4.6 server 接入

`nyanpasu-server/src/main.rs::start`：建 runtime 之后、`logging::start` 之前 claim（这样"已在运行"的提示能到 stderr，不依赖日志）。`AlreadyRunning` 时打印 `another nyanpasu-server already serves <config_dir>` 并 `ExitCode::FAILURE`；server 不调用 `notify`。`Err(ClaimError)` 对 server 是启动失败：headless 进程没有人看对话框，带着不确定的独占状态继续运行比退出更危险。`Primary` 由 `start` 持有到 runtime 被丢弃之后，不调用 `into_launches()`：第二个 server 不发 `notify`，没有交接可接。namespace 为 `nyanpasu-server-instance`。

本地 RPC 端点的独占不变：它仍按 `(用户, 数据目录)` 首绑定者独占，只是现在它在单例之后才会被尝试。

### 4.7 深链注册与 macOS

新模块 `backend/tauri/src/desktop/deep_link.rs` 接管 vendored 插件的注册部分：

- `SCHEMES: [&str; 2] = ["clash-nyanpasu", "clash"]`。
- `from_args(args, schemes) -> Option<Url>`：4.5 的过滤规则，纯函数，带测试。
- `register(schemes, product_name, exe) -> Result<(), RegisterError>`（`snafu`）：
  - Windows：`windows-registry 0.6`（tauri crate 已依赖）写 HKCU `Software\Classes\<scheme>` 的四个值，exe 路径经 `dunce::simplified`。每次启动写，与上游一致。
  - Linux：`~/.local/share/applications/<exe 文件名>-handler.desktop`，`Exec` 取 `APPIMAGE` 环境变量或当前 exe，`MimeType` 列出全部 scheme；内容不同时才重写。随后 `update-desktop-database <dir>`（缺失仅 `warn`，非零退出报错）与每个 scheme 一次 `xdg-mime default`（非零退出报错）。
  - macOS：空实现，scheme 来自 `Info.plist`（现状）。
- 不实现 `unregister`/`is_registered`：没有调用方。
- 注册失败只记 `warn`，不阻止启动（现状为 `log_err!`）。

macOS 的 URL 接收改为 `app.run` 闭包里处理 `tauri::RunEvent::Opened { urls, .. }`：每个 URL 过白名单后 `queue_deep_link` 并打开主窗口。删除 `macos.rs` 的 objc2 处理器与 debug 模式 socket。

### 4.8 `migrate-home-dir`

`cmds/migrate.rs` 用同一 namespace 与配置目录轮询 `claim`（每秒一次）直到得到 `Primary`，表示主程序已退出；杀进程、迁移目录后 `drop(primary)` 再拉起应用。该子命令运行在 `cmds::parse` 里，没有 runtime 上下文，用 `nyanpasu_utils::runtime::get_runtime_handle().block_on` 调用它。

## 5. 安全

- 端点继承 `nyanpasu-rpc` 端点的全部约束：Windows 管道 DACL 只含当前用户 SID；unix 目录 0700、socket 0600、`nlink == 1`、目录链无其他用户可写的祖先；只在 `ECONNREFUSED` 时回收陈旧 socket，并在 unlink 前复核 inode。
- 主实例对收到的 `Launch` 不做任何执行：只打开自己的窗口，URL 只有过白名单才入队，前端再做一次 `parseInstallConfigDeepLink`。
- 帧上限 64 KiB，坏帧只断开那条连接，不影响接收任务。
- `AllowSetForegroundWindow` 只授予 ack 里的 pid，而 ack 来自只有本用户能 bind 的管道。

## 6. 错误模型

`snafu`，每个失败步骤一个变体：

- `ClaimError`：`Locate { root, source: EndpointError }`、`Bind { root, source: EndpointError }`。
- `FrameError`：`Io`、`Version { found }`、`TooLarge { len }`、`Encode`、`Decode`。
- `NotifyError`：`Connect`、`Send { source: FrameError }`、`Receive { source: FrameError }`、`Timeout`。
- `RegisterError`（shell）：`Registry { scheme, source }`、`DesktopFile { path, source }`、`UpdateDesktopDatabase { status }`、`XdgMime { scheme, status }`。

## 7. 测试与验证

crate 内：

- 同一 namespace 与目录两次 `claim`：第二次得到 `AlreadyRunning`；丢弃 `Primary` 后再 claim 成功。
- `notify` 到 `Launches::recv` 的往返：args、cwd、非 UTF-8 参数的 lossy 结果；ack 携带本进程 pid。
- 坏版本、超长帧、半帧断开：主实例继续接收后续连接。
- unix：陈旧 socket（claim 文件无人持有、socket 文件在、连接被拒）被回收；其他用户目录被拒。Windows：DACL 只含当前用户；第二个 binder 得到 `AlreadyRunning`。
- 端点原语的现有测试随代码一起移动，`nyanpasu-rpc` 的 `endpoint/tests.rs` 改为只测它自己的 `Endpoint` 包装。

shell 内：

- `from_args`：`[bin]`、`[bin, url]`、`[bin, url, extra]`、非白名单 scheme、`C:\x`。
- Linux `.desktop` 内容与"内容相同不重写"。

门禁：`pnpm run lint:backend-boundaries`（新 crate 登记到 `backend-boundaries.ts` 的包清单与规则，`nyanpasu-rpc` 允许依赖它）、`pnpm run lint:architecture-ledger`（`policy.ts` 的 `STATIC_GATE_PREFIXES` 加 `backend/nyanpasu-single-instance/src/`）、`scripts/src/testing/test-backend-musl.ts` 的包清单与 `lib:` 目标、`pnpm test:backend`、三平台 clippy。

冒烟（Windows 本机，macOS/Linux 由 CI 编译、行为留待各平台验证）：

1. 启动两次：第二个进程退出，第一个的主窗口被提到前台。
2. 主程序运行中打开 `clash-nyanpasu://install-config?url=...`：导入对话框出现。
3. 主程序未运行时打开同一链接：冷启动后导入对话框出现。
4. 托盘"重启"：新实例正常成为主实例，不被旧实例吞掉。
5. 便携版与安装版同时启动：两者并存（新行为）。
6. `migrate-home-dir`：等待主程序退出后完成迁移并重启。
7. `nyanpasu-server` 同一 `--config-dir` 启动两次：第二个以失败码退出且不组装应用。

## 8. 实施拆分

每条是一个可构建提交，按栈推进：

| PR  | 内容                                                                                                                                                                                                                                         | 完成标准                                                                                 |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| S1  | 新建 `nyanpasu-single-instance`，把 `nyanpasu-rpc::local::endpoint` 的平台原语移入其 `endpoint` 模块并参数化前缀；rpc 改调用方；登记三处门禁与 musl 清单                                                                                     | rpc 的端点测试全过；`lint:backend-boundaries`、`lint:architecture-ledger` 通过；行为不变 |
| S2  | `instance` 模块：`claim`、`Peer::notify`、`Primary::into_launches`，含第 7 节 crate 内测试                                                                                                                                                   | 测试通过；三平台 clippy 无新告警                                                         |
| S3  | 桌面接入：`desktop/deep_link.rs`、`lib.rs` 顺序、macOS `Opened`、`migrate-home-dir`；删除 vendored 插件、`single-instance` 依赖、`utils/init` 的 SID 代码；更新 `rust.md` 的 vendored 说明与 `backend-packages.md`、`architecture.md` 的包表 | 冒烟 1–6；`cargo test -p clash-nyanpasu` 通过                                            |
| S4  | server 接入                                                                                                                                                                                                                                  | 冒烟 7；server 的 run 测试覆盖"已在运行"路径                                             |

## 9. 已决策

| 项                                  | 决定                                                                                                | 日期       |
| ----------------------------------- | --------------------------------------------------------------------------------------------------- | ---------- |
| 代码归属                            | 新 leaf crate，并接管 rpc 的端点原语                                                                | 2026-11-12 |
| 身份键                              | 用户 + 应用名 + 规范化配置目录，三平台统一                                                          | 2026-11-12 |
| 深链注册位置                        | Tauri shell `desktop/deep_link.rs`                                                                  | 2026-11-12 |
| 工作区                              | 新 worktree `G:/Programs/Rust/.ccg/clash-nyanpasu/single-instance`，分支 `refactor/single-instance` | 2026-11-12 |
| 被否决：单例复用本地 RPC 端点早绑定 | 桌面本地 RPC 是显式 opt-in；唤醒要等应用组装完才能被 accept，启动失败时第二实例只能超时             | 2026-11-12 |

## 10. 待决策

- crate 名：`nyanpasu-single-instance` 还是 `nyanpasu-local-endpoint`。前者说清主要用途，后者更贴合它同时是 rpc 端点原语的所有者。
- `ClaimError` 时桌面是否仍按现状继续启动。本 spec 维持现状；若希望与 server 一致改为退出并弹启动失败对话框，S3 顺手改。
- 是否把 scheme 列表加进 `tauri.conf.json` 的 `plugins.deep-link.desktop.schemes`，让 NSIS 在 `SHCTX` 下注册。本 spec 不做。

## 11. 核验状态

- 第 2 节的位置与行为均对照当前源码核实。
- 第 3 节的上游行为来自对 v2 分支源码的阅读（版本见基线），未逐条在本机运行上游插件。
- `interprocess` 的 Tokio 监听器需要 runtime 上下文：依据 `nyanpasu-rpc` 现有代码在 `spawn_blocking` 内 bind 的做法推断，S2 实施时以编译与测试为准。
- 便携版与安装版并存、`AllowSetForegroundWindow` 不带假按键是否足够把窗口提到前台：冒烟 1 和 5 验证。
