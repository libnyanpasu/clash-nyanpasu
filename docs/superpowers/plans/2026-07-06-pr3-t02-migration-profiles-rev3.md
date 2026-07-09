# PR-3 T02 — migration V2 `profiles` revision 3(legacy → clean schema)Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 注册 `profiles` 模块 revision 3 迁移步骤,把 legacy `profiles.yaml`(post-rev2 形态)在 Value 层转换为 nyanpasu-config clean schema;强制 `.bak` 备份;任何歧义显式失败(带 uid + 字段路径);幂等可重入。

**Architecture:** 纯函数 `migrate_clean_schema(Mapping) -> Result<Mapping, CleanSchemaError>` 做全部 Value 层转换(可无 fs 单测,沿用 rev2 的 `migrate_profile_data` 先例);`run()` 负责 读取 → `.bak` → 转换 → 反序列化为新 `Profiles` → `validate()` → 带头注释原子写回(design §14.4 执行顺序)。`rollback()` = 恢复 `.bak`。

**Tech Stack:** serde_yaml_ng(crate 内别名 `serde_yaml`)、`nyanpasu_config::profile::*`(typed 校验背书)、`core/migration/fs::atomic_write`、既有 `MigrationStep`/`ModuleMigrator` 框架(`core/migration/mod.rs:103,115`)。

## Global Constraints(task.md §0 + design D3)

- rev3 的输入契约 = **post-rev2 形态**(runner 按 revision 串行执行:rev1 已修顶层 null、rev2 已把 `!script x` 展开为 `type: script` + `script_type: x`)。
- 新类型永不解析旧 wire(铁律 2);Value 层完成全部形状转换后才 typed 反序列化。
- 强制 `.bak`;歧义显式失败(`CleanSchemaError{uid, field_path, reason}`),**不静默丢弃任何键**。
- 幂等重入:已是 clean schema 的文件 no-op(revision 账本之外的第二道保险)。
- 每个 commit `cargo build` + `cargo test` 绿;conventional commits。

## 基线事实(2026-07-06 实测)

- 现有步骤:`profiles/null_value`(rev1,`modules/profiles.rs:47`)、`profiles/script_newtype`(rev2,`:114`);`STEPS` 数组 `:14`;`MIGRATOR` 注册于 `registry.rs:6`。
- 原子写:`write_profiles_atomic(path, mapping, prefix)`(`modules/profiles.rs:183`),头注释 `# Profiles Config for Clash Nyanpasu`。
- e2e 样板:`runner.rs:372` `real_1_6_1_fixture_migrates_to_2_0_shape`——**本卡必改**:`fixtures/v2_0_expected/profiles.yaml` 变为 clean schema,`applied_revision` 断言 2→3。
- 新 schema 序列化形状(实测 `nyanpasu-config/src/profile/`):`ProfileItem{uid, ..metadata(flatten: name, desc?), ..definition(flatten: type=config|transform + config|transform 字段)}`;`ConfigDefinition` tag=`type`(file|composition);`TransformDefinition` tag=`type`(overlay|script);`ProfileSource` tag=`type`(local{binding}|remote{..materialized(flatten), url, option, subscription});`LocalBinding` tag=`type`(managed{..materialized}|external{..materialized, target, mode});`MaterializedFile{file, updated_at?(int ts)}`。
- `Profiles.current`/`global_transforms` 空时省略键;`valid` 恒序列化(default = [dns, unified-delay, tcp-concurrent]);`items` 序列化为序列、重复 uid 反序列化报错。

## 映射规则总表(R1–R15,每条至少一个测试;规则源 = guide §6 / clean-design §14 + 本次 legacy 实测补遗)

| #   | 规则                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R1  | `type` 映射:`local→config/file(source: local)`、`remote→config/file(source: remote)`、`merge→transform/overlay(source: local)`、`script→transform/script(source: local, runtime=script_type)`;`type` 缺失或未知值 → **FAIL**`{uid, "type"}`                                                                                                                                                                                                                                                         |
| R2  | shared 字段:`uid`(缺失→FAIL)、`name→metadata.name`(缺失→FAIL)、`desc: null→省略`,`desc: ""→保留 ""`;`updated`(整数秒)→ `MaterializedFile.updated_at` 同值,缺失→省略                                                                                                                                                                                                                                                                                                                                 |
| R3  | `file`:缺失→FAIL`{uid,"file"}`;为 HTTP/HTTPS URL → 规则 R3b(design §14.2):定义仍按旧 type,Source 改 Remote,URL 入 `source.url`,`file` 重新生成为 `{uid}.yaml`(config/overlay)或 `{uid}.js`/`{uid}.lua`(script,按 runtime),option 用 R5「absent」值;否则必须是合法相对托管路径(拒绝绝对路径/`..`/`.`),违规→FAIL                                                                                                                                                                                      |
| R4  | `remote` 项 `url` 缺失→FAIL`{uid,"url"}`;scheme 非 http/https 由收尾 `validate()` 拒绝                                                                                                                                                                                                                                                                                                                                                                                                              |
| R5  | `remote.option`(**legacy 语义实测补遗,两种缺省不同**):`option` 键整体缺失 → `{with_proxy: false, self_proxy: true, update_interval_minutes: 120}`(旧 `Default` self_proxy=Some(true)+运行期 with_proxy None→false);`option` 存在 → `with_proxy = 旧值∥false`、`self_proxy = 旧值∥false`(旧 `apply_default` 对 None 双双置 false,remote.rs:474-479)、`update_interval → update_interval_minutes`(缺失→120,**为 0→FAIL**`{uid,"option.update_interval"}`)、`user_agent` 有则携带;option 内未知键→FAIL |
| R6  | `remote.extra → subscription`:整体缺失→省略;`upload/download/total` 原样携带;`expire: 0 → 省略(None)`,非 0 原样;extra 内未知键→FAIL                                                                                                                                                                                                                                                                                                                                                                 |
| R7  | 顶层 `chain → global_transforms` 原样(空→省略键);成员非 Transform 由 `validate()` 拒绝(TransformTargetNotTransform → FAIL)                                                                                                                                                                                                                                                                                                                                                                          |
| R8  | `local/remote` 项 `chain`(读 `chain`,无则读别名 `chains`;**两键同存→FAIL**)→ `config.transforms` 原样;`merge/script` 项出现 `chain/chains` → 未知键 FAIL                                                                                                                                                                                                                                                                                                                                            |
| R9  | **(spec 补遗,legacy `LocalProfile.symlinks: Option<PathBuf>` 未列入 guide §6)** `local.symlinks` 存在且非 null → `binding = external{ ..materialized, target: <symlinks 值>, mode: symlink }`;target 非绝对路径→FAIL`{uid,"symlinks"}`;null→按 managed 处理                                                                                                                                                                                                                                         |
| R10 | `current`(rev1 后可能形态:缺失/字符串/序列):缺失或 `[]`→省略;`"a"` 或 `[a]`→`current: a`;`[a,b,…]`→合成 `CompositionConfig{base: Some(a), extend_proxies_from: [b,…], transforms: []}`,uid 用**确定性**碰撞安全生成(`combined-profile`,占用则 `combined-profile-2`…),`name: "Combined Profile"`,`current` 指向之,顺序原样;任一成员缺失或映射后非直接 FileConfig → FAIL`{member_uid,"current"}`                                                                                                      |
| R11 | `valid` 原样携带;缺失→由新类型默认值补齐(typed round-trip 恒写出)                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| R12 | 顶层未知键→FAIL`{None, key}`;item 未知键→FAIL`{uid, key}`(白名单见各 Task 代码)                                                                                                                                                                                                                                                                                                                                                                                                                     |
| R13 | 重复 uid → typed 反序列化拒绝 → FAIL(背书测试)                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| R14 | 幂等:`is_clean_schema` 检测到 items 均为 `config/transform`(或无任何旧标记)→ run() no-op                                                                                                                                                                                                                                                                                                                                                                                                            |
| R15 | `.bak`:实际转换前把当前原文写 `profiles.yaml.bak`(覆盖旧 .bak);no-op 时不写;写回带头注释 `# Profiles Config for Clash Nyanpasu`                                                                                                                                                                                                                                                                                                                                                                     |

