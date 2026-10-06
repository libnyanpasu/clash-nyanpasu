# PR-3 T06 — RuntimeBuilder(add-only,不切换调用点)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新建 `RuntimeBuilder` 纯 service(把域快照确定性降解为 `RuntimePipelineInputs` 并调 executor)+ 两个 executor 适配器(`FsProfileContentSource`、boa/lua `ScriptRunner`);golden 集成测试证明「真实适配器 + executor」产出与旧 `enhance()` 语义等价。**不改 `Config::generate()`、不删旧 enhance 逻辑**(切换在 T07);本卡纯增量、可独立回滚。

**Architecture:** builder 无状态、零全局读:`build(&RuntimeBuildInput, &dyn ProfileContentSource, &dyn ScriptRunner) -> Result<RuntimeArtifact, RuntimeBuildError>`。组装职责(design §19 勘误②):`current→ExecutionTarget::{Selected|Bare}`、`TunFlavor` 推导(**忠实移植 legacy 两处怪癖**:仅 `core==ClashRs` 走 ClashRs 分支、`ClashPremium+Mixed→Gvisor` 降级,tun.rs:47-60)、`ClashCore` bitflags 门控 builtins(**顺序与 gating 与 chain.rs:170-175 逐位一致**,含 `clash_rs_comp` 不含 ClashRsAlpha 的 legacy 事实)、`cfg!(windows)` 传参、guard overrides/whitelist/端口透传。脚本适配器自持一个 current-thread tokio runtime(`build` 是同步纯函数,可在任意 `spawn_blocking` 上下文运行,无 Handle 依赖);`eval_item_*` 直接复用 `enhance/merge.rs:63-90` 的 `create_lua_context` + `item` 全局求值语义。

