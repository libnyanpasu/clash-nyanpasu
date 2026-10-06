# PR-3 T07 — composition root + facade 接线(⚠️ 切换组起点)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** profiles 域接入 composition root(`ProfilesClient` 进 `NyanpasuClient`);facade 暴露全部 profiles 方法 + `rebuild_running_config()`;运行配置生成链路从 legacy `enhance()` 切到 RuntimeBuilder 管线;`RebuildNotifier`/端口解析/UI 事件/连接中断全部接线。**自本卡起进入 §4 BC 中间态**(编译+测试绿即可,真实数据端到端不可运行属预期)。

**Architecture:** 生成编排收口在 facade:`regenerate_runtime()`(快照 → `spawn_blocking` 内 RuntimeBuilder → `RuntimeArtifact→IRuntime` 映射 → 写 `Config::runtime()` draft[TODO B8])+ `rebuild_running_config()`(regenerate → `RunningCoreBridge::apply_config`[TODO B8] → `UiEventSink::refresh_clash` → `on_profile_change`[用户决策:所有 rebuild 统一触发])。`Config::generate()`/`Config::init_config()` **删除**;六个 legacy 调用点经 **`FIXME(actor-migration)` 再生成桥**(oneshot 应答保序)或直接删依赖(run_core)。端口解析 = 会话级 `SessionPortResolver`(`PortStrategy::pick_and_try_port` + 指纹缓存,兼任 `SelfProxyPortSource`)。core 侧效应走 `RunningCoreBridge` 适配器(prod = LegacyCoreBridge 包 CoreManager/ConnectionInterruption,test = mock)——facade 单测不碰任何全局。

**Tech Stack:** ractor client(T04)、`RuntimeBuilder`/`FsProfileContentSource`/`EnhanceScriptRunner`(T06)、`PortStrategy::pick_and_try_port`(nyanpasu-config)、tokio mpsc/oneshot、`mockall`、`tauri::async_runtime`。

## Global Constraints

- 一切 cargo 操作前设 `$env:CARGO_TARGET_DIR='F:\codex-target\clash-nyanpasu-pr3'`。
- 每个 commit `cargo build` + `cargo test` 绿(BC 中间态只要求编译+测试,不要求 app 可运行);clippy 干净。
- **T06A golden 套件必须原样全绿**(切换回归安全网;fixtures 不得改动)。
- 新增 legacy 桥注释块恰好 7 处(见 Task 9 台账判据);除此之外零新增全局可变服务。
- facade 写方法统一模式:`CommitReport.affects_current == true` → 顺序 `rebuild_running_config().await`(design §6.4);后台提交经 channel 去抖。

## 基线事实(2026-07-06 实测)

- **composition root**:`setup.rs:12-43`(migration `Runner::with_paths(paths, false).run_pending()` → `NyanpasuClient::try_new_with_args`(同步,内部 `tauri::async_runtime::block_on`)→ `app.manage`)。`resolve_setup(app: &mut App)`(resolve.rs:130)在 builder 之后运行:Handle init:147 → 随机端口块:154-179(patch `verge_mixed_port` + IClashTemp `mixed-port` + `prepare_external_controller_port`)→ `Config::init_config()`:183(首铸)→ `CoreManager::global().init()`:189(启核)。
- **`Config::generate()` 调用面(6 处)**:`core/clash/core.rs:599`(update_config)、`core.rs:469`(run_core 内)、`core.rs:561`(patch 流,失败 `Config::verge().discard()`)、`feat.rs:279/339/363`(core 切换/tun 流,后接 `run_core`)。`Config::init_config()` 唯一调用 = resolve.rs:183。`update_config` 本体(core.rs:595-624)= generate → `check_config`(:424,生成 Check 文件跑核校验)→ `generate_file(Run)` → `api::put_configs` ×5。
- **IRuntime 三元组**:`IRuntime{config: Option<Mapping>, exists_keys: Vec<String>, postprocessing_output: PostProcessingOutput}`(generate() 写法 core.rs:91-95;仅写 draft 不 apply,`generate_file` 读 `latest()`)。`PostProcessingOutput{scopes: IndexMap<ProfileUid, IndexMap<ProfileUid, Logs>>, global: IndexMap<ProfileUid, Logs>, advice: Logs}`(chain.rs:23-31);`Logs = Vec<(LogSpan, String)>`(utils.rs:38,`LogSpan` 与 `StepLogLevel` 逐 variant 同名——adapter.rs `to_step_logs` 的既有正向映射为准)。
- **artifact 侧**:`RuntimeArtifact{final_config: Arc<ConfigValue>, graph, step_logs: Vec<StepLog>, applied_fields: IndexSet<String>}`;artifact.rs:53 注释「Covers every consumer of the legacy IRuntime triple(spec §9.3)」→ `exists_keys ← applied_fields` 有 spec 背书。`StepLog{key: SnapshotNodeKey, entries: Vec<StepLogEntry{level, message}>}`;`SnapshotNodeKey` 全变体(snapshot.rs:186-216):`FileRoot/CompositionRoot/ExtendProxies/ScopedTransform{host_profile_id, role, step_index}/GlobalTransform{selected, step_index}/Builtin{step: GuardOverrides|WhitelistFieldFilter|Finalizing}/BareRoot/BuiltinTransform{selected, step_index}`。
- **端口模型**:`ClashConfig{overrides, enable_tun_mode, web_ui_list, enable_clash_fields, external_controller: ExternalControllerStrategy{host: IpAddr, port: PortStrategy}, mixed_port: PortStrategy, socks_port/http_port: Option<PortStrategy>, break_connection, tun_stack}`(clash/config/mod.rs:27-59);`PortStrategy{kind: Fixed|Random|AllowFallback(默认), start_port}` 自带 `pick_and_try_port() -> Result<PickedPort, PickPortError>`(port.rs:105-147,含探测 IO)。**探测型解析只能在核未占端口时做**——启动时机(resolve_setup 前核未启)与指纹缓存(避免与自家核抢端口)是正确性关键。
- **UI 事件**:`UiEventSink`(event_sink.rs:11-35)已有 `refresh_clash/refresh_profiles`(默认经 `state_changed`,`TauriUiEventSink` 发 `nyanpasu://mutation`,与 legacy `Handle::refresh_clash`(handle.rs:29,52-56)同 URI 同 payload)——**注入适配器即可,不占 TODO 台账**。`NoopUiEventSink` 测试替身已备。
- **连接中断**:`ConnectionInterruptionService::on_profile_change()`(connection_interruption.rs:39-49,静态方法,内读 `Config::verge().break_when_profile_change`,默认 false);legacy 仅 `patch_profiles_config_inner` 成功路径调用。用户决策(2026-07-06):统一挂所有 rebuild。
- **ProfilesClient**:`pub(crate) async fn new(profiles_path: Utf8PathBuf, fs, fetcher, notifier)`(client/profiles.rs:31-77,内部建 PersistentStateManager + validate + spawn actor);`ProfileFileService::new(paths: PathResolver, self_proxy_port: Arc<dyn SelfProxyPortSource>)` 一实例双 port(profile_file.rs:126/202);`normalize_yaml_document(&str) -> anyhow::Result<String>`(profile_file.rs:120,pub)。`PathResolver::app_profiles_dir()`/`profiles_path()`(path.rs:94-101)。
- **错误管道**:`ClientError`(client/error.rs,thiserror,有 Anyhow/Custom)→ `IpcError`(ipc.rs:32-67 有 `From<ClientError>` 穷举 match)。`ProfilesError` 定义于 `state/profiles/actor.rs:20-50`。
- **T06A 交付**(前置):golden 套件(`enhance/golden.rs` + fixtures);Mirror spawn_blocking;`ManagedProfilePath` 防御已钉。

## 契约修正(执行后回写 task.md T07 卡,§5.3;发现于 plan 期)

