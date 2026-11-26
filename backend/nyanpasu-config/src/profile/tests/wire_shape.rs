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

#[test]
fn profile_timestamps_keep_the_legacy_integer_wire_shape() {
    for seconds in [-1, 0, 1_720_954_186] {
        let materialized = json!({"file": "profile.yaml", "updated_at": seconds});
        let file: MaterializedFile = serde_json::from_value(materialized.clone()).unwrap();
        assert_eq!(
            file.updated_at,
            Some(jiff::Timestamp::from_second(seconds).unwrap())
        );
        assert_eq!(serde_json::to_value(&file).unwrap(), materialized);

        let subscription = json!({"expire": seconds});
        let info: SubscriptionInfo = serde_json::from_value(subscription.clone()).unwrap();
        assert_eq!(info.expire, file.updated_at);
        assert_eq!(serde_json::to_value(&info).unwrap(), subscription);

        let managed = ProfileListingBinding::Managed {
            updated_at: file.updated_at,
        };
        assert_eq!(
            serde_json::to_value(managed).unwrap(),
            json!({"type": "managed", "updated_at": seconds})
        );
        let external = ProfileListingBinding::External {
            updated_at: file.updated_at,
            mode: ExternalMode::Mirror,
        };
        assert_eq!(
            serde_json::to_value(external).unwrap(),
            json!({"type": "external", "updated_at": seconds, "mode": "mirror"})
        );
        let remote = ProfileListingSource::Remote {
            updated_at: file.updated_at,
            option: RemoteListingOptions {
                has_user_agent: false,
                with_proxy: true,
                self_proxy: true,
                update_interval_minutes: RemoteSyncInterval::default(),
            },
            subscription: info,
        };
        let listing = serde_json::to_value(remote).unwrap();
        assert_eq!(listing["updated_at"], seconds);
        assert_eq!(listing["subscription"], subscription);
    }
}

#[test]
fn profile_timestamps_keep_the_legacy_yaml_shape_and_pre_epoch_rounding() {
    let fixture = "file: profile.yaml\nupdated_at: -1\n";
    let mut file: MaterializedFile = yaml_serde::from_str(fixture).unwrap();
    assert_eq!(yaml_serde::to_string(&file).unwrap(), fixture);
    file.updated_at = Some(jiff::Timestamp::new(0, -1).unwrap());
    assert_eq!(yaml_serde::to_string(&file).unwrap(), fixture);

    let fixture = "expire: 1720954186\n";
    let info: SubscriptionInfo = yaml_serde::from_str(fixture).unwrap();
    assert_eq!(yaml_serde::to_string(&info).unwrap(), fixture);
}