**Tech Stack:** `nyanpasu_config::runtime::executor::{execute, RuntimePipelineInputs, ...}`(#4877 实物)、既有 `RunnerManager`(`enhance/script/runner.rs:71`,async)、`create_lua_context`、`ConfigValue ⇄ serde_yaml::Value` 转换(`runtime/value/convert.rs`)、tempfile。

## Global Constraints(task.md §0)

- `runtime_builder.rs` 纯度:无 `crate::config::`、无 `tauri::` import(输入全部显式传参;grep 断言)。
- 本卡不动任何调用点;`enhance/mod.rs` 旧函数原样保留。
- 每个 commit `cargo build` + `cargo test` 绿。

## 基线事实(2026-07-06 实测)

- executor 入口/输入:`executor/mod.rs:38-105,196`(`RuntimePipelineInputs{profiles, target, guard: GuardInputs{overrides: &ClashGuardOverrides, ports: ResolvedPortBindings}, whitelist_enabled, tun: TunParams{enable, flavor, windows_fake_ip_filter}, builtin_transforms: &[BuiltinTransform{name, runtime, source}]}`);`RuntimeArtifact{final_config: Arc<ConfigValue>, graph, step_logs, applied_fields}`(artifact.rs:57-62)。
- 新域字段实名:`ClashConfig{overrides, enable_tun_mode, enable_clash_fields, external_controller, mixed_port, socks_port, http_port, tun_stack, ...}`(clash/config/mod.rs:27-59);`NyanpasuAppConfig{core: ClashCore(alias clash_core), enable_builtin_enhanced, ...}`(application/mod.rs:118-129);`ClashCore{ClashPremium, ClashRs, Mihomo, MihomoAlpha, ClashRsAlpha}` 支持 enumflags2(clash_core.rs:7-35);`TunStack{System, Gvisor(默认), Mixed}`。
- legacy builtin 表(chain.rs:145-176,顺序即执行序):`verge_hy_alpn`(js,Mihomo|MihomoAlpha)→ `verge_meta_guard`(js,Mihomo|MihomoAlpha)→ `config_fixer`(js,all)→ `clash_rs_comp`(lua,**仅 ClashRs**);整表由 `enable_builtin_enhanced`(默认 true)门控;脚本文件在 `enhance/builtin/*.{js,lua}`(include_str!)。
- legacy tun 怪癖(tun.rs:47-60):`core==ClashRs`(不含 Alpha)→ ClashRs 分支;否则 stack = `tun_stack`,`ClashPremium && Mixed → Gvisor`。
- 脚本运行:`RunnerManager::process_script(&ScriptWrapper(ScriptType, String), Mapping) -> (Result<Mapping>, Logs)`(async);`Logs = Vec<(LogSpan, String)>`(utils.rs:38),`LogSpan` 与 `StepLogLevel` 逐 variant 同名;逐项 Lua 求值先例 = merge.rs:63-90。
- executor 自带 tests(`golden/parity/orders/...`)是 crate 内 `#[cfg(test)]`,tauri 侧**不可 import**——T06 单测自备极小 fakes(仿 support.rs 形态),集成 golden 用真实适配器。

## 契约修正(执行后回写 T06 卡,§5.3)

1. `RuntimeBuildInput` 最终形态(替换卡内草签):

```rust
pub struct RuntimeBuildInput {
    pub profiles: Arc<Profiles>,                 // ProfilesClient 快照(须已 validate)
    pub clash: ClashConfig,                      // ClashConfigClient 快照
    pub app: NyanpasuAppConfig,                  // ApplicationClient 快照
    pub resolved_ports: ResolvedPortBindings,    // 端口解析(IO)在调用方(T07)完成
}
impl RuntimeBuilder {
    pub fn build(input: &RuntimeBuildInput, content: &dyn ProfileContentSource, scripts: &dyn ScriptRunner)
        -> Result<RuntimeArtifact, RuntimeBuildError>;
}
pub enum RuntimeBuildError { Validation(Vec<ProfileValidationError>), Pipeline(RuntimePipelineError) }
```

2. `build()` 前置校验 `profiles.validate()`(executor 契约要求输入已验证,builder 防御性背书)。
3. 脚本适配器 `EnhanceScriptRunner` 自持 current-thread runtime(阻塞式 `block_on`);`RuntimeBuilder::build` 因而必须在阻塞上下文调用(T07 统一 `spawn_blocking`)——记入 T07 卡 Consumes。
4. 忠实移植的 legacy 怪癖(不"修复"):`clash_rs_comp` 门控不含 `ClashRsAlpha`;tun ClashRs 分支不含 `ClashRsAlpha`。若要修复属行为变更,须走 spec 勘误,不在本卡。

---

### Task 1: `FsProfileContentSource` 适配器

**Files:**

- Create: `backend/tauri/src/enhance/content_source.rs`
- Modify: `backend/tauri/src/enhance/mod.rs`(`mod content_source; pub use content_source::FsProfileContentSource;`)

**Interfaces:**

- Produces: `FsProfileContentSource::new(profiles_dir: PathBuf)`,impl `nyanpasu_config::runtime::executor::ProfileContentSource`(T07 用 `paths.app_profiles_dir()` 构造)。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::{profile::ManagedProfilePath, runtime::executor::ProfileContentSource};

    #[test]
    fn reads_relative_managed_paths_from_profiles_dir() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("abc.yaml"), "proxies: []\n").unwrap();
        let source = FsProfileContentSource::new(temp.path().to_path_buf());
        let content = source.read(&ManagedProfilePath::new("abc.yaml").unwrap()).unwrap();
        assert_eq!(content, "proxies: []\n");
        assert!(source.read(&ManagedProfilePath::new("missing.yaml").unwrap()).is_err());
    }
}
```

- [ ] **Step 2: 确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu reads_relative_managed`
Expected: FAIL(类型未定义)。

- [ ] **Step 3: 实现**

