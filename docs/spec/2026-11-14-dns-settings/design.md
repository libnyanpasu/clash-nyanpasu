# DNS 与 Hosts 设置 设计

**日期：** 2026-11-14
**基线：** `main @ 0f067133f`；runtime submodule pin `v2.0.3`（`3a79dab`）；2026-11-20 rebase 到 `main @ 04592faf0`，下文的修订号、权限与命令位置按 rebase 后更新
**参照：** Clash Verge Rev `dev @ b3da333b5`（v2.5.10）、Clash Party `smart_core @ acae7112c`；mihomo `Meta` 分支 `docs/config.yaml`
**范围：** 在 GUI 中接管运行配置的 `dns` 与 `hosts`：存储、生成、与 TUN 的关系、内核差异、界面；DNS 工具（查询、清缓存），同一轮交付。
**不在范围：** 系统 DNS 接管（macOS `scutil`、Windows/Linux 写系统解析器），见 §9；sing-box 家族的 DNS；sniffer。

---

## 0. 结论摘要

1. **基础设施基本够用，不需要新的配置层。** 后处理的 `Ownership` 模型、能力表 `ConfigFeatures`、`clash-config.yaml` 的修订戳迁移、应用前的内核 `-t` 检查、`dns`/`hosts` 的热重载判定都已存在。缺的是类型化的 DNS 模型、`dns` 键的单一产出者、能力表的 DNS 领域，以及 DNS 工具的 RPC（§1）。
2. **全局设置，存在 `ClashConfig` 里。** 新增 `dns_control`/`dns` 与 `hosts_control`/`hosts` 两组兄弟字段，`clash-config.yaml` 升到修订 5（修订 4 已被 Script 模式的移除占用）。不做 Verge 的按 Profile 开关（§3）。
3. **接管即整表替换。** 接管时 Nyanpasu 以 `Forced` 写入整个 `dns` 表（与 `hosts` 表），界面上看到的就是内核运行的；不接管时完全交给订阅与转换。不做字段级合并，避开 Verge “`false`/空列表清不掉订阅值”与 Party “深合并后不知道最终值”两类问题（§4）。
4. **接管值位于转换与脚本之后。** 与现有 guard 字段同一层：脚本改不动被接管的 `dns`；想让脚本控制 DNS 就关闭接管。
5. **TUN 仍然强制 `dns.enable: true`。** `dns` 键只由一个函数产出，组合“用户设置”与“TUN 的需要”。用户在 TUN 下关闭 DNS 时，构建日志记一条警告，界面就地提示（§5）。
6. **内核差异走能力表。** 新增 `DnsFeatures`；已按下发版本的源码核对 mihomo、clash-rs、meow-rs 三者的差异，sing-box 为 `Ignored`。当前内核不读的字段不写入，前端隐藏（§6）。
7. **界面：** 新路由 `settings/clash/dns`，表单与 YAML 两种编辑方式编辑同一份类型化数据；未覆盖的键放进 `extra` 透传，YAML 能完整往返（§7）。
8. **DNS 工具同一轮交付。** 经 `NyanpasuClient` 暴露内核 DNS 查询、清 fake-ip 缓存、清内核 DNS 缓存，三者开放给 WebUI；clash-api 已有实现（§8）。

---

## 1. 现状与基础设施评估

