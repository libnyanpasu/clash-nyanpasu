# PR-4 Runtime 派生化收尾 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消灭可写 runtime 状态——`Config::runtime()`/`IRuntime`/`generate_file` 删除,facade 持有 `SimpleStateManager<Option<RuntimeState>>`,产物走「唯一候选 → check(显式 target core)→ atomicwrites 晋升 → 发布」管线,重建/换核/legacy 更新统一在 `rebuild_gate` 单次持有内完成,变更类 profile IPC 返回 `RebuildOutcome`(committed/degraded)。

**Architecture:** 方案 A(spec D4)+ 2026-07-12 审计修订 r2(spec §12):重建时一次派生只读 `RuntimeState`,**check 通过并晋升后才发布 manager**(P0-1),四条读 IPC wire 保形;`RunningCoreBridge` 四操作 `check_and_promote(candidate, target_core)` / `apply_config` / `restart_core` / `on_profile_change`(P0-3:target core 与 builder 同源显式传参);`change_core` 编排迁 facade,全事务 gate + 强回滚(P0-2/P0-4);legacy 桥升级为 `regenerate` / `regenerate_and_apply` / `regenerate_and_restart` 三组合操作(均在 gate 单次持有内完成);boot 兜底走同一管线(P0-5);`patch_clash_config` 保留 API-first + 失败补偿回推(P0-6,D6)。降级模型只覆盖 facade 的 post-commit 内联 rebuild 路径(唯一收口点 `after_commit`),legacy 桥调用方维持 Err/draft-discard。

**Tech Stack:** Rust(tauri crate + nyanpasu-core SimpleStateManager + atomicwrites + mockall)、tauri-specta 绑定、React/TS(react-query MutationCache、paraglide i18n)。

**Spec:** `docs/superpowers/specs/2026-07-12-pr4-runtime-derivation-cleanup-design.md`(已批准;r2 审计修订版,处置记录见其 §12)。

## Global Constraints

- 分支 `refactor/pr4-runtime-derivation`(已存在,含 spec commit;**尚未推送远端**);实施前先执行 Task 0 rebase 到 main @ `fbb72905b`;worktree 隔离按 CLAUDE.md §17 可选。
- **Rust toolchain(r2 勘误)**:仓库 `rust-toolchain.toml` 为浮动 `channel = "nightly"`,**未钉日期**——plan 不得假设已 pin。若出现 ICE/LNK1120,按项目记忆在环境侧处理(nightly-2026-05-27 override、按符号名定点 `cargo clean -p <crate>`,kache 污染),不随本 PR 入库。
- **pre-commit 对 backend 文件跑全量 clippy(3–8 分钟)**:所有含 `.rs` 的 `git commit` 必须后台运行(Bash `run_in_background`),不可用默认 2 分钟超时等待。
- **bindings.ts 提交时序(铁律 3 / T09 先例)**:`cargo test` 会运行 `export_typescript_bindings` 就地重写 `frontend/interface/src/ipc/bindings.ts`。Task 1–8 的每次 backend commit 前执行 `git checkout -- frontend/interface/src/ipc/bindings.ts`;bindings 只随 Task 9 与前端适配同 commit 落账;**Task 10 终验用 `git diff --exit-code` 判定零漂移,不得再 checkout 掩盖**(审计 §四.5)。
- push 走 HTTPS:`git push https://github.com/libnyanpasu/clash-nyanpasu.git refactor/pr4-runtime-derivation`(origin SSH 已死)。
- 四条读命令(`get_runtime_config/yaml/exists/postprocessing_output`)wire 形状不得变化;`enhance_profiles`、`save_profile_file` 返回类型不变(无前置 commit,不套降级模型——此为对 spec §6.2 括注的枚举期修正,规则「post-commit 内联 rebuild 才降级」优先)。
- i18n 覆盖 **5** 语言:en / zh-cn / zh-tw / ru / **ko**(rebase 后 main 已含韩语 locale,审计 §六.1)。
- 测试统一 `cargo test --workspace --all-features <filter>`;**`cargo test` 只接受一个位置过滤参数,多个过滤器必须拆成多条命令**(审计 §四.2);全量验证在 Task 10。

---

### Task 0: rebase 到当前 main + 基线复核(审计 §四.6)

**Files:** 无代码变更;分支操作。

- [ ] **Step 1: rebase**

```bash
git fetch https://github.com/libnyanpasu/clash-nyanpasu.git main
git rebase FETCH_HEAD
```

Expected: spec/plan 两个 docs commit 干净重放(与 #4923 韩语 / #4928 i18n 同步仅 locale 面交集,预期无冲突;若冲突以文档侧内容为准解决)。

- [ ] **Step 2: 基线断言**

```bash
git merge-base --is-ancestor dcceed54f HEAD && echo ko-locale-present
ls frontend/nyanpasu/messages/ko.json
cat rust-toolchain.toml
```

Expected: ko.json 存在;toolchain 仅 `channel = "nightly"`(浮动)。

- [ ] **Step 3: 基线测试绿**

Run: `cargo test --workspace --all-features`(后台运行,3–8 分钟)
Expected: 全绿;随后 `git checkout -- frontend/interface/src/ipc/bindings.ts` 还原 test 重写。

- [ ] **Step 4: 推送 rebase 后的分支**

```bash
git push https://github.com/libnyanpasu/clash-nyanpasu.git refactor/pr4-runtime-derivation
```

(远端尚无此分支,首推无需 force;若已存在则 `--force-with-lease`。)

---

### Task 1: `RuntimeState` 类型与 artifact 映射

**Files:**

- Create: `backend/tauri/src/client/runtime.rs`
- Modify: `backend/tauri/src/client/mod.rs`(注册模块)
- Modify: `backend/tauri/src/enhance/artifact_bridge.rs`
- Modify: `backend/tauri/src/enhance/mod.rs`(导出)

**Interfaces:**

- Produces: `crate::client::runtime::{RuntimeState, runtime_config_path() -> anyhow::Result<PathBuf>, candidate_config_path() -> PathBuf, RUNTIME_CONFIG_DIR, RUNTIME_CONFIG}`(候选文件唯一命名,无固定常量——r2);`crate::enhance::runtime_state_from_artifact(&RuntimeArtifact, &Profiles, ClashCore, bool) -> anyhow::Result<RuntimeState>`。
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

/// Read model of the current runtime derivation (former `IRuntime`, minus the
/// draft machinery). Derived once per rebuild while the profiles snapshot is
/// in hand; the four runtime read commands serve straight from this.
///
/// Semantics (spec §5.1, r2): the latest TARGET config that passed the core
/// binary's check and was promoted to the product. It does NOT promise the
/// running core accepted it — a failed apply is reported as
/// `RebuildOutcome::Degraded`, not reflected here.
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
/// Unique per attempt (spec §5.2, r2): a fixed temp path is a TOCTOU /
/// multi-instance / parallel-test clobber hazard. The pipeline best-effort
/// deletes the candidate after `check_and_promote`.
pub fn candidate_config_path() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "clash-nyanpasu-candidate-{}-{seq}.yaml",
        std::process::id()
    ))
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

