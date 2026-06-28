# Tauri 调用点 Profile 迁移指南

**状态：** 参考文档（供后期迁移迭代使用）  
**范围：** `backend/tauri` 中所有 Profile 相关 IPC 命令的现状分析与迁移映射  
**本期约束：** 本文档不实现任何代码改动。`backend/tauri/**` 本期零改动。

---

## 1. 目的与范围

本指南为后期将 `backend/tauri` 的 Profile 调用点迁移到 `nyanpasu_config::profile` 新类型提供逐步参考。

**本期已完成的工作：**

- `backend/nyanpasu-config/src/profile/` 已落地新领域类型（`Profiles`、`ProfileItem`、`ProfileDefinition{Config|Transform}`、`ConfigDefinition{File|Composition}`、`TransformDefinition{Overlay|Script}`、`ProfileSource{Local{binding}|Remote}`、`TransformKind`、`ProfileMetadataPatch`、`RemoteProfileOptionsPatch` 及 `patch.rs` 中的特化 mutator）。

**本指南不完成的工作（留给后期迭代）：**

- 修改 `backend/tauri` 中任何源文件；
- 在 tauri crate 中引入 `nyanpasu-config` 依赖；
- 实现新的 `NyanpasuClient` API（`get_profiles_v2`、`patch_profile` 等）；
- specta/TypeScript 绑定更新；
- 旧数据迁移的执行（由 migration V2 子系统负责）。

**参考文档：**

- 新领域设计：`docs/design/profile-composition-clean-design.md`（下文以 §N 形式引用章节号）
- 新类型实现：`docs/design/profile-composition-clean-types.rs`

---

## 2. 调用点清单

### 2.1 `backend/tauri/src/ipc.rs`

| 命令                       | 大致行号 | 职责一句话                                                                                                            |
| -------------------------- | -------- | --------------------------------------------------------------------------------------------------------------------- |
| `get_profiles`             | ~100     | 从 `NyanpasuClient` 读取整个 `Profiles` 快照并返回给前端                                                              |
| `enhance_profiles`         | ~128     | 调用 `CoreManager::global().update_config()` 重建运行配置并刷新 Clash 连接                                            |
| `import_profile`           | ~136     | 解析 URL 构造 `RemoteProfile`，追加到 `Profiles.items`；若 `current` 为空则自动激活                                   |
| `create_profile`           | ~175     | 按 `ProfileBuilder` 变体（Remote/Local/Merge/Script）构建新 Profile，可写入初始文件内容，首个 Local/Remote 时自动激活 |
| `reorder_profile`          | ~242     | 传入 `active_id`/`over_id` 拖拽重排单条 Profile                                                                       |
| `reorder_profiles_by_list` | ~250     | 按完整 uid 列表整体重排 `Profiles.items` 顺序                                                                         |
| `update_profile`           | ~258     | 调用 `feat::update_profile`：Remote 触发订阅刷新，Local/Merge/Script 更新 `updated` 时间戳                            |
| `delete_profile`           | ~265     | 删除 Profile 及其物化文件；若被删除的是 current 则重建运行配置                                                        |
| `patch_profiles_config`    | ~288     | 通过 `NyanpasuClient::patch_profiles_config(ProfilesBuilder)` 修改全局配置（current、chain 等）                       |
| `patch_profile`            | ~299     | 通过 `Profiles::patch_item(uid, ProfileBuilder)` 修改单个 Profile；判断是否影响运行配置以决定是否重建                 |
| `view_profile`             | ~345     | 读取 Profile 的 `file` 字段，拼接完整路径，调用系统工具打开                                                           |
| `read_profile_file`        | ~365     | 读取 Profile 文件内容：Local/Remote 先 YAML 规范化，Merge/Script 返回原始文本                                         |
| `save_profile_file`        | ~382     | 向 Profile 的物化文件写入新内容（仅供编辑器保存使用）                                                                 |

### 2.2 `backend/tauri/src/feat.rs`

| 函数                              | 大致行号 | 职责一句话                                                                                                                                                                                                                          |
| --------------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `feat::update_profile(uid, opts)` | ~441–470 | 区分 Remote/Local/Merge/Script：Remote 调用 `item.subscribe(opts)` 后 `replace_item`；其他类型用 `LocalProfileBuilder`/`MergeProfileBuilder`/`ScriptProfileBuilder` 构造只包含 `updated` 字段的 patch，并调用 `profiles.patch_item` |

### 2.3 `backend/tauri/src/enhance/mod.rs` 与 `enhance/chain.rs`

