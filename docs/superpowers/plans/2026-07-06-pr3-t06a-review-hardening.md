# PR-3 T06A — 评审加固 + golden 基线(add-only)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地 T06 评审处置遗留的三项加固:① `ManagedProfilePath` 路径穿越拒绝的**反序列化面回归钉**(实测防御已在位,本卡钉死它);② actor Mirror 同步的阻塞 I/O 移入 `spawn_blocking`;③ RuntimeBuilder **golden snapshot 文件套件**——在 T07 切换 `Config::generate()` 之前锁定行为基线。全部 add-only(②是等价重构),不入 §4 原子切换组,可独立回滚。

**Architecture:** ①只加测试(防御在 `nyanpasu-config` 域层,`Deserialize` 委托 `new()`);②把 `ExternalFileChanged` handler 的 Mirror 分支(读外部文件→校验→原子写镜像)整体搬进 `tokio::task::spawn_blocking`,handler `await` 该任务,**actor 消息顺序语义不变**,三条 warn 日志逐字保留;③新增 `#[cfg(test)] mod golden`,用真实适配器(`FsProfileContentSource` + `EnhanceScriptRunner`)驱动 `RuntimeBuilder::build`,`final_config` 与 `src/enhance/fixtures/golden/*.yaml` 期望文件比对,`GOLDEN_BLESS=1` 重铸。

**Tech Stack:** `serde_yaml_ng`(nyanpasu-config 测试)、`tokio::task::spawn_blocking`、`tempfile`、`serde_yaml::Value` 结构比对(免格式噪声)。

## Global Constraints

- 一切 cargo 操作前设 `$env:CARGO_TARGET_DIR='F:\codex-target\clash-nyanpasu-pr3'`(G 盘近满,禁止写默认 target)。
- add-only:不改 `Config::generate()`、不动任何调用点;②仅等价重构。
- 每个 commit `cargo build` + `cargo test` 绿;conventional commits;lint-staged 会跑 clippy --all-targets --all-features,测试代码也须 clippy 干净。
- golden 确定性铁律:**固定 `secret: golden-secret`**(`ClashGuardOverrides::default()` 的 secret 是随机 uuid,overrides/mod.rs:75)、**tun 关闭**(`windows_fake_ip_filter: cfg!(windows)` 平台相关)。

## 基线事实(2026-07-06 实测)

- `ManagedProfilePath::new`(nyanpasu-config `profile/path.rs:17-43`)已拒绝 `Component::{Prefix,RootDir,ParentDir,CurDir}` → `ProfilePathError::ManagedContainsTraversal`;`Deserialize` 委托 `new()`(path.rs:60-67)。构造侧测试已在:`managed_path_rejects_absolute_traversal_and_url`(`profile/tests/validation.rs:166-171`)。**T06 评审处置④「反序列化侧缺 `..` 拒绝」与实物不符**——缺的只是「整份 profiles 文档反序列化拒绝」这一真实攻击面(磁盘上的 profiles.yaml)的回归钉。文档 wire 形状先例:`duplicate_uid_fails_to_deserialize`(validation.rs:181-203)。
- Mirror 同步现状(`state/profiles/actor.rs:807-892`,`ExternalFileChanged` 分支):`state.fs.read_external(target)` → `Self::validate_fetched_content(&item.definition, &content)`(签名 `(&ProfileDefinition, &str) -> Result<(), String>`,actor.rs:325)→ `state.fs.write_atomic(&materialized.file, &content)`,三步全部直接跑在 actor 的 async 线程;watcher 回调本身只 `cast`(scheduler.rs:70-77),无 I/O。既有测试:`external_mirror_change_syncs_copy_and_bumps_updated_at`(client/profiles.rs:785)、`external_watcher_smoke_mirror_real_file_event`(client/profiles.rs:880,`#[ignore]`)。
- golden 素材:`RuntimeBuilder::build(&RuntimeBuildInput, &dyn ProfileContentSource, &dyn ScriptRunner)`(runtime_builder.rs:103);真实适配器 e2e 先例 `golden_selected_file_with_script_transform_end_to_end`(runtime_builder.rs:274-353,tempdir + 真 boa);executor 侧固定 secret 先例 `fixed_overrides()`(nyanpasu-config `runtime/executor/tests/builtin.rs:19-24`,经反序列化构造,kebab-case)。`ClashConfig.overrides` 为 pub 字段(clash/config/mod.rs:29),`ClashGuardOverrides` 可从 `nyanpasu_config::clash::config::overrides` 导入。
- 域类型实名:`CompositionConfig { base: Option<ProfileId>, extend_proxies_from: Vec<ProfileId>, transforms: Vec<ProfileId> }`(definition.rs:35-51);`OverlayTransform { source }`;`Profiles` 的 `global_transforms: Vec<ProfileId>`/`items`/`valid` 均 pub(profiles.rs:20-28),`append_item`/`set_current` 可用。
- 仓库无 `insta` 依赖 → 手写 bless 流(`GOLDEN_BLESS` 环境变量 + `CARGO_MANIFEST_DIR` 定位 fixtures)。

