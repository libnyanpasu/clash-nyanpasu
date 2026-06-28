# Profile Config / Transform 数据模型重构设计

**状态：** 提案  
**范围：** 仅覆盖 `Profiles`、`ProfileItem`、Source/Binding、引用验证、migration 映射和依赖索引  
**不在本轮范围：** SnapshotStore、OperatorTag、StepId、Pipeline IR、ConfigTarget/ConfigType、preview/diff/report 执行层  
**兼容策略：** 只通过 migration system 兼容旧数据；新 Rust 类型不兼容旧 wire format

## 1. 背景

现有 Profile 模型中存在几组混在一起的概念：

- 内容产物：可以生成并激活一个完整 Clash 配置；
- 后处理：Merge/Script 消费一个配置并修改它；
- 内容来源：本地文件或远程订阅；
- 本地绑定：应用托管文件、外部文件、符号链接或镜像副本；
- 选择状态：`Profiles.current` 可以是单值，也可以是列表。

旧 `current` 列表的实际语义并不是“同时激活多个完整 Profile”，而是：

1. 第一个 Profile 提供完整配置；
2. 后续 Profile 只贡献 `proxies` / nodes；
3. 后处理阶段把这些节点追加到第一个配置。

新设计把 Profile 拆成两个主类：

```text
ProfileDefinition = Config | Transform

Config    = 产生完整配置，可以被 current 选中
Transform = 对配置做后处理，不能被 current 选中
```

旧 `Merge` 在新 schema 中重命名为 `Overlay`，避免和“组合多个 Config”的语义冲突。旧 `current: [a, b, c]` 迁移为一个可命名、可选择的一等 `CompositionConfig`。

本版进一步允许 `CompositionConfig.base = None`。这表示从一个干净的空配置 seed 开始，只继承多个 Profile 的 nodes/proxies，再通过 transforms 重写 `proxy-groups`、`rules`、`dns` 等字段。

## 2. 设计目标

1. `ProfileItem` 只分为 `Config` 和 `Transform`。
2. 只有 `Config` 可以作为 `Profiles.current`。
3. `Transform` 只能被 `Config.transforms` 或 `Profiles.global_transforms` 引用。
4. 文件型 `Config`、`Overlay` 和 `Script` 都可以使用 Local 或 Remote 来源。
5. Remote 是“定期更新的本地物化文件”；解析器永远读取本地文件。
6. Local 可以绑定到应用托管文件或外部文件。
7. Rust 类型系统禁止 `Remote + External`。
8. `Profiles.current` 只保存单个 `ProfileId`。
9. 多 Profile 节点汇总通过 `CompositionConfig { base, extend_proxies_from }` 表达。
10. `CompositionConfig.base` 是 `Option<ProfileId>`：
    - `Some(id)`：继承一个完整 base Config；
    - `None`：使用 clean seed，只通过 `extend_proxies_from` 继承 nodes/proxies。
11. `extend_proxies_from` 只是字段，不是 Composition Step，也没有 StepId。
12. `transforms` 和 `global_transforms` 使用 `Vec<ProfileId>`，不预定义步骤 ID。
13. 新领域模型不保留旧 alias、旧枚举或 URL/path 猜测逻辑。
14. 所有跨 Profile 引用在加载和事务提交前统一验证。

## 3. 非目标

本轮不处理：

- SnapshotStore / OperatorTag 调整；
- PipelineOperation / Pipeline IR；
- StepId / StepAddress / step report；
- ConfigTarget、ConfigType、TargetSelector、sing-box 支持；
- Script 静态分析策略；
- GUI preview / diff / diagnostics；
- 跨 target 通用 IR 或转换器；
- 对多个 Config 的 `rules`、`dns`、`proxy-groups` 等字段做泛化合并；
- 在 `extend_proxies_from` 层做节点去重、重命名或覆盖。

这些能力可以在后续设计中基于当前 schema 继续扩展，但不进入本轮 Profiles/ProfileItem 模型。

## 4. 核心概念

### 4.1 Config

`Config` 表示“如何产生一个完整配置”。它可以被 `current` 选中。

```text
ConfigDefinition
├── File
└── Composition
```

