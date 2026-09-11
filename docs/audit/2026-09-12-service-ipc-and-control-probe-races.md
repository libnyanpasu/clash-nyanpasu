# 服务 IPC 管道忙与控制通道探测竞态分析

日期：2026-09-12。本文记录排查证据和修复方案；管道等待重试与探测竞态修复尚未实施。

## 范围与基线

- 用户报告：main 使用服务模式正常，控制通道恢复 PR 启用服务模式后持续显示 API 不可用。
- 最初比较的本地 main：`705650033`。
- PR 分支：`fix/control-channel-recovery`，分析时 HEAD 为 `9bb516f7a`；新增控制通道逻辑来自 `e2cf802e9`。
- 更新远端后确认：`origin/main` 为 `41ad189c5`，已包含日志功能提交，但未包含上述控制通道恢复提交。
- 运行中的安装版程序未验证其构建提交，因此源码差异能证明风险路径，不能单独证明运行程序的具体错误码。

用户看到的错误：

```text
the running core's API is unavailable: backend_unavailable:
service endpoint unreachable: IPC request `/v2/core/api-connection` failed:
error sending request for url (http://localhost/v2/core/api-connection)
```

## 已确认的事实

1. Windows 服务 IPC 的实际地址是 `\\.\pipe\nyanpasu_ipc`。`http://localhost` 是 HTTP 协议占位 URL，不是核心 external-controller 地址。
2. 排查时服务及 Mihomo 进程均在运行。使用当前用户通过命名管道请求 `/v2/core/api-connection` 得到 `HTTP/1.1 200 OK`。这说明该时刻接口存在且当前用户能够连接，不代表应用的每次并发请求都成功。
3. 使用 8 个并发线程执行 40 次直接打开管道的诊断测试，12 次成功，28 次返回 Windows 错误 231（`ERROR_PIPE_BUSY`）。测试只打开并关闭连接，没有提交核心修改命令。它证明真实管道存在并发竞争窗口，但不等同于复现应用原始请求的完整错误链。
4. main 与 PR 均依赖 reqwest 0.13.4。其 Windows connector 调用一次 `ClientOptions::new().open(pipe)`，没有针对错误 231 的等待重试。两版 runtime 的 IPC 客户端连接实现没有相关变化。
5. 原始日志含控制通道重试汇总及流握手失败提示，但没有保留底层 OS 错误链。汇总的原因位于独立字段，不能替代消息正文中的完整失败诊断。服务日志目录在当前权限下不可读。

结论边界：管道忙是已复现的传输层问题，也是应用失败的候选原因；应用原始失败是否为 231，仍需在完整失败日志下复现确认。

## 问题一：管道忙

忙表示当前没有可立即连接的管道实例，不等同于服务退出、接口不存在或权限不足。连接器立即返回错误，会把短暂竞争直接暴露为请求失败。

### 修复方案

在 Windows 命名管道连接器的打开阶段处理 231：

```text
打开管道
  成功 -> 开始 HTTP 通信
  错误 231 -> 异步退避 -> 再次打开
  其他错误 / 超过连接期限 -> 返回错误
```

- 仅重试错误 231；建议初始退避间隔为 5、10、20、50 毫秒，之后保持上限。参数需通过并发测试验证。
- 建议设置 1 秒的连接总期限；外层取消应立即终止等待。连接期限不限制已建立连接的长轮询或流读取。
- 重试发生在发送 HTTP 请求之前，不能通过重放整个请求实现，以免重复执行修改操作。
- 最终失败应记录完整错误链、尝试次数及等待时间。
- 当前 reqwest 虽有 `connector_layer`，但内部连接请求类型 `Unnameable` 未实现 `Clone`，无法直接套用普通 Tower 请求重试层。
- 建议对 reqwest 的 Windows connector 提交最小补丁，通过固定的依赖版本或提交引入并向上游提交；不能依赖修改本机 Cargo 缓存。补丁应同时适用于服务 IPC 和核心 Named Pipe 连接。

### 验收

使用真实 Windows 命名管道和明确的同步信号控制竞争窗口：先忙后可用时成功；持续忙在期限内失败；非 231 不重试；取消及时生效；服务端修改请求只收到一次。保留 main 与 PR 在同一服务进程上的对照结果。

## 问题二：PR 新增探测及 lease 竞态

PR 中 `control_channel.rs` 的 `ControlMonitor` 激活后每 5 秒执行探测。每轮依次执行：

1. `endpoint.status()`：一次服务 IPC。
2. `endpoint.api_connection()`：一次服务 IPC。
3. 构造临时 `ApiClient` 并执行 `version()`。
4. `version()` 的前后实例校验：再执行两次服务 IPC，中间访问核心 `/version`。

因此一轮新增四次服务 IPC，其中三次访问 `/v2/core/api-connection`。这些请求会与原有状态轮询、lease 监视及流数据实例校验并发。前端每秒状态查询负责激活探测，并非每秒都运行完整探测；代码已有单个恢复探测在途限制。

更关键的是 `control_probe_completed()` 的失败分支无条件 `state.api.take()`。丢弃 `ApiLease` 会撤销所有共享客户端并断开现有流，触发重建及新一轮请求。

存在以下可由代码推导的竞态：

```text
旧探测启动
  -> 其他 API 调用建立或替换 lease
  -> 旧探测失败返回
  -> host generation 与 probe serial 仍匹配
  -> 新 lease 被无条件清除
```

成功结果也应进行 lease 代次校验，以防旧结果覆盖后来建立的连接。管道忙与这条竞态独立存在，任一种短暂探测失败都可能触发后者。

### 修复方案

- 已有有效 lease 时复用其现有监视，恢复探测仅在无可用 lease 或已断线时运行，减少重复查询。
- 由 `CoreActor` 持有 lease 代次，替换或清除时递增；探测启动记录该代次。
- 探测完成除校验 host generation 和 probe serial 外，还需校验 lease 代次。迟到的成功和失败都不能覆盖或撤销新 lease。
- 辅助探测失败记录降级信息；已有 lease 的有效性由其权威校验决定。保留实例切换、停止及 handoff 时的撤销保护。
- 保留单探测在途限制及明确的重试时序，不通过放宽实例校验或长期缓存凭据规避失败。

### 验收

使用注入的 endpoint 和同步信号，覆盖旧探测启动后新 lease 建立、再返回旧成功或旧失败的两种顺序；新连接均须保留。另验证真实断线恢复、停止、handoff 和实例替换，不通过任意 sleep 制造竞态。

## 失败日志审查与本次迁入范围

本次迁入 main 的修改只补齐失败日志，不实施上述两项运行行为修复：

- 服务 IPC 在转换为领域错误前记录 WARN 和完整 source 链，包括操作路径及底层 OS 错误；操作等待失败也经过该记录路径。
- WebSocket API 客户端获取、握手、读取失败和任务异常退出记录原因。
- API 生命周期订阅失败、超时、事件流错误或结束，以及绑定监视失败和超时记录原因。
- 正常取消、已退休实例和主动 abort 不提升为故障 WARN。已有畸形帧丢弃 WARN 保留。
- PR 专属 `control_channel.rs` 中的重试消息调整留在 PR，避免为迁入日志而引入探测逻辑。

回归测试应断言实际 JSON 日志的 `fields.message` 包含接口路径和 OS 错误码，不能只断言调用返回错误。两个运行行为问题应分别提交、分别测试，最后做组合验证。