## 契约修正(执行后回写 task.md T06A 卡,§5.3)

1. 卡内第 1 项按实物收缩:「构造/反序列化拒绝 `..`」防御与构造侧测试**已在位**(path.rs:30-40/60-67 + validation.rs:166-171);本卡交付 = 不可信文档反序列化面的回归钉测试。T06 卡评审处置④的「缺失」表述一并修正。
2. golden 套件机制实名:fixtures 在 `backend/tauri/src/enhance/fixtures/golden/`;4 场景(clean-seed Composition + global chain / builtin Mihomo / builtin ClashRs / whitelist-on);重铸方式 `GOLDEN_BLESS=1`;确定性约束 = 固定 secret + tun off。T07 切换 `generate()` 后本套件必须原样全绿(改动 fixtures = 行为回归,需勘误)。
3. Mirror 同步改 `spawn_blocking`(handler await,消息顺序不变;三条 warn 日志逐字保留)。

---

### Task 1: profiles 文档反序列化面的穿越拒绝回归钉

**Files:**

- Modify: `backend/nyanpasu-config/src/profile/tests/validation.rs`(文件尾部追加一个测试)

**Interfaces:**

- Consumes: `ManagedProfilePath::new` 既有校验(path.rs:30-40)、`Profiles` 的 serde wire 格式(validation.rs:181-202 先例)。
- Produces: 回归钉——未来任何人把 `Deserialize` 改成不经 `new()` 的实现(如 derive transparent)会立刻红。

- [ ] **Step 1: 写回归测试**

在 `validation.rs` 文件末尾追加(imports 复用文件既有的 `serde_yaml_ng`/`Profiles`/`ManagedProfilePath`):

```rust
/// T06A 回归钉:不可信文档面(磁盘 profiles.yaml)在反序列化时就必须拒绝
/// 穿越型 managed 路径。防御在 ManagedProfilePath::new(path.rs:30-40),
/// Deserialize 委托 new(path.rs:60-67);本测试锁死该委托关系。
#[test]
fn profiles_document_with_traversal_managed_path_fails_to_deserialize() {
    let yaml = r#"items:
  - uid: evil
    name: evil
    type: transform
    transform:
      type: overlay
      source:
        type: local
        binding:
          type: managed
          file: ../escape.yaml
"#;
    let error = serde_yaml_ng::from_str::<Profiles>(yaml).unwrap_err();
    assert!(
        error.to_string().contains("parent"),
        "error must name the traversal rejection: {error}"
    );

    // 直接类型面同理(serde 字符串标量 → ManagedProfilePath)。
    assert!(serde_yaml_ng::from_str::<ManagedProfilePath>("../escape.yaml").is_err());
}
```

- [ ] **Step 2: 运行——预期直接 PASS(这是回归钉,不是新行为)**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p nyanpasu-config profiles_document_with_traversal`
Expected: PASS(1 passed)。若 FAIL,说明防御被移除或 wire 格式变了——停下按 systematic-debugging 排查,不得改断言迁就。

- [ ] **Step 3: Commit**

```powershell
git add backend/nyanpasu-config/src/profile/tests/validation.rs
git commit -m "test(nyanpasu-config): pin traversal rejection at profiles deserialization surface"
```

---

### Task 2: Mirror 同步阻塞 I/O 移入 spawn_blocking

**Files:**

- Modify: `backend/tauri/src/state/profiles/actor.rs`(`ExternalFileChanged` 分支,现 824-856 行的 `if *mode == ExternalMode::Mirror { ... }` 块)

**Interfaces:**

- Consumes: `state.fs: Arc<dyn ProfileFsPort>`(clone 进闭包)、`Self::validate_fetched_content(&ProfileDefinition, &str) -> Result<(), String>`(actor.rs:325,关联函数,闭包内可用 `Self::` 调用)。
- Produces: 行为不变(镜像成功→继续 commit updated_at;任一步失败→warn + 提前返回,消息逐字同旧);唯一变化 = 三步 I/O 在 blocking 池线程执行。

- [ ] **Step 1: 先跑基线(确认重构前绿)**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu external_mirror_change_syncs`
Expected: PASS(1 passed)。

