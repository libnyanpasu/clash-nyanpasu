# PR-4 Runtime 派生化收尾 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消灭可写 runtime 状态——`Config::runtime()`/`IRuntime`/`generate_file` 删除,facade 持有 `SimpleStateManager<Option<RuntimeState>>`,产物走「候选 check → atomicwrites 晋升」管线,变更类 profile IPC 返回 `RebuildOutcome`(committed/degraded)。

**Architecture:** 方案 A(spec D4):重建时一次派生只读 `RuntimeState`,四条读 IPC wire 保形;`RunningCoreBridge` 拆 `check_and_promote` + `apply_config` 两操作(spec D5 保守顺序);回滚 = 从已提交状态重建。降级模型只覆盖 facade 的 post-commit 内联 rebuild 路径(唯一收口点 `after_commit`),legacy 桥调用方维持 Err/draft-discard。

**Tech Stack:** Rust(tauri crate + nyanpasu-core SimpleStateManager + atomicwrites + mockall)、tauri-specta 绑定、React/TS(react-query MutationCache、paraglide i18n)。

**Spec:** `docs/superpowers/specs/2026-07-12-pr4-runtime-derivation-cleanup-design.md`(已批准)。

## Global Constraints

- 分支 `refactor/pr4-runtime-derivation`(已存在,含 spec commit `d4be2e2b`);worktree 隔离按 CLAUDE.md §17 可选。
- Rust toolchain 钉 nightly-2026-05-27(rust-toolchain 已钉);若出现 ICE/LNK1120,按符号名定点 `cargo clean -p <crate>`(kache 污染,见项目记忆)。
- **pre-commit 对 backend 文件跑全量 clippy(3–8 分钟)**:所有含 `.rs` 的 `git commit` 必须后台运行(Bash `run_in_background`),不可用默认 2 分钟超时等待。
- **bindings.ts 提交时序(铁律 3 / T09 先例)**:`cargo test` 会运行 `export_typescript_bindings` 就地重写 `frontend/interface/src/ipc/bindings.ts`。Task 1–8 的每次 backend commit 前执行 `git checkout -- frontend/interface/src/ipc/bindings.ts`;bindings 只随 Task 9 与前端适配同 commit 落账。
- push 走 HTTPS:`git push https://github.com/libnyanpasu/clash-nyanpasu.git refactor/pr4-runtime-derivation`(origin SSH 已死)。
- 四条读命令(`get_runtime_config/yaml/exists/postprocessing_output`)wire 形状不得变化;`enhance_profiles`、`save_profile_file` 返回类型不变(无前置 commit,不套降级模型——此为对 spec §6.2 括注的枚举期修正,规则「post-commit 内联 rebuild 才降级」优先)。
- 测试统一 `cargo test --workspace --all-features <filter>`;全量验证在 Task 10。

---

### Task 1: `RuntimeState` 类型与 artifact 映射

**Files:**

- Create: `backend/tauri/src/client/runtime.rs`
- Modify: `backend/tauri/src/client/mod.rs`(注册模块)
- Modify: `backend/tauri/src/enhance/artifact_bridge.rs`
- Modify: `backend/tauri/src/enhance/mod.rs`(导出)

**Interfaces:**

- Produces: `crate::client::runtime::{RuntimeState, runtime_config_path() -> anyhow::Result<PathBuf>, candidate_config_path() -> PathBuf, RUNTIME_CONFIG_DIR, RUNTIME_CONFIG, CANDIDATE_CONFIG}`;`crate::enhance::runtime_state_from_artifact(&RuntimeArtifact, &Profiles, ClashCore, bool) -> anyhow::Result<RuntimeState>`。
- Consumes: 现有 `map_postprocessing`/`builtin_transforms_for`(artifact_bridge 内部)。

- [ ] **Step 1: 新建 `client/runtime.rs`**

```rust
//! Runtime derived state (PR-4): the read model the facade holds after each
//! rebuild, plus the product/candidate config file locations. Runtime is a
//! pure derivation — there is no writable runtime state anywhere else.

use std::path::PathBuf;

use serde_yaml::Mapping;

use crate::{enhance::PostProcessingOutput, utils::dirs};

pub const RUNTIME_CONFIG_DIR: &str = "runtime";
pub const RUNTIME_CONFIG: &str = "clash-config.yaml";
pub const CANDIDATE_CONFIG: &str = "clash-config-check.yaml";

/// Read model of the current runtime derivation (former `IRuntime`, minus the
/// draft machinery). Derived once per rebuild while the profiles snapshot is
/// in hand; the four runtime read commands serve straight from this.
#[derive(Debug, Clone, Default)]
pub struct RuntimeState {
    pub config: Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
}

/// The promoted (checked) product consumed by core start/hot-reload. Same
/// location the legacy `Config::runtime_config_path()` used.
pub fn runtime_config_path() -> anyhow::Result<PathBuf> {
    Ok(dirs::app_config_dir()?
        .join(RUNTIME_CONFIG_DIR)
        .join(RUNTIME_CONFIG))
}

/// Where a rebuild writes the unchecked candidate before check + promote
/// (spec D5: the product only ever holds configs that passed the check).
pub fn candidate_config_path() -> PathBuf {
    std::env::temp_dir().join(CANDIDATE_CONFIG)
}
```

- [ ] **Step 2: 在 `client/mod.rs` 注册模块**(mod 声明区,`pub mod rebuild;` 之后)

```rust
pub mod runtime;
```

- [ ] **Step 3: `artifact_bridge.rs` 移主体、留薄壳**

`runtime_from_artifact`(:95-121)的函数体移入新函数,旧函数改为委托(Task 7 删除):

```rust
pub fn runtime_state_from_artifact(
    artifact: &RuntimeArtifact,
    profiles: &Profiles,
    core: ClashCore,
    builtin_enabled: bool,
) -> anyhow::Result<crate::client::runtime::RuntimeState> {
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
    Ok(crate::client::runtime::RuntimeState {
        config,
        exists_keys,
        postprocessing_output: map_postprocessing(&artifact.step_logs, profiles, &builtin_names),
    })
}

// FIXME(actor-migration): thin legacy shell over runtime_state_from_artifact.
// New code must use runtime_state_from_artifact. Removed by PR-4 Task 7 with
// IRuntime itself.
pub fn runtime_from_artifact(
    artifact: &RuntimeArtifact,
    profiles: &Profiles,
    core: ClashCore,
    builtin_enabled: bool,
) -> anyhow::Result<IRuntime> {
    let state = runtime_state_from_artifact(artifact, profiles, core, builtin_enabled)?;
    Ok(IRuntime {
        config: Some(state.config),
        exists_keys: state.exists_keys,
        postprocessing_output: state.postprocessing_output,
    })
}
```

- [ ] **Step 4: `enhance/mod.rs` 导出新函数**

```rust
pub use artifact_bridge::{runtime_from_artifact, runtime_state_from_artifact};
```

- [ ] **Step 5: 编译 + 既有测试作钉**

Run: `cargo test --workspace --all-features artifact_bridge`
Expected: PASS(既有 `maps_scoped_global_and_builtin_logs_to_legacy_layout` 等全绿;旧函数委托新函数,映射语义由既有测试钉住)

- [ ] **Step 6: Commit**(backend commit → 先还原 bindings,后台运行)

```bash
git checkout -- frontend/interface/src/ipc/bindings.ts
git add backend/tauri/src/client/runtime.rs backend/tauri/src/client/mod.rs backend/tauri/src/enhance/artifact_bridge.rs backend/tauri/src/enhance/mod.rs
git commit -m "refactor(tauri): introduce RuntimeState read model and artifact mapping (PR-4 T1)"
```

