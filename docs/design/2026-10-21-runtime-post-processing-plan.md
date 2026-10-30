# 运行时后处理：归属树与可观测的步骤

**日期：** 2026-10-21
**状态：** 已实施，作为 #5889 之下的叠加 PR
**Issue：** [#5791](https://github.com/libnyanpasu/clash-nyanpasu/issues/5791)，相关 PR [#5669](https://github.com/libnyanpasu/clash-nyanpasu/pull/5669)、[#5889](https://github.com/libnyanpasu/clash-nyanpasu/pull/5889)

## 1. 背景

"后处理"指 Nyanpasu 在 profile 链和全局 transform 之后、交给内核之前，对运行时配置做的全部改写，代码在 `nyanpasu-config/src/runtime/executor/`。#5889 要让 `tun.auto-route` 可托管，这又牵出了后处理中三个结构性问题。本文梳理整个后处理，作为 #5889 之下的基础设施。

### 1.1 改动前

管线尾部依次推入这些快照节点：

| 节点                          | 做什么                                                                                                                   |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `WhitelistFieldFilter`        | 第一次过滤：保留 `valid ∩ OTHERS_FIELDS` 与 `DEFAULT_FIELDS`，`HANDLE_FIELDS` 一律丢弃；过滤关闭时也推入一个无变化的节点 |
| `GuardOverrides`              | 强制写入 overrides 与端口，删除关闭的 `port` / `socks-port`                                                              |
| `BuiltinTransform` × N        | 内核兼容脚本                                                                                                             |
| `IncludeAllExpansion`（可选） | 展开 `include-all*`                                                                                                      |
| `Finalizing`                  | 一个节点内完成三件事：第二次过滤（只留 45 个已知键）、TUN 与 DNS 接管、`profile` 缓存默认值                              |

每次 `ConfigSnapshotsBuilder::push` 都要做以下几件事：

- 把父、子两棵配置树各转换一次为 JSON；
- 对两者求 diff；
- 回放 diff，检查表示是否一致；不一致时改存全量关键帧。

### 1.2 问题

1. **"谁提供这个键"的规则散在四处**：第一次过滤、guard、`apply_tun` 中的 `revise!`/`append!` 写法、`apply_cache`。
2. **字段过滤与托管设置互相否决**：设为跟随配置文件（`Unmanaged`）的字段，仍会被过滤掉。要让 profile 的值生效，需要配置两处（#5791；`unified-delay`、`tcp-concurrent` 自 #5669 起也是如此）。
3. **在巡检页分不清原因**：一个键消失或变了，只看得到一个大的 `Finalizing` 节点。
4. **内存**：
   - 每次推入节点，无论改动多小，都要分配约两棵配置树；
   - `json_patch` 删除对象键时用的是 `swap_remove`，所以过滤删掉不在末尾的键后，回放顺序不一致，节点退化为全量关键帧；如果过滤之前有脚本，这个关键帧会多保留一整棵树。

## 2. 设计

### 2.1 归属树

```rust
// nyanpasu-config/src/runtime/post_process/ownership.rs
pub enum Ownership {
    Forced(ConfigValue),   // Nyanpasu 覆盖写入
    Removed,               // Nyanpasu 删除（关闭的监听端口）
    Default(ConfigValue),  // 仅在缺失时写入
    Inherited,             // 交给 profile，且不受字段过滤影响
    Table { entries: OwnershipEntries, create: bool },
}
pub type OwnershipEntries = Vec<(&'static str, Ownership)>;
```

`#[derive(nyanpasu_macro::OwnedFields)]` 按 serde 名为设置结构体生成归属树：

| 字段                          | 生成的叶子                                 |
| ----------------------------- | ------------------------------------------ |
| 普通字段                      | `Forced`                                   |
| `ManageableField::Managed(v)` | 按 `v` 本身的类型生成，普通值即为 `Forced` |
| `ManageableField::Unmanaged`  | `Inherited`                                |
| 嵌套结构体                    | `Table { create: true }`                   |

此外，字段上的 `#[cfg]` 会原样带到生成的条目上。`ClashGuardOverrides` 用它取代了手写的 `forced_entries`，#5889 中的 `TunOverrides` 也用它。

### 2.2 计划

`executor::plan` 根据设置列出后处理会写、删、补或交给 profile 的每一个键：

- `guard_ownership`：overrides 和端口；
- `tun_ownership`：TUN 开启时，含 `tun` 与它需要的 DNS 接管；
- `cache_ownership`：`profile` 缓存。

`ownership()` 把三者拼成一棵树。`RuntimePipelineInputs` 不变，计划在每次构建时生成一次，大约 30 个叶子。

默认设置、TUN 开启、`Standard` flavor、Windows 下的计划如下：

| 路径                                                                                        | 归属                                                                                              |
| ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `log-level`、`allow-lan`、`mode`、`secret`、`ipv6`                                          | `Forced`                                                                                          |
| `unified-delay`、`tcp-concurrent`                                                           | `Forced` / `Inherited`（`ManageableField`）                                                       |
| `mixed-port`                                                                                | `Forced`                                                                                          |
| `port`、`socks-port`                                                                        | `Forced` / `Removed`                                                                              |
| `external-controller`                                                                       | 有值时 `Forced`                                                                                   |
| `tun.enable`                                                                                | `Forced(true)`。TUN 关闭时为 `Table { create: false }`：profile 写了 `tun` 就关掉它，没写就不新建 |
| `tun.auto-route`                                                                            | 本栈中为 `Default(true)`；#5889 改为由 `TunOverrides` 决定，即 `Forced` / `Inherited`             |
| `tun.stack`、`dns-hijack`、`auto-detect-interface` / `device-id`                            | `Default`                                                                                         |
| `dns.enable`                                                                                | `Forced(true)`                                                                                    |
| `dns.enhanced-mode`、`fake-ip-range`、`nameserver`、`fallback`、`fake-ip-filter`（Windows） | `Default`                                                                                         |
| `profile`                                                                                   | `Default`（整表：已有时一个键都不补）                                                             |

### 2.3 遍历函数

`runtime::post_process` 提供三个遍历函数。每个函数返回结果，以及把输入变成结果的补丁：

| 函数                                  | 处理的叶子                                          |
| ------------------------------------- | --------------------------------------------------- |
| `apply_settings`                      | `Forced`、`Removed`                                 |
| `apply_defaults`                      | `Default`                                           |
| `keep_inherited(filtered, source, …)` | `Inherited`：把过滤前存在的路径补回，需要时补建父表 |

字段过滤本身（`executor::builtin::filter_fields`）也返回补丁。几个函数共用以下约定：

- 每张被修改的映射只浅克隆一次；
- 删除用 `shift_remove`；
- 覆盖已有的键时原地替换值；
- 写入相同的值不算改动；
- 子树中没有本函数负责的叶子时，整张表跳过，因此不会建出空映射。

### 2.4 管线尾部

```text
全局 transform（用户）
├─ FieldFilter        仅过滤开启时
├─ KeepUnmanaged      仅过滤开启时
├─ ManagedSettings
├─ Defaults
├─ BuiltinTransform×N
└─ IncludeAllExpansion（可选）
   └─ CoreController  巡检时附加
```

- 四个 Nyanpasu 步骤都用 `ConfigSnapshotsBuilder::push_delta` 推入自己的补丁，不再转换整棵树、也不再求 diff。
- 所有写入都移到了内置脚本之前。四个内置脚本都不读写这些键：
  - `verge_hy_alpn` 只改 `proxies[*].alpn`；
  - `verge_meta_guard` 只改 `mode`；
  - `config_fixer` 只改 `log-level`；
  - `clash_rs_comp` 删除 `ipv6` 和 `allow_lan`，读的是 `dns.enabled` 而不是 `dns.enable`。

  `Forced` 和 `Default` 作用于同一个键时，结果与顺序无关。因此除键序外，输出不变。

- 删除第二次过滤：第一次过滤之后，留下的和 Nyanpasu 写入的都是已知键，现有内置脚本也不新增未知顶层键。为防止以后的内置脚本引入未知键，新增了一个 golden 测试：开启过滤时，用真实的内置脚本跑一遍，断言最终的顶层键全部已知。
- 执行器在 `RuntimeArtifact.final_node` 中记录最终节点，巡检改为读取这个字段，不再按 `Finalizing` 查找。快照图只存在于内存中，调整节点种类不涉及存储兼容。

### 2.5 未托管优先于字段过滤

开启过滤时，`KeepUnmanaged` 只把每条 `Inherited` 路径从过滤前的配置中补回。几点说明：

- 同一顶层键下的其他键仍按过滤处理；
- 过滤前配置中没有的路径不会凭空出现；
- `tun` 下的 `Inherited` 只在 TUN 开启时出现在计划中；
- 过滤只删除顶层键、不修改保留的值，所以补回不会与已有的值冲突。

| `ManageableField` | 过滤关闭 / 放行该键 | 过滤开启且丢弃该键                   |
| ----------------- | ------------------- | ------------------------------------ |
| `Managed(v)`      | `v`                 | `v`                                  |
| `Unmanaged`       | profile 的值        | **profile 的值**（改动前：键不存在） |

### 2.6 可观测性

节点不写日志，只靠节点种类和变化的字段说明原因：

| 现象           | 在哪个节点看到            |
| -------------- | ------------------------- |
| 被字段过滤丢掉 | `FieldFilter` 的删除      |
| 因未托管而保留 | `KeepUnmanaged` 的新增    |
| 被托管设置覆盖 | `ManagedSettings` 的 diff |
| 补上默认值     | `Defaults`                |

不写日志，是因为这些信息与 diff 重复，而日志会随巡检数据一直常驻。前端只新增了 `stepKind` 的标签，以及对应的 i18n 键。

## 3. 示例

示例 1：#5791 原样配置，字段过滤开启、`valid` 为空，内核为 meow，TUN 开启，`auto-route` 为 `Unmanaged`。profile 如下：

```yaml
tun:
  enable: true
  auto-route: global
  dns-hijack: [any:53]
```

| 节点              | 变化的字段                                                                     |
| ----------------- | ------------------------------------------------------------------------------ |
| `FieldFilter`     | `tun`（删除）                                                                  |
| `KeepUnmanaged`   | `tun.auto-route`（新增 `global`）                                              |
| `ManagedSettings` | `tun.enable`、`dns.enable` 与 guard 的键                                       |
| `Defaults`        | `tun.stack`、`tun.dns-hijack`、`tun.auto-detect-interface`、`dns.*`、`profile` |

最终结果中 `tun.auto-route: global`。profile 自带的其他 `tun` 键（如 `mtu`）仍会被过滤掉；要使用整个 `tun` 块，仍需放行 `tun`。

示例 2：`auto-route` 为 `Managed(true)`，并且 `tun` 已放行。`ManagedSettings` 的 diff 中会显示 `global → true`，用户能确定这个值是被托管设置覆盖的，而不是被过滤掉的。

示例 3：TUN 关闭。

- 过滤开启并丢弃了 `tun`：没有 `tun` 键。
- 过滤关闭：结果为 `tun: {enable: false, auto-route: global, …}`，与改动前一致。

## 4. 内存实测

### 4.1 单元级

测试中带计数分配器，debug 构建。样本为 2000 个代理加 5000 条规则，配置树 2.16 MB：

- 一次 `push` 的瞬时峰值约为 5.2 MB，与改动大小无关；`push_delta` 的瞬时峰值和常驻量都低于树的 1/10（`tests/snapshot_heap.rs`）。
- 删除中间键的节点：改动前存为 `Full`；改动后存为 `Delta`（`snapshot.rs` 测试）。
- 链路测试（profile 脚本 → 过滤 → guard → 脚本）中，改动前释放快照图时多回收了 2.16 MB，正好是一棵树。

### 4.2 dhat

`--features verge-dev,dhat-heap` 的 profiling 构建，Windows。两边使用同一份 dev 配置隔离副本：字段过滤开启，`valid` 为 `dns`、`unified-delay`、`tcp-concurrent`、`tun`、`profile`，内核为 mihomo-alpha，启用内置增强。运行 90 s 后经关机消息退出。基线为 `origin/main`（`2da8a05c4`），对照为 PR 4（`fix/keep-unmanaged-fields`）。两次运行的构建次数相同：`stage1_fields` 与 `builtin_transforms_for` 的分配块数两边一致。

| 项目                               | 基线               | 本栈               | 变化       |
| ---------------------------------- | ------------------ | ------------------ | ---------- |
| 运行时管线（`execute` 下）总分配   | 23.5 MiB / 234k 块 | 18.1 MiB / 186k 块 | −23%       |
| 其中快照 diff 编码                 | 13.8 MiB           | 8.1 MiB            | −41%       |
| 其中 `ConfigValue::to_json`        | 14.3 MiB           | 8.7 MiB            | −39%       |
| `push_delta`                       | —                  | 106 块，<0.1 MiB   |            |
| 管线在 t-gmax 时存活               | 1.55 MiB           | 1.35 MiB           | −0.20      |
| 管线在 t-end 时存活                | 0.49 MiB           | 0.29 MiB           | −0.20      |
| 其中脚本输出树（过滤节点的关键帧） | 225.8 KiB          | 0                  | 关键帧消失 |
| 全应用总分配                       | 216.2 MiB          | 209.3 MiB          | −6.9       |
| 全应用 t-gmax / t-end              | 10.67 / 6.51 MiB   | 10.52 / 6.28 MiB   |            |

说明：

- 这份 dev 配置较小，一棵树约 0.23 MiB，所以常驻量的节省只有一棵树的量。节省会随配置大小线性增长，单元级样本中为 2.16 MB。
- 剩下的 diff 编码来自 profile 链和内置脚本的推入，它们的改动无法预先知道。缩减这部分需要改变脚本接口，不在本次范围内。
- 全应用 t-end 的差值（0.23 MiB）与管线 t-end 的差值一致；其余部分的差异在噪声范围内。

## 5. 兼容性

- 存储格式不变，不需要迁移。快照归档特性 `snapshot-persistence` 未被任何 crate 启用，快照图只存在于进程内存中，回放方式改变不涉及兼容。
- 唯一改变输出值的是 `KeepUnmanaged`。#5669 已随 `v2.0.0-beta.3`、`rc.1`、`rc.2` 发布，同时满足以下条件的用户会受影响：
  1. 开启了字段过滤；
  2. 把 `unified-delay` 或 `tcp-concurrent` 设为跟随配置文件；
  3. 没有放行该键；
  4. profile 写了这个键。

  这些用户最终配置中的该键会从"不存在"变为 profile 的值，需要写进发布说明。

- 其余改动不改变值。键序可能变化，例如 `tun.auto-route` 现在位于 `enable` 之后；golden 测试按映射比较，不受影响。

## 6. PR 栈

| PR    | 分支                          | 提交                                                                                                 |
| ----- | ----------------------------- | ---------------------------------------------------------------------------------------------------- |
| 1     | `perf/snapshot-known-deltas`  | 删除时保持顺序的回放；`push_delta`                                                                   |
| 2     | `refactor/post-process-plan`  | `OwnedFields` 派生与归属树；计划与遍历函数（输出不变）                                               |
| 3     | `refactor/post-process-steps` | 拆分步骤、已知增量、删除第二次过滤、`final_node`、前端标签                                           |
| 4     | `fix/keep-unmanaged-fields`   | `KeepUnmanaged`                                                                                      |
| #5889 | `fix/5791-tun-parameters`     | 作者的 `tun_auto_route` 提交（解决冲突时按第 2.5 节调整了一个测试的预期）；`TunOverrides` 嵌套与迁移 |

## 7. 未做

- 设置页：
  - 未托管选项的说明尚未改为"不受字段过滤影响"；
  - 字段过滤卡片中，`unified-delay`、`tcp-concurrent` 的开关已不起作用，尚未禁用或加提示。
- 真机 TUN 冒烟（Windows + meow-rs）尚未进行。