```rust
//! ProfileContentSource over the real profiles directory (PR-3 T06).

use std::path::PathBuf;

use nyanpasu_config::{
    profile::ManagedProfilePath,
    runtime::executor::{PortError, ProfileContentSource},
};

pub struct FsProfileContentSource {
    profiles_dir: PathBuf,
}

impl FsProfileContentSource {
    pub fn new(profiles_dir: PathBuf) -> Self {
        Self { profiles_dir }
    }
}

impl ProfileContentSource for FsProfileContentSource {
    fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
        let full = self.profiles_dir.join(path.as_path());
        std::fs::read_to_string(&full)
            .map_err(|e| format!("read profile content {}: {e}", full.display()).into())
    }
}
```

- [ ] **Step 4: 确认通过 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu reads_relative_managed`
Expected: PASS。

```bash
git add backend/tauri/src/enhance/content_source.rs backend/tauri/src/enhance/mod.rs
git commit -m "feat(tauri): add fs profile content source for runtime executor"
```

---

### Task 2: `EnhanceScriptRunner` 适配器(boa/lua + 逐项求值)

**Files:**

- Create: `backend/tauri/src/enhance/script/adapter.rs`
- Modify: `backend/tauri/src/enhance/script/mod.rs`(模块声明 + re-export;现有 runner 不动)

**Interfaces:**

- Produces: `EnhanceScriptRunner::new() -> anyhow::Result<Self>`,impl `nyanpasu_config::runtime::executor::ScriptRunner`(三方法)。

- [ ] **Step 1: 写失败测试(真实 boa/lua,微型脚本)**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::{
        profile::ScriptRuntime,
        runtime::{executor::ScriptRunner as _, value::ConfigValue},
    };

    fn value(yaml: &str) -> ConfigValue {
        let value: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        ConfigValue::try_from(value).unwrap()
    }

    #[test]
    fn runs_javascript_transform_and_captures_logs() {
        let runner = EnhanceScriptRunner::new().unwrap();
        let script = r#"
function main(config) {
  console.log("hello from js");
  config["mode"] = "rule";
  return config;
}
"#;
        let outcome = runner.run(ScriptRuntime::JavaScript, script, &value("mixed-port: 7890\n"));
        let result = outcome.result.expect("script should succeed");
        let yaml: serde_yaml::Value = (&result).try_into().or_else(|_| {
            serde_yaml::to_value(serde_json::Value::from(result.to_json())).map_err(|e| format!("{e}"))
        }).unwrap();
        assert_eq!(yaml["mode"], serde_yaml::Value::from("rule"));
        assert!(!outcome.logs.is_empty(), "console.log must surface as step log");
    }

    #[test]
    fn failing_script_returns_error_with_logs_preserved() {
        let runner = EnhanceScriptRunner::new().unwrap();
        let outcome = runner.run(ScriptRuntime::JavaScript, "not valid js ][", &value("a: 1\n"));
        assert!(outcome.result.is_err());
    }

    #[test]
    fn eval_item_predicate_and_expr_use_lua_item_global() {
        let runner = EnhanceScriptRunner::new().unwrap();
        let item = value("name: test-node\ntype: ss\n");
        let keep = runner.eval_item_predicate(r#"item.name == "test-node""#, &item).unwrap();
        assert!(keep);
        let drop = runner.eval_item_predicate(r#"item.name == "other""#, &item).unwrap();
        assert!(!drop);
        let replaced = runner
            .eval_item_expr(r#"(function() item.name = "renamed"; return item end)()"#, &item)
            .unwrap();
        let yaml = serde_yaml::to_value(replaced.to_json()).unwrap();
        assert_eq!(yaml["name"], serde_yaml::Value::from("renamed"));
    }
}
```

(`ConfigValue` 与 serde 值的互转以 `runtime/value/{convert,ser,de}.rs` 实物 API 为准——若无 `to_json()`,改用 `serde_json::to_value(&result)`/`ConfigValue` 的 Serialize impl;编译错误时对齐,断言语义不变。legacy js runner 的入口形态(`function main(config)`)以 `enhance/script/js.rs` 现物为准,若需 `export default` 形态则改脚本文本。)

