# nyanpasu-config Profiles 类型迁移设计

**状态:** 已批准设计,待 writing-plans
**日期:** 2026-06-28
**范围:** 在 `nyanpasu-config` 内把 profile 类型迁移到 `docs/design` 的 clean 模型,并产出三份迁移文档
**参考:** `docs/design/profile-composition-clean-design.md`、`docs/design/profile-composition-clean-types.rs`

---

## 1. 背景与现状

仓库当前存在**两套平行的 profile 类型**:

- **legacy(tauri 自有)** —— `backend/tauri/src/config/profile/**`,含其自有的 `ProfileItemType`、`Profile` 枚举、`ProfileBuilder` patch、`Profiles` 集合。`ipc.rs` / `feat.rs` / `enhance/chain.rs` 都消费它。
- **nyanpasu-config(本期改造对象)** —— `backend/nyanpasu-config/src/profile/**`,目前是 legacy wire 格式的**忠实镜像**(`ProfileSource::{Remote,Local,Merge,Script}`、`ProfileFile`、`ProfileMeta`、`RemoteProfileOptions`,带 struct_patch 派生与 compat 测试),能直接反序列化原始 `profiles.yaml`。

`docs/design/` 已给出一个**全新的、更干净的目标模型**(设计文档 + 示例 `.rs`),但**尚未在 crate 落地**。其核心:`ProfileDefinition = Config | Transform`、`ConfigDefinition = File | Composition`、`TransformDefinition = Overlay | Script`、`ProfileSource = Local{binding} | Remote{materialized}`、`current: Option<ProfileId>`、`global_transforms`、`MaterializedFile`、`Managed/ExternalProfilePath`、`validate()`、`ProfileDependencyIndex`。

### 已核实的关键事实

1. **`nyanpasu-config` 不被任何其它 crate 依赖**(只是 workspace member)。改它的类型**不会**导致 tauri 编译失败 —— tauri 用自己的 `ProfileItemType`。
2. **本期代码 blast radius 限定在 nyanpasu-config 内两处**:`src/profile/**` 与 `src/runtime/snapshot.rs`(`OperatorTag::ChainNode.profile_kind` 是 crate 内对被删 `ProfileItemType` 的唯一引用)。

---

## 2. 目标与非目标

### 目标

1. 以 `profile-composition-clean-types.rs` 为权威基线,把 `nyanpasu-config/src/profile/**` 原地迁移为 clean 模型,**含 Patch / validation / dependency index 的实现**。
2. 同步修掉 `snapshot.rs` 对 `ProfileItemType` 的引用,使 crate 编译通过且语义正确。
3. 测试**完全基于新格式**覆盖。
4. 产出三份文档:tauri 调用迁移指南、patch 接口分析、snapshot store 迁移思路。

### 非目标(本期不做)

- 不修改 `backend/tauri/**`(含其自有 `config/profile/`、`ipc.rs`、`feat.rs`、`enhance/**`)与任何 service 位置。
- 不在 nyanpasu-config 内实现旧 `profiles.yaml` → 新格式的数据迁移(归 tauri migration V2 子系统,后期实现;本期只在迁移指南里写映射规则)。
- 不保留对旧 wire 格式的兼容反序列化(设计文档明确:新类型不兼容旧格式)。
- 不做 snapshot store 的全量重做(设计文档 §3 已推迟;本期只做最小同步 + 思路文档)。

---

## 3. 范围与 blast radius

**改代码(仅 nyanpasu-config):**

- `src/profile/**` —— clean 模型替换 legacy-mirror 类型。
- `src/runtime/snapshot.rs` —— 仅修 `OperatorTag::ChainNode.profile_kind` 的类型引用。

**不改:** tauri、service、其它 crate。

**产文档(`docs/design/`):** 见 §7。

---

## 4. 类型模型落地

以 `docs/design/profile-composition-clean-types.rs` 为**权威基线**,按现有 `profile/` 目录习惯拆分落地。模块拆分粒度交给 writing-plans(无偏好)。

### 4.1 直接采纳(示例已有)

`Profiles` / `ProfileItem` / `ProfileMetadata` / `ProfileDefinition(Config|Transform)` / `ConfigDefinition(File|Composition)` / `FileConfig` / `CompositionConfig` / `TransformDefinition(Overlay|Script)` / `OverlayTransform` / `ScriptTransform` / `ProfileSource(Local{binding}|Remote)` / `LocalBinding(Managed|External)` / `ExternalMode` / `MaterializedFile` / `ManagedProfilePath` / `ExternalProfilePath` / `RemoteProfileOptions` / `SubscriptionInfo` / `ScriptRuntime` / `ProfileCategory` / `TransformKind` / `ProfileId`,以及 `validate()` / `sanitize_top_level()` / `ProfileDependencyIndex::build()` / `items_serde`(拒绝重复 uid)。

