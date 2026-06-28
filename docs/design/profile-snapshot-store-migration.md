# Runtime Snapshot Store 迁移思路

**状态：** 迁移说明（部分已实施，全量重做推迟）
**范围：** `backend/nyanpasu-config/src/runtime/snapshot.rs` 的现状分析、本期最小同步、词汇映射及后续路径

---

## 1. 现状耦合点

`snapshot.rs` 定义了两个核心枚举：`OperatorTag` 和 `ChainNodeKind`，它们共同标注了处理图（`ConfigSnapshotsGraph`）中每个节点的语义来源。

### 1.1 `ChainNodeKind`

```rust
pub enum ChainNodeKind {
    Scoped { parent_profile_id: Arc<str> },
    Global,
}
```

| 变体                           | 含义                                           | 携带 Profile 语义？                     |
| ------------------------------ | ---------------------------------------------- | --------------------------------------- |
| `Scoped { parent_profile_id }` | 该 chain 节点属于特定 Profile 的 scoped 处理流 | 是，`parent_profile_id` 标识宿主 Config |
| `Global`                       | 该 chain 节点属于全局处理流                    | 否（全局，不绑定单一 Profile）          |

### 1.2 `OperatorTag`

```rust
pub enum OperatorTag {
    Root { primary_profile_id: Arc<str> },
    SecondaryProcessing { profile_id: Arc<str> },
    SelectedProfilesProxiesMerge {
        primary_profile_id: Arc<str>,
        other_profiles_ids: Vec<Arc<str>>,
    },
    ChainNode {
        kind: ChainNodeKind,
        profile_id: Arc<str>,
        profile_kind: TransformKind,
    },
    BuiltinChain { name: Arc<str> },
    GuardOverrides,
    WhitelistFieldFilter,
    Finalizing,
}
```

| 变体                                                                      | 含义                                                 | 携带 Profile 语义？                                          |
| ------------------------------------------------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------ |
| `Root { primary_profile_id }`                                             | 处理图的根节点，标识主配置 Profile                   | 是，`primary_profile_id` 直接引用 Profile uid                |
| `SecondaryProcessing { profile_id }`                                      | 第二来源 Profile 的处理节点（旧多 current 语义残留） | 是，`profile_id` 直接引用 Profile uid                        |
| `SelectedProfilesProxiesMerge { primary_profile_id, other_profiles_ids }` | 将多个 Profile 的 proxies 合并到主配置               | 是，携带所有参与合并的 Profile uid 列表                      |
| `ChainNode { kind, profile_id, profile_kind }`                            | 一个 Transform 节点在处理链中的执行步骤              | 是，`profile_id` + `profile_kind` 标识具体 Transform Profile |
| `BuiltinChain { name }`                                                   | 内建处理步骤（不对应用户 Profile）                   | 否（内建逻辑，无 Profile uid）                               |
| `GuardOverrides`                                                          | 覆盖守卫节点                                         | 否                                                           |
| `WhitelistFieldFilter`                                                    | 白名单字段过滤节点                                   | 否                                                           |
| `Finalizing`                                                              | 最终化步骤                                           | 否                                                           |

携带 Profile 语义的变体（共 4 个）：`Root`、`SecondaryProcessing`、`SelectedProfilesProxiesMerge`、`ChainNode`。

---

## 2. 本期已做的最小同步

### 2.1 变更内容

Task 1 将 `ChainNode.profile_kind` 的字段类型从旧的 `ProfileItemType` 替换为新的 `TransformKind`：

```rust
// 之前（已删除的旧类型）
ChainNode {
    kind: ChainNodeKind,
    profile_id: Arc<str>,
    profile_kind: ProfileItemType,  // 旧 Merge / Script / Local / Remote 枚举
},

// 当前（本期同步结果）
ChainNode {
    kind: ChainNodeKind,
    profile_id: Arc<str>,
    profile_kind: TransformKind,    // Overlay | Script { runtime }
},
```

### 2.2 语义说明

`ChainNode` 在处理图中表示的是"执行一次 Transform 操作"。旧 `ProfileItemType` 混用了内容产物类型（`Local`、`Remote`）和后处理类型（`Merge`、`Script`），与新领域模型的二分设计（`Config` vs `Transform`）不符。

新的 `TransformKind` 只携带后处理的操作类型：

```rust
pub enum TransformKind {
    Overlay,
    Script { runtime: ScriptRuntime },
}
```

这与新模型的 `TransformDefinition { Overlay | Script }` 直接对应，表达了 `ChainNode` 的实际语义：处理链中的每个节点都是某个 `Transform Profile` 的执行实例。

### 2.3 验证测试

`snapshot.rs` 的 `#[cfg(test)] mod tests` 中新增了以下测试，验证字段类型替换后序列化往返的正确性：

