# PR-5 bridge 阶段 · runtime 侧实施计划（2026-08-31）

- **依据**：控制面设计（2026-08-08，修订 A1/A2/A6 为规范性裁定）、app 集成设计（2026-08-12 §202 行 wire 裁定）、审计资产处置（2026-08-12 §4）、PR-A～D 编排计划（2026-08-13 §0）、v2 实施审计入口（2026-08-13 §4 L10）
- **范围**：仅 `backend/nyanpasu-runtime` submodule（外加一张明确标注归属的 app 仓守卫卡）。app 侧换线（Tauri commands、`core/clash/core.rs` 退役、`ipc.rs` 三 statics 吸收）由并行编写的 app-switch 计划负责，本文不覆盖。
- **停止线状态**：用户已解除"实施到 legacy bridge 阶段之前停止"的限制，bridge/清算阶段获授权。
- **两个工作包**：① daemon 版本推进（解 L10）；② v1 IPC wire 删除（PR-E 清算的 runtime 半边）。二者**不是**一个 PR，排序见 §7。

---

## 1. 事实基线（全部逐条核实，勿重新推导）

| #   | 事实                                                                                                                                                                                                                     | 证据                                                                                                                                                                                                                     |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| F1  | app 仓 main = `3d5a518d`（#5116 squash），其上是 `f8e09521`（#5070 PR-5-pre）。工作树与 main 逐字节相同                                                                                                                  | 会话给定，已核                                                                                                                                                                                                           |
| F2  | submodule gitlink = `6717e44` = runtime main = runtime PR #391 的 merge commit（被合入 tip `e523ada`）                                                                                                                   | `git -C backend/nyanpasu-runtime log --oneline -1`                                                                                                                                                                       |
| F3  | `git -C backend/nyanpasu-runtime describe --tags` = `v2.0.0-rc.1-29-g6717e44`——即 `v2.0.0-rc.1` tag 落后 main **29 个提交**，整套控制面都在 tag 之后                                                                     | 已实跑                                                                                                                                                                                                                   |
| F4  | runtime 全部 crate 版本仍是 `2.0.0-rc.1`（`nyanpasu_service`、`nyanpasu_ipc`、`crates/nyanpasu-service-runtime`）；`crates/nyanpasu-{core-manager,core-metadata,clash-api}` 是 `1.0.0-rc.1`；`nyanpasu-utils` 是 `0.1.0` | `nyanpasu_service/Cargo.toml:3`、`nyanpasu_ipc/Cargo.toml:3`、`crates/nyanpasu-service-runtime/Cargo.toml:6`                                                                                                             |
| F5  | daemon 在 `/status` 上报的版本是 **`nyanpasu-service-runtime` 的** `CARGO_PKG_VERSION`，不是 `nyanpasu_service` 的                                                                                                       | `crates/nyanpasu-service-runtime/src/consts.rs:3`（`APP_VERSION`）→ `src/server/routing/status.rs:23`                                                                                                                    |
| F6  | 两者必须锁步，且 `publish.yml` 会一起 bump                                                                                                                                                                               | `crates/nyanpasu-service-runtime/Cargo.toml:3-5` 注释；`.github/workflows/publish.yml:76-84`（对除 `nyanpasu-utils` 外的每个 workspace member 调 `cargo set-version`）                                                   |
| F7  | `check.ts` 从 submodule 工作树的 `nyanpasu_service/Cargo.toml` 解析版本，拼成 tag `v<version>`，从 `libnyanpasu/nyanpasu-runtime` 的 release 下载                                                                        | `scripts/check.ts:674-712`（`NYANPASU_SERVICE_MANIFEST` / `getNyanpasuServiceVersion` / `getNyanpasuServiceInfo`）                                                                                                       |
| F8  | GitHub 上**只有 `v2.0.0-rc.1` 这一个 v2 release**，资产齐全且命名与 `check.ts` 期望一致（`nyanpasu-service-<triple>.{zip,tar.gz}`）                                                                                      | `gh release list/view --repo libnyanpasu/nyanpasu-runtime` 已实跑                                                                                                                                                        |
| F9  | compat gate **只比较 major**，`2.0.0-rc.1` 已经放行——所以 rc.1 daemon 通过门禁却没有 `/v2/core/*`。这正是 L10                                                                                                            | `backend/tauri/src/core/service/compat.rs:11`（`REQUIRED_SERVICE_MAJOR = 2`）、`:44`（`version.major != REQUIRED_SERVICE_MAJOR`）                                                                                        |
| F10 | app 启动时的 daemon 自动升级判据是 `sidecar 版本 > 运行中 daemon 版本` 的 semver 比较                                                                                                                                    | `backend/tauri/src/utils/init/mod.rs:249-259`                                                                                                                                                                            |
| F11 | app 侧 compat gate 的输入来自 CLI `nyanpasu-service status --json`（子进程），CLI 内部再走 IPC `/status`                                                                                                                 | `backend/tauri/src/core/service/control.rs:326-351`；`crates/nyanpasu-service-runtime/src/cmds/status.rs:43,51`                                                                                                          |
| F12 | app 今天实际调用的 v1 IPC 只有四个：`start_core` / `stop_core` / `status` / `set_dns`                                                                                                                                    | `backend/tauri/src/core/clash/core.rs:270-272`、`:299-301`、`:329`、`:366`、`:703-704`                                                                                                                                   |
| F13 | app 侧 `apply_config` / `check_config` / `restart_core` / `recover_core` 的命中**全是 app 自身 facade / lease / mock 方法**，不是 IPC client                                                                             | `src/client/mod.rs:1622,1626,1676,1680,1734,1738`（`TestRunningCoreBridge` mock）、`src/client/core_bridge.rs:169,202,225`、`src/core/clash/core.rs:409-421`（`CoreLifecycleLease`）、`src/core/updater/instance.rs:216` |
| F14 | app 换线后的 Service host 面只用 `/v2/core/submit`、`/v2/core/operation`、`/v2/core/status`                                                                                                                              | `backend/tauri/src/core/actor_v2/endpoint.rs:297,317,330`                                                                                                                                                                |
| F15 | app **完全不消费** daemon 的 ws 事件流（`CoreStateChanged` / `CoreStatusChanged` / `CoreLog` 零命中）                                                                                                                    | 在 `backend/tauri/src` 上 grep `CoreStateChanged\|\.events()\|EVENT_URI` 无命中                                                                                                                                          |
| F16 | 服务端**没有任何地方注入 `DnsController`**：`ManagerOptions::dns` 默认 `None`，唯一的 `dns_controller(...)` 调用在测试里                                                                                                 | `crates/nyanpasu-core-manager/src/manager/mod.rs:222`（builder）、`crates/nyanpasu-core-manager/tests/dns_override.rs:111,127`（仅测试）；`nyanpasu-service-runtime` 全仓无命中                                          |
| F17 | `MacosDnsController` 是未编译未验证的 cfg(macos) 骨架（L1 已如实登记）                                                                                                                                                   | `crates/nyanpasu-core-manager/src/dns.rs:101-118` doc                                                                                                                                                                    |
| F18 | submodule 内提交**不触发** app 仓 husky：app 设了 `core.hooksPath=.husky/_`，submodule 未设，且 `.git/modules/backend/nyanpasu-runtime/hooks` 无活跃 hook                                                                | 已实跑 `git config --get core.hooksPath` 与 hooks 目录列举                                                                                                                                                               |
| F19 | `resolveSidecar` 在目标文件已存在且未加 `--force` 时直接返回 cached，**不校验版本**                                                                                                                                      | `scripts/check.ts:492-499`                                                                                                                                                                                               |

