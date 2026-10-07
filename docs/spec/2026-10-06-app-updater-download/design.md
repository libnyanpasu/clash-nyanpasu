# 应用 updater 测速修复与 bolt-load 下载回退

**日期：** 2026-10-06（2026-10-07 按源码审计修订）

**状态：** 提案；本 PR 只提交 spec，未实施。阶段 1 可独立实施；阶段 2 被 bolt-load 上游能力阻塞（§7.1）。

**调查基线：** `main@dc2a1e630f19c3a1a860bd195c16c048f4ee768a`；2026-10-07 在 `main@8ac8ba84c` 复核，`client/app_update` 与 `core/download` 源码未变。依赖按 `Cargo.lock` 锁定版本阅读：bolt-load 0.1.0（crates.io 唯一版本）、tauri-plugin-updater 2.13.1（当前最新 2.x）。

**用户报告：** 关于页面的应用更新下载速度突然很大。

**权威顺序：** 当前 [AGENTS.md](../../../AGENTS.md) 与 [development guides](../../development/README.md) > 本 spec > 后续实施计划。

## 1. 决策与范围

1. 先修应用更新的测速，再接入 bolt-load；测速正确性不依赖更换下载引擎。
2. 每个下载源在 bolt-load 能并发下载时先尝试 bolt-load；bolt 因取消、本地存储错误以外的任何原因失败后，在**同一 URL** 调用一次现有 Tauri `Update::download()` 单流下载。两者都失败才按现有顺序尝试下一个源。
3. 所有成功下载的应用包都经过等价于当前 Tauri updater 的签名与签名版本校验，再进入 `Ready`。保留现有 Tauri 安装器。
4. 显式取消、关闭、磁盘错误与程序不变量失败不触发网络回退。取消完成必须等待下载工作结束及临时文件清理。
5. 保留现有 Unified RPC 操作、`AppUpdateSnapshot` 字段、下载源顺序和自动下载策略。内部进度消息可调整；不新增用户设置或前端下载入口。
6. 测速与下载尝试属于现有 operation 的生命周期；`AppUpdateActor` 继续串行拥有应用更新状态。发往父 actor 的进度只从 operation 任务单点发出，不引入尝试 ID、样本序号或额外采样任务。
7. bolt-load 自身的缺陷（失败不终止、无可等待的终止）在 bolt-load 上游修复，不在应用层用截止计时器或看门狗兜底。网络与安装通过适配器注入，不增加全局服务或业务层 Tauri 依赖。

包含：应用 updater 速度、进度、bolt-load 下载、单流回退、验签、取消清理、相关测试与实测。

不包含：内核 updater 行为变更、跨启动断点续传、跨源拼接、下载器设置界面、安装器重写、整个 updater 的 core 迁移。

## 2. 已确认事实与证据

### 2.1 应用更新

