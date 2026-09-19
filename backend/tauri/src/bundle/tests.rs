use super::*;
use serde_json::json;

fn config(nightly: bool) -> Config {
    let mut config: Config = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
    if nightly {
        let overrides: serde_json::Value =
            serde_json::from_str(include_str!("../../overrides/nightly.conf.json")).unwrap();
        config
            .plugins
            .0
            .insert("updater".into(), overrides["plugins"]["updater"].clone());
    }
    config
}

fn add_runtime(directory: &Path) {
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(directory.join("msedgewebview2.exe"), b"runtime").unwrap();
}

#[test]
fn resolves_portable_and_fixed_webview_independently() {
    for portable in [false, true] {
        for fixed in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            if portable {
                std::fs::create_dir(directory.path().join(".config")).unwrap();
                std::fs::write(directory.path().join(".config/PORTABLE"), b"").unwrap();
            }
            if fixed {
                add_runtime(&directory.path().join("WebView2"));
            }
            for windows in [false, true] {
                let metadata =
                    BundleMetadata::resolve(windows, &config(false), directory.path()).unwrap();
                assert_eq!(metadata.is_portable, windows && portable);
                assert_eq!(metadata.is_fixed_webview, windows && fixed);
            }
        }
    }
}

#[test]
fn compiled_runtime_path_takes_precedence_over_directory_detection() {
    let directory = tempfile::tempdir().unwrap();
    add_runtime(&directory.path().join("WebView2"));
    let mut config = config(false);
    config.bundle.windows.webview_install_mode = WebviewInstallMode::FixedRuntime {
        path: "custom-runtime".into(),
    };
    assert!(BundleMetadata::resolve(true, &config, directory.path()).is_err());
    add_runtime(&directory.path().join("custom-runtime"));
    let metadata = BundleMetadata::resolve(true, &config, directory.path()).unwrap();
    assert!(metadata.is_fixed_webview);
    metadata.setup(&mut config, directory.path()).unwrap();
    assert!(matches!(
        config.bundle.windows.webview_install_mode,
        WebviewInstallMode::FixedRuntime { path } if path == directory.path().join("custom-runtime")
    ));
}

#[test]
fn incomplete_runtime_fails_without_changing_configuration() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("WebView2")).unwrap();
    let config = config(false);
    let before = serde_json::to_value(&config).unwrap();
    let error = BundleMetadata::resolve(true, &config, directory.path()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("fixed WebView2 runtime is incomplete")
    );
    assert_eq!(serde_json::to_value(&config).unwrap(), before);
}

#[test]
fn setup_injects_runtime_and_preserves_compiled_feeds() {
    for fixed in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        if fixed {
            add_runtime(&directory.path().join("WebView2"));
        }
        for nightly in [false, true] {
            let mut config = config(nightly);
            let mut expected = serde_json::to_value(&config).unwrap();
            let mut metadata = BundleMetadata::resolve(true, &config, directory.path()).unwrap();
            metadata.release_channel = if nightly {
                Channel::Nightly
            } else {
                Channel::Stable
            };
            metadata.setup(&mut config, directory.path()).unwrap();
            if fixed {
                expected["bundle"]["windows"]["webviewInstallMode"] = json!({
                    "type": "fixedRuntime", "path": directory.path().join("WebView2"),
                });
            }
            assert_eq!(serde_json::to_value(config).unwrap(), expected);
        }
    }
}

#[test]
fn metadata_selects_matching_update_downloads() {
    let release: tauri_plugin_updater::RemoteRelease = serde_json::from_value(json!({
        "version": "2.0.0",
        "platforms": {
            "windows-x86_64": {"url": "https://example.com/standard-x86_64.zip", "signature": "standard"},
            "windows-x86_64-fixed-webview": {"url": "https://example.com/fixed-x86_64.zip", "signature": "fixed"},
            "windows-aarch64": {"url": "https://example.com/standard-aarch64.zip", "signature": "standard"},
            "windows-aarch64-fixed-webview": {"url": "https://example.com/fixed-aarch64.zip", "signature": "fixed"},
        },
    })).unwrap();
    for arch in ["x86_64", "aarch64"] {
        for fixed in [false, true] {
            let metadata = BundleMetadata {
                is_portable: false,
                is_fixed_webview: fixed,
                release_channel: crate::bundle::Channel::Stable,
            };
            let target = metadata.updater_target(&format!("windows-{arch}"));
            let variant = if fixed { "fixed" } else { "standard" };
            assert_eq!(
                release.download_url(&target).unwrap().as_str(),
                format!("https://example.com/{variant}-{arch}.zip")
            );
            assert_eq!(release.signature(&target).unwrap(), variant);
        }
    }
}