- [ ] **Step 2: 等价重构**

把 `if *mode == ExternalMode::Mirror { ... }` 整块(现 824-856 行)替换为:

```rust
                if *mode == ExternalMode::Mirror {
                    // T06A: keep the read→validate→write mirror sync off the
                    // async actor thread. The handler awaits the blocking task,
                    // so per-actor message ordering is unchanged.
                    let fs = state.fs.clone();
                    let target = target.clone();
                    let mirror_file = materialized.file.clone();
                    let definition = item.definition.clone();
                    let log_uid = uid.clone();
                    let synced = tokio::task::spawn_blocking(move || {
                        let content = match fs.read_external(&target) {
                            Ok(content) => content,
                            Err(error) => {
                                tracing::warn!(
                                    uid = %log_uid,
                                    target = %target,
                                    error = %error,
                                    "failed to read changed external profile"
                                );
                                return false;
                            }
                        };
                        if let Err(message) =
                            Self::validate_fetched_content(&definition, &content)
                        {
                            tracing::warn!(
                                uid = %log_uid,
                                target = %target,
                                error = %message,
                                "changed external profile failed validation"
                            );
                            return false;
                        }
                        if let Err(error) = fs.write_atomic(&mirror_file, &content) {
                            tracing::warn!(
                                uid = %log_uid,
                                path = %mirror_file,
                                error = %error,
                                "failed to mirror changed external profile"
                            );
                            return false;
                        }
                        true
                    })
                    .await
                    .unwrap_or_else(|join_error| {
                        tracing::warn!(
                            uid = %uid,
                            error = %join_error,
                            "mirror sync task failed to run"
                        );
                        false
                    });
                    if !synced {
                        return Ok(());
                    }
                }
```

要点:闭包捕获全部为 owned clone(`Arc` clone + `ExternalProfilePath`/`ManagedProfilePath`/`ProfileDefinition` 均 `Clone`),不跨 `await` 持有 `snapshot` 派生借用;后续 `Self::run_write(...)` 段(858 行起)原样不动。

- [ ] **Step 3: 验证等价 + 全量绿**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu external_mirror`
Expected: PASS(`external_mirror_change_syncs_copy_and_bumps_updated_at` 过;smoke 测试保持 ignored)。
Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 全绿(209+ passed,0 failed)。

- [ ] **Step 4: Commit**

```powershell
git add backend/tauri/src/state/profiles/actor.rs
git commit -m "refactor(tauri): run external mirror sync in spawn_blocking"
```

---

### Task 3: RuntimeBuilder golden snapshot 文件套件

**Files:**

- Create: `backend/tauri/src/enhance/golden.rs`(`#[cfg(test)]` 专用模块)
- Create: `backend/tauri/src/enhance/fixtures/golden/`(4 个期望 YAML,由 bless 流生成后入库)
- Modify: `backend/tauri/src/enhance/mod.rs`(mod 声明区追加 `#[cfg(test)] mod golden;`)

**Interfaces:**

- Consumes: `RuntimeBuilder::build` / `RuntimeBuildInput` / `FsProfileContentSource::new(PathBuf)` / `EnhanceScriptRunner::new()`(均 `crate::enhance` 顶层 pub use);`ClashGuardOverrides`(`nyanpasu_config::clash::config::overrides`,固定 secret 经反序列化构造);域类型(`nyanpasu_config::profile::*`)。
- Produces: `src/enhance/fixtures/golden/{composition_global_chain,builtin_mihomo,builtin_clash_rs,whitelist_on}.yaml` 基线文件 + 4 个 `golden_` 测试。**T07 的切换回归安全网:切换后本套件必须原样全绿。**

- [ ] **Step 1: mod.rs 挂模块**

