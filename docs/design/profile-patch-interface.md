# Profile Patch 接口分析

**本文对应设计 spec §5（`docs/superpowers/specs/2026-06-28-nyanpasu-config-profiles-type-migration-design.md`），是交付文档 #2。**

---

## 1. 问题：原始 patch 需求清单

以下需求来源于 legacy Tauri 命令层的 `ProfileBuilder`/`ProfilesBuilder` 以及原始 `nyanpasu-config` 对整体 `PrfItem`/`Profiles` 结构体派生的 struct_patch 机制。

### 1.1 元数据字段

- `name`：用户可见的 profile 名称，必须支持局部更新（只改名字、不触碰其他字段）。
- `desc`：可选描述，支持三态语义：缺席（保留原值）、`null`（清空为 `None`）、字符串值（覆写）。

### 1.2 Remote 选项字段

- `user_agent`：HTTP User-Agent，支持三态语义（缺席/`null`/字符串）。
- `with_proxy`：是否通过系统代理拉取，布尔值，缺席时保留原值。
- `self_proxy`：是否使用 Clash 自身代理拉取，布尔值，缺席时保留原值。
- `update_interval_minutes`：定时更新间隔（分钟），缺席时保留原值。
- `update_interval`（旧别名）：旧版本使用此字段名，migration 期间需处理兼容性。

### 1.3 Per-type 字段与 kind/source 切换

- Profile 类型（kind）在旧模型中以 `profile_type` 字符串表示，`Local`/`Remote`/`Script`/`Script(Lua)`/`Merge` 等，可通过 `ProfileBuilder` 切换。
- `Local → Remote`：需要同时增加 `url`、`option`、`subscription`，并去掉 `binding`。
- `Remote → Local`：需要同时去掉 `url`、`option`、`subscription`，并加回 `binding`。
- `FileConfig → CompositionConfig`：需要替换 definition 内的全部字段。
- `Overlay → Script`：切换 transform 的具体子类型。
- 这些切换在旧模型中通过对 nullable 字段的 patch 隐式实现，容易产生半改状态。

### 1.4 Chain（transforms 列表）

- 旧模型中 `chain: Vec<String>` 存储 Transform profile 的 uid 列表，支持追加、删除、重排。
- 同理 `global_transforms`（全局后处理链）和 `extend_proxies_from`（Composition proxy 贡献者列表）。

### 1.5 顶层 Profiles 状态

- `current`：当前激活的 Config profile uid，旧模型为 `Vec<String>`（允许多选），新模型为 `Option<ProfileId>`（单选）。
- `valid`：Clash 运行时保留字段列表，需支持整体替换。
- `chain`（顶层全局链）：对应新模型的 `global_transforms`。

### 1.6 整项替换

- `update_profile`：整体替换远程配置文件内容（即重新拉取）。
- `patch_profile`：整体或局部替换 profile 项的元数据或 source 信息。

### 1.7 已取消/重新归类的旧需求

- `updated` patch：旧模型通过 `ProfileBuilder.updated` 字段记录上次更新时间，可通过 patch 修改。
- `subscription` extra patch：旧模型允许直接 patch `SubscriptionInfo`（upload/download/total/expire）字段。
- `update_interval` 别名：旧配置文件中以 `update_interval` 键存储，新模型仅认 `update_interval_minutes`。

---

## 2. 结论

**新模型能提供完整的特化 patch 接口，且必须是分层/特化的，而非对整个嵌套 tagged enum 做单一 struct_patch。**

新 patch 面通过以下三类机制完整覆盖所有原始 patch 需求：

1. **Leaf struct_patch**：对纯数据叶子结构体（`ProfileMetadata`、`RemoteProfileOptions`）派生 struct_patch，支持稀疏更新与 `double_option` 三态语义。
2. **原子替换**：对 enum variant 切换（`Local↔Remote`、`FileConfig↔CompositionConfig`、`Overlay↔Script`）使用完整的 `ProfileDefinition`/`ProfileSource` 原子替换，不做 field-level patch。
3. **List-ops**：对 `transforms`/`extend_proxies_from`/`global_transforms` 等 `Vec<ProfileId>` 列表提供 `add`/`remove`/`move` 原子操作，保证去重与边界安全。