| 位置                                   | 大致行号        | 职责一句话                                                                                                                                                               |
| -------------------------------------- | --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `enhance::enhance()`                   | mod.rs ~22–104  | 从 `Config::profiles()` 读取 `current_mappings()` 和 scoped `profile_chain_mapping`，以及 `global_chain`；并行执行 scoped chain，合并多配置，执行 global chain，过滤字段 |
| `ChainTypeWrapper::try_from(&Profile)` | chain.rs ~59–84 | 按 `ProfileItemType` 分发：`Script(JS/Lua)` 加载文件为字符串，`Merge` 加载为 `Mapping`；其他类型返回 error                                                               |

### 2.4 `backend/tauri/src/client/mod.rs`

| 方法                                                               | 大致行号 | 职责一句话                                                                                                                        |
| ------------------------------------------------------------------ | -------- | --------------------------------------------------------------------------------------------------------------------------------- |
| `NyanpasuClient::patch_profiles_config(profiles: ProfilesBuilder)` | ~80–99   | 把 `ProfilesBuilder` 应用到 `Config::profiles()` 草稿，调用 `CoreManager::global().update_config()`；成功提交并保存，失败回滚草稿 |

---

## 3. 类型映射表

下表列出 tauri legacy 类型（`backend/tauri/src/config/profile/`）与新 `nyanpasu_config::profile` 类型的对应关系。

| 旧类型（tauri）                                                                 | 语义                                         | 新类型（nyanpasu-config）                                                                                                                                                                                                        | 说明                                                                             |
| ------------------------------------------------------------------------------- | -------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `Profile` enum {`Remote`,`Local`,`Merge`,`Script`}                              | Profile 四变体                               | `ProfileItem { uid, metadata, definition: ProfileDefinition }`                                                                                                                                                                   | 不再用枚举变体表达文件来源，改用 `definition` 二分类                             |
| `ProfileItemType` {`Remote`,`Local`,`Script(ScriptType)`,`Merge`}               | 种类枚举                                     | `ProfileDefinition::Config(ConfigDefinition::File)` 对应 Remote/Local；`ProfileDefinition::Transform(TransformDefinition::Overlay)` 对应 Merge；`ProfileDefinition::Transform(TransformDefinition::Script{runtime})` 对应 Script | 见设计 §4                                                                        |
| `ProfileBuilder` enum {`Remote(RemoteProfileBuilder)`,`Local`,`Merge`,`Script`} | 统一创建/patch DTO                           | 分层：`ProfileMetadataPatch`（name/desc）+ `RemoteProfileOptionsPatch`（url/option）+ 完整 `ProfileDefinition`/`ProfileSource` 替换                                                                                              | 设计 §15：枚举 variant 变化用原子替换；metadata/options 可细粒度 patch           |
| `ProfilesBuilder` {current, chain, valid}                                       | Profiles 全局 patch DTO                      | `Profiles` 直接通过 `StateActor` 事务 mutator 修改；无全局 builder                                                                                                                                                               | `ProfilesBuilder.current: Vec<String>` 的多值语义将改为 `Option<ProfileId>` 单值 |
| `Profiles.current: Vec<ProfileUid>`                                             | 当前激活列表，可为空可多值                   | `Profiles.current: Option<ProfileId>`                                                                                                                                                                                            | 单值；多订阅语义迁移为 `CompositionConfig`（见 §4.1 及设计 §4.3）                |
| `Profiles.chain: Vec<ProfileUid>`                                               | 全局 chain，引用 Merge/Script                | `Profiles.global_transforms: Vec<ProfileId>`                                                                                                                                                                                     | 只允许引用 `Transform`；字段重命名（见设计 §8.2）                                |
| `LocalProfile.chain` / `RemoteProfile.chain`                                    | 每 Profile 的 scoped chain                   | `ConfigDefinition::File.transforms: Vec<ProfileId>`                                                                                                                                                                              | 只允许引用 `Transform`（见设计 §8.3）                                            |
| `ProfileMetaGetter.updated(): usize`                                            | Unix epoch 时间戳（usize）                   | `MaterializedFile.updated_at: Option<OffsetDateTime>`                                                                                                                                                                            | 移入文件型 Profile 内部；`CompositionConfig` 无自己的 `updated_at`（见设计 §11） |
| `Profile.file(): &str`                                                          | 相对路径或 HTTP URL（运行时猜测）            | `ManagedProfilePath`（相对路径 newtype）；Remote URL 单独存入 `ProfileSource::Remote { url: Url, .. }`                                                                                                                           | URL/path 猜测逻辑只存在于 migration（见设计 §12、§14.2）                         |
| `MergeProfile`                                                                  | Merge 后处理类型                             | `TransformDefinition::Overlay(OverlayTransform)`                                                                                                                                                                                 | 重命名 Merge → Overlay，避免与 CompositionConfig 语义混淆（见设计 §1）           |
| `ScriptProfile` + `ScriptType { JavaScript, Lua }`                              | JS/Lua script 后处理                         | `TransformDefinition::Script { source, runtime: JavaScript \| Lua }`                                                                                                                                                             | `ScriptType` 枚举迁移为 `runtime` 字段（见设计 §14.1）                           |
| `Profiles.items: Vec<Profile>`                                                  | 有序列表（无 key，靠 uid 查找）              | `Profiles.items: IndexMap<ProfileId, ProfileItem>`                                                                                                                                                                               | 有序 map，反序列化时禁止重复 uid（见设计 §18 第 27 条）                          |
| `ProfileUid = String`                                                           | uid 字符串                                   | `ProfileId` newtype（`pub struct ProfileId(pub String)`）                                                                                                                                                                        | 类型系统区分，见 `profile-composition-clean-types.rs`                            |
| `RemoteProfile.option: RemoteProfileOptions`                                    | 订阅更新选项                                 | `ProfileSource::Remote { option: RemoteProfileOptions, .. }`                                                                                                                                                                     | 见设计 §9；字段 `update_interval` 重命名为 `update_interval_minutes`             |
| `RemoteProfile.extra: Option<RemoteProfileExtra>`                               | 订阅流量信息（upload/download/total/expire） | `ProfileSource::Remote { subscription: SubscriptionInfo, .. }`（空值经 `skip_serializing_if = "SubscriptionInfo::is_empty"` 省略）                                                                                               | `extra.expire: 0` 在 migration 中转为 `None`                                     |