在 `backend/tauri/src/enhance/mod.rs` 的 mod 声明区(`mod utils;` 之后)追加:

```rust
#[cfg(test)]
mod golden;
```

- [ ] **Step 2: 写 golden.rs(完整文件)**

```rust
//! Golden snapshot suite over RuntimeBuilder + real adapters (PR-3 T06A).
//! Locks the pre-switch behavior baseline for the T07 `Config::generate()`
//! cutover: identical inputs must keep producing structurally identical
//! final configs. Re-bless intentionally with:
//!   GOLDEN_BLESS=1 cargo test -p clash-nyanpasu golden_
//! Determinism rules: fixed `secret` (default is a random uuid) and tun off
//! (`windows_fake_ip_filter` is platform-dependent).

use std::{path::PathBuf, sync::Arc};

use nyanpasu_config::{
    application::ClashCore,
    clash::config::overrides::ClashGuardOverrides,
    profile::{
        CompositionConfig, ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath,
        MaterializedFile, OverlayTransform, ProfileDefinition, ProfileId, ProfileItem,
        ProfileMetadata, ProfileSource, Profiles, ScriptRuntime, ScriptTransform,
        TransformDefinition,
    },
    runtime::executor::ResolvedPortBindings,
};

use super::{EnhanceScriptRunner, FsProfileContentSource, RuntimeBuildInput, RuntimeBuilder};

const SUB_A: &str = "proxies:\n  - name: a1\n    type: ss\n    server: a.example.com\n    port: 443\n";
const SUB_B: &str = "proxies:\n  - name: b1\n    type: vmess\n    server: b.example.com\n    port: 8080\n";
const BUILD_GROUPS: &str =
    "proxy-groups:\n  - name: Auto\n    type: select\n    proxies:\n      - a1\n      - b1\n";
const GLOBAL_FIX: &str = "append__rules:\n  - MATCH,DIRECT\n";
const BASE_CONFIG: &str = "mode: direct\nproxies: []\nextra-key: keep\ncustom-field: 1\n";

/// 固定 secret 的 guard overrides(Default 的 secret 是随机 uuid,
/// nyanpasu-config overrides/mod.rs:75;先例 executor tests/builtin.rs:19-24)。
fn fixed_overrides() -> ClashGuardOverrides {
    serde_yaml::from_str(
        "log-level: info\nallow-lan: false\nmode: rule\nsecret: golden-secret\nunified-delay: true\ntcp-concurrent: true\nipv6: false\n",
    )
    .unwrap()
}

fn golden_input(profiles: Profiles) -> RuntimeBuildInput {
    let mut input = RuntimeBuildInput {
        profiles: Arc::new(profiles),
        clash: Default::default(),
        app: Default::default(),
        resolved_ports: ResolvedPortBindings {
            mixed_port: 7890,
            port: Some(7891),
            socks_port: Some(7892),
            external_controller: Some("127.0.0.1:9090".to_string()),
        },
    };
    input.clash.overrides = fixed_overrides();
    input.clash.enable_tun_mode = false; // windows_fake_ip_filter 平台相关
    input.app.enable_builtin_enhanced = false; // 各场景按需显式开启
    input
}

fn managed(name: &str) -> MaterializedFile {
    MaterializedFile {
        file: ManagedProfilePath::new(name).unwrap(),
        updated_at: None,
    }
}

fn metadata(name: &str) -> ProfileMetadata {
    ProfileMetadata {
        name: name.into(),
        desc: None,
    }
}

fn file_config(uid: &str, file: &str, transforms: &[&str]) -> ProfileItem {
    ProfileItem {
        uid: ProfileId(uid.into()),
        metadata: metadata(uid),
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: managed(file),
                    },
                },
                transforms: transforms.iter().map(|t| ProfileId((*t).into())).collect(),
            }),
        },
    }
}

fn overlay(uid: &str, file: &str) -> ProfileItem {
    ProfileItem {
        uid: ProfileId(uid.into()),
        metadata: metadata(uid),
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(OverlayTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: managed(file),
                    },
                },
            }),
        },
    }
}

fn composition(uid: &str, base: Option<&str>, extend: &[&str], transforms: &[&str]) -> ProfileItem {
    ProfileItem {
        uid: ProfileId(uid.into()),
        metadata: metadata(uid),
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::Composition(CompositionConfig {
                base: base.map(|b| ProfileId(b.into())),
                extend_proxies_from: extend.iter().map(|e| ProfileId((*e).into())).collect(),
                transforms: transforms.iter().map(|t| ProfileId((*t).into())).collect(),
            }),
        },
    }
}

fn build_to_yaml(input: &RuntimeBuildInput, dir: &std::path::Path) -> serde_yaml::Value {
    let content = FsProfileContentSource::new(dir.to_path_buf());
    let scripts = EnhanceScriptRunner::new().unwrap();
    let artifact = RuntimeBuilder::build(input, &content, &scripts).expect("golden build");
    serde_yaml::to_value(&*artifact.final_config).unwrap()
}

fn assert_matches_fixture(actual: &serde_yaml::Value, name: &str) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/enhance/fixtures/golden");
    let path = dir.join(name);
    let rendered = serde_yaml::to_string(actual).unwrap();
    if std::env::var_os("GOLDEN_BLESS").is_some() {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, &rendered).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!("missing golden fixture {name}; run with GOLDEN_BLESS=1 to create it")
    });
    let expected: serde_yaml::Value = serde_yaml::from_str(&expected).unwrap();
    assert_eq!(
        actual, &expected,
        "golden drift for {name}; re-bless with GOLDEN_BLESS=1 only if the change is intentional"
    );
}

/// clean-seed Composition(base=None)+ 成员贡献 proxies + 组合内 overlay
/// + global chain overlay(append__rules 落在缺失键上 → 按执行器语义处理)。
#[test]
fn golden_clean_seed_composition_with_global_chain() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("sub_a.yaml"), SUB_A).unwrap();
    std::fs::write(temp.path().join("sub_b.yaml"), SUB_B).unwrap();
    std::fs::write(temp.path().join("build_groups.yaml"), BUILD_GROUPS).unwrap();
    std::fs::write(temp.path().join("global_fix.yaml"), GLOBAL_FIX).unwrap();

    let mut profiles = Profiles::default();
    profiles.append_item(file_config("sub-a", "sub_a.yaml", &[]));
    profiles.append_item(file_config("sub-b", "sub_b.yaml", &[]));
    profiles.append_item(overlay("build-groups", "build_groups.yaml"));
    profiles.append_item(overlay("global-fix", "global_fix.yaml"));
    profiles.append_item(composition(
        "all",
        None,
        &["sub-a", "sub-b"],
        &["build-groups"],
    ));
    profiles.set_current(Some(ProfileId("all".into())));
    profiles.global_transforms = vec![ProfileId("global-fix".into())];

    let input = golden_input(profiles);
    let yaml = build_to_yaml(&input, temp.path());
    assert_matches_fixture(&yaml, "composition_global_chain.yaml");
}

/// builtin 门控 golden(Mihomo:hy_alpn + meta_guard + config_fixer,真 boa)。
#[test]
fn golden_builtin_gating_mihomo() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("base.yaml"), BASE_CONFIG).unwrap();

    let mut profiles = Profiles::default();
    profiles.append_item(file_config("base", "base.yaml", &[]));
    profiles.set_current(Some(ProfileId("base".into())));

    let mut input = golden_input(profiles);
    input.app.enable_builtin_enhanced = true;
    input.app.core = ClashCore::Mihomo;
    let yaml = build_to_yaml(&input, temp.path());
    assert_matches_fixture(&yaml, "builtin_mihomo.yaml");
}

/// builtin 门控 golden(ClashRs:config_fixer + clash_rs_comp,真 boa + 真 lua)。
#[test]
fn golden_builtin_gating_clash_rs() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("base.yaml"), BASE_CONFIG).unwrap();

    let mut profiles = Profiles::default();
    profiles.append_item(file_config("base", "base.yaml", &[]));
    profiles.set_current(Some(ProfileId("base".into())));

    let mut input = golden_input(profiles);
    input.app.enable_builtin_enhanced = true;
    input.app.core = ClashCore::ClashRs;
    let yaml = build_to_yaml(&input, temp.path());
    assert_matches_fixture(&yaml, "builtin_clash_rs.yaml");
}

/// whitelist-on:enable_clash_fields=true 时未知键被过滤。
#[test]
fn golden_whitelist_on_filters_unknown_keys() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("base.yaml"), BASE_CONFIG).unwrap();

    let mut profiles = Profiles::default();
    profiles.append_item(file_config("base", "base.yaml", &[]));
    profiles.set_current(Some(ProfileId("base".into())));

    let mut input = golden_input(profiles);
    input.clash.enable_clash_fields = true;
    let yaml = build_to_yaml(&input, temp.path());
    assert!(
        yaml.get("extra-key").is_none(),
        "whitelist must drop unknown keys"
    );
    assert_matches_fixture(&yaml, "whitelist_on.yaml");
}
```

