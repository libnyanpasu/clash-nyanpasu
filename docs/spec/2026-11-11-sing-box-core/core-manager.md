# core-manager 与进程管理

[sing-box 内核接入设计](design.md) 的子设计。覆盖 runtime 子模块（`libnyanpasu/nyanpasu-runtime`）里的改动，并回答需求 ②：能否用信号重载 sing-box。

路径缩写：`M/` = `backend/nyanpasu-runtime/crates/nyanpasu-core-manager/src/`，`R/` = `backend/nyanpasu-runtime/crates/`，`SB/` = sing-box `v1.14.4`，`BL/` = `keiko233/blossom` 的 `server-agent/`。

这些改动在 runtime 仓提交与发版，主仓随后更新 pin。它们不依赖主仓的任何改动，可以最先开始。

---

## 1. 范围

| 在范围                                   | 不在范围                                                   |
| ---------------------------------------- | ---------------------------------------------------------- |
| 内核种类、启动与校验参数、版本探测       | 应用侧的配置生成（[`config-family.md`](config-family.md)） |
| 配置的读入、控制器解析、输出格式、文件名 | 应用侧的控制面驱动（[`core-api.md`](core-api.md)）         |
| 健康探测                                 | TUN、透明代理、DNS 接管（S7）                              |
| 配置变更分类与重载策略                   | sing-box 的优雅切换（重叠 epoch）                          |
| 控制台日志解析、native store             |                                                            |
| `sing-box-api` 协议 crate                |                                                            |
| 服务侧桥接与线格式的增量                 |                                                            |
| fake-core 的 sing-box 参数形状           |                                                            |

---

## 2. 内核种类

现状：`M/kind.rs:13` 把 `nyanpasu_core_metadata::ClashCoreKind` 重新导出为 `CoreKind`，所以 core-manager 里的“内核种类”只有四个 Clash 系取值。`nyanpasu-core-metadata` 另有一个两层枚举 `CoreKind { Clash(ClashCoreKind), SingBox }`（`R/nyanpasu-core-metadata/src/kind.rs:6-11`），目前没有使用者。

决策：core-manager 改用 metadata 的两层 `CoreKind`。

- Clash 系的分支写成 `CoreKind::Clash(ClashCoreKind::Mihomo)` 这样的形状；只关心家族的地方匹配外层。
- `M/` 里现有约 140 处 `CoreKind::` 引用里，日志解析与 native store 占多数（`M/log.rs`、`M/native_store/fs.rs`），它们本来就按种类分支，加一个 `SingBox` 分支即可。
- `CoreDistribution.kind` 的类型从 `ClashCoreKind` 改为 `CoreKind`（`R/nyanpasu-core-metadata/src/dist.rs`）。两层枚举是 `#[serde(untagged)]`，`"mihomo"` 等旧取值照常解码；`SingBox` 的线上取值在这次改动里定为 `"sing-box"` 并补上 golden 测试。
- `FeatureSupport` 为 `CoreKind` 实现：Clash 分支委托给现有实现；`SingBox` 对四个 `Feature` 全部返回 `Support::No`，依据是 API 服务与 Clash API 都只监听 TCP（主设计 §1.3）。

`LocalIpcPolicy` 的行为随之确定：`Prefer` 回落到 TCP，`Force` 在准备阶段报 `RequiredLocalIpcUnsupported`，与 Clash Premium、Meow 相同，不需要新代码路径。

---

## 3. 启动、校验、版本

| 动作       | Clash 系（现状）                           | sing-box                     | 位置                    |
| ---------- | ------------------------------------------ | ---------------------------- | ----------------------- |
| 启动       | `-d <dir> -f <config>`（Mihomo 另加 `-m`） | `run -c <config> -D <dir>`   | `M/kind.rs:38-50`       |
| 校验       | `-t -d <dir> -f <config>`                  | `check -c <config> -D <dir>` | `M/kind.rs:69-80`       |
| 版本       | `-v`                                       | `version`                    | `M/capability.rs:95-99` |
| 环境变量   | `SAFE_PATHS`、`CLICOLOR_FORCE=0`           | 都不设；颜色见 §7            | `M/kind.rs:147-152`     |
| 控制器参数 | clash-rs 需要 `--controller-ipc`           | 无                           | `M/kind.rs:57-67`       |