| 需要的能力            | 现状                                                                                                                                                                                                                                                    | 结论                                                   |
| --------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------ |
| 在订阅/转换之上覆写键 | `Ownership::{Forced, Removed, Default, Inherited, Table}`（`nyanpasu-config/src/runtime/post_process/ownership.rs`），`OwnedFields` 派生宏，`ManageableField<T>`                                                                                        | 够用。整表 `Forced` 与按键 `Table` 都能表达            |
| TUN 对 DNS 的要求     | `tun_ownership` 在 TUN 开启时追加一条 `dns` 的 `Table`：`enable` 强制、`enhanced-mode`/`fake-ip-range`/`nameserver`/`fallback`/`fake-ip-filter` 为默认（`runtime/executor/plan.rs:73-200`）                                                             | 需要改：新设置也要写 `dns`，两个产出者须合成一个（§5） |
| 字段过滤              | `dns` 在默认 `valid` 里；`hosts` 在 `OTHERS_FIELDS` 但不在默认 `valid`，订阅的 `hosts` 默认被过滤（`builtin.rs`、`profiles.rs:540`）                                                                                                                    | 接管值在过滤之后写入，不受影响；不接管时行为不变       |
| 设置存储与迁移        | `ClashConfig`（`clash/config/mod.rs`），修订戳 `SCHEMA_REVISION = 4`，每次形状变化加一个迁移步骤（`nyanpasu-migration/src/modules/clash_config.rs`）                                                                                                    | 够用。加修订 5                                         |
| 修改后生效            | `patch_clash_config` → `RuntimeImpact::Reconcile` → 构建 → 内核 `-t` 检查（`application_workflow/adapters.rs:112`）→ core-manager 判定：`dns`/`hosts` 在 `RELOAD_FIELDS`，`dns.listen` 变化重启（`nyanpasu-core-manager/src/config/mihomo.rs:119-300`） | 够用，无需新命令                                       |
| 内核差异              | `ConfigFeatures` + `FieldSupport<T>`，经 `get_config_features` 查询（`clash/features.rs`，`docs/design/core-specific-config.md`）；目前只有 `tun` 领域                                                                                                  | 需要扩展 `DnsFeatures`                                 |
| 界面三态/编辑器       | `ManagedGuardFieldSelector`（受管/启用/停用），monaco-editor + monaco-yaml 已是依赖（`(editor)/editor/profile`）                                                                                                                                        | 够用                                                   |
| DNS 工具              | clash-api 有 `dns` 查询（`DnsQuery`、`DnsRecordType`）、`flush_fake_ip_cache`、`flush_dns_cache`（`nyanpasu-runtime/crates/clash-api/src/api/{dns,maintenance}.rs`）；`NyanpasuClient` 只暴露系统 DNS 缓存清理                                          | 需要在 `client/clash_api.rs` 加三个方法与 RPC          |
| 系统 DNS 接管         | `DnsController` 与 `dns_sync` 收敛已在 core-manager；macOS `MacosDnsController` 已接线（`actor_v2/local_host.rs:70`），但注释写明未在 macOS 上验证，`desired()` 是占位规则                                                                              | 不在本 spec；见 §9                                     |

`Ownership` 遍历按条目顺序执行（`post_process/apply.rs` 的 `walk`），同一键出现两次时后者覆盖前者，技术上能跑；但快照归因与“谁决定了这个键”会变得含糊，所以 §5 改为单一产出者。

## 2. 参照：Verge Rev 与 Party

| 方面          | Verge Rev                                                                              | Party                                          | 本设计                                      |
| ------------- | -------------------------------------------------------------------------------------- | ---------------------------------------------- | ------------------------------------------- |
| 编辑          | 表单 + Monaco YAML                                                                     | 只有表单                                       | 表单 + YAML，同一份数据                     |
| 存储          | 独立 `dns_config.yaml`；开关按 Profile                                                 | 受控 `mihomo.yaml`；全局 `controlDns`，默认关  | `ClashConfig`，全局，默认关                 |
| 合并          | `dns.extend` 浅合并，过滤“未设置”值（`false`、空列表无法清除订阅值）                   | 递归深合并，数组替换                           | 整表替换                                    |
| 顺序          | 转换前写入，脚本后再强制一次（脚本改动被丢弃并提示）                                   | 所有转换与脚本之后                             | 转换与脚本之后（现有后处理层）              |
| 订阅 DNS 保护 | 订阅含 `nameserver-policy`/`proxy-server-nameserver*` 时开启须确认；订阅变化后自动关闭 | 同左，可在设置里关掉保护；确认只存内存         | 不拦截，改为构建日志 + 界面差异提示（§4.3） |
| 校验          | 保存时只查 YAML 语法；应用时 `-t`                                                      | 保存时无；启动时 `-t`，热重载路径未见 `-t`     | 保存即重建，`-t` 失败则不应用               |
| TUN           | `use_tun` 强制 fake-ip 默认值，但之后的 DNS 覆写可以把它关掉                           | 不强制；DNS 关闭时清空 `tun.dns-hijack`        | 强制 `enable`，其余为默认值（§5）           |
| 系统 DNS      | 脚本，TUN + fake-ip 时设占位 DNS                                                       | 仅 macOS，TUN 启动时设 `223.5.5.5`，退出时还原 | 不在范围                                    |

