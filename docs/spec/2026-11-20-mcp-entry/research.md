# MCP 入口：调研原始记录

[设计](design.md) 的证据附录。调研日期 2026-11-20。

**可信度约定：** 「源码」指本机读到的源文件（给出路径与行号）；「一手」指规范原文或客户端官方文档；「二手」指第三方文章、论坛、搜索摘要。只有一个来源的结论标注「单一来源」。网页结论由两个只读调研子代理抓取并回报，我没有逐页复核原文；与 rmcp 源码能交叉印证的条目另行标注「已与源码互证」。

---

## 1. rmcp 3.11.0（源码）

来源：`https://static.crates.io/crates/rmcp/rmcp-3.11.0.crate` 解包后阅读；本机 cargo registry 里也已有 `rmcp-3.11.0`。发布日期 2026-11-17（`CHANGELOG.md:10`）。`edition = "2024"`，`rust-version = "1.88"`。

### 1.1 协议版本

`src/model.rs:169-209`：

```rust
pub const V_2026_07_28: Self = Self(Cow::Borrowed("2026-07-28"));
pub const V_2025_11_25: Self = Self(Cow::Borrowed("2025-11-25"));
pub const V_2025_06_18: Self = Self(Cow::Borrowed("2025-06-18"));
pub const V_2025_03_26: Self = Self(Cow::Borrowed("2025-03-26"));
pub const V_2024_11_05: Self = Self(Cow::Borrowed("2024-11-05"));
pub const LATEST: Self = Self::V_2026_07_28;
pub const NO_INITIALIZE: Self = Self::V_2026_07_28;          // 该版本起没有 initialize 握手
pub const LATEST_WITH_INITIALIZE: Self = Self::V_2025_11_25;
```

`ServerHandler::supported_protocol_versions` 默认返回全部 `KNOWN_VERSIONS`（`src/handler/server.rs:452-454`），即同一个服务同时服务两个时代的客户端。这与规范调研（§2）“最新修订是 2026-07-28、取消会话与握手”互证。

### 1.2 Streamable HTTP 服务端

`src/transport/streamable_http_server/tower.rs`：

- `StreamableHttpService<S, M>` 实现 `tower_service::Service<http::Request<_>>`（`:1473-1495`），构造为 `StreamableHttpService::new(service_factory, Arc<M: SessionManager>, StreamableHttpServerConfig)`（`:1554`）。依赖只有 `http 1`、`tower-service 0.3`，可以直接 `nest_service` 进 axum 0.8（仓库锁定 `axum 0.8.9`、`http 1.5.0`、`tower-service 0.3.3`）。
- `StreamableHttpServerConfig`（`:219-328`）的相关字段：
  - `legacy_session_mode: bool`：“Only applies to legacy protocol versions (`< 2026-07-28`). Per SEP-2567, sessions are removed from the `2026-07-28` version, so requests negotiating that version are always served statelessly regardless of this setting.”
  - `json_response: bool`：无会话时简单请求直接回 `application/json`；handler 在最终应答前发出通知或请求时退回 `text/event-stream`。
  - `allowed_hosts`：“By default, Streamable HTTP servers only accept loopback hosts to prevent DNS rebinding attacks”；`disable_allowed_hosts()` 关闭。
  - `allowed_origins`：默认空表即不校验；缺少 `Origin` 的请求放行。
  - `cancellation_token`、`max_request_body_bytes`（默认 4 MiB，`:54`）、`stateless_protocol_metadata_required`（默认 `false`，“an absent header is treated as protocol version `2025-03-26`”）。
- 会话管理器：`session/local.rs:33` `LocalSessionManager`（内存会话）、`session/never.rs:19` `NeverSessionManager`（无状态）。
- **HTTP 请求数据进入 handler。** `:1410-1470`：“The service consumes the request body but injects the remaining `http::request::Parts` into `crate::model::Extensions`, which is accessible through `crate::service::RequestContext`.” axum 中间件写进 `Parts.extensions` 的值可用 `ctx.extensions.get::<http::request::Parts>().unwrap().extensions.get::<T>()` 取出。**这是把认证中间件产出的 `AuthContext` 交给 MCP handler 的通道。**

### 1.3 ServerHandler

`src/handler/server.rs`：

```rust
fn call_tool(&self, request: CallToolRequestParams, context: RequestContext<RoleServer>)
    -> impl Future<Output = Result<CallToolResponse, McpError>> + MaybeSendFuture + '_;   // :611
fn list_tools(&self, request: Option<PaginatedRequestParams>, context: RequestContext<RoleServer>)
    -> impl Future<Output = Result<ListToolsResult, McpError>> + MaybeSendFuture + '_;    // :618
fn get_info(&self) -> ServerConfig;                                                       // :690
fn list_skills(..); fn get_skill(..); fn read_resource_directory(..);                     // :727-749
```

`call_tool` 的文档（`:596-610`）规定错误的两条通道：模型应当读到的失败用 `Ok(CallToolResult::error(...))`（`isError: true`）；`Err(McpError)` 只用于请求本身不可路由，“MCP clients typically render protocol errors opaquely; the caller will not see your message”。

`Tool`（`src/model/tool.rs:17-39`）：`name`、`title`、`description`、`input_schema: Arc<JsonObject>`、`output_schema`、`annotations`、`icons`、`meta`。`ToolAnnotations`（`:54-91`）：`title`、`read_only_hint`、`destructive_hint`、`idempotent_hint`、`open_world_hint`。不使用 `#[tool]` 宏时可以手工构造 `Tool`，因此 `macros` 特性不是必需的。

### 1.4 征询（elicitation）与 MRTR

