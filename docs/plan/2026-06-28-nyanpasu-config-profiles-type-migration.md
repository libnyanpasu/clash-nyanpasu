# nyanpasu-config Profiles 类型迁移 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 `nyanpasu-config` 的 profile 类型原地迁移到 `docs/design` 的 clean 模型(含 Patch / validation / dependency index),同步修复 `snapshot.rs`,并产出三份迁移文档;`backend/tauri/**` 零改动。

**Architecture:** 以 `docs/design/profile-composition-clean-types.rs` 为权威基线,按职责拆分为多个聚焦的子模块替换现有 `profile/` 模块;在示例基础上补齐 struct_patch leaf patch 与特化 mutator;`snapshot.rs` 仅把 `OperatorTag::ChainNode.profile_kind` 从被删的 `ProfileItemType` 改引用为新 `TransformKind`。旧→新数据迁移本期不实现,只写入迁移指南。

**Tech Stack:** Rust(workspace edition)、serde / serde_yaml_ng、specta(`Type` 派生)、struct-patch 0.12、indexmap、time、url、thiserror。

## Global Constraints

- 仅改 `backend/nyanpasu-config/src/profile/**` 与 `backend/nyanpasu-config/src/runtime/snapshot.rs`。`backend/tauri/**`、service、其它 crate **零改动**。
- 新类型**只定义新 wire 格式**(设计文档 §6),**不**保留对旧 `profiles.yaml` 的兼容反序列化,**不**保留任何字段 alias(如旧 `update_interval` / `chains`)。
- 所有公开类型保留 `#[derive(specta::Type)]`(或 `#[specta(type = ...)]`),与现有 crate 一致。
- 不新增全局单例 / 可变 static(CLAUDE.md §7)。
- 可选字段的 patch 用 `serde_with::rust::double_option` 区分 absent-keep 与 null-clear。
- 类型体**逐字**取自 `docs/design/profile-composition-clean-types.rs`(下称"示例文件"),除本计划明确要求修改的两处(`ProfileMetadata`、`RemoteProfileOptions` 加 `Patch` 派生)外不得改动。
- 每个 task 结束 `cargo build -p nyanpasu-config` 与 `cargo test -p nyanpasu-config` 必须可编译/通过。
- 频繁提交,每个 task 至少一次提交。

---

## File Structure

新建(`backend/nyanpasu-config/src/profile/`):

- `id.rs` — `ProfileId`
- `metadata.rs` — `ProfileMetadata` + `ProfileMetadataPatch`
- `path.rs` — `ManagedProfilePath`、`ExternalProfilePath`、`ProfilePathError`、`looks_like_url`
- `source.rs` — `ProfileSource`、`LocalBinding`、`ExternalMode`、`MaterializedFile`、`RemoteProfileOptions` + `RemoteProfileOptionsPatch`、`SubscriptionInfo`、`default_true`、`default_update_interval_minutes`
- `definition.rs` — `ProfileDefinition`、`ConfigDefinition`、`FileConfig`、`CompositionConfig`、`TransformDefinition`、`OverlayTransform`、`ScriptTransform`、`ProfileCategory`、`TransformKind`、`ScriptRuntime`
- `item.rs` — `ProfileItem`
- `profiles.rs` — `Profiles`、`ProfilesSanitizeReport`、`TransformOwner`、`CompositionMemberRole`、`ProfileValidationError`、`validate_*` 辅助、`default_valid`、`items_serde`
- `dependency.rs` — `ProfileDependencyIndex`
- `patch.rs` — 特化 mutator(`list_*` 辅助 + `Profiles` / `ProfileItem` / `CompositionConfig` 方法)
- `tests/{mod,round_trip,metadata_patch,remote_options_patch,mutators,validation,dependency}.rs`

重写:`profile/mod.rs`。修改:`runtime/snapshot.rs`。

删除:`profile/builder.rs`、`profile/item/`(整目录)、`profile/tests/{compat,patch,profiles}.rs`。

文档(`docs/design/`):`profile-tauri-migration-guide.md`、`profile-patch-interface.md`、`profile-snapshot-store-migration.md`。

---

## 类型 → 文件 → 示例文件位置 映射表(Task 1 转录用)

| 类型 / 项                                                                                                 | 目标文件        | 示例文件位置(行)           |
| --------------------------------------------------------------------------------------------------------- | --------------- | -------------------------- |
| `ProfileId`(struct + Display + FromStr)                                                                   | `id.rs`         | 15–33                      |
| `ProfileMetadata`                                                                                         | `metadata.rs`   | 80–86(**需改:加 Patch**)   |
| `ManagedProfilePath`(struct+impl+Display+Deserialize)                                                     | `path.rs`       | 232–290                    |
| `ExternalProfilePath`(同上)                                                                               | `path.rs`       | 292–336                    |
| `ProfilePathError`                                                                                        | `path.rs`       | 338–354                    |
| `looks_like_url`                                                                                          | `path.rs`       | 1006–1008                  |
| `ProfileSource`(enum)+impl                                                                                | `source.rs`     | 171–192 + 518–536          |
| `LocalBinding`(enum)+impl                                                                                 | `source.rs`     | 194–208 + 538–550          |
| `ExternalMode`                                                                                            | `source.rs`     | 210–215                    |
| `MaterializedFile`                                                                                        | `source.rs`     | 217–230                    |
| `RemoteProfileOptions`                                                                                    | `source.rs`     | 356–380(**需改:加 Patch**) |
| `SubscriptionInfo`                                                                                        | `source.rs`     | 382–409                    |
| `default_true` / `default_update_interval_minutes`                                                        | `source.rs`     | 990–996                    |
| `ProfileDefinition`(enum)+impl                                                                            | `definition.rs` | 88–94 + 420–468            |
| `ConfigDefinition`(enum)+impl                                                                             | `definition.rs` | 96–105 + 470–491           |
| `FileConfig`                                                                                              | `definition.rs` | 107–114                    |
| `CompositionConfig`                                                                                       | `definition.rs` | 116–133                    |
| `TransformDefinition`(enum)+impl                                                                          | `definition.rs` | 135–144 + 493–516          |
| `OverlayTransform` / `ScriptTransform`                                                                    | `definition.rs` | 146–155                    |
| `ProfileCategory`                                                                                         | `definition.rs` | 157–162                    |
| `TransformKind`                                                                                           | `definition.rs` | 164–169                    |
| `ScriptRuntime`                                                                                           | `definition.rs` | 411–418                    |
| `ProfileItem`                                                                                             | `item.rs`       | 68–78                      |
| `Profiles`(struct+Default)+impl                                                                           | `profiles.rs`   | 35–66 + 552–700            |
| `ProfilesSanitizeReport`                                                                                  | `profiles.rs`   | 702–708                    |
| `TransformOwner` / `CompositionMemberRole`                                                                | `profiles.rs`   | 710–722                    |
| `ProfileValidationError`                                                                                  | `profiles.rs`   | 724–794                    |
| `validate_transforms` / `validate_composition_config` / `validate_composition_member` / `validate_source` | `profiles.rs`   | 862–988                    |
| `default_valid`                                                                                           | `profiles.rs`   | 998–1004                   |
| `items_serde`(mod)                                                                                        | `profiles.rs`   | 1010–1049                  |
| `ProfileDependencyIndex`+impl                                                                             | `dependency.rs` | 798–860                    |