已取消的旧需求有明确的新归属，见第 5 节映射表。

---

## 3. 为何不做单一 enum patch

### 3.1 struct_patch 无法组合到 internally-tagged enum

`struct_patch` crate 的 `Patch` derive 宏要求目标类型是具名字段的结构体，并为每个字段生成 `Option<FieldType>` 形式的 patch 类型。对于 `internally-tagged enum`，如：

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileSource {
    Local { binding: LocalBinding },
    Remote {
        materialized: MaterializedFile,
        url: Url,
        option: RemoteProfileOptions,
        subscription: SubscriptionInfo,
    },
}
```

不同 variant 的字段集合是不相交的。若强行生成一个 "union patch"（将所有 variant 的字段合并为 `Option<T>` 字段），结果是一个含大量语义冲突字段的结构体，无法在类型层面区分"字段缺席"和"字段与当前 variant 不匹配"。

进一步，`ProfileDefinition` 是两层嵌套的 tagged enum（外层区分 `Config`/`Transform`，内层区分 `FileConfig`/`CompositionConfig`/`OverlayTransform`/`ScriptTransform`），flatten 成单一 patch 类型既语义不清，也会被 serde 的 `tag` 反序列化逻辑拒绝。

### 3.2 变体切换会产生非法中间态

以 `Local → Remote` 为例，旧模型允许通过分步 patch 实现：

```text
步骤 1：patch { url: "https://example.com/sub.yaml" }
步骤 2：patch { option: { with_proxy: true } }
步骤 3：patch { binding: null }  // 清除 Local binding
```

在步骤 1 完成后、步骤 3 完成前，对象处于"同时有 `url` 和 `binding`"的中间状态，而这在 Rust 类型系统中是不可表示的（`ProfileSource` 是枚举，不可能同时持有 `Local` 的 `binding` 字段和 `Remote` 的 `url` 字段）。若允许分步 patch，必须在运行时引入一个"部分构造的 profile"状态，或者用 `Option` 包裹所有字段退化为无结构的可空记录。

类似的问题出现在：

- `FileConfig → CompositionConfig`：`transforms` 语义不同，`source` 字段消失，`base`/`extend_proxies_from` 字段出现。
- `Overlay → Script`：`runtime` 字段在 Script 中存在，在 Overlay 中不存在。

设计 §15 明确规定：**枚举 variant 变化应当是原子替换，而非分步 field patch**，以确保持久化前模型始终处于合法状态。

### 3.3 类型安全边界

Rust 类型系统要求枚举值在任意时刻都处于某个合法 variant。任何"单一 enum patch"方案都必须放弃这一保证，转而在业务逻辑层做运行时检查。新模型的分层 patch 面将这一约束维持在类型层面，消除了一整类运行时错误。

---

## 4. 特化分层 patch 面

源文件：`backend/nyanpasu-config/src/profile/patch.rs`、`metadata.rs`、`source.rs`

### 4.1 Leaf struct_patch 类型

#### `ProfileMetadataPatch`

由 `ProfileMetadata` 派生（`metadata.rs`）：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, Type)))]
pub struct ProfileMetadata {
    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    pub desc: Option<String>,
}
```

生成的 `ProfileMetadataPatch` 包含 `name: Option<String>` 和 `desc: Option<Option<String>>`。`desc` 字段使用 `serde_with::rust::double_option`，实现三态语义：JSON/YAML 中缺席 → 保留原值；`null` → 清空为 `None`；字符串 → 覆写。

#### `RemoteProfileOptionsPatch`