两者的 issue 里反复出现的问题：订阅 DNS 保护的弹窗与自动关闭让人困扰（Verge #8503，Party #2186）；关闭接管后 TUN + fake-ip 超时（Verge #7986）；macOS 系统 DNS 同步与还原（Verge #8564/#8269/#8526，Party #2274/#2253/#2249）。

## 3. 存储

```rust
pub struct ClashConfig {
    // ...现有字段

    /// Whether Nyanpasu writes the runtime `dns` table.
    #[serde(default)]
    pub dns_control: ControlSource,
    /// Kept while `dns_control` is `Profile`, so switching back restores it.
    #[serde(default)]
    #[patch(nesting)]
    pub dns: DnsSettings,

    #[serde(default)]
    pub hosts_control: ControlSource,
    #[serde(default)]
    pub hosts: Hosts,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ControlSource {
    /// The profiles and transforms decide the key.
    #[default]
    Profile,
    /// Nyanpasu writes the whole key.
    Nyanpasu,
}
```

- **为什么不用 `ManageableField<DnsSettings>`：** `Unmanaged` 不带值，切到“交给订阅”会丢掉用户填好的 DNS。按 `core-specific-config.md` §5.1，选择器与子配置做成兄弟字段，读取方只经视图方法 `ClashConfig::managed_dns() -> Option<&DnsSettings>` 访问；`impact.rs` 的运行时输入判断同样走视图，未接管时改 `dns` 不触发重建。
- **为什么不按 Profile：** Verge 的按 Profile 开关加上全局共享的文件内容，是两套状态；我们的 Profile 已有 Merge/脚本转换可以做按 Profile 的 DNS。全局设置覆盖“我想要一套稳定 DNS”的主要诉求。
- **为什么 `hosts` 单独一个开关：** 订阅很少写 `hosts`，用户常常只想加几条本地映射而保留订阅的 DNS。
- **迁移：** `clash-config.yaml` 修订 3 → 4，迁移步骤只写入 `dns_control: profile`、`hosts_control: profile`；`dns` 与 `hosts` 缺省时按类型的 `Default` 读取。升级后的运行配置与升级前一致。

### 3.1 `DnsSettings`

字段名与 mihomo `docs/config.yaml` 一致（`kebab-case`）。可选字段为 `Option`，`None` 不写入。

| 字段                                                             | 类型                                                            | 说明                                                                                                    |
| ---------------------------------------------------------------- | --------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `enable`                                                         | `bool`                                                          |                                                                                                         |
| `listen`                                                         | `Option<String>`                                                | 变化会重启内核（现有判定）                                                                              |
| `ipv6`                                                           | `bool`                                                          |                                                                                                         |
| `enhanced-mode`                                                  | `EnhancedMode { FakeIp, RedirHost, Normal }`                    |                                                                                                         |
| `fake-ip-range` / `fake-ip-range6`                               | `Option<String>`                                                |                                                                                                         |
| `fake-ip-filter-mode`                                            | `FakeIpFilterMode { Blacklist, Whitelist, Rule }`               | `rule` 为 mihomo 新值，能力表决定是否可选                                                               |
| `fake-ip-filter`                                                 | `Vec<String>`                                                   |                                                                                                         |
| `use-hosts` / `use-system-hosts` / `respect-rules` / `prefer-h3` | `bool`                                                          |                                                                                                         |
| `default-nameserver` / `nameserver` / `fallback`                 | `Vec<String>`                                                   |                                                                                                         |
| `proxy-server-nameserver` / `direct-nameserver`                  | `Vec<String>`                                                   |                                                                                                         |
| `direct-nameserver-follow-policy`                                | `bool`                                                          |                                                                                                         |
| `nameserver-policy`                                              | `IndexMap<String, NameserverList>`                              | 值为单个或列表；保持顺序                                                                                |
| `fallback-filter`                                                | `FallbackFilter { geoip, geoip-code, ipcidr, domain, geosite }` |                                                                                                         |
| `extra`                                                          | `#[serde(flatten)] Mapping`                                     | 其余键原样透传（`cache-algorithm`、`fake-ip-ttl`、`proxy-server-nameserver-policy` 等）；类型化字段优先 |