每个文件顶部按需加 `use super::*;` 引入兄弟类型(经 `mod.rs` 的 `pub use` 重导出),再加该文件用到的外部 crate `use`(下方各 task 给出确切 header)。

---

## Task 1: 核心类型替换 + snapshot 修复 + 新格式 round-trip

把整套 clean 类型落地、删除 legacy、修复 snapshot.rs,使 `cargo test` 在新格式下转绿。本 task 是不可再分的原子类型替换(类型、snapshot、测试必须同批落地)。

**Files:**

- Create: `backend/nyanpasu-config/src/profile/{id,metadata,path,source,definition,item,profiles,dependency}.rs`
- Create: `backend/nyanpasu-config/src/profile/tests/round_trip.rs`
- Rewrite: `backend/nyanpasu-config/src/profile/mod.rs`、`backend/nyanpasu-config/src/profile/tests/mod.rs`
- Modify: `backend/nyanpasu-config/src/runtime/snapshot.rs:14`、`:82`(ChainNode 字段)及其 `#[cfg(test)] mod tests`
- Delete: `backend/nyanpasu-config/src/profile/builder.rs`、`backend/nyanpasu-config/src/profile/item/{mod,kind,remote}.rs`、`backend/nyanpasu-config/src/profile/tests/{compat,patch,profiles}.rs`

**Interfaces:**

- Produces:
  - `nyanpasu_config::profile::Profiles { current: Option<ProfileId>, global_transforms: Vec<ProfileId>, valid: Vec<String>, items: IndexMap<ProfileId, ProfileItem> }`
  - `ProfileItem { uid: ProfileId, metadata: ProfileMetadata, definition: ProfileDefinition }`
  - `ProfileDefinition::{Config { config: ConfigDefinition }, Transform { transform: TransformDefinition }}`,方法 `category()/is_config()/is_transform()/is_direct_file_config()/source()/source_mut()`
  - `ConfigDefinition::{File(FileConfig), Composition(CompositionConfig)}`,方法 `transforms()/transforms_mut()/source()`
  - `TransformDefinition::{Overlay(OverlayTransform), Script(ScriptTransform)}`,方法 `kind() -> TransformKind/source()/source_mut()`
  - `ProfileSource::{Local { binding: LocalBinding }, Remote { materialized, url, option, subscription }}`,方法 `materialized()/materialized_mut()/is_remote()`
  - `TransformKind::{Overlay, Script { runtime: ScriptRuntime }}`、`ScriptRuntime::{JavaScript, Lua}`、`ProfileCategory::{Config, Transform}`
  - `Profiles::{get_item, append_item, replace_item, remove_item_unchecked, reorder, sanitize_top_level, validate}`
  - `ProfileDependencyIndex::build(&Profiles)`
  - 全部公开类型在 `crate::profile::` 顶层可达(`mod.rs` 重导出)。

- [ ] **Step 1: 写失败测试 —— 新格式 round-trip**

创建 `backend/nyanpasu-config/src/profile/tests/round_trip.rs`:

```rust
//! New-format wire round-trip coverage for the clean profile model.
use crate::profile::*;

fn parse(yaml: &str) -> Profiles {
    serde_yaml_ng::from_str::<Profiles>(yaml)
        .unwrap_or_else(|e| panic!("profiles must deserialize, got: {e}"))
}

#[test]
fn clean_document_round_trips() {
    let yaml = r#"current: all-subscriptions
global_transforms:
  - global-fix
valid:
  - dns
items:
  - uid: subscription-a
    name: Subscription A
    type: config
    config:
      type: file
      source:
        type: remote
        file: subscription-a.yaml
        updated_at: 1720954186
        url: https://example.com/a.yaml
        option:
          with_proxy: false
          self_proxy: true
          update_interval_minutes: 120
      transforms:
        - normalize-nodes
  - uid: subscription-b
    name: Subscription B
    type: config
    config:
      type: file
      source:
        type: remote
        file: subscription-b.yaml
        url: https://example.com/b.yaml
  - uid: all-subscriptions
    name: All Subscriptions
    type: config
    config:
      type: composition
      base: subscription-a
      extend_proxies_from:
        - subscription-b
      transforms:
        - finalize-all
  - uid: normalize-nodes
    name: Normalize Nodes
    type: transform
    transform:
      type: script
      runtime: javascript
      source:
        type: local
        binding:
          type: external
          file: normalize-nodes.js
          target: /home/user/clash-scripts/normalize.js
          mode: symlink
  - uid: finalize-all
    name: Finalize All
    type: transform
    transform:
      type: overlay
      source:
        type: local
        binding:
          type: managed
          file: finalize-all.yaml
  - uid: global-fix
    name: Global Fix
    type: transform
    transform:
      type: overlay
      source:
        type: local
        binding:
          type: managed
          file: global-fix.yaml
"#;
    let profiles = parse(yaml);
    assert_eq!(profiles.current, Some(ProfileId("all-subscriptions".into())));
    assert_eq!(
        profiles.global_transforms,
        vec![ProfileId("global-fix".into())]
    );
    assert_eq!(profiles.items.len(), 6);

    let a = profiles
        .get_item(&ProfileId("subscription-a".into()))
        .unwrap();
    assert!(a.definition.is_direct_file_config());

    let comp = profiles
        .get_item(&ProfileId("all-subscriptions".into()))
        .unwrap();
    match &comp.definition {
        ProfileDefinition::Config {
            config: ConfigDefinition::Composition(c),
        } => {
            assert_eq!(c.base, Some(ProfileId("subscription-a".into())));
            assert_eq!(
                c.extend_proxies_from,
                vec![ProfileId("subscription-b".into())]
            );
        }
        other => panic!("expected composition, got {other:?}"),
    }

    profiles.validate().expect("document must validate");

    let dumped = serde_yaml_ng::to_string(&profiles).expect("serialize");
    let reparsed = parse(&dumped);
    assert_eq!(reparsed.current, profiles.current);
    assert_eq!(reparsed.items.len(), profiles.items.len());
    reparsed.validate().expect("reparsed must validate");
}
```