1. **spec 缺口——`Config::generate()` 有 6 个调用点**(卡片只提 config/core.rs:88):legacy core/verge 流(run_core/update_config/core.rs:561/feat.rs×3)属 PR-4/5/6 域却依赖生成。处置:新增 **`client/rebuild.rs` 再生成桥**(`FIXME(actor-migration)` 显式全局 OnceCell + mpsc/oneshot,保序应答;composition root 安装,handler = facade `regenerate_runtime`);`update_config` 拆分为 `apply_config()`(check+file+put,facade 用)+ 原名(桥 regenerate + apply,legacy 调用者不动);`run_core:469` 的 generate 直接删除(所有变更路径均已前置再生成,重启用当前 draft 语义正确);`core.rs:561`/`feat.rs:279/339/363` 改调桥。`Config::generate()`/`Config::init_config()` 删除;`enhance()` 挂 `#[allow(dead_code)]`+FIXME 待 T10 删。
2. **台账判据修正**:「恰好三处 TODO」不成立——新增 TODO/FIXME(actor-migration) 注释块**恰好 7 处**:①facade `regenerate_runtime` 写 runtime draft(B8);②`LegacyCoreBridge::apply_config`(CoreManager,B8);③`LegacyCoreBridge::on_profile_change`(Config::verge 读);④`REGEN_BRIDGE` 定义;⑤`resolve_setup` 端口回写 legacy 镜像;⑥`enhance()` dead_code 保留;⑦`update_config` legacy 再生成路径。
3. **UI 事件走注入 `UiEventSink`**(refresh_clash 事件与 Handle 同 URI 同 payload)——不占台账;`ClientSetupArgs` 增 `ui_sink`。core 侧效应走新 `RunningCoreBridge` 适配器(prod=LegacyCoreBridge 集中 ②③ 两处桥;test=mock)——facade 可单测。
4. **`SessionPortResolver`**:`pick_and_try_port` 复用 + 端口指纹缓存(mixed/socks/http/external 策略元组);解析在 client 构造期 eager 执行一次(核未启,探测安全);快照指纹不变则复用(避免与自家核抢端口),变则重解析(设置变更生效)。**兼任 `SelfProxyPortSource`**(同 Arc 注入 ProfileFileService,同步读缓存)——零 legacy 读。
5. **legacy 镜像端口回写**(⑤):resolve.rs:154-179 的随机端口块替换为「读 client 解析结果 → patch `verge_mixed_port`+IClashTemp `mixed-port`+`external-controller`」,`prepare_external_controller_port` 调用删除(避免双头解析);**已知风险记 T11**:typed `overrides.secret`(uuid)与 legacy IClashTemp secret 可能不一致 → api 客户端 401,T11 e2e 必查(字段私有,本卡不回写 secret)。
6. **`runtime_from_artifact` 映射规则**:config←`final_config`(serde_yaml Value→Mapping);exists_keys←`applied_fields`(顺序保留);postprocessing_output:`ScopedTransform`→`scopes[host][host.transforms[step_index]]`、`GlobalTransform`→`global[global_transforms[step_index]]`、`BuiltinTransform`→`global[builtin 名]`(与 legacy builtin 以 uid 入 global chain 对齐)、其余(Guard/Whitelist/Finalizing/根节点)→`advice`;空 entries 跳过。
7. 文件三方法直用 `Arc<dyn ProfileFsPort>`(inner 持同一 ProfileFileService);`read_profile_file` 的 Config::File 分支复用 `normalize_yaml_document`(profile_file.rs:120,T05 预备项归置于此);save/read 为同步小 IO 直调(与 legacy ipc 等同,不 spawn_blocking——记录为有意取舍)。
8. `add_profile` 返回 `Result<ProfileId>`(承接 T04 `CommitReport.created` 增补);写方法失败于 rebuild 时**状态已提交**,错误向上传播由 T08 映射(degraded 语义)。

---

### Task 1: `CommitReport.created`(T04 波及)

**Files:**

- Modify: `backend/tauri/src/state/profiles/actor.rs`(CommitReport 定义 :52-59;Add handler :473 起;`run_write` 构造点)
- Modify: `backend/tauri/src/client/profiles.rs`(既有 add 测试增断言;其余测试补字段)

**Interfaces:**

- Produces: `CommitReport { snapshot, affects_current, warnings, created: Option<ProfileId> }`——仅 Add 置 `Some(服务端生成 uid)`,其余消息 `None`。facade(Task 7)与 T08 `import_profile` 消费。

- [ ] **Step 1: 写失败断言**——在 client/profiles.rs 既有 add 成功测试中追加:

```rust
        let created = report
            .created
            .clone()
            .expect("add must report the server-generated uid");
        assert!(report.snapshot.items.contains_key(&created));
```

- [ ] **Step 2: 确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu state::profiles 2>&1 | Select-String "created"`
Expected: 编译错(`CommitReport` 无 `created` 字段)。

- [ ] **Step 3: 实现**——actor.rs:

```rust
#[derive(Debug, Clone)]
pub struct CommitReport {
    pub snapshot: Arc<Profiles>,
    /// Dependency-closure judgement per the T04 affects_current rule table.
    pub affects_current: bool,
    /// Post-commit side-effect failures are degraded, not rolled back.
    pub warnings: Vec<String>,
    /// Server-generated uid (D13); set only by Add, consumed by import
    /// auto-activation (design §9).
    pub created: Option<ProfileId>,
}
```

`run_write` 构造 `CommitReport` 处补 `created: None`;Add 分支在 `run_write` 结果上回填:

```rust
                let result = result.map(|mut report| {
                    report.created = Some(uid.clone());
                    report
                });
```

(`uid` 为 :476 `Self::generate_uid(...)` 的产物;若 Add 分支结构是先算 uid 再 `run_write`,在 reply 前映射即可。)其余测试里字面构造的 `CommitReport` 补 `created: None`。

- [ ] **Step 4: 全量绿 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 全绿。

```powershell
git add backend/tauri/src/state/profiles/actor.rs backend/tauri/src/client/profiles.rs
git commit -m "feat(tauri): report created uid in profiles commit report"
```

---

### Task 2: `ClientError::Profiles` + IpcError 映射臂

**Files:**

- Modify: `backend/tauri/src/client/error.rs`
- Modify: `backend/tauri/src/ipc.rs:56-67`(`From<ClientError> for IpcError` 穷举 match 补臂)

**Interfaces:**

- Produces: `ClientError::Profiles(ProfilesError)`(`#[from]`,facade `?` 直传);`IpcError::Profiles(ProfilesError)`(T08 命令错误面)。

- [ ] **Step 1: 实现**——error.rs 追加 variant:

```rust
    #[error(transparent)]
    Profiles(#[from] crate::state::profiles::actor::ProfilesError),
```

(若 `ProfilesError` 未派生 `thiserror::Error`/Display,改用 `#[error("{0:?}")]` 并去 transparent;以 actor.rs:20 现场 derive 为准。)ipc.rs:

```rust
    #[error(transparent)]
    Profiles(#[from] crate::state::profiles::actor::ProfilesError),
```

加入 `IpcError`,并在 `From<ClientError>` match 补 `ClientError::Profiles(err) => IpcError::Profiles(err),`。

- [ ] **Step 2: 验证 + Commit**