只做轻量的本地校验：枚举取值、`nameserver-policy` 的键非空。服务器地址的语法（`https://`、`tls://`、`quic://`、`dhcp://`、`system`、`#代理组` 后缀等）交给内核 `-t`，不在 Nyanpasu 复刻。

### 3.2 默认值

接管默认关闭，所以默认值只决定“第一次打开接管时看到什么”。取 Verge Rev（`src/components/setting/mods/dns-viewer.tsx` 的 `DEFAULT_DNS_CONFIG`）与 Party（`src/shared/appConfig.ts` 的 `DEFAULT_MIHOMO_DNS_CONFIG`）的共识；两者分歧时按下表取舍。

```yaml
enable: true
ipv6: false
enhanced-mode: fake-ip
fake-ip-range: 198.18.0.1/16
fake-ip-filter-mode: blacklist
fake-ip-filter:
  - '*'
  - '+.lan'
  - '+.local'
  - '*.arpa'
  - 'time.*.com'
  - 'ntp.*.com'
  - '+.market.xiaomi.com'
  - 'localhost.ptlogin2.qq.com'
  - '*.msftncsi.com'
  - 'www.msftconnecttest.com'
use-hosts: false
use-system-hosts: false
respect-rules: false
default-nameserver: ['tls://223.5.5.5']
nameserver: ['https://doh.pub/dns-query', 'https://dns.alidns.com/dns-query']
proxy-server-nameserver:
  ['https://doh.pub/dns-query', 'https://dns.alidns.com/dns-query']
fallback: []
fallback-filter:
  geoip: true
  geoip-code: CN
  ipcidr: ['240.0.0.0/4', '0.0.0.0/32']
  domain: ['+.google.com', '+.facebook.com', '+.youtube.com']
```

| 字段                              | Verge Rev                                                        | Party             | 取值与理由                                                                                              |
| --------------------------------- | ---------------------------------------------------------------- | ----------------- | ------------------------------------------------------------------------------------------------------- |
| `listen`                          | `:53`                                                            | 不设              | 不设：在 53 端口监听需要权限且与本机解析器冲突；TUN 走 `dns-hijack`，不需要监听                         |
| `ipv6`                            | `true`                                                           | `false`           | `false`：与现有 `overrides.ipv6` 的默认一致                                                             |
| `fake-ip-range6`                  | `fdfe:dcba:9876::1/64`                                           | 不设              | 不设：`ipv6` 默认关                                                                                     |
| `fake-ip-filter`                  | 9 项                                                             | 6 项              | 两者并集，去掉写法不同的重复项（`+.arpa`/`*.arpa` 取 Verge 写法）                                       |
| `default-nameserver`              | `system, 223.6.6.6, 8.8.8.8, 2400:3200::1, 2001:4860:4860::8888` | `tls://223.5.5.5` | Party：Verge 的 `system` 在 Linux 上经 mihomo 自身的 TUN DNS 解析，fake-ip 泄漏到 DIRECT（Verge #8437） |
| `nameserver`                      | `8.8.8.8` + 两个 DoH                                             | 两个 DoH          | 两个 DoH：明文 `8.8.8.8` 在国内常不可达且无加密                                                         |
| `proxy-server-nameserver`         | 空                                                               | 两个 DoH          | Party：节点域名用加密 DNS 解析，避免被污染                                                              |
| `prefer-h3`、`direct-nameserver*` | `false`/空                                                       | 不设              | 不设                                                                                                    |

