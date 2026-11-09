//! Pure hotkey parsing and facade action tests.

use super::ports::{HotkeyAction, HotkeyBindings, HotkeyOp, HotkeyParseError};
use crate::client::test_support::AnyAccelerator;

fn entries(raw: &[&str]) -> Vec<String> {
    raw.iter().map(ToString::to_string).collect()
}

#[test]
fn transparent_proxy_action_patches_mode_and_listener_without_changing_scope() {
    use nyanpasu_config::clash::config::{
        ClashConfig, TransparentProxyMode, clash_strategy::PortStrategyKind,
    };
    use struct_patch::Patch as _;
    let mut config = ClashConfig::default();
    config.transparent_proxy.local = false;
    config.transparent_proxy.interfaces = vec!["eth0".into()];
    config.apply(super::transparent_proxy_patch(&config, true));
    assert_eq!(config.transparent_proxy.mode, TransparentProxyMode::Tproxy);
    assert_eq!(
        config.tproxy_port.as_ref().unwrap().kind,
        PortStrategyKind::Fixed
    );
    assert_eq!(config.tproxy_port.as_ref().unwrap().start_port, 7894);
    assert!(!config.transparent_proxy.local);
    assert_eq!(config.transparent_proxy.interfaces, ["eth0"]);
    config.apply(super::transparent_proxy_patch(&config, false));
    assert_eq!(
        config.transparent_proxy.mode,
        TransparentProxyMode::Disabled
    );
    assert!(config.tproxy_port.is_some());
}

#[test]
fn transparent_proxy_hotkey_names_round_trip() {
    for action in [
        HotkeyAction::ToggleTransparentProxy,
        HotkeyAction::EnableTransparentProxy,
        HotkeyAction::DisableTransparentProxy,
    ] {
        assert_eq!(action.as_str().parse::<HotkeyAction>().unwrap(), action);
    }
}

#[test]
fn parse_accepts_the_legacy_func_comma_key_format() {
    let bindings = HotkeyBindings::parse(
        &entries(&[
            "open_or_close_dashboard,Control+Q",
            " toggle_tun_mode , Control+Shift+T ",
        ]),
        &AnyAccelerator,
    )
    .expect("both entries are well formed");

    assert_eq!(
        bindings.as_map().get("Control+Q"),
        Some(&HotkeyAction::OpenOrCloseDashboard)
    );
    assert_eq!(
        bindings.as_map().get("Control+Shift+T"),
        Some(&HotkeyAction::ToggleTunMode),
        "surrounding whitespace is not part of the accelerator"
    );
}

#[test]
fn parse_rejects_malformed_unknown_invalid_and_missing_super() {
    let error =
        HotkeyBindings::parse(&entries(&["open_or_close_dashboard"]), &AnyAccelerator).unwrap_err();
    assert!(
        matches!(&error, HotkeyParseError::MalformedEntry { entry } if entry == "open_or_close_dashboard"),
        "{error:?}"
    );
    let error =
        HotkeyBindings::parse(&entries(&["make_coffee,Control+Q"]), &AnyAccelerator).unwrap_err();
    assert!(
        matches!(&error, HotkeyParseError::UnknownFunction { function } if function == "make_coffee"),
        "{error:?}"
    );
    let error = HotkeyBindings::parse(&entries(&["toggle_tun_mode,Control++"]), &AnyAccelerator)
        .unwrap_err();
    assert!(
        matches!(&error, HotkeyParseError::EmptyKeySegment { accelerator } if accelerator == "Control++"),
        "{error:?}"
    );
    let error =
        HotkeyBindings::parse(&entries(&["toggle_tun_mode,Q"]), &AnyAccelerator).unwrap_err();
    assert!(
        matches!(&error, HotkeyParseError::MissingSuperKey { accelerator } if accelerator == "Q"),
        "a bare key would swallow ordinary typing: {error:?}"
    );
}