Run: `cargo build --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 编译绿(穷举 match 强制补臂,漏则编译错)。

```powershell
git add backend/tauri/src/client/error.rs backend/tauri/src/ipc.rs
git commit -m "feat(tauri): route profiles domain errors through client error"
```

---

### Task 3: `SessionPortResolver`(兼任 SelfProxyPortSource)

**Files:**

- Create: `backend/tauri/src/client/ports.rs`
- Modify: `backend/tauri/src/client/mod.rs`(`mod ports;` + `pub use ports::SessionPortResolver;`)

**Interfaces:**

- Consumes: `ClashConfig` 端口字段、`PortStrategy::pick_and_try_port`、`SelfProxyPortSource`(service/profile_file.rs:25-30)。
- Produces: `SessionPortResolver::resolve(&ClashConfig) -> anyhow::Result<ResolvedPortBindings>`(指纹缓存);`impl SelfProxyPortSource`(读缓存 mixed_port)。

- [ ] **Step 1: 写失败测试**(ports.rs 内 `#[cfg(test)]`):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::clash::config::{ClashConfig, clash_strategy::port::{PortStrategy, PortStrategyKind}};

    fn fixed(port: u16) -> PortStrategy {
        PortStrategy { kind: PortStrategyKind::Fixed, start_port: port }
    }

    #[test]
    fn resolves_fixed_strategies_and_formats_external_controller() {
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = fixed(48231);
        clash.socks_port = Some(fixed(48232));
        clash.http_port = None;
        clash.external_controller.port = fixed(48233);
        let ports = resolver.resolve(&clash).unwrap();
        assert_eq!(ports.mixed_port, 48231);
        assert_eq!(ports.socks_port, Some(48232));
        assert_eq!(ports.port, None);
        assert_eq!(ports.external_controller.as_deref(), Some("127.0.0.1:48233"));
        assert_eq!(resolver.mixed_port(), Some(48231)); // SelfProxyPortSource 面
    }

    #[test]
    fn random_pick_is_sticky_until_fingerprint_changes() {
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = PortStrategy { kind: PortStrategyKind::Random, start_port: 0 };
        let first = resolver.resolve(&clash).unwrap();
        let second = resolver.resolve(&clash).unwrap();
        assert_eq!(first, second, "same fingerprint must reuse the session pick");
        clash.socks_port = Some(fixed(48234)); // 指纹变化 → 重解析
        let third = resolver.resolve(&clash).unwrap();
        assert_eq!(third.socks_port, Some(48234));
    }

    #[test]
    fn allow_fallback_moves_off_an_occupied_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let taken = listener.local_addr().unwrap().port();
        let resolver = SessionPortResolver::default();
        let mut clash = ClashConfig::default();
        clash.mixed_port = PortStrategy::new_allow_fallback(taken);
        let ports = resolver.resolve(&clash).unwrap();
        assert_ne!(ports.mixed_port, taken);
    }
}
```

(导入路径以 `clash_strategy` 模块实际再导出为准:`nyanpasu_config::clash::config::clash_strategy::port::{...}` 或经 `clash_strategy::*` glob——bridge/clash.rs:58 的既有 import 是权威样板。)

- [ ] **Step 2: 确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu client::ports`
Expected: 编译错(类型未定义)。

- [ ] **Step 3: 实现**(ports.rs 完整文件):

```rust
//! Session-scoped port resolution over typed `PortStrategy` values (PR-3 T07).
//! Probing (`pick_and_try_port`) is only safe while our own core is not
//! holding the ports, so picks are cached per port-config fingerprint: the
//! running session reuses its picks unless the user changes port settings.
//! Doubles as the fetcher's `SelfProxyPortSource` (sync read of the cache).

use std::sync::Mutex;

use anyhow::Context as _;
use nyanpasu_config::{
    clash::config::{
        ClashConfig,
        clash_strategy::port::{ExternalControllerStrategy, PortStrategy},
    },
    runtime::executor::ResolvedPortBindings,
};

use crate::service::profile_file::SelfProxyPortSource;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PortsFingerprint {
    mixed: PortStrategy,
    socks: Option<PortStrategy>,
    http: Option<PortStrategy>,
    external: ExternalControllerStrategy,
}

impl PortsFingerprint {
    fn of(clash: &ClashConfig) -> Self {
        Self {
            mixed: clash.mixed_port.clone(),
            socks: clash.socks_port.clone(),
            http: clash.http_port.clone(),
            external: clash.external_controller.clone(),
        }
    }
}

#[derive(Default)]
pub struct SessionPortResolver {
    cached: Mutex<Option<(PortsFingerprint, ResolvedPortBindings)>>,
}

impl SessionPortResolver {
    pub fn resolve(&self, clash: &ClashConfig) -> anyhow::Result<ResolvedPortBindings> {
        let fingerprint = PortsFingerprint::of(clash);
        let mut cached = self
            .cached
            .lock()
            .expect("port resolver cache should not poison");
        if let Some((previous, ports)) = cached.as_ref() {
            if *previous == fingerprint {
                return Ok(ports.clone());
            }
        }

        let mixed_port = *clash
            .mixed_port
            .pick_and_try_port()
            .context("failed to resolve mixed port")?;
        let port = clash
            .http_port
            .as_ref()
            .map(|strategy| strategy.pick_and_try_port())
            .transpose()
            .context("failed to resolve http port")?
            .map(|picked| *picked);
        let socks_port = clash
            .socks_port
            .as_ref()
            .map(|strategy| strategy.pick_and_try_port())
            .transpose()
            .context("failed to resolve socks port")?
            .map(|picked| *picked);
        let external = clash
            .external_controller
            .port
            .pick_and_try_port()
            .context("failed to resolve external controller port")?;
        let external_controller = Some(format!(
            "{}:{}",
            clash.external_controller.host, *external
        ));

        let ports = ResolvedPortBindings {
            mixed_port,
            port,
            socks_port,
            external_controller,
        };
        *cached = Some((fingerprint, ports.clone()));
        Ok(ports)
    }

    pub fn cached_ports(&self) -> Option<ResolvedPortBindings> {
        self.cached
            .lock()
            .expect("port resolver cache should not poison")
            .as_ref()
            .map(|(_, ports)| ports.clone())
    }
}

impl SelfProxyPortSource for SessionPortResolver {
    fn mixed_port(&self) -> Option<u16> {
        self.cached_ports().map(|ports| ports.mixed_port)
    }
}
```

(`ExternalControllerStrategy` 若未派生 PartialEq,则指纹存 `(IpAddr, PortStrategy)` 二元组代替。测试里 `resolver.mixed_port()` 经 trait 调用需 `use crate::service::profile_file::SelfProxyPortSource;`。)

- [ ] **Step 4: 验证 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu client::ports`
Expected: 3 passed。

```powershell
git add backend/tauri/src/client/ports.rs backend/tauri/src/client/mod.rs
git commit -m "feat(tauri): add session port resolver over typed strategies"
```

---

### Task 4: `runtime_from_artifact`(artifact → IRuntime 纯映射)

**Files:**

- Create: `backend/tauri/src/enhance/artifact_bridge.rs`
- Modify: `backend/tauri/src/enhance/mod.rs`(`mod artifact_bridge; pub use artifact_bridge::runtime_from_artifact;`)

**Interfaces:**

- Produces: `pub fn runtime_from_artifact(artifact: &RuntimeArtifact, profiles: &Profiles, core: ClashCore, builtin_enabled: bool) -> anyhow::Result<IRuntime>`;内部 `map_postprocessing(step_logs, profiles, &builtin_names) -> PostProcessingOutput`(直接可测)。

- [ ] **Step 1: 写失败测试**(artifact_bridge.rs 内;`StepLog`/`StepLogEntry` 均可直接构造):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::{
        profile::{ProfileId, Profiles},
        runtime::{
            executor::{StepLog, StepLogEntry, StepLogLevel},
            snapshot::SnapshotNodeKey,
        },
    };

    fn log(key: SnapshotNodeKey, message: &str) -> StepLog {
        StepLog {
            key,
            entries: vec![StepLogEntry::new(StepLogLevel::Info, message)],
        }
    }

    #[test]
    fn maps_scoped_global_and_builtin_logs_to_legacy_layout() {
        // host 拥有一个 scoped transform(uid=scr1),全局链一个(uid=gfix)
        let mut profiles = Profiles::default();
        // 复用 T06A golden.rs 的构造模式:file_config("host", "h.yaml", &["scr1"]) + overlay("scr1"/"gfix")
        profiles.append_item(crate::enhance::golden_support::file_config(
            "host", "h.yaml", &["scr1"],
        ));
        profiles.append_item(crate::enhance::golden_support::overlay("scr1", "s.yaml"));
        profiles.append_item(crate::enhance::golden_support::overlay("gfix", "g.yaml"));
        profiles.global_transforms = vec![ProfileId("gfix".into())];

        let host = ProfileId("host".into());
        let logs = vec![
            log(
                SnapshotNodeKey::ScopedTransform {
                    host_profile_id: host.clone(),
                    role: Default::default(),
                    step_index: 0,
                },
                "scoped ran",
            ),
            log(
                SnapshotNodeKey::GlobalTransform {
                    selected_profile_id: Some(host.clone()),
                    step_index: 0,
                },
                "global ran",
            ),
            log(
                SnapshotNodeKey::BuiltinTransform {
                    selected_profile_id: Some(host.clone()),
                    step_index: 0,
                },
                "builtin ran",
            ),
        ];
        let out = map_postprocessing(&logs, &profiles, &["verge_hy_alpn".to_string()]);
        assert_eq!(out.scopes["host"]["scr1"][0].1, "scoped ran");
        assert_eq!(out.global["gfix"][0].1, "global ran");
        assert_eq!(out.global["verge_hy_alpn"][0].1, "builtin ran");
        assert!(out.advice.is_empty());
    }
}
```