`hosts` 默认空。默认值只在 Rust 类型的 `Default` 中定义一次；首版界面不提供“恢复默认”，因为前端拿不到这份默认值而不另加命令（见 §12）。

## 4. 生成

### 4.1 位置与优先级

在 `plan.rs::ownership` 里，`dns` 与 `hosts` 的条目由新函数 `dns_ownership(dns: Option<&DnsSettings>, tun: &TunParams, features: &DnsFeatures)` 与 `hosts_ownership(hosts: Option<&Hosts>)` 产出，替换 `tun_ownership` 里现有的 `dns` 条目。执行顺序不变：订阅 → 转换/脚本 → 字段过滤 → 后处理（`Forced`/`Removed` → `Default`）。

| `dns_control` | TUN | `dns` 的 `Ownership`                                                                |
| ------------- | --- | ----------------------------------------------------------------------------------- |
| `Profile`     | 关  | 不产出（与现在相同）                                                                |
| `Profile`     | 开  | 现有 `Table`：`enable` 强制，其余默认（与现在相同）                                 |
| `Nyanpasu`    | 关  | `Forced(整表)`                                                                      |
| `Nyanpasu`    | 开  | `Forced(整表)`，且整表中的 `enable` 写 `true`；用户设置为 `false` 时记 `Warn`（§5） |

`hosts_control: Nyanpasu` → `hosts` 为 `Forced(整表)`，空表时 `Removed`。整表写入，不与订阅的 `hosts` 合并。

### 4.2 为什么整表替换

- **可预测：** 界面里的内容就是内核运行的 `dns`；检视页的快照差异只有一处 `replace /dns`。
- **能清除：** 用户留空 `fallback` 就是没有 `fallback`；Verge 的“未设置过滤”让这类修改静默失效。
- **字段级合并的代价：** 每个字段要三态（交给订阅/启用/停用），20 多个字段的界面与存储都会膨胀，正是 `ManageableField` 只用于个别字段的原因。需要按字段混合时，用户可以关闭接管，改用 Merge 转换。

### 4.3 订阅自带 DNS 的处理

Verge 与 Party 在订阅含 `nameserver-policy`/`proxy-server-nameserver*` 时弹确认并在订阅变化后自动关闭接管，两边都收到了“太烦”的反馈。本设计不拦截：

- 接管时若被替换的订阅 `dns` 含这些键，`ManagedSettings` 步骤记一条 `Warn`，列出被覆盖的键。
- DNS 设置页的接管开关说明接管会替换订阅的整个 `dns` 段；被替换的具体键见检视页中 ManagedSettings 步骤的构建日志。首版不在设置页展示订阅 `dns` 的摘要（见 §12）。

### 4.4 内核能力

`dns_ownership` 只写当前内核读取的字段；某个 Nyanpasu 写入的取值不被支持时按 §6.2 处理并记 `Warn`。`extra` 中的键原样写入，由内核判断。

## 5. 与 TUN 的关系

- TUN 依赖内核 DNS 应答被劫持的 53 端口，所以 TUN 开启时 `dns.enable` 仍为 `true`，无论接管与否。这与当前行为一致，也避免 Verge #7986 那样“覆写关掉 DNS 后 TUN 超时”。
- 接管时 TUN 不再补 `nameserver`/`fake-ip-range` 等默认值：整表来自用户，缺什么就是缺什么，`-t` 会拦下真正无效的组合。
- 不采用 Party“DNS 关闭时清空 `dns-hijack`”的做法，因为 TUN 下 DNS 不会关闭。
- 界面：TUN 开启时，`enable` 开关显示为锁定开启，附说明。

## 6. 能力表