- `FileConfig`：从一个 Local 或 Remote 本地物化文件解析配置；
- `CompositionConfig`：从 base seed 或 clean seed 开始，并从其他 Config 扩展 `proxies`。

### 4.2 Transform

`Transform` 表示“如何变换一个已经解析出的配置”。它不可被 `current` 选中。

```text
TransformDefinition
├── Overlay
└── Script
```

- `Overlay`：声明式 YAML overlay/patch，是旧 `Merge` 的新名称；
- `Script`：JS/Lua imperative transform。

Transform 是可复用 Profile。它可以出现在：

- `FileConfig.transforms`；
- `CompositionConfig.transforms`；
- `Profiles.global_transforms`。

### 4.3 CompositionConfig

`CompositionConfig` 是一个可被选中的 Config：

```rust
pub struct CompositionConfig {
    pub base: Option<ProfileId>,
    pub extend_proxies_from: Vec<ProfileId>,
    pub transforms: Vec<ProfileId>,
}
```

它的完整语义是：

```text
CompositionConfig = seed + extend_proxies_from + transforms

seed:
  Some(base) → 从 base 的 scoped result 继承完整配置
  None       → 从 clean config seed 开始

extend_proxies_from:
  从这些 FileConfig 的 scoped result 中提取 proxies/nodes 并追加

transforms:
  proxies 扩展完成后执行的后处理
```

这覆盖两个主要场景：

1. 兼容旧 `current` 列表：

   ```text
   base = Some(current[0])
   extend_proxies_from = current[1..]
   ```

2. 干净节点集合配置：

   ```text
   base = None
   extend_proxies_from = 多个订阅
   transforms = 构造 proxy-groups/rules/dns 的后处理
   ```

### 4.4 Source

Source 描述“谁负责维护本地可读内容”。

- `Local`：内容由用户、本地编辑或本地同步机制维护；
- `Remote`：内容由远程更新任务维护。

所有文件型 Profile 都通过 `MaterializedFile` 暴露统一的本地读取位置。解析和后处理代码不需要知道它来自 Local、Remote、Symlink 或 Mirror。

### 4.5 Local Binding

Local 来源有两种绑定方式：

- `Managed`：源文件本身位于应用管理目录；
- `External`：源文件位于外部绝对路径，并通过以下模式映射到管理目录：
  - `Symlink`：应用管理的本地入口是指向外部目标的符号链接；
  - `Mirror`：监听外部目标，并原子复制到本地入口。

`External` 只存在于 `ProfileSource::Local` 分支。`ProfileSource::Remote` 直接持有 `MaterializedFile`，因此类型上不存在 `Remote + External`。

## 5. 数据结构总览

```text
Profiles
├── current: Option<ProfileId>
├── global_transforms: Vec<ProfileId>
├── valid: Vec<String>
└── items: IndexMap<ProfileId, ProfileItem>

ProfileItem
├── uid
├── metadata
└── definition
    ├── Config
    │   ├── File
    │   │   ├── source: ProfileSource
    │   │   └── transforms: Vec<ProfileId>
    │   └── Composition
    │       ├── base: Option<ProfileId>
    │       ├── extend_proxies_from: Vec<ProfileId>
    │       └── transforms: Vec<ProfileId>
    └── Transform
        ├── Overlay
        │   └── source: ProfileSource
        └── Script
            ├── source: ProfileSource
            └── runtime: JavaScript | Lua

ProfileSource
├── Local
│   └── binding
│       ├── Managed
│       └── External
│           ├── target
│           └── mode: Symlink | Mirror
└── Remote
    ├── materialized file
    ├── url
    ├── update options
    └── subscription info
```

建议 Rust 命名：

```rust
pub struct Profiles {
    pub current: Option<ProfileId>,
    pub global_transforms: Vec<ProfileId>,
    pub valid: Vec<String>,
    pub items: IndexMap<ProfileId, ProfileItem>,
}

pub struct ProfileItem {
    pub uid: ProfileId,
    pub metadata: ProfileMetadata,
    pub definition: ProfileDefinition,
}

pub enum ProfileDefinition {
    Config { config: ConfigDefinition },
    Transform { transform: TransformDefinition },
}

pub enum ConfigDefinition {
    File(FileConfig),
    Composition(CompositionConfig),
}

pub struct FileConfig {
    pub source: ProfileSource,
    pub transforms: Vec<ProfileId>,
}

pub struct CompositionConfig {
    pub base: Option<ProfileId>,
    pub extend_proxies_from: Vec<ProfileId>,
    pub transforms: Vec<ProfileId>,
}

pub enum TransformDefinition {
    Overlay(OverlayTransform),
    Script(ScriptTransform),
}
```