---

## 4. 语义迁移要点

### 4.1 `current: Vec<ProfileUid>` → `Option<ProfileId>` + `CompositionConfig`

**现状：** `Profiles.current` 是 `Vec<ProfileUid>`，支持零值（无激活）、单值或多值。多值时 `enhance()` 调用 `current_mappings()` 并行加载所有配置并通过 `merge_profiles()` 合并，第一个 Profile 提供完整配置，后续 Profile 只贡献 `proxies`/nodes。

**新语义：** `Profiles.current: Option<ProfileId>` 只能保存单个 `ProfileId`。多订阅合并语义由一等 `CompositionConfig` 表达（设计 §4.3）：

```yaml
# 旧：current: [a, b, c]
# 新：
current: merged

items:
  - uid: merged
    type: config
    config:
      type: composition
      base: a # 提供完整配置
      extend_proxies_from: [b, c] # 只贡献 proxies
```

`CompositionConfig.base = None` 是新增能力（无完整 base，只从订阅继承节点），旧数据 migration 不自动生成（见设计 §14.3）。

迁移 mapping 规则（见设计 §14.3）：

- 旧 `current` 缺失或 `[]` → `current = None`
- 旧 `current = [a]` → `current = Some(a)`
- 旧 `current = [a, b, c]` → 新建 `CompositionConfig { base: Some(a), extend_proxies_from: [b, c] }`，`current` 指向该新 uid

### 4.2 `chain` / `chains` → `transforms` + `global_transforms`

**现状：**

- `Profiles.chain`：全局 chain，枚举时调用 `convert_uids_to_scripts` 解析成可执行脚本列表；
- `LocalProfile.chain` / `RemoteProfile.chain`：per-Profile scoped chain；
- 两者都可以引用 Merge 或 Script 类型的 Profile。

**新语义（设计 §5、§8.2、§8.3）：**

```
Profiles.chain         →  Profiles.global_transforms: Vec<ProfileId>
                           只引用 ProfileDefinition::Transform

local/remote.chain     →  ConfigDefinition::File.transforms: Vec<ProfileId>
                           只引用 ProfileDefinition::Transform
```

类型约束通过引用验证在加载和事务提交时强制执行，不再是运行时 try_from 失败的静默丢弃。

### 4.3 `Merge` → `Overlay`

**现状：** `Profile::Merge` / `ProfileItemType::Merge`，`ChainTypeWrapper::Merge(Mapping)`，通过 `ChainTypeWrapper::try_from(&Profile)` 加载。

**新语义：** `TransformDefinition::Overlay(OverlayTransform)`，文件来源用 `ProfileSource`（Local/Remote 均可），见设计 §4.2。

重命名原因：`Merge` 容易与"合并多个 Config"混淆，而 `Overlay` 明确表示"对已解析配置做声明式叠加/patch"。

### 4.4 `ProfileMeta.updated: usize` → `MaterializedFile.updated_at: Option<OffsetDateTime>`