---

## 2. 分支拓扑与提交纪律

| 仓库                                                            | 分支                         | 基点 | 内容                                                   |
| --------------------------------------------------------------- | ---------------------------- | ---- | ------------------------------------------------------ |
| nyanpasu-runtime（submodule 工作树 `backend/nyanpasu-runtime`） | `refactor/drop-v1-core-wire` | 见下 | 阶段 2a + 2b                                           |
| clash-nyanpasu                                                  | 不新建分支                   | —    | 本计划只含一张 app 仓守卫卡（G1），归属待裁定（§9-D1） |

**基点规则**：阶段 1 的 `chore: bump version to v2.0.0-rc.2` 提交由 `publish.yml` 直接打到 runtime `main`（见 §3）。

- 若阶段 1 已完成：`git -C backend/nyanpasu-runtime fetch origin && git -C backend/nyanpasu-runtime switch -c refactor/drop-v1-core-wire origin/main`
- 若先开工：基于 `6717e44` 建分支，阶段 1 落地后 `git rebase origin/main`

**纪律（AGENTS.md §18）**：

- submodule 内的提交**绝不能动 app 仓的 gitlink**。gitlink 前移属于 app-switch 阶段的提交，不在本计划内。
- 全部显式路径 `git add <path>`，禁止 `git add .` / `-A` / `*`；提交前 `git -C backend/nyanpasu-runtime diff --cached --stat` 复核。
- subject 祈使句 ≤72 字符、不带句号；每个提交独立可构建（`cargo build --workspace` 通过）。
- 因 F18，submodule 内提交不会自动跑 clippy/fmt——**必须手工跑**（命令见 §8）。
- commitlint（`@commitlint/config-conventional`）只作用于 app 仓；runtime 仓无 commit-msg hook，但 `cliff.toml:97-113` 的 changelog 分组按 conventional 前缀走，所以仍用 `refactor(...)` / `feat(...)` / `test(...)`。

---

## 3. 阶段 1 · daemon 版本推进（解 L10）

### 3.1 问题的准确形状

L10 不是"compat gate 判错了"。gate 只比 major（F9），rc.1 本来就该放行。真正的问题只有一个：**`check.ts` 拉下来的那个二进制里没有 `/v2/core/*` 路由**，因为 tag `v2.0.0-rc.1` 落后 main 29 个提交（F3），而整套控制面都在这 29 个提交里。

所以修复动作是"让 `check.ts` 解析出的 tag 指向一个含控制面的 release"，而**不是**改 gate、不是改 `check.ts` 的解析逻辑。

### 3.2 目标版本裁定：`2.0.0-rc.2`

| 候选                       | 结论       | 理由                                                                                                                                                                                                                                                                                                                                         |
| -------------------------- | ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `2.0.0-rc.2`               | **采纳**   | `bump-version.py --op prerelease --preid rc` 对 `2.0.0-rc.1` 的产出就是它（`.github/scripts/bump-version.py:13,64+`）；semver 上 rc.2 > rc.1，因此 F10 的自动升级判据会让已安装的 rc.1 daemon 在 app 下次启动时被自动替换；prerelease 分支下 `publish.yml:112` 跳过 `CHANGELOGS.md` 写入，rc 通道保持开放                                    |
| `2.0.0`                    | **不采纳** | 控制面仍有未验证部分：L1（macOS DNS 是 Windows 上零编译零验证的骨架，F17）、L2（quarantine 死亡证明绕过 RuntimeBackend trait）、L4（facade 编排未实现）、F16（服务端根本没注入 DnsController）。app 侧 `/v2/core/*` 一次真机都没跑过。把第一个 stable v2 定成"没被任何调用方验证过的那一版"是错的。等 app cutover 跑通再 `--op release` 提升 |
| 手工编辑 Cargo.toml 版本号 | **禁止**   | `publish.yml:66-98` 自己算下一个版本并 `cargo set-version`。手工先 bump 到 rc.2 之后再跑 workflow 会得到 rc.3，且手工提交的 tag 与 release 不存在，`check.ts` 会 404                                                                                                                                                                         |

### 3.3 任务卡