- [ ] **Step 2: 运行测试,确认失败**

Run: `cargo test -p nyanpasu-config clean_document_round_trips`
Expected: 编译失败(类型/路径未定义,例如 `cannot find type `Profiles``、旧 tests 引用旧类型报错)。

- [ ] **Step 3: 创建 `id.rs`**

```rust
use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use specta::Type;
```

其后**逐字**粘贴示例文件 `ProfileId` 段(15–33 行:struct 定义 + `impl fmt::Display` + `impl FromStr`)。

- [ ] **Step 4: 创建 `metadata.rs`(加 Patch)**

```rust
use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;

/// Public, user-editable profile metadata.
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

- [ ] **Step 5: 创建 `path.rs`**

```rust
use std::{
    fmt,
    path::{Component, Path},
};

use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use specta::Type;
use thiserror::Error;
```

其后逐字粘贴示例文件:`ManagedProfilePath`(232–290)、`ExternalProfilePath`(292–336)、`ProfilePathError`(338–354)、`looks_like_url`(1006–1008)。

- [ ] **Step 6: 创建 `source.rs`(`RemoteProfileOptions` 加 Patch)**

```rust
use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;
use time::OffsetDateTime;
use url::Url;

use super::*;
```

逐字粘贴:`ProfileSource`(171–192)+其 impl(518–536)、`LocalBinding`(194–208)+impl(538–550)、`ExternalMode`(210–215)、`MaterializedFile`(217–230)、`SubscriptionInfo`(382–409)、`default_true`(990–992)、`default_update_interval_minutes`(994–996)。

`RemoteProfileOptions` 用下面这段**替代**示例的 356–380(只加 `Patch` 派生与 patch 属性,字段与默认值不变):

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

impl Default for RemoteProfileOptions {
    fn default() -> Self {
        Self {
            user_agent: None,
            with_proxy: true,
            self_proxy: true,
            update_interval_minutes: default_update_interval_minutes(),
        }
    }
}
```

- [ ] **Step 7: 创建 `definition.rs`**

```rust
use serde::{Deserialize, Serialize};
use specta::Type;

use super::*;
```

逐字粘贴:`ProfileDefinition`(88–94)+impl(420–468)、`ConfigDefinition`(96–105)+impl(470–491)、`FileConfig`(107–114)、`CompositionConfig`(116–133)、`TransformDefinition`(135–144)+impl(493–516)、`OverlayTransform`/`ScriptTransform`(146–155)、`ProfileCategory`(157–162)、`TransformKind`(164–169)、`ScriptRuntime`(411–418)。

- [ ] **Step 8: 创建 `item.rs`**

```rust
use serde::{Deserialize, Serialize};
use specta::Type;

use super::*;
```

逐字粘贴示例文件 `ProfileItem`(68–78)。

- [ ] **Step 9: 创建 `profiles.rs`**

```rust
use std::collections::HashMap;

use indexmap::{map::Entry, IndexMap, IndexSet};
use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;
use thiserror::Error;

use super::*;
```

逐字粘贴:`Profiles`(35–66)+impl(552–700)、`ProfilesSanitizeReport`(702–708)、`TransformOwner`(710–715)、`CompositionMemberRole`(717–722)、`ProfileValidationError`(724–794)、`validate_transforms`(862–883)、`validate_composition_config`(885–936)、`validate_composition_member`(938–968)、`validate_source`(970–988)、`default_valid`(998–1004)、`items_serde`(1010–1049)。

> 注:示例 `items_serde` 内部用了 `serde::de::Error as _`;在 mod 内补 `use serde::de::Error as _;`(示例第 1014 行已含),保持原样粘贴即可。

- [ ] **Step 10: 创建 `dependency.rs`**

```rust
use std::collections::HashMap;

use indexmap::IndexSet;

use super::*;
```

逐字粘贴示例文件 `ProfileDependencyIndex` + impl(798–860)。

- [ ] **Step 11: 重写 `profile/mod.rs`**

```rust
mod definition;
mod dependency;
mod id;
mod item;
mod metadata;
mod path;
mod profiles;
mod source;

pub use definition::*;
pub use dependency::*;
pub use id::*;
pub use item::*;
pub use metadata::*;
pub use path::*;
pub use profiles::*;
pub use source::*;

#[cfg(test)]
mod tests;
```

- [ ] **Step 12: 删除 legacy 文件**

```bash
git rm backend/nyanpasu-config/src/profile/builder.rs \
       backend/nyanpasu-config/src/profile/item/mod.rs \
       backend/nyanpasu-config/src/profile/item/kind.rs \
       backend/nyanpasu-config/src/profile/item/remote.rs \
       backend/nyanpasu-config/src/profile/tests/compat.rs \
       backend/nyanpasu-config/src/profile/tests/patch.rs \
       backend/nyanpasu-config/src/profile/tests/profiles.rs
```

- [ ] **Step 13: 重写 `profile/tests/mod.rs`**

```rust
mod round_trip;
```

- [ ] **Step 14: 修复 `runtime/snapshot.rs`**

把第 14 行

```rust
use crate::{profile::item::kind::ProfileItemType, runtime::value::ConfigValue};
```

改为

```rust
use crate::{profile::TransformKind, runtime::value::ConfigValue};
```

把 `OperatorTag::ChainNode` 里的

```rust
        profile_kind: ProfileItemType,
```

改为

```rust
        profile_kind: TransformKind,
```

- [ ] **Step 15: 在 `snapshot.rs` 的 `#[cfg(test)] mod tests` 内追加 ChainNode round-trip 测试**

在 `mod tests` 内新增:

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

- [ ] **Step 16: 运行全量测试**

Run: `cargo test -p nyanpasu-config`
Expected: PASS(`clean_document_round_trips`、`chain_node_tag_round_trips_with_transform_kind` 及既有 snapshot/其它测试全绿)。

- [ ] **Step 17: 提交**

```bash
git add backend/nyanpasu-config/src/profile backend/nyanpasu-config/src/runtime/snapshot.rs
git commit -m "refactor(nyanpasu-config): migrate profile types to clean composition model"
```

---

## Task 2: `ProfileMetadataPatch` 语义测试

`ProfileMetadata` 已在 Task 1 派生 `Patch`。本 task 用测试钉死 leaf patch 语义(absent-keep / null-clear / sparse serialize)。

**Files:**

- Create: `backend/nyanpasu-config/src/profile/tests/metadata_patch.rs`
- Modify: `backend/nyanpasu-config/src/profile/tests/mod.rs`

**Interfaces:**

- Consumes: `ProfileMetadata`、`ProfileMetadataPatch`(Task 1)、`struct_patch::{Patch, Status}`。

- [ ] **Step 1: 写失败测试**