**现状：** `updated` 是存储在 Profile shared 字段中的 usize Unix 时间戳，由 `feat::update_profile` 手动设置。`CompositionConfig`（旧 multi-current 合并配置）没有对应字段。

**新语义（设计 §11）：**

- 文件型 Profile（`Config::File`、`Transform::Overlay`、`Transform::Script`）的 `MaterializedFile.updated_at: Option<OffsetDateTime>` 由 Remote 更新器或本地同步任务维护；
- `CompositionConfig` 没有自己的 `updated_at`，运行时计算为 `max(base.updated_at?, extend_proxies_from[*].updated_at)`；
- `feat::update_profile` 中手动 `set_updated()` 的逻辑由更新器原子写入 `MaterializedFile` 时自动更新。

### 4.5 `Profile.file()` URL/path 猜测 → `ManagedProfilePath` + `Remote.url`

**现状：** `file` 字段是 `String`，可能是相对路径（如 `abc123.yaml`）或 HTTP URL。`view_profile`、`read_profile_file`、`chain.rs` 等处通过 `dirs::app_profiles_dir().join(file)` 拼接路径。旧 Local/Merge/Script 的 `file` 如果是 URL，则 `ProfileItemType` 与来源语义不一致。

**新语义（设计 §12、§14.2）：**

- `ManagedProfilePath`：强 newtype，必须是相对于应用 Profile 目录的规范相对路径，反序列化时拒绝绝对路径、`..`、`.` 和 URL；
- Remote URL 单独存储在 `ProfileSource::Remote { url: Url, .. }`；
- "旧 file 字段为 HTTP URL"的情况只在 migration 中处理：根据旧 `type` 决定新定义，将 URL 迁移到 `source.url`，并生成 `ManagedProfilePath`（见设计 §14.2）。

---

## 5. 命令逐条迁移

本节给出每个 IPC 命令的"现状签名 → 目标 `NyanpasuClient` API"草图（伪代码）。新 API 依赖 `nyanpasu-config` 类型，需要在 `NyanpasuClient` 上添加对应方法，并由 `StateActor` 或 `ProfilesActor` 持有 `Profiles` 状态。

---

### `get_profiles`

**现状（ipc.rs ~100）：**

```rust
pub fn get_profiles(client: State<'_, NyanpasuClient>) -> Result<Profiles> {
    Ok(client.get_profiles())  // 返回旧 tauri::Profiles
}
```

**目标草图：**

```rust
// NyanpasuClient 新方法
pub async fn get_profiles(&self) -> nyanpasu_config::profile::Profiles {
    self.profiles_client.get_snapshot().await
}

// ipc.rs（thin adapter）
pub async fn get_profiles(client: State<'_, NyanpasuClient>)
    -> Result<nyanpasu_config::profile::Profiles>
{
    Ok(client.get_profiles().await)
}
```

**注意：** specta/TS 类型绑定需随旧 `Profiles` → 新 `nyanpasu_config::profile::Profiles` 同步更新（见 §7）。

---

### `enhance_profiles`

**现状（ipc.rs ~128）：**

```rust
pub async fn enhance_profiles() -> Result {
    CoreManager::global().update_config().await?;
    handle::Handle::refresh_clash();
    Ok(())
}
```

**目标草图：**

```rust
pub async fn enhance_profiles(client: State<'_, NyanpasuClient>) -> Result {
    client.rebuild_running_config().await?;
    Ok(())
}
```

**迁移要点：** `enhance()` 函数本身需要基于新 `Profiles` 类型重写（读取 `current: Option<ProfileId>`，解析 `CompositionConfig`，执行 `transforms`/`global_transforms`）。`CoreManager::global()` 需迁移为 `CoreClient` actor 调用。

---

### `import_profile`

**现状（ipc.rs ~136）：**

```rust
pub async fn import_profile(
    client: State<'_, NyanpasuClient>,
    url: String,
    option: Option<RemoteProfileOptionsBuilder>,
) -> Result {
    // 构建 RemoteProfile，追加到 Profiles.items
    // 若 current 为空则设置 current
}
```

**目标草图：**

```rust
pub async fn import_profile(
    client: State<'_, NyanpasuClient>,
    url: String,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result {
    // 1. 构造新 ProfileItem：
    //    definition = Config(File(source: Remote { url, option, .. }))
    // 2. client.add_profile(item).await?
    // 3. 若 Profiles.current == None，client.activate_profile(new_uid).await?
}
```

**迁移要点：**

- 旧 `RemoteProfileOptionsBuilder` → 新 `RemoteProfileOptionsPatch`；
- `current.is_empty()` 判断 → `current == None`；
- 不再使用 `ProfilesBuilder.current(vec![uid])` 多值接口。

