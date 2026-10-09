use std::sync::Arc;

use nyanpasu_config::{
    profile::{
        ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
        ProfileDefinition, ProfileId, ProfileItem, ProfileMetadata, ProfileSource, Profiles,
        ScriptRuntime, ScriptTransform, TransformDefinition,
    },
    runtime::executor::ResolvedPortBindings,
};
use nyanpasu_core::runtime::config::{
    FsProfileContentSource, RuntimeBuildInput, RuntimeBuilder, RuntimeConfigScriptRunner,
    ScriptDirs,
};
use serde_yaml_ng as serde_yaml;

#[test]
fn builder_runs_a_managed_script_through_the_platform_adapters() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join("config.yaml"),
        "proxies: []\nmode: direct\nextra-key: keep\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("transform.js"),
        "function main(config) { config.mode = 'rule'; console.log('script ran'); return config; }",
    )
    .unwrap();

    let managed = |name: &str| MaterializedFile {
        file: ManagedProfilePath::new(name).unwrap(),
        updated_at: None,
    };
    let mut profiles = Profiles::default();
    profiles.append_item(ProfileItem {
        uid: ProfileId("config".into()),
        metadata: ProfileMetadata {
            name: "Config".into(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: managed("config.yaml"),
                    },
                },
                transforms: vec![ProfileId("transform".into())],
            }),
        },
    });
    profiles.append_item(ProfileItem {
        uid: ProfileId("transform".into()),
        metadata: ProfileMetadata {
            name: "Transform".into(),
            desc: None,
            custom_name: true,
        },
        definition: ProfileDefinition::Transform {
            transform: TransformDefinition::Script(ScriptTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: managed("transform.js"),
                    },
                },
                runtime: ScriptRuntime::JavaScript,
            }),
        },
    });
    profiles.set_current(Some(ProfileId("config".into())));

    let mut input = RuntimeBuildInput {
        profiles: Arc::new(profiles),
        clash: Default::default(),
        app: Default::default(),
        resolved_ports: ResolvedPortBindings {
            mixed_port: 7890,
            ..Default::default()
        },
    };
    input.app.enable_builtin_enhanced = false;
    input.clash.enable_clash_fields = false;

    let content = FsProfileContentSource::new(temp.path().to_path_buf());
    let scripts = RuntimeConfigScriptRunner::new(ScriptDirs::under(temp.path())).unwrap();
    let artifact = RuntimeBuilder::build(&input, &content, &scripts).unwrap();
    let yaml = serde_yaml::to_value(&*artifact.final_config).unwrap();

    assert_eq!(yaml["mode"], serde_yaml::Value::from("rule"));
    assert_eq!(yaml["extra-key"], serde_yaml::Value::from("keep"));
    assert_eq!(yaml["mixed-port"], serde_yaml::Value::from(7890));
    assert!(artifact.step_logs.iter().any(|log| {
        log.entries
            .iter()
            .any(|entry| entry.message.contains("script ran"))
    }));
    RuntimeBuilder::validate_transforms(&artifact).unwrap();
}