## 6. 配置格式

### 6.1 完整示例

```yaml
current: clean-subscriptions

global_transforms:
  - global-fix

valid:
  - dns
  - unified-delay
  - tcp-concurrent

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
          user_agent: clash-nyanpasu
          with_proxy: false
          self_proxy: true
          update_interval_minutes: 120
        subscription:
          upload: 123
          download: 456
          total: 789
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
        option:
          with_proxy: true
          self_proxy: true
          update_interval_minutes: 120

  - uid: all-subscriptions
    name: All Subscriptions
    desc: Compatible migration of old multi-current selection
    type: config
    config:
      type: composition
      base: subscription-a
      extend_proxies_from:
        - subscription-b
      transforms:
        - finalize-all

  - uid: clean-subscriptions
    name: Clean Subscriptions
    desc: Clean config that only inherits nodes from subscriptions
    type: config
    config:
      type: composition
      extend_proxies_from:
        - subscription-a
        - subscription-b
      transforms:
        - build-groups
        - build-rules

  - uid: remote-overlay
    name: Remote Overlay
    type: transform
    transform:
      type: overlay
      source:
        type: remote
        file: remote-overlay.yaml
        url: https://example.com/overlay.yaml
        option:
          with_proxy: true
          self_proxy: true
          update_interval_minutes: 240

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

  - uid: build-groups
    name: Build Proxy Groups
    type: transform
    transform:
      type: overlay
      source:
        type: local
        binding:
          type: managed
          file: build-groups.yaml

  - uid: build-rules
    name: Build Rules
    type: transform
    transform:
      type: script
      runtime: lua
      source:
        type: local
        binding:
          type: external
          file: build-rules.lua
          target: /home/user/clash-scripts/build-rules.lua
          mode: mirror

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
      type: script
      runtime: lua
      source:
        type: local
        binding:
          type: managed
          file: global-fix.lua
```

### 6.2 兼容旧 current 列表语义

旧格式：

```yaml
current:
  - subscription-a
  - subscription-b
  - subscription-c
```

新格式：

```yaml
current: all-subscriptions

items:
  - uid: all-subscriptions
    name: All Subscriptions
    type: config
    config:
      type: composition
      base: subscription-a
      extend_proxies_from:
        - subscription-b
        - subscription-c
```

语义：

```text
subscription-a 提供完整配置
subscription-b / subscription-c 只贡献 proxies
```

### 6.3 干净节点集合配置

```yaml
- uid: clean-subscriptions
  name: Clean Subscriptions
  type: config
  config:
    type: composition
    extend_proxies_from:
      - subscription-a
      - subscription-b
      - subscription-c
    transforms:
      - build-proxy-groups
      - build-rules
      - finalize-clean-config
```

这里没有 `base`。运行时从 clean seed 开始：

```yaml
proxies: []
```

随后追加多个订阅的 nodes/proxies，并通过 transforms 生成最终配置结构。

## 7. 处理顺序

### 7.1 FileConfig 被 current 选中

```text
读取 source.materialized_file
→ 解析 Config
→ 执行 FileConfig.transforms
→ 执行 Profiles.global_transforms
→ finalize
```

### 7.2 FileConfig 作为 CompositionConfig 成员

```text
读取 source.materialized_file
→ 解析 Config
→ 执行 FileConfig.transforms
→ 返回 scoped result
```

该模式不会执行 `Profiles.global_transforms`，也不会执行最终 selected-config finalize。全局 transforms 只对最终被 current 选中的 Config 执行一次。

### 7.3 CompositionConfig with base

