# 内核特有配置：能力表与变体配置保留 设计

**日期：** 2026-10-31
**基线：** `main @ 9fa0cc568`
**来源：** [#6091](https://github.com/libnyanpasu/clash-nyanpasu/issues/6091) 请求 mihomo 新增的 `tun.stack: mips`。讨论中确认，`ClashConfig` 里会有越来越多的设置只被部分内核读取，或者只在某个取值下才有意义。
**范围：** 两条约定。一是 Nyanpasu 如何回答“当前内核读不读某个配置键、接受哪些值”，供配置生成、校验与界面共用；二是“某个取值才有意义的子配置”如何存储，使切换取值不丢失子配置。
**不在范围：** 具体字段的实现，第一个落地见 [TUN mips 栈 spec](../spec/2026-10-31-tun-mips-stack/design.md)。

---

## 0. 结论摘要

1. **能力表只按内核类型判断，不看版本。** Nyanpasu 只为自己下发的内核负责；用户自行替换的内核不在考虑范围内，因此不探测版本，也不维护版本下限。
2. **能力表是一组领域结构体，由 `ConfigFeatures` 汇总。** 每个领域一个结构体（例如 `TunFeatures`），字段统一用 `FieldSupport<T>` 表示“这个键内核读不读、接受哪些值”。`ClashCore::config_features()` 是唯一的来源。
3. **能力判断写成没有通配的穷尽 `match`。** 按（内核, 取值）逐格决定；新增取值或新增内核都会编译失败，迫使作者逐格判断。不写“全部支持”或黑名单，因为两者都会在取值枚举扩张时默认放行。
4. **三种值分开处理。** Nyanpasu 写入的值遇到不支持时按固定规则降级，并在构建日志里记一条警告；来自订阅或转换脚本的已知不支持值，拒绝构建并给出结构化诊断；Nyanpasu 不认识的值放行，由内核自己判断。
5. **只在某个取值下才有意义的子配置，存成兄弟字段。** 取值仍是纯标签枚举，子配置是与它平级、有默认值、可局部修改的字段；读取方只通过一个“当前选中视图”访问。切换取值不丢子配置。
6. **前端不保存能力知识。** 后端以查询 RPC 返回当前内核的 `ConfigFeatures`，前端据此决定显示哪些选项。

## 1. 问题

| 事实                                                                                                         | 位置                                                                                                          |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------- |
| 按内核分支散落在配置生成里：`derive_tun_flavor` 判断内核名，内置脚本按 `MIHOMO_FAMILY`、`CLASH_RS_ONLY` 分组 | `backend/nyanpasu-core/src/enhance/runtime_builder.rs:63,70,109`                                              |
| 前端自己写死了选项和内核判断（只对 Premium 去掉 mixed）                                                      | `frontend/nyanpasu/src/pages/(main)/main/settings/clash/network/_modules/tun-stack-selector.tsx:33,43`        |
| 不读某个键的内核表现不同：clash-rs 的 `TunConfig` 静默忽略未知键；meow 能解析 `stack` 但只打 warn 后忽略     | `Watfaq/clash-rs` `clash-lib/src/config/def.rs:51`；`meow-rs/meow-rs` `crates/meow-config/src/raw.rs:339-383` |
| mihomo 反序列化整份配置时就解析 `tun.stack`（类型 `C.TUNStack`），未知值使整份配置失败，与 `tun.enable` 无关 | `MetaCubeX/mihomo@v1.19.33` `config/config.go:273-276,462,618`；`constant/tun.go`                             |
| 带数据的枚举在切换变体时丢弃旧变体的数据；struct-patch 修改时整体替换枚举值                                  | 验证 crate `enum_yaml_and_atomic_replacement`（§5.3）                                                         |

## 2. 被否决的方案

| 方案                                                   | 否决理由                                                                                                                                                                                      |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 每个设置类型实现一个 `trait Cap`                       | 能力知识散落到各个类型，没有一处能看到某个内核支持什么，也难以整体导出给前端。只有一个使用者的 trait 也不符合 [Rust 代码指南](../development/rust.md) 对 trait 的要求。                       |
| 运行时探测（`-t` 校验配置，或写进去看是否报错）        | clash-rs 与 meow 会忽略不认识的键，探测会把“被忽略”误判为“支持”。                                                                                                                             |
| 并入 `nyanpasu-core-metadata` 的 `Feature`             | 该枚举描述 core-manager 拉起进程所需的控制通道能力，使用者不同；它在 runtime 子模块里，每加一个配置字段都要走 runtime 发版和主仓改 pin；加入 TUN 项还会改变 core-manager 是否探测版本的判断。 |
| 按版本判断能力                                         | 需要在构建配置前确定要启动的二进制并探测版本，而现在先构建配置、后解析 `CoreSpec`；Nyanpasu 不为用户替换的内核负责，收益不抵成本。                                                            |
| 每个内核一套独立配置（`mihomo: {…}`、`clash_rs: {…}`） | 目前读 `stack` 的只有 mihomo 系。为一个字段引入按内核分组的结构和迁移，过度设计。                                                                                                             |

## 3. 能力表

### 3.1 类型

```rust
/// Whether a core reads one config key, and which values it accepts there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FieldSupport<T> {
    /// The core does not read the key: Nyanpasu writes none and checks none.
    Ignored,
    /// The core reads the key and fails on any value outside this list.
    Values(Vec<T>),
}

/// What the selected core reads, by config domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct ConfigFeatures {
    pub tun: TunFeatures,
}

impl ClashCore {
    /// The only capability matrix.
    pub fn config_features(self) -> ConfigFeatures;
}
```

- 线格式沿用 `ManageableField` 的相邻标签形式（`backend/nyanpasu-config/src/clash/config/overrides/manageable.rs`），前端按 `kind` 分支：`{ kind: "ignored" }` 或 `{ kind: "values", value: [...] }`。
- 类型放在 `nyanpasu-config`：它持有 `ClashConfig` 与 `ClashCore`，配置生成、校验和 RPC 都从这里取。
- 取值枚举 `T` 是各内核取值的并集，是 Nyanpasu 自己的类型，用 strum 派生 `VariantArray`（列出全部取值）与 `EnumString` 加 `ascii_case_insensitive`（与内核一样不区分大小写地解析）。不手写取值列表。
- 能力表覆盖的配置键也不手写字符串：每个领域一个键枚举（如 `TunKey`），用 strum 的 `IntoStaticStr` 加 `serialize_all = "kebab-case"` 从变体名派生键名，并以关联常量 `TABLE` 给出所在映射。配置生成写键、校验取值和诊断路径（`tun.stack`）都从它取，两边不会拼写不一致。

### 3.2 写法

每个字段一个私有判断函数，对（内核, 取值）做没有 `_` 通配的穷尽 `match`；`Values` 由 `T::VARIANTS` 按它过滤得到。

```rust
fn reads_stack(core: ClashCore, stack: TunStack) -> bool {
    use {ClashCore::*, TunStack::*};
    match (core, stack) {
        (Mihomo | MihomoAlpha, System | Gvisor | Mixed | Mips) => true,
        (ClashPremium, System | Gvisor) => true,
        (ClashPremium, Mixed | Mips) => false,
        (ClashRs | ClashRsAlpha | Meow | MeowAlpha, System | Gvisor | Mixed | Mips) => false,
    }
}
```

- 一个内核对某个键的所有取值都是 `false`，该字段就是 `Ignored`。
- 每一格都要有上游源码依据，写在函数旁的注释里，形式同 `nyanpasu-core-metadata/src/feature/clash.rs` 的版本下限注释。
- 测试：每个内核断言一次 `config_features()` 的完整结果，表格化写法，作为能力表的回归基线。

### 3.3 取值不封闭的字段

数字或任意字符串（如 `mtu`）没有可列举的取值，不适用 `Values`。出现第一个这样的字段时，再给 `FieldSupport` 增加“读这个键、不限制取值”的变体；范围或格式检查另行设计。现在不加。

## 4. 生成与校验

### 4.1 三种值

| 值的来源                         | 当前内核不支持时                                                                                                   |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| Nyanpasu 自己的设置              | 按该字段在能力表中声明的固定规则降级；用户保存的设置不改；在内置 `ManagedSettings` 步骤的构建日志里记一条 `Warn`。 |
| 订阅或转换脚本，且 Nyanpasu 认识 | 拒绝构建，返回 `RuntimeBuildError` 的新变体，带配置路径、实际值与支持的取值。                                      |
| 订阅或转换脚本，Nyanpasu 不认识  | 放行，由内核判断。我们只拦截确定会失败的组合；alpha 内核可能先于 Nyanpasu 加入新值，不应被挡住。                   |
| 字段在当前内核为 `Ignored`       | 不写入，也不校验。                                                                                                 |

### 4.2 校验位置

- 校验对象是**最终配置**：订阅合并、Nyanpasu 设置、默认值、内置转换与 `include-all` 展开全部完成之后（`backend/nyanpasu-config/src/runtime/executor/mod.rs:319-330` 之后的流程）。只校验 Nyanpasu 写入的值不够，订阅和转换脚本都能写入。
- 校验在 `RuntimeBuilder::build` 内始终执行，不只在严格模式下执行（对比 `validate_transforms`，`runtime_builder.rs:125`）。内核会拒绝的配置，任何模式下都不应发布。
- 校验不看 `tun.enable` 一类开关：mihomo 在反序列化时就解析这些键（§1）。
- 一个字段的有效性依赖另一个字段时（例如只在某个栈下才读的键），条件写在该领域的校验函数里，`FieldSupport` 本身不表达跨字段条件。

### 4.3 写入优先级

`FieldSupport` 只回答能力，不决定谁的值优先。优先级仍由 `Ownership` 表达（`backend/nyanpasu-config/src/runtime/post_process/ownership.rs`）。内核特有的设置默认用 `Forced`：大多数配置提供方按 mihomo 编写配置，让订阅接管这类键，换内核时容易失败。是否提供“交给订阅”的选项（`ManageableField`）逐个字段决定，优先级低。

## 5. 变体配置保留

### 5.1 约定

某个设置只在另一个设置取特定值时才有意义（例如 `congestion-controller` 只在 `stack: mips` 下才被读取）：

1. 选择器保持纯标签枚举，存储格式是字符串，不变。
2. 每个带子配置的取值，对应一个与选择器平级的兄弟字段：`#[serde(default)]`，可局部修改的加 `#[patch(nesting)]`。
3. 生成配置、判断运行时输入是否变化等读取方，只通过一个返回借用视图的方法访问，视图里只有当前选中取值的子配置。
4. 切换选择器只修改选择器本身；兄弟字段原样保留，切回时恢复。

```rust
pub struct ClashConfig {
    pub tun_stack: TunStack,
    #[serde(default)]
    #[patch(nesting)]
    pub tun_mips: MipsConfig,
}

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

### 5.2 不变量与代价

- 存储能表示“选的是 gvisor、`tun_mips` 也有值”。这就是保留子配置的代价，类型不禁止这种状态。
- 读取方只用视图，是靠约定保证的；兄弟字段为了 serde、specta 与 struct-patch 必须是 `pub`。评审时检查：除视图方法、默认值与迁移外，不直接读兄弟字段。
- 判断“这次修改是否影响运行时”的输入（`backend/tauri/src/client/application_workflow/impact.rs:340`）同样用视图，因此修改未选中取值的子配置不会触发重建。

### 5.3 方案对比与验证

在一个临时 crate 中，以与仓库相同的依赖版本（serde 1.0.229、specta rc.25、struct-patch 0.14.4、strum 0.28.0、serde_with 3.24.0）编译验证，16 个测试全部通过。

| 方案                                  | 结论                                                                                                                                        |
| ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| **兄弟字段（采用）**                  | 旧文件与手写的 `tun_stack: mips` 都能读，缺字段取默认值；只改一个子字段时其他子字段保持；切走、保存重读、切回后子配置完整恢复。             |
| 带数据的枚举 `Mips(MipsConfig)`       | 手写 `tun_stack: mips` 报 `expected newtype variant`；修改整体替换，其他子字段被重置；struct-patch 不能为枚举派生 `Patch`；切换丢失子配置。 |
| 分组结构 `{ kind, mips }`             | 读不了旧的字符串格式，需要迁移或兼容解码器；兼容解码器使前端输入类型变成“字符串或对象”。                                                    |
| 通用 `Remembered<E>`（标签 → 变体值） | 能存下标签与变体不对应的条目；只能整体替换映射，不支持深层局部修改。                                                                        |
| enum-map / enum-collections           | 所有变体的值必须同一类型；缺键报错；enum-map 没有 specta 支持，也派生不了嵌套 `Patch`。                                                     |
| `serde_with::EnumMap`                 | 能解码出重复的变体键，不保证唯一。                                                                                                          |
| 只在前端记住                          | 重启即丢，多客户端不一致。                                                                                                                  |

有设置界面的 Rust 应用采用同样的形状：Zed 的 `SoftWrap` 是派生 `VariantArray` 的纯标签枚举，只给 `Bounded` 用的 `preferred_line_length` 是兄弟字段（`zed-industries/zed` `crates/settings_content/src/language.rs:462,588`）；Alacritty 把只在闪烁时有意义的 `blink_interval`、`blink_timeout` 作为私有兄弟字段，经访问方法读取（`alacritty/alacritty` `alacritty/src/config/cursor.rs:18-25,58-70`）。带数据枚举见于只靠手写配置文件的 Helix（`helix-view/src/editor.rs:949`），用于“简写或详写”这种不需要在界面里来回切换的场景。

### 5.4 何时抽象

先按 §5.1 为每个领域写具体字段和视图方法。出现第三个“多个取值各有结构不同的子配置”的领域，并且它们重复同一组操作（视图、局部修改、迁移）时，再评估通用包装或派生宏，参考已有的 `nyanpasu_macro::OwnedFields`。

## 6. 前端

- 新增一个查询 RPC，返回当前内核的 `ConfigFeatures`，按 [Unified RPC](../development/rpc.md) 注册为 query，经 `@nyanpasu/rpc` 与 `@nyanpasu/query` 调用；内核切换后随应用配置的状态事件失效。
- 前端按 `FieldSupport` 渲染：`ignored` 隐藏设置项，`values` 只列出这些取值。前端只负责显示文案，不保存支持规则。
- 子配置只在选择器取对应值时显示，修改时只发送子配置的局部 patch。

## 7. 新增一个内核特有字段的步骤

1. 取值枚举派生 `VariantArray`、`EnumString`（`ascii_case_insensitive`）。
2. 在对应领域结构体中加 `FieldSupport<T>` 字段，在领域键枚举中加对应变体，写穷尽的判断函数并附上游依据，更新每个内核的能力表测试。
3. 若它是某个取值的子配置，按 §5.1 加兄弟字段和视图分支。
4. 配置生成按能力表决定写不写；写入优先级用 `Ownership` 表达；声明 Nyanpasu 自己的值不支持时的降级规则。
5. 在领域校验函数中加入该字段（含跨字段条件），补拒绝、放行、`Ignored` 三类测试。
6. `ClashConfig` 改变存储形状时，按文件戳规则升 revision 并加迁移步骤。
7. 前端读 `ConfigFeatures` 渲染，补文案。
