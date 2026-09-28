# 基于 inventory 和 Specta 的应用 API

**日期：** 2026-09-28
**状态：** 首批 profiles 与 Clash 调用链已实现；桌面实机与后续 WebUI 验证待完成

## 契约与注册

应用 API 使用项目自己的 `fn_name + params` 契约，不使用 JSON-RPC 或 OpenRPC。普通方法写成带 `#[rpc(name = "profiles.list")]` 的 typed async Rust 函数。过程宏生成 Serde 参数适配、结果序列化、Specta 类型登记函数和 `inventory::submit!` 注册项。业务函数通过显式注入的 `ApiContext` 调用 `NyanpasuClient`，不读取 Tauri 状态或全局服务。

启动时 `ApplicationApi` 一次性收集已链接的注册项，拒绝重复 `fn_name`，建立方法表。`inventory` 只负责发现；请求分发查询方法表。Clash 事件作为 stream 注册项，输入和输出类型同样进入 Specta 导出。方法、事件和客户端类型都从 Rust 注册项生成，不维护第二份手写契约。

## 两个入口

Tauri 插件暴露固定的 `call`、`subscribe`、`unsubscribe` 命令；它们根据 `fnName` 调用同一个 `ApplicationApi`。Tauri 的逐条 `generate_handler!` 列表需要在编译期确定，不能直接从运行时 inventory 枚举生成。插件初始化脚本由 `scripts/application-plugin-init.ts` 编译生成，向 WebView 注入窄的调用对象；前端 typed client 隐藏固定命令和 Channel 细节。

Axum adapter 从同一个 `ApplicationApi` 构建路由组：`POST /call` 接收 `{ "fn_name": "profiles.list", "params": {} }`；`GET /events/{fn_name}` 提供 SSE。桌面端本阶段不启动 HTTP 监听器；Axum 路由已通过进程内请求测试，后续最小 WebUI 直接调用该接口。

事件源由 Clash actor client 提供。Axum 将事件序列化为 SSE `data`，连接断开即丢弃接收端；Tauri 用 Channel 单向发送相同的事件 DTO，并由显式 `unsubscribe` 停止转发任务。SSE 表示单向事件流；普通请求仍通过 `call` 返回。

## 生成链

后端导出器遍历 `ApplicationApi`，写入 `backend/tauri/gen/application-api.json`，并把 Specta 类型直接写到 `frontend/interface/src/application-api/types.ts`。仓库根目录的 `pnpm generate:application-api` 先编译插件初始化 TypeScript，再运行后端导出器，最后运行 `scripts/generate-application-api.ts` 生成前端源码中的 `generated.ts` 方法映射。`pnpm check:application-api` 检查四份生成产物是否与源代码一致。前端生成脚本均为 TypeScript。

Clash 事件中的 `u64` 沿用现有 tauri-specta 绑定的 `number` 映射；JS 对超过 `Number.MAX_SAFE_INTEGER` 的值存在精度限制。后续若有需要精确保留的大整数，应给对应字段定义明确的字符串线格式。

首批方法为 profiles 查询/激活、Clash snapshot 和 Clash 事件。旧 Tauri commands、events 与业务 hooks 暂时并行；桌面 Host API、LuCI/OpenWrt 和完整 WebUI 不属于此切片。