- [ ] **Step 3: bless(首铸期望文件)**

Run(PowerShell):

```powershell
$env:GOLDEN_BLESS = '1'
cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu golden_
Remove-Item Env:GOLDEN_BLESS
```

Expected: 4 个 `golden_` 测试 PASS,`backend/tauri/src/enhance/fixtures/golden/` 出现 4 个 yaml。若编译错(域类型字段名/导入路径与实测漂移),按编译器提示修 golden.rs,不改生产代码。

- [ ] **Step 4: 检视 blessed 基线(人工 sanity)**

逐个打开 4 个 fixtures 检查:

- `composition_global_chain.yaml`:`proxies` 含 a1+b1(顺序 sub-a→sub-b)、`proxy-groups` 含 Auto、guard 四键在位(`mixed-port: 7890`/`port: 7891`/`socks-port: 7892`/`external-controller`)、`secret: golden-secret`、`mode: rule`(guard 覆盖)。
- `builtin_mihomo.yaml` vs `builtin_clash_rs.yaml`:两者内容有差(不同 builtin 组合生效);均无 `extra-key` 丢失(whitelist off)。
- `whitelist_on.yaml`:无 `extra-key`/`custom-field`。

任一不符 → 停,按 systematic-debugging 查(多半是场景装配错,不是 executor 错)。