---

### Task 2: facade 持有 SimpleStateManager(双写过渡)

**Files:**

- Modify: `backend/tauri/src/client/mod.rs`(inner 字段、构造、`regenerate_runtime_with`、测试)
- Modify: `backend/tauri/src/client/runtime.rs`(manager 构造 helper)

**Interfaces:**

- Consumes: Task 1 的 `RuntimeState`、`runtime_state_from_artifact`。
- Produces: `NyanpasuClient::runtime_state() -> Arc<Option<RuntimeState>>`(pub async);`client::runtime::new_runtime_state_store() -> anyhow::Result<RuntimeStateStore>`;类型别名 `RuntimeStateStore = tokio::sync::RwLock<SimpleStateManager<Option<RuntimeState>>>`;`with_parts` 增第 10 参 `runtime: RuntimeStateStore`。

- [ ] **Step 1: 确认 nyanpasu-core 导入路径**

Run: `rg -n "PersistentStateManager" backend/tauri/src/state/application.rs | head -5`
以该文件使用的 `use nyanpasu_core::...` 路径为准书写下面的 import(预期为 `nyanpasu_core::state::{SimpleStateManager, SimpleStateManagerSetup}`)。

- [ ] **Step 2: `client/runtime.rs` 添加 store 别名与构造**

```rust
use nyanpasu_core::state::{SimpleStateManager, SimpleStateManagerSetup};

/// Facade-held runtime store. The RwLock is a narrowly scoped implementation
/// detail (CLAUDE.md §8 exception): `upsert` needs `&mut`, writers are already
/// serialized by the facade `rebuild_gate`, readers take `snapshot()`.
/// SimpleStateManager (not a bare RwLock<Option<..>>) is deliberate: its
/// StateCoordinator ack subscribers are the landing point for the
/// TODO(post-PR-7) ack-driven rollback direction (spec D2).
pub type RuntimeStateStore = tokio::sync::RwLock<SimpleStateManager<Option<RuntimeState>>>;

pub async fn new_runtime_state_store() -> anyhow::Result<RuntimeStateStore> {
    let manager = SimpleStateManagerSetup::builder()
        .initial_state(None)
        .assemble()
        .initialize()
        .await
        .map_err(|_| anyhow::anyhow!("failed to initialize runtime state store"))?;
    Ok(tokio::sync::RwLock::new(manager))
}
```

- [ ] **Step 3: 写失败测试(manager 持有重建产物)**

`client/mod.rs` tests 模块中,定位含断言字符串 `"runtime draft written"` 的测试(约 :1130),在其 `block_on` 块内 **追加**:

```rust
let state = client.runtime_state().await;
let state = state
    .as_ref()
    .as_ref()
    .expect("runtime state stored after rebuild");
assert!(state.config.get("mixed-port").is_some());
```

Run: `cargo test --workspace --all-features runtime_draft` (按该测试实名过滤)
Expected: FAIL — `runtime_state` 方法不存在(编译错误)

- [ ] **Step 4: inner 字段 + 构造 + 访问器**

`NyanpasuClientInner` 增字段(`rebuild_gate` 之后):

```rust
    /// PR-4: derived runtime read model (see client/runtime.rs docs).
    runtime: runtime::RuntimeStateStore,
```

`try_new_with_args` 的 `block_on` 闭包内(profiles 构造之后)增 `let runtime_store = runtime::new_runtime_state_store().await?;` 并加入返回元组;`with_parts` 增参数 `runtime: runtime::RuntimeStateStore` 并填入 struct。两个测试 helper(约 :845 与 :909 的 `NyanpasuClient::with_parts(` 调用处)各增实参:

```rust
crate::client::runtime::new_runtime_state_store()
    .await
    .expect("runtime state store"),
```

访问器(`session_ports` 之后):

```rust
    pub async fn runtime_state(&self) -> std::sync::Arc<Option<runtime::RuntimeState>> {
        self.inner.runtime.read().await.snapshot()
    }
```

- [ ] **Step 5: `regenerate_runtime_with` 双写**

spawn_blocking 闭包改为返回 `(RuntimeState, IRuntime)`:

```rust
        let (state, legacy_runtime) = tokio::task::spawn_blocking(
            move || -> anyhow::Result<(crate::client::runtime::RuntimeState, crate::config::IRuntime)> {
                let content = FsProfileContentSource::new(profiles_dir);
                let scripts = EnhanceScriptRunner::new()?;
                let input = RuntimeBuildInput {
                    profiles: profiles.clone(),
                    clash,
                    app,
                    resolved_ports,
                };
                let artifact = RuntimeBuilder::build(&input, &content, &scripts)?;
                let state = runtime_state_from_artifact(&artifact, &profiles, core, builtin_enabled)?;
                let legacy_runtime = crate::config::IRuntime {
                    config: Some(state.config.clone()),
                    exists_keys: state.exists_keys.clone(),
                    postprocessing_output: state.postprocessing_output.clone(),
                };
                Ok((state, legacy_runtime))
            },
        )
        .await
        .map_err(|error| ClientError::Custom(format!("runtime build task failed: {error}")))?
        .map_err(ClientError::Anyhow)?;
        {
            let mut store = self.inner.runtime.write().await;
            store
                .upsert(Some(state))
                .await
                .map_err(|error| ClientError::Custom(format!("failed to store runtime state: {error}")))?;
        }
        // TODO(actor-migration): temporary dual-write to Config::runtime() draft (B8).
        // Reason: legacy readers (four runtime IPCs / generate_file consumers)
        // migrate in PR-4 Tasks 3-6. Remove in PR-4 Task 7.
        *crate::config::Config::runtime().draft() = legacy_runtime;
        Ok(())
```

同时把 `use crate::enhance::{..., runtime_from_artifact}` 中的 `runtime_from_artifact` 改为 `runtime_state_from_artifact`。若 `upsert` 的错误类型不满足 `Display`,改用 `{error:?}` 格式化。

- [ ] **Step 6: 测试通过**

Run: `cargo test --workspace --all-features runtime_draft`(同 Step 3 过滤)
Expected: PASS(draft 断言与 manager 断言同时成立——双写期)

- [ ] **Step 7: Commit**

```bash
git checkout -- frontend/interface/src/ipc/bindings.ts
git add backend/tauri/src/client/runtime.rs backend/tauri/src/client/mod.rs
git commit -m "refactor(tauri): facade holds SimpleStateManager<Option<RuntimeState>> (PR-4 T2, dual-write)"
```

---

### Task 3: 四条 runtime 读 IPC 改读 manager(wire 保形)

**Files:**

- Modify: `backend/tauri/src/ipc.rs:305-348`

**Interfaces:**

- Consumes: Task 2 `client.runtime_state()`。
- Produces: 四命令签名变为 `pub async fn xxx(client: State<'_, NyanpasuClient>, ..)`,返回类型不变(wire 零变化;State 注入对前端透明)。

- [ ] **Step 1: 改写四命令**