| 卡     | 内容                                                                                                                                                                                                                    | 归属                                                            | 验证                                                                                                                                                                                                                                                                                        |
| ------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **V1** | 在 `libnyanpasu/nyanpasu-runtime` 上 workflow_dispatch 触发 `Publish`，参数 `versionType=prerelease`、`preid=rc`。产出：main 上一个 `chore: bump version to v2.0.0-rc.2` 提交 + tag `v2.0.0-rc.2` + 六平台 release 资产 | **用户动作项**（需要目标仓的 Actions 写权限，实施代理无法代劳） | `gh release view v2.0.0-rc.2 --repo libnyanpasu/nyanpasu-runtime --json assets --jq '.assets[].name'` 列出 16 个资产（8 个二进制 + 8 个 `.sha256`），含 `nyanpasu-service-x86_64-pc-windows-msvc.zip`                                                                                       |
| **V2** | V1 之后：`git -C backend/nyanpasu-runtime fetch origin && git -C backend/nyanpasu-runtime checkout origin/main`（**只动 submodule 工作树，不 `git add` app 仓的 gitlink**），确认三个版本号都变成 `2.0.0-rc.2`          | 实施代理                                                        | `grep -m1 '^version' backend/nyanpasu-runtime/nyanpasu_service/Cargo.toml backend/nyanpasu-runtime/nyanpasu_ipc/Cargo.toml backend/nyanpasu-runtime/crates/nyanpasu-service-runtime/Cargo.toml` 三行均为 `2.0.0-rc.2`；`git -C backend/nyanpasu-runtime describe --tags` 输出 `v2.0.0-rc.2` |
| **V3** | 清掉缓存的 rc.1 sidecar 再重拉。**因 F19，只跑 `pnpm prepare:check` 不会替换已存在的文件**                                                                                                                              | 实施代理                                                        | `rm -f backend/tauri/sidecar/nyanpasu-service-*` 后 `pnpm prepare:check`；`backend/tauri/sidecar/nyanpasu-service-x86_64-pc-windows-msvc.exe` 重新出现，且 `./backend/tauri/sidecar/nyanpasu-service-x86_64-pc-windows-msvc.exe -v` 打印 `2.0.0-rc.2`                                       |

> **worktree 警告**：AGENTS.md §17 规定 worktree 里的 `backend/tauri/sidecar/` 是指向主检出的符号链接。在 worktree 里执行 V3 的 `rm` 会删掉**主检出**的真实文件。要么在主检出里做 V3，要么接受主检出一并更新。

### 3.4 V1 落地之前的本地开发回退（已验证可行，不需要任何版本号改动）

app 侧 bridge 换线要在真 daemon 上跑，而 rc.2 release 尚不存在时，用本地构建替代下载：

```bash
# 1. 从 submodule 构建 daemon（build.rs 需要 git，可用）
cd backend/nyanpasu-runtime && cargo build --release -p nyanpasu-service

# 2. 放到 check.ts 期望的位置（Windows 示例；triple 取 `rustc -vV` 的 host）
cp backend/nyanpasu-runtime/target/release/nyanpasu-service.exe \
   backend/tauri/sidecar/nyanpasu-service-x86_64-pc-windows-msvc.exe
```

为什么这条路成立：

- `resolveSidecar` 见到文件已存在就返回 cached（F19，`check.ts:492-499`），所以后续 `pnpm prepare:check` **不会**用 rc.1 覆盖它；
- `tauri.conf.json:41-49` 把 `sidecar/nyanpasu-service` 列为 `externalBin`，`tauri dev` 会把它复制到 dev 可执行文件旁；
- `app_install_dir()` 就是"可执行文件所在目录"（`backend/tauri/src/utils/dirs.rs:174-181`），`SERVICE_PATH` 由它拼出（`src/core/service/mod.rs:13-16`），因此 app 的 install/status/update 全部落到这个本地构建上；
- 该二进制的 `APP_VERSION` 仍报 `2.0.0-rc.1`，但 gate 只比 major（F9），照样放行；它带着 main 的全部 `/v2/core/*` 路由。

**回退的两个已知代价，必须如实告知使用者**：

1. 版本号仍是 rc.1，所以 F10 的自动升级判据 `app_ver > server_ver` 为假——已安装的旧 daemon **不会**被自动替换，必须手工 `nyanpasu-service uninstall` 后重装（需提权）；
2. 这是 dev-loop 权宜，**不能**作为出货路径。出货必须走 V1。

---

## 4. 阶段 2 · v1 wire 删除集

### 4.1 授权依据

- 修订 **A1**（规范性）："BC 不是约束。删除 v1 wire 行为仿真与 legacy adapter 保真义务；仅保留协议版本 fail-closed 门。v1 endpoints / legacy error 文本 golden / `CoreManager` facade 兼容包装全部不做。"
- app 集成设计第 202 行："wire | IPC **v2 only**（`/v2/core/*`）；v1 删除（修订 A1）"。
- 编排计划 §0.1：v1 删除顺延到 bridge/清算阶段。

设计正文 §35「兼容验收」里的"v1 IPC endpoint、payload 和 legacy error 文本不变"是**被 A1 覆盖的原文**（文档头已声明"冲突处以修订记录为准"），不构成阻碍。

### 4.2 删除集