```text
解析 base，并执行 base 自身 transforms，得到 scoped base
→ 以 scoped base 作为工作配置
→ 解析 extend_proxies_from 中的每个成员，并执行各自 transforms，得到 scoped member
→ 从 scoped member 中提取 proxies/nodes
→ 按声明顺序追加到工作配置.proxies
→ 执行 CompositionConfig.transforms
→ 执行 Profiles.global_transforms
→ finalize
```

### 7.4 CompositionConfig without base

```text
创建 clean config seed
→ 解析 extend_proxies_from 中的每个成员，并执行各自 transforms，得到 scoped member
→ 从 scoped member 中提取 proxies/nodes
→ 按声明顺序追加到 clean seed.proxies
→ 执行 CompositionConfig.transforms
→ 执行 Profiles.global_transforms
→ finalize
```

clean seed 首版建议保持最小：

```yaml
proxies: []
```

不要隐式注入 `proxy-groups`、`rules`、`dns`、`tun` 等字段。这些字段应由 transforms 显式生成或覆盖。

### 7.5 `extend_proxies_from` 规则

`extend_proxies_from` 的第一版规则固定如下：

1. 若 `base = Some(id)`，工作配置从 base scoped result 开始；
2. 若 `base = None`，工作配置从 clean seed 开始；
3. 保留工作配置已有的所有字段；
4. 保留工作配置已有的 `proxies`；
5. 按 `extend_proxies_from` 声明顺序处理成员；
6. 每个成员先作为 scoped FileConfig 完成解析和自身 transforms；
7. 只从成员 scoped result 中提取 `proxies` / nodes；
8. 将成员节点依次追加到工作配置的 `proxies`；
9. 不合并成员的其他字段；
10. 不在 `extend_proxies_from` 层去重、覆盖或重命名同名节点；
11. 重复节点的最终处理继续沿用当前 Clash 配置验证/后处理行为。

该规则可以无损表达旧 `current: [a, b, c]`，同时新增 clean seed + nodes + transforms 的配置方式。

## 8. 引用约束

### 8.1 Current

`Profiles.current` 类型为：

```rust
Option<ProfileId>
```

只有 `ProfileDefinition::Config` 可以被选中，包括 File 和 Composition。Transform 不能成为 current。

### 8.2 Global transforms

`Profiles.global_transforms` 只能引用命名 Transform：

```text
ProfileDefinition::Transform(Overlay | Script)
```

Config 不能被放入 global transforms。

### 8.3 Config transforms

`FileConfig.transforms` 和 `CompositionConfig.transforms` 只能引用命名 Transform。

允许同一个 Transform Profile 在同一 transforms 数组中出现多次；首版不引入 StepId，所以重复引用的定位仍然通过数组 index 处理。

### 8.4 CompositionConfig 成员

初始版本中，`CompositionConfig.base` 和 `CompositionConfig.extend_proxies_from` 必须引用直接文件型 Config：

```text
ProfileDefinition::Config(ConfigDefinition::File { ... })
```

`base` 为 `None` 时跳过 base 引用检查。

禁止引用：

- 不存在的 Profile；
- Transform；
- 自己；
- 另一个 CompositionConfig；
- 重复 contributor；
- 当 `base = Some(id)` 时，同时把同一个 id 放入 `extend_proxies_from`。

该限制避免递归、循环、重复执行 transforms 和难以解释的代理顺序。未来若需要嵌套 CompositionConfig，可以在不改变持久化字段的情况下放宽验证并增加循环检测。

### 8.5 空 CompositionConfig

允许以下高级形式：

```yaml
config:
  type: composition
  transforms:
    - generate-everything
```

它表示从 clean seed 开始，并完全依赖 transforms 生成最终配置。

但以下配置应作为 validation error：

```yaml
config:
  type: composition
```

即：

```text
base = None
extend_proxies_from = []
transforms = []
```

因为它只能得到一个没有意义的空配置。

## 9. Source 与任务调度

文件型 Profile 包括：

- `ConfigDefinition::File`；
- `TransformDefinition::Overlay`；
- `TransformDefinition::Script`。

`CompositionConfig` 不对应单个物化文件，也不产生远程更新任务。

每次 Profile 集合提交后，调度器执行幂等 reconcile：