由 `RemoteProfileOptions` 派生（`source.rs`）：

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, Serialize, Type)))]
pub struct RemoteProfileOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    pub user_agent: Option<String>,

    #[serde(default = "default_true")]
    pub with_proxy: bool,

    #[serde(default = "default_true")]
    pub self_proxy: bool,

    #[serde(default = "default_update_interval_minutes")]
    pub update_interval_minutes: u64,
}
```

生成的 `RemoteProfileOptionsPatch` 字段均为 `Option<T>`；`user_agent` 同样使用 `double_option`。

### 4.2 自由函数：List-ops（`patch.rs`）

三个通用 list 操作函数，供所有 `Vec<ProfileId>` 字段复用：

```rust
/// 追加 uid，已存在则返回 false（去重）。
pub fn list_add(list: &mut Vec<ProfileId>, uid: ProfileId) -> bool;

/// 移除第一次出现的 uid，不存在则返回 false。
pub fn list_remove(list: &mut Vec<ProfileId>, uid: &ProfileId) -> bool;

/// 将 from 位置元素移动到 to 位置，索引越界或相等时返回 false（no-op）。
pub fn list_move(list: &mut Vec<ProfileId>, from: usize, to: usize) -> bool;
```

### 4.3 `Profiles` 顶层 setter（`patch.rs`）

```rust
impl Profiles {
    pub fn set_current(&mut self, uid: Option<ProfileId>);
    pub fn clear_current(&mut self);
    pub fn set_valid(&mut self, valid: Vec<String>);
    pub fn add_global_transform(&mut self, uid: ProfileId) -> bool;
    pub fn remove_global_transform(&mut self, uid: &ProfileId) -> bool;
    pub fn move_global_transform(&mut self, from: usize, to: usize) -> bool;
}
```

### 4.4 `ProfileItem` 原子操作（`patch.rs`）

```rust
impl ProfileItem {
    /// 应用叶子元数据 patch（不触碰 definition）。
    pub fn apply_metadata_patch(&mut self, patch: ProfileMetadataPatch);

    /// 原子替换整个 definition（kind / source / binding 一次性切换）。
    pub fn set_definition(&mut self, definition: ProfileDefinition);

