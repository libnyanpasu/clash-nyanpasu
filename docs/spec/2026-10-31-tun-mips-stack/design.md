# TUN mips 栈与内核能力查询

**日期：** 2026-10-31

**状态：** 设计已确认，待实施计划。

**基线：** main `9fa0cc568`。

**需求：** [#6091](https://github.com/libnyanpasu/clash-nyanpasu/issues/6091)，在“设置 → 端口与网络 → TUN 堆栈”中提供 mihomo 新增的 mips 栈。同时提供 mips 栈的拥塞控制设置，并让 TUN 栈选项按当前内核的能力显示。

**依据：** [内核特有配置设计](../../design/core-specific-config.md)（能力表、三种值的处理、兄弟字段约定）。

**权威顺序：** 当前 AGENTS.md 与 development guides > 内核特有配置设计 > 本 spec > 实施计划。

## 1. 决策

1. TUN 栈新增 `Mips`。界面显示为 “Mips”，说明文字注明它是 Mihomo IP Stack（mihomo 自研的用户态网络栈），与 MIPS 处理器架构无关。
2. 拥塞控制作为 mips 栈的子配置，存成与 `tun_stack` 平级的 `tun_mips: MipsConfig`。`congestion_controller` 是 `Option`，`None` 表示不写这个键，由内核使用默认值（cubic）。
3. 能力表只按内核类型判断，不看版本。
4. `tun.stack` 与 `tun.congestion-controller` 改由 Nyanpasu 设置优先（`Ownership::Forced`）。现在 `stack` 是 `Ownership::Default`，订阅写了就用订阅的值。本次不引入 `ManageableField`，不提供“交给订阅”的选项。
5. Nyanpasu 自己的栈设置在当前内核不支持时，降级为 gvisor，并在构建日志里记一条警告；用户保存的设置不改。
6. 来自订阅或转换脚本、Nyanpasu 认识但当前内核不支持的值，拒绝构建并给出诊断；不认识的值放行。
7. 校验不论 TUN 是否开启都执行。
8. `clash-config.yaml` 的文件戳 revision 从 2 升到 3，使旧版 Nyanpasu 明确拒绝读取新文件，而不是在 `tun_stack: mips` 上解析失败，或在重写文件时丢掉 `tun_mips`。
9. Nyanpasu 的默认栈仍是 gvisor，不跟随 mihomo v1.19.32 改为 mips。

## 2. 上游事实

| 内核                     | `stack`                                                                             | `congestion-controller`                                                                                                          | 依据                                                                                                                                                                                           |
| ------------------------ | ----------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| mihomo、mihomo-alpha     | gvisor、system、mixed、mips，不区分大小写；v1.19.31 加入 mips，v1.19.32 起默认 mips | v1.19.32 加入；cubic、reno、bbr、bbr3，不区分大小写；只有 mips 栈读取，未知值使 TUN 启动失败（`invalid TCP congestion control`） | `MetaCubeX/mihomo@v1.19.33` `constant/tun.go`、`config/config.go:273-276,548`、`docs/config.yaml:136,153`；提交 `ab405bad`、`63bd52ec`；`metacubex/sing-tun@v0.4.28` `stack_mipstack.go:48-50` |
| Clash Premium            | system、gvisor                                                                      | 不读                                                                                                                             | 闭源，沿用现有 `derive_tun_flavor` 把 Mixed 降为 Gvisor 的处理，无法再核对源码                                                                                                                 |
| clash-rs、clash-rs-alpha | 没有这个键，静默忽略                                                                | 不读                                                                                                                             | `Watfaq/clash-rs` `clash-lib/src/config/def.rs:51`                                                                                                                                             |
| meow、meow-alpha         | 只有 lwIP；能解析，但打 warn 后忽略                                                 | 不读                                                                                                                             | `meow-rs/meow-rs` `crates/meow-config/src/raw.rs:339-383`、`crates/meow-listener/src/tun/mod.rs:800`                                                                                           |

mihomo 在反序列化整份配置时就解析 `tun.stack`，与 `tun.enable` 无关（`config/config.go:462,618`）。Nyanpasu 下发的 mihomo 为 v1.19.33（`manifest/version.json`）。

## 3. 现状

| 事实                                                                                                                                                     | 位置                                                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `TunStack` 只有 `System`、`Gvisor`（默认）、`Mixed`                                                                                                      | `backend/nyanpasu-config/src/clash/config/tun_stack.rs`                                                         |
| `ClashConfig.tun_stack` 是普通字段，没有 `#[patch(nesting)]`                                                                                             | `backend/nyanpasu-config/src/clash/config/mod.rs:85`                                                            |
| `derive_tun_flavor` 只让 `ClashRs` 走 clash-rs 分支；clash-rs-alpha 与 meow 走 Standard 分支，被写入 `stack`；Premium 的 Mixed 降为 Gvisor，没有任何提示 | `backend/nyanpasu-core/src/enhance/runtime_builder.rs:109`                                                      |
| TUN 开启时 `stack` 以 `Ownership::Default` 写入；TUN 关闭时只强制 `tun.enable: false`，订阅的 `tun.stack` 原样保留                                       | `backend/nyanpasu-config/src/runtime/executor/plan.rs:52,93,101`                                                |
| 内置 `ManagedSettings`（Forced）先于默认值与内置转换执行                                                                                                 | `backend/nyanpasu-config/src/runtime/executor/mod.rs:319-330`                                                   |
| 判断修改是否影响运行时：`of_clash` 看 `tun_stack` 是否出现在 patch 中；`ClashRuntimeInputs` 存 `tun_stack`                                               | `backend/tauri/src/client/application_workflow/impact.rs:171,340,356`                                           |
| `clash-config.yaml` 文件戳 revision 为 2，迁移步骤为 `MigrateManageableGuardFields`、`MigrateTunOverrides`；读取器只接受自己的 revision                  | `backend/tauri/src/core/migration/modules/clash_config.rs:34,45,259`；`backend/nyanpasu-core/src/format.rs:196` |
| 前端写死 System、gVisor、Mixed，只对 Premium 去掉 Mixed；clash-rs 与 meow 下照常显示但不起作用                                                           | `frontend/nyanpasu/src/pages/(main)/main/settings/clash/network/_modules/tun-stack-selector.tsx:33,43`          |

## 4. 设计

### 4.1 设置类型（`nyanpasu-config`）

```rust
#[derive(/* existing */, strum::VariantArray, strum::EnumString)]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum TunStack {
    System,
    #[default]
    Gvisor,
    Mixed,
    Mips,
}

/// mihomo `tun.congestion-controller`, read only by the mips stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type,
         strum::AsRefStr, strum::VariantArray, strum::EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case", ascii_case_insensitive)]
pub enum TunCongestionController {
    Cubic,
    Reno,
    Bbr,
    Bbr3,
}

/// Options of the mips stack, kept while another stack is selected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type, Patch)]
pub struct MipsConfig {
    /// `None` writes no key, so the core uses its own default.
    pub congestion_controller: Option<TunCongestionController>,
}

pub struct ClashConfig {
    // ...
    pub tun_stack: TunStack,
    #[serde(default)]
    #[patch(nesting)]
    pub tun_mips: MipsConfig,
}

/// The selected stack with only its own options reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SelectedTunStack<'a> {
    System,
    Gvisor,
    Mixed,
    Mips(&'a MipsConfig),
}

impl ClashConfig {
    pub fn selected_tun_stack(&self) -> SelectedTunStack<'_>;
}
```

- `TunStack` 的存储格式仍是字符串；`tun_stack: mips` 可以手写。
- `MipsConfigPatch` 的 `congestion_controller` 由 struct-patch 生成两层 `Option`，前端可以把它改回 `None`（内核默认）。
- 实施时用测试确认 `Bbr3` 序列化为 `bbr3`。

### 4.2 能力表

按设计文档 §3 定义 `FieldSupport<T>`、`ConfigFeatures`、`ClashCore::config_features()`，本次只有 TUN 一个领域：

```rust
pub struct TunFeatures {
    pub stack: FieldSupport<TunStack>,
    pub congestion_controller: FieldSupport<TunCongestionController>,
}
```

| 内核                                       | `stack`                     | `congestion_controller` |
| ------------------------------------------ | --------------------------- | ----------------------- |
| mihomo、mihomo-alpha                       | system、gvisor、mixed、mips | cubic、reno、bbr、bbr3  |
| Clash Premium                              | system、gvisor              | `Ignored`               |
| clash-rs、clash-rs-alpha、meow、meow-alpha | `Ignored`                   | `Ignored`               |

### 4.3 配置生成

`derive_tun_flavor` 改为根据 `TunFeatures` 与 `SelectedTunStack` 决定写入什么，不再按内核名判断 `stack`：

| 能力表中的 `stack`       | 写入                                                                                                            |
| ------------------------ | --------------------------------------------------------------------------------------------------------------- |
| `Ignored`                | 不写 `stack`，也不写 `congestion-controller`                                                                    |
| `Values`，包含所选的栈   | `stack` 以 `Forced` 写入所选的栈                                                                                |
| `Values`，不包含所选的栈 | `stack` 以 `Forced` 写入 gvisor；在 `ManagedSettings` 步骤日志中记 `Warn`，说明所选的栈、当前内核和实际使用的栈 |

实际写入的栈为 mips 时，`congestion-controller` 也由 Nyanpasu 决定：`congestion_controller` 为 `Some` 且能力表包含该值时以 `Forced` 写入；为 `None` 时删除订阅里的这个键，使“内核默认”确实是内核的默认值。其余栈不读这个键，订阅里的值原样保留，由 §4.4 校验。

`TunFlavor::ClashRs` 写 `device-id`，Standard 分支写 `dns-hijack`、`auto-detect-interface`，这部分按内核选择分支的旧行为不变。clash-rs-alpha 与 meow 因此仍走 Standard 分支，只是不再写入 `stack`。

### 4.4 校验

`RuntimeBuilder::build` 拿到最终配置后，用当前内核的 `ConfigFeatures` 校验，不论是否严格模式，也不论 TUN 是否开启：

| 键                          | 何时检查                                          | 拒绝条件                                                         |
| --------------------------- | ------------------------------------------------- | ---------------------------------------------------------------- |
| `tun.stack`                 | 能力为 `Values`                                   | 值是字符串，能不区分大小写地解析为 `TunStack`，且不在支持列表中  |
| `tun.congestion-controller` | 能力为 `Values`，且最终的 `tun.stack` 解析为 mips | 值是字符串，能解析为 `TunCongestionController`，且不在支持列表中 |

解析不出来的值放行。拒绝时返回 `RuntimeBuildError` 的新变体，按 snafu 规范以失败步骤命名，携带每个不支持的值。变体不带内核：`ClashCore` 带有 serde 别名，序列化与反序列化的形状不同，放进错误会让 specta 把整个 `IpcError` 拆成 `_Serialize`、`_Deserialize` 两套类型；用户看到这条错误时刚应用过当前内核，`supported` 已给出可用的值。

```rust
#[snafu(display("the selected core does not support {values:?}"))]
UnsupportedConfigValues { values: Vec<UnsupportedConfigValue> },

pub struct UnsupportedConfigValue {
    /// Dotted path in the final config, e.g. `tun.stack`.
    pub path: String,
    pub value: String,
    pub supported: Vec<String>,
}
```

因为决策 4，Nyanpasu 在 TUN 开启时会覆盖订阅的 `tun.stack`，所以 `stack` 的拒绝主要发生在 TUN 关闭、订阅自带 `tun.stack` 的情况，例如订阅为 mihomo 写了 `stack: mips`，当前内核是 Premium。

### 4.5 运行时输入

- `ClashRuntimeInputs` 的 `tun_stack: TunStack` 改为 `tun_stack: SelectedTunStack<'a>`。未选中 mips 时修改 `tun_mips`，运行时输入不变，不触发重建。
- `of_clash` 增加 `!patch.tun_mips.is_empty()`。被它列入的字段，即使值没变也会让运行时重新调整一次，这样重新提交一个上次没有生效的拥塞控制仍会重试。未选中 mips 时修改 `tun_mips` 因此也会调整一次运行时；界面只在选中 mips 时提供这项设置，这种情况只来自直接调用 API。

### 4.6 迁移

- `ClashConfigDocument::SCHEMA_REVISION` 改为 3。
- 新增步骤 `MigrateTunMips`（id `clash_config/tun_mips`，revision 3，`introduced_in` 为 2.0.0：迁移器只执行 `introduced_in` 不高于当前应用版本的步骤，标成尚未发布的版本会让读取器先要求 revision 3 而步骤不执行），写法同 `add_tun_overrides`：文件里没有 `tun_mips` 时写入空映射，已有则不动，然后以 revision 3 写回。
- `read_typed` 按文件的 revision 逐步补齐，每个步骤各有一处调用，因此同样加上 `add_tun_mips`。

### 4.7 RPC

- `NyanpasuClient` 新增读取当前内核能力的方法，返回 `app.core.config_features()`。
- 新增查询命令 `get_config_features`，按 [Unified RPC](../../development/rpc.md) 声明，在 `specta_export.rs` 注册为 query，HTTP 可用性与现有配置读取命令一致；重新生成绑定。
- 前端在应用配置变化的状态事件到来时让该查询失效，以便切换内核后刷新。

### 4.8 前端

- 选择器读取 `get_config_features`：`stack` 为 `ignored` 时隐藏整张卡片；为 `values` 时只列出这些栈。删除 `coreType.value === 'clash'` 的判断与写死的选项。
- 选中 mips 且 `congestion_controller` 为 `values` 时，在下方显示拥塞控制下拉：“内核默认”加上支持的取值。修改时只发送 `{ tun_mips: { congestion_controller } }`。
- 新增文案走 paraglide，修改消息后按仓库流程重新编译 i18n。

## 5. 测试

| 位置                     | 内容                                                                                                                                                   |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `nyanpasu-config` 设置   | 旧文件 `tun_stack: gvisor` 无 `tun_mips` 时读出默认值；`tun_stack: mips` 可读；局部修改 `congestion_controller`；切走再切回后子配置保留；`Bbr3` 序列化 |
| `nyanpasu-config` 能力表 | 每个内核一行，断言 `config_features()` 的完整结果                                                                                                      |
| 校验函数                 | 拒绝已知不支持的值；放行未知值；`Ignored` 不检查；大小写不敏感；`congestion-controller` 只在最终栈为 mips 时检查；TUN 关闭时同样检查                   |
| `runtime_builder`        | mihomo 下 mips 与拥塞控制被强制写入并覆盖订阅值；Premium 下 mips 降为 gvisor 并有 `Warn` 日志；clash-rs-alpha 与 meow 不再写入 `stack`                 |
| `impact`                 | 栈不是 mips 时修改 `tun_mips` 不改变运行时输入；栈是 mips 时改变                                                                                       |
| 迁移                     | revision 2 文件迁移到 3 后包含 `tun_mips`；已有 `tun_mips` 不被覆盖；revision 3 读取器拒绝 revision 2 文件                                             |
| 前端                     | 选择器按 `values` 渲染、`ignored` 时隐藏；mips 下显示拥塞控制；修改只发送局部 patch                                                                    |

## 6. 用户可见的行为变化

1. TUN 开启时，界面上的栈设置总是生效，订阅里的 `tun.stack` 不再覆盖它。此前依赖订阅指定栈的用户，会改用界面上的设置（默认 gvisor）。需写进发布说明。
2. 订阅或转换脚本写入当前内核不支持的栈时，构建失败并提示原因。此前这类配置会被交给内核，由内核报错或忽略。
3. Premium 下选择 mips 或 mixed 时，构建日志会出现降级警告。此前 mixed 被静默降级。
4. clash-rs-alpha 与 meow 的运行时配置不再包含 `tun.stack`，两者原本就忽略这个键。
5. 降级到旧版 Nyanpasu 时，旧版按文件戳拒绝读取 `clash-config.yaml`。

## 7. 不在范围

- 按内核版本判断能力，以及“读取版本时与实际启动的二进制不一致”的问题（`backend/tauri/src/utils/core_version.rs:18` 读取安装目录，启动优先使用数据目录），另行处理。
- 跟随上游把默认栈改为 mips，或提供“交给内核决定”的选项。
- 把 `tun_stack` 改为 `ManageableField`。
- `TunFlavor` 其余按内核分支的行为（`device-id`、`dns-hijack`）和内置脚本的 `MIHOMO_FAMILY`、`CLASH_RS_ONLY` 分组改用能力表。

## 8. 风险

- Clash Premium 闭源，“只支持 system、gvisor”沿用现有代码中的结论，无法重新核对。Premium 已在弃用流程中。
- 决策 4 改变了订阅与界面设置的优先级，是本次最大的行为变化。