```rust
/// get the runtime config
#[tauri::command]
#[specta::specta]
// TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value). Wrapped in
// Any<> to avoid infinite type expansion. Replace with a typed ClashConfig struct if desired.
pub async fn get_runtime_config(
    client: State<'_, NyanpasuClient>,
) -> Result<Option<specta_typescript::Any<serde_json::Value>>> {
    let state = client.runtime_state().await;
    match state.as_ref() {
        Some(state) => {
            let yaml_value = serde_yaml::to_value(&state.config)?;
            let json_value = serde_json::to_value(&yaml_value)?;
            let wrapped: specta_typescript::Any<serde_json::Value> =
                serde_json::from_value(json_value)?;
            Ok(Some(wrapped))
        }
        None => Ok(None),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn get_runtime_yaml(client: State<'_, NyanpasuClient>) -> Result<String> {
    let state = client.runtime_state().await;
    let mapping = (state
        .as_ref()
        .as_ref()
        .map(|state| &state.config)
        .ok_or(anyhow::anyhow!("failed to parse config to yaml file"))
        .and_then(|config| {
            serde_yaml::to_string(config).context("failed to convert config to yaml")
        }))?;
    Ok(mapping)
}

#[tauri::command]
#[specta::specta]
pub async fn get_runtime_exists(client: State<'_, NyanpasuClient>) -> Result<Vec<String>> {
    Ok(client
        .runtime_state()
        .await
        .as_ref()
        .as_ref()
        .map(|state| state.exists_keys.clone())
        .unwrap_or_default())
}

#[tauri::command]
#[specta::specta]
pub async fn get_postprocessing_output(
    client: State<'_, NyanpasuClient>,
) -> Result<PostProcessingOutput> {
    Ok(client
        .runtime_state()
        .await
        .as_ref()
        .as_ref()
        .map(|state| state.postprocessing_output.clone())
        .unwrap_or_default())
}
```

空态语义与今日 `IRuntime::new()` 逐条对齐:config→`Ok(None)`、yaml→`Err`(报错文案保持逐字)、exists→`Ok(vec![])`、postprocessing→`Ok(default)`。

- [ ] **Step 2: 空态回归测试**

`client/mod.rs` tests 增(挨着现有 facade 测试,复用其 client 构造 helper):

```rust
    #[test]
    fn runtime_state_is_none_before_first_rebuild() {
        let dir = tempdir().unwrap();
        let client = tauri::async_runtime::block_on(test_client(&dir));
        let state = tauri::async_runtime::block_on(client.runtime_state());
        assert!(state.as_ref().is_none());
    }
```

(若 :845 的构造 helper 实名不同,按实名替换 `test_client`。)

Run: `cargo test --workspace --all-features runtime_state_is_none_before_first_rebuild`
Expected: PASS

- [ ] **Step 3: 全量编译钉**

Run: `cargo check --workspace --all-targets --all-features`
Expected: 绿(bindings.ts 会被后续 test 重写——本任务不跑 export 测试也无 wire 变化)

- [ ] **Step 4: Commit**

```bash
git checkout -- frontend/interface/src/ipc/bindings.ts
git add backend/tauri/src/ipc.rs backend/tauri/src/client/mod.rs
git commit -m "refactor(tauri): serve runtime read IPCs from facade RuntimeState (PR-4 T3)"
```

---

### Task 4: 桥拆分 + 候选→check→晋升管线

**Files:**

- Modify: `backend/tauri/src/client/core_bridge.rs`(trait + impl + promote helper + 单测)
- Modify: `backend/tauri/src/core/clash/core.rs`(`check_config` 收窄、`apply_config` 只 put、`Instance::try_new` 只读产物路径、`change_core` 的 check 调用适配)
- Modify: `backend/tauri/src/client/mod.rs`(`regenerate_runtime_with` 尾部接管线;mock 期望)
- Modify: `backend/tauri/src/utils/resolve.rs:175-193`(boot 段)

**Interfaces:**

- Consumes: Task 1 `runtime_config_path()/candidate_config_path()`。
- Produces: `RunningCoreBridge::check_and_promote(&self, candidate: &camino::Utf8Path) -> anyhow::Result<()>`(trait 新方法;`apply_config`/`on_profile_change` 签名不变);`CoreManager::check_config(&self, config_path: &camino::Utf8Path) -> Result<()>`。

- [ ] **Step 1: 失败测试——promote helper 与调用顺序**

`core_bridge.rs` tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn promote_candidate_atomically_replaces_product() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join("candidate.yaml");
        let product = dir.path().join("runtime").join("clash-config.yaml");
        std::fs::write(&candidate, "mode: rule\n").unwrap();
        promote_candidate(&candidate, &product).await.unwrap();
        assert_eq!(std::fs::read_to_string(&product).unwrap(), "mode: rule\n");
        // second promote overwrites
        std::fs::write(&candidate, "mode: direct\n").unwrap();
        promote_candidate(&candidate, &product).await.unwrap();
        assert_eq!(std::fs::read_to_string(&product).unwrap(), "mode: direct\n");
    }
}
```

`client/mod.rs` tests 增顺序断言测试(复用 :909 一带的 harness——它接收注入的 `core` mock;按该 harness 实名调用):

```rust
    #[test]
    fn rebuild_checks_and_promotes_before_core_apply() {
        let mut core = MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_| Ok(()));
        core.expect_apply_config()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());
        // 构造 client(注入上面的 core)、add + activate 一个最小 File profile,
        // 与 :1100 的既有测试同套路;断言 activate 成功即可——顺序由 mockall 钉住。
    }
```

Run: `cargo test --workspace --all-features rebuild_checks_and_promotes`
Expected: FAIL — `expect_check_and_promote` 不存在(trait 无此方法)

- [ ] **Step 2: trait + impl**

`core_bridge.rs` 全文重写为:

```rust
//! Boundary adapter for "apply the regenerated runtime to the running core"
//! (PR-3 T07, reshaped by PR-4). The facade depends on this trait so it stays
//! testable; the production impl concentrates the legacy-global touches behind
//! documented bridges.

use std::path::Path;

use async_trait::async_trait;
use camino::Utf8Path;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RunningCoreBridge: Send + Sync + 'static {
    /// Check the candidate config with the selected core binary, then
    /// atomically promote it to the runtime product (spec D5: the product
    /// only ever holds checked configs). Usable on the boot path where the
    /// core is not running yet.
    async fn check_and_promote(&self, candidate: &Utf8Path) -> anyhow::Result<()>;
    /// Push the promoted product to the running core over its api.
    async fn apply_config(&self) -> anyhow::Result<()>;
    async fn on_profile_change(&self);
}

/// Atomic candidate -> product replacement (atomicwrites: temp file + durable
/// rename; readers never observe a half-written product).
pub(crate) async fn promote_candidate(candidate: &Path, product: &Path) -> anyhow::Result<()> {
    let bytes = tokio::fs::read(candidate).await?;
    if let Some(parent) = product.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let product = product.to_path_buf();
    tokio::task::spawn_blocking(move || {
        atomicwrites::AtomicFile::new(&product, atomicwrites::OverwriteBehavior::AllowOverwrite)
            .write(|file| std::io::Write::write_all(file, &bytes))
    })
    .await?
    .map_err(|error| anyhow::anyhow!("failed to promote runtime config: {error}"))?;
    Ok(())
}

pub struct LegacyCoreBridge;

#[async_trait]
impl RunningCoreBridge for LegacyCoreBridge {
    async fn check_and_promote(&self, candidate: &Utf8Path) -> anyhow::Result<()> {
        // TODO(actor-migration): temporary bridge to CoreManager::global().
        // Reason: core lifecycle is PR-5 (CoreActor).
        // Remove when: PR-5 lands CoreActor and the facade owns core apply.
        crate::core::CoreManager::global()
            .check_config(candidate)
            .await?;
        let product = crate::client::runtime::runtime_config_path()?;
        promote_candidate(candidate.as_std_path(), &product).await
    }

    async fn apply_config(&self) -> anyhow::Result<()> {
        // TODO(actor-migration): temporary bridge to CoreManager::global().
        // Reason: core lifecycle is PR-5 (CoreActor).
        // Remove when: PR-5 lands CoreActor and the facade owns core apply.
        crate::core::CoreManager::global().apply_config().await
    }

