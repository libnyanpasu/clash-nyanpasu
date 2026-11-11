# 种类隔离：内核家族、Profile 与配置生成

[sing-box 内核接入设计](design.md) 的子设计。回答需求 ③：Profile 以及其他持有“某种内核的配置”的位置都带上种类，防止混用；不做内置转换。

路径缩写同主设计：`C/`、`Q/`、`T/`、`R/`、`F/`。

---

## 1. 原则

1. **一个轴。** 全部区分都落到 `CoreFamily { Clash, SingBox }`。不引入第二个表达同一件事的类型（配置格式、目标、方言）。
2. **在提交前拒绝。** 混用在加载与事务提交前的统一校验里被拒绝，不留到内核启动时才失败（`profile-composition-clean-design.md` 第 14 条）。
3. **不转换。** 一个家族的内容永远不会被解释成另一个家族的内容。跨家族的唯一操作是“切换”，不是“迁移”。
4. **默认是 Clash。** 新字段缺省时按 `clash` 解，已有安装不需要迁移（`rust.md` §6）。
5. **编译器把关。** 家族相关的分支写成无通配的穷尽 `match`。

---

## 2. `CoreFamily` 与内核身份

```rust
/// 内核家族：决定配置格式、控制协议与重载方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum CoreFamily {
    #[default]
    Clash,
    SingBox,
}
```

- 位置：`nyanpasu-config`。它同时被应用配置、Profile、运行配置构建使用，是这三者的共同下层。
- 序列化：`clash`、`sing-box`。

内核身份（现名 `ClashCore`，`Q/application/clash_core.rs`）的处理分两步，各自一个 PR：

| 步骤   | 内容                                                                                                                           | 说明                                                                                                             |
| ------ | ------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------- |
| 改名   | `ClashCore` → `CoreId`；`change_clash_core` → `change_core`；`UnsupportedCoreTypeError` 等随之调整。只改 Rust 名与生成的 TS 名 | 纯机械改动，不改序列化值，不改行为。完成 `clash_core.rs:6` 的 TODO。先做，避免出现 `ClashCore::SingBox` 这种名字 |
| 加变体 | `CoreId::SingBox`，序列化为 `sing-box`；`CoreId::family()`；`From<&CoreId> for CoreType` 映射到已有的 `CoreType::SingBox`      | 在 S4 生命周期接线可用时才加，避免应用配置里能选一个起不来的内核                                                 |

要点：

- 应用配置的 `core` 键保持原名，旧取值全部保留。
- `CoreId` 现在是 `#[bitflags] repr(u8)`，7 个变体；`SingBox` 是第 8 个，用满一个字节。本次检索没有发现 `BitFlags<ClashCore>` 的使用点；改名 PR 里确认后去掉 `bitflags` 派生，否则将来加 sing-box 的预发布通道时要改底层类型。
- `binary_name()` 对 `SingBox` 返回 `sing-box`，与上游一致。runtime 仓 `CoreType::get_executable_name` 现在返回 `singbox`（`R/nyanpasu-utils/src/core/definition.rs:126-127`），随 S3 改为 `sing-box`；该内核不在已支持列表里，没有既有安装受影响。线格式的 `"singbox"` 是协议契约，不改。
- `agent_skills()`、`config_features()` 等穷尽 `match` 会在加变体时逐个报错，这正是想要的。

家族与种类的对应：

| `CoreId`                     | `CoreFamily` | core-manager `CoreKind` |
| ---------------------------- | ------------ | ----------------------- |
| `mihomo`、`mihomo-alpha`     | `clash`      | `Mihomo`                |
| `clash-rs`、`clash-rs-alpha` | `clash`      | `ClashRust`             |
| `clash`                      | `clash`      | `ClashPremium`          |
| `meow`、`meow-alpha`         | `clash`      | `Meow`                  |
| `sing-box`                   | `sing-box`   | `SingBox`               |

---

## 3. Profile 的 `family`