- `check_args` 现在与种类无关，改为接收种类。
- 校验的超时沿用 `CHECK_CONFIG_TIMEOUT`（30 秒）。
- 校验失败时的输出摘要走现有的 `summarize_output(kind, …)`，为 sing-box 增加分支：取 stderr，去 ANSI，截断。`BL/src/process.rs:163-176` 的做法可以直接参照。
- `CoreVersion::parse` 已经能从整行横幅里取第一个可识别的版本记号（`R/nyanpasu-core-metadata/src/feature.rs`），`sing-box version 1.14.4` 不需要特判。假设 V1 验证输出形状。

---

## 4. 配置

### 4.1 读入与输出

- 读入不改：`ConfigSnapshot::from_bytes` 用 YAML 解析器读成 `Mapping`（`M/config/mod.rs:72-86`），标准 JSON 是合法 YAML。语义哈希、规范化、差异比较都工作在 `Mapping` 上，可以复用。
- 输出按种类：Clash 系序列化为 YAML（现状），sing-box 序列化为 JSON。sing-box 只读 JSON。
- 文件名按种类取扩展名。现在写死 `.yaml` 的位置：`M/config/runtime_store.rs:144,393`、`M/control/executor.rs:359`、`M/control/mod.rs:565`。运行期目录的发现逻辑同时认两种扩展名，避免升级后把旧的 `.yaml` 当成孤儿清掉。

### 4.2 控制器解析

Clash 系从顶层 `external-controller*` 与 `secret` 解析控制器（`M/config/clash.rs:40-62`）。sing-box 新增同级模块 `M/config/sing_box.rs`：

- 在 `services` 数组里找 `type == "api"` 的元素，取 `listen`、`listen_port`、`secret`。
- 找不到、或 `listen_port` 缺失：`Error::InvalidConfig`，说明 sing-box 配置必须带 API 服务。应用侧的受管设定保证它存在（[`config-family.md`](config-family.md) §4.3）；独立使用 core-manager 的调用方需要自己写。
- 有多个 `type == "api"` 的服务：取第一个。应用侧已经移除多余的。
- 不改写配置：没有受管的本地 IPC 端点可换，`rewrite_managed_controller` 不适用。

### 4.3 控制器类型

`ResolvedController { host: clash_api::Host, secret }`（`M/spec.rs:72-76`）改为带协议：

```rust
pub struct ResolvedController {
    pub endpoint: ControllerEndpoint,
    pub secret: Option<String>,
}

pub enum ControllerEndpoint {
    /// Clash REST / WebSocket。
    Clash(clash_api::Host),
    /// sing-box API 服务：回环 TCP 上的 gRPC。
    SingBoxGrpc(std::net::SocketAddr),
}
```

对外发布的状态里 `controller: Option<clash_api::Host>`（`M/state.rs:193`）与线格式 `CoreControllerInfo`（Http、UnixSocket、NamedPipe）随之扩展：线格式的连接信息增加 `protocol` 字段，取值 `clash-rest`、`sing-box-grpc`，缺省为 `clash-rest`。sing-box 的端点用现有的 `Http` 形状承载地址，由 `protocol` 说明怎么连。这样旧的应用读到新服务发的 Clash 连接信息不受影响。

### 4.4 不适用于 sing-box 的部分

| 机制                                    | 位置                           | 处理                              |
| --------------------------------------- | ------------------------------ | --------------------------------- |
| 引导配置与入站清零（让新旧 epoch 重叠） | `M/config/mihomo.rs:21-27,130` | 不调用。sing-box 一律硬切换（§6） |
| 运行配置变更分类表与 `PATCH` 字段表     | `M/config/mihomo.rs:29-110`    | 不调用。sing-box 的分类见 §6.4    |
| `GET /configs` 读回校验                 | `M/manager/apply.rs:262-270`   | 不调用                            |
| 路径改写（把相对路径改到数据目录）      | `M/native_store/paths.rs`      | 首版对 sing-box 不改写，见 §8     |
| 受管本地 IPC 端点改写                   | `M/config/clash.rs:66-85`      | 不调用                            |

---

## 5. 健康探测

现有探测 `ControllerVersionProbe` 调 Clash 的 `/version`（`M/health/probe.rs:111-140`）。探测已经是可替换的（`HealthProbe` trait 与 `ProbeHandle`，`M/health/probe.rs:46-60`）。

sing-box 新增一个探测实现，按控制器端点的协议选择：

- 调 `grpc.health.v1.Health/Check`，服务名为 `daemon.StartedService`，`SERVING` 为健康。API 服务注册了标准健康服务（`SB/daemon/server.go:22-24`）。
- 带密钥。超时、阈值、间隔沿用 `HealthPolicy`。
- 就绪、存活、协调三个阶段都用它。

API 服务在 sing-box 的启动阶段末尾才开始监听（`SB/service/api/server.go:64` 的 `StartStateStarted`），所以它健康意味着入站与路由已经就绪，语义上不弱于 Clash 的 `/version`。