> R5/R9 是本次 plan 侦察发现的 spec 缺口,Task 7 回写 task.md T02 卡(§5.3 契约规则)。

---

### Task 1: rev3 骨架 + 幂等检测 + detect_baseline

**Files:**

- Modify: `backend/tauri/src/core/migration/modules/profiles.rs`(新增 step 结构 + `STEPS` 3 元素 + `detect_baseline`)

**Interfaces:**

- Produces: `MigrateProfilesCleanSchema`(`id = "profiles/clean_schema"`, `revision = 3`)、`fn is_clean_schema(&Mapping) -> bool`(Task 5 run() 用)。

- [ ] **Step 1: 写失败测试(幂等检测)**

在 `modules/profiles.rs` 的 `#[cfg(test)] mod tests` 内追加:

```rust
    const CLEAN_SAMPLE: &str = r#"valid:
- dns
items:
- uid: aaa
  name: A
  type: config
  config:
    type: file
    source:
      type: local
      binding:
        type: managed
        file: aaa.yaml
"#;

    #[test]
    fn clean_schema_detection() {
        let clean: Mapping = serde_yaml::from_str(CLEAN_SAMPLE).unwrap();
        assert!(is_clean_schema(&clean));
        let legacy: Mapping = serde_yaml::from_str(MIGRATED_SAMPLE).unwrap();
        assert!(!is_clean_schema(&legacy));
        // 空文档没有任何旧标记,视为 clean(无事可做)
        assert!(is_clean_schema(&Mapping::new()));
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu clean_schema_detection`
Expected: FAIL(`is_clean_schema` 未定义,编译错误)。

- [ ] **Step 3: 实现骨架**

在 rev2 结构体之后追加(run/rollback 本 Task 先占位为 no-op 转发,Task 5 填实):

```rust
static CLEAN_SCHEMA: MigrateProfilesCleanSchema = MigrateProfilesCleanSchema;
// STEPS 改为:
static STEPS: [&dyn MigrationStep; 3] = [&NULL_VALUE, &SCRIPT_NEWTYPE, &CLEAN_SCHEMA];

#[derive(Debug, Clone, Copy)]
pub struct MigrateProfilesCleanSchema;

impl MigrationStep for MigrateProfilesCleanSchema {
    fn id(&self) -> &'static str {
        "profiles/clean_schema"
    }

    fn module(&self) -> &'static str {
        "profiles"
    }

    fn revision(&self) -> u64 {
        3
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_2_0_0
    }

    fn name(&self) -> &'static str {
        "MigrateProfilesCleanSchema"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        run_clean_schema(ctx)
    }

    fn rollback(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        rollback_clean_schema(ctx)
    }
}

/// New-schema marker: every item is a `config`/`transform` definition. A doc
/// with zero legacy markers (no legacy item types, no top-level `chain`) has
/// nothing to migrate and counts as clean.
fn is_clean_schema(doc: &Mapping) -> bool {
    if doc.contains_key("chain") {
        return false;
    }
    match doc.get("items").and_then(Value::as_sequence) {
        None => true,
        Some(items) => items.iter().all(|item| {
            item.as_mapping()
                .and_then(|item| item.get("type"))
                .and_then(Value::as_str)
                .is_some_and(|ty| matches!(ty, "config" | "transform"))
        }),
    }
}

// Task 5 填实;先让骨架编译通过
fn run_clean_schema(_ctx: &mut Ctx) -> anyhow::Result<()> {
    Ok(())
}

fn rollback_clean_schema(_ctx: &mut Ctx) -> anyhow::Result<()> {
    Ok(())
}
```

`detect_baseline`(`modules/profiles.rs:23`)的条件改为:

```rust
        if needs_null_value_migration(&profiles)
            || needs_script_newtype_migration(&profiles)
            || !is_clean_schema(&profiles)
        {
            Ok(0)
        } else {
            Ok(current_revision())
        }
```