Run: `cargo test --workspace --all-features facade_add_activate_rebuilds_via_core_bridge`
(审计 §四.1:该测试实名如此;`runtime_draft` 过滤器匹配零测试并静默通过,禁止使用)
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

> **过渡注记(P0-1)**:本任务的 upsert 暂时落在旧 draft 写入位置(check 管线尚未存在,check 仍在 `apply_config` 内,与今日 draft 语义一致);**Task 4 引入候选→check→晋升管线时,upsert 与 legacy 双写一并移至 `check_and_promote` 成功之后**——终态严禁「先发布后 check」。

- [ ] **Step 6: 测试通过**

Run: `cargo test --workspace --all-features facade_add_activate_rebuilds_via_core_bridge`(同 Step 3 过滤)
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

- [ ] **Step 2b: seeded 态断言补强**(审计:不只测空态)

在 `facade_add_activate_rebuilds_via_core_bridge` 的 manager 断言(Task 2 Step 3 追加处)再追加,钉住四读命令的三个数据面在重建后均有真值:

```rust
assert!(
    !state.exists_keys.is_empty(),
    "guard overrides must register applied fields"
);
let _ = state.postprocessing_output.clone(); // postprocessing 面可达(无脚本 profile 时为 default)
```

(若 `exists_keys` 对最小 File profile 的实测为空,以实测行为修正断言——目的是钉住 seeded 三字段面,不是猜测 executor 语义。)

Run: `cargo test --workspace --all-features facade_add_activate_rebuilds_via_core_bridge`
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

### Task 4: 桥拆分 + 候选→check→晋升→发布管线(P0-1/P0-3/P0-5)

**Files:**

- Modify: `backend/tauri/src/client/core_bridge.rs`(trait + impl + promote/restore helper + 单测)
- Modify: `backend/tauri/src/core/clash/core.rs`(`check_config(path, core)` 收窄、`apply_config` 只 put、`Instance::try_new` 只读产物路径、`change_core` 的 check 调用临时适配)
- Modify: `backend/tauri/src/client/mod.rs`(`regenerate_runtime_with` 尾部接管线并**后移发布**;测试 helper 提取;mock 期望)
- Modify: `backend/tauri/src/client/rebuild.rs`(boot 兜底 `promote_default_runtime_config`)
- Modify: `backend/tauri/src/utils/resolve.rs:175-193`(boot 段)

**Interfaces:**

- Consumes: Task 1 `runtime_config_path()/candidate_config_path()`。
- Produces: `RunningCoreBridge::{check_and_promote(&self, candidate: &camino::Utf8Path, target_core: ClashCore) -> anyhow::Result<()>, restart_core(&self) -> anyhow::Result<()>}`(trait 新方法,`restart_core` 由 Task 5 消费;`apply_config`/`on_profile_change` 签名不变);`CoreManager::check_config(&self, config_path: &camino::Utf8Path, clash_core: ClashCore) -> Result<()>`(**不再内部读 `Config::verge()` 选核**,P0-3);`NyanpasuClient::promote_default_runtime_config()`。
- trait 中 `ClashCore` 用 typed `nyanpasu_config::application::ClashCore`(facade 词汇);`LegacyCoreBridge` impl 内转换到 legacy 枚举/CoreType——先 `rg -n "application::ClashCore" backend/tauri/src/bridge` 确认既有映射,无则在 impl 内 match 一一对应。

- [ ] **Step 1: 失败测试——promote helper、调用顺序、check 失败不发布**

先在 `client/mod.rs` tests 把 `facade_add_activate_rebuilds_via_core_bridge`(:1101-1121)内联的 `NewProfileRequest` 字面量提取为 helper 并复用:

```rust
    fn minimal_file_profile_request() -> NewProfileRequest {
        NewProfileRequest {
            metadata: ProfileMetadata {
                name: "t".into(),
                desc: None,
            },
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
        }
    }
```

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

`client/mod.rs` tests 增两测试(完整代码,审计 §四.7——不留占位):

```rust
    #[test]
    fn rebuild_checks_and_promotes_before_core_apply() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_apply_config()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        tauri::async_runtime::block_on(async {
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add");
            client.activate_profile(Some(uid)).await.expect("activate");
        });
    }

    /// D5+P0-1 不变式:check 失败 → manager 不发布(产物不变由
    /// LegacyCoreBridge 的顺序代码 + promote 原子性单测保证)。
    #[test]
    fn failed_check_keeps_runtime_state_unpublished() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .returning(|_, _| Err(anyhow::anyhow!("check boom")));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        tauri::async_runtime::block_on(async {
            let uid = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add");
            // T4 时点 rebuild 失败仍向上冒 Err(T8 才引入降级模型)
            let _ = client.activate_profile(Some(uid)).await;
            assert!(
                client.runtime_state().await.as_ref().is_none(),
                "a rejected candidate must never be published to readers"
            );
        });
    }
```

(若 harness 实名非 `test_profiles_client_args`,按 :909 一带实名替换。)

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
use nyanpasu_config::application::ClashCore;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait RunningCoreBridge: Send + Sync + 'static {
    /// Check the candidate config with the EXPLICIT target core's binary, then
    /// atomically promote it to the runtime product (spec D5: the product
    /// only ever holds checked configs). `target_core` must come from the same
    /// input snapshot the builder used — implementations must not re-read
    /// global state to pick the core (spec §5.3, P0-3). Usable on the boot
    /// path where the core is not running yet.
    async fn check_and_promote(
        &self,
        candidate: &Utf8Path,
        target_core: ClashCore,
    ) -> anyhow::Result<()>;
    /// Push the promoted product to the running core over its api.
    async fn apply_config(&self) -> anyhow::Result<()>;
    /// Restart the core off the current promoted product (consumed by the
    /// facade change_core / regenerate_and_restart transactions, spec §5.4/5.5).
    async fn restart_core(&self) -> anyhow::Result<()>;
    async fn on_profile_change(&self);
}

/// Atomic candidate -> product replacement (atomicwrites: temp file + durable
/// rename; readers never observe a half-written product).
pub(crate) async fn promote_candidate(candidate: &Path, product: &Path) -> anyhow::Result<()> {
    let bytes = tokio::fs::read(candidate).await?;
    restore_product(product, &bytes).await
}