#[test]
fn parse_rejects_duplicate_accelerator() {
    let error = HotkeyBindings::parse(
        &entries(&["enable_tun_mode,Control+Q", "disable_tun_mode,Control+Q"]),
        &AnyAccelerator,
    )
    .unwrap_err();
    assert!(
        matches!(
            &error,
            HotkeyParseError::DuplicateAccelerator { accelerator, first, second }
                if accelerator == "Control+Q"
                    && *first == HotkeyAction::EnableTunMode
                    && *second == HotkeyAction::DisableTunMode
        ),
        "{error:?}"
    );
}

#[test]
fn diff_emits_unbind_rebind_and_bind() {
    let before = HotkeyBindings::from_pairs([
        ("Control+A", HotkeyAction::EnableTunMode),
        ("Control+B", HotkeyAction::EnableSystemProxy),
        ("Control+C", HotkeyAction::ClashModeRule),
    ]);
    let after = HotkeyBindings::from_pairs([
        ("Control+B", HotkeyAction::DisableSystemProxy),
        ("Control+C", HotkeyAction::ClashModeRule),
        ("Control+D", HotkeyAction::ToggleTunMode),
    ]);

    let ops = before.diff(&after);

    assert_eq!(
        ops,
        vec![
            HotkeyOp::Unbind {
                accelerator: "Control+A".to_owned()
            },
            HotkeyOp::Rebind {
                accelerator: "Control+B".to_owned(),
                action: HotkeyAction::DisableSystemProxy,
            },
            HotkeyOp::Bind {
                accelerator: "Control+D".to_owned(),
                action: HotkeyAction::ToggleTunMode,
            },
        ],
        "an unchanged binding produces nothing at all"
    );
}

mod facade {
    //! What each action does once it reaches the client. The effect port is the
    //! no-op one, so these assert on committed configuration rather than on the
    //! OS.

    use std::sync::Arc;

    use nyanpasu_config::application::NyanpasuAppConfig;
    use tempfile::{TempDir, tempdir};

    use super::super::ports::{HotkeyAction, HotkeyParseError};
    use crate::{
        actor_v2::test_support::test_idle_endpoint,
        client::{
            ClientError, NyanpasuClient, hotkey::ports::MockWindowControl,
            test_support::test_client_args_with_endpoint,
        },
        state::config_error::ConfigError,
    };

    async fn client_with_window(dir: &TempDir, window: MockWindowControl) -> NyanpasuClient {
        let mut args = test_client_args_with_endpoint(
            dir,
            test_idle_endpoint(),
            crate::client::test_support::profile_test_adapters(dir),
        )
        .await;
        args.window = Some(Arc::new(window));
        NyanpasuClient::try_new(args)
            .await
            .expect("client should construct")
    }

    async fn client(dir: &TempDir) -> NyanpasuClient {
        client_with_window(dir, MockWindowControl::new()).await
    }