- [ ] **Step 4: 跑测试确认通过 + registry 排序测试仍绿**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu clean_schema_detection module_steps_are_sorted_by_revision`
Expected: 两者 PASS。

- [ ] **Step 5: Commit**

```bash
git add backend/tauri/src/core/migration/modules/profiles.rs
git commit -m "feat(migration): scaffold profiles revision 3 clean-schema step"
```

---

### Task 2: 错误类型 + remote 项映射(R2–R6)

**Files:**

- Modify: `backend/tauri/src/core/migration/modules/profiles.rs`

**Interfaces:**

- Produces: `CleanSchemaError{uid, field_path, reason}`、`fn migrate_item(item: Mapping) -> Result<Mapping, CleanSchemaError>`(Task 3 扩展 local/merge/script 分支,Task 4 调用)。

- [ ] **Step 1: 写失败测试(remote 全字段 + 两种 option 缺省 + expire:0 + 失败路径)**

```rust
    fn item(yaml: &str) -> Mapping {
        serde_yaml::from_str(yaml).unwrap()
    }

    fn migrated(yaml: &str) -> Mapping {
        migrate_item(item(yaml)).unwrap()
    }

    fn yaml_eq(actual: &Mapping, expected: &str) {
        let expected: Mapping = serde_yaml::from_str(expected).unwrap();
        pretty_assertions::assert_eq!(
            serde_yaml::to_value(actual).unwrap(),
            serde_yaml::to_value(&expected).unwrap()
        );
    }

    #[test]
    fn remote_item_full_mapping() {
        let out = migrated(
            r#"uid: r1
type: remote
name: Cloud
file: r1.yaml
desc: hello
updated: 1758110672
url: https://example.com
extra: {upload: 1, download: 2, total: 3, expire: 1769123200}
option: {with_proxy: false, self_proxy: true, update_interval: 1440, user_agent: ua}
chain: [t1, t2]
"#,
        );
        yaml_eq(
            &out,
            r#"uid: r1
name: Cloud
desc: hello
type: config
config:
  type: file
  source:
    type: remote
    file: r1.yaml
    updated_at: 1758110672
    url: https://example.com
    option: {user_agent: ua, with_proxy: false, self_proxy: true, update_interval_minutes: 1440}
    subscription: {upload: 1, download: 2, total: 3, expire: 1769123200}
  transforms: [t1, t2]
"#,
        );
    }

    #[test]
    fn remote_option_absent_defaults_to_legacy_effective_values() {
        // 旧 Default: self_proxy Some(true);运行期 with_proxy None→false
        let out = migrated("uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\n");
        let option = out["config"]["source"]["option"].as_mapping().unwrap();
        assert_eq!(option["with_proxy"], Value::Bool(false));
        assert_eq!(option["self_proxy"], Value::Bool(true));
        assert_eq!(option["update_interval_minutes"], Value::from(120));
    }

    #[test]
    fn remote_option_partial_uses_apply_default_semantics() {
        // 旧 apply_default: 键存在时 None→false(with_proxy 与 self_proxy 都是)
        let out = migrated(
            "uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\noption: {update_interval: 60}\n",
        );
        let option = out["config"]["source"]["option"].as_mapping().unwrap();
        assert_eq!(option["with_proxy"], Value::Bool(false));
        assert_eq!(option["self_proxy"], Value::Bool(false));
        assert_eq!(option["update_interval_minutes"], Value::from(60));
    }

    #[test]
    fn remote_extra_expire_zero_becomes_absent() {
        let out = migrated(
            "uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\nextra: {upload: 0, download: 0, total: 0, expire: 0}\n",
        );
        let subscription = out["config"]["source"]["subscription"].as_mapping().unwrap();
        assert!(!subscription.contains_key("expire"));
        assert_eq!(subscription["upload"], Value::from(0));
    }

    #[test]
    fn remote_failures_carry_uid_and_field() {
        let err = migrate_item(item("uid: r1\ntype: remote\nname: A\nfile: r1.yaml\n")).unwrap_err();
        assert_eq!(err.uid.as_deref(), Some("r1"));
        assert_eq!(err.field_path, "url");

        let err = migrate_item(item(
            "uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\noption: {update_interval: 0}\n",
        ))
        .unwrap_err();
        assert_eq!(err.field_path, "option.update_interval");

        let err = migrate_item(item(
            "uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\noption: {bogus: 1}\n",
        ))
        .unwrap_err();
        assert_eq!(err.field_path, "option.bogus");

        let err = migrate_item(item(
            "uid: r1\ntype: remote\nname: A\nfile: r1.yaml\nurl: https://e.com\nwhatever: 1\n",
        ))
        .unwrap_err();
        assert_eq!(err.field_path, "whatever");
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu remote_item -- --nocapture`
Expected: FAIL(`migrate_item`/`CleanSchemaError` 未定义)。

- [ ] **Step 3: 实现(错误类型 + item 公共骨架 + remote 分支)**

```rust
/// Explicit migration failure: design D3 requires uid + field path, never a
/// silent drop.
#[derive(Debug, thiserror::Error)]
#[error("profiles clean-schema migration failed (uid={uid:?}, field={field_path}): {reason}")]
pub struct CleanSchemaError {
    pub uid: Option<String>,
    pub field_path: String,
    pub reason: String,
}

impl CleanSchemaError {
    fn new(uid: Option<&str>, field_path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            uid: uid.map(str::to_owned),
            field_path: field_path.into(),
            reason: reason.into(),
        }
    }
}

fn str_value(v: &Value) -> Option<&str> {
    v.as_str()
}