/// Atomically write known-good product bytes back (change_core last-resort
/// rollback, spec §5.4). Shared with promote_candidate.
pub(crate) async fn restore_product(product: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = product.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let product = product.to_path_buf();
    let bytes = bytes.to_vec();
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
    async fn check_and_promote(
        &self,
        candidate: &Utf8Path,
        target_core: ClashCore,
    ) -> anyhow::Result<()> {
        // TODO(actor-migration): temporary bridge to CoreManager::global().
        // Reason: core lifecycle is PR-5 (CoreActor).
        // Remove when: PR-5 lands CoreActor and the facade owns core apply.
        crate::core::CoreManager::global()
            .check_config(candidate, target_core.into()) // typed -> legacy 映射,见 Interfaces 注
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

    async fn restart_core(&self) -> anyhow::Result<()> {
        // TODO(actor-migration): temporary bridge to CoreManager::global().
        // Reason: core lifecycle is PR-5 (CoreActor).
        // Remove when: PR-5 lands CoreActor and the facade owns core restart.
        crate::core::CoreManager::global().run_core().await
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

(注意:`on_profile_change` 的 TODO 由「PR-4/PR-6」改为「PR-6」——spec §7 勘误项。typed→legacy `ClashCore` 若无既有 `From` 映射,在本文件加一个私有转换函数。)

- [ ] **Step 3: `CoreManager` 侧收窄**

`core.rs:424-447` `check_config` 改签名(删 generate 两行、改参数;**core 显式传入,不再读 `Config::verge()` 隐式选核**——P0-3,选核责任上移到与 builder 同源的调用方):

```rust
    pub async fn check_config(&self, config_path: &Utf8Path, clash_core: ClashCore) -> Result<()> {
        use nyanpasu_utils::core::instance::CoreInstance;

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

`change_core`(:564)的 `self.check_config().await` 调用暂改为传产物路径 + draft 核(Task 5 将整个函数删除、编排迁 facade):

```rust
        let product = camino::Utf8PathBuf::from_path_buf(
            crate::client::runtime::runtime_config_path()?,
        )
        .map_err(|_| anyhow::anyhow!("failed to convert config path to utf8 path"))?;
        let draft_core = Config::verge().latest().clash_core.unwrap_or_default();
        if let Err(err) = self.check_config(&product, draft_core).await {
```

- [ ] **Step 4: facade 管线尾部(check→晋升→**发布**,P0-1)**

`regenerate_runtime_with` 重排:spawn_blocking 闭包同步多算一个 yaml 串,返回三元组 `(state, legacy_runtime, yaml)`:

```rust
                let yaml = format!(
                    "# Generated by Clash Nyanpasu\n\n{}",
                    serde_yaml::to_string(&state.config)?
                );
```

(闭包内、构造 legacy_runtime 之后)然后**把 Task 2 放置的 manager upsert 与 legacy 双写整体移到 check 之后**,管线尾部为:

```rust
        // Candidate -> check -> promote -> PUBLISH (spec §5.2, P0-1): readers
        // only ever see checked-and-promoted configs; a rejected candidate
        // leaves both the product and the manager untouched. target core =
        // the same input snapshot the builder used (P0-3).
        let candidate = crate::client::runtime::candidate_config_path();
        tokio::fs::write(&candidate, yaml)
            .await
            .map_err(|error| ClientError::Custom(format!("failed to write candidate: {error}")))?;
        let candidate = utf8_path(candidate).map_err(ClientError::Anyhow)?;
        let checked = self.inner.core.check_and_promote(&candidate, core).await;
        let _ = tokio::fs::remove_file(candidate.as_std_path()).await; // best-effort 清理,成功失败都执行
        checked.map_err(ClientError::Anyhow)?;
        {
            let mut store = self.inner.runtime.write().await;
            store
                .upsert(Some(state))
                .await
                .map_err(|error| ClientError::Custom(format!("failed to store runtime state: {error}")))?;
        }
        // TODO(actor-migration): temporary dual-write to Config::runtime() draft (B8).
        // Reason: legacy readers migrate in PR-4 Tasks 3-6. Remove in PR-4 Task 7.
        *crate::config::Config::runtime().draft() = legacy_runtime;
        Ok(())
```

(`core` 为 :698 的 `let core = app.core;`——typed `ClashCore` 为 `Copy` 枚举,闭包 move 后仍可用;若非 `Copy` 则先 clone。upsert 失败语义 = spec §5.2:产物权威、manager 下次重建自愈、本次返回 Err。)

- [ ] **Step 5: boot 段(resolve.rs:175-193)——兜底走管线(P0-5,spec §5.6)**

`client/rebuild.rs` legacy impl 区新增:

```rust
    /// Boot fallback (spec §5.6, D8): the default config is ALSO routed through
    /// candidate -> check -> promote — D5 has no exceptions. A failed check
    /// leaves no product; boot continues and the core start fails visibly.
    pub(crate) async fn promote_default_runtime_config(&self) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        // TODO(actor-migration): boot fallback reads the legacy clash mapping
        // directly (same source the old resolve.rs fallback used).
        // Remove when: PR-6 migrates boot/resolve onto typed clients.
        let mapping = crate::config::Config::clash().latest().0.clone();
        let (app, _clash) = Self::legacy_regen_inputs()?;
        let yaml = format!(
            "# Clash Nyanpasu Runtime (default fallback)\n\n{}",
            serde_yaml::to_string(&mapping)
                .map_err(|error| ClientError::Custom(format!("serialize default: {error}")))?
        );
        let candidate = crate::client::runtime::candidate_config_path();
        tokio::fs::write(&candidate, yaml)
            .await
            .map_err(|error| ClientError::Custom(format!("failed to write candidate: {error}")))?;
        let candidate = super::utf8_path(candidate).map_err(ClientError::Anyhow)?;
        let checked = self.inner.core.check_and_promote(&candidate, app.core).await;
        let _ = tokio::fs::remove_file(candidate.as_std_path()).await;
        checked.map_err(ClientError::Anyhow)
    }
```

`resolve.rs:175-193` 改为:

```rust
        // 启动首铸:profiles/clash/app 快照 → RuntimeBuilder → 候选 check → 晋升产物 → 发布
        log::trace!("init config");
        log_err!(tauri::async_runtime::block_on(client.regenerate_runtime()));
        // 兜底(spec §5.6):产物缺失时默认配置也必须过 check——check 失败则不落
        // 未检产物(P0-5),boot 继续,核心启动失败可见。
        let runtime_path = crate::client::runtime::runtime_config_path()
            .expect("failed to resolve runtime config path");
        if !runtime_path.exists() {
            log_err!(tauri::async_runtime::block_on(
                client.promote_default_runtime_config()
            ));
        }
```

(`help::save_yaml` 兜底删除;`help` import 若因此孤立则一并移除。)

- [ ] **Step 6: 修既有 mock 期望**

Run: `rg -n "expect_apply_config" backend/tauri/src`
每个设置 `expect_apply_config` 的测试(rebuild 会先 check_and_promote)前面补:

```rust
        core.expect_check_and_promote().returning(|_, _| Ok(()));
```

- [ ] **Step 7: 测试通过**(`cargo test` 单过滤参数,逐条执行——审计 §四.2)

```bash
cargo test --workspace --all-features promote_candidate_atomically_replaces_product
cargo test --workspace --all-features rebuild_checks_and_promotes_before_core_apply
cargo test --workspace --all-features failed_check_keeps_runtime_state_unpublished
cargo test --workspace --all-features client::
```

Expected: PASS

- [ ] **Step 8: Commit**

```bash
git checkout -- frontend/interface/src/ipc/bindings.ts
git add backend/tauri/src/client/core_bridge.rs backend/tauri/src/client/mod.rs backend/tauri/src/client/rebuild.rs backend/tauri/src/core/clash/core.rs backend/tauri/src/utils/resolve.rs
git commit -m "refactor(tauri): candidate->check->promote->publish runtime pipeline behind split core bridge (PR-4 T4)"
```

---

### Task 5: gate 统一——facade `change_core`(强回滚)+ 组合桥操作 + C-M5(P0-2/P0-4)

**Files:**

- Modify: `backend/tauri/src/client/rebuild.rs`(`RegenKind` 枚举、三 pub 入口、facade 组合方法、`change_core`、测试)
- Modify: `backend/tauri/src/client/mod.rs`(桥安装点 dispatch;`rg -n "install_regen_bridge" backend/tauri/src` 定位)
- Modify: `backend/tauri/src/core/clash/core.rs`(**删除** `change_core` 全函数、`update_config` 改组合桥、`run_core_inner` reload 删除)
- Modify: `backend/tauri/src/feat.rs`(`patch_verge` 的 :328-329 / :352-353 两对「regenerate + run_core」;patch_clash 的 :268-269 留给 Task 6 一并收编)
- Modify: `backend/tauri/src/ipc.rs`(`change_clash_core` 改调 client)

**Interfaces:**

- Consumes: Task 4 的管线与 `RunningCoreBridge::restart_core` / `restore_product`。
- Produces: `rebuild::{regenerate(), regenerate_and_apply(), regenerate_and_restart()}`(桥三入口);`NyanpasuClient::change_core(nyanpasu::ClashCore) -> Result<()>`;`NyanpasuClient::{regenerate_and_apply_for_legacy, regenerate_and_restart_for_legacy}`。
- 后置条件:**所有 apply/restart 都与其 regenerate 在同一次 `rebuild_gate` 持有内完成**(P0-2 判据);`CoreManager::change_core` 与 `Config::runtime()` 在 core.rs 零残留。

- [ ] **Step 1: 桥 request 枚举 + 三入口**(`rebuild.rs`)

`RegenRequest` 改为携带类型:

```rust
pub(super) enum RegenKind {
    /// 仅重建(build→check→晋升→发布)。
    Regenerate,
    /// 重建 + put_configs,gate 单次持有内完成(P0-2:消灭「gate 内 regen、
    /// gate 外 apply」的产物覆盖窗口)。
    RegenerateAndApply,
    /// 重建 + 重启核心,gate 单次持有内完成(P0-2)。
    RegenerateAndRestart,
}
struct RegenRequest {
    kind: RegenKind,
    reply: oneshot::Sender<anyhow::Result<()>>,
}
```

`install_regen_bridge` 的 handler 闭包签名改为 `Fn(RegenKind) -> Fut`;安装点(client/mod.rs)dispatch:

```rust
        RegenKind::Regenerate => client.regenerate_runtime_for_legacy().await,
        RegenKind::RegenerateAndApply => client.regenerate_and_apply_for_legacy().await,
        RegenKind::RegenerateAndRestart => client.regenerate_and_restart_for_legacy().await,
```

pub 入口 `regenerate()` 保形不变;新增 `regenerate_and_apply()` / `regenerate_and_restart()`(同 oneshot 模式,发对应 kind)。

- [ ] **Step 2: facade 组合方法**(`rebuild.rs` legacy impl 区)

`regenerate_runtime_for_legacy` 拆为 gated wrapper + `regenerate_for_legacy_inner`(无 gate,供组合方法在自己的 gate 持有内复用),然后:

```rust
    pub(crate) async fn regenerate_and_apply_for_legacy(&self) -> Result<()> {
        // P0-2: one gate hold covers regenerate AND apply — a concurrent rebuild
        // cannot replace the product between the two steps.
        let _rebuild = self.inner.rebuild_gate.lock().await;
        self.regenerate_for_legacy_inner().await?;
        self.inner.core.apply_config().await.map_err(ClientError::Anyhow)
    }

    pub(crate) async fn regenerate_and_restart_for_legacy(&self) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;
        self.regenerate_for_legacy_inner().await?;
        self.inner.core.restart_core().await.map_err(ClientError::Anyhow)
    }
```

(锁序不变式:先 `rebuild_gate` 后 CoreManager `run_lock`,全仓无反向嵌套——`run_core`/`recover_core` 只拿 `run_lock`,不碰 gate。)

- [ ] **Step 3: facade `change_core`**(`rebuild.rs` legacy impl 区;spec §5.4 全事务 gate + 强回滚)

```rust
    /// Core-switch transaction (spec §5.4). The WHOLE draft→rebuild→restart→
    /// commit/rollback sequence holds the rebuild gate, so no concurrent
    /// rebuild can replace the checked product between check and start (P0-2).
    pub async fn change_core(&self, new_core: crate::config::nyanpasu::ClashCore) -> Result<()> {
        let _rebuild = self.inner.rebuild_gate.lock().await;

        // Last-resort rollback material: the previous product passed its own
        // check when it was promoted, so restoring its bytes needs no re-check.
        let product = crate::client::runtime::runtime_config_path().map_err(ClientError::Anyhow)?;
        let old_product = tokio::fs::read(&product).await.ok();

        // TODO(actor-migration): core selection still drafts the legacy verge.
        // Reason: verge feature flows migrate in PR-5/6.
        // Remove when: core selection patches the typed app config.
        crate::config::Config::verge().draft().clash_core = Some(new_core);

        if let Err(error) = self.regenerate_for_legacy_inner().await {
            crate::config::Config::verge().discard();
            return Err(error); // 产物 / manager 零变化(P0-1 管线保证)
        }

        // TODO(actor-migration): legacy log sink clear on core switch (C7).
        // Remove when: PR-5 injects the LogSink into CoreActor.
        crate::core::logger::Logger::global().clear_log();

        match self.inner.core.restart_core().await {
            Ok(()) => {
                crate::config::Config::verge().apply();
                if let Err(error) = crate::config::Config::verge().latest().save_file() {
                    tracing::error!(%error, "failed to persist verge after core switch");
                }
                Ok(())
            }
            Err(new_core_error) => {
                tracing::error!("failed to change core: {new_core_error:?}");
                crate::config::Config::verge().discard();
                // Rollback = rebuild from committed state. A rollback failure is
                // NEVER swallowed (P0-4): degrade to restoring the previous
                // checked product bytes; the old core must not start on a
                // product built for the new core.
                if let Err(rebuild_error) = self.regenerate_for_legacy_inner().await {
                    let restored: anyhow::Result<()> = match &old_product {
                        Some(bytes) => {
                            crate::client::core_bridge::restore_product(&product, bytes).await
                        }
                        None => tokio::fs::remove_file(&product)
                            .await
                            .map_err(|e| anyhow::anyhow!(e)),
                    };
                    // 注:此分支下 manager 仍持有新核 RuntimeState(发布已随新核
                    // regenerate 完成)——按 spec §5.2 语义:产物权威,下次成功重建自愈。
                    if let Err(restore_error) = restored {
                        return Err(ClientError::Anyhow(
                            new_core_error
                                .context(format!("rollback rebuild failed: {rebuild_error}"))
                                .context(format!(
                                    "product restore failed: {restore_error}; core left stopped"
                                )),
                        ));
                    }
                    if let Err(restart_error) = self.inner.core.restart_core().await {
                        return Err(ClientError::Anyhow(
                            new_core_error
                                .context(format!("rollback rebuild failed: {rebuild_error}"))
                                .context(format!("old core restart failed: {restart_error}")),
                        ));
                    }
                    return Err(ClientError::Anyhow(
                        new_core_error.context(format!(
                            "rollback rebuild failed: {rebuild_error}; restored previous product"
                        )),
                    ));
                }
                if let Err(restart_error) = self.inner.core.restart_core().await {
                    return Err(ClientError::Anyhow(new_core_error.context(format!(
                        "old core restart failed after rollback: {restart_error}"
                    ))));
                }
                Err(ClientError::Anyhow(new_core_error))
            }
        }
    }
```

(复合错误用 anyhow `context` 链——spec §12 决策,不新增 `ChangeCoreError` 结构。)

- [ ] **Step 4: core.rs 三处**

1. **删除** `change_core` 全函数(:548-589,含 Task 4 的临时 check 适配与三处 `Config::runtime()` discard/apply)。
2. `update_config`(:593-603)改为:

```rust
    pub async fn update_config(&self) -> Result<()> {
        log::debug!(target: "app", "try to update clash config");
        // FIXME(actor-migration): legacy regenerate path. Sole remaining caller
        // chain: feat::patch_verge (TUN/service toggles) -> update_core_config
        // -> update_config. New code must use
        // NyanpasuClient::rebuild_running_config(). Remove when PR-5 migrates
        // the verge feature flows onto injected clients.
        // P0-2: regenerate+apply 在桥另一侧的单次 gate 持有内一体完成。
        crate::client::rebuild::regenerate_and_apply().await
    }
```

3. C-M5:删除 `run_core_inner` 的 :464-466 三行:

```rust
        // Reload clash config from file to get latest user preferences (e.g., mode)
        Config::clash().reload();
        log::debug!(target: "app", "reloaded clash config from file");
```

- [ ] **Step 5: feat.rs 两对成对调用改组合桥**

`patch_verge` service 分支(:328-329)与 tun 分支(:352-353)的

```rust
            crate::client::rebuild::regenerate().await?;
            CoreManager::global().run_core().await?;
```

均改为:

```rust
            crate::client::rebuild::regenerate_and_restart().await?;
```

(patch_clash 的 :268-269 由 Task 6 随激活段重写一并收编。)

- [ ] **Step 6: ipc.rs `change_clash_core` 改调 client**

```rust
#[tauri::command]
#[specta::specta]
pub async fn change_clash_core(
    client: State<'_, NyanpasuClient>,
    legacy: State<'_, LegacyVergeBridge>,
    clash_core: Option<nyanpasu::ClashCore>,
) -> Result {
    let clash_core =
        clash_core.ok_or_else(|| IpcError::Custom("clash core is null".to_string()))?;
    // reseed wrapper 语义不变:核心切换动了 legacy verge,须回灌 typed actors。
    let client = client.inner().clone();
    legacy
        .run_legacy_verge_mutation(move || async move {
            client.change_core(clash_core).await.map_err(Into::into)
        })
        .await?;
    Ok(())
}
```

(`NyanpasuClient` 为 Arc-inner 的 Clone facade;若实际非 `Clone`,以借用捕获重写闭包。)

- [ ] **Step 7: 测试**(完整代码;P0-4 钉)

`rebuild.rs` tests 追加(mock 次序钉回滚链;测试内 verge draft 最终被 discard/apply 复原,但仍属进程级全局写——保持该测试自包含、不与其它读 verge 的测试共享断言):

```rust
    #[test]
    fn change_core_rolls_back_via_second_regenerate_and_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut core = crate::client::core_bridge::MockRunningCoreBridge::new();
        let mut seq = mockall::Sequence::new();
        // 新核:check+晋升成功 → 启动失败
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Err(anyhow::anyhow!("new core boom")));
        // 回滚:旧核 check+晋升成功 → 旧核启动成功
        core.expect_check_and_promote()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|_, _| Ok(()));
        core.expect_restart_core()
            .times(1)
            .in_sequence(&mut seq)
            .returning(|| Ok(()));
        core.expect_on_profile_change().returning(|| ());
        let client = crate::client::NyanpasuClient::try_new_with_args(
            crate::client::tests::test_profiles_client_args(&dir, std::sync::Arc::new(core)),
        )
        .unwrap();
        let result = tauri::async_runtime::block_on(
            client.change_core(crate::config::nyanpasu::ClashCore::ClashRs),
        );
        assert!(result.is_err(), "change_core must surface the new-core error");
    }
