# 本地类型化通道 `nyanpasu-local-channel`

**日期：** 2026-10-16

**状态：** 已实施（分支 `feat/local-channel`）。本次只交付库本身，widget 与 tray 的迁移另行进行。

**基线：** `main@0f7c150a1`；interprocess 2.4.5，tokio 1.53，tokio-util 0.7.20，postcard 1.1.3。

**需求：** 用 `interprocess` 封装一个开箱即用、类似 `ipc-channel` 的父子进程通道，供统计浮窗与未来的独立托盘进程使用；e2e 测试全覆盖，并有压力测试。

**权威顺序：** 当前 AGENTS.md 与 development guides > 本 spec。

## 1. 决策与范围

1. 新增中立叶子 crate `backend/nyanpasu-local-channel`。它不依赖 Tauri、egui、配置或 core，任何 crate 都可以依赖它。名字避开 runtime 里的 `nyanpasu_ipc`（service IPC）。
2. 保留 `ipc-channel` 的用法：父进程创建 one-shot 服务端，把 bootstrap 通过环境变量交给子进程，子进程连接。与 `ipc-channel` 的区别：
   - 一个连接同时承载两个方向；
   - 提供 tokio 版（服务端与客户端）和阻塞版客户端；
   - `accept` 可以直接取消；
   - 内置访问控制与 token；
   - 握手时校验两端的消息类型。
3. 不做的：阻塞版服务端、多客户端服务端、自动重连、在消息里传递通道、共享内存、请求/响应关联、跨版本兼容。
4. widget 迁移暂缓：`feat/slint-widget` 分支（`5dd4b58f2`）正把 widget 的 IPC 抽到 `nyanpasu-widget-protocol`，等它落地后在其上迁移，避免冲突。

## 2. 公开 API

| 项                                           | 说明                                                                                                                                                                                                         |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `tokio::OneShotServer<S, R>`                 | `new(prefix)`（前缀最多 16 个 ASCII 字母、数字、`-`、`_`）/ `with_options` 立即开始监听，返回 `(server, Bootstrap)`；`accept(self)` 等待子进程，返回 `(Sender<S>, Receiver<R>)`。须在 tokio runtime 内调用。 |
| `tokio::connect` / `connect_with_options`    | 异步客户端。                                                                                                                                                                                                 |
| `blocking::connect` / `connect_with_options` | 阻塞客户端，供没有 async runtime 的子进程（例如事件循环占用主线程的 GUI）使用。                                                                                                                              |
| `Sender<T>`                                  | 可 `Clone`；每条消息整帧写出，克隆之间不会交错。tokio 版的 `send().await` 在帧交给操作系统后才返回。                                                                                                         |
| `Receiver<T>`                                | tokio 版 `recv` 可安全取消，可放进 `select!`；阻塞版实现 `Iterator`，对端断开时结束。                                                                                                                        |
| `Bootstrap`                                  | `v1:<64 位十六进制 token>:<name>`，经 `Display`/`FromStr` 往返；`Debug` 隐去 token；`from_env()` 读 `NYANPASU_LOCAL_CHANNEL`。                                                                               |
| `Options`                                    | `with_max_frame_len`，默认 16 MiB，同时约束本端的发送与接收。                                                                                                                                                |
| `Never`                                      | 空枚举，用作单向通道中不承载消息的那个方向。                                                                                                                                                                 |

## 3. 线上格式与握手

- 帧格式：`u32` 小端长度，后接 postcard 编码的负载。tokio 端用自写的 `Decoder`，阻塞端用 `read_exact`，两边的帧格式由跨模式的 e2e 测试锁定。
- 客户端先发 `ClientHello { magic: "NYCH", wire: 1, token, fingerprint }`，服务端回 `Accepted` 或 `Rejected(reason)`。握手帧上限 256 字节。
- 服务端的判定（`wire::judge`，纯函数）：
  - 不是 hello，或 magic 不符：不回复，继续等待；
  - token 不符：回复 `Rejected(Token)`，继续等待；
  - token 正确但 wire 版本或类型指纹不符：说明这是自己的子进程，但它没法与服务端通信，于是回复拒绝，`accept` 返回错误。
- token 最先检查，所以只有真正的子进程才能知道自己被拒的原因。token 比较不随不同字节的位置变化耗时。
- 类型指纹是按服务端方向对 `type_name::<(S, R)>()` 做的 FNV-1a 哈希。它只保证同一构建内稳定，正好覆盖"父进程启动自己的子进程"这一场景。

## 4. 安全模型

| 平台    | 位置                                 | 访问控制                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------- | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Windows | `\\.\pipe\<prefix>-<pid>-<random>`   | 受保护的 DACL `D:P(A;;GA;;;<当前用户 SID>)`，SID 由 `GetTokenInformation(TokenUser)` 取得。e2e 测试读回的结果是 `D:P(A;;FA;;;<SID>)`。                                                                                                                                                                                                                                                                        |
| Unix    | `<dir>/<prefix>-<pid>-<random>.sock` | `dir` 依次取第一个私有目录（不是符号链接、属主为当前用户、权限位没有对其他人放开）：`$XDG_RUNTIME_DIR`（须为绝对路径）；临时目录本身（macOS 的 `$TMPDIR` 即每用户私有）；`temp_dir()/nyanpasu-local-channel-<uid>`，以 0700 创建后再校验。私有目录直接使用而不再套一层，是为了让路径放得进 `sockaddr_un`：macOS 的 `$TMPDIR` 长 48 字节，上限 103 字节。不使用 Linux abstract namespace，因为它没有权限控制。 |

