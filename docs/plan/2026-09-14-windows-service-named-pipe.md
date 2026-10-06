# Windows 服务模式 NamedPipe：主仓库实施草案

状态：代码接入已实施，准备草稿 PR；系统服务双身份验收待完成。基于 clash-nyanpasu `260e743db`，核对日期 2026-09-14。

## 目标与范围

Windows 服务模式选择 `prefer_ipc` 后，支持 NamedPipe 的内核能够通过管道承载应用的 REST 与 WebSocket 请求。内核支持权限配置时由内核创建正确的 ACL，否则由服务设置并验证权限；此能力选择已在 runtime 内实现。

主仓库负责接入已发布的 runtime、升级旧服务、验证应用完整调用链。复用现有控制通道设置和 actor/client，不增加配置项或重复实现 ACL 管理。

## 已确认的基础

- [runtime PR #424](https://github.com/libnyanpasu/nyanpasu-runtime/pull/424) 已合并；[v2.0.0-rc.8](https://github.com/libnyanpasu/nyanpasu-runtime/releases/tag/v2.0.0-rc.8) 包含合并提交，其发布提交为 `a7e44026a70894053aafca9eea5f28e2430139d6`。
- 起草期间发布资产陆续上传，最终复核已包含 Windows x86_64、ARM64 服务包及校验文件。实施时仍需实际下载并验证版本，不能仅以 release 存在认定可打包。
- `backend/Cargo.toml` 要求 runtime gitlink 指向发布 tag；`scripts/check.ts` 从该子模块的服务 manifest 读取下载版本，因此 pin、客户端依赖与服务二进制必须一致。
- `client/core_lifecycle/workflow.rs` 已将 `PreferIpc` / `HttpOnly` 映射为 `Prefer` / `Disable`，并传递保留 HTTP controller 的设置。
- `core/actor_v2/endpoint.rs` 已向服务传递控制通道设置，`core/actor_v2/api.rs` 已支持 `CoreControllerInfo::NamedPipe`。前端设置没有 Windows 平台禁用逻辑。
- `core/service/compat.rs` 当前最低服务版本为 rc.5；`core/actor_v2/service_actor.rs` 已有版本门禁和启动升级流程。

上述 Rust 模块路径均相对于 `backend/tauri/src/`。

## 实施步骤

### 1. 更新发布依赖与打包输入

- 在独立 worktree 中将 `backend/nyanpasu-runtime` 固定到 `v2.0.0-rc.8`，初始化其递归子模块。
- 更新 `backend/Cargo.lock`，保留与 runtime 更新有关的依赖变化，避免全量升级无关依赖。
- 核对所有项目支持平台的服务发布资产及下载 URL，确认 `prepare:check` 解析到 rc.8；仅在发现下载或缓存逻辑确有问题时修改脚本。
- 使用独立下载目录验证新服务，避免覆盖主 checkout 共享的 sidecar。

验证：gitlink 与 release tag 一致；锁文件包含新安全 crate；Windows x86_64/ARM64 资产可下载，二进制版本与 manifest 一致；应用 Cargo 检查通过。

### 2. 将最低服务版本提升至 rc.8

- 将 `REQUIRED_SERVICE_MIN` 更新为 `2.0.0-rc.8`，说明 Windows 管道授权所需版本。
- 复用当前统一版本门禁。采用全平台统一最低版本，代价是非 Windows 的旧 rc 服务也需要升级；这避免增加按平台、设置和内核分支的兼容规则。
- 在纯兼容性测试中覆盖 rc.5–rc.7 被拒绝、rc.8 和后续同主版本被接受，并保留 bundled version 不低于最低版本的断言。
- 扩展已有 ServiceActor fake adapter 测试，验证旧服务启动时升级一次、升级后重新探测、升级失败仍不能进入 Ready。

验证：版本判断和服务升级测试通过；旧服务不会被当作已具备新功能的服务使用；安装失败能经现有状态/错误通道反馈。

### 3. 补齐应用侧控制通道回归

- 复用 lifecycle 的 committed-config 测试，验证服务请求包含 `Prefer` / `Disable` 及正确的 `keep_http_controller` 值。
- 检查并补充服务返回 NamedPipe controller 后到应用 API client 的必要测试，覆盖 REST 和 WebSocket；优先复用现有 fake endpoint/core。
- 验证切换设置后的 endpoint 更新，以及停止状态下修改设置不会意外启动内核。
- 仅修复这些测试发现的实际接线缺口；Core Feature 由 runtime 消费，主仓库不另设能力表。仅在公开 schema 实际变化时重新生成前端绑定。

验证：控制通道、服务 actor 和 API 相关测试通过；用户看到的实际通道与正在使用的 endpoint 一致；HTTP-only 保持可用。

### 4. Windows 端到端验收与草稿 PR

| 场景                                          | 验收结果                                                     |
| --------------------------------------------- | ------------------------------------------------------------ |
| Mihomo 支持原生 SDDL，服务模式，Prefer IPC    | 实际通道为 NamedPipe，普通用户应用可读 REST 并订阅 WebSocket |
| clash-rs 无原生权限参数，服务模式，Prefer IPC | 服务完成 ACL 设置，应用可连接管道                            |
| 上述两类内核关闭 HTTP controller              | 应用仍可控制内核，HTTP controller 未监听                     |
| 内核退出后重建管道                            | 新实例重新授权，应用恢复连接                                 |
| HTTP-only、Local 模式                         | 既有控制通道行为保持正常                                     |
| 安装了 rc.7 的机器                            | 升级至 rc.8 后才能进入服务后端 Ready                         |

必须补做 LocalSystem 服务与普通用户 GUI 的双身份验收，核对非授权用户不能连接。runtime 已有真实内核测试不能代替此验证；无法执行的项目在 PR 中明确标为未验证。

运行应用相关测试、格式检查和 Clippy，随后检查平台 CI。提交范围以 runtime pin、锁文件、兼容门禁及必要应用测试为主，形成一个完整接入提交并创建草稿 PR。

## 边界与完成标准

- 服务后置 ACL 设置存在管道创建到授权完成之间的时间窗口，且无法撤销已打开的句柄；主仓库接入不改变这一限制。原生权限模式可在创建时设置权限。
- 发布资产齐备、应用检查通过、旧服务升级路径通过、两种权限模式的服务端到端验收有明确记录，才可将本接入标记完成。
- 已更新 runtime pin、锁文件和最低服务版本；补充 rc.7 启动升级及升级失败门禁测试、控制通道四种配置组合，以及服务 endpoint 经真实 NamedPipe 的 REST/WebSocket 和失效撤销测试。
- Windows x86_64 与 ARM64 发布包均已下载并通过 SHA-256 校验；x86_64 二进制报告 rc.8 和上述发布提交。全工作区格式检查及全目标、全功能 Clippy 已通过，存在既有警告。
- 应用全功能库测试中 531 项通过、1 项忽略；唯一因缺少 `pnpm.cmd` 失败的绑定导出测试，在安装 worktree 独立依赖并添加测试目录中的命令转发后重跑通过。合计 532 项通过，生成的 TypeScript 绑定无变化。
- 管道测试使用同一用户下的测试服务端与注入的 Service endpoint；真实 LocalSystem 服务、普通用户 GUI、非授权用户拒绝及两种真实内核权限模式的系统级验收仍待完成。