### 4.2 需新增(示例未给)

- **Patch 类型**(见 §5):`ProfileMetadataPatch`、`RemoteProfileOptionsPatch`(struct_patch 派生),及一组特化 mutator。
- **新格式测试**(见 §8)。
- 模块拆分(示意,最终由 writing-plans 定):`profiles.rs`(顶层 + validate + sanitize + dependency)、`item.rs`、`definition.rs`、`source.rs`、`path.rs`、`patch.rs`、`tests/`。

### 4.3 关键语义变化(会写进迁移文档)

| legacy                                       | clean 模型                                                                                       |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `current: Vec<ProfileId>`                    | `current: Option<ProfileId>`;多选语义改由 `CompositionConfig{base, extend_proxies_from}` 表达    |
| `chain` / `chains`                           | `FileConfig/CompositionConfig.transforms` + `Profiles.global_transforms`                         |
| `ProfileMeta.updated`(用户可见)              | `MaterializedFile.updated_at`(运行时维护,不再用户 patch)                                         |
| `Merge`                                      | `Overlay`                                                                                        |
| `ProfileItemType{Remote,Local,Script,Merge}` | 删除;改由 `ProfileCategory` + `ConfigDefinition` + `TransformKind` 表达                          |
| `ProfileFile`(URL/path 猜测)                 | `MaterializedFile.file: ManagedProfilePath` + `ProfileSource::Remote.url`;URL 识别只在 migration |

---

## 5. Patch 接口设计(= 交付文档 #2 内核)

### 5.1 结论

**能给出特化 patch 接口,且必须是分层/特化的,而非对整个嵌套 enum 做单一 struct_patch。** 新结构体能完全满足原始 patch 需求,且更安全。

理由:对 internally-tagged enum(`ProfileDefinition` / `ProfileSource`)做细粒度 struct_patch 会产生非法中间态(如 `Local→Remote` 切换途中半个 binding),且 struct_patch 无法干净地组合到 tagged enum 上。设计文档 §15 已确立此方向。

### 5.2 特化 patch 面(本期实现)

| 原始 patch 需求                                                              | 新接口                                                                 | 机制                                         |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------- | -------------------------------------------- |
| 元数据 name / desc                                                           | `ProfileMetadataPatch`                                                 | struct_patch(leaf)                           |
| Remote 选项 user_agent / with_proxy / self_proxy / update_interval_minutes   | `RemoteProfileOptionsPatch`                                            | struct_patch(leaf)                           |
| kind / source / binding 切换(Local↔Remote、File↔Composition、Overlay↔Script) | `set_definition` / `set_source`                                        | **原子替换**(非 field patch)                 |
| chain 增删改排                                                               | `transforms` / `extend_proxies_from` / `global_transforms` 的 list-ops | 按 `ProfileId` add / remove / move / replace |
| 顶层 current / valid                                                         | `set_current` / `clear_current` / `set_valid`                          | 直接设值                                     |
| 旧 `updated` patch                                                           | **取消**                                                               | updated_at 运行时维护                        |
| 旧 subscription extra patch                                                  | 由 remote 更新任务写入                                                 | 非用户 patch                                 |

### 5.3 事务流程(设计 §15)

```
clone 当前 Profiles → 应用修改 → validate → 计算 scheduler/watcher diff → 原子持久化 → 提交内存状态 → reconcile runtime
```

保证 enum 变体切换过程中不产生临时非法状态。

---

## 6. snapshot.rs 同步

**本期代码(最小):** `OperatorTag::ChainNode.profile_kind: ProfileItemType` → 改引用新分类法的 `TransformKind`(chain 节点在新模型里就是 Transform:Overlay/Script,`TransformKind` 携带 overlay vs script + runtime,正好够用)。**已确认采纳此映射。** 仅做到编译通过 + 语义正确,**不**重排其它 `OperatorTag` 变体。

`snapshot.rs` 现有测试保持绿。

---

## 7. 三份交付文档大纲(`docs/design/`)

### 7.1 `profile-tauri-migration-guide.md`(不实现,仅指南)