#[test]
fn release_channel_uses_compiled_feature_and_version() {
    assert_eq!(compiled_channel(false, "2.0.0").unwrap(), Channel::Stable);
    assert_eq!(
        compiled_channel(false, "2.1.0-beta.2").unwrap(),
        Channel::Beta
    );
    assert_eq!(
        compiled_channel(false, "2.1.0-rc.1").unwrap(),
        Channel::Beta
    );
    for version in ["2.0.0", "2.0.0-alpha+hash", "2.0.0+alpha.hash"] {
        assert_eq!(compiled_channel(true, version).unwrap(), Channel::Nightly);
    }
    assert_eq!(Channel::Beta.resolve(None), Channel::Beta);
    assert_eq!(
        Channel::Beta.resolve(Some(Channel::Stable)),
        Channel::Stable
    );
    assert_eq!(
        Channel::Nightly.resolve(Some(Channel::Stable)),
        Channel::Nightly
    );
}

#[test]
fn release_channel_selects_feeds_without_changing_fixed_target() {
    for channel in [Channel::Stable, Channel::Beta, Channel::Nightly] {
        let endpoints = update_endpoints(channel);
        assert_eq!(endpoints.len(), 3);
        let suffix = match channel {
            Channel::Stable => "",
            Channel::Beta => "-beta",
            Channel::Nightly => "-nightly",
        };
        assert!(endpoints[2].ends_with(&format!("/update{suffix}.json")));
        for fixed in [false, true] {
            let metadata = BundleMetadata {
                is_portable: false,
                is_fixed_webview: fixed,
                release_channel: channel,
            };
            let mut config = config(false);
            metadata.setup(&mut config, Path::new("/app")).unwrap();
            assert_eq!(config.plugins.0["updater"]["endpoints"], json!(endpoints));
            assert_eq!(
                metadata.updater_target("windows-aarch64"),
                if fixed {
                    "windows-aarch64-fixed-webview"
                } else {
                    "windows-aarch64"
                }
            );
        }
    }
}

#[test]
fn release_channel_never_downgrades_beta_when_returning_to_stable() {
    let now = time::OffsetDateTime::UNIX_EPOCH;
    let local = semver::Version::parse("2.1.0-beta.2").unwrap();
    for (version, expected) in [
        ("2.0.0", false),
        ("2.1.0-beta.3", false),
        ("2.1.0", true),
        ("2.2.0", true),
    ] {
        let remote = serde_json::from_value(
            json!({ "version": version, "url": "https://example.com/app.zip", "signature": "sig" }),
        )
        .unwrap();
        assert_eq!(
            is_newer_release(Channel::Stable, &local, &remote, now),
            expected,
            "{version}"
        );
    }
    let remote = serde_json::from_value(json!({ "version": "2.1.0-beta.10", "url": "https://example.com/app.zip", "signature": "sig" })).unwrap();
    assert!(is_newer_release(Channel::Beta, &local, &remote, now));
}

#[test]
fn release_channel_only_nightly_accepts_newer_build_of_same_version() {
    let local = semver::Version::parse("2.1.0-alpha+old").unwrap();
    for (date, expected) in [
        ("1970-01-01T00:00:00Z", false),
        ("1970-01-02T00:00:00Z", true),
    ] {
        let remote = serde_json::from_value(json!({ "version": "2.1.0-alpha+new", "pub_date": date, "url": "https://example.com/app.zip", "signature": "sig" })).unwrap();
        assert_eq!(
            is_newer_release(
                Channel::Nightly,
                &local,
                &remote,
                time::OffsetDateTime::UNIX_EPOCH
            ),
            expected
        );
        assert!(!is_newer_release(
            Channel::Beta,
            &local,
            &remote,
            time::OffsetDateTime::UNIX_EPOCH
        ));
    }
}