| 事实                                                                                                      | 证据                                                                                                                             |
| --------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| 应用更新调用 Tauri `Update::download()`，将每个 chunk 长度累加成 `downloaded`                             | [app_update/adapters.rs](../../../backend/tauri/src/client/app_update/adapters.rs)，`TauriAppUpdateBackend::download`            |
| 适配器每次尝试发两次 `Source`：开始时 `content_length: None`，首个 chunk 时带长度；`Chunk` 本身也携带总长 | 同文件                                                                                                                           |
| operation callback 在新增不足 256 KiB **且**间隔不足 250ms 时才丢弃进度；新增达到阈值便可立即发送         | [app_update/mod.rs](../../../backend/tauri/src/client/app_update/mod.rs)，`OperationActor::handle`                               |
| 速度用 actor **处理消息时**的 `Instant::now()` 计算，分母下限为 1ms                                       | 同文件，`Message::Progress`                                                                                                      |
| 每条进度消息都发布完整 snapshot（含 release 正文）；高速下载时字节阈值使发布频率随吞吐线性增长            | 同文件，`update_snapshot`                                                                                                        |
| `Message::Source` 清零 snapshot 的进度与速度，却不重置 `last_progress`                                    | 同文件，`Message::Source`                                                                                                        |
| 只有收到 chunk 才重新计算下载速度，没有无数据时的周期更新                                                 | 同文件，`Message::Progress`                                                                                                      |
| UI 直接把 `snapshot.speed` 格式化为 IEC 字节单位并添加 `/s`                                               | [update-controls.tsx](<../../../frontend/nyanpasu/src/pages/(main)/main/settings/about/_modules/update-controls.tsx>)            |
| Tauri `download()` 执行验签；`install()` 不验证调用方传入字节；验签函数及 `Update` 的公钥、策略均为私有   | [Tauri updater 2.13.1 源码](https://github.com/tauri-apps/plugins-workspace/blob/updater-v2.13.1/plugins/updater/src/updater.rs) |
| 应用未设置 `requireSignedVersion`（默认 `false`）；公钥位于 `plugins.updater.pubkey`                      | [tauri.conf.json](../../../backend/tauri/tauri.conf.json)                                                                        |
| 工作区 reqwest 0.13 未启用 gzip/brotli/deflate/zstd，不发送压缩协商也不透明解压                           | [backend/Cargo.toml](../../../backend/Cargo.toml)；`cargo tree -e features -i reqwest@0.13.5`                                    |

### 2.2 bolt-load 0.1.0 与内核下载

内核更新已经使用 bolt-load（[core/download/mod.rs](../../../backend/tauri/src/core/download/mod.rs)、[core/updater/instance.rs](../../../backend/tauri/src/core/updater/instance.rs)），它不是应用更新的当前下载路径。bolt-load 支持单流和并发 Range 下载、自定义 adapter、速度采样与并发降级，见[上游说明](https://github.com/greenhat616/bolt-load)。下表依据 crates.io 的 0.1.0 源码，路径相对各 crate 的 `src/`。

| 事实                                                                                                                                                                          | 证据                                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `TaskBuilder::build()` 先调用 `retrieve_meta()` 再检测 Range；强制 `Singleton` 只跳过 Range 检测，仍读元数据；该请求不受取消 token 约束；运行时还会再读一次                   | `task/builder.rs` `build`；`singleton_task.rs`、`concurrent_task/mod.rs` 的运行入口                                                                      |
| 应用 adapter 的元数据和 Range 探测各发 HEAD，一次下载最多 3 次 HEAD；Range 响应只调用 `error_for_status()`，不检查 `206` / `Content-Range`                                    | [core/download/adapter.rs](../../../backend/tauri/src/core/download/adapter.rs)                                                                          |
| 并发模式中失败分片退回待下载池并在之后重建；策略动作没有「中止」，下载循环只在成功或写入错误时退出；不可重试错误把并发上限降到活跃 runner 数（可为 0），30 秒后才恢复         | `strategy/mod.rs` `StrategyAction`；`runner_manager.rs` 失败分支；`concurrency_control_strategy.rs` `handle_immediate_degradation`                       |
| 因此服务端持续失败时并发下载**不返回错误**；5 秒慢流计时只记日志                                                                                                              | 同上；`runner/mod.rs`                                                                                                                                    |
| 取消直接丢弃下载 future；runner、连接与写入是丢弃句柄的 spawn；`Task::stop()` / `wait()` 只等状态机与事件循环；`TaskRunnerGuard::drop` 只发取消信号（源码 TODO 承认无法等待） | `concurrent_task/mod.rs` 取消分支；`runtime/tokio_impl.rs` `spawn`；`task/mod.rs` `stop` / `wait`；`runner/guard.rs`                                     |
| bolt 不校验 `206` / `Content-Range`；Range 请求得到 `200` 全量响应时按区间长度截断写入并报告成功；没有 ETag / `If-Range`                                                      | `runner/mod.rs` 写入分支                                                                                                                                 |
| 并发模式写入 `<name>.downloading`，成功后改名；失败或取消时不删除。单流模式直接写目标路径且不截断                                                                             | `concurrent_task/mod.rs` `create_file_writer`；`singleton_task.rs`                                                                                       |
| 进度回调由单一事件循环串行调用，但每个进度 tick 从独立 spawn 发出，可能乱序；原生速度为 B/s，250ms 采样加 EMA（α = 0.33）                                                     | `task/mod.rs` 事件循环；`singleton_task.rs`、`concurrent_task/mod.rs` 进度发送；`task/instance/sampler.rs`                                               |
| 内核下载 client 的 120 秒 `timeout` 是包含 body 的整体截止；`OwnedDownload::drop()` 只发取消信号；`start()` 在等待期间持有任务锁                                              | [core/updater/instance.rs](../../../backend/tauri/src/core/updater/instance.rs)；[core/download/mod.rs](../../../backend/tauri/src/core/download/mod.rs) |

结论：0.1.0 不满足 §6.2 的可等待终止，也不能保证失败会返回，阶段 2 不能在 0.1.0 上实施。应用也不能直接复用内核的 client 与 `OwnedDownload`。

## 3. 速度失真的机制

### 3.1 突然变大

当前计算式：

```text
speed = downloaded.saturating_sub(last_downloaded)
        / max(actor_now - last_actor_now, 0.001s)
```

该时间间隔描述消息消费速度，不能可靠描述网络传输时间。网络缓冲区中的数据被突发读出，或 actor 消息积压后集中处理，都会得到很短的分母。字节阈值允许这些进度在 250ms 内连续入队，进一步暴露尖峰。

已按当前公式构造确定性回放：

| 累计下载 | 回调采样时间 | actor 处理时间 | 当前公式结果   |
| -------- | ------------ | -------------- | -------------- |
| 256 KiB  | 250ms        | 900ms          | 约 0.278 MiB/s |
| 512 KiB  | 500ms        | 900.2ms        | 250 MiB/s      |
| 768 KiB  | 750ms        | 900.4ms        | 250 MiB/s      |

回调输入对应 1 MiB/s，处理结果出现 250 MiB/s。1ms 下限仅避免除零，不能修正时间来源；更大字节增量仍会产生更大的读数。

缓冲突发时回调时间本身也很接近，仅把时间戳换成生产侧时间不能消除尖峰；必须按固定时间窗口统计字节。

这是源码与公式回放确认的可触发机制。尚未采集用户现场的回调及 actor 时间序列，不能断言当次尖峰一定来自 mailbox 积压而非缓冲突发。

### 3.2 切源与停滞

- 切源后首次计算仍减旧源累计字节。例如旧源已下载 10 MiB，新源首个样本为 256 KiB，差值被截成零；时间基准也跨越了失败与重试。
- 停止接收数据后没有新的 chunk，snapshot 会保留上一速度。下载完成归零不解决下载中停滞的显示。
- 现有 `fallback_source_after_verification_restarts_download_progress` 测试只覆盖进度清零，不覆盖速度基准或消息处理延迟。

## 4. 测速与进度契约

### 4.1 采样位置

速度的语义为**当前尝试的有效包字节吞吐率，单位 B/s**。它不是网卡总流量，不包含代理协议开销，也不累加已经丢弃的其他尝试。

- 采样在 `OperationActor` 的下载处理中完成。它位于 client 层，不依赖 Tauri，所有下载引擎共享同一语义。后端适配器只报告「尝试开始」「累计有效字节与总长」「验证中」，不计算速度；bolt 原生速度不使用。
- `OperationActor` 在同一任务内用 `tokio::select!` 同时推进 `backend.download(...)` future 与 250ms `tokio::time::interval`。不单独启动采样任务；下载 future 结束时采样随之结束，无需另行等待。
- 适配器回调只更新一个 `Mutex<ProgressSampler>`，锁只保护该采样器，不含 actor 状态。每个 tick 计算一次速度，并向父 actor 发送一条携带 `downloaded / total / speed` 的进度消息。
- 删除 256 KiB 字节触发与现有节流锁：父 actor 每秒最多收到 4 条进度消息，外加尝试开始与验证中事件。父 actor 原样写入 snapshot，删除 `last_progress`，不用接收时间二次计算。

### 4.2 速度算法

`ProgressSampler` 是纯服务，接收累计字节和显式单调时间，不自行访问时钟。

- 速度取最近 1 秒的滑动窗口：保留窗口内的 tick 样本 `(time, downloaded)`，`speed = (downloaded_now - downloaded_oldest) / (now - time_oldest)`。尝试开始时写入 `(start, 0)` 作为首个样本，因此首个 tick 给出该 250ms 的实际速率，之前速度为零。
- 连续 1 秒无新增字节时，窗口两端字节相同，速度自然为零；恢复后自然回升。不需要额外的停滞规则、EMA 重置或首窗口特例。
- 同一尝试内累计字节取已记录值与新值的较大者。这吸收 bolt 进度 tick 的乱序（§2.2），不需要样本序号。
- 时间间隔使用实际单调时间，不假设每次 tick 恰好为 250ms；零间隔返回零。
- 小于一个采样窗口就结束的下载可以全程显示零速；结束事件更新准确字节数并归零，不为最后少量字节制造极短窗口的尖峰。

### 4.3 尝试与重置

把 `AppUpdateDownloadProgress::Source { source, content_length }` 改为每次尝试恰好发送一次的 `Attempt { source }`：切源与同源换引擎都发送，总长只随累计字节传递。适配器删除首个 chunk 处的第二次 `Source`。

每次尝试开始时：

1. operation 在同一回调中重置采样器（累计字节、窗口、total），再通知父 actor。
2. snapshot 回到 `Downloading`，`downloaded = 0`、`speed = 0`、`total = None`，记录当前源并保留 release。

不需要尝试标识。所有发往父 actor 的消息都出自 operation 任务，mailbox 保序；适配器在等待上一引擎终止（§6.2）后才发送回退的 `Attempt`，旧尝试不会再写入采样器。父 actor 继续沿用现有 operation id 过滤。

单流回退从头下载，进度允许显式归零；不可把 bolt 的已下载字节加到默认流进度。未知总长度维持 `None`，不伪造百分比。成功、失败、取消以及 `Verifying` 阶段速度都为零。

## 5. 下载与回退流程

复用现有 `download_first` 的来源顺序，在每个来源内部最多执行一次 bolt 尝试和一次默认尝试：

```text
当前来源 / URL
  ├─ bolt 不能并发（元数据失败、无长度、无 Range）→ 直接默认下载
  ├─ bolt 并发 → 验签成功 → VerifiedAppUpdate → Ready
  └─ bolt 失败（取消、本地存储错误除外）
       → 等待 bolt 终止 → 删除本次临时目录 → Attempt
       → 同一 Update::download() → 验签成功 → Ready
       └─ 失败 → 下一个来源

取消 / 关闭 / 本地存储失败 → 结束及清理 → Cancelled 或 Failed
```

默认路径仍是现有 `Update::download()`，不是强制 bolt `Singleton`：0.1.0 的单流模式不重试、不截断目标文件，且构建时仍读元数据，HEAD 被拒绝时无法替代普通 GET。

| 失败类别                                          | 行为                                     |
| ------------------------------------------------- | ---------------------------------------- |
| bolt 无法并发                                     | 不运行 bolt，直接同源默认下载            |
| bolt 网络、Range 校验、上游失败上限或产物验签失败 | 等待终止并清理后，同源默认下载一次       |
| 默认下载网络失败或验签失败                        | 尝试下一个来源，保留失败原因             |
| 显式取消、设置变更引发取消、应用关闭              | 不重试、不换源；等待清理，遵循现有状态机 |
| 创建临时目录、写盘、读取产物或清理失败            | 报告本地错误，不用重复网络下载掩盖问题   |
| panic / 程序不变量失败                            | 遵循项目 panic 策略，不包装成普通错误    |

bolt 的并发写入与改名错误在 0.1.0 中只剩字符串（`TaskError::Other`），无法可靠区分网络与本地错误。本地存储错误由应用在 bolt 之外的步骤识别（建目录、读产物、删目录）；bolt 内部返回的其他错误一律按可回退处理。

成功回退仅记录结构化诊断日志，不把中间错误设置成最终 `snapshot.error`。所有来源失败时，最终错误应标明源、bolt/默认阶段及原因；不记录带凭据的完整 URL 或代理地址。

## 6. 网络与文件边界

### 6.1 HTTP

复用现有 `NyanpasuReqwestAdapter` 的注入客户端模式，但不复用内核的 client：它的 120 秒整体超时会截断大文件下载。Range 校验也不能照搬当前实现。

- `Accept-Ranges` 仅是提示；并发路径要求长度可确定，并验证实际 Range 响应。
- 每个 Range 响应必须为 `206`，`Content-Range` 起止与请求一致，资源总长度与元数据一致，流字节数与该区间一致。bolt 不做这些检查，`200` 会被截断后当作区间数据，因此校验必须在 adapter 中完成。
- 保持 reqwest 不启用压缩特性，并显式发送 `Accept-Encoding: identity`，使 Range 偏移与保存字节含义一致。
- 下载探测与分片沿用 updater 的代理优先级、`no_proxy`、请求头、重定向及 TLS 配置语义。默认路径继续使用准备好的同一 `Update`。
- 保留现有 15 秒连接超时、60 秒停滞读取超时；不套用 manifest check 的 30 秒整包超时。并发重试与无有效进展的终止由 bolt-load 提供（§7.1），应用不另设计时器。
- 不使用 ETag / `If-Range`。本设计不跨启动续传，资源在下载中变化导致的拼接产物会被签名校验拒绝并回退默认下载；签名是进入 `Ready` 的必要条件。

必要的 adapter 改动若影响内核下载，必须验证其现有测试；不顺带改变内核更新的回退策略。

### 6.2 临时文件与取消

bolt 将数据写入本 operation 的独立临时目录，文件名由调用方指定，不采用远端文件名作为保存路径。每次尝试使用独立目录，不跨源或跨引擎复用 `.downloading` 文件。bolt 不清理失败产物，所以清理由应用删除整个目录完成。

新路径必须在 bolt 的全部工作（元数据请求、连接、runner、writer、事件循环）结束后，才删除目录、启动回退或报告取消完成。这要求 bolt-load 提供可等待的终止（§7.1）；`OwnedDownload::drop()` 与 0.1.0 的 `Task::stop()` 都不满足。

owner cancellation 与「单次尝试失败后的局部停止」使用不同的子 token。局部停止不能取消整个 operation；owner token 已取消时不能启动默认回退。RPC waiter 或页面退出不拥有这个 token，不取消 owner 已启动的下载。

初期保留 `VerifiedAppUpdate.bytes: Arc<Vec<u8>>`：bolt 文件下载完成后读取并验签，继续交给原安装器。不在本次引入安装器的文件路径 API 或持久下载缓存。

## 7. 上游前置条件与实施门槛

### 7.1 bolt-load

阶段 2 需要一个满足以下条件的 bolt-load 发布版本。它们属于下载引擎的边界，在 bolt-load 中修复，不在应用层兜底：

1. **可等待的终止：** 取消或失败后，存在可 await 的接口，返回时全部 runner、连接、文件写入与事件循环任务均已结束，而不是被分离。
2. **失败会返回：** 并发重试有上限，或持续无有效进展超过网络截止后以错误结束。
3. **元数据请求可取消：** `build()` 阶段的元数据请求受取消 token 约束。

升级 bolt-load 会同时改变内核下载的行为，必须运行内核下载与内核更新的现有测试。

### 7.2 验签

Tauri updater 2.13.1 的 `verify_signature` 为私有函数，`Update` 的公钥和 `require_signed_version` 也不可读。单独调用 `install(bytes)` 不会复用 `download()` 中的验签，不能作为自定义下载路径的安全保证。

优先向上游提供并使用 `Update::verify_downloaded_bytes` 一类的公开能力，复用 `Update` 的版本、签名、公钥与 `require_signed_version` 策略。只有该 API 在可用依赖版本中落地后才能采用；本 spec 不假设它已经存在。

若该能力在实施时不可用，允许在 GUI 下载适配器边界实现范围明确的验证 helper；它必须：

1. 复用 Tauri 同类的 minisign 验证库及 base64 编码规则，从应用配置 `plugins.updater` 读取公钥与 `requireSignedVersion`，与插件使用同一份配置。
2. 验证数据签名及全局签名后才读取 trusted comment。
3. 复现签名版本匹配、缺失版本字段、`require_signed_version` 以及 semver 等价处理，不能仅验证包内容签名。
4. 通过与锁定 Tauri 版本的签名样本行为对照测试，覆盖签名/内容/版本篡改、缺失签名版本以及错误公钥。
5. 明确记录对齐的 Tauri 版本；更新 updater 依赖时复核验证语义。

校验 helper 是基础设施适配器，不进入 `NyanpasuClient` 或 actor。此处存在实际的上游私有 API 边界，避免另外维护验证逻辑是优先方案，但不能通过跳过验证来避免重复。

§7.1 或 §7.2 任一未满足，阶段 2 不启用；阶段 1 的测速修复独立交付。

## 8. 分层与接口影响

- `NyanpasuClient` 与 typed client 保持现有领域 API。
- `AppUpdateActor` 拥有 snapshot、当前 operation 身份及状态转换；不获取网络客户端或 Tauri `Update`。
- `OperationActor` 使用注入的 `AppUpdateBackend` 完成本次下载，持有采样器与 tick，向父 actor 发送带 operation 身份的进度事件。
- `ProgressSampler` 为纯服务。
- `TauriAppUpdateBackend` 保留 manifest check、来源选择和平台安装边界，组合 bolt 与默认下载尝试。
- 下载适配器持有 reqwest、临时目录及 bolt 任务，只报告尝试与累计字节，不做采样。
- 任何为了 core/GUI 分离而移动的模块都更新调用方，不留下 Tauri crate re-export shim；本 spec 不要求先完成整个分离。

外部 snapshot 继续提供 `downloaded / total / speed / source / phase`，无需暴露引擎名或线程数。内部新增事件不自动成为公共 RPC 事件。

## 9. 验证与验收

### 9.1 确定性测速及 actor 测试

纯采样器使用显式时间；actor 使用 fake backend、事件确认与 Tokio 暂停时钟（`start_paused`）推进 tick，不使用全局 fixture 或真实睡眠。

| 场景                                     | 验收                                            |
| ---------------------------------------- | ----------------------------------------------- |
| 每 250ms 新增 256 KiB                    | 速度稳定为 1 MiB/s                              |
| 250ms 内突发 1 MiB 后停滞                | 读数不超过窗口平均值，1 秒后归零                |
| 父 actor 延迟处理进度消息                | snapshot 速度等于生产侧计算值，不随处理时间变化 |
| 首个 tick 前、零间隔、累计字节回退       | 不出现零除、NaN、无限值或负速度；回退值被忽略   |
| 切源、同源 bolt → 默认回退               | 窗口与累计字节清零，新样本不减旧尝试字节        |
| 无数据满 1 秒                            | 速度归零；恢复后回升                            |
| 未知长度、短下载、失败、验证、完成、取消 | 字节数准确，阶段与零速语义符合 §4               |
| 高速下载                                 | 每秒进度消息不超过 4 条，与吞吐无关             |

### 9.2 HTTP、验签与生命周期测试

本地 HTTP fixtures 至少覆盖：正常 Range 成功、HEAD 405 而 GET 成功、未知长度、忽略 Range 返回 200、错误 `Content-Range`、短流/超长流、416、分片持续失败后默认成功、两个引擎失败后换源、超时及所有来源失败。

使用明确的请求记录验证尝试顺序和次数。签名测试覆盖 §7.2 的语义以及 bolt 验签失败后默认成功、默认验签失败后换源。任何失败产物都不能进入 `Ready` 或安装器。

在 HEAD、连接、写盘、回退交界和 body 停滞处取消，使用 barrier/ack 确认：取消后没有新请求、任务不再写文件、临时目录已清理。覆盖关闭、修改设置和页面/RPC waiter 消失；后者不取消下载。

### 9.3 检查与实测

- 实施按 [testing guide](../../development/testing.md) 运行相关后端测试、Rustfmt、Clippy 和 architecture gate；公共 DTO 若发生必要变更，重新生成绑定并验证两种传输。
- 复用当前应用更新 UI 检查速度、源、进度重置及错误显示，不修改前端算法来掩盖后端错误。
- Windows、macOS、Linux 验证正常安装与取消清理。测试不得对真实用户配置或真实安装执行破坏性操作。
- 对现有 Nyanpasu、SourceForge、Ghfast、GitHub 候选源测量 HEAD/Range 能力、默认/bolt 耗时、首次有效数据时间、吞吐及回退次数，区分代理开启/关闭。能力可能随重定向和服务端变化，不能硬编码「某镜像永远支持并发」。
- 并发只在服务端和链路允许时可能提速；验收要求可靠回退，不预先承诺固定加速倍数。

## 10. 交付顺序

1. **测速修复：** 独立实现 operation 内的窗口采样、`Attempt` 重置、删除字节触发及确定性回归测试；保留默认下载。
2. **上游前置：** 在 bolt-load 中实现 §7.1 的三项能力并发布；确定 §7.2 的验签路径。任一未完成则继续默认下载。
3. **bolt 下载与回退：** 在应用下载适配器接入 Range 校验与同源回退，完成 HTTP、签名、生命周期测试与多平台验证。

本 spec PR 只新增本文件。CI 按用户要求通过提交消息中的 `[skip ci]` 跳过；文档格式、链接和 diff 在本地检查。