创建 `backend/nyanpasu-config/src/profile/tests/metadata_patch.rs`:

```rust
//! Leaf patch semantics for `ProfileMetadata`.
use crate::profile::ProfileMetadata;
use struct_patch::Patch;

fn seed() -> ProfileMetadata {
    ProfileMetadata {
        name: "Original".into(),
        desc: Some("keep me".into()),
    }
}

#[test]
fn absent_fields_are_kept() {
    let mut meta = seed();
    let patch = serde_yaml_ng::from_str("name: Renamed\n").expect("patch deserializes");
    meta.apply(patch);
    assert_eq!(meta.name, "Renamed");
    assert_eq!(meta.desc.as_deref(), Some("keep me"));
}

#[test]
fn explicit_null_clears_desc() {
    let mut meta = seed();
    let patch = serde_yaml_ng::from_str("desc: null\n").expect("patch deserializes");
    meta.apply(patch);
    assert_eq!(meta.desc, None);
}

#[test]
fn empty_patch_is_noop_and_sparse() {
    let patch = ProfileMetadata::new_empty_patch();
    let dumped = serde_yaml_ng::to_string(&patch).expect("serialize empty patch");
    assert_eq!(dumped.trim(), "{}", "empty patch must be sparse, got:\n{dumped}");

    let before = seed();
    let mut after = before.clone();
    after.apply(patch);
    assert_eq!(before, after);
}
```

- [ ] **Step 2: 在 `tests/mod.rs` 注册模块**

把 `tests/mod.rs` 改为:

```rust
mod metadata_patch;
mod round_trip;
```

- [ ] **Step 3: 运行测试,确认通过**

Run: `cargo test -p nyanpasu-config metadata_patch`
Expected: PASS(3 个测试)。若 `new_empty_patch` 未解析,确认 Task 1 的 `ProfileMetadata` 派生了 `Patch` 且导入了 `struct_patch::Patch`。

- [ ] **Step 4: 提交**

```bash
git add backend/nyanpasu-config/src/profile/tests
git commit -m "test(nyanpasu-config): pin ProfileMetadata leaf-patch semantics"
```

---

## Task 3: `RemoteProfileOptionsPatch` 语义测试

钉死 remote 选项 leaf patch(只覆盖在场字段、null 清空 `user_agent`、diff 只暴露变化字段;**不**再支持旧 `update_interval` alias)。

**Files:**

- Create: `backend/nyanpasu-config/src/profile/tests/remote_options_patch.rs`
- Modify: `backend/nyanpasu-config/src/profile/tests/mod.rs`

**Interfaces:**

- Consumes: `RemoteProfileOptions`、`RemoteProfileOptionsPatch`(Task 1)、`struct_patch::Patch`。

- [ ] **Step 1: 写测试**

创建 `backend/nyanpasu-config/src/profile/tests/remote_options_patch.rs`:

```rust
//! Leaf patch semantics for `RemoteProfileOptions`.
use crate::profile::RemoteProfileOptions;
use struct_patch::Patch;

#[test]
fn applies_only_present_fields() {
    let mut opts = RemoteProfileOptions::default();
    let original_interval = opts.update_interval_minutes;
    let patch = serde_yaml_ng::from_str("with_proxy: false\n").expect("patch deserializes");
    opts.apply(patch);
    assert!(!opts.with_proxy);
    assert_eq!(opts.update_interval_minutes, original_interval);
}

#[test]
fn null_clears_user_agent() {
    let mut opts = RemoteProfileOptions {
        user_agent: Some("clash-nyanpasu".into()),
        ..RemoteProfileOptions::default()
    };
    let patch = serde_yaml_ng::from_str("user_agent: null\n").expect("patch deserializes");
    opts.apply(patch);
    assert_eq!(opts.user_agent, None);
}

#[test]
fn legacy_update_interval_alias_is_gone() {
    // The clean model drops the `update_interval` alias; only the canonical name decodes.
    let patch: RemoteProfileOptionsPatch =
        serde_yaml_ng::from_str("update_interval_minutes: 240\n").expect("canonical decodes");
    assert_eq!(patch.update_interval_minutes, Some(240));

    let alias: RemoteProfileOptionsPatch =
        serde_yaml_ng::from_str("update_interval: 240\n").expect("unknown key is ignored");
    assert_eq!(alias.update_interval_minutes, None, "alias must NOT map");
}

#[test]
fn diff_surfaces_only_changed_fields() {
    let base = RemoteProfileOptions::default();
    let changed = RemoteProfileOptions {
        with_proxy: !base.with_proxy,
        ..base.clone()
    };
    let patch = changed.into_patch_by_diff(base);
    assert_eq!(patch.with_proxy, Some(!RemoteProfileOptions::default().with_proxy));
    assert_eq!(patch.update_interval_minutes, None);
    assert_eq!(patch.user_agent, None);
    assert_eq!(patch.self_proxy, None);
}
```

> 若 `RemoteProfileOptionsPatch` 名称未在 `crate::profile` 顶层导出,在测试顶部加 `use crate::profile::RemoteProfileOptionsPatch;`(struct-patch 生成的类型与原类型同模块,经 `pub use source::*` 重导出后应可达)。

- [ ] **Step 2: 在 `tests/mod.rs` 注册模块**

```rust
mod metadata_patch;
mod remote_options_patch;
mod round_trip;
```

- [ ] **Step 3: 运行测试**

Run: `cargo test -p nyanpasu-config remote_options_patch`
Expected: PASS(4 个测试)。`update_interval: 240` 解析为忽略未知键 → `None`,证明 alias 已移除。

- [ ] **Step 4: 提交**

```bash
git add backend/nyanpasu-config/src/profile/tests
git commit -m "test(nyanpasu-config): pin RemoteProfileOptions patch, prove alias dropped"
```

---

## Task 4: 特化 mutator(`patch.rs`)

实现并测试特化 patch 面:profile-id 列表的 add/remove/move、顶层 current/valid、global transforms 列表操作、`ProfileItem` 的原子 `set_definition`/`set_source`/`apply_metadata_patch`、`CompositionConfig` 的 base/contributor 操作。

**Files:**

- Create: `backend/nyanpasu-config/src/profile/patch.rs`
- Create: `backend/nyanpasu-config/src/profile/tests/mutators.rs`
- Modify: `backend/nyanpasu-config/src/profile/mod.rs`、`backend/nyanpasu-config/src/profile/tests/mod.rs`

**Interfaces:**