```rust
pub struct ProfileItem {
    pub uid: ProfileId,

    /// 这份 Profile 是为哪个内核家族写的。创建时确定，之后不变。
    #[serde(default, skip_serializing_if = "CoreFamily::is_clash")]
    pub family: CoreFamily,

    #[serde(flatten)]
    pub metadata: ProfileMetadata,

    #[serde(flatten)]
    pub definition: ProfileDefinition,
}
```

- 放在 `ProfileItem` 上，不放进 `ProfileMetadata`：元数据是用户可改、可打补丁的，家族不是。
- 缺省为 `clash`，且 `clash` 不写出，旧文档保持逐字节不变（与 `custom_name` 的做法一致，`Q/profile/metadata.rs:17-31`）。
- **不可变。** 任何补丁都不能修改 `family`；`ProfileItem` 的补丁类型里没有这个字段。要换家族只能新建 Profile。
- Config 与 Transform 都带家族。Transform 的家族表示“这个转换是对哪种配置写的”。

### 3.1 校验规则

在 `Profiles::validate()` 里新增，与现有引用校验一起在加载与提交前执行。每条对应一个带结构化字段的 `ProfileValidationError` 变体。

| 规则                                                                | 错误变体（工作名）                                                                |
| ------------------------------------------------------------------- | --------------------------------------------------------------------------------- |
| `Config.transforms` 里的每个 Transform 与该 Config 同家族           | `TransformFamilyMismatch { owner, target, expected, found }`                      |
| `Composition.base` 与该 Composition 同家族                          | `CompositionMemberFamilyMismatch { composition, role, profile, expected, found }` |
| `Composition.extend_proxies_from` 的每个成员与该 Composition 同家族 | 同上                                                                              |
| `sing-box` 家族的 Config 不能是 Composition                         | `CompositionUnsupportedForFamily { composition, family }`                         |

最后一条的原因：组合的语义是“取成员的 `proxies` 并入”（`Q/runtime/executor/compose.rs:128-160`），这是 Clash schema。sing-box 的出站在 `outbounds` 里，且分组本身也是出站，按 tag 互相引用，不能照搬。sing-box 的组合留到有明确需求时单独设计。

`global_transforms` 不做家族校验：它是一个跨 Profile 共用的列表，里面可以同时有两个家族的转换。执行时只运行与选中 Config 同家族的项（§4.1）。

`current` 与内核的家族一致性不放在 `Profiles::validate()` 里：`Profiles` 不知道选中的内核。它是跨状态域的不变量，由切换工作流与运行配置构建共同保证（§5）。

### 3.2 导入

| 来源     | 家族怎么定                                                                              |
| -------- | --------------------------------------------------------------------------------------- |
| 本地文件 | 用户在导入对话框里选；默认值由内容探测给出。探测与所选不一致时拒绝并说明                |
| 远程订阅 | 用户先选家族，再按家族决定默认 `User-Agent`（见下）；下载后同样做内容探测，不一致时拒绝 |
| 深链接   | 链接参数里带家族；没有时默认 `clash`，保持现有链接的行为                                |
| 新建空白 | 用户选；默认取当前内核的家族                                                            |

内容探测是纯函数，替换 `C/profile_import.rs:142-144` 的单一判断：

| 判定为     | 条件                                                                                      |
| ---------- | ----------------------------------------------------------------------------------------- |
| `clash`    | 顶层是映射，且含 `proxies`、`proxy-providers`、`proxy-groups`、`rules` 之一               |
| `sing-box` | 顶层是映射，且含 `outbounds`、`inbounds`、`route`、`endpoints` 之一，且不含上一行的任何键 |
| 无法判定   | 其余情况。Config 导入时拒绝；Transform 导入时接受用户所选（覆盖片段可以很小，无从探测）   |