core-manager 因此依赖 `sing-box-api` crate。依赖方向在 runtime 仓内部，与它依赖 `clash-api` 相同。

---

## 6. 重载策略

### 6.1 上游行为

`sing-box run` 的主循环（`SB/cmd/sing-box/cmd_run.go:186-222`）：

```text
收到 SIGHUP
  └─ check()  用当前配置路径重新读取并校验
       ├─ 失败：记一条 "reload service" 错误日志，继续运行旧实例
       └─ 通过：
            关闭当前实例（全部入站、出站、服务，包括 API 服务）
            重新读取并合并配置
            create(options)
              ├─ 成功：新实例运行
              └─ 失败：run() 返回错误，进程退出
```

要点：

- 这是进程内的完整重建，不是增量应用。连接全部中断，监听端口先关后开，TUN 设备重建。
- 只在能投递 SIGHUP 的平台上可用。Windows 没有等价信号。
- 没有返回值。发信号的一方只知道信号送达。
- 三种结果里有一种是“静默保留旧配置”，有一种是“进程退出”。
- gRPC 的 `ManagedService.ReloadService` 在 API 服务上不可用：API 服务只注册 `StartedService`（`SB/daemon/server.go:22`，`SB/daemon/attached_service.go:10`）。

### 6.2 与重启的比较

| 方面         | SIGHUP 重载                                   | 重启进程                                     |
| ------------ | --------------------------------------------- | -------------------------------------------- |
| 已有连接     | 中断                                          | 中断                                         |
| 监听端口     | 先关后开，有空窗                              | 先关后开，有空窗                             |
| TUN 设备     | 重建                                          | 重建                                         |
| 控制面的流   | 中断，需要重新订阅                            | 中断，需要重新订阅                           |
| 进程与 PID   | 保留                                          | 更换                                         |
| 平台         | 仅 Unix                                       | 全平台                                       |
| 生效的证明   | 没有直接证明，需要另行观测                    | 新进程通过健康探测即证明它在跑提交的那份配置 |
| 失败时       | 可能保留旧配置而不报错，也可能直接退出        | 启动失败是明确的错误，回滚路径已有           |
| 需要的新代码 | 信号投递、生效验证、同 epoch 内吊销控制面句柄 | 无，走现有的 `Switch` 路径                   |
| 省下的时间   | 进程创建与二进制加载                          | —                                            |

SIGHUP 相对重启的实际收益只有最后一行。用户可感知的中断（连接、端口、TUN）两者相同。

### 6.3 先例

`blossom/server-agent` 在同一问题上选择了替换进程，注释说明了理由（`BL/src/main.rs:352-355`）：信号成功只证明送达，sing-box 可能拒绝新文件并继续用内存里的旧配置；启动新进程才能证明提升的配置就是正在运行的配置。它的应用流程是预检、提升、替换进程、观察期、提交，失败时回滚到上一份已知可用配置（`BL/src/main.rs:313-420`）。core-manager 的 epoch 模型（暂存、校验、提交、启动、就绪、失败回滚）已经是同一个形状。

### 6.4 决策：首版一律重启

sing-box 的配置变更分类只有两种结果：

| 差异                             | 分类     | 做法                                         |
| -------------------------------- | -------- | -------------------------------------------- |
| 源配置与有效配置的语义哈希都没变 | `Noop`   | 不动                                         |
| 其余任何差异                     | `Switch` | 硬切换：停止旧 epoch，确认死亡，启动新 epoch |

- 不产生 `Patch` 与 `Reload`。模式切换不经过配置文件（[`core-api.md`](core-api.md) §9.1），不在这张表里。
- 硬切换而不是优雅切换：新旧实例监听同样的端口，无法重叠；`graceful_degrade_reason`（`M/manager/switching.rs:31`）已经对非 Mihomo 返回降级原因，`SingBox` 归入同一分支。
- 回滚路径不变：新 epoch 起不来时用上一份已提交的配置重新启动旧 epoch。

应用侧因此要在界面上说明“应用设定会重启内核”。重启耗时在 S4 用真实二进制测量并记录，作为是否做 §6.5 的依据。

### 6.5 后续可选项：带验证的 SIGHUP（S7）

只在测量表明重启明显慢于进程内重建时才做。设计要点先记录在这里，避免到时重新推导。

适用范围：Unix；Windows 保持重启。

流程，对应 `ConfigChange::Reload`：