- Consumes: `Profiles`、`ProfileItem`、`ProfileDefinition`、`ConfigDefinition`、`CompositionConfig`、`ProfileSource`、`ProfileId`、`ProfileMetadataPatch`(Task 1–2)。
- Produces:
  - 自由函数 `list_add(&mut Vec<ProfileId>, ProfileId) -> bool`、`list_remove(&mut Vec<ProfileId>, &ProfileId) -> bool`、`list_move(&mut Vec<ProfileId>, usize, usize) -> bool`
  - `Profiles::{set_current(Option<ProfileId>), clear_current(), set_valid(Vec<String>), add_global_transform(ProfileId)->bool, remove_global_transform(&ProfileId)->bool, move_global_transform(usize,usize)->bool}`
  - `ProfileItem::{apply_metadata_patch(ProfileMetadataPatch), set_definition(ProfileDefinition), set_source(ProfileSource)->bool}`
  - `CompositionConfig::{set_base(Option<ProfileId>), add_contributor(ProfileId)->bool, remove_contributor(&ProfileId)->bool, move_contributor(usize,usize)->bool}`

- [ ] **Step 1: 写失败测试**

创建 `backend/nyanpasu-config/src/profile/tests/mutators.rs`:

```rust
//! Specialized mutators: list-ops, atomic replacement, top-level setters.
use crate::profile::*;

fn id(s: &str) -> ProfileId {
    ProfileId(s.to_owned())
}

fn managed_overlay(uid: &str, file: &str) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata {
            name: uid.into(),
            desc: None,
        },
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(OverlayTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(file).unwrap(),
                            updated_at: None,
                        },
                    },
                },
            }),
        },
    }
}

#[test]
fn list_ops_dedup_remove_and_move() {
    let mut list = vec![id("a"), id("b")];
    assert!(list_add(&mut list, id("c")));
    assert!(!list_add(&mut list, id("a")), "dedup");
    assert_eq!(list, vec![id("a"), id("b"), id("c")]);

    assert!(list_remove(&mut list, &id("b")));
    assert!(!list_remove(&mut list, &id("zzz")));
    assert_eq!(list, vec![id("a"), id("c")]);

    assert!(list_move(&mut list, 0, 1));
    assert_eq!(list, vec![id("c"), id("a")]);
    assert!(!list_move(&mut list, 0, 9), "out of range is a no-op");
}

#[test]
fn top_level_setters_and_global_transforms() {
    let mut profiles = Profiles::default();
    profiles.set_current(Some(id("x")));
    assert_eq!(profiles.current, Some(id("x")));
    profiles.clear_current();
    assert_eq!(profiles.current, None);

    profiles.set_valid(vec!["dns".into()]);
    assert_eq!(profiles.valid, vec!["dns".to_string()]);

    assert!(profiles.add_global_transform(id("g1")));
    assert!(!profiles.add_global_transform(id("g1")));
    assert!(profiles.add_global_transform(id("g2")));
    assert!(profiles.move_global_transform(1, 0));
    assert_eq!(profiles.global_transforms, vec![id("g2"), id("g1")]);
    assert!(profiles.remove_global_transform(&id("g1")));
    assert_eq!(profiles.global_transforms, vec![id("g2")]);
}

#[test]
fn item_atomic_replacement_and_metadata_patch() {
    let mut item = managed_overlay("ov", "ov.yaml");

    // metadata patch
    let patch = serde_yaml_ng::from_str("name: Renamed\n").unwrap();
    item.apply_metadata_patch(patch);
    assert_eq!(item.metadata.name, "Renamed");

    // set_source on a transform succeeds (transforms have a source)
    let new_source = ProfileSource::Local {
        binding: LocalBinding::Managed {
            materialized: MaterializedFile {
                file: ManagedProfilePath::new("ov2.yaml").unwrap(),
                updated_at: None,
            },
        },
    };
    assert!(item.set_source(new_source));
    assert_eq!(
        item.definition.source().unwrap().materialized().file.as_str(),
        "ov2.yaml"
    );

    // atomic kind switch: Transform -> Composition Config
    item.set_definition(ProfileDefinition::Config {
        config: ConfigDefinition::Composition(CompositionConfig {
            base: None,
            extend_proxies_from: vec![id("sub-a")],
            transforms: vec![],
        }),
    });
    assert!(item.definition.is_config());
    // composition has no source
    assert!(item.definition.source().is_none());
    assert!(!item.set_source(ProfileSource::Local {
        binding: LocalBinding::Managed {
            materialized: MaterializedFile {
                file: ManagedProfilePath::new("nope.yaml").unwrap(),
                updated_at: None,
            },
        },
    }));
}

#[test]
fn composition_contributor_ops() {
    let mut comp = CompositionConfig {
        base: None,
        extend_proxies_from: vec![],
        transforms: vec![],
    };
    comp.set_base(Some(id("base")));
    assert_eq!(comp.base, Some(id("base")));
    assert!(comp.add_contributor(id("c1")));
    assert!(comp.add_contributor(id("c2")));
    assert!(!comp.add_contributor(id("c1")), "dedup");
    assert!(comp.move_contributor(1, 0));
    assert_eq!(comp.extend_proxies_from, vec![id("c2"), id("c1")]);
    assert!(comp.remove_contributor(&id("c2")));
    assert_eq!(comp.extend_proxies_from, vec![id("c1")]);
}
```

- [ ] **Step 2: 注册测试模块并占位 `mod patch`**

`tests/mod.rs`:

```rust
mod metadata_patch;
mod mutators;
mod remote_options_patch;
mod round_trip;
```

`profile/mod.rs` 增加(放在其它 `mod` 行之间,保持字母序):

```rust
mod patch;
```

并在 `pub use` 区追加:

```rust
pub use patch::*;
```

- [ ] **Step 3: 运行测试,确认失败**

Run: `cargo test -p nyanpasu-config mutators`
Expected: 编译失败(`patch.rs` 尚未实现 `list_add` 等)。

- [ ] **Step 4: 实现 `patch.rs`**

创建 `backend/nyanpasu-config/src/profile/patch.rs`:

