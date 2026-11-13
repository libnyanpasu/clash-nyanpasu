# 内核分发接入 resource manifest v2

**日期：** 2026-11-14

**状态：** 设计稿 r5，未实施。经 codex 审查五轮：64 → 80 → 86 → 89 → PASS 95；修订记录见 §17。

**基线：**

- app `main@fdf5760fa`；
- runtime 子模块 `3854f2c`；
- resources-manifest `feat/manifest-publish@0874a44`（下称 manifest 仓）；
- nyanpasu-installer `main@be86409`。

**来源：** 四条需求。① 内核更新改用 manifest v2；保持"同一内核可同时安装多个 channel"，每个 channel 只能选一种变体。② 简化本仓与 runtime 仓里参考 manifest v2 思路写的类型。③ 下载的内核移出 sidecar 目录，放到数据目录；只有随包内核留在 sidecar 路径。④ 评估由 nyanpasu-installer 在安装时下载内核到数据目录。

**已裁定（2026-11-14）：** v2 结构以 manifest 仓为准，nyanpasu 侧用 newtype 扩展，适合放进 manifest 仓的放进去；服务模式先按"服务自有副本"实施（§7）。

**权威顺序：** `AGENTS.md` > `docs/development/*` > 本设计 > [`tasks.md`](tasks.md)。manifest 协议以 manifest 仓的 `docs/design/manifest-v2.md` 为准。

路径缩写：`U/` = `backend/nyanpasu-updater/src/updater/`，`P/` = `backend/nyanpasu-paths/src/`，`O/` = `backend/nyanpasu-os/src/`，`C/` = `backend/nyanpasu-core/src/`，`T/` = `backend/tauri/`，`R/` = `backend/nyanpasu-runtime/crates/`，`S/` = `scripts/src/`，`M/` = manifest 仓，`I/` = nyanpasu-installer 仓。

---

## 0. 结论摘要

1. **manifest v2 可以直接接入。** 三个内核、六个平台键、默认产物都与现有 v1 清单对齐；Rust 侧直接依赖 `manifest-core`，不经 wasm。
2. **安装槽位是（内核, channel）。** 它与现有的 `ClashCore` 六个取值一一对应。配置里的 `core` 键、可执行文件名、服务 IPC 的 `CoreType` 都不变。每个槽位持久化一个变体选择，同一时刻只有一个变体生效。
3. **manifest 仓只加一个字段。** 产物增加可选的 `binary`（归档内可执行文件路径）。"每个 channel 的默认变体"不改协议：nyanpasu 侧的 newtype 校验约定，manifest 仓用生成器测试守住它。
4. **下载的内核放 `<data>/core-bin/`，按内容寻址。** 一次安装的提交点是一个记录文件的原子替换。updater 不再写安装目录，也不再提权复制。
5. **服务只执行自己内核库里的文件，并按哈希取用。** 现在服务以 SYSTEM/root 身份先到用户可写的数据目录找内核，执行前只检查文件存在。本设计让应用在启动请求里指明二进制的哈希，服务只从自己保管的库里取。stable 内核由服务用签名清单自行核对；alpha 没有可预先签名的 digest，是否允许由一个管理员开关决定。
6. **安装器不下载内核。** 安装包只带 Mihomo stable；其余内核由应用按需下载。按 rc.2 的 Windows payload 计，安装内容减少约 131 MB（压缩前）。
7. **`CoreDistribution`、`VariantTag` 等镜像类型删除。** 它们没有生产使用者，语义还与 manifest 分叉。
8. **本设计取代 sing-box 设计的清单结构。** [`schema-compat.md`](../2026-11-11-sing-box-core/schema-compat.md) §4.2–§4.5 的 `manifest/v2/cores.json` 不再采用；世代门控的规则保留，改在 nyanpasu 侧实现（§10）。

---

## 1. 目标与非目标

目标：

1. 六个内核槽位的版本查询、下载、校验全部经 manifest v2 与 `manifest-core`。
2. 用户可以为每个槽位选择一个变体；选择持久化，更新时沿用。
3. 下载的内核只写数据目录；安装目录不可写时更新照常工作。
4. 服务不执行自己内核库之外的二进制。
5. 安装包只带一个默认内核。
6. 删除 v1 清单的读取代码、镜像类型与无调用方的路径函数。

非目标：

- sing-box 的接入。它的清单需求见 §10，实施仍属 sing-box 设计的 S5。
- geodata 与 wintun 的更新改走 manifest。manifest 已收录 geodata，可另行设计。
- manifest 本身的生成、签名与发布流程。
- 内核的差分更新、P2P 或断点续传。
- 经 HTTP（WebUI）触发内核下载。下载与更新命令保持只对桌面开放（§12）。
- 用户自行放置的内核二进制。数据目录根下的同名文件不再被识别（§6.5）。
- 防御以同一用户身份运行的恶意进程在非服务模式下替换内核。那时内核以该用户身份运行，没有权限边界可守。

---

## 2. 现状

### 2.1 应用侧

| 事实 | 位置 |
| --- | --- |
| 内核身份是 `ClashCore`，六个取值：`mihomo`、`mihomo-alpha`、`clash-rs`、`clash-rs-alpha`、`meow`、`meow-alpha`。channel 编码在取值里，不是字段 | `backend/nyanpasu-config/src/application/clash_core.rs` |
| 应用没有编译变体的概念。GOAMD64 `v2`、`static-crt` 写死在 v1 清单的资产名里 | `manifest/version.json`；`S/manifest/manifest.ts` |
| v1 清单的结构体对每个内核写死一个字段；它经 facade 与 RPC 一路到前端 | `U/mod.rs:30-60`；`C/client/mod.rs`；`backend/nyanpasu-rpc/src/commands/updater.rs` |
| 清单从 `clash-nyanpasu` 仓的 `raw/main/manifest/version.json` 取，经测速选出的镜像 | `U/instance.rs:106-132`；`U/mirrors.rs:5-10` |
| alpha 的滚动版本在运行时解析：调度在 `resolve_rolling_versions`，三个上游各有一段专用解析；TODO 写明应由 manifest v2 描述 | `U/instance.rs:317-365`（调度）；`:367-455`（解析） |
| 只有 Meow 校验 SHA-256；其余内核下载后不校验 | `U/instance.rs:158-162,469-496` |
| Meow 另外核对二进制报告的版本：stable 去掉前缀 `v` 比较，alpha 比较 banner 的构建字段 | `U/instance.rs:666-691` |
| zip 解包靠文件名猜可执行文件 | `U/instance.rs:498-562` |
| updater 把内核写到应用可执行文件所在目录 | `T/src/setup.rs:366-376`；`backend/nyanpasu-server/src/compose.rs:299` |
| 写入是原地 `fs::copy`；失败后提权执行 `copy /Y` 或 `cp -f`。没有暂存、没有备份 | `O/core_install.rs:78-123` |
| 安装请求的契约刻意不含 updater 的类型 | `backend/nyanpasu-contracts/src/core_install.rs` |
| 替换内核由内核生命周期工作流串行执行 | `C/client/core_lifecycle/workflow.rs:741` |
| 启动时查找内核：数据目录根 → 安装目录 → 开发候选 | `P/lib.rs:183-205` |
| 读内核版本只看可执行文件所在目录，不走上面的查找 | `O/core_version.rs:25-58` |
| 没有"已安装内核"的查询。前端对六个内核逐个读版本，失败显示 N/A | `frontend/query/src/ipc/use-clash-cores.ts` |
| `update_core` 只对桌面开放，有回归测试保证 HTTP 请求进不了 updater | `T/src/ipc.rs`；`backend/nyanpasu-rpc/src/registry/tests.rs` |
| 安装包带全部六个内核与服务 | `T/tauri.conf.json` 的 `externalBin` |
| 构建时的内核版本来自 v1 清单 | `S/prepare/check.ts:121` |
| Windows portable 包不带 meow 与 meow-alpha | `S/release/portable.ts` |

三条路径规则互相不一致：启动查找认数据目录，updater 写安装目录，版本探测只认安装目录。现在没有任何代码往数据目录根写内核，所以不一致还没有表现出来。

### 2.2 runtime 仓