```rust
pub struct ConfigFeatures {
    pub tun: TunFeatures,
    pub dns: DnsFeatures,
}

pub struct DnsFeatures {
    /// The typed keys the core reads; `Ignored` hides the DNS page.
    pub keys: FieldSupport<DnsKey>,
    pub enhanced_mode: FieldSupport<EnhancedMode>,
    pub fake_ip_filter_mode: FieldSupport<FakeIpFilterMode>,
    /// Whether a `nameserver-policy` value may list several servers.
    pub nameserver_policy_list: bool,
    /// Whether a `hosts` value may list several addresses or an alias.
    pub hosts_list: bool,
}
```

`DnsKey` 是 §3.1 类型化键的枚举（派生 `VariantArray`），按（内核, 键）写穷尽 `match`，不写通配。

### 6.1 核对结果（2026-11-14，按 `manifest/version.json` 下发的版本）

| 键 / 取值                                                                                                                                                        | mihomo v1.19.34                 | clash-rs v0.10.11                                 | meow-rs v0.29.0                                                       |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------- | ------------------------------------------------- | --------------------------------------------------------------------- |
| `enable`、`listen`、`nameserver`、`fallback`、`fallback-filter`、`default-nameserver`、`proxy-server-nameserver`、`fake-ip-range`、`fake-ip-filter`、`use-hosts` | 读                              | 读                                                | 读                                                                    |
| `respect-rules`                                                                                                                                                  | 读                              | 读                                                | 不读                                                                  |
| `ipv6`（`dns` 内）                                                                                                                                               | 读                              | 读                                                | 不读（只有顶层 `ipv6`）                                               |
| `use-system-hosts`、`direct-nameserver`、`direct-nameserver-follow-policy`                                                                                       | 读                              | 不读                                              | 读                                                                    |
| `fake-ip-filter-mode`                                                                                                                                            | `blacklist`/`whitelist`/`rule`  | 不读                                              | `blacklist`/`whitelist`（未知值告警后按 `blacklist`）                 |
| `prefer-h3`、`fake-ip-range6`                                                                                                                                    | 读                              | 不读                                              | 不读                                                                  |
| `enhanced-mode`                                                                                                                                                  | `normal`/`fake-ip`/`redir-host` | `normal`/`fake-ip`/`redir-host`                   | `fake-ip`/`redir-host`；`normal` 告警后按默认（等同）；严格模式下拒绝 |
| `nameserver-policy` 值为列表                                                                                                                                     | 是                              | 否（`HashMap<String, String>`，列表导致解析失败） | 是                                                                    |
| `hosts` 值为列表                                                                                                                                                 | 是                              | 否（`HashMap<String, String>`，且只接受 IP）      | 是                                                                    |
| `fallback-filter.geosite`                                                                                                                                        | 读                              | 不读                                              | 未核对，按不读处理                                                    |
| 未知键                                                                                                                                                           | 忽略                            | 忽略（`--strict-config` 时拒绝）                  | 告警后忽略                                                            |
| 控制器 `/dns/query`、`/cache/dns/flush`、`/cache/fakeip/flush`                                                                                                   | 有                              | 有                                                | 有                                                                    |

来源：mihomo `config/config.go` `RawDNS`、`constant/dns.go`；clash-rs `clash-lib/src/config/def.rs` `DNS`、`clash-lib/src/app/dns/config.rs`、`clash-lib/src/app/api/handlers/{dns,cache}.rs`（v0.10.11 与 master 无差异）；meow-rs `crates/meow-config/src/{raw.rs,dns_parser.rs}`、`crates/meow-api/src/routes.rs`。

- **Clash Premium：** 已从 main 移除（#6343），能力表不含它。
- **sing-box：** `Ignored`，前端隐藏整个 DNS 页。
- **上游地址的协议差异不进能力表：** clash-rs 不认 `system`、`quic`；meow-rs 不认 `quic`。地址写法由内核 `-t` 判断（§3.1）。

### 6.2 不支持时的处理

