# Provider 缓存摘要化实施计划

**日期：** 2026-10-18

**状态：** 已实施并验证。

**基线：** `main@1648f0000`，复用 `memory-profiling` worktree 及其独立 target，在 `perf/provider-cache-summary` 实施。依据 [堆分配调查](../review/2026-10-18-heap-allocation-profile.md)：provider 分配栈常驻约 1.47 MiB；49 个 Compatible provider 含 1122 条节点记录，按名称只有 154 个节点，页面不展示这些 Compatible 列表。

## 目标和边界

`ProxiesActor` 继续一次读取 `/proxies`、`/providers/proxies` 和组列表。完整 provider 数据仅参与统一 `Proxies.nodes` 的组装，随后释放；缓存与 RPC 只保留 provider 摘要。代理组选择、节点 delay、provider 更新与健康检查继续使用现有 actor 和 API 路径。

本次不改变 runtime 子模块、不做节点 interning、不调整 websocket 或托盘，不新增缓存、全局状态或 Tauri 依赖。已有 profiling target、sidecar/resources 链接和独立前端产物可继续使用，不需重新克隆 target 或安装依赖。

## 返回契约

新增应用读取模型 `ProxyProviderSummary`，与现有 `Proxies` 同属 `core/clash/proxies.rs`，只含：

| 字段               | 含义                                            |
| ------------------ | ----------------------------------------------- |
| `name`             | provider 名称                                   |
| `type`             | provider 类型                                   |
| `vehicleType`      | HTTP、File、Inline、Compatible 或未知类型       |
| `updatedAt`        | core 返回的更新时间                             |
| `subscriptionInfo` | 原始订阅额度与有效期，保留有符号值              |
| `proxyCount`       | provider 原始节点列表长度，包含未被组引用的节点 |

缓存保留所有 provider 类型的摘要，前端继续原来的 HTTP/File 显示过滤。HTTP/File/Inline 的完整节点仍须在释放前参与 provider-only 节点补全，不能因为当前测量样本只有 Compatible 就跳过 provider 请求。

Unified RPC 操作 `clash_api_get_providers_proxies` 及 query 分类、HTTP opt-in 均保持，返回值改为按 provider 名称索引的摘要。**这是桌面与 HTTP 共同的返回结构变更**：`proxies` 改为 `proxyCount`，不再暴露节点 history、`testUrl` 和 `expectedStatus`。迁移仓库全部消费者与生成绑定，不伪造空 `proxies` 数组、不保留完整返回值的兼容缓存。HTTP 消费者需使用新摘要结构。

## 实施步骤和验收

1. **摘要与 actor 缓存**
   - 用消费式转换移动元数据并记录原始节点数，在 `Proxies::from_responses` 完成后投影，释放完整 providers。
   - fingerprint 基于统一 proxies 与摘要，继续对实际节点、节点数、订阅及更新时间变化发通知；未展示且未被引用的 provider 节点 history 不再单独触发摘要变化。
   - 验证：摘要 JSON 契约与所有 vehicle 类型；provider-only 节点补全；缓存 TTL、刷新、订阅变化和 actor 失效测试。
2. **RPC 与前端消费迁移**
   - facade、命令返回新模型，重新生成绑定；query hook 直接接收摘要并保留 HTTP/File 过滤。
   - 更新旧 provider 类型别名与测试 fixture，保留页面、dashboard 和 widget 展示行为。
   - 验证：生成绑定与查询类型、桌面/HTTP 返回摘要、provider 页面刷新与健康检查、dashboard/widget 相关浏览器测试、TypeScript 检查。
3. **内存对照与最终检查**
   - 从同一 portable 配置和数据副本分别运行基线与改造版本，主窗口打开约 90 秒后正常退出；保留 DHAT 原始数据及摘要，分别核对 provider 栈、总存活堆与峰值。
   - 验证：缓存不再持有 provider 节点列表，provider 常驻分配下降；不将临时解码量或整个代理缓存都计作可回收收益。
   - 运行相关 Rust 测试、绑定导出、Clippy、rustfmt、前端类型/测试、Prettier 与 architecture ledger；复核每个变更均对应本计划。

## 实施记录

摘要模型、actor 缓存、facade、RPC 和前端消费已迁移。完整 provider 节点在组装 `nodes` 后释放；Compatible 等类型保留摘要，页面的 HTTP/File 过滤不变。生成绑定由既有 Rust 导出测试重新生成，query operation 与 query key 不变。

已完成的功能与静态检查：