```rust
#[test]
fn chain_node_tag_round_trips_with_transform_kind() {
    use crate::profile::{ScriptRuntime, TransformKind};

    let tag = OperatorTag::ChainNode {
        kind: ChainNodeKind::Global,
        profile_id: Arc::from("global-fix"),
        profile_kind: TransformKind::Script {
            runtime: ScriptRuntime::Lua,
        },
    };
    let json = serde_json::to_value(&tag).unwrap();
    let back: OperatorTag = serde_json::from_value(json).unwrap();
    assert_eq!(tag, back);
}
```

该测试覆盖了 `Lua Script` 变体在 JSON 序列化和反序列化中的往返一致性。

---

## 3. 词汇重映射

下表将旧 snapshot 标签中使用的概念映射到新领域模型的对应术语。

| 旧 `OperatorTag` / `ChainNodeKind` 语义                                      | 新模型对应概念                                                                                  | 说明                                                                                                                               |
| ---------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `ChainNode { kind: Scoped { parent_profile_id }, profile_id, profile_kind }` | `FileConfig.transforms` 中的 `Transform Profile` 执行步骤                                       | `parent_profile_id` 对应宿主 `FileConfig` 或 `CompositionConfig` 的 uid；`profile_id` 对应被执行的 `Transform` uid                 |
| `ChainNode { kind: Global, profile_id, profile_kind }`                       | `Profiles.global_transforms` 中的 `Transform Profile` 执行步骤                                  | 不绑定单一宿主 Config，跨所有 current Config 执行                                                                                  |
| `SelectedProfilesProxiesMerge { primary_profile_id, other_profiles_ids }`    | `CompositionConfig { base: Some(primary_profile_id), extend_proxies_from: other_profiles_ids }` | 旧多 current 语义；新模型用一等 `CompositionConfig` 表达，`base` 提供完整配置，`extend_proxies_from` 贡献 proxies/nodes            |
| `Root { primary_profile_id }`                                                | `Profiles.current = Some(uid)`，对应被选中的 `Config` Profile                                   | 新模型 `current` 只保存单个 `ProfileId`；`Root` 节点是整棵处理树的起点，新设计中可绑定到选中的 `FileConfig` 或 `CompositionConfig` |
| `SecondaryProcessing { profile_id }`                                         | `CompositionConfig.extend_proxies_from` 中的成员处理步骤                                        | 旧实现中第二来源 Profile 的独立处理节点；新模型通过 `CompositionConfig` 在组合层统一表达，不再需要单独的 secondary tag             |
| 旧 "chain" 概念（chain node 链条）                                           | `transforms`（scoped）和 `global_transforms`（全局）                                            | 旧 chain 将 Merge/Script 节点线性排列；新模型改用 `Vec<ProfileId>` 明确列出各 Transform Profile                                    |

---

## 4. 推迟项与全量重做思路

### 4.1 本轮推迟的内容

设计文档（`profile-composition-clean-design.md`）§3 明确列出以下内容不在本轮范围：

- `SnapshotStore` / `OperatorTag` 全面调整
- `PipelineOperation` / Pipeline IR
- `StepId` / `StepAddress` / step report
- `ConfigTarget`、`ConfigType`、`TargetSelector`
- GUI preview / diff / diagnostics
- Script 静态分析策略
- 跨 target 通用 IR 或转换器

本期仅做了 `ChainNode.profile_kind: ProfileItemType → TransformKind` 这一最小同步，使 `OperatorTag` 类型与已删除的 `ProfileItemType` 解耦，不引入任何新的 snapshot 结构。

### 4.2 全量重做思路：新分类法 + `ProfileDependencyIndex`

设计文档 §16 定义了运行时依赖索引（`ProfileDependencyIndex`，不持久化），其四个字段对应四类反向依赖关系：

```rust
pub struct ProfileDependencyIndex {
    pub composition_base_dependents:    HashMap<ProfileId, IndexSet<ProfileId>>,
    pub extend_proxies_dependents:      HashMap<ProfileId, IndexSet<ProfileId>>,
    pub transform_dependents:           HashMap<ProfileId, IndexSet<ProfileId>>,
    pub global_transform_ids:           IndexSet<ProfileId>,
}
```

这四类依赖恰好覆盖了处理图中所有需要重建的触发路径：

| 依赖类型                      | 触发条件                                      | 对应 `OperatorTag` 重建影响                                                       |
| ----------------------------- | --------------------------------------------- | --------------------------------------------------------------------------------- |
| `composition_base_dependents` | 某 `FileConfig`（base）内容更新               | 需重建所有以该 Profile 为 base 的 `CompositionConfig` 处理树                      |
| `extend_proxies_dependents`   | 某 `FileConfig`（proxies 来源）内容更新       | 需重建所有在 `extend_proxies_from` 中引用该 Profile 的 `CompositionConfig` 处理树 |
| `transform_dependents`        | 某 `Transform` Profile 内容更新               | 需重建所有在 `transforms` 中引用该 Transform 的 Config 处理树（scoped 部分）      |
| `global_transform_ids`        | `global_transforms` 中任一 Transform 内容更新 | 当前运行配置总是失效，需全量重建                                                  |