| 事实 | 位置 |
| --- | --- |
| 服务按内核名自己找二进制：数据目录 → 应用目录。应用不经 IPC 传路径 | `R/nyanpasu-service-runtime/src/server/manager_bridge.rs:1015-1031` |
| 两个目录来自服务安装时写入的命令行参数 | `O/service.rs:70-80` |
| 启动前对二进制只做存在性检查；没有 ACL、属主、签名或哈希校验 | `R/nyanpasu-core-manager/src/instance.rs` |
| 服务以 root / LocalSystem 运行 | `R/nyanpasu-service-runtime/src/cmds/install.rs` |
| 服务对配置已经采用私有副本：调用方的配置被复制到 `core-runtime/` 下加固过的目录再执行 | `R/nyanpasu-core-manager/src/config/runtime_store.rs` |
| Windows 上现有的目录加固只设置 DACL（属主、SYSTEM、Administrators），不更改属主，也不检查已有对象 | `nyanpasu-utils` 的 `io/atomic_fs.rs` |
| `VariantTag`、`CoreDistribution` 只有定义、再导出与自身单测，没有任何生产使用者 | `R/nyanpasu-core-metadata/src/dist.rs` |
| `VariantTag::new` 把 tag 转成小写；manifest 的 ID 大小写敏感且从不修剪 | `dist.rs:36-38`；`M/docs/design/manifest-v2.md` |
| `ClashCoreResourceVariant` 与 `CoreKind { Clash, SingBox }` 同样没有使用者 | `R/nyanpasu-core-metadata/src/kind.rs` |

结果：任何能写用户数据目录的进程，放一个 `mihomo.exe` 进去，服务下次启动内核时就以 SYSTEM 身份执行它。这是现存问题，不是本设计引入的；但把下载的内核改放数据目录后，这条路径会从潜在变成主路径，所以必须一起处理。

### 2.3 manifest 仓

| 事实 | 位置 |
| --- | --- |
| 收录 `mihomo`、`meow`、`clash_rs` 三个内核与三个 geodata 文件，`category` 分别为 `core`、`geodata` | `M/packages/manifest-generator/manifest.config.ts` |
| 六个平台键与 v1 的 `arch_template` 键相同 | `M/crates/manifest-metadata/src/lib.rs:195-208` |
| 每个内核有 `stable`、`alpha` 两个只带 channel tag 的变体，以及带一个附加 tag 的可选变体（`cpu:x86-64-v3`、`libc:gnu`、`libc:musl`、`crt:static`） | `M/docs/spec/2026-11-06-manifest-core-parsers/design.md` §9.4 |
| `default_variants` 每个平台一个值，全部是 `stable` | `manifest.config.ts:141-143` |
| stable 是 static 历史，内嵌 digest（`authoritative`）；alpha 是 remote，版本与 checksum 都在运行时取（`parsed`） | `manifest.config.ts:120-126` |
| 产物只有 `artifact`、`source`、`version` 三个字段，没有归档内路径 | `M/crates/manifest-metadata/src/lib.rs:422-429` |
| `select_variant` 按 resource、平台与选择条件匹配，不理解 channel 分组 | `M/crates/manifest-core/src/plan.rs:156-248` |
| 镜像不支持某个 registry 时，带该镜像的规划直接报错，不会自动退回 canonical；`nyanpasu` 镜像不支持 `github-api` | `M/crates/manifest-core/src/plan.rs`；`manifest.config.ts` |
| parser 的输入上限默认 2 MiB，只针对版本与 checksum 响应 | `M/crates/manifest-parser/src/limits.rs` |
| `updated_at` 是可选字符串，协议不校验它的格式 | `M/crates/manifest-metadata/src/lib.rs:925` |
| 发布物是 `resources.json` 与分离签名 `resources.json.minisig`，每六小时更新；下载器先对原始字节验签再解码 | `M/README.md`；`M/packages/manifest-downloader/main.ts` |
| `manifest-core` 无 IO、无异步依赖；宿主负责 HTTP 与摘要计算 | `M/docs/design/manifest-v2.md` "Consumers" |
| `specta` 与本仓锁定在同一版本 `2.0.0-rc.25` | 两仓的 `Cargo.lock` |
| `@manifest/*` 的 Node 包没有构建步骤，只能在其工作区内使用 | `M/README.md` "Requirements" |

与 v1 的一处差异：clash-rs 在 `linux-aarch64` 上 v1 用 gnu 构建，v2 默认用 musl。gnu 构建要求 glibc 2.39，高于应用的 2.35 下限，所以 v2 的选择是修正。

### 2.4 nyanpasu-installer

| 事实 | 位置 |
| --- | --- |
| 只支持 Windows；payload 的文件集从 `tauri.conf.json` 读出，包含全部 `externalBin` | `I/docs/design.md` |
| 没有可选组件；每个文件都安装 | `I/crates/nyanpasu-installer-core/src/model.rs` |
| 全机安装时整个安装器进程提权运行 | `I/crates/nyanpasu-installer-core/src/wizard.rs` |
| 降权分派用于卸载阶段的当前用户清理任务；安装完成后能降权启动应用。没有安装期的用户任务，也没有内核下载任务 | `I/crates/nyanpasu-installer-core/src/windows/system_state.rs`；`engine.rs:437-463` |
| 网络访问只有 WinINet 下载 WebView2 与 VC++ 运行库；没有重试、续传、哈希或 minisign 校验 | `I/crates/nyanpasu-installer-core/src/windows/download.rs` |
| 升级时删除旧记录里有、新记录里没有的文件 | `I/crates/nyanpasu-installer-core/src/engine.rs` |
| rc.2 的 Windows payload 里 mihomo 两个各 61.2 MB，clash-rs 两个各 21.6 MB，meow 两个各 13.3 MB | `I/docs/benchmark/compression.md` |

---

## 3. 术语

| 术语 | 含义 |
| --- | --- |
| 槽位 | 一个可安装的内核位置：（内核, channel）。即 `ClashCore` 的一个取值 |
| 变体 | manifest 的 `ResourceVariant`。语义身份是精确 tag 集合，`id` 只是别名 |
| 基础变体 | 某个 channel 里 tag 集合恰好只有 channel tag 的变体，如 `stable`、`alpha` |
| 随包内核 | 安装包自带、位于安装目录的内核。只读 |
| 受管内核 | updater 下载并放进 `<data>/core-bin/` 的内核 |
| 内核库 | 服务私有目录里按哈希保存的内核文件 |
| 有锚 / 无锚 | 内核库条目的来源是否由服务自己用签名清单核对过 |

---

## 4. 槽位与变体

### 4.1 槽位映射

`ClashCore` 保留，新增两个纯函数把它映射到 manifest：

| `ClashCore` | resource | channel tag |
| --- | --- | --- |
| `mihomo` | `mihomo` | `channel:stable` |
| `mihomo-alpha` | `mihomo` | `channel:alpha` |
| `clash-rs` | `clash_rs` | `channel:stable` |
| `clash-rs-alpha` | `clash_rs` | `channel:alpha` |
| `meow` | `meow` | `channel:stable` |
| `meow-alpha` | `meow` | `channel:alpha` |

- 映射写成无通配的穷尽 `match`（`rust.md` §5.5），与 `binary_name()` 放在一起。
- 不新增 channel 字段，不改 `core` 键的取值。将来出现第三个 channel，是新增一个 `ClashCore` 取值。
- "每个 channel 只能选一种变体"由记录保证：一个槽位只有一条当前记录（§6.1）。

### 4.2 `CoreCatalog`

updater 不直接把 `ValidatedManifest` 交给上层，而是包一层 newtype，放在 `nyanpasu-updater`：

```rust
/// A validated manifest narrowed to the core slots this application installs.
pub struct CoreCatalog {
    manifest: ValidatedManifest,
    platform: SupportedPlatform,
}
```

构造时对每个槽位检查：

1. resource 存在，且 `category == "core"`。
2. 该 resource 在本平台上的每个变体恰好有一个 `channel` 组的 tag。
3. 槽位的 channel 下，本平台恰好有一个基础变体。

- 不满足的槽位标记为"清单不可用"，带结构化原因；其他槽位照常工作。清单里应用不认识的 resource 与 channel 被忽略。
- 这三条是 nyanpasu 对清单的约定，不是 manifest 协议。manifest 仓用一条生成器测试断言默认配置满足它（任务 M2），防止配置改动悄悄破坏应用。
- 否决"把 `default_variants` 改成 平台 → channel → 变体"：manifest 协议里 channel 只是一个普通 tag 组，为一个消费者的分组方式改协议不值得；基础变体的约定已经能唯一确定默认值。