| 端点                 | contract op                            | api 模块                                                                                                                        | client shortcut                          | 路由文件                         | bridge 方法                                                                 | CLI 子命令                | 阶段   |
| -------------------- | -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------- | -------------------------------- | --------------------------------------------------------------------------- | ------------------------- | ------ |
| `POST /core/restart` | `CoreRestart`（`contract.rs:87-94`）   | `api/core/restart.rs` 整文件                                                                                                    | `restart_core`（`shortcuts.rs:49-51`）   | `routing/core/restart.rs` 整文件 | `restart`（`manager_bridge.rs:329-346`）                                    | `RpcCommand::RestartCore` | **2a** |
| `POST /core/apply`   | `CoreApply`（`contract.rs:96-104`）    | `api/core/apply.rs` 整文件（含 `CoreApplyReq` / `CoreApplyData` / `CoreApplyRes` / `ApplyOutcomeKind` / `CORE_APPLY_ENDPOINT`） | `apply_config`（`shortcuts.rs:57-67`）   | `routing/core/apply.rs` 整文件   | `apply`（`manager_bridge.rs:348-382`）+ `map_apply_outcome`（`:903-942`）   | `RpcCommand::ApplyConfig` | **2a** |
| `POST /core/recover` | `CoreRecover`（`contract.rs:116-124`） | `api/core/recover.rs` 整文件                                                                                                    | `recover_core`（`shortcuts.rs:112-114`） | `routing/core/recover.rs` 整文件 | `recover`（`manager_bridge.rs:416-423`）                                    | `RpcCommand::RecoverCore` | **2a** |
| `POST /core/start`   | `CoreStart`（`contract.rs:66-74`）     | `api/core/start.rs` 整文件                                                                                                      | `start_core`（`shortcuts.rs:41-43`）     | `routing/core/start.rs` 整文件   | `start`（`manager_bridge.rs:280-315`）+ `MSG_CORE_ALREADY_RUNNING`（`:60`） | `RpcCommand::StartCore`   | **2b** |
| `POST /core/stop`    | `CoreStop`（`contract.rs:76-84`）      | `api/core/stop.rs` 整文件                                                                                                       | `stop_core`（`shortcuts.rs:45-47`）      | `routing/core/stop.rs` 整文件    | `stop`（`manager_bridge.rs:317-327`）+ `MSG_CORE_ALREADY_STOPPED`（`:61`）  | `RpcCommand::StopCore`    | **2b** |

**2a / 2b 的划分依据（F12/F13 实证）**：app 今天对 IPC v1 的调用只有 `start_core` / `stop_core` / `status` / `set_dns` 四处。`/core/restart`、`/core/apply`、`/core/recover` 在 app 侧**零调用**——`Instance::restart`（`core.rs:306-313`）标了 `#[allow(dead_code)]` 且实现是 `self.stop().await` + `self.start().await`，不走 `/core/restart`。因此 2a **可以在 app 换线之前独立落地**，2b 必须等 app 换线。

`MSG_CORE_NOT_STARTED`（`manager_bridge.rs:62`）**保留**：除 `restart` 外还被 `impl From<ManagerError> for OpError`（`:126`）使用，那条路径 v2 也走。

### 4.3 保留集（逐条给出保留理由，不是"忘了删"）