    async fn on_profile_change(&self) {
        // TODO(actor-migration): connection interruption still reads Config::verge()
        // inside the service. Reason: break_when_* options + clash api client are
        // PR-6 scope. Remove when: interruption reads typed
        // ClashConfig.break_connection via an injected client.
        let _ =
            crate::core::connection_interruption::ConnectionInterruptionService::on_profile_change(
            )
            .await;
    }
}
```

(注意:`on_profile_change` 的 TODO 由「PR-4/PR-6」改为「PR-6」——spec §7 勘误项。)

- [ ] **Step 3: `CoreManager` 侧收窄**

`core.rs:424-447` `check_config` 改签名(仅删 generate 两行、改参数):

```rust
    pub async fn check_config(&self, config_path: &Utf8Path) -> Result<()> {
        use nyanpasu_utils::core::instance::CoreInstance;

        let clash_core = { Config::verge().latest().clash_core };
        let clash_core = clash_core.unwrap_or(ClashCore::ClashPremium);
        let clash_core: nyanpasu_utils::core::CoreType = (&clash_core).into();

        let app_dir = dirs::app_data_dir()?;
        let app_dir = Utf8PathBuf::from_path_buf(app_dir)
            .map_err(|_| anyhow::anyhow!("failed to convert app dir to utf8 path"))?;
        let binary_path = find_binary_path(&clash_core)?;
        let binary_path = Utf8PathBuf::from_path_buf(binary_path)
            .map_err(|_| anyhow::anyhow!("failed to convert binary path to utf8 path"))?;
        log::debug!(target: "app", "check config in `{clash_core}`");
        CoreInstance::check_config_(&clash_core, config_path, &binary_path, &app_dir)
            .await
            .context("failed to check config")
            .inspect_err(|e| log::error!(target: "app", "failed to check config: {e:?}"))?;

        Ok(())
    }
```

`apply_config`(:608-632)删 check + generate,只 put 产物:

```rust
    /// Push the promoted runtime product to the running core over the api.
    /// Check + promote happen in the rebuild pipeline (RunningCoreBridge::
    /// check_and_promote) before this is called.
    pub async fn apply_config(&self) -> Result<()> {
        let path = crate::client::runtime::runtime_config_path()?;
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

`Instance::try_new`(:97-100)只读产物路径:

```rust
        let config_path =
            camino::Utf8PathBuf::from_path_buf(crate::client::runtime::runtime_config_path()?)
                .map_err(|e| {
                    anyhow::anyhow!("failed to convert config path to utf8 path: {:?}", e)
                })?;
```

`change_core`(:564)的 `self.check_config().await` 调用暂改为传产物路径(Task 5 将整段重写删除):

```rust
        let product = camino::Utf8PathBuf::from_path_buf(
            crate::client::runtime::runtime_config_path()?,
        )
        .map_err(|_| anyhow::anyhow!("failed to convert config path to utf8 path"))?;
        if let Err(err) = self.check_config(&product).await {
```

- [ ] **Step 4: facade 管线尾部**

`regenerate_runtime_with` 在 manager upsert / 双写之后、`Ok(())` 之前追加(spawn_blocking 闭包同步多算一个 yaml 串,返回三元组 `(state, legacy_runtime, yaml)`):

```rust
                let yaml = format!(
                    "# Generated by Clash Nyanpasu\n\n{}",
                    serde_yaml::to_string(&state.config)?
                );
```

(闭包内、构造 legacy_runtime 之后)然后管线尾部:

```rust
        // Candidate -> check -> promote (spec D5): the product file only ever
        // holds configs that passed the core binary's check.
        let candidate = crate::client::runtime::candidate_config_path();
        tokio::fs::write(&candidate, yaml)
            .await
            .map_err(|error| ClientError::Custom(format!("failed to write candidate: {error}")))?;
        let candidate = utf8_path(candidate).map_err(ClientError::Anyhow)?;
        self.inner
            .core
            .check_and_promote(&candidate)
            .await
            .map_err(ClientError::Anyhow)?;
        Ok(())
```

- [ ] **Step 5: boot 段(resolve.rs:175-193)**

```rust
        // 启动首铸:profiles/clash/app 快照 → RuntimeBuilder → 候选 check → 晋升产物
        log::trace!("init config");
        log_err!(tauri::async_runtime::block_on(client.regenerate_runtime()));
        // 与旧 init_config 相同的兜底:重建/检查失败或首启无产物时落默认 clash 配置
        let runtime_path = crate::client::runtime::runtime_config_path()
            .expect("failed to resolve runtime config path");
        if !runtime_path.exists() {
            if let Some(parent) = runtime_path.parent() {
                log_err!(std::fs::create_dir_all(parent));
            }
            log_err!(help::save_yaml(
                &runtime_path,
                &Config::clash().latest().0,
                Some("# Clash Nyanpasu Runtime"),
            ));
        }
```

- [ ] **Step 6: 修既有 mock 期望**

Run: `rg -n "expect_apply_config" backend/tauri/src`
每个设置 `expect_apply_config` 的测试(rebuild 会先 check_and_promote)前面补:

```rust
        core.expect_check_and_promote().returning(|_| Ok(()));
```

- [ ] **Step 7: 测试通过**

Run: `cargo test --workspace --all-features promote_candidate_atomically_replaces_product rebuild_checks_and_promotes_before_core_apply`
再跑 facade 全套:`cargo test --workspace --all-features client::`
Expected: PASS

- [ ] **Step 8: Commit**

```bash
git checkout -- frontend/interface/src/ipc/bindings.ts
git add backend/tauri/src/client/core_bridge.rs backend/tauri/src/client/mod.rs backend/tauri/src/core/clash/core.rs backend/tauri/src/utils/resolve.rs
git commit -m "refactor(tauri): candidate->check->promote runtime pipeline behind split core bridge (PR-4 T4)"
```

---

### Task 5: `change_core` 回滚=重建 + C-M5 reload 删除

**Files:**

- Modify: `backend/tauri/src/core/clash/core.rs`(`change_core` :548-589、`run_core_inner` :464-466、`update_config` :596-600 注释)

**Interfaces:**

- Consumes: Task 4 管线(`regenerate()` 内含 check+promote,失败即 Err)。
- Produces: 无新接口;`Config::runtime()` 在 core.rs 内零残留。

- [ ] **Step 1: 重写 `change_core`**

```rust
    /// 切换核心
    #[instrument(skip(self))]
    pub async fn change_core(&self, clash_core: Option<ClashCore>) -> Result<()> {
        let clash_core = clash_core.ok_or(anyhow::anyhow!("clash core is null"))?;

        log::debug!(target: "app", "change core to `{clash_core}`");

        let _guard = self.run_lock.lock().await;

        Config::verge().draft().clash_core = Some(clash_core);

        // 重建 = 构建 + 以草稿核 check + 晋升(spec D5);失败即无产物变更。
        if let Err(err) = crate::client::rebuild::regenerate().await {
            Config::verge().discard();
            return Err(err);
        }

        // 清掉旧日志
        Logger::global().clear_log();

        match self.run_core_inner().await {
            Ok(_) => {
                tracing::info!("change core success");
                Config::verge().apply();
                log_err!(Config::verge().latest().save_file());
                Ok(())
            }
            Err(err) => {
                tracing::error!("failed to change core: {err:?}");
                Config::verge().discard();
                // 回滚 = 从已提交状态重建:旧核重新 check + 晋升,产物回旧。
                log_err!(crate::client::rebuild::regenerate().await);
                self.run_core_inner().await?;
                Err(err)
            }
        }
    }
```

(Task 4 Step 3 加的产物路径 check 段与三处 `Config::runtime().discard()/apply()` 全部随重写消失。)

- [ ] **Step 2: C-M5——删 `run_core_inner` 的 reload**

删除 :464-466 三行:

```rust
        // Reload clash config from file to get latest user preferences (e.g., mode)
        Config::clash().reload();
        log::debug!(target: "app", "reloaded clash config from file");
```

- [ ] **Step 3: FIXME 清偿期改写(spec §7)**

1. `core.rs:596-600` 注释中 `Remove when PR-4/5 migrate` 改为 `Remove when PR-5 migrates`(动词随之单数)。
2. `backend/tauri/src/client/rebuild.rs` 两处:`install_regen_bridge` 上方 FIXME 的 `Remove after PR-4/PR-5 migrate those flows onto injected clients.` 改为 `Remove after PR-5/PR-6 migrate those flows onto injected clients.`;`legacy_regen_inputs` FIXME 的 `Remove when: PR-4/5/6 migrate the legacy writers onto typed clients.` 改为 `Remove when: PR-5/6 migrate the legacy writers onto typed clients.`

- [ ] **Step 4: 编译钉 + 残留检查**

Run: `cargo check --workspace --all-targets --all-features && rg -n "Config::runtime" backend/tauri/src/core/`
Expected: 编译绿;core/ 目录零命中

- [ ] **Step 5: Commit**

```bash
git checkout -- frontend/interface/src/ipc/bindings.ts
git add backend/tauri/src/core/clash/core.rs
git commit -m "refactor(tauri): change_core rollback-by-rebuild, drop C-M5 clash reload (PR-4 T5)"
```

---

### Task 6: `feat::patch_clash` 恒重建

**Files:**

- Modify: `backend/tauri/src/feat.rs:221-298`
- Test: 同文件 tests(或文件尾新建 `#[cfg(test)] mod tests`)

**Interfaces:**

- Produces: `pub(crate) fn requires_core_restart(patch: &Mapping) -> bool`(feat.rs)。

- [ ] **Step 1: 失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_restart_only_for_port_controller_secret() {
        let mut patch = serde_yaml::Mapping::new();
        patch.insert("mode".into(), "direct".into());
        patch.insert("allow-lan".into(), true.into());
        assert!(!requires_core_restart(&patch));
        patch.insert("mixed-port".into(), 7890.into());
        assert!(requires_core_restart(&patch));
        let mut patch = serde_yaml::Mapping::new();
        patch.insert("secret".into(), "s".into());
        assert!(requires_core_restart(&patch));
        let mut patch = serde_yaml::Mapping::new();
        patch.insert("external-controller".into(), "127.0.0.1:9090".into());
        assert!(requires_core_restart(&patch));
    }
}
```

Run: `cargo test --workspace --all-features core_restart_only_for_port_controller_secret`
Expected: FAIL — 函数不存在

- [ ] **Step 2: 实现谓词 + 改写激活段**

feat.rs(`patch_clash` 上方)增:

```rust
/// PR-4: every clash patch feeds the rebuild input (guard overrides), so the
/// derived runtime always regenerates; only these fields need a core restart.
pub(crate) fn requires_core_restart(patch: &Mapping) -> bool {
    patch.get("mixed-port").is_some()
        || patch.get("secret").is_some()
        || patch.get("external-controller").is_some()
}
```

`patch_clash` 的「激活配置」段(:263-271)替换为:

```rust
        // 激活配置:任何 clash patch 都会进入 rebuild 输入,恒重建派生配置;
        // 仅端口/控制器/密钥变更需要重启核心(即时性由 IPC 层 api::patch_configs 直推保证)。
        crate::client::rebuild::regenerate().await?;
        if requires_core_restart(&patch) {
            CoreManager::global().run_core().await?;
            handle::Handle::refresh_clash();
        }