`CoreCatalog` 对外提供：

- `variants(slot)`：该槽位在本平台可选的变体，即 tag 集合含槽位 channel tag 的那些。带展开后的 tag、`ChecksumKind` 与当前策略下的裁决。
- `base_variant(slot)`：默认变体。
- `resolve(slot, choice)`：把持久化的选择解析成具体变体（§4.3）。

所有对 `manifest-core` 的调用都以 `CoreCatalog` 选定的 `variant.id` 为参数。上层拿不到绕过槽位的入口。

### 4.3 变体选择的持久化

应用配置新增一个键，按槽位保存选择：

```rust
/// The variant chosen for a core slot. Absent means the slot's base variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct CoreVariantChoice {
    /// The exact tag set; the semantic identity, as in the manifest.
    pub tags: BTreeSet<String>,
    /// The alias the user saw when choosing. Display only.
    pub variant_id: String,
}
```

- 类型放 `nyanpasu-config`，只用字符串，不依赖 manifest crate。键用 `#[serde(default)]`，旧配置文件能读（`rust.md` §6）。
- tag 原样保存，不转小写、不修剪，与 manifest 的比较规则一致。
- 解析只在 `variants(slot)` 内进行，只比较 tag 集合；保存的 `variant_id` 不参与匹配，也不传给 `select_variant`。
  1. 没有选择 → 基础变体。
  2. 有选择，且 `variants(slot)` 里有 tag 集合相同的变体 → 用它。它的 `id` 与保存的别名不同时更新别名。
  3. 有选择，但 `variants(slot)` 里找不到 → 这次操作用基础变体，返回"所选变体已不存在"的提示；不改写保存的选择，直到用户重新选择。
- 写入选择的变更命令在提交前用当前清单核对：tag 集合必须属于该槽位的 `variants(slot)`。不含槽位 channel tag 的选择被拒绝。没有可用清单时拒绝写入。
- 选择变化不触发下载。它只决定下次安装或更新取哪个产物。界面在"所选变体与已安装变体不同"时提示可以重新安装。

### 4.4 已安装内核的记录