- [ ] **Step 2: 确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu --lib script::adapter`
Expected: FAIL。

- [ ] **Step 3: 实现**

```rust
//! ScriptRunner adapter over the legacy boa/lua runners (PR-3 T06).
//! Owns a private current-thread runtime so `run` stays synchronous and the
//! whole pipeline can execute inside spawn_blocking (roadmap §4.0.4).

use nyanpasu_config::{
    profile::ScriptRuntime,
    runtime::{
        executor::{PortError, ScriptRunOutcome, ScriptRunner, StepLogEntry, StepLogLevel},
        value::ConfigValue,
    },
};

use super::{super::utils::{LogSpan, Logs}, ScriptType, create_lua_context};
use crate::enhance::{ChainTypeWrapper, ScriptWrapper, script::RunnerManager};

pub struct EnhanceScriptRunner {
    runtime: tokio::runtime::Runtime,
}

impl EnhanceScriptRunner {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
        })
    }
}

fn to_step_logs(logs: Logs) -> Vec<StepLogEntry> {
    logs.into_iter()
        .map(|(span, message)| {
            let level = match span {
                LogSpan::Log => StepLogLevel::Log,
                LogSpan::Info => StepLogLevel::Info,
                LogSpan::Warn => StepLogLevel::Warn,
                LogSpan::Error => StepLogLevel::Error,
            };
            StepLogEntry::new(level, message)
        })
        .collect()
}

fn config_to_mapping(config: &ConfigValue) -> Result<serde_yaml::Mapping, PortError> {
    let value: serde_yaml::Value =
        serde_yaml::to_value(config).map_err(|e| format!("config to yaml: {e}"))?;
    value
        .as_mapping()
        .cloned()
        .ok_or_else(|| "config is not a mapping".into())
}

fn mapping_to_config(mapping: serde_yaml::Mapping) -> Result<ConfigValue, PortError> {
    ConfigValue::try_from(serde_yaml::Value::Mapping(mapping))
        .map_err(|e| format!("yaml to config: {e:?}").into())
}

impl ScriptRunner for EnhanceScriptRunner {
    fn run(&self, runtime: ScriptRuntime, source: &str, config: &ConfigValue) -> ScriptRunOutcome {
        let script_type = match runtime {
            ScriptRuntime::JavaScript => ScriptType::JavaScript,
            ScriptRuntime::Lua => ScriptType::Lua,
        };
        let mapping = match config_to_mapping(config) {
            Ok(mapping) => mapping,
            Err(error) => {
                return ScriptRunOutcome { result: Err(error), logs: Vec::new() };
            }
        };
        let wrapper = ScriptWrapper(script_type, source.to_string());
        let (result, logs) = self.runtime.block_on(async {
            let mut manager = RunnerManager::new();
            manager.process_script(&wrapper, mapping).await
        });
        ScriptRunOutcome {
            result: result
                .map_err(|e| PortError::from(e.to_string()))
                .and_then(mapping_to_config),
            logs: to_step_logs(logs),
        }
    }

    fn eval_item_predicate(&self, expr: &str, item: &ConfigValue) -> Result<bool, PortError> {
        let lua = create_lua_context().map_err(|e| format!("lua context: {e}"))?;
        let item_yaml: serde_yaml::Value =
            serde_yaml::to_value(item).map_err(|e| format!("item to yaml: {e}"))?;
        use mlua::LuaSerdeExt as _;
        let lua_item = lua.to_value(&item_yaml).map_err(|e| format!("item to lua: {e}"))?;
        lua.globals().set("item", lua_item).map_err(|e| format!("set item: {e}"))?;
        lua.load(expr)
            .eval::<bool>()
            .map_err(|e| format!("predicate eval: {e}").into())
    }