/// Maps one legacy (post-rev2) item onto the clean `ProfileItem` wire shape.
fn migrate_item(item: Mapping) -> Result<Mapping, CleanSchemaError> {
    let uid = item
        .get("uid")
        .and_then(str_value)
        .map(str::to_owned)
        .ok_or_else(|| CleanSchemaError::new(None, "uid", "missing item uid"))?;
    let fail = |field: &str, reason: &str| CleanSchemaError::new(Some(&uid), field, reason);

    let ty = item
        .get("type")
        .and_then(str_value)
        .ok_or_else(|| fail("type", "missing or non-string legacy type"))?
        .to_owned();

    // ---- shared metadata + materialized file --------------------------------
    let name = item
        .get("name")
        .and_then(str_value)
        .ok_or_else(|| fail("name", "missing profile name"))?
        .to_owned();
    let desc = match item.get("desc") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err(fail("desc", "desc must be a string or null")),
    };
    let file = item
        .get("file")
        .and_then(str_value)
        .ok_or_else(|| fail("file", "missing materialized file"))?
        .to_owned();
    let updated = item.get("updated").cloned();

    // ---- per-type allowed keys (R12): anything else is an explicit failure --
    let allowed: &[&str] = match ty.as_str() {
        "remote" => &["uid", "type", "name", "desc", "file", "updated", "url", "extra", "option", "chain", "chains"],
        "local" => &["uid", "type", "name", "desc", "file", "updated", "symlinks", "chain", "chains"],
        "merge" => &["uid", "type", "name", "desc", "file", "updated"],
        "script" => &["uid", "type", "name", "desc", "file", "updated", "script_type"],
        other => return Err(fail("type", &format!("unknown legacy type `{other}`"))),
    };
    for key in item.keys() {
        let Some(key) = key.as_str() else {
            return Err(fail("<non-string key>", "item keys must be strings"));
        };
        if !allowed.contains(&key) {
            return Err(fail(key, "unknown legacy field; refusing to drop it silently"));
        }
    }

    // ---- scoped chain (R8) ---------------------------------------------------
    let transforms = match (item.get("chain"), item.get("chains")) {
        (Some(_), Some(_)) => return Err(fail("chain", "both `chain` and alias `chains` present")),
        (Some(v), None) | (None, Some(v)) => match v {
            Value::Sequence(seq) => Some(seq.clone()),
            Value::Null => None,
            _ => return Err(fail("chain", "chain must be a sequence")),
        },
        (None, None) => None,
    };

    // ---- source (R3/R3b/R9) --------------------------------------------------
    let is_url = file.starts_with("http://") || file.starts_with("https://");
    let materialized_file = if is_url {
        let ext = match ty.as_str() {
            "script" => match item.get("script_type").and_then(str_value) {
                Some("lua") => "lua",
                _ => "js",
            },
            _ => "yaml",
        };
        format!("{uid}.{ext}")
    } else {
        validate_managed_relative(&uid, &file)?;
        file.clone()
    };

    let mut materialized = Mapping::new();
    materialized.insert("file".into(), Value::String(materialized_file));
    if let Some(updated) = updated {
        match &updated {
            Value::Number(_) => {
                materialized.insert("updated_at".into(), updated);
            }
            Value::Null => {}
            _ => return Err(fail("updated", "updated must be an integer timestamp")),
        }
    }

    let source = if ty == "remote" || is_url {
        // R3b: legacy URL-in-file converts to a Remote source regardless of type
        let url = if ty == "remote" {
            item.get("url")
                .and_then(str_value)
                .ok_or_else(|| fail("url", "missing subscription url"))?
                .to_owned()
        } else {
            file.clone()
        };
        let option_value = if ty == "remote" { item.get("option") } else { None };
        let option = migrate_remote_options(&uid, option_value)?;
        let subscription = if ty == "remote" {
            migrate_subscription(&uid, item.get("extra"))?
        } else {
            None
        };

        let mut source = Mapping::new();
        source.insert("type".into(), "remote".into());
        for (k, v) in materialized {
            source.insert(k, v);
        }
        source.insert("url".into(), Value::String(url));
        source.insert("option".into(), Value::Mapping(option));
        if let Some(subscription) = subscription {
            source.insert("subscription".into(), Value::Mapping(subscription));
        }
        source
    } else {
        // Local: Managed, or External Symlink when legacy `symlinks` is set (R9)
        let mut binding = Mapping::new();
        match item.get("symlinks") {
            Some(Value::String(target)) => {
                let is_absolute = std::path::Path::new(target).is_absolute() || target.starts_with('/');
                if !is_absolute {
                    return Err(fail("symlinks", "external symlink target must be absolute"));
                }
                binding.insert("type".into(), "external".into());
                for (k, v) in materialized {
                    binding.insert(k, v);
                }
                binding.insert("target".into(), Value::String(target.clone()));
                binding.insert("mode".into(), "symlink".into());
            }
            None | Some(Value::Null) => {
                binding.insert("type".into(), "managed".into());
                for (k, v) in materialized {
                    binding.insert(k, v);
                }
            }
            Some(_) => return Err(fail("symlinks", "symlinks must be a string path")),
        }
        let mut source = Mapping::new();
        source.insert("type".into(), "local".into());
        source.insert("binding".into(), Value::Mapping(binding));
        source
    };

    // ---- definition (R1) -----------------------------------------------------
    let mut out = Mapping::new();
    out.insert("uid".into(), Value::String(uid.clone()));
    out.insert("name".into(), Value::String(name));
    if let Some(desc) = desc {
        out.insert("desc".into(), Value::String(desc));
    }
    match ty.as_str() {
        "remote" | "local" => {
            let mut config = Mapping::new();
            config.insert("type".into(), "file".into());
            config.insert("source".into(), Value::Mapping(source));
            if let Some(transforms) = transforms {
                config.insert("transforms".into(), Value::Sequence(transforms));
            }
            out.insert("type".into(), "config".into());
            out.insert("config".into(), Value::Mapping(config));
        }
        "merge" | "script" => {
            let mut transform = Mapping::new();
            if ty == "merge" {
                transform.insert("type".into(), "overlay".into());
                transform.insert("source".into(), Value::Mapping(source));
            } else {
                let runtime = item
                    .get("script_type")
                    .and_then(str_value)
                    .ok_or_else(|| fail("script_type", "missing script runtime"))?;
                if !matches!(runtime, "javascript" | "lua") {
                    return Err(fail("script_type", "unknown script runtime"));
                }
                transform.insert("type".into(), "script".into());
                transform.insert("source".into(), Value::Mapping(source));
                transform.insert("runtime".into(), runtime.into());
            }
            out.insert("type".into(), "transform".into());
            out.insert("transform".into(), Value::Mapping(transform));
        }
        _ => unreachable!("validated above"),
    }
    Ok(out)
}

/// R3: managed paths must be relative, traversal-free and not URLs (mirrors
/// nyanpasu-config `ManagedProfilePath::new`, but fails with uid context).
fn validate_managed_relative(uid: &str, file: &str) -> Result<(), CleanSchemaError> {
    use std::path::{Component, Path};
    let path = Path::new(file);
    let bad = file.is_empty()
        || path.is_absolute()
        || file.starts_with('/')
        || path.components().any(|c| {
            matches!(
                c,
                Component::Prefix(_) | Component::RootDir | Component::ParentDir | Component::CurDir
            )
        });
    if bad {
        return Err(CleanSchemaError::new(
            Some(uid),
            "file",
            "materialized file must be a plain relative path",
        ));
    }
    Ok(())
}