(若 `ConfigExecutionRole` 无 `Default`,用其首个 variant 字面量;`golden_support` 指 Task 后述抽出的构造 helper——若 T06A 的 helpers 仍在 `golden.rs` 私有,把 `file_config`/`overlay`/`managed`/`metadata` 四个 fn 移到 `#[cfg(test)] pub(crate) mod golden_support`(enhance/golden_support.rs)并让 golden.rs 复用,一次性小重构。)

- [ ] **Step 2: 确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu artifact_bridge`
Expected: 编译错。

- [ ] **Step 3: 实现**(artifact_bridge.rs 主体):

```rust
//! Pure mapping from the executor's RuntimeArtifact to the legacy IRuntime
//! triple (design §8; executor spec §9.3 endorses applied_fields ↔ exists_keys).
//! Postprocessing layout mirrors legacy PostProcessingOutput: scoped logs keyed
//! by (host profile, transform uid), global/builtin logs keyed by uid/name.

use anyhow::Context as _;
use nyanpasu_config::{
    application::ClashCore,
    profile::{ConfigDefinition, ProfileDefinition, ProfileId, Profiles},
    runtime::{
        executor::{RuntimeArtifact, StepLog, StepLogLevel},
        snapshot::SnapshotNodeKey,
    },
};
use serde_yaml::Mapping;

use crate::{
    config::IRuntime,
    enhance::{Logs, PostProcessingOutput, builtin_transforms_for},
};

fn span(level: StepLogLevel) -> crate::enhance::utils::LogSpan {
    use crate::enhance::utils::LogSpan;
    match level {
        StepLogLevel::Log => LogSpan::Log,
        StepLogLevel::Info => LogSpan::Info,
        StepLogLevel::Warn => LogSpan::Warn,
        StepLogLevel::Error => LogSpan::Error,
    }
}
```

(variant 表与 `enhance/script/adapter.rs` 的 `to_step_logs` 正向映射逐臂对齐——若 `LogSpan` 有额外 variant 以 adapter.rs 为准补齐;`LogSpan` 的可见性若为 `pub(crate)`,在 utils.rs 提为 `pub` 并从 `enhance` 顶层 `pub use`。)

```rust
fn transform_uid_of(profiles: &Profiles, host: &ProfileId, step_index: u32) -> Option<String> {
    let item = profiles.items.get(host)?;
    let list = match &item.definition {
        ProfileDefinition::Config {
            config: ConfigDefinition::File(file),
        } => &file.transforms,
        ProfileDefinition::Config {
            config: ConfigDefinition::Composition(composition),
        } => &composition.transforms,
        _ => return None,
    };
    list.get(step_index as usize).map(|uid| uid.0.clone())
}

pub(crate) fn map_postprocessing(
    step_logs: &[StepLog],
    profiles: &Profiles,
    builtin_names: &[String],
) -> PostProcessingOutput {
    let mut out = PostProcessingOutput::default();
    for log in step_logs {
        let logs: Logs = log
            .entries
            .iter()
            .map(|entry| (span(entry.level), entry.message.clone()))
            .collect();
        if logs.is_empty() {
            continue;
        }
        match &log.key {
            SnapshotNodeKey::ScopedTransform {
                host_profile_id,
                step_index,
                ..
            } => {
                let transform = transform_uid_of(profiles, host_profile_id, *step_index)
                    .unwrap_or_else(|| format!("step-{step_index}"));
                out.scopes
                    .entry(host_profile_id.0.clone())
                    .or_default()
                    .insert(transform, logs);
            }
            SnapshotNodeKey::GlobalTransform { step_index, .. } => {
                let uid = profiles
                    .global_transforms
                    .get(*step_index as usize)
                    .map(|uid| uid.0.clone())
                    .unwrap_or_else(|| format!("global-{step_index}"));
                out.global.insert(uid, logs);
            }
            SnapshotNodeKey::BuiltinTransform { step_index, .. } => {
                let name = builtin_names
                    .get(*step_index as usize)
                    .cloned()
                    .unwrap_or_else(|| format!("builtin-{step_index}"));
                out.global.insert(name, logs);
            }
            _ => out.advice.extend(logs),
        }
    }
    out
}

pub fn runtime_from_artifact(
    artifact: &RuntimeArtifact,
    profiles: &Profiles,
    core: ClashCore,
    builtin_enabled: bool,
) -> anyhow::Result<IRuntime> {
    let value = serde_yaml::to_value(&*artifact.final_config)
        .context("failed to serialize final config")?;
    let config: Mapping = value
        .as_mapping()
        .cloned()
        .context("final config is not a mapping")?;
    let exists_keys: Vec<String> = artifact.applied_fields.iter().cloned().collect();
    let builtin_names: Vec<String> = if builtin_enabled {
        builtin_transforms_for(core)
            .into_iter()
            .map(|builtin| builtin.name)
            .collect()
    } else {
        Vec::new()
    };
    Ok(IRuntime {
        config: Some(config),
        exists_keys,
        postprocessing_output: map_postprocessing(&artifact.step_logs, profiles, &builtin_names),
    })
}
```

(`PostProcessingOutput.scopes` 的键类型 `ProfileUid` 若是 String 别名,`host_profile_id.0.clone()` 直接吻合;若是 newtype 则包一层。`IRuntime` 若无字面量构造用 `IRuntime::new()` + 字段赋值。)

- [ ] **Step 4: 验证 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu artifact_bridge`
Expected: PASS;`cargo test -p clash-nyanpasu` 全绿。

```powershell
git add backend/tauri/src/enhance/
git commit -m "feat(tauri): map runtime artifact to legacy runtime triple"
```

---

### Task 5: `RunningCoreBridge` 适配器 + `CoreManager::apply_config` 拆分

**Files:**

- Create: `backend/tauri/src/client/core_bridge.rs`
- Modify: `backend/tauri/src/client/mod.rs`(`mod core_bridge; pub use core_bridge::{LegacyCoreBridge, RunningCoreBridge};`)
- Modify: `backend/tauri/src/core/clash/core.rs:595-624`(update_config 拆出 apply_config;update_config 本体改造在 Task 8)

**Interfaces:**