    fn eval_item_expr(&self, expr: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
        let lua = create_lua_context().map_err(|e| format!("lua context: {e}"))?;
        let item_yaml: serde_yaml::Value =
            serde_yaml::to_value(item).map_err(|e| format!("item to yaml: {e}"))?;
        use mlua::LuaSerdeExt as _;
        let lua_item = lua.to_value(&item_yaml).map_err(|e| format!("item to lua: {e}"))?;
        lua.globals().set("item", lua_item).map_err(|e| format!("set item: {e}"))?;
        let result = lua
            .load(expr)
            .eval::<mlua::Value>()
            .map_err(|e| format!("expr eval: {e}"))?;
        let yaml: serde_yaml::Value =
            lua.from_value(result).map_err(|e| format!("lua to yaml: {e}"))?;
        ConfigValue::try_from(yaml).map_err(|e| format!("yaml to config: {e:?}").into())
    }
}
```

(import 路径、`ScriptWrapper`/`ChainTypeWrapper` 可见性、`ConfigValue` 的 serde 能力以现物为准修正;`create_lua_context` 若为 `pub(crate)` 需放宽到 `pub(in crate::enhance)`。语义锚点:merge.rs:63-90。)

- [ ] **Step 4: 确认通过 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu --lib script::adapter`
Expected: 全 PASS。

```bash
git add backend/tauri/src/enhance/script
git commit -m "feat(tauri): add script runner adapter over boa/lua for executor"
```

---

### Task 3: `RuntimeBuilder` 组装逻辑(单测 = 本地 fakes)

**Files:**

- Create: `backend/tauri/src/enhance/runtime_builder.rs`
- Modify: `backend/tauri/src/enhance/mod.rs`(`mod runtime_builder; pub use runtime_builder::*;`)

**Interfaces:**

- Produces(T07 依赖):契约修正节的 `RuntimeBuildInput`/`RuntimeBuilder::build`/`RuntimeBuildError` + `pub fn builtin_transforms_for(core: ClashCore) -> Vec<BuiltinTransform>`(门控表单独可测)。