| 情形                                              | 处理                                                                                                                          |
| ------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- |
| Nyanpasu 设置里的键当前内核不读                   | 不写入；值非默认时记 `Warn`                                                                                                   |
| `enhanced-mode: normal` 在 meow-rs                | 不写 `enhanced-mode`（meow-rs 缺省即普通模式）                                                                                |
| `fake-ip-filter-mode: rule` 在不支持的内核        | 不写 `fake-ip-filter-mode` 与 `fake-ip-filter`，记 `Warn`：`rule` 模式下过滤项是规则语法，降为 `blacklist` 会把规则串当作域名 |
| `nameserver-policy` / `hosts` 的列表值在 clash-rs | 取第一项，记 `Warn`                                                                                                           |
| `hosts` 值为别名域名在 clash-rs                   | 跳过该条，记 `Warn`                                                                                                           |

订阅或转换写入的 `dns` 不另做最终配置校验：按 §6.1，下发的内核对这两个枚举键的未知取值只告警或忽略，不会拒绝配置，`core-specific-config.md` §4.1 的“拒绝构建”针对的是内核确定会失败的组合。clash-rs 读不了的列表值由内核 `-t` 拦下。

## 7. 前端

- **路由：** `settings/clash/dns/index.tsx`，在 `settings/clash` 导航中位于“网络”之后；模块放在 `settings/clash/dns/_modules/`。
- **数据：** 四个字段属于控制面（`ClashSystemPatch`，`system:write`），页面经 `useClashSystemSettings` 读完整配置并经 `patch_clash_system_config` 写入，不新增命令；安全视图 `ClashSettings` 不含 `dns`（DoH/DoT 地址可能带私有解析器令牌），同步时四个字段都留在本机。能力表经现有 `get_config_features` 查询。
- **结构：**
  1. 接管：`dns_control`、`hosts_control` 两个开关与替换说明（§4.3）。
  2. 基本：`enable`、`listen`、`ipv6`、`enhanced-mode`、`respect-rules`、`use-hosts`、`use-system-hosts`、`prefer-h3`。
  3. 上游：`default-nameserver`、`nameserver`、`proxy-server-nameserver`、`direct-nameserver`（+ follow-policy）、`fallback`；每项为可增删排序的列表。
  4. Fake-IP（仅 `fake-ip` 模式显示）：范围、过滤模式、过滤列表。
  5. Fallback 过滤（有 `fallback` 时显示）。
  6. 解析策略 `nameserver-policy`：键值行。
  7. Hosts：键值行，值支持多个 IP 或别名。
- **YAML 模式：** 保存栏的“以 YAML 编辑”打开对话框，编辑 `{ dns, hosts }`，`extra` 键并入 `dns`；“应用”替换草稿，保存仍走页面。解析失败或类型化键的形状不对时留在对话框并给出键名。对话框用等宽文本框而非 Monaco：Monaco 只在独立编辑窗口加载，不进主窗口的包。
- **保存：** 整页一个保存按钮，提交一次 patch；`MutationOutcome` 带回构建或 `-t` 的失败时，在页内显示内核给出的错误，草稿保留。
- **未接管时：** 表单仍可编辑并保存（只写存储，不影响运行），页面顶部说明“当前使用订阅的 DNS”。
- 文案键使用 `settings_clash_dns_*` 前缀；遵循 `docs/development/typescript.md` 的分组与 `data-slot` 规则。

## 8. DNS 工具

| 操作            | 后端                                                      | RPC                                      | 分类                    |
| --------------- | --------------------------------------------------------- | ---------------------------------------- | ----------------------- |
| DNS 查询        | `ClashApi::dns_query(DnsQuery)`（clash-api `api/dns.rs`） | `clash_api_query_dns(name, record_type)` | query（与延迟测试同类） |
| 清 fake-ip 缓存 | `flush_fake_ip_cache`                                     | `clash_api_flush_fake_ip_cache`          | mutation                |
| 清内核 DNS 缓存 | `flush_dns_cache`                                         | `clash_api_flush_dns_cache`              | mutation                |

