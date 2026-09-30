# 复用现有 UI 的 Tauri / HTTP 命令适配设计

**日期：** 2026-09-28
**状态：** 现有 UI 命令与事件已接入统一传输入口；HTTP 提供默认关闭、经过认证的 loopback 调试访问

## 目标和范围

短期优先复用现有 UI：页面和 hooks 调用 `rpc` 门面。`rpc-bindings.ts` 承载完整的 Specta 命令、事件与类型契约；桌面传输由只包含 `call_rpc` 等少量入口及 Tauri 事件的 `bindings.ts` 承载。已经具备跨端实现的命令在两条路径上执行同一个业务实现，不再维护两份命令主体。迁移范围是现有 UI 使用的全部命令，而非只选一个试点；窗口、托盘、系统集成等能力当前仍由 Tauri 实现，待平台适配后也应进入全平台接口，不把它们永久定义为桌面专属。

HTTP 是宿主应用内的调试适配器：桌面 Debug 页可显式开启绑定 `127.0.0.1` 随机端口的服务器，退出应用时关闭。生产构建复用内嵌 frontend dist，开发构建反代配置的 dev server 并支持 HMR。它不承诺独立部署、远程访问、完整平台能力或公共外部 API。迁移应尽量局限在命令声明、薄适配层和前端调用底座，不借此重构页面、hooks、actor 图或业务域。允许按可独立验证的批次提交，但完成清单仍覆盖现有 UI 使用的全部命令。

## 统一调用链

```text
现有页面 / hooks
    → rpc 类型化门面
        → Specta 生成的统一 RPC 契约
            → 薄传输入口
                → 同一个 Rust 命令实现
                    → NyanpasuClient / typed actor client / pure service
```

共享实现接受显式依赖，不读取 `::global()`、Tauri 状态、窗口或 HTTP 请求。Tauri wrapper 负责参数及 `tauri::State` 适配，HTTP handler 负责 JSON 反序列化、依赖接入与返回值编码。`NyanpasuClient` 仍是应用门面；命令迁移时依赖可以进一步收窄为 typed client 或 port，传输契约不因此改变。

新设计文档的 `ServiceContext` 是一种可选的 HTTP 接合方式。当前主要依赖是 `NyanpasuClient`，先由组合根显式装配具体上下文；不要仅为复刻示例引入 `TypeId + Any` 服务容器，也不要让业务代码通过容器任意查找服务。若未来的独立依赖确实增加，再评估有界的服务上下文。不得新增全局服务或把 Tauri 类型带入业务层。

## 命令声明、注册与生成

现有 UI 的命令统一使用 `#[nyanpasu_macro::rpc]` 注册。宏生成同一命令实现的 Tauri 分发处理器和 HTTP 处理器；只有声明 `#[nyanpasu_macro::rpc(http)]` 的命令才允许 HTTP 调用，签名检查进一步排除未适配的上下文。纯 lifetime 泛型可用，type/const 泛型仍不可用；依赖窗口、托盘等 Tauri 上下文的命令在桌面端注入当前上下文执行，实验 HTTP 返回明确的 `unsupported`，后续再逐项提取平台 port。两类命令均保留原命令名与 tauri-specta 类型，不另建手写 JSON 清单。顶层参数沿用现有 tauri-specta 的 camelCase 约定，嵌套 DTO 遵守自己的 Serde 属性。

命令由静态注册项收集，启动时拒绝重复命令名；Specta 的逐条收集仍用于生成完整类型契约，不依赖运行时 `inventory` 自动导出类型。`bindings.ts` 由单独的 Tauri transport builder 生成，注册 `call_rpc`、原生 Channel 订阅和事件；`rpc-bindings.ts` 从完整 Specta 命令清单生成，并替换为传输无关的命令调用与浏览器事件入口。生成检查必须在必要替换点失效时失败，不能只打印警告；不得手工维护生成文件。