```

删除 :283 行 `Config::runtime().latest().patch_config(patch);`。

- [ ] **Step 3: 测试通过 + 编译**

Run: `cargo test --workspace --all-features core_restart_only_for_port_controller_secret && cargo check --workspace --all-targets --all-features`
Expected: PASS / 绿

- [ ] **Step 4: Commit**

```bash
git checkout -- frontend/interface/src/ipc/bindings.ts
git add backend/tauri/src/feat.rs
git commit -m "refactor(tauri): patch_clash always regenerates, drop runtime in-memory patch (PR-4 T6)"
```

---

### Task 7: 删除 legacy runtime(双写落幕 + grep 钉)

**Files:**

- Delete: `backend/tauri/src/config/runtime.rs`
- Modify: `backend/tauri/src/config/core.rs`、`backend/tauri/src/config/draft.rs`、`backend/tauri/src/config/mod.rs`
- Modify: `backend/tauri/src/ipc.rs`(收编 `PatchRuntimeConfig`)
- Modify: `backend/tauri/src/client/mod.rs`(删双写与测试 draft 断言)
- Modify: `backend/tauri/src/enhance/artifact_bridge.rs`、`backend/tauri/src/enhance/mod.rs`(删旧壳)

**Interfaces:**

- Produces: `ipc::PatchRuntimeConfig`(定义原样搬迁,仅位置变化;wire 不变)。
- 后置条件: `Config::runtime`、`IRuntime`、`generate_file`、`ConfigType` 在 backend/tauri/src 零命中。

- [ ] **Step 1: `PatchRuntimeConfig` 迁入 ipc.rs**(`patch_clash_config` 上方,连同 serde 属性逐字搬迁)

```rust
#[derive(Default, Debug, Clone, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub struct PatchRuntimeConfig {
    #[serde(default, rename = "allow-lan", skip_serializing_if = "Option::is_none")]
    pub allow_lan: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<bool>,
    #[serde(default, rename = "log-level", skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}
```

ipc.rs 若原经 `crate::config::PatchRuntimeConfig` 引用,删除该 import(本地定义生效);确保 `serde::{Deserialize, Serialize}` 已在 ipc.rs import。

- [ ] **Step 2: 删除文件与残迹**

1. 删除 `backend/tauri/src/config/runtime.rs`。
2. `config/mod.rs`: 删 `mod runtime;` 与 re-export 中的 `runtime::*`。
3. `config/draft.rs`: 删 `IRuntime` import 与 `draft_define!(IRuntime);`。
4. `config/core.rs`: 删 `IRuntime` import、`runtime_config` 字段与初始化、`runtime()` accessor、`generate_file()`、`runtime_config_path()`、`ConfigType`、`RUNTIME_CONFIG_DIR/RUNTIME_CONFIG/CHECK_CONFIG` 常量、`temp_dir` import(文件仅剩 clash/verge 两域)。
5. `client/mod.rs`: 删 `regenerate_runtime_with` 中 legacy_runtime 构造与 `*crate::config::Config::runtime().draft() = legacy_runtime;` 双写段(spawn_blocking 回到返回 `(RuntimeState, String)`);删含 `"runtime draft written"` 断言的旧三行(Task 2 加的 manager 断言保留)。
6. `enhance/artifact_bridge.rs`: 删 `runtime_from_artifact` 薄壳与 `crate::config::IRuntime` import;`enhance/mod.rs` 导出只剩 `runtime_state_from_artifact`。

- [ ] **Step 3: grep 钉 + 全测**

Run:

```bash
rg -n "Config::runtime|IRuntime|generate_file|ConfigType" backend/tauri/src
```

Expected: 零命中。

Run: `cargo test --workspace --all-features`
Expected: 全绿(bindings.ts 被重写属预期,commit 前还原)

- [ ] **Step 4: Commit**

```bash
git checkout -- frontend/interface/src/ipc/bindings.ts
git add -A backend/tauri/src/config backend/tauri/src/client backend/tauri/src/enhance backend/tauri/src/ipc.rs
git commit -m "refactor(tauri)!: delete Config::runtime()/IRuntime/generate_file, runtime is a pure derivation (PR-4 T7)"
```

---

### Task 8: `RebuildOutcome` 降级模型(后端 BC)

**Files:**

- Modify: `backend/tauri/src/client/runtime.rs`(新增两类型)
- Modify: `backend/tauri/src/client/mod.rs`(`after_commit` + 13 个 facade 方法签名 + 测试)
- Modify: `backend/tauri/src/ipc.rs`(12 条命令签名)
- Modify: `backend/tauri/src/specta_export.rs`(冻结测试断言)

**Interfaces:**

- Produces:
  - `client::runtime::RebuildOutcome`(specta,`{status:"ok"} | {status:"degraded",error}`)与 `RebuildOutcome::merge(self, other) -> RebuildOutcome`;
  - `client::runtime::CommitOutcome<T> { value: T, rebuild: RebuildOutcome }`(specta);
  - facade: `after_commit(&CommitReport) -> RebuildOutcome`(不再 Result);`activate_profile/delete_profile/reorder_profile/reorder_profiles_by_list/refresh_profile/patch_profile_metadata/patch_remote_profile_options/replace_profile_definition/set_global_transforms/set_profile_valid_fields -> Result<RebuildOutcome>`;`add_profile/create_profile/import_profile -> Result<(ProfileId, RebuildOutcome)>`;
  - IPC: 上述 10 条 unit 命令 + `create_profile` 返回 `Result<RebuildOutcome>`;`import_profile` 返回 `Result<CommitOutcome<ProfileId>>`。
- 明确不变: `enhance_profiles`(无前置 commit,失败即 Err)、`save_profile_file`(不触发 rebuild)。

- [ ] **Step 1: 类型定义**(`client/runtime.rs` 追加)

```rust
use serde::{Deserialize, Serialize};

