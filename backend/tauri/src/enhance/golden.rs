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
    profile::{ProfileId, Profiles},
    runtime::executor::ResolvedPortBindings,
};

use super::golden_support::{composition, file_config, overlay};
use nyanpasu_core::runtime::config::{
    FsProfileContentSource, RuntimeBuildInput, RuntimeBuilder, RuntimeConfigScriptRunner,
    ScriptDirs,
};

const SUB_A: &str =
    "proxies:\n  - name: a1\n    type: ss\n    server: a.example.com\n    port: 443\n";
const SUB_B: &str =
    "proxies:\n  - name: b1\n    type: vmess\n    server: b.example.com\n    port: 8080\n";
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
    input.clash.enable_clash_fields = false; // 基线依赖 whitelist off,显式声明
    input.app.enable_builtin_enhanced = false; // 各场景按需显式开启
    input
}

fn build_to_yaml(input: &RuntimeBuildInput, dir: &std::path::Path) -> serde_yaml::Value {
    let content = FsProfileContentSource::new(dir.to_path_buf());
    let scripts = RuntimeConfigScriptRunner::new(ScriptDirs::under(dir)).unwrap();
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

/// Real file/script adapters preserve the generated config and rejected-transform diagnostics.
#[test]
fn golden_selected_file_with_script_transform_end_to_end() {
    use nyanpasu_config::profile::{
        ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
        ProfileDefinition, ProfileItem, ProfileMetadata, ProfileSource, ScriptRuntime,
        ScriptTransform, TransformDefinition,
    };

    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("cfg1.yaml"),
        "proxies: []\nmode: direct\nextra-key: keep\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("scr1.js"),
        "function main(config) { config[\"mode\"] = \"rule\"; console.log(\"scoped ran\"); return config; }\n",
    )
    .unwrap();

    let managed = |name: &str| MaterializedFile {
        file: ManagedProfilePath::new(name).unwrap(),
        updated_at: None,
    };
    let mut profiles = Profiles::default();
    profiles.append_item(ProfileItem {
        uid: ProfileId("cfg1".into()),
        metadata: ProfileMetadata {
            name: "CFG1".into(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: managed("cfg1.yaml"),
                    },
                },
                transforms: vec![ProfileId("scr1".into())],
            }),
        },
    });
    profiles.append_item(ProfileItem {
        uid: ProfileId("scr1".into()),
        metadata: ProfileMetadata {
            name: "SCR1".into(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Script(ScriptTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: managed("scr1.js"),
                    },
                },
                runtime: ScriptRuntime::JavaScript,
            }),
        },
    });
    profiles.set_current(Some(ProfileId("cfg1".into())));

    let mut input = RuntimeBuildInput {
        profiles: Arc::new(profiles),
        clash: Default::default(),
        app: Default::default(),
        resolved_ports: ResolvedPortBindings {
            mixed_port: 7890,
            ..Default::default()
        },
    };
    input.app.enable_builtin_enhanced = false; // isolate assembly + adapters

    let content = FsProfileContentSource::new(temp.path().to_path_buf());
    let scripts = RuntimeConfigScriptRunner::new(ScriptDirs::under(temp.path())).unwrap();
    let artifact = RuntimeBuilder::build(&input, &content, &scripts).expect("end-to-end build");

    let yaml = serde_yaml::to_value(&*artifact.final_config).unwrap();
    assert_eq!(yaml["mode"], serde_yaml::Value::from("rule")); // real boa ran the scoped script
    assert_eq!(yaml["extra-key"], serde_yaml::Value::from("keep")); // whitelist off keeps keys
    assert_eq!(yaml["mixed-port"], serde_yaml::Value::from(7890)); // guard ports injected
    assert!(
        artifact.step_logs.iter().any(|log| log
            .entries
            .iter()
            .any(|entry| entry.message.contains("scoped ran"))),
        "script logs must be anchored for the postprocessing_output consumer"
    );
    RuntimeBuilder::validate_transforms(&artifact).unwrap();

    std::fs::write(
        temp.path().join("scr1.js"),
        "function main(config) { console.log('before failure'); throw new Error('transform rejected'); }",
    )
    .unwrap();
    let artifact = RuntimeBuilder::build(&input, &content, &scripts).unwrap();
    let error = RuntimeBuilder::validate_transforms(&artifact).unwrap_err();
    let wire = serde_json::to_value(&error).unwrap();
    assert_eq!(wire["kind"], "transforms_failed");
    let log = wire["logs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|log| log["tag"]["data"]["transform_profile_id"] == "scr1")
        .expect("failed transform must keep its chain tag");
    let entries = log["entries"].as_array().unwrap();
    assert!(entries.iter().any(|entry| {
        entry["message"]
            .as_str()
            .unwrap()
            .contains("before failure")
    }));
    assert!(entries.iter().any(|entry| {
        entry["level"] == "error"
            && entry["message"]
                .as_str()
                .unwrap()
                .contains("transform rejected")
    }));
}