1. 暂存并用同一个二进制校验新配置（现有步骤）。校验失败则不继续。
2. 记下当前实例的 `GetStartedAt`。
3. 把新配置原子地写到当前 epoch 的运行配置路径，generation 加一（现有的 `apply` 路径已经复用同一路径，`M/manager/apply.rs:167`）。
4. 投递 SIGHUP。
5. 等待以下之一，以先发生者为准：
   - 进程退出：重载失败。按现有的崩溃恢复，用上一份已提交的配置重新启动。
   - 健康探测恢复且 `GetStartedAt` 大于第 2 步的值：新实例已建立，重载成功。API 服务随实例重建，它的启动时间在每次实例创建时重置（`SB/daemon/attached_service.go:19`）。
   - 到达期限而 `GetStartedAt` 未变：sing-box 在自己的 `check()` 上拒绝了（§6.1 的第一种结果）。恢复旧文件，改走 `Switch`。
6. 成功后吊销本 epoch 之前发出的控制面绑定，让应用重新取句柄。现在的 `ApiClient` 绑定到实例且不能重绑，Mihomo 的 `PUT /configs` 不会让流断开，所以这是 sing-box 重载特有的新要求。

这条路径需要 `RuntimeInstance` 增加一个重载入口，并让发布的状态带上“控制面代次”。两者都是 runtime 协议的增量，届时单独评审。

---

## 7. 控制台日志

`M/log.rs` 按种类解析控制台输出的头部，解析失败的行降级为未格式化帧而不是丢弃。新增 sing-box 的解析：

- 行形状（来自 `BL/src/process.rs:393-400` 的样例与上游日志格式）：可选时区与时间戳、级别、可选的 `[连接号 耗时]`、`组件/类型[tag]: 消息`。例：`+0800 19:49:27 INFO v2ray-api: grpc server started`。
- 颜色：sing-box 向管道输出时带 ANSI 转义。解析前去除；`M/log.rs` 已经为 Meow 处理 ANSI，复用同一个函数。是否另外通过命令行开关关闭颜色取决于假设 V11，去除逻辑无论如何都保留。
- 时间戳：应用侧的受管设定强制 `log.timestamp = true`（[`config-family.md`](config-family.md) §4.3）。时间戳缺失时帧的时间取接收时刻，与其他种类的降级一致。
- 级别映射：`TRACE`、`DEBUG`、`INFO`、`WARN`、`ERROR`、`FATAL`、`PANIC` 映射到现有 `LogLevel`，`FATAL` 与 `PANIC` 归入最高级别。
- 帧的上限（文本长度、字段数）沿用 `M/log.rs:20-27` 的常量。

控制台日志仍是归档与诊断的来源：它覆盖 API 服务启动之前与崩溃时的输出。应用侧实时日志页用 gRPC 的 `SubscribeLog`（[`core-api.md`](core-api.md) §4.1）。两者并存的方式与 Clash 系相同。

---

## 8. native store

native store 给每个内核种类一个独立的主目录，作为内核的工作目录（`M/native_store/mod.rs:31-33`）。

- sing-box 的主目录：`native-store/v1/sing-box`。
- sing-box 的相对路径（`experimental.cache_file.path` 的默认值、本地 rule-set 的 `path`、证书路径）相对工作目录解析，所以默认落在这个主目录里，不需要改写。
- 首版对 sing-box **不做路径改写**。后果：用户 Profile 里的相对路径相对 native store 主目录解析，而不是相对 Profile 文件所在目录。这在文档与导入提示里说明；需要引用本地文件的用户使用绝对路径。Clash 系的改写规则（`M/native_store/paths.rs`）是逐键维护的，为 sing-box 建同样的表要等有实际需求再做。
- `legacy_kind`（`M/native_store/mod.rs:72-93`）增加 `"sing-box"` 到 `SingBox` 的映射。
- 缓存损坏的识别（`cache_failure`，`M/native_store/mod.rs:50-68`）按种类匹配日志文本。sing-box 的匹配文本在 S3 用真实二进制制造损坏的 `cache.db` 后按实际日志填写，不凭推测写。在此之前 `SingBox` 分支返回 `false`，即不触发缓存隔离，启动失败按普通失败处理。
- 租约、脏标记、`confirm_stopped` 的语义不变。

---

## 9. 停止

- sing-box 处理 `os.Interrupt` 与 `SIGTERM`，收到后关闭实例并正常退出（`SB/cmd/sing-box/cmd_run.go:188,203-218`）。Unix 上现有的停止策略直接适用。
- Windows 上现有策略是通过控制台关闭辅助程序发事件，没有时退回强杀（`M/spec.rs` 的 `windows_shutdown_helper`）。sing-box 对控制台事件的反应是假设 V12。
- “证明死亡”的要求不变：停不下来就隔离，不当作已停止。