/// R5: two distinct legacy "absent" semantics, measured from the old runtime.
fn migrate_remote_options(
    uid: &str,
    option: Option<&Value>,
) -> Result<Mapping, CleanSchemaError> {
    let fail = |field: &str, reason: &str| CleanSchemaError::new(Some(uid), field, reason);
    let mut out = Mapping::new();
    match option {
        None | Some(Value::Null) => {
            // legacy serde default: RemoteProfileOptions::default() → self_proxy Some(true);
            // with_proxy None → effective false at subscribe time (apply_default)
            out.insert("with_proxy".into(), Value::Bool(false));
            out.insert("self_proxy".into(), Value::Bool(true));
            out.insert("update_interval_minutes".into(), Value::from(120u64));
        }
        Some(Value::Mapping(option)) => {
            for key in option.keys() {
                let Some(key) = key.as_str() else {
                    return Err(fail("option", "option keys must be strings"));
                };
                if !["user_agent", "with_proxy", "self_proxy", "update_interval"].contains(&key) {
                    return Err(fail(&format!("option.{key}"), "unknown legacy option field"));
                }
            }
            if let Some(ua) = option.get("user_agent") {
                if !ua.is_null() {
                    out.insert("user_agent".into(), ua.clone());
                }
            }
            // apply_default semantics: present map, missing/None flags → false
            let flag = |key: &str| -> Result<bool, CleanSchemaError> {
                match option.get(key) {
                    None | Some(Value::Null) => Ok(false),
                    Some(Value::Bool(b)) => Ok(*b),
                    Some(_) => Err(fail(&format!("option.{key}"), "must be a boolean")),
                }
            };
            out.insert("with_proxy".into(), Value::Bool(flag("with_proxy")?));
            out.insert("self_proxy".into(), Value::Bool(flag("self_proxy")?));
            let interval = match option.get("update_interval") {
                None | Some(Value::Null) => 120,
                Some(Value::Number(n)) => n
                    .as_u64()
                    .ok_or_else(|| fail("option.update_interval", "must be a non-negative integer"))?,
                Some(_) => return Err(fail("option.update_interval", "must be an integer")),
            };
            if interval == 0 {
                return Err(fail(
                    "option.update_interval",
                    "zero interval is not representable in the clean schema; fix the profile before migrating",
                ));
            }
            out.insert("update_interval_minutes".into(), Value::from(interval));
        }
        Some(_) => return Err(fail("option", "option must be a mapping")),
    }
    Ok(out)
}

/// R6: extra → subscription; expire: 0 → None.
fn migrate_subscription(
    uid: &str,
    extra: Option<&Value>,
) -> Result<Option<Mapping>, CleanSchemaError> {
    let fail = |field: &str, reason: &str| CleanSchemaError::new(Some(uid), field, reason);
    match extra {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Mapping(extra)) => {
            let mut out = Mapping::new();
            for (key, value) in extra {
                let Some(key) = key.as_str() else {
                    return Err(fail("extra", "extra keys must be strings"));
                };
                match key {
                    "upload" | "download" | "total" => {
                        out.insert(key.into(), value.clone());
                    }
                    "expire" => {
                        if value.as_u64() != Some(0) && !value.is_null() {
                            out.insert("expire".into(), value.clone());
                        }
                    }
                    other => return Err(fail(&format!("extra.{other}"), "unknown legacy extra field")),
                }
            }
            Ok((!out.is_empty()).then_some(out))
        }
        Some(_) => Err(fail("extra", "extra must be a mapping")),
    }
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu remote_item remote_option remote_extra remote_failures`
Expected: 全 PASS。

- [ ] **Step 5: Commit**

```bash
git add backend/tauri/src/core/migration/modules/profiles.rs
git commit -m "feat(migration): map legacy remote profiles onto clean schema"
```

---

### Task 3: local / merge / script 分支测试(R1/R3b/R8/R9)

Task 2 的 `migrate_item` 已含全部分支实现;本 Task 用测试钉死 local/merge/script 行为(实现如有出入在此修正)。

**Files:**

- Modify: `backend/tauri/src/core/migration/modules/profiles.rs`(仅测试模块;实现修正按需)

- [ ] **Step 1: 写测试**

```rust
    #[test]
    fn local_item_managed_mapping() {
        let out = migrated("uid: l1\ntype: local\nname: L\nfile: l1.yaml\nupdated: 5\nchains: [t1]\n");
        yaml_eq(
            &out,
            r#"uid: l1
name: L
type: config
config:
  type: file
  source:
    type: local
    binding: {type: managed, file: l1.yaml, updated_at: 5}
  transforms: [t1]
"#,
        );
    }

    #[test]
    fn local_symlinks_becomes_external_symlink_binding() {
        let out = migrated(
            "uid: l1\ntype: local\nname: L\nfile: l1.yaml\nsymlinks: /outside/real.yaml\n",
        );
        yaml_eq(
            &out,
            r#"uid: l1
name: L
type: config
config:
  type: file
  source:
    type: local
    binding: {type: external, file: l1.yaml, target: /outside/real.yaml, mode: symlink}
"#,
        );
        // 相对 target 显式失败
        let err = migrate_item(item(
            "uid: l1\ntype: local\nname: L\nfile: l1.yaml\nsymlinks: not/absolute.yaml\n",
        ))
        .unwrap_err();
        assert_eq!(err.field_path, "symlinks");
    }

    #[test]
    fn merge_and_script_become_transforms() {
        let out = migrated("uid: m1\ntype: merge\nname: M\nfile: m1.yaml\n");
        yaml_eq(
            &out,
            r#"uid: m1
name: M
type: transform
transform:
  type: overlay
  source:
    type: local
    binding: {type: managed, file: m1.yaml}
"#,
        );
        let out = migrated("uid: s1\ntype: script\nname: S\nfile: s1.lua\nscript_type: lua\n");
        yaml_eq(
            &out,
            r#"uid: s1
name: S
type: transform
transform:
  type: script
  source:
    type: local
    binding: {type: managed, file: s1.lua}
  runtime: lua
"#,
        );
    }

    #[test]
    fn url_in_file_converts_to_remote_source_per_legacy_type() {
        // design §14.2: 定义按旧 type,Source 改 Remote,file 重新生成
        let out = migrated("uid: l1\ntype: local\nname: L\nfile: https://e.com/sub.yaml\n");
        assert_eq!(out["type"], Value::from("config"));
        let source = out["config"]["source"].as_mapping().unwrap();
        assert_eq!(source["type"], Value::from("remote"));
        assert_eq!(source["url"], Value::from("https://e.com/sub.yaml"));
        assert_eq!(source["file"], Value::from("l1.yaml"));
        let option = source["option"].as_mapping().unwrap();
        assert_eq!(option["self_proxy"], Value::Bool(true)); // R5 absent 语义

        let out = migrated(
            "uid: s1\ntype: script\nname: S\nfile: https://e.com/x.js\nscript_type: javascript\n",
        );
        assert_eq!(out["transform"]["source"]["file"], Value::from("s1.js"));
    }

    #[test]
    fn item_failures_for_non_remote_kinds() {
        // merge/script 不允许 chain(R8 → 未知键)
        let err = migrate_item(item("uid: m1\ntype: merge\nname: M\nfile: m1.yaml\nchain: []\n"))
            .unwrap_err();
        assert_eq!(err.field_path, "chain");
        // 未知 type(R1)
        let err = migrate_item(item("uid: x\ntype: banana\nname: X\nfile: x.yaml\n")).unwrap_err();
        assert_eq!(err.field_path, "type");
        // 路径穿越(R3)
        let err = migrate_item(item("uid: l1\ntype: local\nname: L\nfile: ../up.yaml\n")).unwrap_err();
        assert_eq!(err.field_path, "file");
        // chain 与 chains 同存(R8)
        let err = migrate_item(item(
            "uid: l1\ntype: local\nname: L\nfile: l1.yaml\nchain: []\nchains: []\n",
        ))
        .unwrap_err();
        assert_eq!(err.field_path, "chain");
    }