```text
遍历所有文件型 Profile
→ ProfileSource::Remote：upsert profile-update:{uid}
→ 其他 Source：移除同 uid 的远程任务
```

远程更新流程：

```text
下载到同目录临时文件
→ 根据目标 Profile 类型校验内容
→ 原子替换 materialized file
→ 更新 updated_at 与 subscription
→ 查询依赖索引
→ 必要时重建当前运行配置
```

Remote 更新器写入前还必须检查目标不是意外的符号链接。类型系统只能阻止配置模型中的 `Remote + External`，不能阻止用户在应用外部篡改文件系统状态。

## 10. 本地同步

### 10.1 External Symlink

- 创建或修复 `materialized.file -> target`；
- 监听 target 或解析后的真实路径；
- 内容变化后触发依赖失效；
- 删除 Profile 时只删除应用管理的链接，不删除 target。

### 10.2 External Mirror

- 监听 target；
- 变化后复制到同目录临时文件；
- 校验后原子替换 materialized.file；
- 删除 Profile 时删除应用管理副本，不删除 target。

## 11. 时间字段

公共 metadata 不再包含旧 `updated` 字段，因为 CompositionConfig 没有自己的物化文件。

文件型 Profile 在 `MaterializedFile` 中保存：

```rust
updated_at: Option<OffsetDateTime>
```

含义是“该本地物化文件最后一次成功产生有效内容的时间”。

CompositionConfig 的有效更新时间运行时计算：

```text
max(base.updated_at?, extend_proxies_from[*].updated_at)
```

当 `base = None` 时，只根据 `extend_proxies_from` 计算；若也没有 contributors，则该值为 `None` 或由运行时生成状态表达，不持久化。

Transform 文件更新时间只用于依赖失效和 UI 展示。

## 12. 路径约束

使用不同 newtype 区分两类路径：

- `ManagedProfilePath`：必须是相对于应用 Profile 目录的规范相对路径，禁止绝对路径、`.` 和 `..`，并禁止 URL；
- `ExternalProfilePath`：必须是绝对路径。

同一份 `Profiles` 中，每个文件型 Profile 的 `ManagedProfilePath` 必须唯一，避免多个远程任务、Mirror 任务或编辑入口同时写入同一文件。外部 target 可以共享，但本地物化入口不能共享。

所有文件解析函数只接受 `ManagedProfilePath`。外部路径只允许交给 Link/Mirror 管理器，从 API 层避免把外部目标误交给 Remote 更新器。

Remote URL 仅允许 HTTP/HTTPS。

## 13. Sanitize 与 Validation

### 13.1 Sanitize

`sanitize_top_level` 只修复可安全恢复的顶层引用：

- current 不存在或不是 Config：清空；
- global transforms 目标不存在或不是 Transform：移除；
- 返回第一个可激活 Config 作为默认候选。

### 13.2 Validation

以下错误不应静默修改，而应由 `validate` 报告：

- Profile map key 与 item.uid 不一致；
- current 指向缺失 Profile；
- current 指向 Transform；
- global transforms 或 Config transforms 指向缺失 Profile；
- transforms 指向 Config；
- `CompositionConfig.base = Some(id)` 时，base 缺失、类型错误、自引用或引用 CompositionConfig；
- `extend_proxies_from` 成员缺失、重复、自引用、引用 Transform 或引用 CompositionConfig；
- `base = Some(id)` 且 contributor 中也包含该 id；
- `base = None`、`extend_proxies_from = []` 且 `transforms = []`；
- 多个文件型 Profile 使用同一 `ManagedProfilePath`；
- Remote URL 不是 HTTP/HTTPS；
- Remote 更新间隔为 0。

自动删除这些引用会改变用户配置语义，因此不得在 sanitize 中执行。

## 14. Migration

新领域类型不接受任何旧格式。Migration 直接处理 YAML/JSON Value，并在完成后反序列化为新 `Profiles`。

### 14.1 旧 Profile 类型映射

| 旧 `type` | 新定义                | 默认来源          |
| --------- | --------------------- | ----------------- |
| `local`   | `Config / File`       | `Local / Managed` |
| `remote`  | `Config / File`       | `Remote`          |
| `merge`   | `Transform / Overlay` | `Local / Managed` |
| `script`  | `Transform / Script`  | `Local / Managed` |