---

### `create_profile`

**现状（ipc.rs ~175）：**

```rust
pub async fn create_profile(
    client: State<'_, NyanpasuClient>,
    item: ProfileBuilder,     // enum: Remote/Local/Merge/Script
    file_data: Option<String>,
) -> Result { ... }
```

**目标草图：**

```rust
pub async fn create_profile(
    client: State<'_, NyanpasuClient>,
    item: NewProfileRequest,  // 新 DTO，携带 ProfileDefinition 完整描述
    file_data: Option<String>,
) -> Result {
    // 1. 根据 item 构造 ProfileItem（uid 由服务端生成）
    // 2. 若 file_data.is_some() 且为 Local/Managed，写入物化文件
    // 3. client.add_profile(profile_item).await?
    // 4. 若 definition 为 Config 且 Profiles.current == None，自动激活
}
```

**迁移要点：**

- 旧 `ProfileBuilder` 四变体 → 新 `ProfileDefinition` 完整结构；Merge → `Transform/Overlay`，Script → `Transform/Script`；
- 自动激活条件从 `profile.is_local() || profile.is_remote()` → `ProfileDefinition::Config(_)`（包含 File 和 Composition）；
- `current.is_empty()` → `current == None`。

---

### `reorder_profile`

**现状（ipc.rs ~242）：**

```rust
pub async fn reorder_profile(active_id: String, over_id: String) -> Result {
    committer.draft().reorder(active_id, over_id)?;
}
```

**目标草图：**

```rust
pub async fn reorder_profile(
    client: State<'_, NyanpasuClient>,
    active_id: ProfileId,
    over_id: ProfileId,
) -> Result {
    client.reorder_profile(active_id, over_id).await?;
}
```

**迁移要点：** `Profiles.items` 迁移为 `IndexMap`，`reorder` 操作保持稳定。`ProfileId` newtype 可在边界处直接从 `String` 解析。

---

### `reorder_profiles_by_list`

**现状（ipc.rs ~250）：**

```rust
pub fn reorder_profiles_by_list(list: Vec<String>) -> Result {
    committer.draft().reorder_by_list(&list)?;
}
```

**目标草图：**

```rust
pub async fn reorder_profiles_by_list(
    client: State<'_, NyanpasuClient>,
    list: Vec<ProfileId>,
) -> Result {
    client.reorder_profiles_by_list(list).await?;
}
```

---

### `update_profile`

**现状（ipc.rs ~258 → feat.rs ~441）：**

```rust
pub async fn update_profile(uid: String, option: Option<RemoteProfileOptionsBuilder>) -> Result {
    feat::update_profile(uid, option).await?;
}
// feat.rs: Remote → subscribe(opts); Local/Merge/Script → patch updated timestamp
```

**目标草图：**

```rust
pub async fn update_profile(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result {
    // 对 Remote 文件型 Profile：触发 Remote 更新任务（下载、校验、原子替换）
    client.refresh_profile(uid, option).await?;
    // updated_at 由更新器自动写入 MaterializedFile，无需手动 patch
}
```

**迁移要点：**

- 旧 `feat::update_profile` 中手动 `set_updated()` 的逻辑迁移到更新器；
- Local/Managed 文件修改后的时间戳更新也应由文件监听器或编辑保存触发，而非手动 patch。

---

### `delete_profile`

**现状（ipc.rs ~265）：**

```rust
pub async fn delete_profile(uid: String) -> Result {
    // 删除 item，删除文件
    // 若删除 current 则重建配置
    // 无引用保护：直接删除
}
```

**目标草图：**

```rust
pub async fn delete_profile(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result {
    // 1. 检查引用（设计 §17）：
    //    - 若 uid == Profiles.current → 拒绝，返回 ProfileInUse 错误
    //    - 若 uid 出现在任意 CompositionConfig.base / extend_proxies_from → 拒绝
    //    - 若 uid 出现在任意 transforms / global_transforms → 拒绝
    // 2. 通过后：
    //    client.delete_profile(uid).await?
    //    （删除文件 + 事务提交 + 依赖索引重建）
}
```

**迁移要点：** 当前实现无引用保护，删除 current 后会尝试自动激活第一个 Local/Remote Profile。新设计（设计 §17）改为**引用保护**：被引用的 Profile 默认拒绝删除，级联删除需要显式事务。

---

### `patch_profiles_config`

**现状（ipc.rs ~288 → client/mod.rs ~80）：**

```rust
pub async fn patch_profiles_config(
    client: State<'_, NyanpasuClient>,
    profiles: ProfilesBuilder,    // { current: Vec<String>, chain: Vec<String>, valid }
) -> Result {
    client.patch_profiles_config(profiles).await?;
}
// client: 应用 draft → update_config → apply/save
```