- Produces: `RunningCoreBridge { async fn apply_config() -> anyhow::Result<()>; async fn on_profile_change(); }`(automock);`LegacyCoreBridge`(prod,台账块②③);`CoreManager::apply_config()`(= 旧 update_config 去掉 generate 的部分)。

- [ ] **Step 1: CoreManager 拆分**——core.rs:595-624 改为:

```rust
    pub async fn update_config(&self) -> Result<()> {
        log::debug!(target: "app", "try to update clash config");
        // 更新配置
        Config::generate().await?;
        self.apply_config().await
    }

    /// Apply the CURRENT runtime draft to the running core: check, write the
    /// runtime file, and push it over the api. Regeneration is the caller's
    /// responsibility (facade `regenerate_runtime` or the legacy bridge).
    pub async fn apply_config(&self) -> Result<()> {
        // 检查配置是否正常
        self.check_config().await?;

        // 更新运行时配置
        let path = Config::generate_file(ConfigType::Run)?;
        let path = dirs::path_to_str(&path)?;

        // 发送请求 发送5次
        for i in 0..5 {
            match api::put_configs(path).await {
                Ok(_) => break,
                Err(err) => {
                    if i < 4 {
                        log::info!(target: "app", "{err:?}");
                    } else {
                        bail!(err);
                    }
                }
            }
            sleep(Duration::from_millis(250)).await;
        }

        Ok(())
    }
```

(本 Task 保持 update_config 语义不变——`Config::generate()` 仍在;Task 8 才替换成桥。)

- [ ] **Step 2: core_bridge.rs 完整文件**

```rust
//! Boundary adapter for "apply the regenerated runtime to the running core"
//! (PR-3 T07). The facade depends on this trait so it stays testable; the
//! production impl concentrates the legacy-global touches behind two
//! documented bridges.

use async_trait::async_trait;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RunningCoreBridge: Send + Sync + 'static {
    async fn apply_config(&self) -> anyhow::Result<()>;
    async fn on_profile_change(&self);
}

pub struct LegacyCoreBridge;

#[async_trait]
impl RunningCoreBridge for LegacyCoreBridge {
    async fn apply_config(&self) -> anyhow::Result<()> {
        // TODO(actor-migration): temporary bridge to CoreManager::global().
        // Reason: core lifecycle is PR-5 (CoreActor).
        // Remove when: PR-5 lands CoreActor and the facade owns core apply.
        crate::core::CoreManager::global().apply_config().await
    }

    async fn on_profile_change(&self) {
        // TODO(actor-migration): connection interruption still reads Config::verge()
        // inside the service. Reason: break_when_* options + clash api client are
        // PR-4/PR-6 scope. Remove when: interruption reads typed
        // ClashConfig.break_connection via an injected client.
        let _ = crate::core::connection_interruption::ConnectionInterruptionService::on_profile_change().await;
    }
}
```

- [ ] **Step 3: 验证 + Commit**

Run: `cargo build --manifest-path ./backend/Cargo.toml -p clash-nyanpasu && cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 绿(行为未变,纯拆分 + 新增未接线适配器)。

```powershell
git add backend/tauri/src/client/core_bridge.rs backend/tauri/src/client/mod.rs backend/tauri/src/core/clash/core.rs
git commit -m "feat(tauri): add running-core bridge adapter and apply_config split"
```

---

### Task 6: rebuild 通道(notifier 实现 + 去抖监听)+ 再生成桥

**Files:**

- Create: `backend/tauri/src/client/rebuild.rs`
- Modify: `backend/tauri/src/client/mod.rs`(`pub mod rebuild;`)

**Interfaces:**

- Produces: `ChannelRebuildNotifier`(impl `RebuildNotifier`);`spawn_rebuild_listener(rebuild_fn, rx)`(泛型,可测);`install_regen_bridge(handler)` + `pub async fn regenerate()`(legacy 调用面,Task 8 消费,台账块④)。

- [ ] **Step 1: 写失败测试**(去抖合并语义):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn listener_coalesces_bursts_into_one_rebuild() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        spawn_listener_with(rx, move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        for _ in 0..5 {
            tx.send(()).unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1, "burst must coalesce");
        tx.send(()).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(900)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}
```

- [ ] **Step 2: 确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu rebuild::tests`
Expected: 编译错。

- [ ] **Step 3: 实现**(rebuild.rs 完整文件):

```rust
//! Rebuild plumbing (PR-3 T07): the actor-side fire-and-forget notifier, the
//! receiver-side debounced listener (design §6.4: debouncing is the receiver's
//! concern), and the legacy regeneration bridge.

use std::future::Future;

use once_cell::sync::OnceCell;
use tokio::sync::{mpsc, oneshot};

use crate::state::profiles::ports::RebuildNotifier;

pub struct ChannelRebuildNotifier(mpsc::UnboundedSender<()>);

impl ChannelRebuildNotifier {
    pub fn new(sender: mpsc::UnboundedSender<()>) -> Self {
        Self(sender)
    }
}

impl RebuildNotifier for ChannelRebuildNotifier {
    fn request_rebuild(&self) {
        let _ = self.0.send(());
    }
}

const COALESCE_WINDOW: std::time::Duration = std::time::Duration::from_millis(500);