订阅的默认 `User-Agent`：订阅服务普遍按 UA 决定返回格式。`clash` 家族保持现值（`clash-nyanpasu/<版本>`，`backend/nyanpasu-net/src/policy.rs:69`）。`sing-box` 家族的默认值需要让服务端识别为 sing-box 客户端，候选是 `sing-box <内核版本>` 或 `clash-nyanpasu/<版本> (sing-box <内核版本>)`；选哪个由假设 V15 的验证结果决定。每个 Profile 已有的自定义 UA 选项不变。

物化文件的扩展名按家族取：`clash` 为 `.yaml`，`sing-box` 为 `.json`。带注释的 JSON 是否需要在导入时正规化取决于假设 V4：pipeline 用 YAML 解析器读入 Profile 文本（`Q/runtime/executor/mod.rs:143-144`），标准 JSON 是合法 YAML，带 `//` 注释的不是。

---

## 4. 运行配置构建

### 4.1 前段共用

以下步骤跑在 `ConfigValue` 树上，与 schema 无关，两个家族共用：

- 读入选中 Config 的文件内容。
- 依次应用 Config 自己的 transforms。
- 依次应用 `global_transforms`，**只执行与选中 Config 同家族的项**。被跳过的项在步骤日志里留一条“家族不符，已跳过”，检视页据此显示。
- 覆盖（Overlay）指令 `prepend-`、`append-`、`override__`、`filter__` 与裸键合并（`Q/runtime/executor/overlay.rs:29-45`）是按路径操作的通用语义，对 sing-box 配置同样成立。
- 脚本（JavaScript、Lua）拿到的是配置对象，不关心 schema。

### 4.2 尾段按家族分开

`execute`（`Q/runtime/executor/mod.rs`）在全局转换之后的尾段改为按家族分派。

| 尾段步骤                 | `clash`                              | `sing-box`                                                                                                |
| ------------------------ | ------------------------------------ | --------------------------------------------------------------------------------------------------------- |
| `applied_fields` 采样    | 执行                                 | 不执行；该字段只服务于字段白名单的 UI                                                                     |
| 字段白名单与保留未受管键 | 执行（开启时）                       | 不执行。白名单是 45 个 Clash 键（`Q/runtime/executor/builtin.rs:12-71`）                                  |
| 受管设定                 | Clash 写入器                         | sing-box 写入器，按世代（§4.3）                                                                           |
| 默认值                   | 执行                                 | sing-box 的默认值集合（§4.3）                                                                             |
| 内置转换                 | 按内核门控                           | 无。现有三个内置脚本都是 Clash 专属（`backend/nyanpasu-profile-transform/src/runtime_builder.rs:87-118`） |
| `include-all` 展开       | 执行（开启时）                       | 不执行。`include-all` 是 Mihomo 分组语法                                                                  |
| 能力校验                 | `ConfigFeatures::unsupported_values` | sing-box 的校验：受管片段之外不做内容校验，交给内核的 `check`                                             |
| 序列化                   | YAML                                 | JSON                                                                                                      |

实现形状：`RuntimePipelineInputs` 里与 Clash 尾段相关的字段（`guard`、`whitelist_enabled`、`tun`、`expand_include_all`、`builtin_transforms`）收进一个按家族区分的枚举，`execute` 对它穷尽匹配。

```rust
pub enum TailInputs<'a> {
    Clash(ClashTail<'a>),
    SingBox(SingBoxTail<'a>),
}
```

`RuntimeArtifact` 带上 `family`，下游（产物转快照、发布、检视、序列化）据此选择格式。`ExecutionTarget::Bare`（没有选中 Profile）对两个家族都保留：sing-box 的空白配置是“受管入站加一个直连出站”。

### 4.3 sing-box 的受管设定

**新原语。** 现有 `Ownership` 只能定址“顶层键”与“嵌套表里的键”（`Q/runtime/post_process/ownership.rs`）。sing-box 的入站、出站、服务都是带 `tag` 的数组元素，需要两个新操作：