- tauri 真实调用点清单:`get_profiles` / `import_profile` / `create_profile` / `reorder_profile(_by_list)` / `update_profile` / `delete_profile` / `patch_profiles_config` / `patch_profile` / `view|read|save_profile_file` / `enhance_profiles`(`ipc.rs`),`feat.rs::update_profile`,`enhance/chain.rs`,`client/mod.rs`。
- 类型映射表:legacy `Profile` / `PrfItem` / `ProfileItemType` / `ProfileBuilder` / `ProfilesBuilder` → 新 `ProfileItem` / `ProfileDefinition` / 分类法 / 特化 patch。
- 重点迁移:`current: Vec`→`Option`+Composition;`chain`→transforms;`patch_item(ProfileBuilder)`→分层 patch;删除策略(设计 §17 引用保护)。
- 旧 `profiles.yaml` 数据迁移 → 指向 tauri migration V2 子系统(设计 §14 映射规则)。
- 迁移顺序与风险。

### 7.2 `profile-patch-interface.md`

- §5 的完整论证:为何不做单一 struct_patch;特化分层 patch 面;原始需求 → 新接口逐条映射;事务流程。

### 7.3 `profile-snapshot-store-migration.md`(思路,不实现)

- 现有耦合点清单:`ChainNode{profile_kind}` / `ChainNodeKind{Scoped,Global}` / `SelectedProfilesProxiesMerge` / `SecondaryProcessing` / `Root`。
- 词汇重映射:旧 chain→transforms;`Scoped`→scoped transforms;`Global`→`global_transforms`;`SelectedProfilesProxiesMerge{primary, others}`→`CompositionConfig{base, extend_proxies_from}`。
- 设计 §3 推迟的 SnapshotStore / OperatorTag / StepId 全量重做,如何由新分类法 + dependency index(设计 §16)驱动失效与重建。
- 持久化影响:snapshot 有 `format_version`,tag enum 变更是对持久化快照的破坏性变更,需版本处理策略。

---

## 8. 测试策略(完全基于新格式)

- 替换 `tests/compat.rs` → 新 wire 格式 round-trip(对照设计 §6 的 YAML 示例)。
- 重写 `tests/patch.rs` → 新 Patch 类型语义(metadata / remote options leaf patch + list-ops + 原子替换 + double_option 清空)。
- 新增 validation 测试,覆盖设计 §18 的 28 条要点(current 单值、Remote+External 不可表示、Managed/External 路径约束、物化路径唯一、Composition 引用约束、空 Composition 失败、重复 uid 失败等)。
- 新增 dependency index 测试(base / extend_proxies / transform / global 反向引用)。
- `snapshot.rs` 现有测试保持绿。

---

## 9. 交付物与落盘位置

| 交付物                | 路径                                                                                  |
| --------------------- | ------------------------------------------------------------------------------------- |
| 设计 spec(本文件)     | `docs/superpowers/specs/2026-06-28-nyanpasu-config-profiles-type-migration-design.md` |
| 文档 #1 迁移指南      | `docs/design/profile-tauri-migration-guide.md`                                        |
| 文档 #2 patch 接口    | `docs/design/profile-patch-interface.md`                                              |
| 文档 #3 snapshot 迁移 | `docs/design/profile-snapshot-store-migration.md`                                     |
| 代码                  | `nyanpasu-config/src/profile/**`、`nyanpasu-config/src/runtime/snapshot.rs`           |

---

## 10. 成功判据

1. `cargo build -p nyanpasu-config` 通过。
2. `cargo test -p nyanpasu-config` 全绿(新格式 round-trip + patch + validation + dependency + 既有 snapshot 测试)。
3. `nyanpasu-config/src/profile/**` 不再保留 legacy 类型(`ProfileSource::Merge`、`ProfileItemType`、`ProfileFile` 的 URL 猜测等)与旧 wire 兼容反序列化。
4. `snapshot.rs` 的 `ChainNode.profile_kind` 已引用 `TransformKind`,不再依赖 `ProfileItemType`。
5. 三份文档落盘,内容与已落地的真实类型一致。
6. `backend/tauri/**` 零改动。

---

## 11. 风险与开放问题

- **snapshot 持久化破坏性变更**:`OperatorTag` 变体内字段类型变化会破坏已持久化快照的兼容;文档 #3 需给出 `format_version` 策略,但本期不实现迁移。
- **specta/TS 绑定**:clean 模型的 specta `Type` 派生会改变前端 TS 绑定形态;因本期 tauri 不接线,绑定的实际消费留待 tauri 迁移期处理,但需确认 `cargo test` / specta 导出不报错。
- **migration 子系统对接**:旧数据映射规则写进文档 #1,实际实现依赖 tauri migration V2(`PathResolver`),属后期。