```

(`test_profiles_client_args` 若为私有,提 `pub(crate)` 或把测试放入 `client/mod.rs` tests。)

**回滚重建也失败**的分支:`restore_product` 已有 tempdir 单测(Task 4);`change_core` 端产物路径经 `dirs::app_config_dir()` 取全局目录——实施时 `rg -n "app_config_dir" backend/tauri/src/utils/dirs.rs` 确认测试可注入性(env override):可注入 → 补「第二次 check 失败 → 产物字节被恢复 + 返回复合错误」集成测试;不可注入 → 该分支由 `restore_product` 单测 + mock 次序断言(第二次 check Err 后不得再有 restart 成功路径的 verge apply)+ 审查覆盖,并在 commit message 注明。

Run(逐条):

```bash
cargo test --workspace --all-features change_core_rolls_back_via_second_regenerate_and_restart
cargo test --workspace --all-features client::
```

Expected: PASS

- [ ] **Step 8: FIXME 清偿期改写(spec §7)**

`backend/tauri/src/client/rebuild.rs` 两处:`install_regen_bridge` 上方 FIXME 的 `Remove after PR-4/PR-5 migrate those flows onto injected clients.` 改为 `Remove after PR-5/PR-6 migrate those flows onto injected clients.`;`legacy_regen_inputs` FIXME 的 `Remove when: PR-4/5/6 migrate the legacy writers onto typed clients.` 改为 `Remove when: PR-5/6 migrate the legacy writers onto typed clients.`

- [ ] **Step 9: 编译钉 + 残留检查**

```bash
cargo check --workspace --all-targets --all-features
rg -n "Config::runtime" backend/tauri/src/core/
rg -n "fn change_core" backend/tauri/src/core/
```

Expected: 编译绿;两个 rg 均零命中

- [ ] **Step 10: Commit**(审计 §四.3:本任务全部触碰文件逐一 stage)

```bash
git checkout -- frontend/interface/src/ipc/bindings.ts
git add backend/tauri/src/core/clash/core.rs backend/tauri/src/feat.rs backend/tauri/src/ipc.rs backend/tauri/src/client/rebuild.rs backend/tauri/src/client/mod.rs backend/tauri/src/client/core_bridge.rs
git commit -m "refactor(tauri)!: unify rebuild/apply/change-core under one gate hold, facade-owned core switch with hard rollback, drop C-M5 reload (PR-4 T5)"
```

---

### Task 6: `feat::patch_clash` 恒重建 + IPC 失败补偿(D6/P0-6)

**Files:**

- Modify: `backend/tauri/src/feat.rs:221-298`
- Modify: `backend/tauri/src/ipc.rs:379-396`(`patch_clash_config` 补偿)
- Modify: `backend/tauri/src/client/runtime.rs`(`compensation_for` 纯函数 + 单测)

**Interfaces:**

- Produces: `pub(crate) fn requires_core_restart(patch: &Mapping) -> bool`(feat.rs);`client::runtime::compensation_for(patch: &Mapping, prev: Option<&Mapping>) -> Option<Mapping>`。
- 前置事实(审计核实):今日 IPC 顺序为「`api::patch_configs` 直推运行核 → `feat::patch_clash` 持久化」;rebuild 失败时 clash draft 被 discard 而**运行核不回滚**——本任务在删除 runtime 内存 patch 的同时补上失败补偿。

- [ ] **Step 1: 失败测试 ×2**

feat.rs(文件尾新建 `#[cfg(test)] mod tests`):

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

