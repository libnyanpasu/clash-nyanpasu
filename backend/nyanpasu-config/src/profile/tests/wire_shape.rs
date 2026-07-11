use serde_json::{Value, json};
use url::Url;

use crate::profile::*;

fn managed(file: &str) -> MaterializedFile {
    MaterializedFile {
        file: ManagedProfilePath::new(file).unwrap(),
        updated_at: None,
    }
}

fn assert_flat_materialized(value: &Value, expected_type: &str, file: &str) {
    assert_eq!(value["type"], expected_type);
    assert_eq!(value["file"], file);
    assert!(value.get("materialized").is_none());
}

#[test]
fn config_definition_variants_match_the_flat_wire_contract() {
    let file = ConfigDefinition::File(FileConfig {
        source: ProfileSource::Local {
            binding: LocalBinding::Managed {
                materialized: managed("config.yaml"),
            },
        },
        transforms: vec![ProfileId("normalize".into())],
    });
    let value = serde_json::to_value(file).unwrap();
    assert_eq!(value["type"], "file");
    assert!(value.get("source").is_some());
    assert_eq!(value["transforms"], json!(["normalize"]));
    assert!(value.get("file").is_none());

    let composition = ConfigDefinition::Composition(CompositionConfig {
        base: Some(ProfileId("base".into())),
        extend_proxies_from: vec![ProfileId("member".into())],
        transforms: vec![],
    });
    let value = serde_json::to_value(composition).unwrap();
    assert_eq!(value["type"], "composition");
    assert_eq!(value["base"], "base");
    assert_eq!(value["extend_proxies_from"], json!(["member"]));
    assert!(value.get("composition").is_none());
}

#[test]
fn transform_definition_variants_match_the_flat_wire_contract() {
    let overlay = TransformDefinition::Overlay(OverlayTransform {
        source: ProfileSource::Local {
            binding: LocalBinding::Managed {
                materialized: managed("overlay.yaml"),
            },
        },
    });
    let value = serde_json::to_value(overlay).unwrap();
    assert_eq!(value["type"], "overlay");
    assert!(value.get("source").is_some());
    assert!(value.get("overlay").is_none());

    let script = TransformDefinition::Script(ScriptTransform {
        source: ProfileSource::Local {
            binding: LocalBinding::Managed {
                materialized: managed("script.js"),
            },
        },
        runtime: ScriptRuntime::JavaScript,
    });
    let value = serde_json::to_value(script).unwrap();
    assert_eq!(value["type"], "script");
    assert_eq!(value["runtime"], "javascript");
    assert!(value.get("source").is_some());
    assert!(value.get("script").is_none());
}

#[test]
fn profile_source_variants_match_the_flat_wire_contract() {
    let local = ProfileSource::Local {
        binding: LocalBinding::Managed {
            materialized: managed("local.yaml"),
        },
    };
    let value = serde_json::to_value(local).unwrap();
    assert_eq!(value["type"], "local");
    assert!(value.get("binding").is_some());
    assert!(value.get("materialized").is_none());

    let remote = ProfileSource::Remote {
        materialized: managed("remote.yaml"),
        url: Url::parse("https://example.com/remote.yaml").unwrap(),
        option: RemoteProfileOptions::default(),
        subscription: SubscriptionInfo::default(),
    };
    let value = serde_json::to_value(remote).unwrap();
    assert_flat_materialized(&value, "remote", "remote.yaml");
    assert_eq!(value["url"], "https://example.com/remote.yaml");
    assert!(value.get("option").is_some());
}

#[test]
fn local_binding_variants_match_the_flat_wire_contract() {
    let managed_binding = LocalBinding::Managed {
        materialized: managed("managed.yaml"),
    };
    let value = serde_json::to_value(managed_binding).unwrap();
    assert_flat_materialized(&value, "managed", "managed.yaml");

    let external = LocalBinding::External {
        materialized: managed("external.yaml"),
        target: ExternalProfilePath::new("/tmp/external.yaml").unwrap(),
        mode: ExternalMode::Mirror,
    };
    let value = serde_json::to_value(external).unwrap();
    assert_flat_materialized(&value, "external", "external.yaml");
    assert_eq!(value["target"], "/tmp/external.yaml");
    assert_eq!(value["mode"], "mirror");
}