实验 HTTP 请求保持现有命令名和参数形状，例如 `{"method":"get_profiles","params":{}}`。成功时返回裸 JSON 值，客户端沿用生成命令函数现有的 `Result` 包装。协议是项目内部轻量 RPC，不宣称符合 JSON-RPC 2.0。Tauri 传输和 HTTP 线上都使用 `{kind, message}` 错误 DTO，区分无效参数、未知命令和应用失败；错误 DTO 保留 `domain_error` 与操作诊断信息，前端传输边界解包领域错误供现有 typed error UI 使用。网络失败继续作为 Promise rejection。HTTP 提取 JSON 失败及非 JSON 响应也需要可读的客户端回退错误。

## 事件与平台能力

普通命令与事件分开适配。桌面继续使用 Tauri Event，实验浏览器入口通过 `GET /bridge/events?name=...` 的 SSE 接收同名通知；Tauri 事件在宿主边界转发到进程内广播总线，payload 沿用 Serde/Specta 类型。SSE 广播可能丢事件，重连必须恢复已有订阅，需要正确恢复的页面在收到通知或重连后拉取权威快照。浏览器 `emit` 明确返回不支持。连接详情在桌面使用原生 Tauri Channel，在浏览器使用按需 SSE 端点；取消最后一个订阅会释放流需求。

所有现有命令均进入统一注册表；尚无 HTTP/全平台适配的命令返回 `unsupported`，前端不维护永久的平台能力或 HTTP 白名单。文件与资源内容后续走独立端点或桌面协议，不放进 JSON RPC。此次不要求完成 Channel、文件或系统能力的浏览器等价实现，后续可逐步替换前端 Tauri 插件。

每次启动生成随机访问凭证，桌面提供含该凭证的访问链接。首次打开时验证凭证、设置 HttpOnly / SameSite=Strict 认证及会话 cookie，然后重定向去掉 URL 凭证。未认证的 RPC、SSE、静态资源和开发代理请求均被拒绝；Host/Origin 校验继续防护跨站调用与 DNS rebinding。凭证在停止或重启后失效，不向开发代理上游转发 cookie。持有访问链接即具有已开放应用操作的权限，应将链接视为凭证。

`url_delay_test` 与网络诊断请求不向 HTTP 开放；打开宿主路径/URL、系统服务安装控制等原生操作也保持桌面限定。HTTP 仅显式开放已有适配的应用功能，例如用户授权的订阅导入/更新、配置和代理操作，不能把任意 URL 诊断接口作为网络请求代理。

组合根注入 HTTP 路由工厂，在 RPC 图构造完成后填入弱依赖引用；HTTP actor 在启用时创建 router。`NyanpasuClient.set_debug_http_enabled(enabled)` 不接受 Axum 类型。actor 启停调用等待真实结果，服务器保留有限的连接排空期限，随后强制结束 SSE/HMR。

## 与早期实验实现的关系

早期 `ApplicationApi` 的固定 `call/subscribe/unsubscribe` 插件、独立客户端与 JSON 目录已被统一命令和事件入口取代。命令名、参数与事件载荷类型由现有 tauri-specta 绑定继续提供，不维护第二套 RPC DTO 与类型导出。

现有 UI 使用的命令已从声明直接注册到统一 RPC；不再维护单独的 JSON 状态清单或 HTTP 命令名单。`get_clash_logs` 已改读 actor 持有的 Clash 日志快照，避免继续使用无生产写入者的 legacy global buffer。

未来若需要重新拆分命令颗粒度，可在共享命令实现下面逐步提取更窄的 typed client、纯服务或 port；不在这次短期复用工作中预先建立大框架。

## 验证与完成标准

1. 现有 UI 使用的所有命令与事件均从声明进入统一传输入口；尚未实现的平台能力明确返回不支持。
2. 已具备跨端实现的命令从现有生成函数调用，桌面结果不变，HTTP 注册项调用同一 Rust 实现。
3. 进程内 HTTP 测试验证请求形状、返回值、未知命令、无效参数及不支持的平台命令；前端测试验证 IPC/HTTP 选择与错误回退。
4. 类型和客户端生成可复现，检查命令能发现过期或失效的传输接入点；不需要改页面和 hooks 来选择传输。
5. 没有新全局服务、通用 service locator、隐藏的 Tauri 业务依赖或第二份命令业务主体。监听器默认关闭、仅 loopback；认证、原生能力限制、SSE/HMR 排空和应用退出均有回归覆盖。