`client/runtime.rs` tests:

```rust
    #[test]
    fn compensation_restores_previous_values_of_patched_keys() {
        let mut prev = Mapping::new();
        prev.insert("mode".into(), "rule".into());
        prev.insert("allow-lan".into(), false.into());
        let mut patch = Mapping::new();
        patch.insert("mode".into(), "direct".into());
        patch.insert("ipv6".into(), true.into()); // prev 无该键 → 略过
        let comp = compensation_for(&patch, Some(&prev)).expect("some");
        assert_eq!(comp.get("mode"), Some(&"rule".into()));
        assert!(comp.get("ipv6").is_none());
        assert!(compensation_for(&patch, None).is_none());
    }
```

Run(逐条):

```bash
cargo test --workspace --all-features core_restart_only_for_port_controller_secret
cargo test --workspace --all-features compensation_restores_previous_values_of_patched_keys
```

Expected: FAIL — 两函数均不存在

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

`patch_clash` 的「激活配置」段(:263-271)替换为(重启对 = gate 内一体,P0-2,收编 Task 5 预留的 :268-269):

```rust
        // 激活配置:任何 clash patch 都会进入 rebuild 输入,恒重建派生配置;
        // 仅端口/控制器/密钥变更需要重启核心(即时性由 IPC 层 api::patch_configs
        // 直推保证,失败补偿见 ipc::patch_clash_config,D6)。
        if requires_core_restart(&patch) {
            crate::client::rebuild::regenerate_and_restart().await?;
            handle::Handle::refresh_clash();
        } else {
            crate::client::rebuild::regenerate().await?;
        }
```