| 操作               | 语义                                                                                       |
| ------------------ | ------------------------------------------------------------------------------------------ |
| 按键增改数组元素   | 在数组里找 `tag == X` 的元素；有则按给定的字段所有权逐键处理，无则创建并追加（或置于首位） |
| 按谓词移除数组元素 | 移除数组里 `type ∈ {…}` 且 `tag` 不属于 Nyanpasu 的元素                                    |

两者与现有步骤一样记录自己做的补丁，供检视页显示，不靠事后比较树。

**首版受管范围（世代 1.14）。** 写法以世代为参数（[`schema-compat.md`](schema-compat.md) §3）。

| Nyanpasu 设定                 | Clash 侧的键                                | sing-box 侧的写法                                                                                                                                                                                          |
| ----------------------------- | ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 控制面                        | `external-controller`、`secret`             | 强制：`services[]` 里 `tag = "nyanpasu-api"`、`type = "api"` 的元素，`listen = "127.0.0.1"`，`listen_port` 为分配的端口，`secret` 为密钥，`dashboard = false`。移除 Profile 自带的其他 `type = "api"` 服务 |
| 混合端口                      | `mixed-port`                                | 强制：`inbounds[]` 里 `tag = "nyanpasu-mixed"`、`type = "mixed"` 的元素，`listen_port` 为解析后的端口                                                                                                      |
| HTTP、SOCKS 端口              | `port`、`socks-port`                        | 设置了才写：`tag = "nyanpasu-http"`（`type = "http"`）、`tag = "nyanpasu-socks"`（`type = "socks"`）                                                                                                       |
| 允许局域网                    | `allow-lan`                                 | 上述入站的 `listen`：开启为 `"::"`，关闭为 `"127.0.0.1"`                                                                                                                                                   |
| Profile 自带的本地代理入站    | 未设置的端口键被移除                        | 移除 `type ∈ {mixed, http, socks}` 且不属于 Nyanpasu 的入站。与 Clash 侧“Profile 不能打开用户关掉的监听”一致（`Q/runtime/executor/plan.rs:28-31`）                                                         |
| 日志级别                      | `log-level`                                 | 强制：`log.level`；`silent` 映射为 `log.disabled = true`。另强制 `log.timestamp = true` 以便解析控制台日志                                                                                                 |
| Profile 自带的 Clash API 监听 | `external-controller` 被覆盖                | 移除 `experimental.clash_api.external_controller` 与 `external_ui`，保留 `default_mode`。外部面板作为受管项在 S7 加回                                                                                      |
| 选择与模式的持久化            | `profile.store-selected` 默认值             | 默认值：`experimental.cache_file.enabled = true`。已有的 `cache_file` 整体保留                                                                                                                             |
| 模式                          | `mode`                                      | 不写。见 [`core-api.md`](core-api.md) §9.1 的 `set_core_mode`                                                                                                                                              |
| IPv6、统一延迟、TCP 并发      | `ipv6`、`unified-delay`、`tcp-concurrent`   | 不写。sing-box 没有对应的全局键；在 `ConfigFeatures` 里标为不读取，UI 隐藏                                                                                                                                 |
| TUN                           | `tun.*`、`dns.*`                            | 首版不支持，见下                                                                                                                                                                                           |
| 透明代理端口与路由标记        | `redir-port`、`tproxy-port`、`routing-mark` | 首版不支持                                                                                                                                                                                                 |

移除 Profile 自带入站有一个待验证点：如果 Profile 的路由规则按 tag 引用了被移除的入站，`check` 是否失败。若失败，改为“沿用 Profile 第一个同类型入站的 tag，只覆盖监听字段”。这一条并入 S0，结果决定最终写法。

**不支持的设定要显式报告。** `backend-packages.md` 规定没有效果的业务操作不能报告为成功应用。TUN 开关、透明代理在 sing-box 家族下：

- `ConfigFeatures` 增加相应的支持项，对 sing-box 返回不支持；
- 前端据此禁用开关并显示原因；
- 用户在 Clash 家族下开了 TUN 再切到 sing-box：保存的设定不改，构建时不写 TUN，并在受管设定步骤的构建日志里记一条警告。这与 `core-specific-config.md` §4.1 对“Nyanpasu 自己的设置遇到不支持”的处理一致。