| 端点                                                     | 保留理由                                                                                                                                                                                                                                                |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GET /status`（`Status`, `contract.rs:56-64`）           | compat gate 的**唯一**数据来源：`control::status()` 起子进程跑 `nyanpasu-service status --json`（`control.rs:326-330`），CLI 内部再打 `/status`（`cmds/status.rs:51`）。删掉它 gate 就瞎了                                                              |
| `GET /logs/retrieve`、`GET /logs/inspect`                | v2 没有等价物；CLI `rpc inspect-logs` 是它们唯一的消费者，属于诊断面而非生命周期面                                                                                                                                                                      |
| `POST /network/set_dns`                                  | **无可用替代**。F16：服务端从未注入 `DnsController`，`ManagerOptions::dns` 恒为 `None`；F17：`MacosDnsController` 是未编译未验证的骨架（L1）。此刻删它等于在没有接替者的情况下删掉 macOS TUN DNS 能力。见 §7 的跨计划契约 X3                            |
| `GET /ws/events`                                         | app 目前零消费（F15），但这是 daemon 唯一的推流面，L7 明确把它留给 PR-D 精化。`CoreStateChanged` 变体的处置见 §9-D2                                                                                                                                     |
| `POST /core/check`（`CoreCheck`, `contract.rs:106-114`） | **修订 A2 明文保留**："独立 `Check` 命令保留但降格为咨询（只读、semaphore 限并发、不进 mutating 队列）"。`v2.rs:11-13` 也写明 v2 故意不带 check。app 用本地 `CoreInstance::check_config_`（`core.rs:502`）做校验，保留它对 app 零成本。替代方案见 §9-D4 |

### 4.4 受影响的测试（全部已定位到函数名）

| 文件                                    | 测试                                                                                 | 处置                                                                                                                                                                                                                                                                                                                           |
| --------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `nyanpasu_ipc/src/api/contract.rs:194`  | `every_operation_keeps_its_legacy_path_and_method`                                   | 删掉 `CoreStart`/`CoreStop`/`CoreRestart` 三条断言（2b），保留 `Status`/`LogsRetrieve`/`LogsInspect`/`NetworkSetDns`                                                                                                                                                                                                           |
| `nyanpasu_ipc/src/api/contract.rs:242`  | `every_s8_operation_is_addressed_as_the_report_says`                                 | 2a 删 `CoreApply`/`CoreRecover` 两条；只剩 `CoreCheck` 一条时整个测试并入 `every_operation_keeps_its_legacy_path_and_method`                                                                                                                                                                                                   |
| `nyanpasu_ipc/src/api/contract.rs:224`  | `every_v2_operation_keeps_its_declared_address`                                      | 不动                                                                                                                                                                                                                                                                                                                           |
| `nyanpasu_ipc/tests/wire_golden.rs:813` | `the_core_apply_request_is_pinned`                                                   | 2a 删除                                                                                                                                                                                                                                                                                                                        |
| `nyanpasu_ipc/tests/wire_golden.rs:856` | `the_core_apply_response_is_pinned`                                                  | 2a 删除                                                                                                                                                                                                                                                                                                                        |
| `nyanpasu_ipc/tests/wire_golden.rs:691` | `the_apply_outcome_kinds_are_pinned`                                                 | 2a **改写**为 pin `ReconcileOutcomeKind`（`api/core/v2.rs:97-106`，比 `ApplyOutcomeKind` 多一个 `started`），别直接删——这是 outcome 词表的唯一 golden                                                                                                                                                                          |
| `nyanpasu_ipc/tests/wire_golden.rs:248` | `the_core_start_request_is_pinned`                                                   | 2b 删除；`every_core_type_tag_is_pinned`（`:260`）依赖 `CoreStartReq` 承载 core type，改为用 `CoreCommandInfo::Reconcile` 的 `core_type` 字段承载                                                                                                                                                                              |
| `nyanpasu_ipc/tests/wire_golden.rs:844` | `the_core_check_request_is_pinned`                                                   | 保留（`/core/check` 保留）                                                                                                                                                                                                                                                                                                     |
| `nyanpasu_ipc/tests/wire_golden.rs:233` | `the_legacy_core_error_envelopes_are_pinned`                                         | 保留：pin 的是 `R` 信封形状，与端点无关                                                                                                                                                                                                                                                                                        |
| `nyanpasu_ipc/tests/roundtrip.rs:439`   | `rest_roundtrip`                                                                     | 2a 起从 `test_router`（`:421-433`）与断言里摘掉对应路由；2b 摘掉 start/stop/restart 段，只留 status / logs / set_dns / ws                                                                                                                                                                                                      |
| `nyanpasu_ipc/tests/roundtrip.rs:722`   | `apply_config_roundtrip`                                                             | 2a **改写为 v2 版本**：语义是"rolled-back 以成功信封过线，调用方不靠 code 就能分辨"。改为打 `CORE_V2_OPERATION_ENDPOINT`、返回 `phase=succeeded` + `OperationOutputInfo::Reconciled{ outcome: RolledBack, failed_apply: Some(..) }`，断言不变。**不要直接删**——删了这条属性就没有传输层覆盖了                                  |
| `nyanpasu_ipc/tests/roundtrip.rs:660`   | `body_less_posts_send_no_body_and_no_content_type`                                   | 2b：删完 v1 后**没有任何 `Req<'a> = ()` 的 POST 操作**（`CoreV2Submit`/`CoreV2Operation` 都带 body）。在测试文件里就地定义一个 `IpcOperation` impl（`METHOD = POST`，`PATH = "/test/body-less"`，`Req<'a> = ()`，`Data = ()`），挂到测试 router 上，保住 `Client::call`（`client/mod.rs:199`）的 `None` 分支（`:204-206`）覆盖 |
| `nyanpasu_ipc/tests/roundtrip.rs:683`   | `json_posts_send_the_exact_payload`                                                  | 2b 改用 `CoreV2Submit` + `CoreSubmitReq` 载荷                                                                                                                                                                                                                                                                                  |
| `routing/tests.rs:118`                  | `stopping_an_idle_core_keeps_the_legacy_error_envelope`                              | 2b 删除；`MSG_CORE_ALREADY_STOPPED` 一并删                                                                                                                                                                                                                                                                                     |
| `routing/tests.rs:139`                  | `restart_before_any_start_reports_the_legacy_error`                                  | 2a 删除                                                                                                                                                                                                                                                                                                                        |
| `routing/tests.rs:222`                  | `every_operation_is_mounted_where_its_contract_says`                                 | 按阶段裁剪 `addresses` 数组（`:225-235`），并**补上 `CoreV2Submit`/`CoreV2Operation`/`CoreV2Status`**——现在这三个 v2 端点不在挂载探测里                                                                                                                                                                                        |
| `routing/tests.rs:366`                  | `applying_to_a_stopped_core_reports_not_started_with_its_kind`                       | 2a 删除（`v2_submit_stop_runs_to_a_classified_terminal_failure`，`:547`，已覆盖 v2 侧的分类失败）                                                                                                                                                                                                                              |
| `routing/tests.rs:398`                  | `checking_an_unresolvable_config_answers_in_the_envelope`                            | 保留                                                                                                                                                                                                                                                                                                                           |
| `routing/tests.rs:428`                  | `applying_without_a_core_binary_reports_binary_not_found`                            | 2a **改写**为经 `/v2/core/submit` 提交 Reconcile：`binary_not_found` 的分类路径必须保留覆盖                                                                                                                                                                                                                                    |
| `routing/tests.rs:461`                  | `recovering_without_a_quarantine_succeeds`                                           | 2a **改写**为 `CoreCommandInfo::Recover` 经 v2 submit                                                                                                                                                                                                                                                                          |
| `routing/tests.rs:132,153,261,281`      | 四处 `CoreStopRes<'static>` 类型标注                                                 | 2b 改为 `nyanpasu_ipc::api::R<'static, ()>`（`CoreStopRes` 只是它的别名）                                                                                                                                                                                                                                                      |
| `manager_bridge.rs:1423`                | `apply_outcomes_map_onto_the_wire_kinds`                                             | 2a **改指** `map_reconcile_outcome`（`:869`）+ `ReconcileOutcomeKind`，并补上 `Started` 用例（`map_apply_outcome` 在 `Started` 上是 `unreachable!`，`:930-932`）                                                                                                                                                               |
| `manager_bridge.rs:1470`                | `a_rolled_back_apply_reports_the_old_revision_and_the_failure`                       | 2a 同上改指 `map_reconcile_outcome`                                                                                                                                                                                                                                                                                            |
| `manager_bridge.rs:1484`                | `durability_warnings_unwrap_to_the_real_outcome`                                     | 2a 同上改指 `map_reconcile_outcome`                                                                                                                                                                                                                                                                                            |
| `manager_bridge.rs:1517`                | `the_not_started_failure_keeps_the_legacy_wire_string`                               | 保留（`MSG_CORE_NOT_STARTED` 保留）                                                                                                                                                                                                                                                                                            |
| `manager_bridge.rs:1961`                | `the_legacy_core_error_strings_are_protocol`                                         | 2b 删掉 `MSG_CORE_ALREADY_RUNNING`/`MSG_CORE_ALREADY_STOPPED` 两条断言，保留 `MSG_CORE_NOT_STARTED` 那条                                                                                                                                                                                                                       |
| `cmds/mod.rs:270`                       | `LEGACY_INVOCATIONS`（`:270-341`）/ `every_legacy_invocation_still_parses`（`:432`） | 见 E6 卡——这是本阶段唯一一处**必须显式打破**的 BC 断言                                                                                                                                                                                                                                                                         |
| `cmds/mod.rs:343`                       | `NEW_INVOCATIONS`（`:343+`）/ `every_new_invocation_parses`（`:440`）                | 同上，摘掉被删子命令的条目                                                                                                                                                                                                                                                                                                     |

---

## 5. 阶段 2a 任务卡（app 换线**之前**即可落地）

分支：`refactor/drop-v1-core-wire`。每张卡 = 一个提交。

| 卡     | 内容                                                                                                                                                                                                                                                                                                                                                                               | 触及文件                                                                                                                                                                                                                                                                                                                                                                | 验证                                                                                                                                                                                                                                                                             |
| ------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **E1** | 删除 `/core/apply` 的整条链路：contract op、`api/core/apply.rs`、`api/core/mod.rs` 的 `pub mod apply;`、client `apply_config`、`routing/core/apply.rs` + `routing/core/mod.rs` 的 `pub mod apply;` 与 `.register(CoreApply, ...)`、`manager_bridge.rs` 的 `apply()` 与 `map_apply_outcome()`、CLI `RpcCommand::ApplyConfig` 与其 match 臂、以及 §4.4 中标 2a 的 apply 相关测试改写 | `nyanpasu_ipc/src/api/{contract.rs,core/mod.rs,core/apply.rs}`、`nyanpasu_ipc/src/client/shortcuts.rs`、`nyanpasu_ipc/tests/{wire_golden.rs,roundtrip.rs}`、`crates/nyanpasu-service-runtime/src/server/routing/core/{mod.rs,apply.rs}`、`crates/nyanpasu-service-runtime/src/server/manager_bridge.rs`、`crates/nyanpasu-service-runtime/src/cmds/{mod.rs,rpc/mod.rs}` | `cd backend/nyanpasu-runtime && cargo test --workspace --all-features` 全绿；`rg -n 'CoreApply\|CORE_APPLY_ENDPOINT\|ApplyOutcomeKind\|map_apply_outcome' backend/nyanpasu-runtime --glob '!target'` 零命中                                                                      |
| **E2** | 删除 `/core/recover` 整条链路（同 E1 的六个层次）；`routing/tests.rs:461` 改写为经 `/v2/core/submit` 提交 `CoreCommandInfo::Recover`                                                                                                                                                                                                                                               | 同类文件集                                                                                                                                                                                                                                                                                                                                                              | `cargo test -p nyanpasu-service-runtime --all-features` 全绿；`rg -n 'CoreRecover\|CORE_RECOVER_ENDPOINT\|recover_core' backend/nyanpasu-runtime --glob '!target'` 零命中（quarantine 清除能力经 `CoreCommandInfo::Recover` → orchestrator → `manager.recover_quarantine` 仍在） |
| **E3** | 删除 `/core/restart` 整条链路；`routing/tests.rs:139` 删除                                                                                                                                                                                                                                                                                                                         | 同类文件集                                                                                                                                                                                                                                                                                                                                                              | `cargo test --workspace --all-features` 全绿；`rg -n 'CoreRestart\|CORE_RESTART_ENDPOINT\|restart_core' backend/nyanpasu-runtime --glob '!target'` 只剩 `manager.restart()`（v2 内部路径）与 `cmds/restart.rs`（daemon 自身重启，非核心）                                        |
| **E4** | 把 `every_operation_is_mounted_where_its_contract_says`（`routing/tests.rs:222`）的 `addresses` 补上 `CoreV2Submit`/`CoreV2Operation`/`CoreV2Status`；`the_apply_outcome_kinds_are_pinned` 改 pin `ReconcileOutcomeKind`                                                                                                                                                           | `crates/nyanpasu-service-runtime/src/server/routing/tests.rs`、`nyanpasu_ipc/tests/wire_golden.rs`                                                                                                                                                                                                                                                                      | `cargo test --workspace --all-features` 全绿；人工确认三个 v2 路径出现在断言数组里                                                                                                                                                                                               |

> E1–E3 各自都必须独立可构建。执行顺序 E1 → E2 → E3 → E4（E4 依赖 E1 已把 `ApplyOutcomeKind` 移走）。若某张卡拆开后不可构建，说明拆分点选错了，合并成一个提交而不是提交坏代码。

---

## 6. 阶段 2b 任务卡（**必须**在 app 换线合入之后）

前置硬条件（逐条可验证，全部满足才能开工）：

1. app 仓 `backend/tauri/src/core/clash/core.rs` 中 `nyanpasu_ipc::client::shortcuts::Client::service_default().start_core(...)`（`:270-272`）与 `.stop_core()`（`:299-301`）已消失；
2. `rg -n 'start_core\|stop_core' backend/tauri/src` 只剩 app 自身的 facade / lease 方法（F13 那一类），无 IPC client 命中；
3. app 换线分支已合入 app main 且 gitlink 已前移。

| 卡                                                   | 内容                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | 触及文件                                                                                                                                                                                                                                                                                  | 验证                                                                                                                                                                                                                                                                                                                |
| ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **E5**                                               | 删除 `/core/start` + `/core/stop` 整条链路（contract / api 模块 / client shortcut / 路由 / bridge 方法 / `MSG_CORE_ALREADY_RUNNING` / `MSG_CORE_ALREADY_STOPPED` / CLI `StartCore`+`StopCore`），并按 §4.4 处置 `routing/tests.rs:118`、`manager_bridge.rs:1961`、四处 `CoreStopRes` 类型标注                                                                                                                                                                                         | `nyanpasu_ipc/src/api/{contract.rs,core/mod.rs,core/start.rs,core/stop.rs}`、`nyanpasu_ipc/src/client/shortcuts.rs`、`crates/nyanpasu-service-runtime/src/server/routing/core/{mod.rs,start.rs,stop.rs}`、`.../manager_bridge.rs`、`.../routing/tests.rs`、`.../cmds/{mod.rs,rpc/mod.rs}` | `cargo test --workspace --all-features` 全绿；`rg -n 'CoreStart\|CoreStop\|CORE_START_ENDPOINT\|CORE_STOP_ENDPOINT' backend/nyanpasu-runtime --glob '!target'` 零命中                                                                                                                                               |
| **E6**                                               | 处理 CLI BC 断言：把 `rpc start-core` / `stop-core` / `restart-core` / `apply-config` / `recover-core` 相关条目从 `LEGACY_INVOCATIONS`（`cmds/mod.rs:270-341`）与 `NEW_INVOCATIONS`（`:343+`）中移除，并**改写 `LEGACY_INVOCATIONS` 的 doc 注释**（现文：`Every invocation that worked before S5. If one of these stops parsing, the change is breaking and must be reverted, not "fixed" here.`）——注明这是 2.0 rc 通道内经修订 A1 授权的一次协议破坏，列出被删子命令与替代路径      | `crates/nyanpasu-service-runtime/src/cmds/mod.rs`                                                                                                                                                                                                                                         | `cargo test -p nyanpasu-service-runtime --all-features cmds::tests` 全绿（`every_legacy_invocation_still_parses` `:432`、`every_new_invocation_parses` `:440`）；`rg -n 'start-core\|stop-core\|restart-core\|apply-config\|recover-core' backend/nyanpasu-runtime/crates/nyanpasu-service-runtime/src/cmds` 零命中 |
| **E7**（条件卡，仅当 §9-D3 裁定为"补 v2 CLI"时执行） | 在 `RpcCommand` 里补 v2 控制面子命令，填上 E5/E6 留下的调试面空洞：`Submit { --operation-id, --command <reconcile\|stop\|recover>, --core-type, --config-file, --expected-digest, --expected-applied }` 打 `Client::submit_core`；`Operation { --operation-id, --wait-ms }` 打 `Client::core_operation`；`CoreStatus` 打 `Client::core_status_v2`。三个都把 `OperationInfo` / `CoreInfos` 以 `serde_json::to_string_pretty` 打印（沿用 `ApplyConfig` 现有写法，`rpc/mod.rs:208-215`） | `crates/nyanpasu-service-runtime/src/cmds/rpc/mod.rs`、`crates/nyanpasu-service-runtime/src/cmds/mod.rs`（`NEW_INVOCATIONS`）                                                                                                                                                             | `cargo test -p nyanpasu-service-runtime --all-features` 全绿；新增 argv 进 `NEW_INVOCATIONS` 且 `every_new_invocation_parses` 通过；`Cli::command().debug_assert()`（`the_cli_definition_is_internally_consistent`，`cmds/mod.rs:427`）通过                                                                         |

---

## 7. 排序依赖与跨计划契约

```text
V1（用户跑 Publish workflow）
   └─> V2/V3（拉 rc.2、清缓存、重下 sidecar）
          └─> app-switch 阶段可在真 daemon 上验证 /v2/core/*
                 └─> app 换线合入 + gitlink 前移
                        └─> 阶段 2b（E5/E6[/E7]）

阶段 2a（E1–E4）与上面这条链**并行无依赖**——app 侧对这三个端点零调用（F12/F13 实证）。
```

**跨计划契约**（app-switch 计划必须知道的三条）：

- **X1**：本计划**不删** `GET /status`。app 的 compat gate 继续走 `nyanpasu-service status --json` 子进程（F11），app-switch 不必为它准备替代路径。
- **X2**：阶段 2b 的开工判据是 §6 的三条前置条件。app-switch 计划落地后需要显式通知，否则 2b 不得开工。反向也成立：app-switch **不得**在 2b 落地前把 gitlink 指向删除后的 runtime——那会让 `core/clash/core.rs` 编译不过。
- **X3**：本计划**保留** `POST /network/set_dns`（理由见 §4.3）。app-switch 若打算退役 `core/clash/core.rs`，必须为其中的 macOS TUN DNS 分支（`core.rs:703-704`）保留一个调用方，或显式接受 macOS DNS 回退并记录。**不要**假设 daemon 侧的 `DnsController` 会接手——F16 证明它一次都没被注入过。

---

## 8. 验证命令总表

```bash
# runtime 全量（PowerShell 下必须加 --config build.rustc-wrapper='' 以禁用 kache）
cd backend/nyanpasu-runtime && cargo test --workspace --all-features
# 基线：484 passed / 24 ignored（2026-08-30 记录，见审计入口 §2）

# 单 crate 收敛
cd backend/nyanpasu-runtime && cargo test -p nyanpasu-ipc --all-features
cd backend/nyanpasu-runtime && cargo test -p nyanpasu-service-runtime --all-features
# 注意 nyanpasu_ipc 的 roundtrip 测试声明了 required-features = ["client","server"]
# （nyanpasu_ipc/Cargo.toml:6-9），必须 --all-features 才会被编译进去

# 格式与 lint（因 F18，submodule 内提交不触发 husky，必须手工跑）
cd backend/nyanpasu-runtime && cargo fmt --all -- --check
cd backend/nyanpasu-runtime && cargo clippy -p nyanpasu-ipc --all-targets --all-features
cd backend/nyanpasu-runtime && cargo clippy -p nyanpasu-service-runtime --all-targets --all-features
# 不要跑 workspace 级 clippy：本机在 clash-api/src/api/connections.rs:208 的 opaque type
# 上确定性 ICE（审计入口 §5），与本轮改动无关。逐 crate 跑可通过。

# app 侧回归（阶段 1 之后、以及 2b 前置条件核对时）
cd backend && cargo build -p fake-core     # process_core_bridge 的 11 个测试依赖它
cd backend && cargo test -p clash-nyanpasu --lib
# 基线：447 passed / 0 failed / 1 ignored
# `cargo check` 在本 workspace 不可用（boa_engine 确定性 ICE），纯编译检查用：
cd backend && cargo build -p clash-nyanpasu --lib

# sidecar 版本核对
grep -m1 '^version' backend/nyanpasu-runtime/nyanpasu_service/Cargo.toml
./backend/tauri/sidecar/nyanpasu-service-x86_64-pc-windows-msvc.exe -v
```

---

## 9. 实施环境注意（前几轮会话的实测记录，逐字保留）

- **满量依赖重建在默认 `-j 32` 下会 OOM**，表现为 `STATUS_STACK_BUFFER_OVERRUN`，并伴随极具误导性的 `only metadata stub found for rlib dependency std`。处置：`cargo clean -p <崩掉的 crate>` 之后用 `-j 6` 配 `CARGO_INCREMENTAL=0` 重跑。**不要**据此推断工具链不兼容，也不要 pin 工具链日期（用户已明确否决）。
- **`cargo check` 在 boa_engine 上确定性 ICE**，用 `cargo build` 代替。
- **仓库的 lint 闸门是 `.lintstagedrc.js` 里的 clippy，没有 `-D warnings`**：`cargo clippy --manifest-path=./backend/Cargo.toml --all-targets --all-features`。不要自行加 `-D warnings` 后把既有 warning 当成本轮引入的问题。
- **PowerShell 下禁用 kache 必须写成 `--config build.rustc-wrapper=''`（空串）**，不能省略。
- **共享的 `backend/target` 会出现 `拒绝访问 (os error 5)` 式增量损坏**，让每个 `--emit=metadata` 构建都不可靠。真需要跑 clippy 时用干净的隔离 target 目录：`CARGO_TARGET_DIR=<某个空目录> CARGO_INCREMENTAL=0`。
- **submodule 内提交不触发 app 仓 husky**（F18），fmt/clippy 必须手工跑，见 §8。

---

## 10. 待用户裁定

| #      | 事项                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | 现状与建议 |
| ------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------- |
| **D1** | **V1 的 Publish workflow 由谁触发。** 这是硬阻塞：`v2.0.0-rc.2` release 不存在，`check.ts` 拉不到含控制面的 daemon（F7/F8），而 workflow 是 `workflow_dispatch` 且需要目标仓 Actions 写权限，实施代理无法代劳。在它落地前，bridge 换线只能用 §3.4 的本地构建回退跑 dev-loop，**不能出货**                                                                                                                                                                                                                                                                                                                                                                                                                                  | 需用户跑   |
| **D2** | **版本策略确认：`2.0.0-rc.2` 还是直接 `2.0.0`。** §3.2 已给出采纳 rc.2 的完整理由（L1/L2/L4 未验证、F16 DNS 未接线、app 侧 `/v2/core/*` 零真机验证）。若用户认为 v2 已经该定版，改跑 `versionType=release`，产出 `2.0.0` 并写入 `CHANGELOGS.md`                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | 建议 rc.2  |
| **D3** | **删掉 v1 CLI 子命令后，是否补 v2 CLI（E7 卡）。** 现状：`RpcCommand` 里**没有任何** `/v2/core/*` 子命令（`cmds/rpc/mod.rs:106-159` 实证）。E5/E6 执行后 CLI 将完全无法驱动控制面，daemon 的命令行调试面归零。建议补（E7 已写成可直接执行的完整卡）；若不补，需接受调试只能靠 app                                                                                                                                                                                                                                                                                                                                                                                                                                          | 建议补     |
| **D4** | **`POST /core/check` 保留还是删。** 本计划按修订 A2 的明文裁定（"独立 Check 命令保留但降格为咨询"）选择保留。反方论据也成立：app 用本地 `CoreInstance::check_config_`（`core.rs:502`）校验，v2 reconcile 事务内部也做 check（`v2.rs:46-50`），所以 `/core/check` 今天的唯一消费者是 CLI，是纯调试面。若用户裁定删除，删除范围与 E1 同构，外加 `MAX_CONCURRENT_CHECKS`（`manager_bridge.rs:46`）、`check_slots` semaphore、`a_third_concurrent_check_is_refused_before_it_can_spawn_a_core`（`:1719`）、`a_missing_core_binary_is_classified_on_the_check_path`（`:1761`）、`checking_an_unresolvable_config_answers_in_the_envelope`（`routing/tests.rs:398`）、`the_core_check_request_is_pinned`（`wire_golden.rs:844`） | 默认保留   |
| **D5** | **`Event::CoreStateChanged` 是否一并删。** 它的 doc 注释写着"Kept because the GUI still consumes it"（`nyanpasu_ipc/src/api/ws/events.rs:28-33`），但 grep 证明 **app 根本不订阅任何 ws 事件**（F15）——注释与事实矛盾。删掉它能去掉一个有损投影，但那是改 tagged enum 的线格式，且 L7 已把 ws 流的处置划给 PR-D。本计划**不含**这项，仅登记该矛盾供裁定                                                                                                                                                                                                                                                                                                                                                                    | 本轮不做   |
| **D6** | **`check.ts` 锁步守卫（G1 卡）的归属。** L10 这个陷阱能悄悄成立，是因为没有任何东西检查"版本号推导出的 tag"与"submodule 实际 HEAD"是否一致。可加一个廉价守卫：在 `getNyanpasuServiceInfo`（`check.ts:694-712`）里比较 `git -C backend/nyanpasu-runtime rev-parse v<version>^{commit}` 与 `HEAD`，不一致时告警（或按裁定 fail）。但这是 **app 仓改动**，与并行编写的 app-switch 计划范围重叠，需明确由谁承担                                                                                                                                                                                                                                                                                                                | 需裁定归属 |
| **D7** | **PR 拆分与推送授权。** 本计划产出的是 runtime 仓的两批提交（2a、2b）。是开成两个 PR、一个 PR 两组提交、还是直接推 main，以及是否授权 push，均未定                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | 需裁定     |