每个槽位在 `<data>/core-bin/<binary_name>/current.json` 有一条记录。类型放 `nyanpasu-contracts`：它由 updater 构造、由 `nyanpasu-os` 的安装实现落盘、由 core 读取，三者都依赖 contracts，彼此不依赖。

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct InstalledCoreRecord {
    /// SHA-256 of the executable, lowercase hex. Names the directory holding it.
    pub binary_sha256: String,
    /// The version the executable reported when it was installed.
    pub version: String,
    pub installed_at: String,
    pub provenance: CoreProvenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CoreProvenance {
    /// Downloaded and verified against the manifest by this application.
    Manifest {
        resource: String,
        tags: BTreeSet<String>,
        variant_id: String,
        /// The verified artifact digest, `<algorithm>:<hex>`.
        artifact_checksum: String,
        /// Whether the digest came from the signed manifest or was fetched.
        anchored: bool,
    },
    /// Copied from the install directory by the migration in §9.3.
    Migrated,
}
```

- contracts 不依赖 manifest crate，所以这里不用 `ChecksumKind`：`anchored` 为真对应 `authoritative`，为假对应 `parsed`。其他 kind 进不了安装流程。
- 没有记录、记录解不出、或记录指向的文件不存在，槽位都按"没有受管内核"处理。应用不为没有记录的文件补写记录。
- 它取代 `CoreDistribution` 设想的用途（"让 `/status` 说出装的是哪个构建"），并且直接用 manifest 的 tag 字符串。

### 4.5 槽位状态

新增查询 `list_core_slots`，一次返回全部槽位，取代前端逐个读版本：

```rust
pub enum CoreSlotSource {
    /// Nothing installed for this slot.
    Missing,
    /// The read-only binary shipped in the install directory.
    Bundled,
    /// A binary this application installed into the data directory.
    Managed(InstalledCoreRecord),
}
```

- 每个槽位还带当前版本与变体选择。受管内核的版本取自记录；随包内核的版本靠探测。
- 清单相关的信息（可更新版本、可选变体、checksum 裁决）由另一个查询 `inspect_core_catalog` 返回。它需要网络，失败不应影响本地状态的显示。

---

## 5. manifest 仓的修订

### 5.1 M1：产物的 `binary` 字段

`ResourceArtifactTemplate` 与模板侧的 `TemplateArtifact` 增加可选字段：

```rust
/// The executable's path inside the artifact archive. Absent when the
/// artifact is the executable itself or a single compressed stream.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub binary: Option<String>,
```

- 值是模板，可引用 `{version}` 等与 `artifact` 相同的变量；渲染结果随下载计划返回。
- 路径用 `/` 分隔，相对归档根；不得为空、不得以 `/` 开头、不得含 `..` 段或反斜杠。违反时校验失败。
- 这是可选字段的新增。manifest 仓的版本规则写明首次发布前的变更都计入 `2.0`。
- 宿主的解包规则：

| 产物形态 | `binary` | 宿主行为 |
| --- | --- | --- |
| 裸可执行文件 | 无 | 原样使用 |
| 单流压缩（`.gz`） | 无 | 解压得到可执行文件 |
| 归档（`.zip`、`.tar.gz`） | 有 | 取出该路径的条目；找不到则失败 |
| 归档 | 无 | 失败，报"清单没有给出归档内路径" |

- 取出的条目必须是普通文件。链接、设备等其他类型，以及同一路径出现多次，都按失败处理。解出的大小有上限（§8.1）。
- 宿主不再按文件名猜。各内核归档内的实际路径在 M1 用真实资产核对后写入生成器配置；generator 在抓取快照时无法验证归档内容，所以另加一条下载真实产物的手动核对任务，不进定时流程。
- Meow 归档里随内核发布的其他文件（agent skill）仍由应用按现有的 companion 机制处理，不进 manifest。

### 5.2 M2：基础变体约定的测试

manifest 仓在生成器测试里断言：默认配置的每个 `category: "core"` 的 resource，在每个它支持的平台上，每个 channel 恰有一个只带 channel tag 的变体。不改 schema，不改 `validate_manifest`。

### 5.3 M3：只验签的依赖面

`manifest-signature` 现在依赖完整的 `minisign`（含签名）。应用与服务只需要验签。给它加一个默认关闭签名的 feature，或让验签路径只依赖 `minisign-verify`。做法由 manifest 仓的任务卡决定，对本设计的要求只有一条：应用与服务的依赖图里不出现签名用的密钥处理代码。

### 5.4 M4：可在仓外使用的下载器

构建脚本需要在本仓里验签并下载内核（§8.2）。`@manifest/downloader` 现在只能在 manifest 仓的工作区内运行。manifest 仓提供一个可在外部调用的形态：发布包，或带 wasm 的单文件构建。它必须保留现有行为：先验签再解码、显式校验策略、校验通过后才落盘。另外它要按产物的 `binary`（§5.1）给出可执行文件：直接产出解包后的文件，或在结果里返回渲染后的归档内路径。现在的下载器只返回归档路径，所以这一条依赖 M1。

### 5.5 依赖方式

- 应用与服务用 git 依赖引用 `manifest-core`、`manifest-metadata`、`manifest-parser`、`manifest-signature`，固定到提交。manifest 仓打出首个版本 tag 后改为固定 tag。
- 在本仓，依赖只放在 `nyanpasu-updater`。`nyanpasu-config`、`nyanpasu-contracts`、`nyanpasu-os` 都不依赖 manifest crate。
- 需要经 RPC 给前端的 manifest 类型（`ResourceTag`、`ChecksumKind`）由 updater 再导出，前端绑定照常由 specta 生成。

---

## 6. 目录与安装

### 6.1 布局

```text
<install>/                          只读
  clash-nyanpasu[.exe]
  mihomo[.exe]                      随包内核（只有这一个）
  nyanpasu-service[.exe]
<data>/
  core-bin/
    <binary_name>/
      current.json                  InstalledCoreRecord；替换它就是提交
      <binary_sha256>/
        <binary_name>[.exe]         可执行文件
        artifact-<checksum>         下载的原始产物，按自身 digest 命名（仅有锚的安装保留）
    .staging/                       暂存，与 core-bin 同一文件系统
  cores/<binary_name>/              内核归档附带的文件（现有，不变）
  cache/manifest/
    bundle.json                     最近一次验签通过的清单与签名
```

- 可执行文件按自身的 SHA-256 放进子目录，文件名保持 `<binary_name>[.exe]`。进程名不变，安装器与任务管理器看到的仍是 `mihomo-alpha.exe`。
- `core-bin` 与现有的 `cores/` 分开：`cores/<binary_name>/` 整个目录会在发布 companion 时被原子替换（`O/core_install.rs`），二进制放进去会被一并换掉。
- portable 模式下 `<data>` 是 `<install>/.data/<app>`，布局相同。

### 6.2 查找顺序

`nyanpasu-contracts` 已经依赖 `nyanpasu-paths`，所以 paths 不能反过来认识记录类型。职责这样分：

- `PathResolver` 只推导路径：受管内核的槽位目录、记录文件路径、给定哈希的可执行文件路径。
- 读记录是 contracts 里的一个端口 `InstalledCoreReader`，由 `nyanpasu-os` 实现并注入 core。它读 `current.json`，返回记录与可执行文件路径；记录缺失、解不出或文件不存在时返回"没有"。
- 现有的 `PathResolver::find_binary_path` 只保留安装目录与开发候选两级；"受管优先"的组合放在 core 里调用它的地方。

合起来的查找顺序：

1. 受管内核；
2. `<install>/<exe>`；
3. 开发候选（仅 debug 构建）。

- 受管内核优先于随包内核：用户更新了 Mihomo stable 之后，随包的旧版本留在安装目录不动，不再被使用。
- 版本探测接收解析好的路径，不再自己推导目录，消除 §2.1 的不一致。
- `open_core_dir` 打开 `<data>/core-bin/`。

### 6.3 安装事务

安装由现有的内核生命周期工作流串行执行（`C/client/core_lifecycle/workflow.rs`），同一时刻只有一个安装或启动在进行。步骤：

| # | 步骤 | 执行者 | 失败时 |
| --- | --- | --- | --- |
| 1 | 下载产物到暂存目录，边写边算 digest，与计划的 checksum 比较 | updater | 删除暂存；无状态变化 |
| 2 | 按 §5.1 解包出可执行文件，算它的 SHA-256 | updater | 同上 |
| 3 | 运行暂存的可执行文件读版本，按 §6.4 与计划的版本比较 | updater | 同上 |
| 4 | 服务模式下，用暂存的文件把这个内核收进服务的内核库（§7.4） | 工作流 | 删除暂存；记录未变，当前内核继续运行。服务库里可能多一个无人引用的条目，由服务清理 |
| 5 | 把可执行文件（与有锚安装的产物）移入 `<binary_name>/<binary_sha256>/` | `BinaryInstaller` | 删除暂存；新目录是孤儿，下次清理 |
| 6 | **提交：** 把新记录写成临时文件并原子替换 `current.json` | `BinaryInstaller` | 返回错误；记录未变 |
| 7 | 该槽位正在运行时，重新启动内核 | 工作流 | 返回 degraded：新内核已安装，但没有运行（`rust.md` 的 degraded 规则） |
| 8 | 清理：删除该槽位下不被记录引用的哈希目录 | `BinaryInstaller` | 只记日志；文件被占用时留到下次 |

- 第 5、6、8 步是一次 `BinaryInstaller::install` 调用，与现在一样是单个等待点。服务收录在它之前由工作流完成，`nyanpasu-os` 不接触服务 IPC。
- 第 5 步遇到已存在的哈希目录时：可执行文件哈希一致则复用它，但新记录需要的证据必须补齐。有锚安装把本次核对过的产物放成 `artifact-<checksum>`，文件名取产物 digest 的十六进制；已有同名文件则不动。产物文件写入后不再修改，记录用 `artifact_checksum` 指明用哪一个。
- 两个不同的归档可能解出相同的可执行文件。它们在同一个哈希目录里各有自己的产物文件，互不覆盖；提交前失败时，旧记录引用的产物仍然完整。
- 第 8 步的清理也删除哈希目录里不被记录引用的产物文件。
- 提交点只有第 6 步的一次改名。任何时刻崩溃，`current.json` 要么是旧记录要么是新记录，两者指向的文件都完整。
- 第 5 步不需要先停止内核：新旧文件在不同目录，Windows 上不存在覆盖正在运行的文件的问题。
- 启动时做一次清理：删除 `.staging/` 的内容与孤儿哈希目录。
- `BinaryInstaller` 端口保留，请求改为携带暂存路径、目标槽位与 `InstalledCoreRecord`。请求仍然不含 updater 的类型。
- 提权路径删除：`InstallCoreBinaryError` 的 `AuthorizeCoreCopy`、`StartElevatedCopy`、`ElevatedCopyFailed` 三个变体，以及 `ElevationOperation::CoreBinaryCopy`。
- 桌面与 server 两个组合根都不再把安装目录传给 updater。server 原先注入 `RefuseElevation`，在安装目录不可写时无法更新内核；改后不再需要提权。
- 更新前的配置预检（sing-box 设计 D11）接在第 3 步之后，本设计不改变它。

### 6.4 版本核对

第 3 步比较"清单里的版本"与"二进制报告的版本"。两者的写法因内核与 channel 而异，所以比较规则是一张按槽位的纯函数表，放在 updater：

| 槽位 | 规则 |
| --- | --- |
| `mihomo`、`mihomo-alpha` | banner 里的版本字段与清单版本相等 |
| `clash-rs` | 清单版本去掉前缀 `v` 后与 banner 的版本相等 |
| `clash-rs-alpha` | 清单版本形如 `<stable>-alpha+sha.<commit>`；banner 的版本以 `<stable>` 开头且含 `<commit>` |
| `meow` | 清单版本去掉前缀 `v` 后与 banner 的版本相等（现有规则） |
| `meow-alpha` | 清单版本形如 `alpha-<sha>`；banner 的 SemVer 构建字段与 `<sha>` 相等，不区分大小写（现有规则） |

- 表里除 Meow 两行外的写法是按上游的版本格式推定的，A4 用六个槽位的真实二进制核对后定稿；核对结果与本表不符时修订本表。
- 每个槽位都有"相符"与"不符"的测试用例；Meow alpha 另有一个"构建字段比期望值长、只是前缀相同"的不符用例。

### 6.5 数据目录根下的旧文件

现有查找会认 `<data>/<exe>`。没有代码往那里写，但用户可能手工放过。新的查找不再认它。启动时如果发现这样的文件，记一条 WARN 日志并在内核管理页给出提示；不移动、不删除、不为它写记录。

---

## 7. 服务模式

### 7.1 威胁、边界与承诺

- 资产：服务的 SYSTEM / root 身份。
- 现状：在用户数据目录放一个同名文件，服务就会执行它（§2.2）。
- 能写该目录的进程通常以同一用户身份运行，而这个用户也在服务的 ACL 里。所以"放文件"与"向服务发请求"是同一个主体能做的两件事。只堵住前者而允许后者提交任意二进制，并不构成边界。

因此本设计的承诺分两档，取决于管理员开关 `allow-unanchored-cores`：

| 开关 | 服务会执行什么 | 承诺 |
| --- | --- | --- |
| 关闭 | 只有有锚的内核：服务自己用签名清单核对过的，或取自管理员控制的安装目录的 | 被授权用户无法让服务把清单与安装目录之外的文件当作内核主程序执行 |
| 开启 | 另外接受调用方担保的内核（无锚） | 与现状等价：被授权用户能让服务执行他提供的二进制。区别只在于必须经过带审计的显式请求，不能靠放文件 |

无论开关如何，下面两点都成立：

1. 服务启动的内核主程序一定来自自己的内核库。
2. 主程序的字节在收录时已经固定，之后改动来源文件不影响它。

**承诺只覆盖内核主程序。** 内核进程还会加载别的可执行内容，本设计没有把它们收进信任边界：

- 内核的工作目录仍是用户数据目录（§7.3）。Windows 的 DLL 搜索顺序包含当前目录。
- 服务现在把 `wintun.dll` 从应用数据目录复制给内核使用（runtime 的 native store）。clash-rs 用相对名 `wintun.dll` 加载它。
- 所以即使开关关闭，能写用户数据目录的进程仍可能通过替换 DLL 让代码在高权限的内核进程里运行。

关掉这条路径需要让服务从自己保管的位置提供经过核对的 DLL，并阻止内核从用户可写的位置加载。这涉及 wintun 的分发与内核的工作目录，超出内核主程序的分发。它列为后续工作（§13 的 U6）；在它完成之前，发布说明与界面都不得把"开关关闭"描述成完整的提权防护。

### 7.2 内核库

```text
<service_data_dir>/cores/
  <binary_sha256>/
    <binary_name>[.exe]
    entry.json            来源：有锚或无锚、core_type、收录时间
```

- 目录由服务自己创建，并在每次打开时验证，不能只套用现有的加固函数。验证从文件系统根开始，覆盖 `<service_data_dir>` 的每个祖先：可写的祖先能把整个库换掉（Windows 上父目录的删除子项权限就足以改名）。
  - 路径分两段，规则不同。**系统祖先**是服务专有目录之上的各级（如 Windows 的 `C:\`、`C:\ProgramData`，Unix 的 `/`、`/var/lib`）；**服务专有目录**是其下由服务创建的各级，直到条目文件。
  - 两段共同的要求：每一级都不是符号链接或重解析点；属主是可信的系统身份（Unix 上 root；Windows 上 SYSTEM、Administrators 或 TrustedInstaller）；非管理员不能修改它的安全描述符。
  - 系统祖先的判据是"非管理员不能替换路径上的下一级"：不能把下一级改名、删除后重建。允许非管理员在其中创建无关的子项。Windows 的 `C:\ProgramData` 默认让 Users 创建子项但不能删除或改名别人的目录，符合这一条；Unix 上带 sticky 位、属主为 root 的目录同理。
  - 服务专有目录更严：Unix 上组与其他用户不可写；Windows 上 DACL 只给 SYSTEM 与 Administrators 写权限，并关闭继承。
  - 条目是普通文件，硬链接数为 1。
- 验证与使用针对同一组已打开的对象：服务逐级打开目录并在句柄上检查，从最后一级的句柄下打开条目文件，并持有这些句柄直到内核进程启动完成。这样验证之后、执行之前，路径上的任何一级都不能被替换。
- 服务的安装步骤负责让 `<service_data_dir>` 及其服务专有的祖先（如 Windows 上的 `ProgramData\<service>`）满足这些条件；系统目录本身（`ProgramData`、`/var/lib`）按系统默认的管理员属主与权限通过检查。
- 预先存在但不满足这些条件的目录或文件不被采用：服务拒绝启动内核并报告库不可信，不尝试修复后继续用。
- 每个 `core_type` 保留最近使用的两个条目，其余在收录成功后清理。

### 7.3 按哈希启动

每一个会让服务运行内核程序的请求都增加一个字段，指明要执行的二进制。这包括启动、协调，也包括配置检查（`/core/check` 同样会运行内核）：

```rust
pub struct CoreBinaryRef {
    pub sha256: [u8; 32],
}
```

- 服务在库里找这个哈希的条目，核对条目的 `core_type` 与请求一致，再按当前的开关检查条目是否允许执行（开关关闭时无锚条目不可执行），然后运行。三种请求走同一段检查。
- 条目不存在时返回专用错误 `CoreNotIngested`，应用据此发起收录后重试。
- 服务不再按名字查找：`find_binary_path` 删除，用户数据目录与应用目录都不再是查找位置。`--nyanpasu-data-dir` 保留，继续用作内核的工作目录。
- 这样服务运行的一定是应用记录里的那个二进制。库里有旧条目不会遮蔽新版本。

### 7.4 收录

IPC 新增收录操作。服务按 `core_type` 推导允许的 resource 与 channel（与 §4.1 相同的表），平台取服务自身的编译目标；这些都不由调用方提供。

```rust
pub struct CoreIngestReq<'a> {
    pub core_type: Cow<'a, CoreType>,
    pub evidence: CoreEvidence<'a>,
}