**目标草图：**

```rust
// 拆分为两个独立操作：
// (a) 激活配置
pub async fn activate_profile(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,   // Option<ProfileId> 若传 None 则取消激活
) -> Result {
    client.activate_profile(Some(uid)).await?;
}

// (b) 修改全局 transforms
pub async fn set_global_transforms(
    client: State<'_, NyanpasuClient>,
    transform_ids: Vec<ProfileId>,
) -> Result {
    client.set_global_transforms(transform_ids).await?;
}
```

**迁移要点：** 旧 `ProfilesBuilder.current: Vec<String>` 多值 → 新 `activate_profile(Option<ProfileId>)` 单值；旧 `ProfilesBuilder.chain` → 新 `set_global_transforms`，只允许引用 Transform，类型不匹配时验证失败。

---

### `patch_profile`

**现状（ipc.rs ~299）：**

```rust
pub async fn patch_profile(
    app_handle: AppHandle,
    uid: String,
    profile: ProfileBuilder,   // Remote/Local/Merge/Script 四变体
) -> Result {
    // committer.draft().patch_item(uid, profile)
    // 检查 uid 是否在 chain/current 中，决定是否重建配置
}
```

**目标草图（分层 patch，见设计 §15）：**

```rust
// (a) Metadata patch（name/desc）
pub async fn patch_profile_metadata(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: ProfileMetadataPatch,
) -> Result {
    client.patch_profile_metadata(uid, patch).await?;
}

// (b) Remote options patch（url/option/user_agent 等）
pub async fn patch_remote_profile_options(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    patch: RemoteProfileOptionsPatch,
) -> Result {
    client.patch_remote_profile_options(uid, patch).await?;
}

// (c) Definition/Source 整体替换（类型变更时用原子替换）
pub async fn replace_profile_definition(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    definition: ProfileDefinition,
) -> Result {
    client.replace_profile_definition(uid, definition).await?;
}
```

**迁移要点：**

- 旧 `patch_item(uid, ProfileBuilder)` 是 variant-typed patch，`Remote(RemoteProfileBuilder)` 与 `Local(LocalProfileBuilder)` 不可互换；新设计拆成三层——metadata patch、options patch、definition 原子替换，设计 §15；
- 旧 `patch_profile` 通过检查 `profiles.chain.contains(&uid) || current_chain.contains(&uid)` 判断是否重建配置；新设计由 `ProfileDependencyIndex`（设计 §16）在事务提交后自动判断并触发 core 重建。

---

### `view_profile`

**现状（ipc.rs ~345）：**

```rust
pub fn view_profile(app_handle: AppHandle, uid: String) -> Result {
    let file = Config::profiles().latest().get_item(&uid)?.file().to_string();
    let path = dirs::app_profiles_dir()?.join(file);
    help::open_file(app_handle, path)?;
}
```

**目标草图：**

```rust
pub async fn view_profile(
    app_handle: AppHandle,
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result {
    let path = client.get_profile_materialized_path(uid).await?;
    // path: 完整 CanonicalPath，由 MaterializedFile + app_profiles_dir 拼接
    help::open_file(app_handle, path)?;
}
```

**迁移要点：** `CompositionConfig` 没有物化文件，`view_profile` 对其应返回 `ProfileHasNoFile` 错误。

---

### `read_profile_file`

**现状（ipc.rs ~365）：**

```rust
pub fn read_profile_file(uid: String) -> Result<String> {
    match item.kind() {
        ProfileItemType::Local | ProfileItemType::Remote => {
            // YAML 规范化
        }
        _ => item.read_file(),   // Merge/Script 原始文本
    }
}
```

**目标草图：**

```rust
pub async fn read_profile_file(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
) -> Result<String> {
    // 对 Config::File 和 Transform（Overlay/Script）读取物化文件
    // Config::File → YAML 规范化
    // Transform::Overlay → YAML 原始文本
    // Transform::Script → 脚本原始文本
    // CompositionConfig → 返回 ProfileHasNoFile 错误
    client.read_profile_file(uid).await
}
```

---

### `save_profile_file`

**现状（ipc.rs ~382）：**

```rust
pub fn save_profile_file(uid: String, file_data: Option<String>) -> Result {
    item.save_file(file_data.unwrap())?;
}
```

**目标草图：**

