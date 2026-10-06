# 运行时快照内存优化的 dhat 对照

**日期：** 2026-10-07

**基线：** `main@5a5d16ecb`（runtime 子模块 rc.13）。改造后 = 基线 + [运行时快照内存优化 spec](../spec/2026-10-06-runtime-snapshot-memory/design.md) 的 P1–P3。

**依据：** 上述 spec 的 §5 M3 与 §6 S5；流程沿用 [Windows 内存剖析报告](2026-10-06-windows-memory-profile.md)。

**结论摘要：**

- Rust 堆 t-end 从 10.99 MiB 降到 8.23 MiB（−2.76 MiB，−25%），峰值从 14.76 MiB 降到 12.44 MiB。
- 运行时快照相关的存活分配从 3.63 MiB 降到 0.61 MiB（−83%），与 spec §2.4 的估算一致。除快照与 core logs 外，其余各类分配在两次运行中逐项相同。
- 改造后堆中最大的几项是：代理数据 1.75 MiB、core-manager 的三份配置 1.50 MiB、tauri context 与插件 1.11 MiB。

## 1. 测量条件

| 项目 | 取值                                                                                                                                                                                                                            |
| ---- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 系统 | Windows 11 Pro 10.0.26300，32 个逻辑处理器                                                                                                                                                                                      |
| 构建 | `pnpm build:profiling --features verge-dev,dhat-heap`，两次构建只差上述改动                                                                                                                                                     |
| 配置 | dev 配置在 2026-10-06T13:08Z 的迁移备份，复制到可执行文件旁的 portable 目录（`.config/PORTABLE`）。共享的 dev 配置目录已被另一个分支的构建改写为本基线无法解析的形态，所以没有使用。本地 mihomo-alpha，运行时产物 YAML 约 62 KB |
| 流程 | 启动 → 窗口打开 90 s → 向关机钩子窗口发 `WM_QUERYENDSESSION` → 等 dhat 写出（约 3 分钟，期间应用无响应）后进程以 0 退出                                                                                                         |
| 归因 | 每个分配点归到调用栈中最靠近栈顶的应用帧（`clash_nyanpasu_lib`、`nyanpasu_*`、`clash_api`），再按模块归类；没有应用帧的按最靠近栈顶的非标准库 crate 归类                                                                        |

已知差异：基线运行开始时，portable 目录已经由一次被中断的运行完成了迁移与订阅同步；改造后的运行从备份的原始状态开始，启动时多做了一次迁移。两次运行的运行时产物相同（同一个当前配置，约 62 KB）。

## 2. 总量

| 指标                | 基线               | 改造后              | 变化                    |
| ------------------- | ------------------ | ------------------- | ----------------------- |
| t-end（第 90 s）    | 10.99 MiB          | 8.23 MiB            | −2.76 MiB               |
| t-gmax              | 14.76 MiB（6.5 s） | 12.44 MiB（15.1 s） | −2.32 MiB               |
| 运行时快照（t-end） | 3.63 MiB           | 0.61 MiB            | −3.02 MiB               |
| core logs 与 redb   | 0.51 MiB           | 0.79 MiB            | +0.28 MiB，日志批次时机 |
| 其余各类            | 6.85 MiB           | 6.83 MiB            | 无变化                  |

## 3. 运行时快照的明细

| 分配点                                                   | 基线 t-end | 改造后 t-end | 内容                                                                                          |
| -------------------------------------------------------- | ---------- | ------------ | --------------------------------------------------------------------------------------------- |
| `snapshot.rs:36`（`ConfigSnapshot::clone`）              | 2.74 MiB   | —            | 展开图每个节点的 `serde_json::Value`，以及 `with_effective_config` 克隆 inspection 时的副本   |
| `client/runtime.rs:74`（`RuntimeSnapshot::clone`）       | 0.42 MiB   | —            | apply 路径克隆快照时深拷的 `config: Mapping`                                                  |
| `runtime_inspection.rs:99`（`with_effective_config`）    | 0.29 MiB   | —            | 追加为节点的 effective 配置 JSON                                                              |
| `adapters.rs`、`core_lifecycle/workflow.rs`              | 0.12 MiB   | 0.06 MiB     | 基线：产物字节与 receipt 文本各一份；改造后：effective 文本一份（`Arc<EffectiveInspection>`） |
| `value/convert.rs`（`ConfigValue`）                      | —          | 0.45 MiB     | 存储图的两个 Full 关键帧：根节点，以及一个内建脚本节点（其输出改变了键序或超过 50% 阈值）     |
| `artifact_snapshot.rs:117`                               | —          | 0.06 MiB     | 只序列化一次的正文 `config_text`，产物、intent、receipt、YAML 视图共用                        |
| `snapshot.rs` 其余（patch、`changed_fields`）与 executor | 0.01 MiB   | 0.03 MiB     |                                                                                               |

