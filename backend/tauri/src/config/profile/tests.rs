// TODO: integrate ClashTestHarness for dynamic profile tests
// Flow:
// 1. harness = ClashTestHarness::new(config).await
// 2. client = harness.client()
// 3. fetch remote profile via subscription URL -> write config -> PUT /configs
// 4. call client.get_proxies() etc. to assert the loaded config is correct
// 5. harness drops and cleans up automatically
// Note: Profile module is being refactored (2026-05); wire this up after the refactor.
// See: specs/20260515-dynamic-clash-testing/

use crate::{
    config::profile::{
        item::{
            LocalProfile, MergeProfile, Profile, RemoteProfile, RemoteProfileOptions,
            ScriptProfile, SubscriptionInfo,
        },
        item_type::ProfileItemType,
    },
    enhance::ScriptType,
};
use serde_yaml;
use tokio_util::sync::CancellationToken;
use url::Url;

const REMOTE_SAMPLE_DATA: &str = include_str!("../../../tests/sample_clash_config.yaml");

struct Guard(CancellationToken, Option<tokio::task::JoinHandle<()>>);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.cancel();
        if let Some(handle) = self.1.take() {
            nyanpasu_utils::runtime::block_on_anywhere(handle).unwrap();
        }
    }
}

async fn create_test_server() -> (Guard, url::Url) {
    let port = port_scanner::request_open_port().unwrap();
    let url = Url::parse(&format!("http://127.0.0.1:{port}")).unwrap();
    let token = CancellationToken::new();
    let token_clone = token.clone();
    let (is_ready_tx, is_ready_rx) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
            .await
            .unwrap();
        let _ = is_ready_tx.send(());
        let app = axum::Router::new().route(
            "/sample_clash_config",
            axum::routing::get(|| async { REMOTE_SAMPLE_DATA }),
        );
        axum::serve(listener, app.into_make_service())
            .with_graceful_shutdown(async move { token.cancelled().await })
            .await
            .unwrap();
    });
    let _ = is_ready_rx.await;
    let guard = Guard(token_clone, Some(handle));
    (guard, url)
}

/// Tests that integer values of different sizes round-trip correctly through YAML.
#[test]
fn test_integer_type_mismatch_in_yaml() {
    let yaml_with_i32 = r#"
type: remote
uid: "test-uid-1"
name: "Test Profile"
updated: 1234567890
url: "https://example.com/config.yaml"
file: sample.yaml
"#;

    let yaml_with_i64 = r#"
type: remote
uid: "test-uid-2"
name: "Test Profile"
updated: 9999999999999
url: "https://example.com/config.yaml"
file: sample.yaml
"#;

    let yaml_with_u64 = r#"
type: remote
uid: "test-uid-3"
name: "Test Profile"
updated: 18446744073709551615
url: "https://example.com/config.yaml"
file: sample.yaml
"#;

    let profile1: Result<Profile, _> = serde_yaml::from_str(yaml_with_i32);
    let profile2: Result<Profile, _> = serde_yaml::from_str(yaml_with_i64);
    let profile3: Result<Profile, _> = serde_yaml::from_str(yaml_with_u64);

    assert!(profile1.is_ok(), "Failed to parse i32: {:?}", profile1);
    assert!(profile2.is_ok(), "Failed to parse i64: {:?}", profile2);
    // usize is 4 bytes on 32-bit platforms, so u64::MAX may overflow there
    if std::mem::size_of::<usize>() == 8 {
        assert!(profile3.is_ok(), "Failed to parse u64: {:?}", profile3);
    }
}

/// Tests tagged-enum serialization/deserialization roundtrip for all profile kinds.
#[test]
fn test_tagged_enum_serialization() {
    let remote_profile = Profile::Remote(RemoteProfile {
        shared: crate::config::profile::item::ProfileShared {
            uid: "remote-1".to_string(),
            name: "Remote Profile".to_string(),
            file: "remote-1.yaml".to_string(),
            desc: Some("A remote profile".to_string()),
            updated: 1234567890,
        },
        url: Url::parse("https://example.com/config.yaml").unwrap(),
        extra: SubscriptionInfo::default(),
        option: RemoteProfileOptions::default(),
        chain: vec![],
    });

    let local_profile = Profile::Local(LocalProfile {
        shared: crate::config::profile::item::ProfileShared {
            uid: "local-1".to_string(),
            name: "Local Profile".to_string(),
            file: "local-1.yaml".to_string(),
            desc: None,
            updated: 1234567890,
        },
        symlinks: None,
        chain: vec![],
    });

    let merge_profile = Profile::Merge(MergeProfile {
        shared: crate::config::profile::item::ProfileShared {
            uid: "merge-1".to_string(),
            name: "Merge Profile".to_string(),
            file: "merge-1.yaml".to_string(),
            desc: Some("Merge multiple profiles".to_string()),
            updated: 1234567890,
        },
    });

    let script_profile = Profile::Script(ScriptProfile {
        shared: crate::config::profile::item::ProfileShared {
            uid: "script-1".to_string(),
            name: "Script Profile".to_string(),
            file: "script-1.js".to_string(),
            desc: None,
            updated: 1234567890,
        },
        script_type: ScriptType::JavaScript,
    });

    let remote_yaml = serde_yaml::to_string(&remote_profile).unwrap();
    let local_yaml = serde_yaml::to_string(&local_profile).unwrap();
    let merge_yaml = serde_yaml::to_string(&merge_profile).unwrap();
    let script_yaml = serde_yaml::to_string(&script_profile).unwrap();

    println!("Remote YAML:\n{}", remote_yaml);
    println!("Local YAML:\n{}", local_yaml);
    println!("Merge YAML:\n{}", merge_yaml);
    println!("Script YAML:\n{}", script_yaml);

    assert!(remote_yaml.contains("type: remote"));
    assert!(local_yaml.contains("type: local"));
    assert!(merge_yaml.contains("type: merge"));
    assert!(script_yaml.contains("type: script"));

    let remote_parsed: Profile = serde_yaml::from_str(&remote_yaml).unwrap();
    let local_parsed: Profile = serde_yaml::from_str(&local_yaml).unwrap();
    let merge_parsed: Profile = serde_yaml::from_str(&merge_yaml).unwrap();
    let script_parsed: Profile = serde_yaml::from_str(&script_yaml).unwrap();

    assert!(matches!(remote_parsed, Profile::Remote(_)));
    assert!(matches!(local_parsed, Profile::Local(_)));
    assert!(matches!(merge_parsed, Profile::Merge(_)));
    assert!(matches!(script_parsed, Profile::Script(_)));
}