pub enum CoreEvidence<'a> {
    /// `artifact` is the file as downloaded. The service verifies the manifest
    /// signature, finds the artifact's embedded checksum and extracts the
    /// binary itself.
    SignedManifest {
        artifact: Cow<'a, Path>,
        manifest: Cow<'a, [u8]>,
        signature: Cow<'a, str>,
        variant_id: Cow<'a, str>,
        version: Cow<'a, str>,
    },
    /// `binary` is an executable the caller vouches for.
    CallerAsserted {
        binary: Cow<'a, Path>,
        sha256: [u8; 32],
    },
    /// The service reads `<app_dir>/<binary_name>` itself; `app_dir` is the
    /// directory an administrator gave it at install time.
    InstallDirectory,
}
```

服务的处理：

1. 打开来源文件，把内容读进库下的暂存文件，读的同时计算哈希。之后的校验都针对这份私有拷贝，不再读来源。来源必须是普通文件；大小有上限。`InstallDirectory` 的打开方式见第 2 步。
2. 按证据核对：
   - `SignedManifest`：
     - 用服务内置的公钥对清单的原始字节验签，然后解码并 `validate_manifest`；
     - 按 `core_type` 得到 resource 与 channel，要求 `variant_id` 指向的变体属于该 resource、带该 channel tag、有本平台的产物；
     - `plan_download` 取该版本的 checksum，要求 kind 为 `authoritative`；
     - 核对私有拷贝的 digest，按清单的 `binary` 与 §5.1 的规则解包。
     - 结果为有锚。
   - `CallerAsserted`：开关关闭时直接拒绝。核对私有拷贝的 SHA-256 与请求一致。结果为无锚。
   - `InstallDirectory`：验证与读取必须针对同一个已打开的对象，不能先按路径检查、再按路径读。
     - 从文件系统根开始逐级打开目录，每一级都在已打开的句柄上检查，规则与 §7.2 相同：系统祖先按"非管理员不能替换下一级"判断，`<app_dir>` 及其下各级按服务专有目录的严格规则判断（非管理员不可写）。`<app_dir>` 的每个祖先都在检查范围内，因为能替换祖先的人能把整个目录换掉。
     - 在最后一级目录的句柄下打开文件，在文件句柄上做同样的检查，然后从这个句柄读出内容。第 1 步的读取对这种证据就是这次读取。
     - 任何一级不满足即拒绝，应用改用其他证据。通过则结果为有锚（由管理员控制的安装位置担保）。
3. 把可执行文件换进 `cores/<sha256>/`，写 `entry.json`。
4. 返回可执行文件的 SHA-256 与有锚标记。任何一步失败都删除暂存文件，库不变。

- 收录与启动、协调经服务里同一个命令队列串行执行。
- 每次收录写一条审计日志：调用方、证据种类、哈希、结果。

应用选择证据的规则：

| 内核 | 证据 |
| --- | --- |
| 受管，`anchored` 为真 | `SignedManifest`，产物取自记录的 `artifact_checksum` 对应的 `artifact-<checksum>`，清单取自缓存。产物或清单缺失、清单已不含该版本时，重新安装该槽位 |
| 受管，`anchored` 为假（alpha） | `CallerAsserted`，哈希取自记录。提交前重新计算文件哈希，与记录不符则拒绝提交并提示重新安装 |
| 受管，来源为 `Migrated` | 不收录。提示重新安装 |
| 随包 | 先试 `InstallDirectory`。被拒绝（按用户安装、portable）时：在线则把该槽位安装为受管内核后走第一行；离线则按 `CallerAsserted` 提交安装目录里的文件 |

- 应用只为"记录里有、哈希相符"的受管文件，以及安装目录里的随包文件担保；不为数据目录里其他文件担保。这是减少误用的措施，不是边界：边界在 §7.1 的开关。
- 重新安装需要网络。离线且无法收录时，服务模式不可用，应用按现有的规则报告并提示。

### 7.5 版本错位

服务状态新增一个能力标识 `core-store`，在服务完整实现 §7.2–§7.4 之后才声明；旧服务的状态里没有这个字段，按不支持处理。这是独立的新标识，只是借用 sing-box 设计 D12"显式检查、缺省即不支持"的原则。

- **新应用 + 旧服务。** 旧服务仍会先执行用户数据目录根下的同名文件，这个问题只能靠升级服务消除。新应用发现服务不声明 `core-store` 时，不向它发送任何内核启动、检查、重启或恢复请求，统一返回"需要升级服务"，并提供现有的服务升级入口。
- **旧应用 + 新服务。** 旧应用的请求不带 `CoreBinaryRef`。新服务拒绝这样的启动请求并返回"需要升级应用"。不保留按名字查找的兼容路径。
- 服务与应用因此必须一起发布：应用的发布说明与服务的最低版本一并更新（任务 A7）。

### 7.6 剩余风险

- 开关开启时，被授权用户能让服务执行任意二进制。这是 alpha 通道的固有性质：上游的滚动构建没有可以预先签名的 digest。应用在界面上标出无锚的槽位。
- `InstallDirectory` 信任管理员控制的安装目录。全机安装被有管理员权限的攻击者改写不在防御范围内。
- 内核主程序之外的可执行内容不在边界内（§7.1）。
- 服务内置的清单公钥随服务发布。轮换密钥需要发布新的服务。

开关的默认值列在 §13 待确认。

---

## 8. 更新流程

### 8.1 运行时

```text
取清单与签名（经镜像） → 对原始字节验签 → 解码 → validate_manifest → CoreCatalog
    → 选变体（§4.3） → plan_version
        static：读 latest
        remote：按计划请求各 part → finish_version
    → plan_download(version)
        static：计划里已有 authoritative checksum
        remote：请求 checksum 资源 → finish_checksum（parsed）
    → check_policy → §6.3 的安装事务