- [ ] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use nyanpasu_config::{
        application::{ClashCore, NyanpasuAppConfig},
        clash::config::{ClashConfig, TunStack},
        profile::{ManagedProfilePath, ProfileId, Profiles, ScriptRuntime},
        runtime::executor::{
            PortError, ProfileContentSource, ResolvedPortBindings, ScriptRunOutcome, ScriptRunner,
            TunFlavor,
        },
        runtime::value::ConfigValue,
    };

    /// 极小 fakes(executor 的 support.rs 是 crate 内部,tauri 不可 import)
    struct EmptyContent;
    impl ProfileContentSource for EmptyContent {
        fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
            Err(format!("no content for {path}").into())
        }
    }
    struct EchoRunner;
    impl ScriptRunner for EchoRunner {
        fn run(&self, _: ScriptRuntime, _: &str, config: &ConfigValue) -> ScriptRunOutcome {
            ScriptRunOutcome { result: Ok(config.clone()), logs: Vec::new() }
        }
        fn eval_item_predicate(&self, _: &str, _: &ConfigValue) -> Result<bool, PortError> {
            Ok(true)
        }
        fn eval_item_expr(&self, _: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
            Ok(item.clone())
        }
    }

    fn base_input() -> RuntimeBuildInput {
        RuntimeBuildInput {
            profiles: Arc::new(Profiles::default()),
            clash: ClashConfig::default(),
            app: NyanpasuAppConfig::default(),
            resolved_ports: ResolvedPortBindings { mixed_port: 7890, ..Default::default() },
        }
    }

    #[test]
    fn builtin_gating_matches_legacy_table() {
        let names = |core: ClashCore| -> Vec<String> {
            builtin_transforms_for(core).into_iter().map(|b| b.name).collect()
        };
        assert_eq!(
            names(ClashCore::Mihomo),
            vec!["verge_hy_alpn", "verge_meta_guard", "config_fixer"]
        );
        assert_eq!(names(ClashCore::ClashRs), vec!["config_fixer", "clash_rs_comp"]);
        // legacy 怪癖忠实移植:Alpha 不吃 clash_rs_comp(chain.rs:174)
        assert_eq!(names(ClashCore::ClashRsAlpha), vec!["config_fixer"]);
        assert_eq!(names(ClashCore::ClashPremium), vec!["config_fixer"]);
    }

    #[test]
    fn tun_flavor_derivation_matches_legacy_quirks() {
        assert_eq!(derive_tun_flavor(ClashCore::ClashRs, TunStack::Mixed), TunFlavor::ClashRs);
        // Alpha 走 Standard 分支(tun.rs:47 legacy 怪癖)
        assert_eq!(
            derive_tun_flavor(ClashCore::ClashRsAlpha, TunStack::Mixed),
            TunFlavor::Standard { stack: TunStack::Mixed }
        );
        // Premium + Mixed → Gvisor 降级(tun.rs:58-60)
        assert_eq!(
            derive_tun_flavor(ClashCore::ClashPremium, TunStack::Mixed),
            TunFlavor::Standard { stack: TunStack::Gvisor }
        );
        assert_eq!(
            derive_tun_flavor(ClashCore::Mihomo, TunStack::System),
            TunFlavor::Standard { stack: TunStack::System }
        );
    }

    #[test]
    fn bare_build_produces_artifact_with_guarded_ports() {
        let input = base_input(); // current = None → Bare
        let artifact = RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner).expect("bare build");
        let yaml = serde_yaml::to_value(&*artifact.final_config).expect("artifact to yaml");
        assert_eq!(yaml["mixed-port"], serde_yaml::Value::from(7890));
    }

    #[test]
    fn invalid_profiles_rejected_before_executor() {
        let mut input = base_input();
        let mut profiles = Profiles::default();
        profiles.set_current(Some(ProfileId("ghost".into())));
        input.profiles = Arc::new(profiles);
        assert!(matches!(
            RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner),
            Err(RuntimeBuildError::Validation(_))
        ));
    }

    #[test]
    fn builtin_disabled_flag_empties_the_list() {
        let mut input = base_input();
        input.app.enable_builtin_enhanced = false;
        input.app.core = ClashCore::Mihomo;
        // 语义断言经 build 输入观察:关闭后 executor 收到空 builtin 列表,
        // 产物 graph 中不得出现 BuiltinTransform 节点(以 graph API 实名核对)
        let artifact = RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner).unwrap();
        let debug = format!("{:?}", artifact.graph);
        assert!(!debug.contains("verge_hy_alpn"));
    }
}
```

- [ ] **Step 2: 确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu runtime_builder`
Expected: FAIL。

- [ ] **Step 3: 实现**

