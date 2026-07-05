//! RuntimeBuilder: pure assembly from domain snapshots to the runtime pipeline
//! executor (PR-3 T06, design §8 + §19). No globals, no IO — port resolution
//! happens at the caller and file reads/script runs arrive via executor ports.
//! Must be called from a blocking context (the script adapter blocks on its
//! own runtime); the facade wraps the whole build in spawn_blocking (T07).

use std::sync::Arc;

use nyanpasu_config::{
    application::{ClashCore, NyanpasuAppConfig},
    clash::config::{ClashConfig, tun_stack::TunStack},
    profile::{ProfileValidationError, Profiles, ScriptRuntime},
    runtime::executor::{
        BuiltinTransform, ExecutionTarget, GuardInputs, ProfileContentSource, ResolvedPortBindings,
        RuntimeArtifact, RuntimePipelineError, RuntimePipelineInputs, ScriptRunner, TunFlavor,
        TunParams, execute,
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

const MIHOMO_FAMILY: &[ClashCore] = &[ClashCore::Mihomo, ClashCore::MihomoAlpha];
const ALL_CORES: &[ClashCore] = &[
    ClashCore::ClashPremium,
    ClashCore::ClashRs,
    ClashCore::Mihomo,
    ClashCore::MihomoAlpha,
    ClashCore::ClashRsAlpha,
];
// Legacy quirk preserved (chain.rs:174): clash_rs_comp never gated in ClashRsAlpha.
const CLASH_RS_ONLY: &[ClashCore] = &[ClashCore::ClashRs];

/// Legacy builtin table, ported 1:1 from enhance/chain.rs:145-176. Order is
/// execution order.
pub fn builtin_transforms_for(core: ClashCore) -> Vec<BuiltinTransform> {
    let table: [(&[ClashCore], &str, ScriptRuntime, &str); 4] = [
        (
            MIHOMO_FAMILY,
            "verge_hy_alpn",
            ScriptRuntime::JavaScript,
            include_str!("./builtin/meta_hy_alpn.js"),
        ),
        (
            MIHOMO_FAMILY,
            "verge_meta_guard",
            ScriptRuntime::JavaScript,
            include_str!("./builtin/meta_guard.js"),
        ),
        (
            ALL_CORES,
            "config_fixer",
            ScriptRuntime::JavaScript,
            include_str!("./builtin/config_fixer.js"),
        ),
        (
            CLASH_RS_ONLY,
            "clash_rs_comp",
            ScriptRuntime::Lua,
            include_str!("./builtin/clash_rs_comp.lua"),
        ),
    ];
    table
        .into_iter()
        .filter(|(gate, ..)| gate.contains(&core))
        .map(|(_, name, runtime, source)| BuiltinTransform {
            name: name.to_string(),
            runtime,
            source: source.to_string(),
        })
        .collect()
}

/// Legacy tun derivation (enhance/tun.rs:47-60), quirks preserved: only
/// `ClashRs` (not Alpha) takes the ClashRs branch; Premium+Mixed → Gvisor.
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

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_config::{
        profile::{ManagedProfilePath, ProfileId, ScriptRuntime},
        runtime::{
            executor::{PortError, ScriptRunOutcome},
            value::ConfigValue,
        },
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
            ScriptRunOutcome {
                result: Ok(config.clone()),
                logs: Vec::new(),
            }
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
            resolved_ports: ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
        }
    }

    #[test]
    fn builtin_gating_matches_legacy_table() {
        let names = |core: ClashCore| -> Vec<String> {
            builtin_transforms_for(core)
                .into_iter()
                .map(|b| b.name)
                .collect()
        };
        assert_eq!(
            names(ClashCore::Mihomo),
            vec!["verge_hy_alpn", "verge_meta_guard", "config_fixer"]
        );
        assert_eq!(
            names(ClashCore::ClashRs),
            vec!["config_fixer", "clash_rs_comp"]
        );
        // legacy 怪癖忠实移植:Alpha 不吃 clash_rs_comp(chain.rs:174)
        assert_eq!(names(ClashCore::ClashRsAlpha), vec!["config_fixer"]);
        assert_eq!(names(ClashCore::ClashPremium), vec!["config_fixer"]);
    }

    #[test]
    fn tun_flavor_derivation_matches_legacy_quirks() {
        assert_eq!(
            derive_tun_flavor(ClashCore::ClashRs, TunStack::Mixed),
            TunFlavor::ClashRs
        );
        // Alpha 走 Standard 分支(tun.rs:47 legacy 怪癖)
        assert_eq!(
            derive_tun_flavor(ClashCore::ClashRsAlpha, TunStack::Mixed),
            TunFlavor::Standard {
                stack: TunStack::Mixed
            }
        );
        // Premium + Mixed → Gvisor 降级(tun.rs:58-60)
        assert_eq!(
            derive_tun_flavor(ClashCore::ClashPremium, TunStack::Mixed),
            TunFlavor::Standard {
                stack: TunStack::Gvisor
            }
        );
        assert_eq!(
            derive_tun_flavor(ClashCore::Mihomo, TunStack::System),
            TunFlavor::Standard {
                stack: TunStack::System
            }
        );
    }

    #[test]
    fn bare_build_produces_artifact_with_guarded_ports() {
        let mut input = base_input(); // current = None → Bare
        input.app.enable_builtin_enhanced = false; // EchoRunner 下 builtin 无意义
        let artifact =
            RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner).expect("bare build");
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
        let artifact = RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner).unwrap();
        let debug = format!("{:?}", artifact.graph);
        assert!(!debug.contains("verge_hy_alpn"));
    }

    /// End-to-end through the REAL adapters (fs content source + boa runner):
    /// selected file config with a scoped JS transform, guard ports injected,
    /// whitelist off keeps unknown keys, step logs anchored for the
    /// postprocessing_output consumer. Serves as the T06 golden baseline;
    /// snapshot-file expansion is tracked as a T07 pre-flight follow-up.
    #[test]
    fn golden_selected_file_with_script_transform_end_to_end() {
        use crate::enhance::{EnhanceScriptRunner, FsProfileContentSource};
        use nyanpasu_config::profile::{
            ConfigDefinition, FileConfig, LocalBinding, MaterializedFile, ProfileDefinition,
            ProfileItem, ProfileMetadata, ProfileSource, ScriptTransform, TransformDefinition,
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

        let mut input = base_input();
        input.profiles = Arc::new(profiles);
        input.app.enable_builtin_enhanced = false; // isolate assembly + adapters

        let content = FsProfileContentSource::new(temp.path().to_path_buf());
        let scripts = EnhanceScriptRunner::new().unwrap();
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
    }
}