字段移动：

```text
old top-level chain
    → Profiles.global_transforms

local.chain / local.chains
remote.chain / remote.chains
    → ConfigDefinition::File.transforms

script.script_type
    → TransformDefinition::Script.runtime

old file + old updated
    → MaterializedFile { file, updated_at }

remote.url / option / extra
    → ProfileSource::Remote { url, option, subscription }
```

`extra.expire: 0` 在 migration 中转换成 `None`。`update_interval` 在 migration 中重命名为 `update_interval_minutes`。新类型中不保留 alias。

### 14.2 旧 `file` 为 URL

对于旧 Local/Merge/Script，若 `file` 是 HTTP/HTTPS URL：

1. 新定义按旧 `type` 决定：Config/File、Transform/Overlay 或 Transform/Script；
2. Source 迁移为 Remote；
3. URL 移入 `source.url`；
4. 根据 uid 和定义类型生成本地 `ManagedProfilePath`；
5. 使用默认 Remote options，除非旧数据提供了相应字段。

URL 识别只存在于 migration，不能出现在新 `ManagedProfilePath` 反序列化中。

### 14.3 Current

```text
旧 current 缺失或 = []
    → current = None

旧 current = a 或 [a]
    → current = Some(a)

旧 current = [a, b, c]
    → 新建 CompositionConfig：
       base = Some(a)
       extend_proxies_from = [b, c]
       transforms = []
       current = 新 CompositionConfig uid
```

顺序必须原样保留。新 CompositionConfig 使用碰撞安全的唯一 uid 和可识别名称，例如 `Combined Profile`。迁移完成后它与普通用户创建的 CompositionConfig 完全相同。

若旧 current 中任一成员不能迁移为直接 FileConfig，migration 必须明确失败，不能悄悄丢弃。

`base = None` 是新 schema 的新增表达能力。旧格式没有等价语义，因此 migration 不会自动生成无 base 的 CompositionConfig，除非后续发现旧数据中存在明确表达“只继承节点、无完整 base”的配置来源。

### 14.4 执行顺序

```text
读取旧文档为 Value
→ 执行 schema migration
→ 反序列化为新 Profiles
→ 执行 referential validation
→ 验证所有文件路径和远程间隔
→ 原子写回
→ 重建依赖索引、调度器和文件监听器
```

任何无法安全迁移的情况都应返回带 Profile uid 和字段路径的 migration error，不得把无效组合带入领域模型。

## 15. Mutation API

不建议对整个 `ProfileDefinition` 生成细粒度 `Patch`。枚举 variant 变化应当是原子替换：

- metadata 可继续使用 `ProfileMetadataPatch`；
- Remote options 可继续使用 `RemoteProfileOptionsPatch`；
- Config kind、Transform kind、Source 或 Binding 的变化使用完整 `ProfileDefinition` / `ProfileSource` 替换；
- `transforms` 和 `extend_proxies_from` 支持按 `ProfileId` 增删改重排；
- uid 不可 patch。

推荐事务流程：

```text
复制当前 Profiles
→ 应用修改
→ validate
→ 计算 scheduler/watcher diff
→ 原子持久化
→ 提交内存状态
→ reconcile runtime services
```

这避免在 `Local → Remote`、`FileConfig → CompositionConfig`、`Overlay → Script` 等切换过程中产生临时非法状态。

## 16. 依赖索引

依赖索引是运行时派生数据，不持久化：

```rust
pub struct ProfileDependencyIndex {
    pub composition_base_dependents:
        HashMap<ProfileId, IndexSet<ProfileId>>,
    pub extend_proxies_dependents:
        HashMap<ProfileId, IndexSet<ProfileId>>,
    pub transform_dependents:
        HashMap<ProfileId, IndexSet<ProfileId>>,
    pub global_transform_ids:
        IndexSet<ProfileId>,
}
```

用途：

- 某个 FileConfig 更新时，定位依赖它作为 base 的 CompositionConfig；
- 某个 FileConfig 更新时，定位从它扩展 proxies 的 CompositionConfig；
- 某个 Transform 更新时，定位 transforms 使用者；
- global transforms 中的 Transform 更新时，当前配置总是失效；
- 当前 Config 本身或其任一传递依赖更新时，重建运行配置。