系统代理不受影响：它由应用设置操作系统代理，只需要混合端口。

### 4.4 家族不符时的构建错误

构建入口先核对选中 Config 的家族与内核家族。不一致时返回新的 `RuntimeBuildError` 变体，带两边的家族与 Profile 标识，不进入 pipeline。正常路径上这个错误不会出现（§5 的工作流保证不变量）；它是给手改配置文件与并发边界准备的防线。

---

## 5. 选中关系

### 5.1 不变量

> `Profiles.current` 为空，或 `family(current) == family(应用配置.core)`。

### 5.2 推荐方案：成对切换，记住每个家族的上次选择

跨家族的切换总是同时改两样东西：内核与当前 Profile。由 `NyanpasuClient` 上的一个工作流完成。

触发点：

| 用户操作                   | 工作流做什么                                           |
| -------------------------- | ------------------------------------------------------ |
| 把内核换成另一个家族的内核 | 当前 Profile 换成该家族上次选中的 Profile；没有则置空  |
| 激活另一个家族的 Profile   | 内核换成该家族上次使用的内核；没有则用该家族的默认内核 |
| 同家族内换内核或换 Profile | 现有路径，不经过这个工作流                             |

步骤，遵循“先提交状态，再触发副作用；runtime 是提交前的投票方”（`architecture.md` 的状态与配置一节）：

1. 解析目标的（内核, Profile）对。
2. 用目标对构建运行配置候选并让内核校验。失败则整个切换不提交，返回结构化错误。
3. 提交应用配置（`core` 与家族记忆）与 Profiles（`current` 与家族记忆）。
4. 应用运行配置。提交后的失败按降级结果报告，不回滚已提交的状态。

两个状态域的联合提交复用现有的应用工作流事务机制（`C/client/application_workflow/`）；S2 细化任务时核对它对“应用配置加 Profiles”这一对参与者的支持情况，不新造事务框架。

新增的持久化字段，都带缺省值：

| 位置     | 字段（工作名）                                                | 含义                       |
| -------- | ------------------------------------------------------------- | -------------------------- |
| 应用配置 | `last_core: { clash?: CoreId, "sing-box"?: CoreId }`          | 每个家族上次使用的内核     |
| Profiles | `last_current: { clash?: ProfileId, "sing-box"?: ProfileId }` | 每个家族上次选中的 Profile |

`last_current` 里的 ProfileId 在 Profile 被删除时一并清理，纳入现有的 sanitize 流程（`Q/profile/profiles.rs` 的 `ProfilesSanitizeReport`）。

这不违反“`Profiles.current` 只保存单个 ProfileId”：`current` 仍是唯一的“正在使用”，新增的只是记忆。

### 5.3 备选方案：只拒绝，不联动

不加记忆字段，不加工作流。内核与当前 Profile 家族不符时，切换内核或激活 Profile 的操作被拒绝并报错，用户自己先取消选中、再换内核、再选 Profile。

实现最省，但每次跨家族切换要三步，且中间有一段“没有选中 Profile”的状态。除非 S2 发现联合提交的成本明显高于预期，否则不选这个方案。

**这一节需要确认。**

---

## 6. “Clash 设定”状态文档

`ClashConfig`（`Q/clash/config/mod.rs:36-93`）是三个状态文档之一，保存端口、覆盖项、TUN、外部控制器等。它的名字与大部分字段是 Clash 的，但其中一部分是两个家族都需要的。

决策：**不新建第四个状态文档，不整体改名。** sing-box 家族读取其中与家族无关的字段；Clash 专属字段在 sing-box 家族下被忽略并由 `ConfigFeatures` 标明。