/// Post-commit rebuild result for mutation IPC (spec §6.2, decision D2):
/// state is committed first; a failed rebuild degrades instead of erroring.
// TODO(post-PR-7): degraded outcome is transitional. State managers already
// expose async commit acks; the end-state is ack-driven rollback when config
// application fails, replacing this degraded-report model. Tracked in
// actor-migration-roadmap §6.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RebuildOutcome {
    Ok,
    Degraded { error: String },
}

impl RebuildOutcome {
    /// Combine sequential outcomes; the first degradation wins.
    pub fn merge(self, other: RebuildOutcome) -> RebuildOutcome {
        match self {
            RebuildOutcome::Degraded { .. } => self,
            RebuildOutcome::Ok => other,
        }
    }
}

/// Mutation payload + rebuild outcome for data-carrying commands (import).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CommitOutcome<T> {
    pub value: T,
    pub rebuild: RebuildOutcome,
}
```

- [ ] **Step 2: 失败测试——degraded 且状态已提交**

`client/mod.rs` tests(复用注入 mock core 的 harness;`check_and_promote` 失败即 rebuild 失败):

```rust
    #[test]
    fn activate_returns_degraded_and_keeps_commit_when_rebuild_fails() {
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .returning(|_| Err(anyhow::anyhow!("check boom")));
        core.expect_on_profile_change().returning(|| ());
        // 构造 client(注入 core)、add 一个最小 File profile(同 :1100 测试套路),然后:
        // let (uid, _) = client.add_profile(...).await.expect("add");
        // let outcome = client.activate_profile(Some(uid.clone())).await.expect("activate must commit");
        // assert!(matches!(outcome, crate::client::runtime::RebuildOutcome::Degraded { .. }));
        // let profiles = client.get_profiles().await.unwrap();
        // assert_eq!(profiles.current.as_ref(), Some(&uid), "state stays committed");
    }
```

(add_profile 若命中 auto-activate 触发 rebuild,先用 `add_profile` 原语而非 `create_profile`,add 本身 `affects_current==false` 不触发 rebuild。)

Run: `cargo test --workspace --all-features activate_returns_degraded`
Expected: FAIL — 类型/签名不存在

- [ ] **Step 3: `after_commit` 与 facade 签名**

```rust
    async fn after_commit(&self, report: &CommitReport) -> runtime::RebuildOutcome {
        // Post-commit side-effect failures are degraded results, not
        // transaction failures (T04 contract): the state is already
        // persisted, so surface them instead of dropping them.
        for warning in &report.warnings {
            tracing::warn!(
                warning = %warning,
                "profile commit completed with a degraded side effect",
            );
        }
        if report.affects_current {
            if let Err(error) = self.rebuild_running_config().await {
                tracing::warn!(%error, "post-commit rebuild failed; state stays committed (degraded)");
                return runtime::RebuildOutcome::Degraded {
                    error: error.to_string(),
                };
            }
        }
        runtime::RebuildOutcome::Ok
    }
```

10 个 unit 方法同构改写(示例 `activate_profile`,其余 `delete_profile/reorder_profile/reorder_profiles_by_list/refresh_profile/patch_profile_metadata/patch_remote_profile_options/replace_profile_definition/set_global_transforms/set_profile_valid_fields` 逐一同样处理):

```rust
    pub async fn activate_profile(
        &self,
        uid: Option<ProfileId>,
    ) -> Result<runtime::RebuildOutcome> {
        let report = self.inner.profiles.set_current(uid).await?;
        Ok(self.after_commit(&report).await)
    }
```

`add_profile`:

```rust
    pub async fn add_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> Result<(ProfileId, runtime::RebuildOutcome)> {
        let report = self.inner.profiles.add(request, initial_file).await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| ClientError::Custom("add committed without a created uid".into()))?;
        let rebuild = self.after_commit(&report).await;
        Ok((created, rebuild))
    }
```

`create_profile`:add 调用解构 `(uid, mut rebuild)`;auto-activate 块内 `rebuild = rebuild.merge(self.after_commit(&report).await);`,返回 `Ok((uid, rebuild))`。

`import_profile`:返回 `Result<(ProfileId, runtime::RebuildOutcome)>`——`refreshed` 块改为:

```rust
        let refreshed = self
            .inner
            .profiles
            .refresh_import(created.clone(), update_interval_explicit)
            .await;
        let mut rebuild = match refreshed {
            Ok(report) => self.after_commit(&report).await,
            Err(error) => {
                // First download failed = import failed; delete the empty shell...
                //(占位清理块原样保留)
                return Err(error.into());
            }
        };