```

- **清单来源。** `https://libnyanpasu.github.io/resources-manifest/resources.json` 与同名的 `.minisig`。清单仓的 `manifest` 分支经 GitHub raw 与现有镜像提供后备来源。清单走哪个来源不影响信任，因为验签在解码之前。
- **公钥。** 编译进应用，取自 manifest 仓提交的 `manifest.pub`。支持同时信任多把，为轮换留余地。
- **缓存。** 验签通过的清单字节与签名合成一个文件 `bundle.json`，写临时文件后原子替换，所以两者总是配对。启动与离线时读它，读出后重新验签。
- **防重放。** 应用要求清单带 `updated_at` 且能解析为 RFC 3339 时间；缺失或无效的清单被拒绝。新取到的清单时间早于缓存时拒绝并继续用缓存；比较的是时间值，不是字符串。这只防网络上的重放。缓存在用户可写的目录里，不是可信的防降级状态；服务不依赖它（§7.4 的有锚核对只看签名与 digest）。
- **校验策略。** stable 槽位只接受 `authoritative`。alpha 槽位设 `allow_parsed`。`allow_snapshot` 不开：离线时不安装 alpha。策略由 updater 按槽位的 channel 决定，不做成用户设置。
- **镜像。** 对每个请求，只在清单声明支持该请求 registry 的镜像里选；没有可用镜像时不带镜像规划。例如 clash-rs alpha 的版本请求走 `github-api`，`nyanpasu` 镜像不支持它，这些请求直连。请求失败后退回计划里的 `canonical_url`。现有 `mirrors.rs` 的 URL 拼接删除；测速只用来在清单声明的镜像里排序。
- **大小限制。** 分四类，各自有常量上限：清单与签名；版本与 checksum 响应（沿用 `manifest-core` 的 parser 上限）；产物下载；解包后的可执行文件。产物流式写盘并计算 digest，不整体读进内存。
- **HTTP。** 仍由 `nyanpasu-updater` 的后端执行，沿用现有的代理与超时设置。
- **依赖方向。** updater 依赖 manifest crate、`nyanpasu-config` 与 `nyanpasu-contracts`，不依赖 core；与现状相同。
- **错误。** manifest 的错误带稳定的 `code`、`path` 与字段。updater 的错误类型按来源分变体并保留这些字段（`rust.md` §7.2），前端按 `code` 选文案。

删除的代码：`ManifestVersion`、`ManifestVersionLatest`、`ArchTemplate`、`get_matches`、`get_arch`、`get_download_path`、`resolve_rolling_versions` 及其三个上游专用解析、`verify_sha256` 的 Meow 专用分支、`extract_core` 的文件名猜测。`verify_meow_version` 并入 §6.4 的规则表。这些删除与它们的消费者（facade、RPC 命令、前端）的迁移在同一张任务卡里完成。

### 8.2 构建时

- 安装包只带 Mihomo stable 与服务。`externalBin` 改为 `sidecar/mihomo` 与 `sidecar/nyanpasu-service`。
- 准备脚本通过 manifest 仓的下载器（§5.4）取内核。它用提交在本仓的公钥验签，stable 只接受 `authoritative`，alpha 显式放开 `parsed`。准备脚本自己不解析清单、不拼 URL。
- 开发时仍然需要六个内核。`pnpm run prepare` 在开发模式下经同一个下载器把其余五个取到 `backend/tauri/sidecar/`，由开发候选路径找到；它们不进 `externalBin`。
- 在下载器可用之前，构建继续读 v1 清单 `manifest/version.json`。v1 清单与它的生成脚本在那之后仍然保留，为已发布的应用版本服务，直到这些版本不再维护。

---

## 9. 安装包与安装器

### 9.1 不在安装时下载

否决由 nyanpasu-installer 下载内核到数据目录：

| 问题 | 说明 |
| --- | --- |
| 写到谁的数据目录 | 全机安装时安装器整体提权。管理员代普通用户安装时，写进的是管理员的 `LocalAppData`。安装期没有以使用者身份执行任务的步骤 |
| 能力 | 需要补齐清单解析、镜像、重试、minisign 与 digest 校验。这些应用里都会有，等于实现两遍 |
| 覆盖面 | 安装器只支持 Windows。macOS 与 Linux 仍然需要应用内的方案 |
| 离线 | 用户常在没有代理时安装代理软件。安装必须在离线时得到一个能用的内核 |
| 多用户 | 一次全机安装对应多个用户的数据目录，安装器只能处理其中一个 |

采用：安装包带一个默认内核；其余由应用按需下载。

### 9.2 安装器的改动

- 安装器的文件集来自 `tauri.conf.json`，`externalBin` 缩减后自动生效，不需要改安装器代码。
- `installer.toml` 的 `[processes] names` 保留全部六个内核名：升级与卸载时仍要结束从数据目录启动的内核进程。
- 卸载时"删除用户数据"已经覆盖 `<data>`，受管内核随之删除。不勾选时保留，与其他用户数据一致。服务的内核库需要在卸载服务时删除。runtime 的卸载命令现在只停止并注销服务，不删数据目录；Windows 上由应用安装器另行清理。R3 把删除内核库加进服务的卸载命令，使三个平台一致。

### 9.3 升级迁移

安装器升级时会删除新版本不再包含的文件。直接缩减 `externalBin` 会让正在用 clash-rs 或 meow 的用户在升级后丢失内核。分两个版本处理：

1. **版本 N（迁移版）。** 仍然带全部六个内核。应用启动时对每个非默认槽位检查：安装目录里有该内核、且槽位没有受管记录，就把它装成来源为 `Migrated` 的受管内核：复制该文件到暂存、计算 SHA-256、运行它读出版本、移入哈希目录、提交记录。迁移没有清单计划，不做版本比较，也不收进服务。失败只记日志。
2. **版本 N+1。** `externalBin` 缩减。此时用过版本 N 的用户已经在数据目录里有自己的内核。

选中的槽位缺失时（跳过版本 N、迁移失败、或首次安装后选了非随包内核）：

- 内核不启动。应用不自动换成别的内核：不同内核的能力与受管配置不同，替用户换内核会带来未经确认的行为变化。
- 界面给出两个明确的操作：下载所选内核；或切换到随包的 Mihomo。切换走现有的换内核工作流，会改写保存的 `core` 选择。
- 状态查询把这种情况报告为"所选内核未安装"，与内核崩溃区分。

来源为 `Migrated` 的内核在非服务模式下正常使用；服务模式下需要重新安装（§7.4）。界面对这类槽位提示重新安装。

---

## 10. 与 sing-box 设计的关系

[`schema-compat.md`](../2026-11-11-sing-box-core/schema-compat.md) §4 设计了一份独立的 `manifest/v2/cores.json`。按 2026-11-14 的裁定，它被本设计取代：