| 字段                                                             | sing-box 家族下                                                            |
| ---------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `mixed_port`、`socks_port`、`http_port`                          | 使用                                                                       |
| `overrides.allow_lan`、`overrides.log_level`、`overrides.secret` | 使用                                                                       |
| `external_controller`                                            | 首版用于分配 API 服务端口；S7 起另用于可选的 Clash 面板                    |
| `overrides.mode`                                                 | 不使用（模式不持久化在这里）                                               |
| `overrides.ipv6`、`unified_delay`、`tcp_concurrent`              | 不使用                                                                     |
| `enable_tun_mode`、`tun`                                         | 首版不使用；S7 设计 sing-box 的 TUN 时决定是复用还是加 sing-box 专属子结构 |
| `enable_clash_fields`、`expand_include_all`                      | 不使用                                                                     |
| `clash_control_channel`、`clash_ipc_disable_http_controller`     | 不使用：sing-box 的 API 服务只有 TCP                                       |
| `redir_port`、`tproxy_port`、`transparent_proxy`                 | 首版不使用                                                                 |
| `web_ui_list`                                                    | 与内核无关，照常                                                           |

理由：

- 一个字段只为 sing-box 新开一套配置结构是 `core-specific-config.md` §2 否决过的过度设计；等 sing-box 专属设定真的出现（最可能是 TUN）再加子结构。
- 整体改名会让一次功能接入变成一次大范围重命名，且 `ClashConfig` 里确实大部分是 Clash 的东西。

因此 `get_clash_config`、`patch_clash_config` 这两个命令保留原名。前端的设置路由 `settings/clash/*` 是否改名属于 S6 的界面决策。

`ConfigFeatures` 的扩展：现有结构只有 `tun` 一个领域。S4 按 `core-specific-config.md` §3 的方式为上表里“不使用”的各项补上支持信息，使前端能隐藏它们，而不是按家族名写死。

---

## 7. 其他需要带家族的位置

逐项列出，避免遗漏。

| 位置                                      | 现状                                                   | 处理                                                                                                                                     | 阶段   |
| ----------------------------------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| 应用配置 `core`                           | `ClashCore`                                            | §2                                                                                                                                       | S2、S4 |
| Profile 项、转换链、组合                  | 无家族                                                 | §3                                                                                                                                       | S2     |
| 导入与订阅                                | 只认 Clash                                             | §3.2                                                                                                                                     | S2、S6 |
| pipeline 尾段                             | 固定 Clash                                             | §4                                                                                                                                       | S4     |
| 运行配置产物、快照、检视                  | 隐含 YAML                                              | `RuntimeArtifact` 带家族；`get_runtime_yaml` 改为返回文本加格式；快照差异按家族的格式渲染                                                | S4     |
| core-manager 的内核种类、配置读写、文件名 | Clash 系                                               | [`core-manager.md`](core-manager.md)                                                                                                     | S3     |
| 线格式（`nyanpasu_ipc`）                  | `CoreType::SingBox` 已有；控制器信息不带协议           | `CoreApiConnection` 加 `protocol`；服务状态加受支持的内核种类列表（主设计 D12）                                                          | S3     |
| 控制面                                    | Clash API                                              | [`core-api.md`](core-api.md)                                                                                                             | S1、S4 |
| 内核更新清单与 updater                    | 写死七个 Clash 内核                                    | [`schema-compat.md`](schema-compat.md)                                                                                                   | S5     |
| 内核版本读取                              | `CoreVersionReader::read(ClashCore)`，探测参数按内核取 | 加 sing-box 的 `version` 子命令与输出解析（`backend/nyanpasu-os/src/core_version.rs`、`backend/nyanpasu-contracts/src/core_version.rs`） | S4     |
| native store                              | 按内核种类分目录，路径改写针对 Clash 键                | [`core-manager.md`](core-manager.md) §8                                                                                                  | S3     |
| 控制台日志解析                            | 四种 Clash 系布局                                      | [`core-manager.md`](core-manager.md) §7                                                                                                  | S3     |
| geodata 模式跟随                          | 读内核的 `geodata-mode`                                | sing-box 固定为 mmdb，不调内核（[`core-api.md`](core-api.md) §4.4）                                                                      | S4     |
| 流量记账                                  | 以 `chains`、`rule` 为维度                             | 不改结构；sing-box 的规则字符串原样入库                                                                                                  | S4     |
| 代理分组布局、固定选择                    | 按组名保存                                             | 不改；组名来自各自的 Profile，不同家族的 Profile 本来就不同                                                                              | —      |
| 托盘                                      | 按内核枚举分支                                         | 读能力表（[`core-api.md`](core-api.md) §10）                                                                                             | S1、S6 |
| Linux 透明代理                            | 仅 Mihomo                                              | sing-box 标为 Deferred                                                                                                                   | S7     |
| macOS DNS 接管                            | 读 Clash 的 DNS 配置                                   | sing-box 不启用                                                                                                                          | S7     |
| 诊断报告                                  | 报告内核类型                                           | 加家族与世代                                                                                                                             | S4     |
| 设置迁移（`nyanpasu-migration`）          | 旧 schema 一律是 Clash                                 | 不改：迁移来的数据都是 `clash`                                                                                                           | —      |
| 备份与恢复                                | 整体打包状态文档                                       | 不改：新字段随文档走                                                                                                                     | —      |
| WebUI 列表与外部面板                      | 用 Clash 控制器地址                                    | sing-box 下在外部控制器未开启时禁用“打开面板”                                                                                            | S6、S7 |