```

尾部 `set_current_if_none` 块:`rebuild = rebuild.merge(self.after_commit(&report).await);`,返回 `Ok((created, rebuild))`。语义注意:rebuild 失败不再触发占位回删(committed/degraded 决策的直接效果),仅下载/事务失败(`refresh_import` Err)回删——行为变化在 commit message 标注。

内部调用点同步:`create_profile` 里 `self.add_profile(...)` 解构;`use-profile` 无关;tests 中所有 `client.add_profile(...)` 解构 `(uid, _)`,`activate_profile` 等断言 `.expect(...)` 后按需检查 outcome。

- [ ] **Step 4: IPC 签名**

10 条 unit 命令同构(示例;`reorder_profile/reorder_profiles_by_list/update_profile/delete_profile/set_global_transforms/set_profile_valid_fields/patch_profile_metadata/patch_remote_profile_options/replace_profile_definition` 相同处理):

```rust
#[tauri::command]
#[specta::specta]
pub async fn activate_profile(
    client: State<'_, NyanpasuClient>,
    uid: Option<ProfileId>,
) -> Result<crate::client::runtime::RebuildOutcome> {
    Ok(client.activate_profile(uid).await?)
}
```

`create_profile`(丢 uid、保留 rebuild):

```rust
pub async fn create_profile(
    client: State<'_, NyanpasuClient>,
    request: NewProfileRequest,
    file_data: Option<String>,
) -> Result<crate::client::runtime::RebuildOutcome> {
    let (_uid, rebuild) = client.create_profile(request, file_data).await?;
    Ok(rebuild)
}
```

`import_profile`:

```rust
pub async fn import_profile(
    client: State<'_, NyanpasuClient>,
    url: String,
    option: Option<RemoteProfileOptionsPatch>,
) -> Result<crate::client::runtime::CommitOutcome<ProfileId>> {
    let url = url::Url::parse(&url).context("failed to parse the url")?;
    let (uid, rebuild) = client.import_profile(url, option).await?;
    Ok(crate::client::runtime::CommitOutcome { value: uid, rebuild })
}
```

`enhance_profiles` 加边界注释(签名不变):

```rust
/// Rebuild-only command: there is no prior state commit, so a failure is a
/// plain error — the committed/degraded model (spec §6.2) does not apply.
```

- [ ] **Step 5: specta 冻结测试**

`specta_export.rs` 命名导出断言列表(`for name in [...]`)追加 `"RebuildOutcome"`, `"CommitOutcome"`。

- [ ] **Step 6: 回归钉——legacy 桥路径仍返 Err(spec §8)**

`client/mod.rs` tests 增(与 Step 2 同一 harness,mock `check_and_promote` 失败):

```rust
    #[test]
    fn legacy_regeneration_path_still_errors_on_rebuild_failure() {
        // 同 Step 2 构造注入失败 core 的 client,然后:
        // let result = tauri::async_runtime::block_on(client.regenerate_runtime_for_legacy());
        // assert!(result.is_err(), "legacy callers rely on Err to discard their drafts");
    }
```

(`regenerate_runtime_for_legacy` 读进程级 legacy 单例的默认值做输入转换,测试内只读不写,无竞态。)

Run: `cargo test --workspace --all-features legacy_regeneration_path_still_errors`
Expected: PASS

- [ ] **Step 7: 测试通过**

Run: `cargo test --workspace --all-features activate_returns_degraded && cargo test --workspace --all-features client::`
Run: `cargo test --workspace --all-features export_typescript_bindings`
Expected: 全绿(bindings.ts 已更新——**本任务不提交它**)

- [ ] **Step 8: Commit**

```bash
git checkout -- frontend/interface/src/ipc/bindings.ts
git add backend/tauri/src/client backend/tauri/src/ipc.rs backend/tauri/src/specta_export.rs
git commit -m "feat(tauri)!: profile mutation IPC returns RebuildOutcome (committed/degraded, C-M2) (PR-4 T8)"
```

---

### Task 9: 前端适配(bindings 落账 + 全局降级 toast)

**Files:**

- Modify: `frontend/interface/src/ipc/bindings.ts`(重生成,**本任务提交**)
- Modify: `frontend/interface/src/utils/index.ts`(`extractDegradedRebuild`)
- Modify: `frontend/interface/src/provider/index.tsx`(MutationCache + handler 注册)
- Modify: `frontend/interface/src/ipc/use-profile.ts`(create 归一化)
- Modify: `frontend/nyanpasu/src/pages/__root.tsx`(接线 toast)
- Modify: `frontend/nyanpasu/src/pages/(main)/main/profiles/$type/_modules/remote-profile-button.tsx`(uid 解构)
- Modify: `frontend/nyanpasu/messages/{en,zh-cn,zh-tw,ru}.json`(新 key)

**Interfaces:**

- Consumes: Task 8 的 `RebuildOutcome`/`CommitOutcome<ProfileId>` TS 绑定。
- Produces: `@nyanpasu/interface` 导出 `setDegradedRebuildHandler(handler: (error: string) => void)`;`useProfile().create.mutateAsync` 解析值归一为 `{ uid: ProfileId | null; rebuild: RebuildOutcome }`。

- [ ] **Step 1: 重生成 bindings**

Run: `cargo test --workspace --all-features export_typescript_bindings`
Expected: PASS;`git status` 显示 bindings.ts 变更(本任务保留提交)

- [ ] **Step 2: interface utils——降级嗅探**(`frontend/interface/src/utils/index.ts` 追加)

```ts
/**
 * Extract a degraded rebuild error from a mutation's resolved payload.
 * Handles both wire shapes from PR-4: a bare `RebuildOutcome`
 * (`{status:'degraded',error}`) and a `CommitOutcome<T>` (`{value, rebuild}`),
 * plus locally normalized `{ uid, rebuild }` shapes.
 */