```

- [ ] **Step 2: 跑测试,修正实现直到全绿**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu local_item local_symlinks merge_and_script url_in_file item_failures`
Expected: 全 PASS(失败则修 `migrate_item` 对应分支,禁止改测试期望)。

- [ ] **Step 3: Commit**

```bash
git add backend/tauri/src/core/migration/modules/profiles.rs
git commit -m "test(migration): pin local/merge/script clean-schema mapping rules"
```

---

### Task 4: 顶层文档映射(R7/R10/R11/R12/R14)

**Files:**

- Modify: `backend/tauri/src/core/migration/modules/profiles.rs`

**Interfaces:**

- Produces: `fn migrate_clean_schema(doc: Mapping) -> Result<Mapping, CleanSchemaError>`(Task 5 run() 调用)。

- [ ] **Step 1: 写失败测试**

```rust
    #[test]
    fn top_level_current_forms() {
        // 缺失/[] → 无 current 键
        let out = migrate_clean_schema(item("items: []\n")).unwrap();
        assert!(!out.contains_key("current"));
        let out = migrate_clean_schema(item("current: []\nitems: []\n")).unwrap();
        assert!(!out.contains_key("current"));

        // 标量与单元素(rev1 前旧数据可能是标量,防御性支持)
        let doc = "current: l1\nitems:\n- {uid: l1, type: local, name: L, file: l1.yaml}\n";
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["current"], Value::from("l1"));
        let doc = "current: [l1]\nitems:\n- {uid: l1, type: local, name: L, file: l1.yaml}\n";
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["current"], Value::from("l1"));
    }

    #[test]
    fn multi_current_synthesizes_composition() {
        let doc = r#"current: [a, b, c]
items:
- {uid: a, type: local, name: A, file: a.yaml}
- {uid: b, type: remote, name: B, file: b.yaml, url: "https://e.com"}
- {uid: c, type: local, name: C, file: c.yaml}
"#;
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["current"], Value::from("combined-profile"));
        let items = out["items"].as_sequence().unwrap();
        let combined = items
            .iter()
            .find(|i| i["uid"] == Value::from("combined-profile"))
            .expect("synthesized composition present");
        assert_eq!(combined["name"], Value::from("Combined Profile"));
        yaml_eq(
            combined["config"].as_mapping().unwrap(),
            "type: composition\nbase: a\nextend_proxies_from: [b, c]\n",
        );
    }

    #[test]
    fn combined_profile_uid_is_collision_safe_and_deterministic() {
        let doc = r#"current: [a, b]
items:
- {uid: a, type: local, name: A, file: a.yaml}
- {uid: b, type: local, name: B, file: b.yaml}
- {uid: combined-profile, type: local, name: Taken, file: t.yaml}
"#;
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["current"], Value::from("combined-profile-2"));
    }

    #[test]
    fn multi_current_with_transform_member_fails() {
        let doc = r#"current: [a, s]
items:
- {uid: a, type: local, name: A, file: a.yaml}
- {uid: s, type: script, name: S, file: s.js, script_type: javascript}
"#;
        let err = migrate_clean_schema(item(doc)).unwrap_err();
        assert_eq!(err.uid.as_deref(), Some("s"));
        assert_eq!(err.field_path, "current");
    }

    #[test]
    fn chain_becomes_global_transforms_and_unknown_top_level_fails() {
        let doc = r#"chain: [t1]
items:
- {uid: t1, type: merge, name: T, file: t1.yaml}
"#;
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["global_transforms"], item("x: [t1]")["x"]);

        let err = migrate_clean_schema(item("bogus: 1\nitems: []\n")).unwrap_err();
        assert_eq!(err.field_path, "bogus");
        assert!(err.uid.is_none());
    }

    #[test]
    fn valid_carries_verbatim() {
        let doc = "valid: [dns, tun]\nitems: []\n";
        let out = migrate_clean_schema(item(doc)).unwrap();
        assert_eq!(out["valid"], item("x: [dns, tun]")["x"]);
    }
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu top_level_current multi_current combined_profile chain_becomes valid_carries`
Expected: FAIL(`migrate_clean_schema` 未定义)。

- [ ] **Step 3: 实现**