- 在 `nyanpasu-core/src/client/clash_api.rs` 加方法，RPC 为薄适配（`docs/development/rpc.md`）。
- **三个命令位于 `proxy.dns`，namespace 收窄为 `expose = [http]`：** 查询需要 `proxy:read`，清缓存需要 `proxy:write`，开放给 WebUI，Local allowlist 不变。评审依据：三者只经内核的控制器接口操作内核自身的解析器与缓存，不需要提权，也不由 Nyanpasu 访问调用方给定的 URL；查询的出站请求由内核按当前 DNS 配置发出，与 WebUI 已能调用的延迟测试同类。需要提权的 `flush_system_dns_cache` 保持现状，不在此次开放范围。
- 不新增 Local 目标。
- 界面：DNS 页底部“工具”区，查询框（域名 + 记录类型）显示应答；两个清缓存按钮与现有“清除系统 DNS 缓存”放在一起。
- 内核不支持对应接口时（clash-rs 待核对），由能力表或调用错误决定是否显示。

## 9. 不在范围：系统 DNS 接管

两个参照都有“开 TUN 时把系统 DNS 指向公共/本地地址”的功能，且都是 issue 高发区。我们已有 `DnsController` 收敛框架，但 macOS 实现未验证、Windows/Linux 未实现，`desired()` 的产品规则也未定。另立 spec，前提是先在 macOS 上完成 `dns.rs` 注释里列出的三条验收。

## 10. 任务划分

```text
1. nyanpasu-config：ControlSource、DnsSettings、Hosts、视图方法、默认值
   -> verify：serde 往返（含 extra 透传）、patch 局部修改、切换 control 不丢子配置
2. nyanpasu-migration：clash_config 修订 5 与迁移步骤
   -> verify：修订 4 文件迁移后运行配置与迁移前逐字节相同
3. plan.rs：dns_ownership / hosts_ownership，替换 tun_ownership 中的 dns 条目；ManagedSettings 日志
   -> verify：§4.1 表格四种组合的 golden；TUN 下 enable=false 产生 Warn；订阅含 nameserver-policy 时产生 Warn
4. features.rs：DnsFeatures 与逐内核矩阵
   -> verify：能力表测试覆盖每个内核；不支持值的拒绝/放行/Ignored 三类测试
5. impact.rs：运行时输入经视图读取
   -> verify：Profile 模式下改 dns 为 RuntimeImpact::None
6. 前端 settings/clash/dns：表单、YAML 模式、接管与提示、i18n
   -> verify：vitest 浏览器测试（表单↔YAML 同步、保存 patch 形状、sing-box 下隐藏）
7. clash_api 三个方法、`proxy.dns` 命令、specta 注册（查询为 query，清缓存为 mutation）、工具区
   -> verify：假 clash-api 的 client 测试；Tauri IPC 与 HTTP 两种传输的成功与错误映射
```

实际提交按能独立构建与通过测试的边界合并：配置模型与迁移、运行配置生成、DNS 工具命令、前端页面，各一个提交。

## 11. 已决定（2026-11-14）

1. **全局设置**，不按 Profile（§3）。
2. **接管时整表替换**，订阅的 `nameserver-policy`/`proxy-server-nameserver*` 不保留，只记日志并在界面提示（§4.2、§4.3）。
3. **默认值取 Verge Rev 与 Party 的共识**，分歧处按 §3.2 的规则取舍。
4. **DNS 工具与设置同一轮交付，三个命令开放给 WebUI**（§8）。

## 12. 实施记录（2026-11-14）

- clash-rs、meow-rs、mihomo 的能力已按下发版本源码核对（§6.1）。Meow 是独立的 Rust 内核 meow-rs，不是 mihomo 分支。
- `DnsSettings.extra` 在 TypeScript 中导出为 `{ [key: string]: any }`（`specta_typescript::Any`，与 `RpcValue` 相同的做法）：specta 无法导出递归的 `serde_json::Value`。
- 三个 DNS 工具命令加入 HTTP 暴露清单测试（`nyanpasu-macro/src/command_catalog.rs`）与能力清单。
- 未做“恢复默认”（§3.2）与设置页上的订阅 `dns` 摘要（§4.3）；两者都需要新的查询命令，留待后续。