改造后，存储图中的 Delta 节点只占 patch 本身的字节；每个 Full 关键帧是一棵约 0.23 MiB 的 `ConfigValue`，正文文本 0.06 MiB。

## 4. 改造后的其余堆分配（t-end 8.23 MiB）

| 类别                   | MiB  | 主要分配点与原因                                                                                                                                                                                                           |
| ---------------------- | ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 代理数据               | 1.75 | `proxy_providers` 响应解码后的结构（`clash-api` `decode_json` 0.69 MiB，`api/proxies.rs` 反序列化 0.82 MiB）与 `core::clash::proxies` 0.12 MiB。provider 结构内含各自的代理列表，推测与 `proxies` 响应有重复（未逐项核对） |
| core-manager           | 1.50 | `config/mod.rs:145`（`prepare`）、`manager/apply.rs:186`（或 `switching.rs:419`）、`publish.rs:51` 各一份完整文档（各 0.45 MiB），即 spec §2.5 与 P4                                                                       |
| tauri context 与插件   | 1.11 | `lib.rs:270` 的 `generate_context!`：能力与权限的 `BTreeMap` 约 0.3 MiB、jiff 内置时区库的缓存 0.19 MiB；插件初始化时编译的 scope glob 正则约 0.17 MiB                                                                     |
| core logs 与 redb 存储 | 0.79 | `core/logs/redb.rs` `append_batch` 的批次缓冲（0.25–0.47 MiB，随写入时机变化）、redb 页缓存、traffic/jobs/storage 各自的 redb                                                                                              |
| 托盘菜单               | 0.69 | `core/tray/proxies.rs:244` `generate_group_selector` 为每个代理组生成的 check item（0.48 MiB），以及 muda 的加速键表与菜单项                                                                                               |
| websocket 流           | 0.61 | `core::clash::ws::run_stream` 0.26 MiB；traffic 与 memory 两个流各 128 KiB 的读缓冲（`clash-api` `client.rs:284`）                                                                                                         |
| 异步运行时与 actor     | 0.50 | `nyanpasu_utils::runtime::default_runtime`（tokio runtime 结构）0.15 MiB、ractor 处理循环 0.15 MiB、tokio 的 mpsc 块与 IO 缓冲                                                                                             |
| tao 键盘布局缓存       | 0.27 | `keyboard_layout.rs:337` `LayoutCache::prepare_layout`                                                                                                                                                                     |
| 日志 appender          | 0.16 | #5636 之后的 4096 行通道；原报告中为 3.9 MiB                                                                                                                                                                               |
| 其余                   | 0.24 | 单项均不超过 0.06 MiB                                                                                                                                                                                                      |

## 5. 后续方向

只列事实与可能的方向，本报告不改代码。

| #   | 方向                                                                    | 预计收益                              | 备注                                                    |
| --- | ----------------------------------------------------------------------- | ------------------------------------- | ------------------------------------------------------- |
| 1   | spec P4：core-manager 的 source/effective 文档与 config commit 共享一份 | 约 −0.9 MiB（三份变一份），随订阅增长 | runtime 子模块，另开 PR                                 |
| 2   | 代理缓存只保留 provider 中用到的字段，或与 `proxies` 共享代理条目       | 视重复程度而定，随节点数增长          | 需要先确认前端与托盘用到 provider 的哪些字段            |
| 3   | 托盘只为当前可见的代理组生成 check item，或在节点多时折叠               | 随组数与节点数增长                    | 属于交互取舍                                            |
| 4   | websocket 读缓冲按流量调小                                              | 约 −0.3 MiB                           | 需要确认 tungstenite 的缓冲是否可配置，以及大消息的影响 |

## 6. 复现

两次运行的 dhat 文件与分析脚本在本次会话的 scratchpad 中，没有入库。脚本按调用栈中最靠近栈顶的应用帧归因，与 2026-10-06 报告 §3 的方法相同；用它重新分析 2026-10-06 报告的 dhat 文件，得到的 16.33 / 26.33 MiB 与该报告一致。