- `Peer::<RoleServer>::elicit` / `elicit_url`（`src/service/server.rs:1175`、`:1341`）是服务端主动向客户端发请求。文档 `:1171-1172`：“Errors on protocol `2026-07-28` or later, which forbids server-to-client requests; return an `InputRequiredResult` instead.”
- MRTR（`src/model/mrtr.rs:1-37`）：服务端对 `tools/call` 返回 `InputRequiredResult`，客户端完成 `InputRequests` 后带着 `InputResponses` 与回显的 `requestState` 重试原请求。“The SDK only lets an `InputRequiredResult` reach a peer that negotiated protocol version `2026-07-28` or newer; older peers get a protocol error instead.”（实现在 `src/handler/server.rs:309-311`）。“`requestState` is untrusted … a stateless server that stores meaningful data in it MUST verify integrity”，对应 `request-state` 特性的 `RequestStateCodec`。
- CHANGELOG 3.9.0：“reject server-to-client requests on discover sessions (#1397)”。

**推论：** 用征询做确认，需要两套实现——2025 时代的客户端要有状态会话加 SSE 才能收到服务端请求（`legacy_session_mode: true`），2026-07-28 的客户端要走 MRTR 并密封 `requestState`。不用征询时，服务可以对两个时代都保持无状态。

### 1.5 Skills 扩展（SEP-2640）与 `SkillCatalog`

- 3.9.0 加入“support the Skills extension (SEP-2640) (#1392)”，3.10.0 加入“load skills from a directory tree (#1409)”，3.11.0 是若干加固（`CHANGELOG.md`）。
- `src/handler/server/skill_catalog.rs`（984 行，特性 `skills-fs`，引入 `cap-std`、`cap-fs-ext`、`yaml_serde`、`windows-sys`）：
  - `:1-7`：“Each folder below the root that holds a `SKILL.md` is a skill, served at `skill://<path from the root>/SKILL.md`.”
  - 公开 API：`SkillCatalog::from_dir(root)`（`:160`）、`reload()`、`skills()`、`list_skills()`、`get_skill(uri)`、`resources()`、`read_resource(uri)`、`read_resource_directory(uri)`（`:174-238`）。**只有从目录加载这一个构造器，没有从内存内容构造的入口。**
  - 限额：`SKILL_FILE_LIMIT = 512`、`SKILL_BYTE_LIMIT = 16 MiB`（`:113-116`）。“Loading reads the whole tree with blocking I/O; from async code, call it inside `tokio::task::spawn_blocking`.”
  - 服务端用法（`:9-87`）：能力声明 `enable_resources().enable_skills_with_directory_read()`，把 `list_skills`、`get_skill`、`read_resource_directory`、`list_resources`、`read_resource` 委托给它。
- 模型类型在 `src/model/skill.rs`：`SKILLS_EXTENSION_ID = "io.modelcontextprotocol/skills"`（`:21`）、`Skill { uri, frontmatter, resources, meta }`（`:237`）、`SkillFrontmatter { name, description, extra }`（`:152`）、`SkillResource { uri, digest, size }`（`:66`）。不启用 `skills-fs` 也能自己实现 `list_skills`/`get_skill`。

### 1.6 接入本仓库所需的特性集

`default-features = false`，启用 `server`（带入 `schemars 1`、`uuid`、`transport-async-rw`）与 `transport-streamable-http-server`（带入 `server-side-http`、会话与 worker）。仓库 `Cargo.lock` 里已有 `schemars 1.2.2`。`elicitation`、`macros`、`skills-fs`、`auth`（这是**客户端**的 OAuth）都不需要。未实际编译验证。

---

## 2. MCP 规范（一手，子代理抓取）

### 2.1 修订

- `https://modelcontextprotocol.io/specification/versioning`：“The **current** protocol version is **2026-07-28**.”（已与 rmcp 源码互证）
- 2026-07-28 相对 2025-11-25 的主要变化（`https://modelcontextprotocol.io/specification/2026-07-28/changelog`）：取消协议级会话与 `Mcp-Session-Id`；取消 `initialize`/`notifications/initialized` 握手，每个请求在 `_meta` 里带协议版本与客户端能力；服务端必须实现 `server/discover`；GET 流与 `resources/subscribe` 由 `subscriptions/listen` 取代；取消 `Last-Event-ID` 续传；服务端到客户端的请求（征询、采样、roots）改为 MRTR；Tasks 移入扩展；Roots、Sampling、Logging 弃用；动态客户端注册弃用，改用 CIMD。

### 2.2 授权

来源：`https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization`（2025-06-18 措辞相同）。

- “Authorization is **OPTIONAL** for MCP implementations. When supported: Implementations using an HTTP-based transport **SHOULD** conform to this specification.”
- 一旦实现 MCP 授权，就是 OAuth 2.1：“MCP servers **MUST** implement OAuth 2.0 Protected Resource Metadata (RFC9728)”，且元数据“**MUST** include the `authorization_servers` field containing at least one authorization server”。
- 规范没有提到 API key 或静态 token。授权可选、符合性只是 SHOULD，所以静态 `Authorization: Bearer` 属于“不实现 MCP 授权”，不被禁止（这一句是解读，可信度中）。
- 401：2025-11-25 起“**MUST** implement one of”两种发现方式（`WWW-Authenticate` 的 `resource_metadata`，或 well-known URI）；客户端找不到元数据时“Abort or use pre-configured values”。“Invalid or expired tokens **MUST** receive an HTTP 401 response.”
- CIMD 的 client id“**MUST** use the "https" scheme”。OAuth Client Credentials、Enterprise-Managed Authorization 两个扩展都仍然需要授权服务器。

### 2.3 Streamable HTTP（2026-07-28）

来源：`https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http`。

- “Servers **MUST** validate the `Origin` header on all incoming connections to prevent DNS rebinding attacks. If the `Origin` header is present and invalid, servers **MUST** respond with HTTP 403 Forbidden.”
- “When running locally, servers **SHOULD** bind only to localhost (127.0.0.1)”；“Servers **SHOULD** implement proper authentication for all connections.”
- “Every POST request to the MCP endpoint **MUST** include an `MCP-Protocol-Version` header”；新增必需头 `Mcp-Method`，以及 `tools/call` 等请求上的 `Mcp-Name`。
- 旧客户端带来的 `Mcp-Session-Id`：“ignore it, and do not mint or echo session IDs.” 对 GET、DELETE：“respond with `405 Method Not Allowed`.”

### 2.4 征询

来源：`https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation`。

- 表单模式传结构化数据，客户端可见；URL 模式（2025-11-25 引入）用于“sensitive interactions that must _not_ pass through the MCP client”。
- “Servers **MUST NOT** use form mode elicitation to request sensitive information such as passwords, API keys, access tokens, or payment credentials.”
- URL 模式“is _not_ for authorizing the MCP client's access to the MCP server”。
- 规范没有专门的“确认”原语；是/否的表单征询可以用作确认，前提是不经它传秘密。

### 2.5 工具元数据

来源：`https://modelcontextprotocol.io/specification/2026-07-28/server/tools`。

- 注解只是提示：客户端“**MUST** consider tool annotations to be untrusted unless they come from trusted servers”。默认值 `readOnlyHint: false`、`destructiveHint: true`、`idempotentHint: false`、`openWorldHint: true`。
- 工具列表“**MUST NOT** vary per-connection”，但“**MAY** vary by the authorization presented”。
- 列表结果要带 `ttlMs` 与 `cacheScope`（rmcp 3.11 的 `test_cache_hints` 互证其存在）。

---

## 3. 客户端支持矩阵（一手为主，子代理抓取）

`?` 表示官方文档没有覆盖。`modelcontextprotocol.io/clients` 的全客户端特性矩阵在调研当日已重定向到首页，以下各列取自各客户端自己的文档。

| 客户端                                  | Streamable HTTP                                                   | 静态请求头的官方写法                                                                                                               | 变量替换                    | 是否强制 OAuth                        | 明文 http / 私有 CA                                                                                                       | 征询       | 来源                                  |
| --------------------------------------- | ----------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | --------------------------- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------- | ---------- | ------------------------------------- |
| Claude Code                             | 是                                                                | `claude mcp add --transport http <name> <url> --header "Authorization: Bearer <token>"`；`.mcp.json` 的 `headers`；`headersHelper` | `${VAR}`、`${VAR:-default}` | 否；只在 401/403 时走 OAuth           | http 可用（文档未明说）；信任系统证书库与 `NODE_EXTRA_CA_CERTS`                                                           | 表单 + URL | `https://code.claude.com/docs/en/mcp` |
| Claude Desktop：本地配置文件            | 否，`claude_desktop_config.json` 只有 stdio（**二手，可信度中**） | 经 `mcp-remote --header`                                                                                                           | `env` 块                    | 否                                    | 经桥接                                                                                                                    | ?          | —                                     |
| Claude Desktop / claude.ai 自定义连接器 | 远程 HTTPS                                                        | 连接器对话框的“Request headers”，但“in beta and available to a limited set of organizations”                                       | —                           | 否                                    | **连不上 localhost 与局域网**：请求来自 Anthropic 云端，服务器“must be reachable over the public internet”，标准 443 端口 | ?          | Anthropic 支持文档                    |
| OpenAI Codex CLI                        | 是                                                                | `~/.codex/config.toml`：`url`、`bearer_token_env_var`、`http_headers`、`env_http_headers`                                          | 环境变量名字段              | 否                                    | 文档示例是 `http://localhost:3000/mcp`；自定义 CA 经 `CODEX_CA_CERTIFICATE`/`SSL_CERT_FILE`（**二手，可信度低–中**）      | ?          | Codex 文档                            |
| Cursor                                  | 是                                                                | `~/.cursor/mcp.json` 或 `.cursor/mcp.json`：`url` + `headers`，如 `"Authorization": "Bearer ${env:MY_SERVICE_TOKEN}"`              | `${env:NAME}`               | 否                                    | 文档示例是 `http://localhost:3000/mcp`                                                                                    | 支持       | Cursor 文档                           |
| VS Code（Copilot）                      | 是，失败回退 SSE                                                  | `.vscode/mcp.json`：`type: "http"`、`url`、`headers`，配合 `inputs`（`promptString`、`password: true`）                            | `${input:…}`、`${env:NAME}` | 否                                    | 未说明                                                                                                                    | 表单 + URL | VS Code 文档                          |
| Windsurf（文档已并入 Devin Desktop）    | 是                                                                | `mcp_config.json`：`serverUrl` + `headers`                                                                                         | `${env:VAR}`                | 否                                    | ?                                                                                                                         | 未提及     | `docs.windsurf.com`（重定向）         |
| Cline                                   | 是，须写 `"type": "streamableHttp"`，否则默认 SSE                 | `cline_mcp_settings.json` 的 `headers`                                                                                             | ?                           | 否                                    | ?                                                                                                                         | ?          | Cline 文档                            |
| Zed                                     | 远程 `url`                                                        | `settings.json`：`url` + `headers`                                                                                                 | ?                           | 只在没有 `Authorization` 头时走 OAuth | ?                                                                                                                         | **不支持** | Zed 文档                              |
| Gemini CLI                              | 是，字段是 `httpUrl`（`url` 表示 SSE）                            | `gemini mcp add --transport http --header "Authorization: Bearer …"`                                                               | `env` 支持；`headers` 不明  | 否；401 触发 OAuth 发现               | ?                                                                                                                         | ?          | Gemini CLI 文档                       |
| opencode                                | 是（`type: "remote"`）                                            | `opencode.jsonc`：`headers`，须设 `"oauth": false`                                                                                 | `{env:NAME}`                | 默认开启，可关                        | ?                                                                                                                         | 支持       | opencode 文档                         |
| JetBrains AI Assistant                  | 是                                                                | **文档没有写请求头**                                                                                                               | ?                           | 未说明                                | 示例是 `http://…`                                                                                                         | ?          | JetBrains 文档                        |

补充：

- Claude Code v2 只把 MCP OAuth 凭证发给 HTTPS 或回环地址上的 token 端点：“Sign-in fails for a server whose token endpoint is plain `http://` anywhere else, such as a device on your local network.” 即**局域网明文 HTTP 上的 OAuth 在 Claude Code 里不可用**（一手）。
- `mcp-remote`（`https://github.com/geelen/mcp-remote`）：“To bypass authentication, or to emit custom headers on all requests to your remote server, pass `--header` CLI arguments.” 非 HTTPS 地址要 `--allow-http`；Windows 上 Cursor、Codex、Claude Desktop 的 `args` 不转义空格，要写成 `"Authorization:${AUTH_HEADER}"` 并把值放进 `env`；私有 CA 用 `NODE_EXTRA_CA_CERTS`。

未能核实：Claude Desktop 配置文件只支持 stdio（只有二手来源）；自定义连接器是否接受自签证书；Codex 的征询、`list_changed`、注解与 CA 行为；Cursor 与 Windsurf 的 `list_changed` 与 TLS 行为；VS Code 连接局域网 http 与私有 CA；JetBrains 与 Junie 的请求头支持；除 Claude Code 之外哪些客户端已协商 2026-07-28。

---

## 4. 渐进披露的先例（子代理抓取）

### 4.1 Agent Skills 与 Skills over MCP

- Anthropic Agent Skills 的三级加载（`https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview`）：元数据“Always (at startup) | ~100 tokens per Skill”；`SKILL.md` 正文“When Skill is triggered | Under 5k tokens”；附带文件“As needed”。`name` 至多 64 字符，`description` 至多 1024 字符。
- SEP-2640（`https://modelcontextprotocol.io/seps/2640-skills-extension`）：状态 Final，扩展 id `io.modelcontextprotocol/skills`；每个 skill 文件是一个资源，“SHOULD use the `skill://` URI scheme”；服务端 MUST 实现 `skills/list`、`skills/get`（已与 rmcp 源码互证）。宿主“MUST treat MCP-served skills as a higher-risk surface than remote tool invocation”。
- **客户端支持稀少（可信度中）：** 官方扩展矩阵里只有 ChatGPT（Partial）、fast-agent、MCP Inspector 有文字条目；第三方仓库 `zeke/skills-over-mcp`（“as of September 2026”）记 Claude Code 为“Not supported… Internal prototype only”；Claude Code 的 skills 文档没有提到 MCP 提供的 skill。
- MCP 的 Progressive Disclosure 工作组 2026-09-21 成立，状态“Active Exploration”，明确不指定唯一做法（“grouping, tool-search, code-mode”并存），并把“按特权级别组织原语”划在范围之外。

### 4.2 客户端已有的延迟加载

| 客户端          | 行为                                                                                                                                                                             | 来源                                                |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| Claude Code     | 默认开启工具搜索：“Only tool names and server instructions load at session start.”；`_meta["anthropic/alwaysLoad"]` 可让单个工具始终加载；服务端 `instructions` 截断在 4096 字符 | `https://code.claude.com/docs/en/mcp`               |
| Codex CLI       | PR #29486（2026-06-22 合入）：“Defer all effective MCP tools when `tool_search` and namespaced tools are supported.”；建议 `instructions` 前 512 字符自成一体                    | `https://github.com/openai/codex/pull/29486`        |
| Cursor          | 工具描述同步到文件夹，代理按需查阅；“reduced total agent tokens by 46.9%”                                                                                                        | `https://cursor.com/blog/dynamic-context-discovery` |
| VS Code Copilot | “Cannot have more than 128 tools per request.”；实验性的虚拟工具分组                                                                                                             | VS Code 文档                                        |
| Windsurf        | “limit of 100 total tools”                                                                                                                                                       | Windsurf 文档                                       |

本仓库 HTTP 暴露的命令是 120 条（§5）：逐条注册为工具会超过 Windsurf 的上限、逼近 VS Code 的上限，在没有延迟加载的客户端里要预先加载全部 schema。

### 4.3 元工具的先例

| 服务器             | 工具                                                                                                                                     | 备注                                                                                                                                                                | 来源                                                                                    |
| ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Cloudflare API MCP | `search` + `execute`（代码模式）                                                                                                         | 2500+ 端点约 1000 token；权限只靠连接时的 OAuth scope                                                                                                               | `https://blog.cloudflare.com/code-mode-mcp/`                                            |
| GitHub MCP         | 静态 toolset（`--toolsets`、`X-MCP-Toolsets`、`X-MCP-Readonly`）；动态 toolset（`enable_toolset` 等）**已在 PR #2512（2026-05-20）删除** | 删除理由：“Dynamic mode was local-only — never offered by the remote server”                                                                                        | `https://github.com/github/github-mcp-server/pull/2512`                                 |
| Speakeasy Gram     | `search_tools` / `describe_tools` / `execute_tool`                                                                                       | 输入 token −96.7%/−91.2%，但调用次数 2–3 倍、耗时约 +50%；早期版本模型会跳过搜索，“because it could not see what tools existed”，修法是把分类大纲写进搜索工具的描述 | `https://www.speakeasy.com/blog/how-we-reduced-token-usage-by-100x-dynamic-toolsets-v2` |
| Block Square MCP   | `get_service_info` → `get_type_info` → `make_api_request`                                                                                | 描述里写明“You must call this before calling the make_api_request tool.”                                                                                            | `https://engineering.block.xyz/blog/build-mcp-tools-like-ogres-with-layers`             |
| AWS API MCP        | `call_aws`、`suggest_aws_commands`                                                                                                       | 安全在服务端：`READ_OPERATIONS_ONLY`；`REQUIRE_MUTATION_CONSENT` 经征询要同意                                                                                       | `https://awslabs.github.io/mcp/servers/aws-api-mcp-server`                              |
| MotherDuck         | `query`（只读）与 `query_rw`（写）                                                                                                       | 文档教用户在 Claude、Cursor、Claude Code 里单独禁用写工具                                                                                                           | `https://motherduck.com/docs/key-tasks/ai-and-motherduck/securing-read-only-access/`    |

单一通用 `call` 工具的已知代价：

- Claude Code 的允许规则只按工具名匹配（`mcp__srv__tool`），一个通用工具让用户无法表达“读放行、写询问”；`readOnlyHint` 让只读调用可以并行、断线后重试一次。
- Codex 的 `default_tools_approval_mode = "writes"`“prompts for tools that aren't marked read-only”。
- Cursor 论坛（2026-10-13，二手）：`list_tools()`/`call_tool()` 式的通用工具“bypasses Cursor's tool confirmation checks”。
- 校验移到服务端；规范本来就要求“Servers MUST: Validate all tool inputs”，并希望输入校验失败以 `isError: true` 返回，让模型自行修正。

### 4.4 `list_changed`

- Claude Code 交互模式下会刷新，但 `anthropics/claude-code#92033`（未关闭，二手）报告工具列表中途变化使提示缓存整体失效。
- Cursor 员工 2026-10-15 的回复承认缓存陈旧是“known and being tracked”。
- OpenAI 的工具搜索指南：“changing the loaded tool set will break the model's cache from that point forward”。
- GitHub 已删除依赖它的动态 toolset（§4.3）。

### 4.5 资源与 `instructions`

- 模型能否自己读资源：Claude Code 可以（自动提供列出与读取资源的工具）；Codex 有 `list_mcp_resources`/`read_mcp_resource`（可信度中）；VS Code 只能由用户附加，并建议“returning content in a tool result”；Cursor 未核实。
- 结论：只用资源承载的目录在各客户端之间不可靠。

### 4.6 已发表的测量

没有独立研究在同一组任务上比较全部做法；数字多为厂商自报（Anthropic 工具搜索 −85% token；Speakeasy −96.7%；Cloudflare 1.17M → 约 1K；Cursor −46.9%；GitHub Copilot 覆盖率 69.0% → 94.5%）。相对排名的可信度低–中。

---

## 5. 本仓库的事实（源码，`main` 工作区）

统计脚本对 `backend/nyanpasu-rpc-protocol/src/permission/generated.rs` 去重（`cfg` 分支算一条）得出：

- 资源 233 个：命令 213、事件 17、路由 2、流 1。
- 目标含 `Http` 的命令 120 条，按资源 × effect × `sensitive`：

| 资源       | 查询 | 其中敏感 | 变更 | 其中敏感 | 二级命名空间                                                              |
| ---------- | ---- | -------- | ---- | -------- | ------------------------------------------------------------------------- |
| `logs`     | 6    | 3        | 6    | 1        | `app`、`core`                                                             |
| `meta`     | 1    | 0        | 0    | 0        | —                                                                         |
| `profile`  | 15   | 11       | 20   | 0        | `content`、`layout`、`providers`、`runtime`、`store`、`sync`              |
| `proxy`    | 7    | 1        | 6    | 0        | `connections`、`delay`、`groups`、`mode`、`providers`、`rules`            |
| `settings` | 9    | 6        | 7    | 0        | `app`、`clash`、`network`、`storage`                                      |
| `system`   | 25   | 7        | 9    | 0        | `core`、`hotkeys`、`icons`、`network`、`os`、`proxy`、`service`、`status` |
| `traffic`  | 7    | 0        | 2    | 0        | `connections`、`usage`                                                    |

- `remote` 的命令没有一条对 HTTP 暴露。HTTP 可达的敏感命令共 29 条。
- 命令没有 JSON Schema（r6 更正：下一句说“类型信息只存在于 Specta”不准确，`#[rpc]` 宏在语法层面也有每个线上参数的名字与类型，`nyanpasu-rpc-macro/src/unified_command.rs:625-659`）：handler 的签名是 `fn(Arc<RpcDependencies>, RpcCallMeta, RpcCaller, Value) -> Result<RpcFuture, RpcError>`（`backend/nyanpasu-rpc/src/registry.rs:261-262`），参数在宏展开的包装里用 serde 解码。类型信息只存在于 Specta：`specta::datatype::Function { name, args: Vec<(Cow<str>, DataType)>, result: Option<DataType>, docs, .. }`（`specta-2.0.0-rc.28/src/datatype/function.rs:14-27`），由 `backend/tauri/src/specta_export.rs:40-50` 的 `schema_commands!` 收集，导出为 `frontend/rpc/src/rpc-bindings.ts`（约 415 KB）。registry 里没有 `specta-jsonschema` 一类的包。
- CLI 现存一张手写的精选表（`backend/nyanpasu-cli/src/commands.rs:39-53`，27 项，各带手写的 JSON Schema 文本），现存 25 项（`:190-416`；删除 stdio MCP 之前是 27 项），供 CLI 子命令使用；已删除的 stdio MCP 曾经复用它（`git show fb9bfa77e^:backend/nyanpasu-cli/src/mcp.rs`）。

---

## 6. r2 的复核

2026-11-21。对照一份独立提案（codex gpt-6.2-astra）逐条复核它引用的 rmcp 源码，并补查用户裁定涉及的仓库事实。rmcp 路径相对 `rmcp-3.11.0/`（发布包，与本机 cargo registry `G:/packages/cargo/registry/src/rsproxy.cn-e3de039b2554c837/rmcp-3.11.0` 为同一版本）。

### 6.1 rmcp：独立提案的说法与复核结果

| 说法                                                                    | 复核                                                                                                                                                                                                                                                                            | 结论                                                                                                |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| 无状态要显式 `with_legacy_session_mode(false)` 配 `NeverSessionManager` | `StreamableHttpServerConfig::default()` 里 `legacy_session_mode: true`、`json_response: false`（`src/transport/streamable_http_server/tower.rs:336-352`）；`NeverSessionManager` 派生了 `Default`（`session/never.rs:17-21`）                                                   | 属实。r1 写了 `legacy_session_mode(false)`，没有指明默认值是 `true`，也没有确认会话管理器的构造方式 |
| 默认不校验 `Origin`                                                     | 默认 `allowed_origins: vec![]`、`validate_empty_origin_allowlist: false`；字段文档：“Defaults to an empty list, which disables Origin validation”（`tower.rs:251-272`、`:344-346`）                                                                                             | 属实，与 r1 一致                                                                                    |
| 默认只放行回环 `Host`                                                   | `allowed_hosts: vec!["localhost", "127.0.0.1", "::1"]`（`tower.rs:344`）                                                                                                                                                                                                        | 属实                                                                                                |
| `jsonrpc_http_status` 对业务错误回 200                                  | `tower.rs:789-805`：只有 `UNSUPPORTED_PROTOCOL_VERSION`、`MISSING_REQUIRED_CLIENT_CAPABILITY`、`HEADER_MISMATCH` 与缺 `_meta` 的 `INVALID_PARAMS` 回 400，`METHOD_NOT_FOUND` 回 404，其余 `OK`；注释：“Legacy requests bypass this mapper and retain HTTP 200 JSON-RPC errors.” | 属实。要让 HTTP 状态是 403，必须在 rmcp 之外改写                                                    |
| `tracing::trace!(?message)` 会输出消息体                                | `tower.rs:1619`、`:1655`、`:2471` 三处                                                                                                                                                                                                                                          | 属实                                                                                                |
| 手写 `ServerHandler` 要覆盖 `get_tool`                                  | 默认实现返回 `None`（`src/handler/server.rs:633`）；HTTP 层两处调用它取 `input_schema`（`tower.rs:1741`、`:2406`）                                                                                                                                                              | 属实。r1 没有提到                                                                                   |
| 服务工厂无参，拿不到本次身份；`auth` 特性是 OAuth 客户端                | `StreamableHttpService::new(service_factory: impl Fn() -> Result<S, io::Error>, ..)`（`tower.rs:1554`）                                                                                                                                                                         | 属实，与 r1 一致                                                                                    |

### 6.2 schemars

- rmcp 的 `server` 特性启用 `schemars`（`Cargo.toml:117-122`）；依赖声明 `[dependencies.schemars] version = "1.0"`、`features = ["chrono04"]`（`Cargo.toml:924-927`）。
- 仓库 `backend/Cargo.lock` 里有 `schemars` 三个版本：`0.8.22`、`0.9.0`、`1.2.2`，都是传递依赖；`backend/` 下没有任何 `Cargo.toml` 直接写 `schemars`（检索为空，submodule `nyanpasu-runtime` 未计）。所以工作区加 `schemars = '1'` 会与 rmcp 统一到 `1.2.2`，不会引入第四个版本。
- schemars 1 对 serde 属性的支持、与 Specta 派生的并存，本次**未做试验**，只凭对该库的了解写进设计；`alias` 与手写 `Deserialize` 尤其要对拍。列为 M0 的第三项试验。

### 6.3 仓库：暴露面、宏与存储

- **暴露面只能收窄，且只认两个名字。** 生成器对顶层 `namespace!` 要求写出 `expose`（`nyanpasu-rpc-macro/src/resource_table.rs:159-162`），对名字报错“`expose` names `http` and `local`, not `{target}`; the desktop is always exposed”（`:172-179`），对子级检查是父级的子集（`narrow`，`:204-212`、`:235-246`、`:270-272`）。现有声明：`commands/{logs,meta,profile,proxy,settings,system,traffic}/mod.rs` 是 `expose = [http, local]`，`commands/remote/mod.rs:7` 是 `expose = [local]`，若干二级模块收窄为 `[http]` 或 `[]`。
- **宏看不到命名空间。** `namespace!` 是 `macro_rules!`，展开为空（`nyanpasu-rpc/src/lib.rs:27-29`）。`#[rpc]` 在中立 handler 的函数体里生成局部的 `#[derive(Deserialize)] #[serde(rename_all = "camelCase")] struct Args { .. }`，绑定了 operation 的命令用 `type Args = <Op as …>::Request`（`nyanpasu-rpc-macro/src/unified_command.rs:647-661`）。注册项只有 `name`、`kind`、`transport_neutral`、`handler`（`:540-548`）。
- **秘密记录的版本。** `StoredRecord { version, tokens, authority }`（`nyanpasu-core/src/client/remote_access/auth_store.rs:55-61`）；加载时 `record.version != STORE_VERSION` 即报“unsupported secret record version”（`:149-156`）。`StoredRecord` 与 `StoredToken` 都没有 `deny_unknown_fields`，所以同一版本号下新增的字段会被旧版静默丢弃。r1 的“`#[serde(default)]`，不需要迁移”因此是错的：降级后旧应用会把 MCP 凭证当作通用 token。
- **`/bridge/login`。** `login_submit` 调 `login_with_token`（`nyanpasu-rpc/src/http/login.rs:101`、`:126`）；MCP 受众的 token 要在这里被拒。
- **敏感的变更。** 对 HTTP 暴露的 29 条敏感命令里，变更只有 `query_logs` 一条（`logs.app`，要求 `logs:read` 的变更）；其余都是查询。

### 6.4 两份提案在事实上的出入

| 项                                | r1                                                                                                                                          | 独立提案                              | 复核                                                                             |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------- | -------------------------------------------------------------------------------- |
| token 记录加字段是否需要升版本    | 不需要                                                                                                                                      | 需要 v2                               | 独立提案对（§6.3）                                                               |
| 受众不符的状态码                  | `403`                                                                                                                                       | 与坏凭证相同的 `401`                  | 都可行；`401` 不泄露“此 token 在别处有效”，采纳                                  |
| 无状态的构造                      | 未指明会话管理器                                                                                                                            | `NeverSessionManager::default()`      | 独立提案对（§6.1）                                                               |
| Claude Code 对 `403` 的处理       | 调研记有：“marks a remote server as needing authentication when the server responds with `401 Unauthorized` or `403 Forbidden`”（§2.2、§3） | 未提，并建议把准入拒绝改写成 HTTP 403 | 二者不矛盾，但这条事实使 HTTP 403 有副作用；单一来源（Claude Code 文档），未实测 |
| Specta 能否导出入参的 JSON Schema | 认为没有现成的导出器（registry 里没有 `specta-jsonschema`）                                                                                 | 建议用它，自己标注“未验证”            | 未验证；用户已裁定改用 `schemars`                                                |

仍未核实的条目不变（§1.6、§3 末尾），另加：`RpcInput` 标记加生成表的编译错误是否如设想。（r6 更正：内核日志查询的入参在本仓库的 `nyanpasu-observers/src/logs/model.rs`；带入参的流量查询已不在首版，留下的 `get_traffic_summary` 没有入参。）

---

## 7. r4 的复核

2026-11-21。r3 经一轮独立评审（codex gpt-6.2-astra，32 条）。下面是我对照源码复核过的引用；其余条目的引用我只核对了大意，在设计 §14 里注明。

| 评审的说法                                      | 复核                                                                                                                                                                                                                                                                                                                                                                                                    | 结论                                      |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------- |
| rmcp 在 DEBUG 输出完整的请求与结果              | `rmcp-3.11.0/src/service.rs:1628`：`tracing::debug!(%id, ?request, "received request")`；`:1681`：`tracing::debug!(%id, ?result, "response message")`；`:1634`、`:1685` 的 WARN 带错误对象；`tower.rs:654-663` 的 WARN 带调用方给的协议版本头                                                                                                                                                           | 属实。r2 只过滤 TRACE 不够                |
| axum 的 `nest_service` 多注册两条路径           | `axum-0.8.9/src/routing/path_router.rs:256-279`：注册 `{prefix}/{*rest}`、`prefix` 本身，以及 `prefix` 不以 `/` 结尾时的 `{prefix}/`                                                                                                                                                                                                                                                                    | 属实。改用 `route_service`                |
| rmcp 在派生的任务里跑 handler                   | `src/service.rs:1384-1392`（非 `local` 特性下是 `tokio::spawn`）；请求上下文带 `ct: CancellationToken`（`:1275`、`:1660`）                                                                                                                                                                                                                                                                              | 属实。丢弃 HTTP future 不等于丢弃 handler |
| v2 记录会让旧版启动失败                         | `nyanpasu-core/src/client/remote_access/mod.rs:223` 在启动认证 actor 时 `load_record(..).await?`；`client/mod.rs:539-542` 把它的失败作为 `StartRemoteAuth` 传出 `NyanpasuClient` 的构造                                                                                                                                                                                                                 | 属实                                      |
| 流量查询的结果带识别信息                        | `nyanpasu-traffic/src/model.rs:60-70` 的 `Dimensions { process（“Process path or name”）, source, inbound, target（“Host, else destination IP”）}`；`commands/traffic/usage.rs` 里 `query_traffic_report`、`query_traffic_usage`、`query_traffic_usage_by_keys` 都是 `rpc(query, expose = [http])`，没有 `sensitive`，而同文件的 `query_meow_traffic_ledger` 因“can include host and app names”带了标记 | 属实。转为设计的 Q-1                      |
| 签发请求不拒绝未知字段                          | `nyanpasu-rpc/src/commands/remote/tokens.rs:13-21` 的 `CreateRemoteAccessTokenRequest` 没有 `deny_unknown_fields`                                                                                                                                                                                                                                                                                       | 属实                                      |
| `read_profile_file` 绑定了 operation            | `commands/profile/content.rs:10`：`rpc(operation = …::ReadProfileFile, query, sensitive)`                                                                                                                                                                                                                                                                                                               | 属实。设计的示例已改                      |
| 若干命令自带 `expose = [http]`                  | `commands/proxy/groups.rs:8`、`:27`、`:35`；`commands/traffic/usage.rs` 三条                                                                                                                                                                                                                                                                                                                            | 属实                                      |
| `TODO(mcp)` 在第 21–24 行；CLI 精选表现存 25 项 | `nyanpasu-cli/src/lib.rs:21-24`；`commands.rs` 里 `CuratedCommand {` 出现 27 次，含类型定义与 `impl` 各一次                                                                                                                                                                                                                                                                                             | 属实                                      |

**对 Claude Code 的 `401`/`403` 的两种说法。** §3 的调研记的是“marks a remote server as needing authentication when the server responds with `401 Unauthorized` or `403 Forbidden`”；评审指出同一页文档对配置了静态 `Authorization` 头的服务器另有说明：连接阶段的 `401`/`403` 是连接失败，不回退到 OAuth。两者我都没有再取原文核对，也都没有覆盖“连接成功之后的工具调用收到 HTTP 403”。设计的 F-11 因此改为未核实，D-11 不再以它为依据；M0 的第（4）项实测。

---

## 8. r6 的复核

2026-11-21。r5 的第二轮独立评审（19 条新增）。我对照源码复核的：

| 评审的说法                                     | 复核                                                                                                                                                                                                                            | 结论                                  |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------- |
| `get_sys_proxy`、`get_app_settings` 带内网标识 | `commands/system/proxy.rs` 的 `get_sys_proxy` 是不带标记的 `rpc(capabilities(os_integration), query)`；`nyanpasu-config/src/application/settings_view.rs:43` 的 `system_proxy_bypass`、`:70` 的 `stun_servers` 都在“安全视图”里 | 属实。按 D-12 的规则从首版去掉        |
| Origin 被当作完整 URL 解析                     | `nyanpasu-rpc/src/http/mod.rs:891-906`：`url::Url::parse(origin)` 之后检查 `origin.path() != "/"`，所以 `https://host/` 通过                                                                                                    | 属实                                  |
| 撤销先改内存再落盘，失败不回滚                 | `auth_actor.rs:500-510`：`self.tokens.remove(id)` → `announce` → `self.persist().await`，注释“a failed write is reported, not undone”                                                                                           | 属实。再次撤销同一 id 会是 `NotFound` |
| 合并基                                         | `git merge-base 9107b62bb main` = `3f73c81b1`；`… origin/main` = `eb8d01aeb`                                                                                                                                                    | 属实。r4 写的是后者却称作 `main`      |
| `clash_api_get_providers_rules` 不返回规则原文 | 未逐行读 `clash_api.rs:183-223`；评审给出的两段行号与“返回收窄的 DTO、带规则集名字与元数据”的说法我照录进设计 §5.5，标为评审的结论                                                                                              | 未亲自复核                            |

**Claude Code 的 `401`/`403`（更正 §3 与 §7）。** 评审两次指向同一页文档的两节（authenticate with remote MCP servers、use dynamic headers for custom authentication）：配置了静态 `Authorization` 头的服务器在连接阶段收到 `401`/`403` 是连接失败、不回退到 OAuth；`headersHelper` 在工具调用上另有重试。§3 表后“只在 401/403 时走 OAuth”的说法对静态请求头的配置不成立，以此为准。连接成功之后工具调用收到 HTTP 403 的行为没有文档；D-11 之下不会发生，不再安排实测。我没有再取原文，采信的是评审的引用。

其余条目（rmcp 无状态传输忽略另一个请求里的取消通知、`ERROR` 级别里的格式化错误文本、两个壳的日志过滤现状、`nyanpasu-server` 先建路由后有取消信号）我没有逐行复核，按评审给出的文件与行号写进设计，并在对应的任务卡里各有一项会失败的测试。

---

## 9. r7 的复核

2026-11-21。r6 的第三轮独立评审（10 条新增），以及用户在其后的两项裁定（每个 MCP 应答都受共享模式管；本期纳入脱敏）所需的事实。

| 说法                                       | 复核                                                                                                                                                                                                                                                                                                                                          | 结论                                                                                          |
| ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| 导入订阅时没有名字就用 URL 的最后一段      | `nyanpasu-core/src/client/mod.rs:305-319` 的 `url_derived_name`：取最后一个非空路径段（去掉 `.yaml`/`.yml`），否则主机名，否则常量                                                                                                                                                                                                            | 属实。最后一段是订阅密钥时，它成了配置名                                                      |
| 配置清单原样带出这个名字                   | `nyanpasu-config/src/profile/listing_view.rs:149-163`：`name: name.clone()`                                                                                                                                                                                                                                                                   | 属实                                                                                          |
| `Proxies` 的公开构造留着网卡与规则集的名字 | `nyanpasu-core/src/clash/proxies.rs:464-489`：`test_url: None`、`icon: None`，而 `interface`、`routing_mark`、`provider_name`、`provider` 原样                                                                                                                                                                                                | 属实                                                                                          |
| `ClashSettings` 带控制器与透明代理的配置   | `nyanpasu-config/src/clash/config/settings_view.rs:24-41`：`external_controller`、`transparent_proxy` 等字段                                                                                                                                                                                                                                  | 属实；具体哪个子字段是主机、哪个是网卡名，留给卡 C-2 逐字段确认                               |
| `GetSysProxyResponse` 的字段               | `nyanpasu-rpc/src/commands/types.rs:5-11`：`enable`、`host`、`port`、`bypass`                                                                                                                                                                                                                                                                 | 属实                                                                                          |
| 投影机制的形状                             | `ResponseVisibility { Full, Public }`（`nyanpasu-rpc-protocol/src/error.rs:11-16`）；`RpcResponseProjection { summarize_completion, encode_for_visibility }`（`nyanpasu-rpc/src/privacy.rs:112-121`），现有六个实现（`:123-205`）都是变更结果或状态的双形联合；宏在 `privacy` 时调用它（`nyanpasu-rpc-macro/src/unified_command.rs:602-607`） | 属实。普通查询的结果类型直接实现这个 trait 是否被宏接受，**未试**；列为 M0 的第（4）项        |
| 中立的文本脱敏已经存在                     | `nyanpasu-config/src/redact.rs:5` 的 `redact_config(Value) -> Value`、`:53` 的 `redact_text(&str, home) -> Cow<str>`，带测试；`nyanpasu-diagnose/src/archive.rs` 在用                                                                                                                                                                         | 属实。#5946 的正文说现有脱敏“在 `backend/tauri/src/client/runtime_inspection.rs`”，那是旧位置 |

未逐行复核、按评审的引用写进设计的：`read_profile_file` 的读取是同步的（R2）；内核日志的查询由 actor 代做、应答的接收端没了也照常执行（R2）；认证客户端的等待没有内部期限、认证 actor 在消息处理里等落盘（R4）；`ResponsePrepared` 只在 `http_call` 里显式记录（R6）；认证客户端自己持有撤销广播的发送端（R10）。它们各自对应的任务卡里有一项会失败的测试。

---

## 10. r9 的复核

2026-11-21。team-lead 自核 r8 提出的四处更正，逐条对照源码：

| 说法                           | 复核                                                                                                                                                                                                                                                                                                                                                              | 结论                                                                                                                                                             |
| ------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 凭据键的名单                   | `nyanpasu-config/src/redact.rs:9-12`（`redact_config`）与 `:42-44`（三条文本正则）各写了一遍：`secret`、`password`、`token`、`private-key`、`auth-str`、`authentication`                                                                                                                                                                                          | 属实；没有 `uuid` 等                                                                                                                                             |
| 仓库里能看到的 mihomo 认证字段 | `tauri/tests/sample_clash_config.yaml`：`uuid`（vmess `:531`、vless `:610` 起、tuic `:883`）、`psk`（snell `:375`）、`auth-str`（`:775`）、`obfs-password`（`:808`，注释里的示例）、`pre-shared-key`（wireguard `:834`、`:850`，注释里的示例）、`token`（tuic `:881`）、`private-key`（`:835`）。另有 `username`（2 处）、`public-key` 与 `short-id`（各 3 处）   | 据此补 `uuid`、`psk`、`pre-shared-key`、`obfs-password`。hysteria 的 `auth` 在样例里没有作为代理字段出现，未加。我没有对照 mihomo 上游的字段定义，只看了这份样例 |
| URL 的处理                     | `redact.rs:40`：`scheme://…@` 的用户信息换成 `<redacted>@`；`:41`：`[?&]key=` 之后的值换成 `<redacted>`。`redact_config` 只对 `external-controller` 两个键去用户信息（`:13-21`）                                                                                                                                                                                  | 属实。r8 说“订阅 URL 仍在”不准确                                                                                                                                 |
| `query_core_logs` 的结果       | `nyanpasu-observers/src/logs/model.rs:41-50`：`CoreLogRow { id, record, truncated }`、`CoreLogRecord { source, received_at, time, type（`log_type`）, payload }`；`:65` 起 `CoreLogPage { rows, … }`                                                                                                                                                              | 属实。文本在 `payload`                                                                                                                                           |
| `ResponseVisibility` 的匹配    | 非测试代码里出现 `ResponseVisibility::Full` 或 `::Public` 的文件：`nyanpasu-rpc-protocol/src/error.rs`、`nyanpasu-rpc/src/{error,privacy,redaction,registry}.rs`、`nyanpasu-rpc/src/http/routes.rs`、`tauri/src/unified_rpc/icons.rs`；`match visibility` 在 `error.rs:72`、`:85`，`privacy.rs:135`、`:194`、`:214`、`:323`，`http/routes.rs:297`、`:396`、`:509` | 列进设计 §4.3 与卡 C-1。我按文件与行号列出，没有逐个打开确认每处是不是穷尽匹配                                                                                   |

`redact_text` 是按行的规则（`redact.rs:53`）；块标量里的私钥它管不到，这一点是从规则的形状推出来的，没有跑测试。