pub(super) fn spawn_listener_with<F, Fut>(mut rx: mpsc::UnboundedReceiver<()>, rebuild: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send,
{
    tauri::async_runtime::spawn(async move {
        while rx.recv().await.is_some() {
            tokio::time::sleep(COALESCE_WINDOW).await;
            while rx.try_recv().is_ok() {}
            if let Err(error) = rebuild().await {
                tracing::warn!(%error, "background-driven rebuild failed (degraded)");
            }
        }
    });
}

// FIXME(actor-migration): process-level regeneration bridge for legacy core/verge
// flows (CoreManager::update_config / core switch / tun patch paths) that cannot
// receive the client by injection yet. New code must use
// NyanpasuClient::rebuild_running_config() / regenerate_runtime().
// Remove after PR-4/PR-5 migrate those flows onto injected clients.
type RegenRequest = oneshot::Sender<anyhow::Result<()>>;
static REGEN_BRIDGE: OnceCell<mpsc::UnboundedSender<RegenRequest>> = OnceCell::new();

pub(super) fn install_regen_bridge<F, Fut>(handler: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = anyhow::Result<()>> + Send,
{
    let (tx, mut rx) = mpsc::unbounded_channel::<RegenRequest>();
    if REGEN_BRIDGE.set(tx).is_err() {
        tracing::warn!("regeneration bridge already installed; keeping the first");
        return;
    }
    tauri::async_runtime::spawn(async move {
        while let Some(reply) = rx.recv().await {
            let _ = reply.send(handler().await);
        }
    });
}

/// Sequenced regeneration for legacy callers: awaits the facade rebuild of the
/// runtime draft (no core apply) before returning, preserving the legacy
/// `Config::generate().await?` ordering guarantees.
pub async fn regenerate() -> anyhow::Result<()> {
    let bridge = REGEN_BRIDGE
        .get()
        .ok_or_else(|| anyhow::anyhow!("regeneration bridge not installed"))?;
    let (tx, rx) = oneshot::channel();
    bridge
        .send(tx)
        .map_err(|_| anyhow::anyhow!("regeneration bridge is gone"))?;
    rx.await
        .map_err(|_| anyhow::anyhow!("regeneration handler dropped the reply"))?
}
```

(`REGEN_BRIDGE` 是显式豁免的全局:FIXME 注释块④;不写单测——OnceCell 进程级单次安装,测试间会互相污染,逻辑由启动冒烟 + T11 覆盖。)

- [ ] **Step 4: 验证 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu rebuild::tests`
Expected: 1 passed。

```powershell
git add backend/tauri/src/client/rebuild.rs backend/tauri/src/client/mod.rs
git commit -m "feat(tauri): add rebuild channel and legacy regeneration bridge"
```

---

### Task 7: facade profiles 面(15 方法 + rebuild 编排)

**Files:**

- Modify: `backend/tauri/src/client/mod.rs`(ClientSetupArgs、Inner、try_new_with_args、新方法组、测试)

**Interfaces:**

- Consumes: Task 1–6 全部产物;`ProfilesClient` 全 API;`normalize_yaml_document`。
- Produces(T08 依赖,签名以此为准):卡内 Produces 方法表 + 修正:`add_profile(...) -> Result<ProfileId>`;`pub(crate) async fn regenerate_runtime(&self) -> Result<()>`(桥 handler 用);台账块①在 `regenerate_runtime`。

- [ ] **Step 1: 结构接线**——`ClientSetupArgs`/Inner/构造:

```rust
pub struct ClientSetupArgs {
    pub paths: PathResolver,
    pub bridges: LegacyBridgeSet,
    pub ui_sink: Arc<dyn UiEventSink>,
    pub core: Arc<dyn RunningCoreBridge>,
}

struct NyanpasuClientInner {
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
    profiles: profiles::ProfilesClient,
    fs: Arc<dyn ProfileFsPort>,
    ports: Arc<SessionPortResolver>,
    profiles_dir: PathBuf,
    ui_sink: Arc<dyn UiEventSink>,
    core: Arc<dyn RunningCoreBridge>,
}
```

`try_new_with_args`(block_on 序列内,typed clients 之后):

```rust
        let ClientSetupArgs { paths, bridges, ui_sink, core } = args;
        let profiles_dir = paths.app_profiles_dir();
        let profiles_path = utf8_path(paths.profiles_path())?;
        let (application, session_state, clash_config, profiles, ports, fs, rebuild_rx) =
            tauri::async_runtime::block_on(async move {
                let (application, session_state, clash_config) =
                    new_typed_config_clients(paths.clone(), bridges).await?;

                // Eager session port resolution: the core is not running yet,
                // so probing strategies is race-free (design §19.2 caller duty).
                let ports = Arc::new(SessionPortResolver::default());
                let clash_snapshot = clash_config.get().await?.state;
                ports
                    .resolve(&clash_snapshot)
                    .context("failed to resolve session ports")?;

                let file_service =
                    Arc::new(ProfileFileService::new(paths, ports.clone() as Arc<dyn SelfProxyPortSource>));
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
                let profiles = profiles::ProfilesClient::new(
                    profiles_path,
                    file_service.clone() as Arc<dyn ProfileFsPort>,
                    file_service.clone() as Arc<dyn SubscriptionFetcher>,
                    Arc::new(rebuild::ChannelRebuildNotifier::new(tx)),
                )
                .await?;
                anyhow::Ok((application, session_state, clash_config, profiles, ports,
                    file_service as Arc<dyn ProfileFsPort>, rx))
            })?;
        let client = Self::with_parts(application, session_state, clash_config, profiles,
            fs, ports, profiles_dir, ui_sink, core);
        {
            let listener = client.clone();
            rebuild::spawn_listener_with(rebuild_rx, move || {
                let client = listener.clone();
                async move { client.rebuild_running_config().await.map_err(anyhow::Error::from) }
            });
        }
        {
            let bridge = client.clone();
            rebuild::install_regen_bridge(move || {
                let client = bridge.clone();
                async move { client.regenerate_runtime().await.map_err(anyhow::Error::from) }
            });
        }
        Ok(client)
```

(`with_parts` = 扩参版 `with_typed_clients`,测试 helper 同步扩:`test_client` 传 `Arc::new(NoopUiEventSink)` + `Arc::new(MockRunningCoreBridge)`;既有 4 个测试补两参。imports 相应补 `ProfileFsPort/SubscriptionFetcher/SelfProxyPortSource/ProfileFileService/SessionPortResolver/rebuild`。)

- [ ] **Step 2: 写失败集成测试**(client/mod.rs tests;mock core bridge 记录调用):

```rust
    fn test_profiles_client_args(dir: &TempDir, core: Arc<dyn RunningCoreBridge>) -> ClientSetupArgs {
        ClientSetupArgs {
            paths: PathResolver::with_base_dirs(dir.path().into(), dir.path().join("data")),
            bridges: LegacyBridgeSet {
                verge: Arc::new(NoopVergeBridge),
                window: Arc::new(NoopWindowBridge),
                clash: Arc::new(NoopClashBridge),
            },
            ui_sink: Arc::new(crate::client::event_sink::NoopUiEventSink),
            core,
        }
    }

    #[test]
    fn facade_add_activate_rebuilds_via_core_bridge() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_apply_config().times(1).returning(|| Ok(()));
        core.expect_on_profile_change().times(1).returning(|| ());
        let client = NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core))).unwrap();

        tauri::async_runtime::block_on(async {
            let uid = client
                .add_profile(
                    NewProfileRequest {
                        metadata: ProfileMetadata { name: "t".into(), desc: None },
                        definition: ProfileDefinition::Config {
                            config: ConfigDefinition::File(FileConfig {
                                source: ProfileSource::Local {
                                    binding: LocalBinding::Managed {
                                        materialized: MaterializedFile {
                                            file: ManagedProfilePath::new("t.yaml").unwrap(),
                                            updated_at: None,
                                        },
                                    },
                                },
                                transforms: vec![],
                            }),
                        },
                    },
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add");
            // add 时 current=None,不触发 rebuild;activate 触发一次
            client.activate_profile(Some(uid.clone())).await.expect("activate");
            // rebuild 已写 runtime draft(mixed-port 由 guard 注入)
            let runtime = crate::config::Config::runtime();
            let runtime = runtime.latest();
            let config = runtime.config.as_ref().expect("runtime draft written");
            assert!(config.get("mixed-port").is_some());
            // 文件三方法分支
            let path = client.get_profile_materialized_path(uid.clone()).await.unwrap();
            assert!(path.ends_with(format!("{}.yaml", uid.0)));
            let content = client.read_profile_file(uid.clone()).await.unwrap();
            assert!(content.contains("proxies"));
            client.save_profile_file(uid.clone(), "proxies: []\nmode: direct\n".into()).await.unwrap();
        });
    }
```

(mockall 的 `expect_on_profile_change().returning(|| ())` 若 async trait 生成 future 返回,按 automock 生成签名 `.returning(|| Box::pin(async {}))` 调整——以编译器提示为准。`MaterializedFile.file` 的实际落盘名会被 Add 重写为 `{uid}.{ext}`(actor 语义),故断言用 `ends_with`。)

- [ ] **Step 3: 确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu facade_add_activate`
Expected: 编译错(方法未定义)。

- [ ] **Step 4: 实现方法组**(client/mod.rs `impl NyanpasuClient` 追加):

```rust
    // ---- profiles domain (PR-3 T07) ----

    pub async fn get_profiles(&self) -> Result<Arc<Profiles>> {
        Ok(self.inner.profiles.get().await?)
    }

    async fn after_commit(&self, report: &CommitReport) -> Result<()> {
        if report.affects_current {
            self.rebuild_running_config().await?;
        }
        Ok(())
    }

    pub async fn add_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> Result<ProfileId> {
        let report = self.inner.profiles.add(request, initial_file).await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| ClientError::Custom("add committed without a created uid".into()))?;
        self.after_commit(&report).await?;
        Ok(created)
    }

    pub async fn delete_profile(&self, uid: ProfileId) -> Result<()> {
        let report = self.inner.profiles.delete(uid).await?;
        self.after_commit(&report).await
    }

    pub async fn reorder_profile(&self, active: ProfileId, over: ProfileId) -> Result<()> {
        let report = self.inner.profiles.reorder(ReorderOp::Move { active, over }).await?;
        self.after_commit(&report).await
    }

    pub async fn reorder_profiles_by_list(&self, list: Vec<ProfileId>) -> Result<()> {
        let report = self.inner.profiles.reorder(ReorderOp::ByList(list)).await?;
        self.after_commit(&report).await
    }

    pub async fn refresh_profile(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> Result<()> {
        let report = self.inner.profiles.refresh(uid, patch).await?;
        self.after_commit(&report).await
    }

    pub async fn patch_profile_metadata(&self, uid: ProfileId, patch: ProfileMetadataPatch) -> Result<()> {
        let report = self.inner.profiles.patch_metadata(uid, patch).await?;
        self.after_commit(&report).await
    }

    pub async fn patch_remote_profile_options(
        &self,
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
    ) -> Result<()> {
        let report = self.inner.profiles.patch_remote_options(uid, patch).await?;
        self.after_commit(&report).await
    }

    pub async fn replace_profile_definition(&self, uid: ProfileId, definition: ProfileDefinition) -> Result<()> {
        let report = self.inner.profiles.replace_definition(uid, definition).await?;
        self.after_commit(&report).await
    }

    pub async fn activate_profile(&self, uid: Option<ProfileId>) -> Result<()> {
        let report = self.inner.profiles.set_current(uid).await?;
        self.after_commit(&report).await
    }

    pub async fn set_global_transforms(&self, ids: Vec<ProfileId>) -> Result<()> {
        let report = self.inner.profiles.set_global_transforms(ids).await?;
        self.after_commit(&report).await
    }

    pub async fn get_profile_materialized_path(&self, uid: ProfileId) -> Result<PathBuf> {
        let snapshot = self.inner.profiles.get().await?;
        let item = snapshot
            .items
            .get(&uid)
            .ok_or(ProfilesError::ProfileNotFound(uid))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        Ok(self.inner.profiles_dir.join(source.materialized().file.as_path()))
    }

    pub async fn read_profile_file(&self, uid: ProfileId) -> Result<String> {
        let snapshot = self.inner.profiles.get().await?;
        let item = snapshot
            .items
            .get(&uid)
            .ok_or_else(|| ProfilesError::ProfileNotFound(uid.clone()))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        let raw = self
            .inner
            .fs
            .read(&source.materialized().file)
            .map_err(ClientError::Anyhow)?;
        match &item.definition {
            // Config::File 规范化输出(§9;normalize_yaml_document 复用)
            ProfileDefinition::Config { .. } => {
                crate::service::profile_file::normalize_yaml_document(&raw).map_err(ClientError::Anyhow)
            }
            // Overlay / Script 原文
            ProfileDefinition::Transform { .. } => Ok(raw),
        }
    }

    pub async fn save_profile_file(&self, uid: ProfileId, data: String) -> Result<()> {
        let snapshot = self.inner.profiles.get().await?;
        let item = snapshot
            .items
            .get(&uid)
            .ok_or_else(|| ProfilesError::ProfileNotFound(uid.clone()))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        match source {
            ProfileSource::Local { binding: LocalBinding::Managed { materialized } } => {
                self.inner
                    .fs
                    .write_atomic(&materialized.file, &data)
                    .map_err(ClientError::Anyhow)?;
                Ok(())
            }
            ProfileSource::Remote { .. } => Err(ProfilesError::FileNotWritable {
                reason: "remote profiles are updater-owned".into(),
            }
            .into()),
            ProfileSource::Local { binding: LocalBinding::External { .. } } => {
                Err(ProfilesError::FileNotWritable {
                    reason: "external profiles are edited at their source".into(),
                }
                .into())
            }
        }
    }

    pub async fn rebuild_running_config(&self) -> Result<()> {
        self.regenerate_runtime().await?;
        self.inner.core.apply_config().await.map_err(ClientError::Anyhow)?;
        self.inner.ui_sink.refresh_clash();
        // 用户决策 2026-07-06:所有 rebuild 统一触发(选项默认 false 门控)。
        self.inner.core.on_profile_change().await;
        Ok(())
    }

    pub(crate) async fn regenerate_runtime(&self) -> Result<()> {
        let profiles = self.inner.profiles.get().await?;
        let clash = self.get_clash_config().await?;
        let app = self.get_app_config().await?;
        let resolved_ports = self.inner.ports.resolve(&clash).map_err(ClientError::Anyhow)?;
        let profiles_dir = self.inner.profiles_dir.clone();
        let core = app.core;
        let builtin_enabled = app.enable_builtin_enhanced;
        let runtime = tokio::task::spawn_blocking(move || -> anyhow::Result<crate::config::IRuntime> {
            let content = FsProfileContentSource::new(profiles_dir);
            let scripts = EnhanceScriptRunner::new()?;
            let input = RuntimeBuildInput {
                profiles: profiles.clone(),
                clash,
                app,
                resolved_ports,
            };
            let artifact = RuntimeBuilder::build(&input, &content, &scripts)?;
            runtime_from_artifact(&artifact, &profiles, core, builtin_enabled)
        })
        .await
        .map_err(|error| ClientError::Custom(format!("runtime build task failed: {error}")))?
        .map_err(ClientError::Anyhow)?;
        // TODO(actor-migration): temporary bridge to Config::runtime() draft (B8).
        // Reason: runtime derivation cleanup is PR-4.
        // Remove when: PR-4 lands RuntimeArtifact in SimpleStateManager.
        *crate::config::Config::runtime().draft() = runtime;
        Ok(())
    }
```

(imports:`crate::enhance::{EnhanceScriptRunner, FsProfileContentSource, RuntimeBuildInput, RuntimeBuilder, runtime_from_artifact}`、`crate::state::profiles::{actor::{CommitReport, NewProfileRequest, ProfilesError, ReorderOp}, ports::{ProfileFsPort, SubscriptionFetcher}}`、域类型 `nyanpasu_config::profile::*`。`RuntimeBuildError: !Send`? —— `RuntimeBuilder::build` 错误经 `?` 转 anyhow 于闭包内。)

- [ ] **Step 5: 验证 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu facade_add_activate`
Expected: PASS(core mock 恰好各 1 次)。
Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 全绿(既有 4 个 client 测试补参后同过)。

```powershell
git add backend/tauri/src/client/mod.rs
git commit -m "feat(tauri): expose profiles domain on nyanpasu client facade"
```

---

### Task 8: composition root 与 6 处 legacy generate 调用面切换

**Files:**

- Modify: `backend/tauri/src/setup.rs`(args 补 ui_sink/core)
- Modify: `backend/tauri/src/utils/resolve.rs:150-190`(端口回写 + 首铸换源)
- Modify: `backend/tauri/src/config/core.rs`(删 `generate()`/`init_config()`)
- Modify: `backend/tauri/src/core/clash/core.rs`(update_config 换桥;run_core:469 删 generate;:561 换桥)
- Modify: `backend/tauri/src/feat.rs:279,339,363`(换桥)
- Modify: `backend/tauri/src/enhance/mod.rs`(`enhance()` 挂 allow+FIXME)

- [ ] **Step 1: setup.rs**

```rust
    let client = NyanpasuClient::try_new_with_args(ClientSetupArgs {
        paths,
        bridges: LegacyBridgeSet {
            verge: Arc::new(LegacyVergeBridge::default()),
            window: Arc::new(LegacyWindowBridge),
            clash: Arc::new(LegacyClashBridge),
        },
        ui_sink: Arc::new(crate::client::TauriUiEventSink::new(app.app_handle().clone())),
        core: Arc::new(crate::client::LegacyCoreBridge),
    })
    .context("Failed to setup nyanpasu client")?;
```

(`TauriUiEventSink` 泛型默认 `tauri::Wry`;`setup` 的 `M: Manager<R>` 泛型下用 `TauriUiEventSink::<R>::new(...)`,并把 `UiEventSink` 的 impl 泛型对齐——若阻力大,`setup` 收窄为 `tauri::App`/Wry 专用即可,lib.rs 唯一调用点是 `setup::setup(app)`。)

- [ ] **Step 2: resolve.rs 端口块 + 首铸替换**——:153-183 整段替换为:

```rust
    // FIXME(actor-migration): write the session-resolved ports back into the
    // legacy mirrors (IVerge/IClashTemp) so sysproxy & the clash api client keep
    // observing the real ports during the BC window. The typed side is the
    // single resolver (SessionPortResolver); prepare_external_controller_port
    // double-resolution is removed. Remove after PR-4/PR-6 migrate those readers.
    {
        let client = app.state::<crate::client::NyanpasuClient>();
        if let Some(ports) = client.session_ports() {
            Config::verge().data().patch_config(IVerge {
                verge_mixed_port: Some(ports.mixed_port),
                ..IVerge::default()
            });
            let _ = Config::verge().data().save_file();
            let mut mapping = Mapping::new();
            mapping.insert("mixed-port".into(), ports.mixed_port.into());
            if let Some(external_controller) = ports.external_controller.as_deref() {
                mapping.insert("external-controller".into(), external_controller.into());
            }
            Config::clash().data().patch_config(mapping);
            let _ = Config::clash().data().save_config();
        }

        // 启动首铸:profiles/clash/app 快照 → RuntimeBuilder → runtime draft
        log::trace!("init config");
        log_err!(tauri::async_runtime::block_on(client.regenerate_runtime()));
        if let Err(err) = Config::generate_file(ConfigType::Run) {
            log::error!(target: "app", "{err:?}");
            let runtime_path = dirs::app_config_dir()
                .expect("failed to get app config dir")
                .join(crate::config::RUNTIME_CONFIG);
            // 与旧 init_config 相同的兜底:文件缺失时落默认 clash 配置
            if !runtime_path.exists() {
                log_err!(help::save_yaml(
                    &runtime_path,
                    &Config::clash().latest().0,
                    Some("# Clash Nyanpasu Runtime"),
                ));
            }
        }
    }
```

并在 facade 增一个只读访问器(client/mod.rs):

```rust
    pub fn session_ports(&self) -> Option<ResolvedPortBindings> {
        self.inner.ports.cached_ports()
    }
```

(`regenerate_runtime` 对 resolve.rs 可见性:`pub(crate)`。imports 按现文件补齐;`ConfigType`/`RUNTIME_CONFIG` 从 `crate::config` 导出。)

- [ ] **Step 3: config/core.rs 删除**——`generate()`(:88-98)与 `init_config()`(:51-67)整段删除;`use ... enhance ...` 与 `block_on` import 若失去消费一并删。

- [ ] **Step 4: CoreManager**——update_config 的 `Config::generate().await?` 替换:

```rust
        // FIXME(actor-migration): legacy regenerate path for pre-T08 callers
        // (enhance_profiles/delete_profile ipc etc.). New code must use
        // NyanpasuClient::rebuild_running_config(). Remove after T10.
        crate::client::rebuild::regenerate().await?;
```

`run_core` 内 :469 的 `Config::generate().await?;` **删除**(重启语义 = 应用当前 draft;所有变更路径均前置再生成);:561 的 `Config::generate().await` 替换为 `crate::client::rebuild::regenerate().await`(保留原 `Config::verge().discard()` 错误分支)。

- [ ] **Step 5: feat.rs 三处**——:279/:339/:363 的 `Config::generate().await?;` 逐处替换为 `crate::client::rebuild::regenerate().await?;`。

- [ ] **Step 6: enhance/mod.rs**——`pub async fn enhance()` 上方加:

```rust
#[allow(dead_code)]
// FIXME(actor-migration): legacy enhance pipeline kept only for T10 deletion
// bookkeeping; Config::generate() was removed in T07 and nothing calls this.
// New code must use RuntimeBuilder via NyanpasuClient. Remove in T10.
```

- [ ] **Step 7: 全量验证**

Run: `cargo build --manifest-path ./backend/Cargo.toml -p clash-nyanpasu && cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 编译绿 + 全测试绿;**T06A golden 4 测试原样绿**(fixtures 零改动 = 生成行为无回归)。
Run: `Select-String -Path backend/tauri/src -Pattern "Config::generate\(\)" -Recurse`(等价 grep)
Expected: 零命中。

- [ ] **Step 8: Commit**

```powershell
git add backend/tauri/src
git commit -m "refactor(tauri): switch composition root and generate callers to runtime pipeline"
```

---

### Task 9: 台账判据 + 契约回写

**Files:**

- Modify: `docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md`(T07 卡尾部追加执行修正)

- [ ] **Step 1: 台账 grep**

Run(PowerShell):

```powershell
(Get-ChildItem backend/tauri/src -Recurse -Include *.rs | Select-String -Pattern "(TODO|FIXME)\(actor-migration\)").Count
```

Expected: 相对 T07 起点(先在起点 commit 上跑一次记基数)**恰好 +7**;逐处核对为契约修正②所列 ①–⑦。

- [ ] **Step 2: 回写 task.md T07 卡**——「2026-07-06 契约修正」块之后追加:

```markdown
**2026-07-06 执行修正(T07 实物,plan 期即发现)**:

- spec 缺口:`Config::generate()` 实有 6 个调用点(update_config/run_core:469/core.rs:561/feat.rs:279,339,363)+ `init_config` 首铸。处置:`generate()`/`init_config()` 删除;新增 `client/rebuild.rs` 再生成桥(FIXME 全局,oneshot 保序)供 update_config/core.rs:561/feat×3;run_core 内再生成删除(重启=应用当前 draft);首铸与端口回写移入 `resolve_setup`(client 取自 app state)。`CoreManager` 拆出 `apply_config()`(check+file+put),facade 经 `RunningCoreBridge` 适配器调用。
- 台账:新增 TODO/FIXME(actor-migration) 注释块恰好 7 处(regenerate draft 写入/LegacyCoreBridge×2/桥定义/resolve_setup 端口回写/enhance dead_code/update_config legacy 路径);「恰好三处」勘误作废。
- UI 事件走注入 `UiEventSink`(与 Handle 同 URI 同 payload,不占台账);`ClientSetupArgs` 增 `ui_sink`/`core` 两注入点。
- 端口:`SessionPortResolver`(pick_and_try_port + 指纹缓存,eager 首解析于核启动前)兼任 `SelfProxyPortSource`;legacy 镜像回写 mixed-port/external-controller,`prepare_external_controller_port` 双头解析删除。**T11 必查**:typed `overrides.secret` 与 legacy IClashTemp secret 的一致性(api 客户端 401 风险,本卡未回写 secret——字段私有)。
- 映射:exists_keys←applied_fields(artifact.rs §9.3);postprocessing 按 SnapshotNodeKey 变体表(Scoped→scopes[host]transforms[idx]]、Global→global[global_transforms[idx]]、BuiltinTransform→global[builtin 名]、其余→advice)。文件三方法 fs 直调为有意取舍(小 IO,与 legacy ipc 等同)。
```

- [ ] **Step 3: Commit**

```powershell
git add docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md
git commit -m "docs(pr3): record T07 execution addenda in task card"
```

---

## Self-Review 结论(plan 作者自查)

- 覆盖:卡片目标四件套(spawn 进 root/facade 全方法/generate 切换/RebuildNotifier)+ 勘误 8 条全部有 Task;6 个 generate 调用点逐一处置;T06A golden 作为切换回归门禁(Task 8 Step 7)。
- 无占位符:全部代码块完整;三处「以现场为准」均为窄适配注记(derive 列表/泛型参数/automock 签名),附判定方法,非 TBD。
- 类型一致性:facade 方法表与 T08 消费签名一致(add_profile→ProfileId 已同步);`regenerate_runtime`/`apply_config`/`rebuild::regenerate` 三层职责不重叠(regenerate=写 draft;apply=check+file+put;rebuild=regenerate+apply+UI+中断)。