```rust
/// R7/R10/R11/R12: whole-document mapping. Input is the post-rev2 doc.
fn migrate_clean_schema(doc: Mapping) -> Result<Mapping, CleanSchemaError> {
    for key in doc.keys() {
        let Some(key) = key.as_str() else {
            return Err(CleanSchemaError::new(None, "<non-string key>", "top-level keys must be strings"));
        };
        if !["current", "chain", "valid", "items"].contains(&key) {
            return Err(CleanSchemaError::new(None, key, "unknown legacy top-level field"));
        }
    }

    let items: Vec<Mapping> = match doc.get("items") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Sequence(seq)) => seq
            .iter()
            .map(|v| {
                v.as_mapping().cloned().ok_or_else(|| {
                    CleanSchemaError::new(None, "items", "every item must be a mapping")
                })
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(CleanSchemaError::new(None, "items", "items must be a sequence")),
    };

    let mut new_items: Vec<Value> = Vec::with_capacity(items.len() + 1);
    let mut uids: Vec<String> = Vec::with_capacity(items.len());
    let mut direct_file_config: Vec<String> = Vec::new();
    for item in items {
        let migrated = migrate_item(item)?;
        let uid = migrated["uid"].as_str().expect("set by migrate_item").to_owned();
        if migrated.get("type") == Some(&Value::from("config")) {
            direct_file_config.push(uid.clone());
        }
        uids.push(uid);
        new_items.push(Value::Mapping(migrated));
    }

    // R10: current forms
    let old_current: Vec<String> = match doc.get("current") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Sequence(seq)) => seq
            .iter()
            .map(|v| {
                v.as_str().map(str::to_owned).ok_or_else(|| {
                    CleanSchemaError::new(None, "current", "current entries must be strings")
                })
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(CleanSchemaError::new(None, "current", "current must be a string or sequence")),
    };

    let current: Option<String> = match old_current.len() {
        0 => None,
        1 => Some(old_current[0].clone()),
        _ => {
            for member in &old_current {
                if !uids.contains(member) {
                    return Err(CleanSchemaError::new(Some(member), "current", "current member does not exist"));
                }
                if !direct_file_config.contains(member) {
                    return Err(CleanSchemaError::new(
                        Some(member),
                        "current",
                        "multi-current member cannot be represented as a direct FileConfig",
                    ));
                }
            }
            let uid = {
                let mut candidate = "combined-profile".to_owned();
                let mut n = 1;
                while uids.contains(&candidate) {
                    n += 1;
                    candidate = format!("combined-profile-{n}");
                }
                candidate
            };
            let mut config = Mapping::new();
            config.insert("type".into(), "composition".into());
            config.insert("base".into(), Value::String(old_current[0].clone()));
            config.insert(
                "extend_proxies_from".into(),
                Value::Sequence(old_current[1..].iter().cloned().map(Value::String).collect()),
            );
            let mut combined = Mapping::new();
            combined.insert("uid".into(), Value::String(uid.clone()));
            combined.insert("name".into(), "Combined Profile".into());
            combined.insert("type".into(), "config".into());
            combined.insert("config".into(), Value::Mapping(config));
            new_items.push(Value::Mapping(combined));
            Some(uid)
        }
    };

    let mut out = Mapping::new();
    if let Some(current) = current {
        out.insert("current".into(), Value::String(current));
    }
    match doc.get("chain") {
        None | Some(Value::Null) => {}
        Some(Value::Sequence(seq)) if seq.is_empty() => {}
        Some(Value::Sequence(seq)) => {
            out.insert("global_transforms".into(), Value::Sequence(seq.clone()));
        }
        Some(_) => return Err(CleanSchemaError::new(None, "chain", "chain must be a sequence")),
    }
    if let Some(valid) = doc.get("valid") {
        if !valid.is_null() {
            out.insert("valid".into(), valid.clone());
        }
    }
    out.insert("items".into(), Value::Sequence(new_items));
    Ok(out)
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu top_level_current multi_current combined_profile chain_becomes valid_carries multi_current_with_transform`
Expected: 全 PASS。

- [ ] **Step 5: Commit**

```bash
git add backend/tauri/src/core/migration/modules/profiles.rs
git commit -m "feat(migration): map legacy profiles document onto clean schema"
```

---

### Task 5: run()/rollback() 落地(R13/R14/R15 + typed 校验背书)

**Files:**

- Modify: `backend/tauri/src/core/migration/modules/profiles.rs`(替换 Task 1 的占位 `run_clean_schema`/`rollback_clean_schema`)

- [ ] **Step 1: 写失败测试(tempdir 端到端)**

```rust
    fn temp_ctx() -> (tempfile::TempDir, Ctx) {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let ctx = Ctx::new(config, data);
        (temp, ctx)
    }

    #[test]
    fn clean_schema_run_writes_bak_and_validated_new_schema() {
        let (_temp, mut ctx) = temp_ctx();
        let path = ctx.profiles_path();
        std::fs::write(&path, MIGRATED_SAMPLE).unwrap();

        run_clean_schema(&mut ctx).unwrap();

        // .bak = 迁移前原文
        let bak = std::fs::read_to_string(path.with_extension("yaml.bak")).unwrap();
        assert_eq!(bak, MIGRATED_SAMPLE);

        // 输出带头注释,且能被新类型加载并通过 validate
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.starts_with("# Profiles Config for Clash Nyanpasu\n\n"));
        let profiles: nyanpasu_config::profile::Profiles = serde_yaml::from_str(&raw).unwrap();
        profiles.validate().unwrap();
        assert_eq!(profiles.items.len(), 8); // 7 items + multi-current 合成 Composition
        assert!(profiles.current.is_some());

        // 幂等重入:再跑一遍 no-op,内容不变
        run_clean_schema(&mut ctx).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
    }

    #[test]
    fn clean_schema_run_failure_leaves_file_untouched() {
        let (_temp, mut ctx) = temp_ctx();
        let path = ctx.profiles_path();
        // 引用不存在 transform 的 chain → validate 失败
        let bad = "chain: [ghost]\nitems: []\n";
        std::fs::write(&path, bad).unwrap();
        assert!(run_clean_schema(&mut ctx).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
    }

    #[test]
    fn clean_schema_rollback_restores_bak() {
        let (_temp, mut ctx) = temp_ctx();
        let path = ctx.profiles_path();
        std::fs::write(&path, MIGRATED_SAMPLE).unwrap();
        run_clean_schema(&mut ctx).unwrap();
        rollback_clean_schema(&mut ctx).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), MIGRATED_SAMPLE);
    }
```

注:`MIGRATED_SAMPLE` 是文件内既有常量(post-rev2 全量样本,含 multi-current `[rIWXPHuafvEM]`——只有 1 个 current,`items.len()` 断言按实际:7 items、无合成。**执行时先数清样本**:该样本 current 单值 → 7 items、无 Composition;把断言改为 `assert_eq!(profiles.items.len(), 7)`。若想覆盖合成路径,在本测试内用 Task 4 的 multi-current 文档另写一份 tempdir 用例。)

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu clean_schema_run clean_schema_rollback`
Expected: FAIL(占位 run 什么都不做,断言 .bak 不存在)。

- [ ] **Step 3: 实现**

```rust
fn run_clean_schema(ctx: &mut Ctx) -> anyhow::Result<()> {
    let path = ctx.profiles_path();
    if !path.exists() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(&path)?;
    let doc: Mapping = serde_yaml::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("failed to parse profiles: {e}"))?;
    if is_clean_schema(&doc) {
        return Ok(());
    }

    // R15: backup first, then transform (D3: mandatory .bak)
    let bak = path.with_extension("yaml.bak");
    crate::core::migration::fs::atomic_write(&bak, raw.as_bytes())?;

    let migrated = migrate_clean_schema(doc)?;

    // Typed round-trip: the only accepted output is a document the new domain
    // model can load AND validate (design §14.4). Duplicate uids are rejected
    // here by the items deserializer (R13).
    let profiles: nyanpasu_config::profile::Profiles =
        serde_yaml::from_value(Value::Mapping(migrated))
            .map_err(|e| anyhow::anyhow!("clean-schema output rejected by domain model: {e}"))?;
    profiles
        .validate()
        .map_err(|errors| anyhow::anyhow!("clean-schema output failed validation: {errors:?}"))?;

    let body = serde_yaml::to_string(&profiles)
        .map_err(|e| anyhow::anyhow!("failed to serialize migrated profiles: {e}"))?;
    let content = format!("# Profiles Config for Clash Nyanpasu\n\n{body}");
    crate::core::migration::fs::atomic_write(&path, content.as_bytes())?;
    Ok(())
}

