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
use url::Url;

/// 测试整数类型不匹配问题
/// 这是原始问题的核心：YAML 解析时整数类型可能不一致
#[test]
fn test_integer_type_mismatch_in_yaml() {
    // 测试不同的整数表示形式
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

    // 应该都能成功解析
    let profile1: Result<Profile, _> = serde_yaml::from_str(yaml_with_i32);
    let profile2: Result<Profile, _> = serde_yaml::from_str(yaml_with_i64);
    let profile3: Result<Profile, _> = serde_yaml::from_str(yaml_with_u64);

    assert!(profile1.is_ok(), "Failed to parse i32: {:?}", profile1);
    assert!(profile2.is_ok(), "Failed to parse i64: {:?}", profile2);
    // u64 最大值可能会被转换为 usize，在 32 位系统上可能失败
    if std::mem::size_of::<usize>() == 8 {
        assert!(profile3.is_ok(), "Failed to parse u64: {:?}", profile3);
    }
}

/// 测试 tagged enum 的正确序列化和反序列化
#[test]
fn test_tagged_enum_serialization() {
    // 创建不同类型的 Profile
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

    // 测试序列化
    let remote_yaml = serde_yaml::to_string(&remote_profile).unwrap();
    let local_yaml = serde_yaml::to_string(&local_profile).unwrap();
    let merge_yaml = serde_yaml::to_string(&merge_profile).unwrap();
    let script_yaml = serde_yaml::to_string(&script_profile).unwrap();

    println!("Remote YAML:\n{}", remote_yaml);
    println!("Local YAML:\n{}", local_yaml);
    println!("Merge YAML:\n{}", merge_yaml);
    println!("Script YAML:\n{}", script_yaml);

    // 验证 YAML 包含正确的 type 标签
    assert!(remote_yaml.contains("type: remote"));
    assert!(local_yaml.contains("type: local"));
    assert!(merge_yaml.contains("type: merge"));
    assert!(script_yaml.contains("type: script"));

    // 测试反序列化
    let remote_parsed: Profile = serde_yaml::from_str(&remote_yaml).unwrap();
    let local_parsed: Profile = serde_yaml::from_str(&local_yaml).unwrap();
    let merge_parsed: Profile = serde_yaml::from_str(&merge_yaml).unwrap();
    let script_parsed: Profile = serde_yaml::from_str(&script_yaml).unwrap();

    // 验证反序列化后的类型正确
    assert!(matches!(remote_parsed, Profile::Remote(_)));
    assert!(matches!(local_parsed, Profile::Local(_)));
    assert!(matches!(merge_parsed, Profile::Merge(_)));
    assert!(matches!(script_parsed, Profile::Script(_)));
}

#[test]
fn test_backward_compatibility() {
    // 测试新的脚本格式能被正确识别
    let new_format = r#"uid: siL1cvjnvLB6
type: script
script_type: javascript
name: 花☁️处理
file: siL1cvjnvLB6.js
desc: ''
updated: 1720954186"#;
    serde_yaml::from_str::<Profile>(new_format).expect("new format should works");
}

/// 测试 ProfileKindGetter trait
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

/// 测试 builder 的默认值设置
#[tokio::test(flavor = "multi_thread")]
async fn test_builder_defaults() {
    let remote_builder = RemoteProfile::builder();
    let local_builder = LocalProfile::builder();
    let merge_builder = MergeProfile::builder();
    // let script_builder = ScriptProfile::builder(&ScriptType::JavaScript);

    // 构建时应该自动填充默认值
    let mut remote_builder = remote_builder;
    remote_builder.url(
        Url::parse(
            "https://raw.githubusercontent.com/MetaCubeX/mihomo/refs/heads/Meta/docs/config.yaml",
        )
        .unwrap(),
    );
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

/// 测试错误处理
#[test]
fn test_error_handling() {
    // 无效的 type 值
    let invalid_type = r#"
type: invalid_type
uid: "test"
name: "Test"
"#;
    let result: Result<Profile, _> = serde_yaml::from_str(invalid_type);
    assert!(result.is_err());

    // Script 类型但缺少 script_type
    let missing_script_type = r#"
type: script
uid: "script-test"
name: "Script Test"
"#;
    let result: Result<Profile, _> = serde_yaml::from_str(missing_script_type);
    // 应该使用默认的 script_type 或者失败
    println!("Script without script_type result: {:?}", result);

    // Remote 类型但缺少必需的 url 字段
    let missing_url = r#"
type: remote
uid: "remote-test"
name: "Remote Test"
"#;
    let result: Result<Profile, _> = serde_yaml::from_str(missing_url);
    assert!(result.is_err(), "Should fail without required url field");
}

/// 测试大数字的处理
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