`base = None` 时不建立 base 依赖，只建立 `extend_proxies_from` 依赖。

索引应在加载完成以及每次 Profile 事务提交后重建。当前规模下全量重建更简单可靠。

## 17. 删除策略

默认删除操作必须拒绝删除以下 Profile：

- 当前被 `current` 选中；
- 被 `CompositionConfig.base` 引用的 Config；
- 被 `CompositionConfig.extend_proxies_from` 引用的 Config；
- 被 `FileConfig.transforms` 或 `CompositionConfig.transforms` 引用的 Transform；
- 被 `Profiles.global_transforms` 引用的 Transform。

需要级联时应使用显式事务，并把所有引用变更作为同一次持久化提交；不得静默从 `extend_proxies_from` 或 transforms 中移除成员。

## 18. 测试要求

至少覆盖以下测试：

1. FileConfig、Overlay、Script 均能使用 Local/Remote 文件来源；
2. Rust 类型中无法表示 `Remote + External`；
3. Managed 路径拒绝绝对路径、路径穿越和 URL；
4. External 路径拒绝相对路径；
5. 两个文件型 Profile 使用同一物化路径时验证失败；
6. Remote URL 使用非 HTTP/HTTPS scheme 时验证失败；
7. current 只接受单个 ProfileId；
8. current 指向 Transform 时验证失败或被 sanitize；
9. global transforms 指向 Config 时验证失败或被 sanitize；
10. FileConfig.transforms 指向 Config 或缺失 Transform 时验证失败；
11. CompositionConfig.base = Some 时，base 缺失、自引用、引用 Transform 或引用 CompositionConfig 时验证失败；
12. CompositionConfig.base = None 时，不执行 base 引用验证；
13. extend_proxies_from 缺失成员、重复、自引用、引用 Transform 或引用 CompositionConfig 时验证失败；
14. base = Some(id) 且 extend_proxies_from 包含 id 时验证失败；
15. base = None、extend_proxies_from 为空且 transforms 为空时验证失败；
16. base = None、extend_proxies_from 为空但 transforms 非空时允许；
17. extend_proxies_from 追加顺序与旧 current 列表一致；
18. base transforms 在 proxies 扩展前执行；
19. extend_proxies_from 成员 transforms 在提取 proxies 前执行；
20. CompositionConfig.transforms 在 proxies 扩展后执行；
21. global transforms 只执行一次；
22. Remote task reconcile 支持新增、修改、Local/Remote 切换和删除；
23. Symlink/Mirror 更新能使依赖 CompositionConfig 失效；
24. 多 current migration 生成正确 `base = Some(first)` CompositionConfig；
25. clean seed CompositionConfig 可以只从多个 profiles 继承 nodes 并通过 transforms 生成最终配置；
26. 旧 `file: URL` 只在 migration 中转换；
27. 重复 uid 反序列化失败，不再静默保留第一项；
28. 删除被引用 Profile 默认失败。

## 19. 最终决策摘要

最终模型为：

```text
Profile = immutable uid
        + metadata
        + Config | Transform

Config = File(source, transforms)
       | Composition(base: Option<ProfileId>, extend_proxies_from, transforms)

Composition seed:
  Some(base) → inherit full scoped config from base
  None       → start from clean config seed

Transform = Overlay(source)
          | Script(source, runtime)

File-backed profile = Config::File | Transform::Overlay | Transform::Script
File-backed source = Local(binding) | Remote(materialized_file, url, update options)
Local = Managed | External(Symlink | Mirror)

Profiles.current = Option<ProfileId>
Profiles.global_transforms = Vec<ProfileId>
```

该模型保留了参考方案中最重要的抽象：Config/Transform 二分、Overlay 命名、current 单值、Remote 物化文件、Local Binding，以及通过一等 Config 表达多订阅节点扩展；同时去掉本轮不需要的 target、StepId、Pipeline IR 和 Snapshot 复杂度。`CompositionConfig.base = None` 则补充了“干净配置 + 只继承节点 + transforms 重写最终结构”的使用场景。
