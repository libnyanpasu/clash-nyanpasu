//! Real shortcut parser behavior and facade validation before committing hotkeys.

use std::sync::Arc;

use crate::adapters::hotkey::PlatformAcceleratorValidator;
use nyanpasu_config::application::NyanpasuAppConfig;
use nyanpasu_core::{
    actor_v2::{endpoint::EndpointHandle, test_support::test_idle_endpoint},
    client::{
        ClientError, ClientSetupArgs, NyanpasuClient,
        effects::ports::MockApplicationEffectsPort,
        hotkey::ports::{HotkeyBindings, HotkeyParseError},
    },
    state::config_error::ConfigError,
};
use tempfile::{TempDir, tempdir};

fn entries(raw: &[&str]) -> Vec<String> {
    raw.iter().map(ToString::to_string).collect()
}

async fn test_client_args_with_endpoint(
    dir: &TempDir,
    endpoint: EndpointHandle,
) -> ClientSetupArgs {
    let mut args = crate::test_support::test_client_args_with_endpoint(dir, endpoint).await;
    args.accelerators = Some(Arc::new(PlatformAcceleratorValidator));
    args
}

#[test]
fn parse_rejects_what_the_platform_parser_refuses() {
    let error = HotkeyBindings::parse(
        &entries(&["toggle_tun_mode,Control+DefinitelyNotAKey"]),
        &PlatformAcceleratorValidator,
    )
    .unwrap_err();
    assert!(
        matches!(
            &error,
            HotkeyParseError::UnsupportedAccelerator { accelerator, .. }
                if accelerator == "Control+DefinitelyNotAKey"
        ),
        "the shape rules alone cannot tell a key name from a typo: {error:?}"
    );
    assert!(
        HotkeyBindings::parse(
            &entries(&["toggle_tun_mode,Control+Shift+T"]),
            &PlatformAcceleratorValidator,
        )
        .is_ok()
    );
}

/// The OS keys a grab by the parsed shortcut, not by the text, so two
/// spellings of one accelerator would otherwise be two bindings here and one
/// registration there — the second silently replacing the first.
#[test]
fn equivalent_spellings_are_the_same_binding() {
    let abbreviated = HotkeyBindings::parse(
        &entries(&["toggle_tun_mode,Ctrl+Q"]),
        &PlatformAcceleratorValidator,
    )
    .expect("an abbreviated modifier is a valid accelerator");
    let spelled_out = HotkeyBindings::parse(
        &entries(&["toggle_tun_mode,Control+Q"]),
        &PlatformAcceleratorValidator,
    )
    .expect("the long modifier is the same accelerator");

    assert_eq!(abbreviated, spelled_out);
    assert!(
        abbreviated.diff(&spelled_out).is_empty(),
        "rewriting a binding into the other spelling must not touch the OS"
    );
    let error = HotkeyBindings::parse(
        &entries(&["enable_tun_mode,Ctrl+Q", "disable_tun_mode,Control+Q"]),
        &PlatformAcceleratorValidator,
    )
    .unwrap_err();
    assert!(
        matches!(&error, HotkeyParseError::DuplicateAccelerator { accelerator, .. } if accelerator == "Control+Q"),
        "one grab cannot run two functions: {error:?}"
    );
}

/// The platform parser is the authority on an accelerator, and it has to be
/// consulted here: after the commit the effect has already released the
/// shortcuts that used to work, and the junk is on disk.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn platform_invalid_accelerator_is_rejected_before_commit() {
    let dir = tempdir().expect("tempdir should be created");
    let mut effects = MockApplicationEffectsPort::new();
    effects.expect_apply().never();
    let mut args = test_client_args_with_endpoint(&dir, test_idle_endpoint()).await;
    args.effects = Arc::new(effects);
    let client = NyanpasuClient::try_new(args)
        .await
        .expect("client should construct");

    (async {
        let mut patch = <NyanpasuAppConfig as struct_patch::Patch<_>>::new_empty_patch();
        patch.hotkeys = Some(vec!["toggle_tun_mode,Control+DefinitelyNotAKey".to_owned()]);

        let error = client
            .patch_app_config(patch)
            .await
            .expect_err("an accelerator the platform cannot parse must not be persisted");
        assert!(
            matches!(
                &error,
                ClientError::Config(ConfigError::ValidateHotkeys {
                    source: HotkeyParseError::UnsupportedAccelerator { accelerator, .. },
                }) if accelerator == "Control+DefinitelyNotAKey"
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn duplicate_accelerators_are_rejected_before_commit() {
    use nyanpasu_persistence::format::Format as _;

    let dir = tempdir().expect("tempdir should be created");
    let mut effects = MockApplicationEffectsPort::new();
    effects.expect_apply().never();
    let mut args = test_client_args_with_endpoint(&dir, test_idle_endpoint()).await;
    let config_path = args.paths.application_config_path();
    let seed = NyanpasuAppConfig {
        hotkeys: entries(&["toggle_tun_mode,Control+T"]),
        ..Default::default()
    };
    let mut content = Vec::new();
    nyanpasu_migration::modules::application::ApplicationFormat::default()
        .serialize(&mut content, &seed, None)
        .unwrap();
    std::fs::write(&config_path, content).unwrap();
    args.effects = Arc::new(effects);
    let client = NyanpasuClient::try_new(args)
        .await
        .expect("client should construct");
    let before = serde_json::to_value(client.get_app_config().await.unwrap()).unwrap();
    let bytes_before = std::fs::read(&config_path).expect("the initial config is persisted");

    let mut patch = <NyanpasuAppConfig as struct_patch::Patch<_>>::new_empty_patch();
    patch.hotkeys = Some(entries(&[
        "enable_tun_mode,Ctrl+Q",
        "disable_tun_mode,Control+Q",
    ]));

    let error = client
        .patch_app_config(patch)
        .await
        .expect_err("equivalent accelerators for different actions must not be persisted");
    assert!(
        matches!(
            &error,
            ClientError::Config(ConfigError::ValidateHotkeys {
                source: HotkeyParseError::DuplicateAccelerator { accelerator, .. },
            }) if accelerator == "Control+Q"
        ),
        "unexpected error: {error:?}"
    );
    assert_eq!(std::fs::read(&config_path).unwrap(), bytes_before);
    assert_eq!(
        serde_json::to_value(client.get_app_config().await.unwrap()).unwrap(),
        before,
        "validation must leave the whole committed config unchanged"
    );
}