全量重做时，`OperatorTag` 的枚举结构应按照新模型语义重新设计，使每个 tag 的字段直接对应 `ProfileDependencyIndex` 中的查询键，从而让 snapshot 图中的失效判断和增量重建可以通过索引直接驱动，而不需要手工扫描图节点。

---

## 5. 持久化影响

### 5.1 当前持久化结构

`snapshot-persistence` feature 启用时，`StoredConfigSnapshotsGraph` 通过以下结构序列化和反序列化：

```rust
pub const SNAPSHOT_ARCHIVE_VERSION: u16 = 1;

pub struct SnapshotArchive {
    pub format_version: u16,
    pub crate_version: Arc<str>,
    pub graph: StoredConfigSnapshotsGraph,
}
```

编码流程：`rmp_serde`（MessagePack）序列化后经 zstd 压缩写入磁盘（`encode_archive`）；读取时先解压、再反序列化，并检查 `format_version == SNAPSHOT_ARCHIVE_VERSION`（`decode_archive`）。解压后字节数上限为 64 MiB，防止解压炸弹。

### 5.2 破坏性变更分析

`OperatorTag::ChainNode.profile_kind` 字段的类型从 `ProfileItemType` 变更为 `TransformKind`，是对序列化 wire format 的**破坏性变更**：

- 旧 `ProfileItemType` 包含 `Local`、`Remote`、`Merge`、`Script` 等变体（及其对应的 JSON tag/content 形式）；
- 新 `TransformKind` 的变体为 `Overlay`（对应旧 `Merge`）和 `Script { runtime }`；
- 已持久化的 `SnapshotArchive` 若包含旧 `ChainNode` 节点，`decode_archive` 在反序列化 `OperatorTag` 时将失败，因为 `Local` / `Remote` 等旧 tag 在新类型中不存在。

### 5.3 版本处理策略建议

有两种处理方式，推荐方式 A：

**方式 A（推荐）：丢弃旧缓存，不升级 `SNAPSHOT_ARCHIVE_VERSION`**

snapshot 文件是处理缓存，不是用户数据。旧缓存失效时应安静丢弃并重建，无需迁移。在应用启动时若 `decode_archive` 返回 `UnsupportedVersion` 或反序列化错误，直接删除缓存文件并触发全量重建。这是最简单安全的策略。

**方式 B（备选）：bump `SNAPSHOT_ARCHIVE_VERSION`**

将常量从 `1` 升到 `2`，并在 `decode_archive` 检测到版本不匹配时返回 `SnapshotPersistError::UnsupportedVersion`，由调用方触发重建。当未来 snapshot 格式稳定后，若需要支持版本升级迁移路径，可采用此方式。

本期不实现上述任何版本策略。当前 `SNAPSHOT_ARCHIVE_VERSION` 保持为 `1`，持久化 feature 默认不启用。

---

## 6. 后续工作清单

以下步骤将全量 snapshot 重做拆分为可独立跟踪的工作项：

1. **重新设计 `OperatorTag` 枚举**
   将 `Root`、`SecondaryProcessing`、`SelectedProfilesProxiesMerge` 替换为与新模型直接对应的变体：区分 `FileConfigRoot`、`CompositionRoot`、`ExtendProxiesStep` 等，字段类型与 `ProfileId`、`CompositionConfig` 结构对齐。

2. **重新设计 `ChainNodeKind` 或将其并入 Tag 结构**
   `ChainNodeKind::Scoped { parent_profile_id }` 可以改为直接在 Tag 变体上携带宿主 Config uid，消除额外一层枚举嵌套。

3. **引入基于 `ProfileDependencyIndex` 的失效机制**
   snapshot store 的失效逻辑应查询 `composition_base_dependents`、`extend_proxies_dependents`、`transform_dependents`、`global_transform_ids` 四个索引，精确判断哪些子树需要重建，避免全量重建。

4. **处理持久化版本**
   在 `OperatorTag` 结构调整完成后，将 `SNAPSHOT_ARCHIVE_VERSION` 从 `1` 升至 `2`，并在 `decode_archive` 遇到版本不匹配时安静丢弃旧缓存并触发重建。

5. **为新 snapshot 结构补充测试**
   按新 `OperatorTag` 变体，为 `CompositionConfig` 的 base/extend/transform 三条处理路径各编写序列化往返测试，替换现有仅覆盖旧语义的测试。

6. **清理 `SecondaryProcessing` 调用点**
   在运行时处理管线中，定位所有生成 `OperatorTag::SecondaryProcessing` 和 `OperatorTag::SelectedProfilesProxiesMerge` 的代码路径，重写为基于 `CompositionConfig` 的新构建路径。
