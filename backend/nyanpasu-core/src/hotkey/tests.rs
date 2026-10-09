//! Pure parsing and binding-diff tests.

use super::{AcceleratorValidator, HotkeyAction, HotkeyBindings, HotkeyOp, HotkeyParseError};

fn entries(raw: &[&str]) -> Vec<String> {
    raw.iter().map(ToString::to_string).collect()
}

/// Accepts anything the shape rules already let through, so the registration
/// tests read as the spelling under test rather than as a platform verdict.
struct AnyAccelerator;

impl AcceleratorValidator for AnyAccelerator {
    fn validate(&self, _accelerator: &str) -> Result<(), HotkeyParseError> {
        Ok(())
    }

    fn canonical(&self, accelerator: &str) -> Result<String, HotkeyParseError> {
        Ok(accelerator.to_owned())
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