删除 :283 行 `Config::runtime().latest().patch_config(patch);`。

- [ ] **Step 3: `compensation_for` + IPC 补偿**

`client/runtime.rs` 增:

```rust
/// D6 (spec §6.4): previous values of the keys a clash patch touches, taken
/// from the published runtime state. Used to push the running core BACK when
/// the post-patch rebuild fails — the IPC applies the patch API-first, so a
/// failed rebuild would otherwise leave the core ahead of the persisted state.
pub(crate) fn compensation_for(patch: &Mapping, prev: Option<&Mapping>) -> Option<Mapping> {
    let prev = prev?;
    let comp: Mapping = patch
        .iter()
        .filter_map(|(k, _)| prev.get(k).map(|v| (k.clone(), v.clone())))
        .collect();
    (!comp.is_empty()).then_some(comp)
}
```

`ipc.rs` `patch_clash_config`(:379-396)改为:

```rust
#[tauri::command]
#[specta::specta]
pub async fn patch_clash_config(
    client: State<'_, NyanpasuClient>,
    payload: PatchRuntimeConfig,
) -> Result {
    tracing::debug!("patch_clash_config: {payload:?}");

    let mapping = match serde_yaml::to_value(&payload)? {
        serde_yaml::Value::Mapping(m) => m,
        _ => return Err(IpcError::Custom("Expected a mapping".to_string())),
    };

    // D6 补偿快照:manager 为 None(核心尚未构建/运行)→ 无补偿,直推本也会失败。
    let prev = client.runtime_state().await;
    let compensation = crate::client::runtime::compensation_for(
        &mapping,
        prev.as_ref().as_ref().map(|state| &state.config),
    );

    (crate::core::clash::api::patch_configs(&mapping).await)?;

    if let Err(e) = feat::patch_clash(mapping).await {
        tracing::error!("{e}");
        // API-first 已改运行核;rebuild/check 失败时尽力回推旧值(spec §6.4),
        // 避免「运行核=新值、持久态/产物=旧值」的永久分裂(P0-6)。
        if let Some(comp) = compensation {
            if let Err(comp_err) = crate::core::clash::api::patch_configs(&comp).await {
                tracing::error!("compensation patch failed: {comp_err:?}");
            }
        }
        return Err(IpcError::from(e));
    }

    feat::update_proxies_buff(None);
    Ok(())
}
```

> 覆盖说明:补偿路径的端到端验证依赖运行核(api 直推),无法纯单测——由 `compensation_for` 单测 + Task 10 冒烟清单的「坏 profile 下切 mode」场景覆盖。

- [ ] **Step 4: 测试通过 + 编译**(逐条)

```bash
cargo test --workspace --all-features core_restart_only_for_port_controller_secret
cargo test --workspace --all-features compensation_restores_previous_values_of_patched_keys
cargo check --workspace --all-targets --all-features
```

Expected: PASS / 绿

- [ ] **Step 5: Commit**

```bash
git checkout -- frontend/interface/src/ipc/bindings.ts
git add backend/tauri/src/feat.rs backend/tauri/src/ipc.rs backend/tauri/src/client/runtime.rs
git commit -m "refactor(tauri): patch_clash always regenerates with API-first failure compensation, drop runtime in-memory patch (PR-4 T6)"
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

`client/mod.rs` tests(复用 Task 4 的 `minimal_file_profile_request` helper;`check_and_promote` 失败即 rebuild 失败;完整代码,审计 §四.7):

```rust
    #[test]
    fn activate_returns_degraded_and_keeps_commit_when_rebuild_fails() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .returning(|_, _| Err(anyhow::anyhow!("check boom")));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        tauri::async_runtime::block_on(async {
            let (uid, _) = client
                .add_profile(
                    minimal_file_profile_request(),
                    Some("proxies: []\nmode: rule\n".into()),
                )
                .await
                .expect("add");
            let outcome = client
                .activate_profile(Some(uid.clone()))
                .await
                .expect("activate must commit");
            assert!(matches!(
                outcome,
                crate::client::runtime::RebuildOutcome::Degraded { .. }
            ));
            let profiles = client.get_profiles().await.unwrap();
            assert_eq!(profiles.current.as_ref(), Some(&uid), "state stays committed");
        });
    }