---

## 8. 前端

属于 S6，这里只列与种类隔离有关的行为，页面组织遵循 `typescript.md` 的路由页面与模块一节。

| 位置           | 行为                                                                                                       |
| -------------- | ---------------------------------------------------------------------------------------------------------- |
| Profile 列表   | 每项显示家族标记；提供按家族筛选；与当前内核家族不同的 Config 仍可见，点击激活时走 §5.2 的成对切换并先确认 |
| 转换链编辑     | 可选的 Transform 只列同家族的；全局转换列表里标出“对当前 Profile 不生效”的项                               |
| 导入对话框     | 家族选择，默认值来自内容探测或当前内核；探测不一致时显示错误                                               |
| Profile 编辑器 | 按家族选择语言模式（YAML 或 JSON）与校验 schema                                                            |
| 内核选择       | 按家族分组；切到另一个家族时说明将同时切换 Profile                                                         |
| 设置页         | 各项按 `ConfigFeatures` 显示或隐藏                                                                         |
| 检视页         | 运行配置按家族的格式显示；被跳过的全局转换有说明                                                           |

文案进入 i18n，按功能组织消息（`i18n.md`）。

---

## 9. 测试

| 对象              | 方法                                                                                                                              |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| 序列化兼容        | 现有 Profiles 文档样例解码后 `family` 全为 `clash`，再编码与原文逐字节相同                                                        |
| 校验规则          | §3.1 每条规则一个正例一个反例，纯值测试                                                                                           |
| 补丁不可改家族    | 对 `ProfileItem` 应用任意补丁后 `family` 不变                                                                                     |
| 内容探测          | 表格化：Clash 样例、sing-box 样例、同时含两边键的文档、空映射、非映射                                                             |
| 全局转换过滤      | 列表里混有两个家族的转换，断言只执行同家族的，且日志里有跳过记录                                                                  |
| sing-box 受管设定 | golden 测试：给定输入 Profile 与设定，断言最终 JSON；覆盖 Profile 自带入站被移除、已有 `nyanpasu-*` 元素被覆盖、`cache_file` 保留 |
| 数组原语          | 纯值测试：增、改、移除、目标键不是数组、元素不是映射                                                                              |
| 家族不符的构建    | 断言返回专用错误且不调用脚本运行器与内容读取                                                                                      |
| 成对切换          | 经 `NyanpasuClient` 与假适配器：两个方向的切换、没有记忆时的默认、候选校验失败时两个状态域都不变、提交后应用失败时报告降级        |
| 删除 Profile      | 被记住的 Profile 删除后 `last_current` 对应项被清理                                                                               |
