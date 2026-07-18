# fake-core 设计文档

## 设计概述

### 目标

- 为 PR-4S **S09** process-lifecycle / failure-matrix 测试提供**可重复、可注入**的假 Clash 核心进程。
- 使用与真实 core **相同的 argv 形态**（check / mihomo / clash-rs / premium），避免测试只覆盖“假 CLI 方言”。
- 用 **`FAKE_CORE_*` 环境变量** 控制 check/start/apply 成败与流输出，替代早期 illustrative flags（如 `--check-ok` / `--start-fail`）。
- 用 TCP **READY / RELEASE** 屏障做父子同步；超时仅作死锁安全网，**禁止 sleep 排序**。
- 支持动态 hold/http 端口（含 `0` 临时端口）并在 READY 帧中回报**实际**绑定端口，消除 TOCTOU。
- 提供 `ScopedChild` 等 RAII 辅助，保证断言失败时不泄漏子进程与占用端口。
- 保持 **std-only**、loopback-only，便于跨平台编译与最小依赖。

### 非目标

- **永不**作为 production sidecar / resource 打包或随安装包分发。
- 不实现真实 Clash / mihomo HTTP API 面；仅 exact `PUT|PATCH /configs` 的**状态码注入**。
- 不覆盖 Windows service-mode、TUN 提权、系统代理、真实用户配置目录。
- 不替代 S10 的三平台 smoke / architecture ledger CI gate / PR-4S closeout。
- 不提供生产可用的 `CoreLifecyclePort`；消费者侧 adapter 属于 tauri `cfg(test)` 代码。

## 架构设计

### 组件与数据流

```text
                    ┌──────────────────────────────────────┐
                    │  Consumer tests (cfg(test) only)     │
                    │  e.g. ProcessCoreLifecycleAdapter    │
                    │  - require_bin_path()                │
                    │  - scrub inherited FAKE_CORE_*       │
                    │  - inject policy env per operation   │
                    └───────────────┬──────────────────────┘
                                    │ spawn + env
                                    v
┌───────────────┐   READY line    ┌──────────────────────────┐
│ ReadyBarrier  │ <────────────── │  fake-core binary        │
│ (parent TCP   │  RELEASE / EOF  │  parse_args → Mode       │
│  127.0.0.1:0) │ ──────────────> │  strict env parse        │
└───────────────┘                 │  optional hold listener  │
                                  │  optional HTTP thread    │
                                  │  signal_ready_and_wait…  │
                                  └───────────┬──────────────┘
                                              │ optional
                                              v
                                  ┌──────────────────────────┐
                                  │ 127.0.0.1:hold / :http   │
                                  │ exact PUT|PATCH /configs │
                                  │ status + body injection  │
                                  └──────────────────────────┘

Library (fake_core):
  FakeCoreCommand / ScopedChild / ReadyBarrier /
  ReadyAnnouncement / PathEnvGuard / resolve|require_bin_path
```

### 核心组件

| 组件                                                     | 位置                           | 职责                                                               |
| -------------------------------------------------------- | ------------------------------ | ------------------------------------------------------------------ |
| `main` / `run_check` / `run_start`                       | `src/main.rs`                  | 二进制入口；严格 env；hold/http；barrier；HTTP 轮询直到 stop       |
| `parse_args` / `Mode`                                    | `src/lib.rs`                   | 真实 argv → Check / Start                                          |
| `env_keys` + strict parsers                              | `src/lib.rs`                   | 稳定行为键与 fail-closed 数值解析                                  |
| `ReadyBarrier` / `ReadyConnection` / `ReadyAnnouncement` | `src/lib.rs`                   | 父侧屏障与 READY 帧编解码                                          |
| `signal_ready_and_wait_release`                          | `src/lib.rs`                   | 子侧 connect → READY → wait RELEASE/EOF                            |
| `FakeCoreCommand`                                        | `src/lib.rs`                   | 测试 spawn builder（check / start\_\* / env / stdio）              |
| `ScopedChild`                                            | `src/lib.rs`                   | drop 时 kill+wait；防泄漏                                          |
| `resolve_bin_path` / `require_bin_path`                  | `src/lib.rs`                   | 跨 crate 二进制发现与可操作错误                                    |
| `PathEnvLock` / `PathEnvGuard`                           | `src/lib.rs`                   | 串行化 `NYANPASU_FAKE_CORE` 的并发测试读写                         |
| `ProcessCoreLifecycleAdapter`                            | tauri `process_core_bridge.rs` | **S09 消费者**（非本 crate）；TempDir + policy + CoreLifecyclePort |

## 进程生命周期与屏障协议

### Check