```rust
//! RuntimeBuilder: pure assembly from domain snapshots to the runtime pipeline
//! executor (PR-3 T06, design §8 + §19). No globals, no IO — ports resolve and
//! file reads/script runs arrive via explicit parameters and executor ports.

use std::sync::Arc;

use nyanpasu_config::{
    application::{ClashCore, NyanpasuAppConfig},
    clash::config::{ClashConfig, TunStack},
    profile::{ProfileValidationError, Profiles, ScriptRuntime},
    runtime::executor::{
        BuiltinTransform, ExecutionTarget, GuardInputs, ProfileContentSource,
        ResolvedPortBindings, RuntimeArtifact, RuntimePipelineError, RuntimePipelineInputs,
        ScriptRunner, TunFlavor, TunParams, execute,
    },
};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeBuildError {
    #[error("profiles snapshot failed validation: {0:?}")]
    Validation(Vec<ProfileValidationError>),
    #[error(transparent)]
    Pipeline(#[from] RuntimePipelineError),
}

pub struct RuntimeBuildInput {
    pub profiles: Arc<Profiles>,
    pub clash: ClashConfig,
    pub app: NyanpasuAppConfig,
    pub resolved_ports: ResolvedPortBindings,
}

/// Legacy builtin table, ported 1:1 from enhance/chain.rs:145-176 (order is
/// execution order; ClashRsAlpha intentionally not in clash_rs_comp gating).
pub fn builtin_transforms_for(core: ClashCore) -> Vec<BuiltinTransform> {
    use enumflags2::BitFlags;
    let table: [(BitFlags<ClashCore>, &str, ScriptRuntime, &str); 4] = [
        (
            ClashCore::Mihomo | ClashCore::MihomoAlpha,
            "verge_hy_alpn",
            ScriptRuntime::JavaScript,
            include_str!("./builtin/meta_hy_alpn.js"),
        ),
        (
            ClashCore::Mihomo | ClashCore::MihomoAlpha,
            "verge_meta_guard",
            ScriptRuntime::JavaScript,
            include_str!("./builtin/meta_guard.js"),
        ),
        (
            BitFlags::all(),
            "config_fixer",
            ScriptRuntime::JavaScript,
            include_str!("./builtin/config_fixer.js"),
        ),
        (
            ClashCore::ClashRs.into(),
            "clash_rs_comp",
            ScriptRuntime::Lua,
            include_str!("./builtin/clash_rs_comp.lua"),
        ),
    ];
    table
        .into_iter()
        .filter(|(gate, ..)| gate.contains(core))
        .map(|(_, name, runtime, source)| BuiltinTransform {
            name: name.to_string(),
            runtime,
            source: source.to_string(),
        })
        .collect()
}

/// Legacy tun derivation (enhance/tun.rs:47-60), quirks preserved:
/// only `ClashRs` (not Alpha) takes the ClashRs branch; Premium+Mixed → Gvisor.
pub fn derive_tun_flavor(core: ClashCore, stack: TunStack) -> TunFlavor {
    if core == ClashCore::ClashRs {
        return TunFlavor::ClashRs;
    }
    let stack = if core == ClashCore::ClashPremium && stack == TunStack::Mixed {
        TunStack::Gvisor
    } else {
        stack
    };
    TunFlavor::Standard { stack }
}

pub struct RuntimeBuilder;

impl RuntimeBuilder {
    pub fn build(
        input: &RuntimeBuildInput,
        content: &dyn ProfileContentSource,
        scripts: &dyn ScriptRunner,
    ) -> Result<RuntimeArtifact, RuntimeBuildError> {
        input
            .profiles
            .validate()
            .map_err(RuntimeBuildError::Validation)?;

        let target = match &input.profiles.current {
            Some(uid) => ExecutionTarget::Selected(uid.clone()),
            None => ExecutionTarget::Bare,
        };
        let builtin_transforms = if input.app.enable_builtin_enhanced {
            builtin_transforms_for(input.app.core)
        } else {
            Vec::new()
        };
        let inputs = RuntimePipelineInputs {
            profiles: &input.profiles,
            target,
            guard: GuardInputs {
                overrides: &input.clash.overrides,
                ports: input.resolved_ports.clone(),
            },
            whitelist_enabled: input.clash.enable_clash_fields,
            tun: TunParams {
                enable: input.clash.enable_tun_mode,
                flavor: derive_tun_flavor(input.app.core, input.clash.tun_stack),
                windows_fake_ip_filter: cfg!(windows),
            },
            builtin_transforms: &builtin_transforms,
        };
        execute(&inputs, content, scripts).map_err(RuntimeBuildError::Pipeline)
    }
}
```