```rust
pub async fn save_profile_file(
    client: State<'_, NyanpasuClient>,
    uid: ProfileId,
    file_data: String,
) -> Result {
    // 只允许对 Local/Managed 的文件型 Profile 写入
    // Remote 物化文件由更新器管理，不允许手动覆盖
    // External 由外部编辑器负责，应用只监听变化
    // CompositionConfig → 返回 ProfileHasNoFile 错误
    client.save_profile_file(uid, file_data).await?;
}
```

---

## 6. 旧数据迁移

旧数据迁移由 **tauri migration V2 子系统**（`PathResolver`，参见 commit `5ae2c004a`）负责执行，本期不实现。

### 6.1 迁移框架对接点

新 Profile migration step 应注册为 V2 子系统的一个有序 step：

```text
读取旧 profiles.yaml 为 serde_json::Value / serde_yaml::Value
→ 执行 schema migration（Value 层操作）
→ 反序列化为新 nyanpasu_config::profile::Profiles
→ 执行引用验证（validate）
→ 验证路径约束和 Remote 间隔
→ 原子写回 profiles.yaml
→ 重建依赖索引、调度器和文件监听器
```

**错误策略：** 任何无法安全迁移的情况应返回带 Profile uid 和字段路径的 migration error，不得把无效组合带入领域模型（设计 §14.4）。

### 6.2 旧 `type` → 新定义映射（设计 §14.1）

| 旧 `type` 字段 | 新 `ProfileDefinition` | 默认来源          |
| -------------- | ---------------------- | ----------------- |
| `local`        | `Config / File`        | `Local / Managed` |
| `remote`       | `Config / File`        | `Remote`          |
| `merge`        | `Transform / Overlay`  | `Local / Managed` |
| `script`       | `Transform / Script`   | `Local / Managed` |

**字段移动：**

```text
旧顶层 chain                    →  Profiles.global_transforms
local.chain / remote.chain      →  ConfigDefinition::File.transforms
script.script_type              →  TransformDefinition::Script.runtime
旧 file 字段 + 旧 updated       →  MaterializedFile { file: ManagedProfilePath, updated_at }
remote.url / option / extra     →  ProfileSource::Remote { url, option, subscription }
```

**兼容细节：**

- `extra.expire: 0` → `subscription.expire = None`；
- `option.update_interval` → `option.update_interval_minutes`（字段重命名）；
- 旧 `file` 字段为 HTTP/HTTPS URL 时（见设计 §14.2）：按 `type` 决定新定义，将 URL 迁移到 `source.url`，根据 uid 和定义类型生成 `ManagedProfilePath`。

### 6.3 多 current 列表迁移（设计 §14.3）

```text
旧 current 缺失或 []    →  current = None
旧 current = [a]        →  current = Some(a)
旧 current = [a, b, c]  →  新建 CompositionConfig:
                             uid = <碰撞安全 uid>
                             name = "Combined Profile"
                             base = Some(a)
                             extend_proxies_from = [b, c]
                             transforms = []
                             current = Some(<新 uid>)
```

顺序必须原样保留。若 `current` 中任一成员无法迁移为直接 `FileConfig`（如本身已是 Merge/Script），migration 必须明确失败，不可静默丢弃。

### 6.4 `base = None` 语义

`CompositionConfig.base = None` 是新 schema 的新增能力，旧格式没有等价语义，migration 不自动生成无 `base` 的 `CompositionConfig`（设计 §14.3 末段）。

---

## 7. 迁移顺序与风险

### 7.1 建议迁移顺序

以下顺序以"最小增量、可独立验证"为原则：

1. **在 `backend/tauri/Cargo.toml` 中引入 `nyanpasu-config` 依赖**
   - 验证：编译通过，旧代码不受影响。

2. **实现新 `Profiles` 的 specta/serde 导出**
   - 将新 `nyanpasu_config::profile::Profiles` 注册到 specta 导出列表；
   - 验证：TS 类型文件正确生成，无编译错误。

3. **迁移 `get_profiles` 和 `read_profile_file` / `save_profile_file`（只读/只写操作）**
   - 这两组操作不修改 `current` 或激活状态，风险最低；
   - 验证：前端可正常读写 Profile 文件内容。

4. **实现 `ProfilesActor` / `StateActor` 中的 `Profiles` 状态持有与事务 API**
   - 包括 `get_snapshot`、`add_profile`、`delete_profile`（含引用保护）、`reorder_profile`、分层 patch；
   - 验证：actor 测试通过，fake adapter 注入。

5. **迁移 `create_profile`、`import_profile`、`patch_profile`（写操作）**
   - 新建 Profile 使用新 `ProfileDefinition`；
   - 验证：新 Profile 被正确持久化，specta 类型正确。

6. **迁移 `patch_profiles_config` 为 `activate_profile` + `set_global_transforms`**
   - 这是前端最多使用的操作，需要前后端同步更新；
   - 验证：激活 Profile 正确重建运行配置。