| schema-compat 的内容 | 处理 |
| --- | --- |
| §2、§3 世代的定义、按世代写受管设定 | 不变 |
| §4.2 新文件 `manifest/v2/cores.json` 与新生成脚本 | 不采用。清单是 manifest 仓的 `resources.json` |
| §4.3 结构：`releases[].channel`、`generation`、`artifacts.*.name/binary/sha256` | 不采用。channel 是变体的 tag；`binary` 进 manifest（§5.1）；digest 是 static 历史里的 `checksum` |
| §4.3 `generation` 字段 | 不进清单。世代由版本号纯函数得出（`1.14.4` → `1.14`），应用在 static 历史里筛出受支持世代的版本再取最新 |
| §4.4 选版本的四步 | 规则不变，数据来源改为 static 历史 |
| §4.5 每个世代保留最新补丁版 | 需要 manifest 的 history 来源支持按版本段保留，而不只是"最新 N 个"。作为 manifest 仓的后续修订，随 S5 提出 |
| §4.6 随包内核按同一规则取版本 | 不变 |
| §6 升级预检 | 不变，接在 §6.3 第 3 步之后 |

- 世代门控是 nyanpasu 侧的规则，放在 `CoreCatalog` 之上的一层筛选里。Clash 家族不筛。
- sing-box 设计的 `core-manager.md` 计划把 `CoreDistribution.kind` 改成 `CoreKind`。`CoreDistribution` 删除后这一条作废。
- 落地 S5 时在 sing-box 的设计文档里加注指向本设计。本次不改那些文档。

---

## 11. 类型清理

| 类型或函数 | 位置 | 处理 |
| --- | --- | --- |
| `VariantTag`、`CoreDistribution` | `R/nyanpasu-core-metadata/src/dist.rs` | 删除整个模块。无生产使用者；小写化与 manifest 语义不一致 |
| `ClashCoreResourceVariant` | `R/nyanpasu-core-metadata/src/kind.rs` | 删除。无使用者；与 `ClashCoreType` 重复 |
| `CoreKind { Clash, SingBox }` | 同上 | 本设计不动。它与 sing-box S3 的计划有关，由那边决定 |
| `ClashCoreKind` | 同上 | 保留。它是行为轴，随日志帧上线 |
| 服务的 `find_binary_path` | `R/nyanpasu-service-runtime/src/server/manager_bridge.rs` | 删除，由按哈希取用取代（§7.3） |
| `ManifestVersion` 等 v1 结构与解析 | `U/mod.rs`、`U/shared.rs`、`U/instance.rs` | 删除，见 §8.1 |
| `fetch_latest_core_versions` 及其返回类型 | `C/client/mod.rs`；`backend/nyanpasu-rpc/src/commands/updater.rs` | 由 `inspect_core_catalog` 取代，与上一行同卡 |
| `data_or_sidecar_path` | `P/lib.rs:209` | 删除。无调用方 |
| `SidecarCoreVersionReader` 的目录推导 | `O/core_version.rs` | 改为接收解析好的路径；名字随之改为不带 sidecar |
| 提权复制 | `O/core_install.rs`；`ElevationOperation::CoreBinaryCopy` | 删除 |
| 前端 `kebabCase` 合并最新版本 | `frontend/query/src/ipc/use-clash-cores.ts` | 由类型化的查询结果取代 |

runtime 仓 `dist` 模块的删除是破坏性的公开 API 变更，但没有调用方；随该仓的下一个版本发布。

---

## 12. RPC 与前端

命令：

| 命令 | 类别 | 传输 |
| --- | --- | --- |
| `list_core_slots` | 查询，只读本地状态 | 沿用现有 `get_core_version` 的范围 |
| `inspect_core_catalog` | 查询，会发起网络请求 | 沿用现有 `fetch_latest_core_versions` 的范围 |
| `set_core_variant` | 变更 | 沿用应用配置变更命令的范围 |
| `update_core`（含首次下载） | 变更 | 只对桌面开放，不变。HTTP 的回归测试保留 |

- 移除提权不改变下载命令的暴露范围。WebUI 上缺失的槽位显示为不支持下载，并说明需要在桌面端操作。把下载开放给 HTTP 需要单独的效应审查（`rpc.md`），不在本设计内。
- 命令的目标、能力与生成的目录按 `rpc.md` 同步更新，新命令在其开放的每个传输上验证。

页面：

- 内核管理页按槽位列出：来源（随包、受管、缺失）、版本、变体、可更新版本。
- 每个槽位有变体选择。选项来自 `inspect_core_catalog`，直接渲染 manifest 展开的 tag；`tag.id` 作翻译键，`name` 作回退（manifest 设计的约定）。
- 每个变体显示 `ChecksumKind` 与策略裁决。`parsed` 显示提示；被策略拒绝的变体不可安装并说明原因。
- 需要更高 CPU 级别或不同 libc 的变体，界面显示 tag 的 `description`。应用不自动检测 CPU 级别来替用户选择；默认始终是基础变体。
- 缺失的槽位显示"下载"而不是"更新"；所选内核缺失时显示 §9.3 的两个操作。
- 服务模式下，无锚与迁移来源的槽位带标记（§7.4、§7.6）。
- 页面经 `@nyanpasu/rpc` 与 `@nyanpasu/query` 调用，不直接读内核名判断（`frontend-packages.md`）。

---

## 13. 待确认

| # | 问题 | 本稿的取值 | 备选 |
| --- | --- | --- | --- |
| U1 | 服务的 `allow-unanchored-cores` 默认值 | 开启，保持 alpha 在服务模式下可用 | 默认关闭，由管理员显式开启；代价是升级后 alpha 槽位在服务模式下不可用，直到管理员改设置 |
| U2 | 是否随包带第二个内核 | 只带 Mihomo stable | 加带 Mihomo alpha，约多 61 MB |
| U3 | 迁移版与缩减版是否必须是两个版本 | 是 | 一个版本内完成，接受升级后所选内核缺失、需要用户手动下载 |
| U4 | 开发模式下其余五个内核的来源 | 准备脚本下载到 `sidecar/` | 开发时也走应用内下载 |
| U5 | 归档内路径与 §6.4 版本规则的实际取值 | M1 与 A4 用真实资产核对 | — |
| U6 | 内核加载的 DLL（首先是 `wintun.dll`）何时收进服务的信任边界 | 本设计不做，另开设计；在那之前承诺只覆盖主程序（§7.1） | 并入本批，R3 之后加一张卡 |

---

## 14. 风险

| 风险 | 影响 | 缓解 |
| --- | --- | --- |
| 清单发布中断或签名密钥丢失 | 无法检查更新 | 缓存的清单继续可用；应用支持多把公钥；已安装的内核不受影响 |
| GitHub API 未认证限流（clash-rs alpha 的版本要两次 API 请求，且无镜像） | alpha 版本解析失败 | 与现状相同；失败是结构化错误，界面提示稍后重试 |
| 上游改归档内的文件名 | 解包失败 | `binary` 在清单里，改清单即可，不需要发应用；失败在提交之前，旧内核仍在 |
| 服务与应用必须同时升级 | 只升级一方时服务模式不可用 | §7.5 的显式提示；应用内已有服务升级入口 |
| 跳过迁移版的升级 | 所选内核缺失 | §9.3 的两个操作 |
| manifest crate 的 git 依赖 | 构建依赖另一个仓的可达性 | 固定提交；首个 tag 后改固定 tag；必要时 vendor |
| 保留原始产物占用磁盘 | 每个有锚槽位多一个归档的大小 | 只保留当前记录引用的那一个；清理随安装进行 |
| 安全软件拦截数据目录里的可执行文件 | 内核无法启动 | 与 portable 模式的现状同级；错误映射为专用种类并提示 |
| B2 依赖 manifest 仓的下载器 | 缩减安装包的时间取决于另一个仓 | A 阶段不依赖它；在它可用之前构建继续用 v1 |

---

## 15. 测试