```rust
//! Specialized mutators for the profile model.
//!
//! Enum-variant transitions (`Local <-> Remote`, `File <-> Composition`,
//! `Overlay <-> Script`) are done by atomic replacement, never field-level
//! patch, so the model never holds an illegal intermediate state. Leaf structs
//! (`ProfileMetadata`, `RemoteProfileOptions`) keep their struct-patch types.
use struct_patch::Patch;

use super::*;

/// Append `uid` unless already present. Returns whether it was added.
pub fn list_add(list: &mut Vec<ProfileId>, uid: ProfileId) -> bool {
    if list.contains(&uid) {
        false
    } else {
        list.push(uid);
        true
    }
}

/// Remove the first occurrence of `uid`. Returns whether anything was removed.
pub fn list_remove(list: &mut Vec<ProfileId>, uid: &ProfileId) -> bool {
    if let Some(pos) = list.iter().position(|existing| existing == uid) {
        list.remove(pos);
        true
    } else {
        false
    }
}

/// Move the element at `from` to index `to`. No-op (`false`) when either index
/// is out of range or they are equal.
pub fn list_move(list: &mut Vec<ProfileId>, from: usize, to: usize) -> bool {
    if from >= list.len() || to >= list.len() || from == to {
        return false;
    }
    let item = list.remove(from);
    list.insert(to, item);
    true
}

impl Profiles {
    pub fn set_current(&mut self, uid: Option<ProfileId>) {
        self.current = uid;
    }

    pub fn clear_current(&mut self) {
        self.current = None;
    }

    pub fn set_valid(&mut self, valid: Vec<String>) {
        self.valid = valid;
    }

    pub fn add_global_transform(&mut self, uid: ProfileId) -> bool {
        list_add(&mut self.global_transforms, uid)
    }

    pub fn remove_global_transform(&mut self, uid: &ProfileId) -> bool {
        list_remove(&mut self.global_transforms, uid)
    }

    pub fn move_global_transform(&mut self, from: usize, to: usize) -> bool {
        list_move(&mut self.global_transforms, from, to)
    }
}

impl ProfileItem {
    pub fn apply_metadata_patch(&mut self, patch: ProfileMetadataPatch) {
        self.metadata.apply(patch);
    }

    /// Atomically replace the whole definition (kind / source / binding switch).
    pub fn set_definition(&mut self, definition: ProfileDefinition) {
        self.definition = definition;
    }

    /// Replace the source in place. Returns `false` for definitions without a
    /// source (a `CompositionConfig` has no materialized source).
    pub fn set_source(&mut self, source: ProfileSource) -> bool {
        match self.definition.source_mut() {
            Some(slot) => {
                *slot = source;
                true
            }
            None => false,
        }
    }
}

impl CompositionConfig {
    pub fn set_base(&mut self, base: Option<ProfileId>) {
        self.base = base;
    }

    pub fn add_contributor(&mut self, uid: ProfileId) -> bool {
        list_add(&mut self.extend_proxies_from, uid)
    }

    pub fn remove_contributor(&mut self, uid: &ProfileId) -> bool {
        list_remove(&mut self.extend_proxies_from, uid)
    }

    pub fn move_contributor(&mut self, from: usize, to: usize) -> bool {
        list_move(&mut self.extend_proxies_from, from, to)
    }
}
```

- [ ] **Step 5: 运行测试,确认通过**

Run: `cargo test -p nyanpasu-config mutators`
Expected: PASS(4 个测试)。

- [ ] **Step 6: 全量回归 + 提交**

Run: `cargo test -p nyanpasu-config`
Expected: PASS。

```bash
git add backend/nyanpasu-config/src/profile
git commit -m "feat(nyanpasu-config): specialized profile mutators (list-ops + atomic replacement)"
```

---

## Task 5: validation 与 dependency index 特征测试

`validate()` / `ProfileDependencyIndex::build()` 已随 Task 1 转录落地。本 task 用测试覆盖设计文档 §18 的代表性引用约束,以及依赖索引反向映射。

**Files:**

- Create: `backend/nyanpasu-config/src/profile/tests/validation.rs`
- Create: `backend/nyanpasu-config/src/profile/tests/dependency.rs`
- Modify: `backend/nyanpasu-config/src/profile/tests/mod.rs`

**Interfaces:**

- Consumes: `Profiles`、`ProfileValidationError`、`ProfileDependencyIndex`、各类型构造器、`ManagedProfilePath`、`ExternalProfilePath`(Task 1)。

- [ ] **Step 1: 写 validation 测试**

创建 `backend/nyanpasu-config/src/profile/tests/validation.rs`:

```rust
//! Referential/semantic validation coverage (design doc §18 subset).
use crate::profile::*;

fn id(s: &str) -> ProfileId {
    ProfileId(s.to_owned())
}

fn file_config(uid: &str, file: &str) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata { name: uid.into(), desc: None },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(file).unwrap(),
                            updated_at: None,
                        },
                    },
                },
                transforms: vec![],
            }),
        },
    }
}

fn overlay(uid: &str, file: &str) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata { name: uid.into(), desc: None },
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(OverlayTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(file).unwrap(),
                            updated_at: None,
                        },
                    },
                },
            }),
        },
    }
}

fn profiles_with(items: Vec<ProfileItem>) -> Profiles {
    let mut profiles = Profiles::default();
    for item in items {
        assert!(profiles.append_item(item));
    }
    profiles
}

fn has_error(errors: &[ProfileValidationError], pred: impl Fn(&ProfileValidationError) -> bool) -> bool {
    errors.iter().any(pred)
}

#[test]
fn current_must_be_an_existing_config() {
    let mut profiles = profiles_with(vec![overlay("ov", "ov.yaml")]);
    profiles.current = Some(id("ov"));
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(e, ProfileValidationError::CurrentNotConfig(_))));

    profiles.current = Some(id("ghost"));
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(e, ProfileValidationError::CurrentNotFound(_))));
}

#[test]
fn transform_target_must_be_a_transform() {
    let mut cfg = file_config("c", "c.yaml");
    if let ProfileDefinition::Config { config: ConfigDefinition::File(f) } = &mut cfg.definition {
        f.transforms = vec![id("c")]; // points at a Config, not a Transform
    }
    let profiles = profiles_with(vec![cfg]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::TransformTargetNotTransform { .. }
    )));
}

#[test]
fn composition_member_must_be_direct_file_config() {
    let comp = ProfileItem {
        uid: id("comp"),
        metadata: ProfileMetadata { name: "comp".into(), desc: None },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::Composition(CompositionConfig {
                base: Some(id("ov")),
                extend_proxies_from: vec![],
                transforms: vec![],
            }),
        },
    };
    let profiles = profiles_with(vec![overlay("ov", "ov.yaml"), comp]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::CompositionMemberNotDirectFileConfig { .. }
    )));
}

#[test]
fn empty_composition_is_rejected() {
    let comp = ProfileItem {
        uid: id("comp"),
        metadata: ProfileMetadata { name: "comp".into(), desc: None },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::Composition(CompositionConfig {
                base: None,
                extend_proxies_from: vec![],
                transforms: vec![],
            }),
        },
    };
    let profiles = profiles_with(vec![comp]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::EmptyCompositionConfig { .. }
    )));
}

#[test]
fn duplicate_materialized_file_is_rejected() {
    let profiles = profiles_with(vec![
        file_config("a", "same.yaml"),
        file_config("b", "same.yaml"),
    ]);
    let errors = profiles.validate().unwrap_err();
    assert!(has_error(&errors, |e| matches!(
        e,
        ProfileValidationError::DuplicateMaterializedFile { .. }
    )));
}

#[test]
fn managed_path_rejects_absolute_traversal_and_url() {
    assert!(ManagedProfilePath::new("/abs.yaml").is_err());
    assert!(ManagedProfilePath::new("../escape.yaml").is_err());
    assert!(ManagedProfilePath::new("https://x/y.yaml").is_err());
    assert!(ManagedProfilePath::new("ok.yaml").is_ok());
}

#[test]
fn external_path_requires_absolute() {
    assert!(ExternalProfilePath::new("relative.yaml").is_err());
    assert!(ExternalProfilePath::new("/abs/target.yaml").is_ok());
}

#[test]
fn duplicate_uid_fails_to_deserialize() {
    let yaml = r#"items:
  - uid: dup
    name: first
    type: transform
    transform:
      type: overlay
      source:
        type: local
        binding:
          type: managed
          file: a.yaml
  - uid: dup
    name: second
    type: transform
    transform:
      type: overlay
      source:
        type: local
        binding:
          type: managed
          file: b.yaml
"#;
    assert!(serde_yaml_ng::from_str::<Profiles>(yaml).is_err());
}
```