7. **迁移 `enhance()` 函数（`enhance_profiles`）以支持 `CompositionConfig`**
   - 解析 `current: Option<ProfileId>` → 判断 `Config::File` 或 `Config::Composition`；
   - `CompositionConfig` 按设计 §7.3/7.4 执行多步处理；
   - 验证：旧单 current 行为不变；新 CompositionConfig 能正确合并节点。

8. **执行旧数据 migration（V2 子系统注册新 step）**
   - 见 §6；migration 完成后旧 `profiles.yaml` 格式不再加载；
   - 验证：migration 测试覆盖 §14 所有规则（设计 §18 第 24/25/26 条）。

9. **删除 `backend/tauri/src/config/profile/` 中的旧 legacy 类型**
   - 当所有调用点均已迁移且旧 `Config::profiles()` 不再被调用时执行；
   - 验证：编译通过，`grep -r "Config::profiles()"` 无结果。

### 7.2 specta / TypeScript 绑定变化

以下 TS 类型将发生**破坏性变更**，前端需要同步更新：

| 旧 TS 类型                                                     | 新 TS 类型                                                                      | 变更描述                        |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------- |
| `Profiles.current: string[]`                                   | `Profiles.current: string \| null`                                              | 从数组到可选单值                |
| `Profiles.chain: string[]`                                     | `Profiles.global_transforms: string[]`                                          | 字段重命名                      |
| `Profiles.items: Profile[]`                                    | `Profiles.items: ProfileItem[]`（序列，uid 作为字段）                           | 内存为 IndexMap，序列化仍为数组 |
| `Profile` union `{type: "remote"\|"local"\|"merge"\|"script"}` | `ProfileItem { uid, metadata, definition: ProfileDefinition }`                  | 完全重构                        |
| `ProfileDefinition` union `{Config: ...} \| {Transform: ...}`  | 新二分结构                                                                      | 新增类型                        |
| `ProfileBuilder` union                                         | 拆分为 `ProfileMetadataPatch`、`RemoteProfileOptionsPatch`、`NewProfileRequest` | 按操作类型拆分                  |
| `ProfilesBuilder`                                              | 废弃；由 `activate_profile`/`set_global_transforms` 替代                        | 破坏性删除                      |

**迁移策略建议：** 先保留旧命令别名（如 `patch_profiles_config_v1` 对应旧行为），待前端适配完成后统一删除旧命令。

### 7.3 破坏性变更点汇总

1. **`current` 语义变更**：从 `Vec<String>` 到 `Option<String>`。前端所有读取 `current` 的地方需要适配；选中多个 Profile 的交互逻辑需要迁移为 CompositionConfig 管理界面。

2. **`chain` 字段删除**：`Profiles.chain` 消失，替换为 `global_transforms`。仅允许引用 Transform 类型，传入 Config uid 将在验证时拒绝。

3. **`patch_profile` 签名变更**：从 `ProfileBuilder` enum 到分层 patch API；枚举 variant 不匹配的 patch 不再静默忽略，改为返回错误。

4. **`delete_profile` 行为变更**：引入引用保护（设计 §17），被 `current`/`base`/`extend_proxies_from`/`transforms`/`global_transforms` 引用的 Profile 默认拒绝删除，返回明确错误而非静默成功或自动修改 current。

5. **`ProfileItemType` 枚举消失**：`read_profile_file` 中的 `match item.kind()` 分支逻辑需要改为 `match item.definition`；`ChainTypeWrapper::try_from(&Profile)` 整体需要基于新类型重写，不再使用 `ProfileItemType`。

6. **`Profiles.items` 从 `Vec` 到 `IndexMap`**：序列化格式对前端兼容，但服务端所有通过 `items.iter().find(|e| e.uid() == uid)` 的线性查找改为 `items.get(&uid)` O(1) 查找。重复 uid 不再静默保留第一项，反序列化报错（设计 §18 第 27 条）。

### 7.4 风险提示

- **`enhance()` 改写风险高**：多 Profile 合并逻辑（旧 `merge_profiles`）与新 `CompositionConfig` 处理有语义差异，需要完整测试覆盖（设计 §18 第 17–21 条）。
- **TS 绑定可能出现 specta 类型推导问题**：新 `ProfileDefinition` 是嵌套枚举，specta 2.x 对内联递归类型有限制，建议对每个枚举 variant 单独导出命名类型。
- **旧数据迁移不可逆**：migration 写回后，旧格式 `profiles.yaml` 不再被新类型接受。建议在执行 migration 前自动备份，并在 migration 失败时回滚到备份。