    /// 原地替换 source。对无 source 的 definition（CompositionConfig）返回 false。
    pub fn set_source(&mut self, source: ProfileSource) -> bool;
}
```

`set_source` 通过内部 `definition.source_mut()` 方法定位可变引用，`CompositionConfig` 不持有 source，调用时返回 `false` 而非 panic，由调用方决定是否升级为 `set_definition`。

### 4.5 `CompositionConfig` contributor 操作（`patch.rs`）

```rust
impl CompositionConfig {
    pub fn set_base(&mut self, base: Option<ProfileId>);
    pub fn add_contributor(&mut self, uid: ProfileId) -> bool;
    pub fn remove_contributor(&mut self, uid: &ProfileId) -> bool;
    pub fn move_contributor(&mut self, from: usize, to: usize) -> bool;
}
```

`add_contributor`/`remove_contributor`/`move_contributor` 内部委托给上述自由函数，操作 `self.extend_proxies_from`。

---

## 5. 原始需求 → 新接口逐条映射表

| 原始 patch 需求                                            | 新接口                                                                                                                                       | 机制                              | 说明                                                                               |
| ---------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------- | ---------------------------------------------------------------------------------- |
| `name` 更新                                                | `apply_metadata_patch(ProfileMetadataPatch { name: Some(...), .. })`                                                                         | struct_patch leaf                 | 缺席不覆写                                                                         |
| `desc` 更新/清空                                           | `apply_metadata_patch(ProfileMetadataPatch { desc: Some(Some(...)) / Some(None), .. })`                                                      | struct_patch leaf + double_option | `Some(None)` 触发清空                                                              |
| `user_agent` 更新/清空                                     | `RemoteProfileOptionsPatch { user_agent: Some(Some(...)) / Some(None), .. }` + `set_source` 中的 `option.apply(patch)`                       | struct_patch leaf + double_option | 由 Remote update task 或 Tauri 命令调用                                            |
| `with_proxy` / `self_proxy` 更新                           | `RemoteProfileOptionsPatch { with_proxy: Some(b), .. }`                                                                                      | struct_patch leaf                 | 缺席不覆写                                                                         |
| `update_interval_minutes` 更新                             | `RemoteProfileOptionsPatch { update_interval_minutes: Some(n), .. }`                                                                         | struct_patch leaf                 | 仅认 canonical 字段名                                                              |
| `update_interval`（旧别名）                                | **已删除**                                                                                                                                   | —                                 | 旧数据在 migration 中转换；新代码不再解析此别名                                    |
| `Local → Remote` 切换                                      | `item.set_definition(ProfileDefinition::Config { config: ConfigDefinition::File(FileConfig { source: ProfileSource::Remote { .. }, .. }) })` | 原子替换                          | 一次替换，不存在中间态                                                             |
| `Remote → Local` 切换                                      | 同上，替换为 `ProfileSource::Local { binding: .. }`                                                                                          | 原子替换                          |                                                                                    |
| `FileConfig → CompositionConfig` 切换                      | `item.set_definition(ProfileDefinition::Config { config: ConfigDefinition::Composition(..) })`                                               | 原子替换                          |                                                                                    |
| `Overlay → Script` 切换                                    | `item.set_definition(ProfileDefinition::Transform { transform: TransformDefinition::Script(..) })`                                           | 原子替换                          |                                                                                    |
| Remote url / source 原地替换                               | `item.set_source(ProfileSource::Remote { url, option, .. })`                                                                                 | 原子替换                          | 仅限已有 source 的 definition                                                      |
| transforms 追加                                            | `list_add(&mut file.transforms, uid)`                                                                                                        | list-op                           | FileConfig / CompositionConfig 内                                                  |
| transforms 删除                                            | `list_remove(&mut file.transforms, &uid)`                                                                                                    | list-op                           |                                                                                    |
| transforms 重排                                            | `list_move(&mut file.transforms, from, to)`                                                                                                  | list-op                           |                                                                                    |
| global_transforms 追加/删除/重排                           | `profiles.add_global_transform` / `remove_global_transform` / `move_global_transform`                                                        | list-op                           |                                                                                    |
| extend_proxies_from（contributor）追加/删除/重排           | `composition.add_contributor` / `remove_contributor` / `move_contributor`                                                                    | list-op                           |                                                                                    |
| CompositionConfig base 设置/清空                           | `composition.set_base(Some(uid))` / `composition.set_base(None)`                                                                             | 直接设值                          |                                                                                    |
| 顶层 `current` 更新/清空                                   | `profiles.set_current(Some(uid))` / `profiles.clear_current()`                                                                               | 直接设值                          | 新模型单值，旧模型为 Vec                                                           |
| 顶层 `valid` 替换                                          | `profiles.set_valid(vec![...])`                                                                                                              | 直接设值                          | 整体替换，非逐项 patch                                                             |
| `updated` / `updated_at` patch                             | **已取消**                                                                                                                                   | —                                 | `MaterializedFile.updated_at` 由 remote 拉取任务在物化成功后写入，不接受用户 patch |
| `subscription` extra patch（upload/download/total/expire） | **非用户 patch**                                                                                                                             | —                                 | `SubscriptionInfo` 由 remote 更新任务解析响应头后写入，不暴露为用户 patch 接口     |

---

## 6. 事务流程

设计 §15 确立了如下事务流程，保证任何 mutation 在持久化前都通过验证，不产生临时非法状态：

```text
1. clone 当前 Profiles
         ↓
2. 应用修改（调用上述 patch 接口）
         ↓
3. validate（Profiles::validate 检查引用完整性、materialized 路径唯一、URL scheme、interval > 0 等）
         ↓
4. 计算 scheduler / watcher diff（比较修改前后的 remote source 集合）
         ↓
5. 原子持久化（写入 profiles.yaml）
         ↓
6. 提交内存状态（替换 StateActor 持有的 Profiles 快照）
         ↓