- [ ] **Step 2: 写 dependency 测试**

创建 `backend/nyanpasu-config/src/profile/tests/dependency.rs`:

```rust
//! Reverse-reference dependency index coverage.
use crate::profile::*;

fn id(s: &str) -> ProfileId {
    ProfileId(s.to_owned())
}

fn file_config(uid: &str, file: &str, transforms: Vec<ProfileId>) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata { name: uid.into(), desc: None },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(file).unwrap(),
                            updated_at: None,
                        },
                    },
                },
                transforms,
            }),
        },
    }
}

fn overlay(uid: &str, file: &str) -> ProfileItem {
    ProfileItem {
        uid: id(uid),
        metadata: ProfileMetadata { name: uid.into(), desc: None },
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(OverlayTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new(file).unwrap(),
                            updated_at: None,
                        },
                    },
                },
            }),
        },
    }
}

#[test]
fn index_maps_base_extend_transform_and_global() {
    let mut profiles = Profiles::default();
    assert!(profiles.append_item(file_config("a", "a.yaml", vec![id("ov")])));
    assert!(profiles.append_item(file_config("b", "b.yaml", vec![])));
    assert!(profiles.append_item(overlay("ov", "ov.yaml")));
    assert!(profiles.append_item(overlay("g", "g.yaml")));

    let comp = ProfileItem {
        uid: id("comp"),
        metadata: ProfileMetadata { name: "comp".into(), desc: None },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::Composition(CompositionConfig {
                base: Some(id("a")),
                extend_proxies_from: vec![id("b")],
                transforms: vec![id("ov")],
            }),
        },
    };
    assert!(profiles.append_item(comp));
    profiles.global_transforms = vec![id("g")];

    let index = ProfileDependencyIndex::build(&profiles);

    assert!(index.composition_base_dependents[&id("a")].contains(&id("comp")));
    assert!(index.extend_proxies_dependents[&id("b")].contains(&id("comp")));
    // `ov` is referenced both by file-config `a` and by composition `comp`.
    assert!(index.transform_dependents[&id("ov")].contains(&id("a")));
    assert!(index.transform_dependents[&id("ov")].contains(&id("comp")));
    assert!(index.global_transform_ids.contains(&id("g")));
}
```

- [ ] **Step 3: 注册测试模块**

`tests/mod.rs`:

```rust
mod dependency;
mod metadata_patch;
mod mutators;
mod remote_options_patch;
mod round_trip;
mod validation;
```

- [ ] **Step 4: 运行测试**

Run: `cargo test -p nyanpasu-config validation dependency`
Expected: PASS。若某断言因示例 `validate()` 的具体错误变体命名不符,以示例文件 `ProfileValidationError`(724–794)的实际变体名为准修正 `matches!` 分支。

- [ ] **Step 5: 全量回归 + 提交**

Run: `cargo test -p nyanpasu-config`
Expected: PASS。

```bash
git add backend/nyanpasu-config/src/profile/tests
git commit -m "test(nyanpasu-config): cover profile validation and dependency index"
```

---

## Task 6: 文档 #1 —— tauri 调用迁移指南

**Files:**

- Create: `docs/design/profile-tauri-migration-guide.md`

**Interfaces:**

- Consumes: 已落地的 `nyanpasu_config::profile` 类型;现状调用点(`backend/tauri/src/ipc.rs`、`feat.rs`、`enhance/chain.rs`、`config/profile/**`、`client/mod.rs`)。

- [ ] **Step 1: 写文档**

创建 `docs/design/profile-tauri-migration-guide.md`,包含且仅包含以下章节(每节填实,不留 TODO):