| 检查                                               | 结果                                                             |
| -------------------------------------------------- | ---------------------------------------------------------------- |
| Rust provider 相关测试                             | 16 项通过，含 provider 元数据、通知和真实 HTTP 路由              |
| `core::proxies::tests`                             | 20 项通过，含 TTL、变更/失效、健康检查及 provider-only 节点      |
| `core::clash::proxies::tests`                      | 18 项通过，含各 vehicle 类型、顺序、跨组节点和摘要 JSON 契约     |
| `specta_export::tests::export_typescript_bindings` | 通过；仅 RPC 返回类型绑定产生预期变化                            |
| 前端 provider、dashboard、widget 和 RPC transport  | 5 个文件、18 项测试通过                                          |
| 前端类型检查                                       | 9 个包及 Node、performance、tests 项目通过                       |
| Clippy                                             | 全 workspace、全部 targets/features 通过；没有本次代码新增的告警 |
| rustfmt、Prettier、architecture ledger             | 通过                                                             |

本机 pnpm 对根脚本中的目录 filter 报 `No projects matched`，因此额外执行 `pnpm -r --filter '@nyanpasu/*' typecheck`，确认各前端包实际被检查；未修改与本次无关的脚本配置。Clippy 使用 worktree 内独立的 `backend/target/provider-cache-clippy`，与 profiling 构建分开。

### 内存对照结果

使用 `pnpm build:profiling --features verge-dev,dhat-heap`，从 `backend/target/provider-cache-measurement/seed` 恢复同一份 portable 配置和数据；两次启动前均核对全部 144 个输入文件的 SHA256。基线为上述 `main` 提交，改造版本为本计划的工作区改动。主窗口打开约 90 秒后正常退出，两次退出码均为 0；DHAT 记录时长分别为 90.58 秒和 90.37 秒。

| 指标                    | 基线                      | 改造后                    | 变化                        |
| ----------------------- | ------------------------- | ------------------------- | --------------------------- |
| provider 分配栈存活字节 | 1,540,495（1.47 MiB）     | 9,832（9.60 KiB）         | 减少 1,530,663 字节，99.36% |
| 总存活堆 `eb`           | 8,487,508（8.09 MiB）     | 6,915,302（6.59 MiB）     | 减少 1,572,206 字节，18.52% |
| 峰值堆 `gb`             | 11,943,673（11.39 MiB）   | 10,687,828（10.19 MiB）   | 减少 1,255,845 字节         |
| 累计分配 `tb`           | 165,960,321（158.27 MiB） | 165,052,088（157.41 MiB） | 减少 908,233 字节           |

调用栈复核：改造后 provider 相关存活分配仅有摘要 map 容量 8,824 字节和移入摘要的名称字符串 1,008 字节，完整 provider 节点数组与 history 不再常驻。该归因按调用栈中包含 `ProxyProvider` 或 `ProviderMap` 的分配点求和，**包含新摘要自身的分配**。统一 `nodes` 中仍保留实际被使用的节点。总存活堆下降比 provider 归因多 41,543 字节，包含日志批次等运行时差异，不把这部分算作确定的 provider 收益。

运行产物 `runtime/clash-config.yaml` 与冻结输入逐字节一致，SHA256 为 `f1f77486e2ca8c17d298ef5e49e83fb32de10d7b64e6c1205f572f483fb6acaf`。完整 core 响应仍需临时解码，本次不消除这部分分配；累计量与峰值取决于启动、日志及请求时序，不能替代常驻收益。本次为单组约 90 秒 idle 对照，只覆盖 Rust 分配器可见内存，不代表 RSS、WebView2 原生堆或长期增长。

原始证据保留在 worktree 的 gitignored 目录 `backend/target/provider-cache-measurement/`，不提交私有配置与节点数据：

- `seed-manifest.json`：144 个输入文件的哈希；`seed/`：冻结输入。
- `baseline/`、`after/`：各自的 `binary.json`、`run.json`、`samples.json`、`run-console.txt`、`dhat.json`、`analysis.json` 和 `site-stacks.json`。
- `compare.py`、`comparison.json`：聚合结果，同时核对 DHAT/控制台总量、进程 ID、退出码和运行配置字节。
- 基线 DHAT SHA256：`3938ccc7116305ed08433b4773bc536d8dd655f84f561c964e7fb6ece4a573e6`；改造后：`102822cb56f48ac35f22909ca1d46e8a2204bdbc2fc843aed92ada8528b24aca`。

复算：`python backend/target/provider-cache-measurement/compare.py`。最终复核确认变更只覆盖本计划的摘要、调用链、生成绑定、消费者、测试与文档，没有引入全局服务状态或新的 Tauri 依赖。