```

(add 本身 `affects_current==false` 不触发 rebuild,故用 `add_profile` 原语而非 `create_profile`。)

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

- [ ] **Step 5: specta 冻结测试(r2 强化——泛型实例化与 union 形状,审计 §三.5)**

`specta_export.rs`:

1. 命名导出断言列表(`for name in [...]`)追加 `"RebuildOutcome"`, `"CommitOutcome"`。
2. `export_typescript_bindings` 测试尾部追加对导出文本的逐字冻结(命名断言不足以保证 `CommitOutcome<ProfileId>` 正确实例化、`rebuild` 字段指向正确 tagged union):

```rust
        let bindings = std::fs::read_to_string(BINDINGS_PATH).unwrap();
        // RebuildOutcome union 形状(以首次导出产物逐字为准;此处为预期形态)
        assert!(
            bindings.contains(r#"{ status: "ok" } | { status: "degraded"; error: string }"#),
            "RebuildOutcome union shape drifted"
        );
        // importProfile 必须返回实例化后的泛型
        assert!(
            bindings.contains("CommitOutcome<ProfileId>"),
            "importProfile must return CommitOutcome<ProfileId>"
        );
```

(两条断言文本在首次生成 bindings 后与实际产物**逐字对齐**再冻结——specta 的空格/引号风格以产物为准,不凭记忆。)

- [ ] **Step 6: 回归钉——legacy 桥路径仍返 Err(spec §8)**

`client/mod.rs` tests 增(与 Step 2 同一 harness,mock `check_and_promote` 失败;完整代码):

```rust
    #[test]
    fn legacy_regeneration_path_still_errors_on_rebuild_failure() {
        let dir = tempdir().unwrap();
        let mut core = MockRunningCoreBridge::new();
        core.expect_check_and_promote()
            .returning(|_, _| Err(anyhow::anyhow!("check boom")));
        core.expect_on_profile_change().returning(|| ());
        let client =
            NyanpasuClient::try_new_with_args(test_profiles_client_args(&dir, Arc::new(core)))
                .unwrap();
        let result = tauri::async_runtime::block_on(client.regenerate_runtime_for_legacy());
        assert!(result.is_err(), "legacy callers rely on Err to discard their drafts");
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
- Modify: `frontend/nyanpasu/messages/{en,zh-cn,zh-tw,ru,ko}.json`(新 key;**ko 为 rebase 后 main 新增 locale,不可遗漏**——审计 §六.1)

**Interfaces:**

- Consumes: Task 8 的 `RebuildOutcome`/`CommitOutcome<ProfileId>` TS 绑定。
- Produces: `@nyanpasu/interface` 导出 `setDegradedRebuildHandler(handler: (error: string) => void): () => void`(**返回 disposer**,r2);`useProfile().create.mutateAsync` 解析值归一为 `{ uid: ProfileId | null; rebuild: RebuildOutcome }`。
- 测试决策(spec §12):仓库无 JS 单测设施(仅 `cargo test`),`extractDegradedRebuild` / MutationCache 去重的单测**不在本 PR 引入 vitest**,记为后续项;行为由 `pnpm typecheck` + Task 10 冒烟覆盖。

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
 * Returns a disposer so HMR / StrictMode double-mount / tests can unregister
 * (r2, 审计 §六.2).
 */
export const setDegradedRebuildHandler = (handler: (error: string) => void) => {
  degradedRebuildHandler = handler
  return () => {
    if (degradedRebuildHandler === handler) {
      degradedRebuildHandler = null
    }
  }
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

- [ ] **Step 6: i18n key ×5**(含 ko,r2)

`frontend/nyanpasu/messages/en.json`:

```json
"profile_rebuild_degraded_message": "Saved, but applying the new config failed: {error}"
```

`zh-cn.json`: `"profile_rebuild_degraded_message": "已保存，但应用新配置失败：{error}"`
`zh-tw.json`: `"profile_rebuild_degraded_message": "已儲存，但套用新設定失敗：{error}"`
`ru.json`: `"profile_rebuild_degraded_message": "Сохранено, но не удалось применить новую конфигурацию: {error}"`
`ko.json`: `"profile_rebuild_degraded_message": "저장되었지만 새 구성을 적용하지 못했습니다: {error}"`(措辞可按项目翻译规范复核)

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
  useEffect(
    () =>
      // setDegradedRebuildHandler 返回 disposer:useEffect cleanup 直接透传,
      // StrictMode 双挂载 / HMR 下不留悬挂 handler(r2)。
      setDegradedRebuildHandler((error) => {
        message(m.profile_rebuild_degraded_message({ error }), {
          title: 'Warning',
          kind: 'warning',
        })
      }),
    [],
  )
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

### Task 10: roadmap 重写 + 文档勘误 + 全量验证收官(r2:整节重写,非追加——审计「两套权威并存」项)

**Files:**

- Modify: `docs/design/actor-migration-roadmap.md`(§4.6 重写、§4.0 图、§4.7 任务/契约/时序图、§2.4、§5、§6)
- Modify: `docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md`
- Modify: `docs/superpowers/specs/2026-07-12-pr4-runtime-derivation-cleanup-design.md`(§6.2 括注勘误)

- [ ] **Step 1: roadmap §4.6 整节重写**(:537-542 正文全部替换——不得只追加状态行,否则「保存完整 RuntimeArtifact」与「只保存 RuntimeState」两套互斥架构并存):

```markdown
### 4.6 PR-4 — runtime 派生化收尾

**目标:** 消灭「可写的 runtime 状态」:删 `Config::runtime()`/`IRuntime`,runtime 成为纯派生快照。

**可执行任务:** ① 存放点 = facade 持有 `SimpleStateManager<Option<RuntimeState>>`——**改判(2026-07-12,PR-4 spec D4/§12):不保存完整 `RuntimeArtifact`**,重建时一次派生只读 `RuntimeState { config, exists_keys, postprocessing_output }`;graph / step_logs **明确放弃**(YAGNI:无现实消费者,四读 IPC 与 PR-5 核心消费只需产物与读模型;图谱/诊断需求出现时 post-PR-5 再引入),`client.rebuild_running_config()` 统一重建入口;② `clash-config.yaml` 降级为「产物」:唯一候选文件 → 核心二进制 check(与 builder 同源的显式 target core)→ atomicwrites 晋升 → 发布 manager——产物与发布状态任何时刻只含已检查配置;③ 四条 IPC(`get_runtime_config/yaml/exists/postprocessing_output`)改读 facade manager,wire 保形;④ `feat::patch_clash` 的 runtime 内存 patch 删除,四字段并入 rebuild 输入,IPC 层 API-first + 失败补偿(spec D6);⑤ 重建/换核/legacy 更新统一 `rebuild_gate` 事务域:`change_core` 编排迁 facade(强回滚),legacy 成对调用改组合桥操作(spec D7);⑥ 删 `Config::runtime()`、`IRuntime`、`Config::generate()`/`generate_file()`。
**验证:** `grep "Config::runtime\(\)"` 零命中;`enhance_profiles` IPC 行为不变;check 失败时产物与 manager 双双保旧。
**状态:** ✅ 已实施(2026-07,分支 `refactor/pr4-runtime-derivation`,PR 号合并后回填)。详见 `docs/superpowers/specs/2026-07-12-pr4-runtime-derivation-cleanup-design.md`。
```

- [ ] **Step 2: roadmap §4.0 端态图两节点修正**

1. `:258` `NC2` 节点文案「commit 后副作用分发·持有 RuntimeArtifact(PR-4)」→「commit 后副作用分发·持有 RuntimeState 读模型(PR-4)」。
2. `:326` `ART` 节点「RuntimeArtifact<br/>(facade 持有,SimpleStateManager,PR-4)」→「RuntimeState 读模型<br/>(facade 持有,SimpleStateManager;由 RuntimeArtifact 重建点一次投影,PR-4)」。

- [ ] **Step 3: roadmap §4.7(PR-5)契约、任务与时序图同步**

1. 任务 ① 消息集中 `UpdateConfig(artifact 或 path)` **定死为 `UpdateConfig(产物路径)`**(artifact 选项随 D4 改判废弃)。
2. 任务清单追加(C-M4 显式登记,防文档迁移丢失——审计 §三.6):

```markdown
⑦ C-M4 端口生命周期编排:restart/change-core 时序 stop → resolve ports → mirror API/sysproxy consumers → build/check/promote → start;验收含 fixed-port 被旧核占用场景的测试。
```

3. `:579-581` 时序图三行改为:

```
    RB-->>NC: RuntimeArtifact(final_config·graph·step_logs·applied_fields)
    NC->>NC: 派生 RuntimeState → 唯一候选 → check(target core)→ atomicwrites 晋升产物 → 发布(PR-4)
    NC->>CO: call(UpdateConfig(产物路径), None)
```

- [ ] **Step 4: roadmap §2.4 遗留计数刷新**(spec §9 承诺项,审计 §四.8-5)

rebase 后实测重算并改写 §2.4 首行计数(至少 `runtime()`、`generate()` 两项归零/变化):

```bash
rg -c "Config::verge\(\)|Config::clash\(\)|Config::profiles\(\)|Config::runtime\(\)" backend/tauri/src
rg -c "Config::generate" backend/tauri/src
```

- [ ] **Step 5: roadmap §5 B8 行与收尾登记**

B8 行「删除条件」列改为:

```
PR-4 已清偿 runtime draft 写入;残余仅 `CoreManager` check/apply/restart 桥(`client/core_bridge.rs`,PR-5)
```

§5 末尾(2026-07-11 收尾登记段后)追加(**计数以 Step 9 的 rg 枚举实测回填,不得硬编码预测值**——审计 §四.8-6;PR-4 净变化 = −1 B8 +新增 core_bridge/change_core 桥面若干,均带 TODO+删除条件):

```markdown
**2026-07-12 PR-4 收尾登记:** B8 的 runtime draft 写入桥已删除;`TODO/FIXME(actor-migration)` 台账 17 → <N>(逐处枚举见 PR-4 spec §7 对账;新增桥面 = LegacyCoreBridge check/restart、facade change_core 的 verge/Logger 触点,PR-5/6 清偿)。`RebuildOutcome` 降级模型为过渡语义,终态方向见 §6 新增行。
```

- [ ] **Step 6: roadmap §6 风险表追加行**

```markdown
| 降级模型(`RebuildOutcome`)为过渡语义 | TODO(post-PR-7):state 层异步 ack 就绪后,配置应用失败改走 ack 驱动 rollback,取代降级上报(用户决策 2026-07-11,PR-4 spec D2) |
```

- [ ] **Step 7: task.md §6 回填**(文末追加)

```markdown
**2026-07-12 用户决策与处置(PR-4 落地):**

- **C-M2(后半):** 已落地——变更类 profile IPC 返回 `RebuildOutcome`(committed/degraded);ack-based rollback 记为 post-PR-7 方向。见 `docs/superpowers/specs/2026-07-12-pr4-runtime-derivation-cleanup-design.md` §6.2。
- **C-M5:** 已落地——`run_core_inner` 的 `Config::clash().reload()` 删除(重启=应用当前 draft)。
- **C-M4(后半)勘误:** 端口生命周期编排改挂 **PR-5**(编排需控制核心启停时序,属 CoreActor 职责),已登记进 roadmap §4.7 任务 ⑦ 与验收。
```

同时把 :761「知悉不修」行内的「(端口生命周期编排 stop→resolve→mirror→start 与 legacy sysproxy/API 镜像回写时机,PR-4)」中的 `PR-4` 改为 `PR-5(2026-07-12 勘误)`。

- [ ] **Step 8: PR-4 spec §6.2 括注勘误**

spec 文件 §6.2 覆盖规则句中「(已知含 activate / save / patch / delete 触发重建的路径)」替换为:

```
(实施期枚举定稿:11 条 unit 命令 + `create_profile` 返回 `RebuildOutcome`,`import_profile` 返回 `CommitOutcome<ProfileId>`;`enhance_profiles` 无前置 commit、`save_profile_file` 不触发 rebuild,均不适用)
```

- [ ] **Step 9: 台账枚举与验收判据 sweep**

Run(逐条,应全部满足;bash 语法——不得混用 PowerShell 的 `2>$null`,审计 §四.4):

```bash
rg -n "TODO\(actor-migration\)|FIXME\(actor-migration\)" backend/tauri/src   # 全量枚举:逐处与 spec §7 对账,总数回填 Step 5 的 <N>
rg -n "Config::runtime|IRuntime|generate_file|ConfigType" backend/tauri/src  # 零命中
cargo test --workspace --all-features                                        # 全绿(后台运行)
pnpm -F interface build && pnpm typecheck && pnpm web:build                  # 全绿
git diff --exit-code -- frontend/interface/src/ipc/bindings.ts               # 零漂移:T9 已落账,test 重写后必须与提交逐字一致;发现漂移即失败——不得 checkout 还原掩盖(审计 §四.5)
git status --short                                                            # 工作树干净
```

若枚举与 spec §7 不符,逐处对账后修正注释(不改行为)再重跑。

- [ ] **Step 10: Commit + push**

```bash
git add docs/design/actor-migration-roadmap.md "docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md" docs/superpowers/specs/2026-07-12-pr4-runtime-derivation-cleanup-design.md
git commit -m "docs: PR-4 closeout — roadmap 4.6 rewritten to RuntimeState read model, C-M4 registered under PR-5, ledger re-enumerated"
git push https://github.com/libnyanpasu/clash-nyanpasu.git refactor/pr4-runtime-derivation
```

- [ ] **Step 11: 人工冒烟清单**(报告给用户,不阻塞)

1. 启动应用(真实数据)→ 核心运行、代理可用(boot 首铸 + 晋升);删产物文件重启 → 兜底默认配置经 check 落盘(D8)。
2. 切换 profile → 生效;编辑 clash mode/allow-lan → 生效且 `get_runtime_yaml` 反映。
3. 换核(mihomo ↔ clash-rs)→ 成功;人为造失败(改坏 core 二进制名)→ 回滚到旧核、旧配置,错误信息含回滚链上下文(P0-4)。
4. 断网后切 profile → UI 出现降级 warning toast,列表状态已切换。
5. 人为造 rebuild 失败(坏 profile)后切 mode → IPC 报错,运行核 mode 被补偿回推为旧值(D6/P0-6)。