7. reconcile runtime services（启动/停止定时更新任务，更新 file watcher）
```

### 为何能避免临时非法状态

步骤 1 对 `Profiles` 做完整 clone，使修改操作发生在副本上而非原始内存状态。步骤 2 执行的所有操作（`set_definition`/`set_source`/list-ops/setter）均是对单个字段或整个 variant 的原子替换，不存在"修改了一半"的时间窗口——Rust 所有权规则保证每一步替换要么完成要么不发生（`mem::replace` 语义）。

步骤 3 的验证在持久化之前拒绝非法状态（如 `current` 指向不存在的 uid、`extend_proxies_from` 包含 Transform uid、materialized 路径冲突等），防止脏数据落盘。

步骤 5 和步骤 6 之间如果 I/O 失败，内存状态保持旧值，不存在"写了磁盘但内存未更新"的不一致窗口。步骤 6 和步骤 7 之间的 runtime 协调失败被归类为 degraded result，不回滚已持久化的状态——这符合设计方针：后提交副作用失败应汇报为降级，而非静默回滚已写入的持久化状态。

对比旧模型的分步 patch：旧代码在每次 `patch_profile` Tauri 命令中直接修改 `Config::global()` 持有的 profile，没有 clone-validate-persist 事务，任何中途失败都会留下脏内存状态。

---

## 7. 证据：测试覆盖

以下测试文件位于 `backend/nyanpasu-config/src/profile/tests/`，证明新结构体满足原始 patch 需求。

### `tests/metadata_patch.rs`

| 测试名                           | 验证内容                                                                              |
| -------------------------------- | ------------------------------------------------------------------------------------- |
| `absent_fields_are_kept`         | 缺席字段（`desc`）在 patch 应用后保留原值，证明稀疏更新语义正确                       |
| `explicit_null_clears_desc`      | `desc: null` 将 `desc` 清空为 `None`，证明 `double_option` 三态语义有效               |
| `empty_patch_is_noop_and_sparse` | 空 patch 序列化为 `{}`（无多余字段），应用后不改变任何字段，证明 patch 类型可安全传输 |

### `tests/remote_options_patch.rs`

| 测试名                                 | 验证内容                                                                    |
| -------------------------------------- | --------------------------------------------------------------------------- |
| `applies_only_present_fields`          | 只改 `with_proxy`，`update_interval_minutes` 保留原值，证明稀疏更新         |
| `null_clears_user_agent`               | `user_agent: null` 清空为 `None`，证明 `double_option` 三态语义             |
| `legacy_update_interval_alias_is_gone` | `update_interval: 240` 不映射到 `update_interval_minutes`，证明旧别名已删除 |
| `diff_surfaces_only_changed_fields`    | `into_patch_by_diff` 只输出变更字段，证明 diff patch 可用于精确同步         |

### `tests/mutators.rs`

| 测试名                                       | 验证内容                                                                                                                                                                                                                           |
| -------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---- | ---------------------------------------------------------------------------------------- |
| `list_ops_dedup_remove_and_move`             | `list_add` 去重、`list_remove` 幂等、`list_move` 边界安全，证明三个 list-op 原语的正确性                                                                                                                                           |
| `top_level_setters_and_global_transforms`    | `set_current`/`clear_current`/`set_valid`/`add                                                                                                                                                                                     | move | remove_global_transform`均按预期工作，证明顶层 setter 覆盖旧`current`/`valid`/chain 需求 |
| `item_atomic_replacement_and_metadata_patch` | `apply_metadata_patch` 仅改元数据；`set_source` 在有 source 的 Transform 上成功返回 `true`；`set_definition` 切换到 `CompositionConfig` 后 `source()` 返回 `None`，`set_source` 返回 `false`，证明原子替换与 source 缺失的安全处理 |
| `composition_contributor_ops`                | `set_base`/`add_contributor`/`move_contributor`/`remove_contributor` 正确管理 `extend_proxies_from`，去重有效，证明 Composition 的 list-op 需求                                                                                    |

---

_本文档与 `patch.rs`（`backend/nyanpasu-config/src/profile/patch.rs`）、`metadata.rs`、`source.rs` 的实现保持一致；如实现变更，应同步更新本文档。_