token 只是防止误连的第二道防线。同一用户的进程本来就能读到子进程的环境变量，不在防护范围内。

## 5. 连接语义

- **写入完成：** tokio 版 `send` 把帧交给后台写任务，等它确认写入后才返回；future 中途被取消，结果只有"整帧未发"或"整帧发出"两种。阻塞版 `send` 直接写入。因此，一个进程发完消息后立即退出，对端仍能收到全部消息，这一点有 e2e 测试覆盖。
- **关闭：** 一端只有在所有 `Sender` 和 `Receiver` 都 drop 后才关闭连接；只 drop 其中一边，另一方向照常工作。tokio 在 Unix 上 drop `OwnedWriteHalf` 时会对写方向执行 shutdown，所以写任务结束后，写半边交给 `Receiver` 保管，等它也被 drop 再关闭。没有这一步，Unix 的对端会提前读到 EOF，而 Windows 测不出这个问题，有回归测试和 Linux 上的变异验证覆盖。
- **背压：** 只有操作系统在缓冲已写出的数据。读端卡死时，发送端在缓冲写满后停住（压力测试以 64 KiB 一条发送，Windows 停在 64 KiB，Linux 停在 192 KiB）。
- **one-shot：** 服务端被 accept 或 drop 之后，不能再建立新连接。Unix 下晚到的客户端会立即失败。Windows 下管道名字随任一实例存在，已接受的连接本身就是一个实例；interprocess 的客户端会无限等待空闲实例，所以晚到的客户端会等到通道关闭后才失败（见 §7）。
- **错误：** 非断连的 I/O 错误（`SendError::Io`、`RecvError::Io`）和超长的入站帧都会结束通道；解码失败只丢弃这一条消息，通道继续可用。

## 6. 测试

| 层                            | 内容                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 单元（27 个，Linux 上 34 个） | 判定逻辑、帧编解码、bootstrap 格式、名字与私有目录（unix）、DACL 文本（Windows）、解码器的分段与 EOF 处理                                                                                                                                                                                                                                                                                                                                                                                  |
| e2e（29 个）                  | 测试二进制以 `peer_entry` 重新启动自己作为子进程：两种客户端的回显、克隆跨线程发送、退出前写出的消息全部送达、崩溃或被杀视为断连、单向 `Never`、冒充者被拒后真正的子进程仍被接受、非客户端连接被忽略、类型不符两端报错、帧超限（双向、两种接收端）、解码失败后通道仍可用、`select!` 取消 `recv` 不丢消息、取消或 drop 后名字被释放、accept 之后无法再连接、非 Unicode 的 bootstrap、假服务端破坏握手（两种客户端各四种情形）、只 drop 发送方向时连接保持、Windows DACL 读回、Unix 目录权限 |
| 压力（7 个）                  | 双向洪泛 10 万条且保序、24 条 4 MiB 消息内容完整、16 个克隆并发发送、读端卡死时缓冲有上界、8 个子进程并行、300 次进程内建连加 20 次子进程循环且不泄漏句柄或 socket 文件。`NYANPASU_LOCAL_CHANNEL_STRESS_SCALE` 可以放大负载。                                                                                                                                                                                                                                                              |

验证情况：

- Windows 与 Linux（WSL）都跑通了全部测试，包括 5 倍负载的压力测试；
- 加了一个守卫测试，证明子进程确实在执行场景，防止依赖子进程断言的测试空转通过；
- 对冒充者测试和半关闭回归测试分别做了变异验证，撤掉对应逻辑后测试都会失败；
- Windows 行覆盖率 95.7%（含子进程）。未覆盖的只剩三类：Windows API 失败分支、非断连 I/O 错误分支、写 hello 失败这一时序竞争分支。

## 7. 已知限制

- interprocess 2.4.5 在 Windows 上的 local socket `connect` 忽略 `ConnectOptions::wait_mode`，固定无限等待，见 `os/windows/named_pipe/local_socket/stream.rs` 的 `from_options`。由此产生 §5 中 Windows 晚到客户端的行为。要改成立即失败，只能在 Windows 客户端整套改用命名管道 API 并对读写半边做类型擦除；我们的用法是一个服务端只对应一个子进程，不值得这样做。
- macOS 只在本地做了 clippy 交叉检查，没有实际运行，要靠 CI。
- 当前 nightly cargo 的新 build-dir 布局让 cargo-llvm-cov 0.6.15 找不到目标文件；覆盖率是用 `llvm-cov report` 手工指定可执行文件得到的。
- `accept` 按顺序处理连接，一个连上后什么都不发的同用户进程会让它一直等待。这种进程已经不在防护范围内（§4）；调用方可以给 `accept` 加截止时间。

## 8. 后续

1. `feat/slint-widget` 落地后，把 `nyanpasu-widget-protocol` 的 IPC 改用本库，删掉 `release_server` 和握手用的 `spawn_blocking`，移除 `ipc-channel` 依赖。
2. 独立托盘进程按 tray 方案建立在本库之上（host 用 tokio 服务端，tray 用阻塞客户端）。