| 对象 | 方法 |
| --- | --- |
| 槽位映射 | 表格化：六个 `ClashCore` 取值各自的 resource 与 channel |
| `CoreCatalog` | 用 manifest 仓的真实快照构造；逐条破坏三条约定，断言对应槽位不可用而其余可用 |
| 变体解析 | §4.3 的三条规则各一个用例；tag 顺序无关；大小写敏感；另一个 channel 的 tag 集合被拒绝；陈旧别名不影响匹配 |
| 更新流程 | 假 HTTP 后端：static 与 remote 各走一遍；checksum 不符、策略拒绝、版本 part 失败；clash-rs alpha 配 `nyanpasu` 镜像时 API 请求直连；镜像请求失败后退回 canonical |
| 大小限制 | 四类请求各自超限被拒；产物不整体进内存 |
| 清单缓存 | 验签失败不落盘；缺失或无效的 `updated_at` 被拒；时间回退被拒；清单与签名总是配对；离线用缓存 |
| 版本核对 | §6.4 每个槽位"相符"与"不符"各一个用例 |
| 安装事务 | 真实临时目录：在每一步之后注入失败，断言记录与文件的状态符合 §6.3 的表；崩溃后重启的清理；首次安装（没有旧记录）；重启失败报告 degraded；迁移来的目录重新安装为有锚后带有产物，之后启用服务模式能完成有锚收录；两个不同归档解出相同可执行文件时，在第 5、6 步之间失败后旧记录仍能完成有锚收录 |
| 查找顺序 | 受管优先于随包；记录损坏时退回随包；版本探测与启动用同一路径 |
| v1 等价 | 用同一时刻的 v1 清单与 v2 清单，断言六个槽位基础变体的版本与资产名相同（clash-rs `linux-aarch64` 除外，断言为 musl） |
| 内核库的验证 | 受支持的默认布局通过（Windows 的 `C:\ProgramData\<service>\data`，属主含 TrustedInstaller、`ProgramData` 允许 Users 创建子项；Unix 的 `/var/lib/<service>`）；`<service_data_dir>` 的某个祖先能被非管理员替换下一级时被拒；验证之后经父目录替换 `<service_data_dir>` 不能改变被执行的文件；祖先目录是链接、目录属主不对、文件可被非管理员写、条目是重解析点或硬链接、预先存在的不可信目录：各自被拒 |
| 按哈希运行 | 启动、协调、配置检查三种请求各自：库里没有该哈希返回 `CoreNotIngested`；`core_type` 不符被拒；开关关闭时无锚条目不可执行；用户数据目录与应用目录里的同名文件不被执行 |
| 收录 | 有锚：签名错、digest 错、版本不在历史、kind 非 authoritative、`variant_id` 属于另一个 resource 或 channel、归档条目是链接或重复，各自被拒。无锚：SHA-256 不符被拒；开关关闭被拒。`InstallDirectory`：路径任一级（含 `<app_dir>` 的祖先）不满足条件被拒；检查与读取之间替换目录或文件不能得到有锚条目。收录后改动来源不影响库 |
| 版本错位 | 新应用对不声明 `core-store` 的服务不发任何内核请求；新服务拒绝不带 `CoreBinaryRef` 的请求 |
| 迁移 | 安装目录有、没有受管记录时装为 `Migrated`；已有记录时不动；所选槽位缺失时内核不启动并报告专用状态 |
| RPC | 下载与更新的 HTTP 请求仍进不了 updater；新查询在其开放的传输上各验证一次 |
| 准备脚本 | 假下载器：打包模式只取 Mihomo stable；开发模式取其余五个；验签失败时构建失败（`pnpm run test:scripts`） |
| manifest 仓 | `binary` 的解码、校验、渲染与一致性用例；基础变体约定的生成器测试 |

---

## 16. 涉及的规范条款

| 条款 | 落点 |
| --- | --- |
| `backend-packages.md`：updater 不依赖实现 crate 与 GUI；实现 crate 只依赖 foundation | manifest crate 只进 updater；`InstalledCoreRecord` 放 contracts；`nyanpasu-os` 不依赖 updater |
| `backend-packages.md`：OS 机制归 `nyanpasu-os`；paths 不依赖 contracts | 目录移动、记录的读写与清理在 `nyanpasu-os`，经 contracts 的 `BinaryInstaller` 与 `InstalledCoreReader` 端口；`PathResolver` 只推导路径 |
| `rust.md` §5.2、§5.5：有限状态用枚举；工作区内枚举保持穷尽 | `CoreSlotSource`、`CoreProvenance`、槽位映射 |
| `rust.md` §6：持久化字段要能解旧文件 | `CoreVariantChoice` 的键用 `#[serde(default)]` |
| `rust.md` §7.2 与 degraded 规则：类型化错误；提交后的失败报告 degraded | updater 错误保留 manifest 的 `code`、`path` 与字段；§6.3 第 7 步 |
| `rpc.md`：命令是薄适配；HTTP 显式开启并经效应审查 | §12 的传输范围 |
| `testing.md`：纯函数用值测试；宿主效应用本地夹具 | §15 |
| `workflow.md`：一个提交做一件事 | [`tasks.md`](tasks.md) 的卡片划分 |
| manifest 仓 `AGENTS.md` §5：core 无 IO；生成物不手改；宿主显式策略 | M1 经 schema 生成器输出；策略由 updater 与服务显式给出 |

---

## 17. 修订记录

r2 对 r1 的主要改动（codex 审查，2026-11-14）：

- **服务信任模型重写。** r1 允许服务直接执行"通过目录检查"的应用目录，并在缺内核时自动接受调用方担保，同时声称关闭了植入路径。r2 改为：服务只按哈希执行内核库里的文件；承诺按管理员开关分两档写明（§7.1）；内核库的验证覆盖完整路径、属主、链接与预先存在的对象（§7.2）；应用目录改为由服务自己读取并验证后收录（§7.4）。
- **按哈希启动。** 取代"库里有就用"，消除旧副本遮蔽新版本的问题（§7.3）。
- **收录请求不再由调用方指定 resource 与平台**（§7.4）。
- **新应用不向旧服务发内核请求**，不再假设旧服务只会找到随包内核（§7.5）。
- **安装改为内容寻址加单一提交点**，写明每一步的失败结果与服务收录的先后（§6.3）。
- **保留原始产物**，使延后启用服务模式时仍能做有锚收录（§6.1、§7.4）。
- **版本核对改为按槽位的规则表**，保留 Meow 的现有语义（§6.4）。
- **变体解析限定在槽位内**，别名不参与匹配（§4.3）。
- **记录改为带来源枚举**，迁移来源不伪造已验证字段；类型归属 contracts（§4.4）。
- **所选内核缺失时不自动换内核**（§9.3）。
- **构建期必须验签**：准备脚本只经 manifest 仓的下载器取内核，删去手写读取清单的过渡方案（§5.4、§8.2）。
- **镜像按 registry 筛选；四类大小限制分开；缓存的清单与签名合成一个文件并写明防重放的范围**（§8.1）。
- **下载命令保持只对桌面开放**（§12）。
- 事实修正：安装器的降权机制描述、两处行号范围、体积估算改为 131 MB（§2、§0）。

r3 对 r2 的主要改动（codex 审查，2026-11-14）：

- **承诺收窄到内核主程序。** 内核的工作目录与 `wintun.dll` 仍在用户可写的位置，DLL 加载路径没有被本设计关闭；写明并列为 U6（§7.1、§7.6、§13）。
- **`InstallDirectory` 的检查与读取绑定在同一个已打开的对象上**，并覆盖 `<app_dir>` 的祖先（§7.4）。
- **配置检查同样按哈希取用二进制**（§7.3）。
- **服务收录移到 `BinaryInstaller` 调用之前**，安装端口仍是单个等待点，`nyanpasu-os` 不接触服务 IPC（§6.3）。
- **复用哈希目录时补齐证据**（§6.3）。
- **读记录改为 contracts 的端口**，避免 paths 与 contracts 互相依赖（§6.2）。
- **Meow alpha 的版本比较保持精确相等**（§6.4）。
- **下载器要给出可执行文件**，M4 依赖 M1（§5.4）。
- **迁移有自己的步骤**，不借用需要清单计划的安装步骤（§9.3）。
- **卸载服务时删除内核库**，写入 R3（§9.2）。

r4 对 r3 的改动（codex 审查，2026-11-14）：

- **内核库的验证从文件系统根开始**，覆盖服务数据目录的祖先，并持有句柄直到内核启动，使验证后的路径不能被替换（§7.2）。
- **原始产物按自身 digest 命名且不可变**，由记录选择；相同可执行文件的不同归档互不覆盖（§6.1、§6.3）。

r5 对 r4 的改动（codex 审查，2026-11-14）：

- **系统祖先与服务专有目录分开判断。** r4 把专有目录的严格规则套到了所有祖先上，会拒绝 Windows 的默认系统目录。系统祖先改按"非管理员不能替换下一级"判断，可信属主包含 TrustedInstaller；祖先的完整覆盖与在句柄上验证不变（§7.2、§7.4）。
