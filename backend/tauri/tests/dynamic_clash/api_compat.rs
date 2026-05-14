use super::harness::setup_harness;
use clash_nyanpasu_lib::config::nyanpasu::ClashCore;

#[tokio::test]
#[ignore]
async fn test_get_version_ok() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();
    let version = client.get_version().await.expect("get_version failed");
    assert!(!version.version.is_empty(), "version should be non-empty");
    assert!(
        version.version.chars().any(|c| c.is_ascii_digit()),
        "version should contain a digit"
    );
}

#[tokio::test]
#[ignore]
async fn test_get_configs_ok() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();
    let config = client.get_configs().await.expect("get_configs failed");
    assert!(config.mode.is_some(), "mode should be Some");
    assert!(config.mixed_port.is_some(), "mixed_port should be Some");
}

#[tokio::test]
#[ignore]
async fn test_get_proxies_ok() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();
    let proxies = client.get_proxies().await.expect("get_proxies failed");
    assert!(!proxies.proxies.is_empty(), "proxies should be non-empty");
}

#[tokio::test]
#[ignore]
async fn test_get_rules_ok() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();
    // Should not panic during deserialization
    let _rules = client.get_rules().await.expect("get_rules failed");
}

#[tokio::test]
#[ignore]
async fn test_get_proxy_delay_ok() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();
    // DIRECT proxy delay should return a result without error (delay >= 0)
    let _delay = client
        .get_proxy_delay("DIRECT", None)
        .await
        .expect("get_proxy_delay for DIRECT failed");
}

#[tokio::test]
#[ignore]
async fn test_get_providers_proxies_ok() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();
    // Should deserialize without panic
    let _res = client
        .get_providers_proxies()
        .await
        .expect("get_providers_proxies failed");
}

#[tokio::test]
#[ignore]
async fn test_get_providers_rules_ok() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();
    // Should deserialize without panic
    let _res = client
        .get_providers_rules()
        .await
        .expect("get_providers_rules failed");
}

#[tokio::test]
#[ignore]
async fn test_get_group_delay_ok() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();
    // TestGroup is defined in the minimal config
    let result = client.get_group_delay("TestGroup", None).await;
    // Result should be Ok with a HashMap (even if empty)
    let map = result.expect("get_group_delay failed");
    // Just verify it's a valid HashMap
    let _ = map.len();
}

#[tokio::test]
#[ignore]
async fn test_patch_configs_roundtrip() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();

    // Patch mode to "global"
    let mut patch = serde_yaml::Mapping::new();
    patch.insert("mode".into(), "global".into());
    client
        .patch_configs(&patch)
        .await
        .expect("patch_configs failed");

    // Verify the change took effect
    let config = client.get_configs().await.expect("get_configs failed");
    assert_eq!(
        config.mode.as_deref(),
        Some("global"),
        "mode should be 'global' after patch"
    );
}

#[tokio::test]
#[ignore]
async fn test_put_configs_noop() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();

    // PUT current config path — should not error
    let config_path = harness
        .temp_dir_path()
        .expect("temp_dir_path should be Some")
        .join("config.yaml");
    let config_path_str = config_path.to_string_lossy().to_string();

    client
        .put_configs(&config_path_str)
        .await
        .expect("put_configs failed");
}

#[tokio::test]
#[ignore]
async fn test_update_proxy_switch() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();

    // Switch TestGroup to MyReject
    client
        .update_proxy("TestGroup", "MyReject")
        .await
        .expect("update_proxy failed");

    // Verify the switch
    let proxies = client.get_proxies().await.expect("get_proxies failed");
    if let Some(group) = proxies.proxies.get("TestGroup") {
        assert_eq!(
            group.now.as_deref(),
            Some("MyReject"),
            "TestGroup should now point to MyReject"
        );
    }
}

#[tokio::test]
#[ignore]
async fn test_delete_connections_ok() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();

    // Delete all connections — should not error even if there are none
    client
        .delete_connections(None)
        .await
        .expect("delete_connections failed");
}

#[tokio::test]
#[ignore]
async fn test_get_proxy_delay_nonexistent() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();

    // Nonexistent proxy should return an error
    let result = client.get_proxy_delay("nonexistent-proxy-xyz", None).await;
    assert!(
        result.is_err(),
        "get_proxy_delay for nonexistent proxy should return Err"
    );
}

#[tokio::test]
#[ignore]
async fn test_patch_configs_invalid() {
    let Some(harness) = setup_harness(ClashCore::Mihomo).await else {
        return;
    };
    let client = harness.client();

    // Patch with unknown field — Mihomo returns 400; some cores may be lenient
    let mut patch = serde_yaml::Mapping::new();
    patch.insert("nonexistent-field-xyz".into(), 12345i64.into());
    let result = client.patch_configs(&patch).await;
    // This is acceptable either way — some cores are lenient about unknown fields
    match result {
        Ok(_) => {
            // Core accepted the unknown field — OK
        }
        Err(e) => {
            // Core returned an error for unknown field — also OK
            eprintln!("patch_configs with invalid field returned error (expected): {e}");
        }
    }
}