fn rollback_clean_schema(ctx: &mut Ctx) -> anyhow::Result<()> {
    let path = ctx.profiles_path();
    let bak = path.with_extension("yaml.bak");
    if !bak.exists() {
        eprintln!("profiles.yaml.bak not found, nothing to roll back");
        return Ok(());
    }
    let raw = std::fs::read(&bak)?;
    crate::core::migration::fs::atomic_write(&path, &raw)
}
```

(若 `path.with_extension("yaml.bak")` 与实际文件名拼接不符——`profiles.yaml` → `profiles.yaml.bak` 需要的是**追加**扩展——改用:
`let bak = path.with_file_name(format!("{}.bak", path.file_name().unwrap().to_string_lossy()));`
以测试断言为准。)

- [ ] **Step 4: 跑本模块全部测试**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu --lib migration`
Expected: 全 PASS(含 rev1/rev2 既有测试)。

- [ ] **Step 5: Commit**

```bash
git add backend/tauri/src/core/migration/modules/profiles.rs
git commit -m "feat(migration): wire profiles clean-schema step run/rollback with .bak"
```

---

### Task 6: 1.6.1 → 2.0 端到端 fixture 更新

**Files:**

- Modify: `backend/tauri/src/core/migration/fixtures/v2_0_expected/profiles.yaml`(整体替换为 clean schema 终态)
- Modify: `backend/tauri/src/core/migration/runner.rs`(`real_1_6_1_fixture_migrates_to_2_0_shape` 的 profiles revision 断言 2→3)

**Interfaces:**

- Consumes: v1_6_1 输入 fixture(current: null、chain: null、remote 无 option/extra、script js)——经 rev1(null→[])→ rev2(script 展开)→ rev3(clean schema)。

- [ ] **Step 1: 更新期望 fixture**

`fixtures/v2_0_expected/profiles.yaml` 替换为(shape 对比、键序无关;R5 absent → self_proxy true;current []→省略;chain []→省略;valid 原样;desc null→省略、desc ''→保留):

```yaml
valid:
  - dns
  - unified-delay
  - tun
items:
  - uid: rIWXPHuafvEM
    name: Test Remote
    type: config
    config:
      type: file
      source:
        type: remote
        file: rIWXPHuafvEM.yaml
        updated_at: 1758110672
        url: https://example.com
        option:
          with_proxy: false
          self_proxy: true
          update_interval_minutes: 120
  - uid: siL1cvjnvLB6
    name: Script Chain
    desc: ''
    type: transform
    transform:
      type: script
      source:
        type: local
        binding:
          type: managed
          file: siL1cvjnvLB6.js
          updated_at: 1720954186
      runtime: javascript
```

- [ ] **Step 2: 更新 runner 断言**

`runner.rs:418` 的
`assert_eq!(runner.store.module_state("profiles").applied_revision, 2);`
改为
`assert_eq!(runner.store.module_state("profiles").applied_revision, 3);`

- [ ] **Step 3: 跑端到端测试**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu real_1_6_1_fixture`
Expected: PASS。若 shape 不匹配,以**实现输出**核对期望文件的键(不得为了过测试改映射规则;先人工核对差异属于期望笔误还是实现 bug)。

- [ ] **Step 4: 全量回归 + Commit**

Run: `cargo test --manifest-path ./backend/Cargo.toml -p clash-nyanpasu`
Expected: 全绿。

```bash
git add backend/tauri/src/core/migration/fixtures/v2_0_expected/profiles.yaml backend/tauri/src/core/migration/runner.rs
git commit -m "test(migration): pin 1.6.1 fixture to clean-schema terminal shape"
```

---

### Task 7: 契约回写(task.md §5.3)

**Files:**

- Modify: `docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md`(T02 卡「规则清单」段)

- [ ] **Step 1: 把本 plan 侦察发现的两条 spec 补遗追记进 T02 卡规则清单**

追加两行:

- `local.symlinks: Some(target)` → `LocalBinding::External{target, mode: symlink}`(legacy 字段,guide §6 未列;target 非绝对路径显式失败)
- `remote.option` 缺省语义按旧运行期实测:键整体缺失 → `{with_proxy: false, self_proxy: true, 120}`;键存在但字段缺失 → `{with_proxy: false, self_proxy: false}`(`apply_default`,remote.rs:474-479);`update_interval == 0` → 显式失败

- [ ] **Step 2: Commit**

```bash
git add docs/superpowers/specs/2026-07-04-pr3-profiles-domain-switch-tauri/task.md
git commit -m "docs(pr3): record T02 mapping rule addenda (symlinks, option defaults)"
```

---

## 验证总表(对应 T02 卡验证段)

| 判据                                        | 覆盖                                                           |
| ------------------------------------------- | -------------------------------------------------------------- |
| fixtures 覆盖 design §10 全规则             | Task 2/3/4 逐规则测试(R1–R12 映射到测试名)                     |
| clean-design §18 第 24 条(multi-current)    | Task 4 `multi_current_synthesizes_composition`                 |
| §18 第 26 条(URL 只在 migration 转换)       | Task 3 `url_in_file_converts_to_remote_source_per_legacy_type` |
| §18 第 27 条(重复 uid 拒绝)                 | Task 5 typed round-trip 背书(items 反序列化拒绝)               |
| `.bak` 生成断言                             | Task 5 `clean_schema_run_writes_bak_...`                       |
| 幂等重入                                    | Task 1 检测 + Task 5 重入断言                                  |
| 仿 `runner.rs:367` 端到端                   | Task 6                                                         |
| `cargo test -p clash-nyanpasu migration` 绿 | Task 5 Step 4 + Task 6 Step 4                                  |