1. **目的与范围** —— 本指南为后期 tauri 迁移提供映射,本期不实现代码改动。
2. **调用点清单** —— 逐条列出 `ipc.rs` 的 profile 命令(`get_profiles`、`import_profile`、`create_profile`、`reorder_profile`、`reorder_profiles_by_list`、`update_profile`、`delete_profile`、`patch_profiles_config`、`patch_profile`、`view_profile`、`read_profile_file`、`save_profile_file`、`enhance_profiles`),附 file:line 与一句话职责;并列出 `feat.rs::update_profile`、`enhance/chain.rs`、`client/mod.rs::patch_profiles_config`。
3. **类型映射表** —— legacy(tauri `Profile`/`PrfItem`/`ProfileItemType`/`ProfileBuilder`/`ProfilesBuilder`)→ 新(`ProfileItem`/`ProfileDefinition`/`ProfileCategory`+`ConfigDefinition`/`TransformKind`/特化 patch)。
4. **语义迁移要点** —— `current: Vec` → `Option` + `CompositionConfig{base, extend_proxies_from}`;`chain`/`chains` → `transforms` + `global_transforms`;`Merge` → `Overlay`;`ProfileMeta.updated` → `MaterializedFile.updated_at`;`ProfileFile` 的 URL/path 猜测 → `MaterializedFile.file: ManagedProfilePath` + `Remote.url`。
5. **命令逐条迁移** —— 每个命令给出"现状签名 → 目标 `NyanpasuClient`/类型 API 调用"草图(伪代码即可),并标注 `patch_item(ProfileBuilder)` → 分层 patch(见文档 #2)、删除策略 → 引用保护(设计 §17)。
6. **旧数据迁移** —— 指向 tauri migration V2 子系统(`PathResolver`),引用设计文档 §14 的映射规则(旧 `type` → 新定义、`file:URL` 只在 migration、多 current → `CompositionConfig`)。本期不实现。
7. **迁移顺序与风险** —— 建议顺序、specta/TS 绑定变化、破坏性变更点。

- [ ] **Step 2: 校验无占位符**

Run: `grep -nE "TODO|TBD|占位|fill in" docs/design/profile-tauri-migration-guide.md`
Expected: 无输出。

- [ ] **Step 3: 提交**

```bash
git add docs/design/profile-tauri-migration-guide.md
git commit -m "docs(design): tauri profiles call-site migration guide"
```

---

## Task 7: 文档 #2 —— patch 接口分析

**Files:**

- Create: `docs/design/profile-patch-interface.md`

**Interfaces:**

- Consumes: Task 1–4 已实现的 patch 面(`ProfileMetadataPatch`、`RemoteProfileOptionsPatch`、`patch.rs` mutator)。

- [ ] **Step 1: 写文档**

创建 `docs/design/profile-patch-interface.md`,章节:

1. **问题** —— 原始 patch 需求清单(元数据、remote 选项、per-type 字段、chain、顶层 current/chain/valid、整项替换),来源:legacy tauri `ProfileBuilder`/`ProfilesBuilder` 与原 `nyanpasu-config` struct_patch。
2. **结论** —— 能提供特化 patch 接口;**不**对整个嵌套 tagged enum 做单一 struct_patch。
3. **为何不做单一 enum patch** —— internally-tagged enum 上 struct_patch 不可组合;变体切换会产生非法中间态(举 `Local→Remote` 例)。
4. **特化分层 patch 面**(对照 spec §5.2 表)—— leaf struct_patch(`ProfileMetadataPatch`/`RemoteProfileOptionsPatch`)+ 原子替换(`set_definition`/`set_source`)+ list-ops(`transforms`/`extend_proxies_from`/`global_transforms`)+ 顶层 setter。给出已实现的方法签名(取自 Task 4 Produces)。
5. **原始需求 → 新接口逐条映射表**(含已取消项:`updated` patch、subscription extra patch、`update_interval` alias)。
6. **事务流程** —— clone → mutate → validate → persist → commit(设计 §15),解释为何能避免临时非法状态。
7. **证据** —— 引用 `tests/{metadata_patch,remote_options_patch,mutators}.rs` 中对应测试名,证明新结构体满足原始 patch 需求。

- [ ] **Step 2: 校验无占位符**

Run: `grep -nE "TODO|TBD|占位|fill in" docs/design/profile-patch-interface.md`
Expected: 无输出。

- [ ] **Step 3: 提交**

```bash
git add docs/design/profile-patch-interface.md
git commit -m "docs(design): profile patch interface analysis"
```

---

## Task 8: 文档 #3 —— snapshot store 迁移思路

**Files:**

- Create: `docs/design/profile-snapshot-store-migration.md`

**Interfaces:**

- Consumes: `runtime/snapshot.rs`(`OperatorTag`、`ChainNodeKind`、Task 1 已把 `ChainNode.profile_kind` 改为 `TransformKind`)。

- [ ] **Step 1: 写文档**

创建 `docs/design/profile-snapshot-store-migration.md`,章节:

1. **现状耦合点** —— 列出 `OperatorTag` 各变体(`Root`/`SecondaryProcessing`/`SelectedProfilesProxiesMerge`/`ChainNode{kind, profile_id, profile_kind}`/`BuiltinChain`/`GuardOverrides`/`WhitelistFieldFilter`/`Finalizing`)与 `ChainNodeKind{Scoped, Global}`,标注哪些携带 profile 语义。
2. **本期已做的最小同步** —— `ChainNode.profile_kind: ProfileItemType` → `TransformKind`,并说明语义(chain 节点即新模型的 Transform)。附 `tests::chain_node_tag_round_trips_with_transform_kind` 为证。
3. **词汇重映射** —— 旧 chain → `transforms`;`ChainNodeKind::Scoped` → `FileConfig/CompositionConfig.transforms`;`ChainNodeKind::Global` → `Profiles.global_transforms`;`SelectedProfilesProxiesMerge{primary, others}` → `CompositionConfig{base, extend_proxies_from}`;`Root.primary_profile_id` / `SecondaryProcessing` 的新归属。
4. **推迟项与全量重做思路** —— 设计文档 §3 推迟的 SnapshotStore / OperatorTag / StepId;如何由新分类法 + `ProfileDependencyIndex`(设计 §16)驱动失效与重建(base/extend/transform/global 四类反向依赖 → 重建当前运行配置)。
5. **持久化影响** —— `snapshot.rs` 的 `format_version`/`SnapshotArchive`;`OperatorTag` 变体内字段类型变化是对已持久化快照的破坏性变更;给出版本处理策略建议(bump `SNAPSHOT_ARCHIVE_VERSION` 或丢弃旧缓存),本期不实现。
6. **后续工作清单** —— 把全量 snapshot 重做拆为可跟踪的后续 step。

- [ ] **Step 2: 校验无占位符**

Run: `grep -nE "TODO|TBD|占位|fill in" docs/design/profile-snapshot-store-migration.md`
Expected: 无输出。

- [ ] **Step 3: 提交**

```bash
git add docs/design/profile-snapshot-store-migration.md
git commit -m "docs(design): runtime snapshot store migration approach"
```

---

## 收尾验证

- [ ] **全量构建/测试**

Run: `cargo build -p nyanpasu-config && cargo test -p nyanpasu-config`
Expected: 构建通过,全部测试 PASS。

- [ ] **确认 tauri 零改动**

Run: `git diff --name-only main -- backend/tauri`
Expected: 无输出。

- [ ] **确认 legacy 类型已清除**

Run: `grep -rnE "ProfileItemType|ProfileSource::Merge|ProfileFile" backend/nyanpasu-config/src`
Expected: 无输出(`ProfileItemType` 已被 `TransformKind` 取代,`Merge`/`ProfileFile` 已删除)。

---

## Self-Review 记录

- **Spec 覆盖:** spec §3 范围 → Task 1(类型+snapshot);§4 类型落地 → Task 1;§5 patch → Task 2/3/4 + 文档 #2(Task 7);§6 snapshot 同步 → Task 1 Step 14–15 + 文档 #3(Task 8);§7 三文档 → Task 6/7/8;§8 测试策略 → Task 1/2/3/4/5;§10 成功判据 → 收尾验证。无缺口。
- **占位符:** 代码 task 全部给出完整代码;文档 task 给出完整章节骨架 + 内容指令并带 grep 校验。
- **类型一致性:** `set_source`/`source_mut`/`is_direct_file_config`/`transforms_mut`/`TransformKind`/`ScriptRuntime`/`ProfileMetadataPatch`/`RemoteProfileOptionsPatch` 在 Task 间命名一致;`ProfileItem` 字段(`uid`/`metadata`/`definition`)、`Profiles` 字段(`current`/`global_transforms`/`valid`/`items`)与示例文件一致。