export const extractDegradedRebuild = (data: unknown): string | undefined => {
  if (!data || typeof data !== 'object') {
    return undefined
  }
  const outcome =
    'rebuild' in data ? (data as { rebuild?: unknown }).rebuild : data
  if (!outcome || typeof outcome !== 'object') {
    return undefined
  }
  const candidate = outcome as { status?: unknown; error?: unknown }
  return candidate.status === 'degraded' && typeof candidate.error === 'string'
    ? candidate.error
    : undefined
}
```

- [ ] **Step 3: provider——全局 MutationCache**(`frontend/interface/src/provider/index.tsx` 全文)

```tsx
import type { PropsWithChildren } from 'react'
import {
  MutationCache,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query'
import { extractDegradedRebuild } from '../utils'
import { ClashWSProvider, useClashWSContext } from './clash-ws-provider'
import { MutationProvider } from './mutation-provider'

let degradedRebuildHandler: ((error: string) => void) | null = null

/**
 * Register the app-side notifier for committed-but-degraded rebuilds (PR-4
 * spec §6.3). The interface package owns detection (every mutation result
 * passes through the MutationCache); the app owns presentation (toast + i18n).
 */
export const setDegradedRebuildHandler = (handler: (error: string) => void) => {
  degradedRebuildHandler = handler
}

const queryClient = new QueryClient({
  mutationCache: new MutationCache({
    onSuccess: (data) => {
      const error = extractDegradedRebuild(data)
      if (error) {
        degradedRebuildHandler?.(error)
      }
    },
  }),
})

export const NyanpasuProvider = ({ children }: PropsWithChildren) => {
  return (
    <QueryClientProvider client={queryClient}>
      <MutationProvider>
        <ClashWSProvider>{children}</ClashWSProvider>
      </MutationProvider>
    </QueryClientProvider>
  )
}

export { useClashWSContext }
```

确认 `setDegradedRebuildHandler` 经 interface 包出口可达:`rg -n "provider" frontend/interface/src/index.ts`,若 index 仅 re-export 具名成员则补一行 export。

- [ ] **Step 4: `use-profile.ts` create 归一化**(:103-118 替换)

```ts
const create = useMutation({
  mutationFn: async (params: CreateParams) => {
    if (params.type === 'url') {
      const outcome = unwrapResult(
        await commands.importProfile(
          params.data.url,
          params.data.option ?? null,
        ),
      )
      return { uid: outcome.value, rebuild: outcome.rebuild }
    }
    const rebuild = unwrapResult(
      await commands.createProfile(params.data.request, params.data.fileData),
    )
    return { uid: null, rebuild }
  },
  onSuccess: invalidate,
})
```

其余 mutation(update/patchMetadata/patchRemoteOptions/replaceDefinition/activate/setValidFields/sort/drop)的 `unwrapResult(...)` 返回值即 `RebuildOutcome`,类型自动流经 bindings,**代码零改动**(全局 MutationCache 负责嗅探)。

- [ ] **Step 5: remote-profile-button uid 解构**(:71-82)

```ts
const { uid } = await create.mutateAsync({
  type: 'url',
  data: {
    url: data.url,
    option: {
      user_agent: data.option.user_agent,
      with_proxy: data.option.with_proxy,
      self_proxy: data.option.self_proxy,
      update_interval_minutes: data.option.update_interval,
    },
  },
})
```

(后续 `if (uid && data.name)` 原样成立。)

- [ ] **Step 6: i18n key ×4**

`frontend/nyanpasu/messages/en.json`:

```json
"profile_rebuild_degraded_message": "Saved, but applying the new config failed: {error}"
```

`zh-cn.json`: `"profile_rebuild_degraded_message": "已保存，但应用新配置失败：{error}"`
`zh-tw.json`: `"profile_rebuild_degraded_message": "已儲存，但套用新設定失敗：{error}"`
`ru.json`: `"profile_rebuild_degraded_message": "Сохранено, но не удалось применить новую конфигурацию: {error}"`

(插入位置对齐各文件中 `profile_import_rename_failed_message` 的邻近字母序。)

- [ ] **Step 7: `__root.tsx` 接线**

import 区追加:

```tsx
import { m } from '@/paraglide/messages'
import { message } from '@/utils/notification'
```

`@nyanpasu/interface` import 行追加 `setDegradedRebuildHandler`。`App` 组件上方新增:

```tsx
function DegradedRebuildNotifier() {
  useEffect(() => {
    setDegradedRebuildHandler((error) => {
      message(m.profile_rebuild_degraded_message({ error }), {
        title: 'Warning',
        kind: 'warning',
      })
    })
  }, [])
  return null
}
```

`<NyanpasuProvider>` 内(`<WindowReveal />` 同级)挂 `<DegradedRebuildNotifier />`。

- [ ] **Step 8: 前端验证**

Run: `pnpm -F interface build && pnpm typecheck && pnpm web:build`
Expected: 全绿

- [ ] **Step 9: Commit(bindings 随本 commit 落账)**

```bash
git add frontend/interface/src/ipc/bindings.ts frontend/interface/src/utils/index.ts frontend/interface/src/provider/index.tsx frontend/interface/src/index.ts frontend/interface/src/ipc/use-profile.ts "frontend/nyanpasu/src/pages/__root.tsx" "frontend/nyanpasu/src/pages/(main)/main/profiles/\$type/_modules/remote-profile-button.tsx" frontend/nyanpasu/messages
git commit -m "feat(ui): surface committed-but-degraded rebuilds via global mutation cache toast (PR-4 T9)"
```

---

### Task 10: 文档勘误 + 全量验证收官

**Files:**

- Modify: `docs/design/actor-migration-roadmap.md`
- Modify: `docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md`
- Modify: `docs/superpowers/specs/2026-07-12-pr4-runtime-derivation-cleanup-design.md`(§6.2 括注勘误)

- [ ] **Step 1: roadmap §4.6 状态行**(章节标题下追加)

```markdown
> ✅ **PR-4 已实施**(2026-07,分支 `refactor/pr4-runtime-derivation`,PR 号合并后回填)——`Config::runtime()`/`IRuntime`/`generate_file` 删除;`RuntimeState` 由 facade 持有(`SimpleStateManager`);产物走「候选 check → atomicwrites 晋升」(D5);变更类 profile IPC 返回 `RebuildOutcome`(committed/degraded,C-M2 后半)。详见 `docs/superpowers/specs/2026-07-12-pr4-runtime-derivation-cleanup-design.md`。
```

- [ ] **Step 2: roadmap §5 B8 行与收尾登记**

B8 行「删除条件」列改为:

```
PR-4 已清偿 runtime draft 写入;残余仅 `CoreManager` apply/check 桥(`client/core_bridge.rs`,PR-5)
```

§5 末尾(2026-07-11 收尾登记段后)追加:

```markdown
**2026-07-12 PR-4 收尾登记:** B8 的 runtime draft 写入桥已删除;`TODO/FIXME(actor-migration)` 台账 17 → 16。`RebuildOutcome` 降级模型为过渡语义,终态方向见 §6 新增行。
```

- [ ] **Step 3: roadmap §6 风险表追加行**

```markdown
| 降级模型(`RebuildOutcome`)为过渡语义 | TODO(post-PR-7):state 层异步 ack 就绪后,配置应用失败改走 ack 驱动 rollback,取代降级上报(用户决策 2026-07-11,PR-4 spec D2) |
```

- [ ] **Step 4: task.md §6 回填**(文末追加)

```markdown
**2026-07-12 用户决策与处置(PR-4 落地):**

- **C-M2(后半):** 已落地——变更类 profile IPC 返回 `RebuildOutcome`(committed/degraded);ack-based rollback 记为 post-PR-7 方向。见 `docs/superpowers/specs/2026-07-12-pr4-runtime-derivation-cleanup-design.md` §6.2。
- **C-M5:** 已落地——`run_core_inner` 的 `Config::clash().reload()` 删除(重启=应用当前 draft)。
- **C-M4(后半)勘误:** 端口生命周期编排改挂 **PR-5**(编排需控制核心启停时序,属 CoreActor 职责)。
```

同时把 :761「知悉不修」行内的「(端口生命周期编排 stop→resolve→mirror→start 与 legacy sysproxy/API 镜像回写时机,PR-4)」中的 `PR-4` 改为 `PR-5(2026-07-12 勘误)`。

- [ ] **Step 5: PR-4 spec §6.2 括注勘误**

spec 文件 §6.2 覆盖规则句中「(已知含 `enhance_profiles`、activate / save / patch / delete 触发重建的路径)」替换为:

```
(实施期枚举定稿:11 条 unit 命令 + `create_profile` 返回 `RebuildOutcome`,`import_profile` 返回 `CommitOutcome<ProfileId>`;`enhance_profiles` 无前置 commit、`save_profile_file` 不触发 rebuild,均不适用)
```

- [ ] **Step 6: 台账计数与验收判据 sweep**

Run(逐条,应全部满足):

```bash
rg -c "TODO\(actor-migration\)|FIXME\(actor-migration\)" backend/tauri/src   # 合计 = 16
rg -n "Config::runtime|IRuntime|generate_file|ConfigType" backend/tauri/src  # 零命中
cargo test --workspace --all-features                                        # 全绿
pnpm -F interface build && pnpm typecheck && pnpm web:build                  # 全绿
git checkout -- frontend/interface/src/ipc/bindings.ts 2>$null; git status   # 工作树干净(bindings 已在 T9 落账,test 重写后应无 diff)
```

若台账计数≠16,`rg -n` 列出并与 spec §7 表逐处对账后修正注释(不改行为)。

- [ ] **Step 7: Commit + push**

```bash
git add docs/design/actor-migration-roadmap.md "docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md" docs/superpowers/specs/2026-07-12-pr4-runtime-derivation-cleanup-design.md
git commit -m "docs: PR-4 ledger closeout — B8 draft bridge cleared, C-M4 retagged PR-5, ack-rollback direction noted"
git push https://github.com/libnyanpasu/clash-nyanpasu.git refactor/pr4-runtime-derivation
```

- [ ] **Step 8: 人工冒烟清单**(报告给用户,不阻塞)

1. 启动应用(真实数据)→ 核心运行、代理可用(boot 首铸 + 晋升)。
2. 切换 profile → 生效;编辑 clash mode/allow-lan → 生效且 `get_runtime_yaml` 反映。
3. 换核(mihomo ↔ clash-rs)→ 成功;人为造失败(改坏 core 二进制名)→ 回滚到旧核。
4. 断网后切 profile → UI 出现降级 warning toast,列表状态已切换。