    /// A mode change would otherwise ask the core to drop connections, which
    /// the stub endpoint cannot answer.
    async fn disable_mode_interruption(client: &NyanpasuClient) {
        use struct_patch::Patch as _;
        let mut config = client.get_clash_config().await.unwrap();
        config.break_connection.on_mode_change = false;
        let mut patch = nyanpasu_config::clash::config::ClashConfig::new_empty_patch();
        patch.break_connection = config.break_connection.into_patch();
        client.patch_clash_config(patch).await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispatch_toggle_system_proxy_flips_app_config() {
        let dir = tempdir().expect("tempdir should be created");
        let client = client(&dir).await;

        (async {
            assert!(!client.get_app_config().await.unwrap().enable_system_proxy);

            client
                .dispatch_hotkey_action(HotkeyAction::ToggleSystemProxy)
                .await
                .expect("the toggle should commit");
            assert!(client.get_app_config().await.unwrap().enable_system_proxy);

            client
                .dispatch_hotkey_action(HotkeyAction::ToggleSystemProxy)
                .await
                .expect("the toggle should commit");
            assert!(!client.get_app_config().await.unwrap().enable_system_proxy);

            client
                .dispatch_hotkey_action(HotkeyAction::EnableSystemProxy)
                .await
                .expect("the explicit enable should commit");
            assert!(client.get_app_config().await.unwrap().enable_system_proxy);

            client
                .dispatch_hotkey_action(HotkeyAction::DisableSystemProxy)
                .await
                .expect("the explicit disable should commit");
            assert!(!client.get_app_config().await.unwrap().enable_system_proxy);
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn transparent_proxy_action_does_not_commit_without_a_capable_service() {
        use nyanpasu_config::clash::config::TransparentProxyMode;
        let dir = tempdir().unwrap();
        let client = client(&dir).await;
        assert!(
            client
                .dispatch_hotkey_action(HotkeyAction::EnableTransparentProxy)
                .await
                .is_err()
        );
        let config = client.get_clash_config().await.unwrap();
        assert_eq!(
            config.transparent_proxy.mode,
            TransparentProxyMode::Disabled
        );
        assert!(config.tproxy_port.is_none());
        client
            .dispatch_hotkey_action(HotkeyAction::DisableTransparentProxy)
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispatch_toggle_tun_mode_flips_clash_config() {
        let dir = tempdir().expect("tempdir should be created");
        let client = client(&dir).await;

        (async {
            let before = client.get_clash_config().await.unwrap().enable_tun_mode;

            client
                .dispatch_hotkey_action(HotkeyAction::ToggleTunMode)
                .await
                .expect("the toggle should commit");
            assert_eq!(
                client.get_clash_config().await.unwrap().enable_tun_mode,
                !before
            );

            client
                .dispatch_hotkey_action(HotkeyAction::DisableTunMode)
                .await
                .expect("the explicit disable should commit");
            assert!(!client.get_clash_config().await.unwrap().enable_tun_mode);

            client
                .dispatch_hotkey_action(HotkeyAction::EnableTunMode)
                .await
                .expect("the explicit enable should commit");
            assert!(client.get_clash_config().await.unwrap().enable_tun_mode);
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispatch_clash_mode_patches_runtime_overrides() {
        let dir = tempdir().expect("tempdir should be created");
        let client = client(&dir).await;

        (async {
            disable_mode_interruption(&client).await;

            client
                .dispatch_hotkey_action(HotkeyAction::ClashModeGlobal)
                .await
                .expect("the mode change should commit");

            assert_eq!(
                serde_json::to_value(client.get_clash_config().await.unwrap().overrides).unwrap()["mode"],
                "global"
            );
        }).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dispatch_dashboard_goes_to_the_injected_window_control() {
        let dir = tempdir().expect("tempdir should be created");
        let mut window = MockWindowControl::new();
        window
            .expect_toggle_dashboard()
            .times(1)
            .returning(|| Ok(()));
        let client = client_with_window(&dir, window).await;

        (async {
            client
                .dispatch_hotkey_action(HotkeyAction::OpenOrCloseDashboard)
                .await
                .expect("the dashboard toggle should succeed");
        })
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn invalid_hotkey_is_rejected_before_commit() {
        let dir = tempdir().expect("tempdir should be created");
        let client = client(&dir).await;

        (async {
            let mut patch = <NyanpasuAppConfig as struct_patch::Patch<_>>::new_empty_patch();
            patch.hotkeys = Some(vec!["toggle_tun_mode,Q".to_owned()]);

            let error = client
                .patch_app_config(patch)
                .await
                .expect_err("a hotkey without a modifier must not be persisted");
            assert!(
                matches!(
                    &error,
                    ClientError::Config(ConfigError::ValidateHotkeys {
                        source: HotkeyParseError::MissingSuperKey { accelerator },
                    }) if accelerator == "Q"
                ),
                "unexpected error: {error:?}"
            );
            assert!(
                client.get_app_config().await.unwrap().hotkeys.is_empty(),
                "nothing may be written when validation fails"
            );
        })
        .await;
    }
}