- [ ] **Step 5: 不带 bless 重跑两遍(确定性验证)**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu golden_`(连续两次)
Expected: 两次均 4 passed(期望文件比对,无随机漂移)。

- [ ] **Step 6: 全量绿 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 全绿。

```powershell
git add backend/tauri/src/enhance/mod.rs backend/tauri/src/enhance/golden.rs backend/tauri/src/enhance/fixtures/golden/
git commit -m "test(tauri): add RuntimeBuilder golden snapshot suite"
```

---

### Task 4: 契约回写 task.md T06A 卡

**Files:**

- Modify: `docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md`(T06A 卡尾部)

- [ ] **Step 1: 在 T06A 卡「单独 plan 时读」行之后追加执行修正块**

```markdown
**2026-07-06 执行修正(T06A 实物)**:

- 第 1 项按实物收缩:`..`/穿越拒绝防御与构造侧测试**原已在位**(nyanpasu-config path.rs:30-40/60-67 + validation.rs:166-171;T06 评审处置④的「反序列化侧缺失」表述与实物不符)——本卡实际交付 = profiles 文档反序列化面的回归钉测试(validation.rs 尾部)。
- golden 套件实名:`enhance/golden.rs` + `enhance/fixtures/golden/{composition_global_chain,builtin_mihomo,builtin_clash_rs,whitelist_on}.yaml`;重铸 `GOLDEN_BLESS=1 cargo test -p clash-nyanpasu golden_`;确定性 = 固定 `secret: golden-secret` + tun off。**T07 切换后本套件须原样全绿,改 fixtures = 行为回归须勘误。**
- Mirror 同步已移入 `spawn_blocking`(actor.rs `ExternalFileChanged` Mirror 分支;消息顺序不变,warn 日志逐字保留)。
```

- [ ] **Step 2: Commit**

```powershell
git add docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md
git commit -m "docs(pr3): record T06A execution addenda in task card"
```

---

## Self-Review 结论(plan 作者自查)

- 覆盖:卡内三项全部有 Task(1→Task1、2→Task2、3→Task3)+ §5.3 回写(Task4)。
- 无占位符:所有代码块完整可粘贴;golden 期望内容不可预先写死,采用显式 bless 流 + Step 4 人工 sanity 清单,非占位。
- 类型一致性:`golden_input`/`file_config`/`overlay`/`composition` 的字段名与 definition.rs:26-73、profiles.rs:20-28 实测一致;`fixed_overrides` YAML 与 executor builtin.rs:21 逐字节相同。