---

## 10. macOS DNS 接管

`DnsController` 按有效配置推导要接管的 DNS（`M/dns.rs`、`M/manager/dns_sync.rs`），推导逻辑读的是 Clash 的 `dns` 段。对 sing-box：推导结果恒为“不接管”，收敛步骤照常执行，效果是恢复任何遗留的接管记录。sing-box 的 DNS 接管随 TUN 在 S7 设计。

---

## 11. `sing-box-api` crate

接口与取舍见 [`core-api.md`](core-api.md) §8.1。与 core-manager 相关的点：

- core-manager 只用它的健康检查，以及 §6.5 落地后的 `GetStartedAt`。
- 它不依赖 core-manager，也不依赖 `clash-api`。
- 许可证：proto 取自 sing-box（GPL-3.0-or-later），runtime 仓是 GPL-3.0，兼容。文件头保留来源与上游提交。

---

## 12. 服务侧

| 位置                                                              | 改动                                                                                                |
| ----------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `R/nyanpasu-service-runtime/src/server/manager_bridge.rs:727-737` | `core_kind()` 把 `CoreType::SingBox` 映射到 `CoreKind::SingBox`，去掉 `bail!`                       |
| `R/nyanpasu-service-runtime/src/cmds/rpc/mod.rs:14`               | 把 `SingBox` 加入受支持集合，更新说明它“有意缺席”的注释                                             |
| `R/nyanpasu-utils/src/core/definition.rs:126-127,141`             | 可执行文件名改为 `sing-box`；加入 `get_supported_cores`                                             |
| `R/nyanpasu-utils/src/core/instance.rs:230`                       | 旧启动路径的 `unimplemented!`：确认该路径是否仍有调用方；没有则随清理移除，有则返回明确的不支持错误 |
| 服务状态                                                          | 增加受支持的内核种类列表，供应用在启动前检查（主设计 D12）。旧应用忽略这个字段                      |
| 线格式的连接信息                                                  | 增加 `protocol`（§4.3）                                                                             |
| Linux 透明代理捕获                                                | 维持只对 Mihomo 生效的门控；对 sing-box 报告不支持                                                  |

线格式的每个增量都加进 `R/../nyanpasu_ipc/tests/wire_golden.rs`，并验证两个方向的错位：新应用对旧服务、旧应用对新服务。

---

## 13. fake-core

`backend/fake-core` 是主仓里的测试替身，接受真实内核的参数形状（`backend/fake-core/README.md`）。增加：

- 参数形状：`run -c <config> -D <dir>`、`check -c <config> -D <dir>`、`version`。
- `version` 输出 `sing-box version <可配置>`。
- 不实现 gRPC。需要健康探测的生命周期测试通过注入自定义 `ProbeHandle` 完成；gRPC 本身由 `sing-box-api` 的假服务测试与真实二进制集成测试覆盖。

理由：fake-core 是只依赖标准库的二进制，为它加 HTTP/2 与 protobuf 会改变它的性质。

---

## 14. 测试

| 对象           | 方法                                                                                                                  |
| -------------- | --------------------------------------------------------------------------------------------------------------------- |
| 参数           | 表格化断言每个种类的启动、校验、版本参数，延续 `M/kind.rs` 现有测试的写法                                             |
| 控制器解析     | 纯值测试：标准配置、缺 API 服务、缺端口、多个 API 服务、`listen` 为 IPv6                                              |
| 输出格式       | sing-box 的准备结果是合法 JSON，往返后语义哈希不变；Clash 系的输出逐字节不变                                          |
| 运行期文件发现 | 目录里同时有 `.yaml` 与 `.json` 时两者都被识别                                                                        |
| 变更分类       | sing-box 只产生 `Noop` 与 `Switch`                                                                                    |
| 日志解析       | 样例行（带与不带时间戳、带 ANSI、带连接号、续行）到帧的表格化测试；未识别的行降级而不丢失                             |
| 生命周期       | 用 fake-core 的 sing-box 参数形状加注入探测：启动、就绪超时、应用即重启、启动失败回滚、停止                           |
| 线格式         | golden 测试覆盖新字段的缺省与往返                                                                                     |
| 真实二进制     | 标记为需要 `sing-box` 的集成测试：启动到健康、应用新配置、校验失败的错误映射、停止并确认死亡。默认不跑，CI 里按需开启 |

真实二进制测试需要的 sing-box 版本固定在测试配置里，与应用支持的世代一致（[`schema-compat.md`](schema-compat.md) §2）。