- [ ] **Step 4: 确认通过 + 纯度断言 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu runtime_builder`
Expected: 全 PASS。
Run(Git Bash): `grep -n "crate::config\|tauri::" backend/tauri/src/enhance/runtime_builder.rs && echo LEAK || echo CLEAN`
Expected: `CLEAN`。

```bash
git add backend/tauri/src/enhance/runtime_builder.rs backend/tauri/src/enhance/mod.rs
git commit -m "feat(tauri): add RuntimeBuilder over runtime pipeline executor"
```

---

### Task 4: golden 集成测试(真实适配器)+ 契约回写

**Files:**

- Create: `backend/tauri/src/enhance/runtime_builder.rs` 内 `#[cfg(test)] mod golden`(或独立 `tests/` 集成目标,以编译最快者为准)
- Create: `backend/tauri/tests/fixtures/pr3-golden/`(profile 文件 + expected YAML)

- [ ] **Step 1: 构造 fixture 场景(覆盖 design §15 golden 清单的可 add-only 部分)**

fixtures:`base.yaml`(含 proxies/proxy-groups/rules 的完整配置)、`ovl.yaml`(Overlay:追加字段 + `filter__proxies`)、`scr.js`(改 mode 的脚本)、`member.yaml`(Composition 贡献者)。场景:

1. 单 current(File + scoped [ovl, scr])+ global [ovl2] + guard overrides + whitelist on + Mihomo builtin;
2. Composition{base, extend:[member]}(经 migration 语义构造的形态,验证 §18 第 24 条运行半);
3. bare(current=None);
4. whitelist off(enable_clash_fields=false)对照。

每个场景:`RuntimeBuilder::build` 用 `FsProfileContentSource`(tempdir 写入 fixtures)+ `EnhanceScriptRunner`,产物 `final_config` 序列化为 YAML 与 `expected_{n}.yaml` 逐值比对(`pretty_assertions`);`step_logs` 断言:脚本步骤有日志锚点(覆盖 `get_postprocessing_output` 消费需求)。

**expected 文件的产生方式(一次性,记录在测试文件头注释):** 首轮运行以 `--nocapture` 打印产物 → 人工对照旧 `enhance()` 语义(HANDLE_FIELDS overlay 结果、whitelist 过滤集、builtin 效果如 alpn 数组化、tun 键形态)与 PR-3-pre② parity 套件的既有期望 → 审阅通过后落盘为 expected;此后回归锁死。**禁止盲录**——审阅记录写入 PR 描述。

- [ ] **Step 2: 跑 golden**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu golden`(测试在 `spawn_blocking` 中调 build,或 `#[test]`(非 tokio)直接调——builder 自身同步,EnhanceScriptRunner 自持 runtime,普通 `#[test]` 即可)
Expected: 全 PASS。

- [ ] **Step 3: 全量回归**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu && cargo clippy --manifest-path ./backend/Cargo.toml -p clash-nyanpasu --all-targets --all-features`
Expected: 全绿。

- [ ] **Step 4: 契约回写 + Commit**

按「契约修正」1–4 更新 T06 卡(RuntimeBuildInput 实名、RuntimeBuildError、阻塞上下文要求 → 同步 T07 卡 Consumes、怪癖忠实移植记录)。

```bash
git add backend/tauri/tests backend/tauri/src/enhance docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md
git commit -m "test(tauri): add runtime builder golden suite against legacy semantics"
```

---

## 验证总表(对应 T06 卡验证段)

| 判据                                        | 覆盖                                                 |
| ------------------------------------------- | ---------------------------------------------------- |
| golden 对照(同输入产物与旧 enhance 等价)    | Task 4(真实适配器 + 人工审阅的 expected)             |
| `step_logs` 覆盖 postprocessing_output 需求 | Task 4 日志锚点断言                                  |
| 纯度:runtime_builder 无 Config/tauri import | Task 3 Step 4 grep                                   |
| builtin 门控与顺序 = chain.rs:170-175       | Task 3 `builtin_gating_matches_legacy_table`         |
| tun 推导两怪癖忠实                          | Task 3 `tun_flavor_derivation_matches_legacy_quirks` |
| bare 目标支持                               | Task 3 `bare_build_produces_artifact_...`            |
| 输入未验证防御                              | Task 3 `invalid_profiles_rejected_...`               |