1. 解析 argv 为 `Mode::Check`。
2. 可选写出 `CHECK_STDOUT` / `CHECK_STDERR`。
3. 解析 `CHECK_EXIT`（默认 0；非法 → exit 2）。
4. 以该码退出。无端口、无屏障。

### Start — immediate failure

1. 写出可选 start streams。
2. 若 `START_EXIT` 已设置且合法 → **立即**以该码退出。
3. 不绑定 hold/http，不连接 READY。

### Start — long-running

1. 严格解析全部数值 env；非法 → exit 2（在 barrier connect 之前）。
2. 无 `START_EXIT` 且无 `READY_ADDR` → exit 2。
3. 绑定 hold / http（若配置）：
   - 端口相等（含双 `0`）：单 listener + HTTP 线程；READY 中 `hold=` 与 `http=` 相同。
   - 否则分别绑定；hold 仅占用；http 为 nonblocking accept 循环。
4. `signal_ready_and_wait_release(READY_ADDR, announcement, 10s)`。
5. 收到 `RELEASE\n` 或父侧 EOF → 停 HTTP、join 线程、以 `EXIT_AFTER_RELEASE` 退出。
6. barrier 失败 → stop flag、join、exit 1。

### 清理不变量

- 测试路径优先 `spawn_scoped()`：`ScopedChild::drop` 在子进程仍存活时 `kill` + `wait`。
- hold listener 存活至 RELEASE 返回前（`_held` 持有 `TcpListener`）。
- HTTP 线程以 `AtomicBool` stop + nonblocking accept 轮询退出；不作为测试排序手段。
- 父侧 `wait_with_timeout` 超时会 kill 子进程并返回 `TimedOut`（安全网，非 happy-path 排序）。

## Env / Argv 契约

### Argv

