# OpenRPC 实验性接入设计

本设计把 Tauri IPC 接成应用内部 JSON-RPC 的一个 transport。旧 Tauri commands、events、Specta bindings 和现有 hooks 继续可用。实验入口不启动 HTTP 服务，也不提供 Electron、LuCI 或 OpenWrt 入口。实现代码与本文档分开提交。

## 入口与调用链

[OpenRPC 契约](./openrpc.json)定义实验 API；`rpc.discover` 返回同一份文档。实现时启动流程比对文档与 Rust 注册的方法名。Rust RPC 模块位于仓库相对路径 `backend/tauri/src/rpc/mod.rs`，注册 `jsonrpsee` 方法并调用 `NyanpasuClient`。Tauri 的 `rpc_dispatch` 接收完整 JSON-RPC 请求并返回完整响应；`rpc_subscribe` 用 Tauri Channel 转发 JSON-RPC notification。二者限制单条消息为 1 MiB，且只接受单个带 `id` 的请求。

注册名由 Rust 操作路径生成，例如 `profiles::list` 生成 `nyanpasu.v1.profiles.list`。注册宏会检查该路径对应的函数存在；OpenRPC 的 schema、说明和错误仍需维护，启动时的方法列表比对负责发现名称漂移。

这意味着 Rust 操作函数的模块名和函数名也是 wire API 的一部分。重命名时必须同步评估版本兼容性；若只改代码而漏改 OpenRPC 文档，启动时的方法列表比对会失败。

前端生成脚本位于仓库相对路径 `frontend/interface/scripts/generate-openrpc.mjs`，从同一份 OpenRPC 文档生成 `frontend/interface/src/openrpc/generated.ts`，包括组件 DTO、方法名、参数/结果签名和 unary client。实现提交加入后，可运行 `pnpm -F interface generate:openrpc` 更新产物，`pnpm -F interface check:openrpc` 检查产物是否过期；interface 构建先运行生成器。生成器用 `json-schema-to-typescript` 处理 DTO 的 JSON Schema。`frontend/interface/src/openrpc/index.ts` 负责 Tauri invoke、Channel 订阅生命周期和 JSON-RPC envelope，提供 `createTauriApplicationRpc()`。实验方法如下：

| RPC 方法                                      | 应用操作                                                                    | 原有入口                |
| --------------------------------------------- | --------------------------------------------------------------------------- | ----------------------- |
| `nyanpasu.v1.profiles.list`                   | `NyanpasuClient::get_profiles` 的摘要视图                                   | `get_profiles`          |
| `nyanpasu.v1.profiles.activate`               | `NyanpasuClient::activate_profile`，保留 `MutationOutcome` 的 degraded 语义 | `activate_profile`      |
| `nyanpasu.v1.clash.snapshot`                  | `NyanpasuClient::clash_ws_snapshot`                                         | `get_clash_ws_snapshot` |
| `nyanpasu.v1.clash.subscribe` / `unsubscribe` | `NyanpasuClient::subscribe_clash_ws`                                        | `clash-ws-event`        |

订阅通知保留原 `ClashWsEvent` 的 `sequence` 和 `update`。客户端发现 sequence 跳跃时，应调用 `clash.snapshot()` 恢复；新封装只提供事件回调，不在 transport 层决定 UI 的恢复策略。取消订阅由 JSON-RPC unsubscribe 执行，Tauri Channel 发送失败时后端转发任务退出。

## 当前边界

- 方法和 DTO 的生成来源是 [OpenRPC 契约](./openrpc.json)；Tauri transport 与订阅生命周期是手写适配。尚未建立 Rust Serde DTO 对 JSON Schema 的完整校验流水线。
- OpenRPC 对 Clash update 和 snapshot 的嵌套数据只声明了外层结构；补全所有 variant 的 JSON Schema 后才能把这份文档视为完整的独立客户端生成契约。
- 本期没有切换业务 hooks 或移除旧 command/event。这个入口用于在真实 facade 上验证协议、IPC 和订阅生命周期。
- `NyanpasuClient` 当前仍由 Tauri composition root 创建。RPC handler 自身不依赖 Tauri 类型，但独立后端启动还需要后续拆分 composition root。