#[test]
fn test_backward_compatibility() {
    let new_format = r#"uid: siL1cvjnvLB6
type: script
script_type: javascript
name: 花☁️处理
file: siL1cvjnvLB6.js
desc: ''
updated: 1720954186"#;
    serde_yaml::from_str::<Profile>(new_format).expect("new format should works");
}

/// Tests ProfileKindGetter returns the correct variant for each profile kind.
#[test]
fn test_profile_kind_getter() {
    use crate::config::ProfileKindGetter;

    let remote = RemoteProfile {
        shared: Default::default(),
        url: Url::parse("https://example.com").unwrap(),
        extra: SubscriptionInfo::default(),
        option: RemoteProfileOptions::default(),
        chain: vec![],
    };
    assert_eq!(remote.kind(), ProfileItemType::Remote);

    let local = LocalProfile {
        shared: Default::default(),
        symlinks: None,
        chain: vec![],
    };
    assert_eq!(local.kind(), ProfileItemType::Local);

    let merge = MergeProfile {
        shared: Default::default(),
    };
    assert_eq!(merge.kind(), ProfileItemType::Merge);

    let script_js = ScriptProfile {
        shared: Default::default(),
        script_type: ScriptType::JavaScript,
    };
    assert_eq!(
        script_js.kind(),
        ProfileItemType::Script(ScriptType::JavaScript)
    );
}

/// Tests that builders auto-fill uid, name, and file with sensible defaults.
#[test_log::test(tokio::test(flavor = "multi_thread"))]
async fn test_builder_defaults() {
    let (_guard, mut url) = create_test_server().await;
    let remote_builder = RemoteProfile::builder();
    let local_builder = LocalProfile::builder();
    let merge_builder = MergeProfile::builder();
    // let script_builder = ScriptProfile::builder(&ScriptType::JavaScript);

    let mut remote_builder = remote_builder;
    url.set_path("sample_clash_config");
    remote_builder.url(url.clone());
    let remote = remote_builder.build().expect("build remote profile");
    assert!(!remote.shared.uid.is_empty());
    assert!(!remote.shared.name.is_empty());
    assert!(!remote.shared.file.is_empty());

    let local = local_builder.build();
    assert!(local.is_ok());
    let local = local.unwrap();
    assert!(!local.shared.uid.is_empty());
    assert_eq!(local.shared.name, "Local Profile");

    let merge = merge_builder.build();
    assert!(merge.is_ok());
    let merge = merge.unwrap();
    assert!(!merge.shared.uid.is_empty());
    assert_eq!(merge.shared.name, "Merge Profile");
}

/// Tests that invalid or incomplete YAML produces deserialization errors.
#[test]
fn test_error_handling() {
    let invalid_type = r#"
type: invalid_type
uid: "test"
name: "Test"
"#;
    let result: Result<Profile, _> = serde_yaml::from_str(invalid_type);
    assert!(result.is_err());

    let missing_script_type = r#"
type: script
uid: "script-test"
name: "Script Test"
"#;
    let result: Result<Profile, _> = serde_yaml::from_str(missing_script_type);
    println!("Script without script_type result: {:?}", result);

    let missing_url = r#"
type: remote
uid: "remote-test"
name: "Remote Test"
"#;
    let result: Result<Profile, _> = serde_yaml::from_str(missing_url);
    assert!(result.is_err(), "Should fail without required url field");
}

/// Tests that usize-range values round-trip correctly through YAML.
#[test]
fn test_large_numbers() {
    let test_cases = vec![
        (0usize, "zero"),
        (1234567890usize, "normal"),
        (usize::MAX, "max"),
    ];

    for (value, desc) in test_cases {
        let profile = Profile::Local(LocalProfile {
            shared: crate::config::profile::item::ProfileShared {
                uid: format!("test-{}", desc),
                name: format!("Test {}", desc),
                file: format!("test-{}.yaml", desc),
                desc: None,
                updated: value,
            },
            symlinks: None,
            chain: vec![],
        });

        let yaml = serde_yaml::to_string(&profile).unwrap();
        let parsed: Profile = serde_yaml::from_str(&yaml).unwrap();

        if let Profile::Local(local) = parsed {
            assert_eq!(local.shared.updated, value, "Failed for {}", desc);
        } else {
            panic!("Expected Local profile");
        }
    }
}