完整 argv 形态（check / mihomo / clash-rs / premium，与 nyanpasu-utils 真实调用一致）见 [README.md § Argv shapes](README.md#argv-shapes)。未知 flags 或缺失 `-d` / `-f` / `-c` → stderr + exit `2`。

### 行为 env

完整键表与默认值见 [README.md § Environment contract](README.md#environment-contract)。设计要点：

- **Unset vs set-but-invalid**：unset 可走默认；set 必须可解析，否则 exit 2（禁止静默忽略导致假绿）。
- **`START_EXIT` 与 barrier 互斥语义**：`START_EXIT` 表示立即结束，不是长跑失败模式。
- **`NYANPASU_FAKE_CORE` 不在行为键集合内**：仅选择二进制路径；空串忽略。
- 消费者 adapter 应在注入 policy 前 **scrub** 继承的 `FAKE_CORE_*`，避免 CI/开发者 shell 污染矩阵语义。

### READY 帧

```text
READY\n
READY hold=<u16>\n
READY http=<u16>\n
READY hold=<u16> http=<u16>\n
```

未知字段或非法端口 → 父侧 `InvalidData`。

## 确定性测试规则

- 步骤边界以 barrier / oneshot / `try_wait` 为准，**禁止**用 sleep 表达 “子进程已就绪 / 已释放端口”。
- `thread::sleep` 仅允许出现在：accept 超时轮询、HTTP stop 轮询、connect 重试安全网；注释须标明非排序用途。
- 同包集成测试使用 `CARGO_BIN_EXE_fake-core`。
- 跨 crate 使用 `require_bin_path()`，并在失败文案中保留稳定 `PREBUILD_COMMAND`。
- 并发修改/观测 `NYANPASU_FAKE_CORE` 必须持有 `PathEnvLock` / `PathEnvGuard`（不依赖 libtest 串行）。
- 临时目录与配置文件使用进程 PID + 时间戳命名的 scratch paths；不写真实用户配置目录。

## Cargo / 包边界

| 项               | 决策                                              |
| ---------------- | ------------------------------------------------- |
| Workspace member | `backend/Cargo.toml` 列出 `fake-core`             |
| `publish`        | `false`                                           |
| Dependencies     | 空（std only）；dev-dependencies 空               |
| Artifacts        | `lib` `fake_core` + `bin` `fake-core`             |
| Production link  | **禁止**；仅 tauri 等包以 **dev-dependency** 引用 |
| Packaging        | 不进入 sidecar / resources / 安装包               |

`dev-dependency` **不会**自动构建 bin，也**不会**为依赖包设置 `CARGO_BIN_EXE_fake-core`。这是 Cargo 语义，不是 bug；文档与 `require_bin_path` 错误文案必须显式要求 prebuild。

## 安全威胁模型

| 威胁                               | 影响                                 | 缓解                                                                                                 |
| ---------------------------------- | ------------------------------------ | ---------------------------------------------------------------------------------------------------- |
| 误当作生产 sidecar 分发            | 用户运行无能力假核心                 | `publish = false`；不进 resources/sidecar；描述与 README 明确 test-only；S09 consumer 仅 `cfg(test)` |
| 环境注入污染测试                   | 错误 exit/port/barrier 导致假绿/假红 | 严格解析；adapter scrub 行为键；`START_EXIT`/barrier 语义分离                                        |
| 非本机绑定                         | 测试服务暴露到网络                   | 强制 `127.0.0.1`                                                                                     |
| 子进程/端口泄漏                    | CI 资源耗尽、端口占用 flaky          | `ScopedChild` kill+wait；barrier 后释放 hold；超时 kill                                              |
| 将 status injection 误认为安全边界 | 错误信任假 HTTP                      | 文档标明仅测试注入；无鉴权、无完整 API                                                               |
| `NYANPASU_FAKE_CORE` 指向恶意路径  | 测试跑任意二进制                     | 仅测试/harness 使用；不在生产路径读取                                                                |

## 设计决策与权衡

| 日期    | 决策                                                   | 理由                                     | 影响                             |
| ------- | ------------------------------------------------------ | ---------------------------------------- | -------------------------------- |
| 2026-07 | real argv + `FAKE_CORE_*` env，而非 `--check-ok` flags | 与生产 spawn 路径同构，减少测试/生产分叉 | 测试必须设 env；文档需列完整键表 |
| 2026-07 | TCP READY/RELEASE 屏障                                 | 确定性父子同步，避免 sleep flaky         | 长跑 start 强制 `READY_ADDR`     |
| 2026-07 | 动态端口经 READY 宣布                                  | 消除 “先探空闲端口再绑定” 的 TOCTOU      | 父侧必须解析 READY 字段          |
| 2026-07 | exact `/configs` only                                  | 只服务 apply 失败矩阵所需最小表面        | 非 prefix 匹配；其它路径 404     |
| 2026-07 | std-only 空依赖                                        | 跨平台、可预测、无运行时耦合             | 手写极简 HTTP；无完整协议栈      |
| 2026-07 | fail-closed invalid env                                | 曾静默忽略非法值导致假成功               | exit 2 + 稳定错误串              |
| 2026-07 | `ScopedChild` 默认 kill-on-drop                        | 断言失败不得泄漏                         | 需 `into_inner` 才能解除         |

## 已知限制

1. **Focused consumer tests 需 prebuild**

   `cargo test -p clash-nyanpasu --lib ...` 等不会仅因 path dev-dep 构建 `fake-core` bin。须先 `cargo build -p fake-core` 或 `cargo test -p fake-core`。

2. **Windows PID 断言有限**

   协议测试中 “drop 后 `/proc/<pid>` 消失” 仅 Linux 启用。Windows 上仍执行 kill+wait，但无等价 tombstone 硬断言。

3. **HTTP 仅为 status injection**

   不解析 body 语义、不实现其它 Clash 路由、不支持 HTTPS。

4. **非生产生命周期**

   不模拟 service 提升、TUN、外部 DNS、真实流量转发。

5. **平台 smoke 不在本包**

   Windows service-mode / TUN 权限 / 三平台证据链属 **S10**，不在本包自动化范围。

## S09 / S10 所有权边界

### S09 拥有（本包 + 直接消费者）

- `backend/fake-core` 协议、二进制、同包 protocol tests。
- tauri `cfg(test)` `ProcessCoreLifecycleAdapter` 与 process matrix（check fail、immediate start fail、apply 500、port conflict/release、lease serialization、clean stop/reap、two-graph isolation、process-level `change_core` rollback 等）。
- instance-owned `RebuildCoordinator` 去全局化（dispatcher 侧，非本 crate 实现，但同属 S09 交付）。
- 文档中的 prebuild/discovery 契约与 “never packaged” 约束。

### S09 不拥有 / 明确不关闭

- **不得**因本包或 focused S09 matrix 宣称 `cargo test --workspace --all-features` 全绿。
- **不得**因 S09 完成宣告 PR-4S 完成。
- PR-5/6 residual：legacy `Config` / `CoreManager::global()` 与 full graph desired-state isolation 不在 S09 关闭范围。

### S10 仍负责

- architecture ledger CI gate / residual budget。
- Windows / macOS / Linux smoke 与可审计 evidence。
- PR-4 review final disposition 表。
- roadmap closeout 与 **PR-4S 完成宣告**。
- 超出 fake-core 协议的真实 service/TUN 权限路径手工验证。

## 变更历史

### 2026-07-18 — 模块落地与文档实化

**变更内容：** 实现 test-only `fake-core` 包（lib + bin + protocol tests）；以 README/DESIGN 替换 gen-docs 占位内容。

**变更理由：** PR-4S S09 process matrix 需要确定性假核心与稳定跨 crate 发现契约；占位文档不可用。
