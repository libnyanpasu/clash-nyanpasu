use super::*;
use crate::{
    client::{self, SetupError},
    core::{
        actor_v2::local_host,
        migration::{runner, store},
    },
};

fn paths() -> PathResolver {
    PathResolver::with_base_dirs("config".into(), "data".into())
}

#[test]
fn typed_setup_and_rpc_sources_select_failure_kinds_and_paths() {
    let paths = paths();
    let cases = [
        (
            setup::Error::LocateInstallDirectory {
                source: nyanpasu_paths::InstallDirError::NotUtf8 {
                    path: "install".into(),
                },
            },
            FailureKind::Directories,
            Some("install".into()),
        ),
        (
            setup::Error::RunMigrations {
                source: runner::Error::ReadConfigDirectory {
                    path: "config".into(),
                    source: io::Error::other("cannot read"),
                },
            },
            FailureKind::MigrationFailed,
            Some("config".into()),
        ),
        (
            setup::Error::PrepareMigrations {
                source: runner::Error::LoadState {
                    source: store::Error::NewerSchema {
                        path: "migration-state.yaml".into(),
                        read: 100,
                        supported: 1,
                    },
                },
            },
            FailureKind::NewerVersion,
            Some("migration-state.yaml".into()),
        ),
        (
            setup::Error::BuildClient {
                source: SetupError::LoadApplication {
                    source: client::application::Error::ValidateUpdateSources { reason: "empty" },
                },
            },
            FailureKind::UserConfig,
            Some(paths.application_config_path().into_std_path_buf()),
        ),
        (
            setup::Error::BuildCoreHost {
                source: local_host::Error::BuildManager {
                    source: nyanpasu_core_manager::Error::UnsafeRuntimeArtifact(
                        "runtime/control/lock".into(),
                    ),
                },
            },
            FailureKind::CoreRuntimeDirectory,
            Some("runtime/control/lock".into()),
        ),
        (
            setup::Error::StartJobs {
                source: client::jobs::Error::OpenStore {
                    path: paths.jobs_path().into_std_path_buf(),
                    source: nyanpasu_jobs::Error::Journal("sequence exhausted".into()),
                },
            },
            FailureKind::JobsDatabase,
            Some(paths.jobs_path().into_std_path_buf()),
        ),
        (
            setup::Error::StartJobs {
                source: client::jobs::Error::OpenStore {
                    path: paths.jobs_path().into_std_path_buf(),
                    source: nyanpasu_jobs::Error::JournalDirectory(std::io::Error::from(
                        std::io::ErrorKind::PermissionDenied,
                    )),
                },
            },
            FailureKind::Permission,
            Some(paths.jobs_path().into_std_path_buf()),
        ),
        (
            setup::Error::BuildClient {
                source: SetupError::StartAppLogs {
                    source: nyanpasu_logging::LogError::Unavailable,
                },
            },
            FailureKind::Logging,
            Some(paths.app_logs_dir().into_std_path_buf()),
        ),
        (
            setup::Error::BuildCoreHost {
                source: local_host::Error::InspectConfig {
                    path: "application.yaml".into(),
                    source: io::ErrorKind::PermissionDenied.into(),
                },
            },
            FailureKind::Permission,
            Some("application.yaml".into()),
        ),
        (
            setup::Error::BuildCoreHost {
                source: local_host::Error::BuildManager {
                    source: nyanpasu_core_manager::Error::RuntimeDirectoryOwned(
                        "runtime/control".into(),
                    ),
                },
            },
            FailureKind::Busy,
            Some("runtime/control".into()),
        ),
    ];
    for (error, kind, path) in cases {
        let failure = StartupFailure::from_setup(&error, &paths);
        assert_eq!(failure.kind, kind, "{error:?}");
        assert_eq!(failure.path, path, "{error:?}");
        assert!(!failure.detail.is_empty());
    }
    assert_eq!(
        StartupFailure::from_rpc(&setup::UnifiedRpcError::RegisterState).kind,
        FailureKind::Internal
    );
    // A5 owns the pre-builder caller; A4 provides its presentation kind.
    let path = PathBuf::from("WebView2");
    let error = crate::bundle::Error::ValidateWebview { path: path.clone() };
    let failure = StartupFailure::from_error(FailureKind::WebView2, Some(path.clone()), &error);
    assert_eq!(failure.kind, FailureKind::WebView2);
    assert_eq!(failure.path, Some(path));
}

#[test]
fn configuration_parse_location_survives_the_typed_source_chain() {
    let parse = serde_yaml::from_str::<serde_yaml::Value>("test: [\n  broken").unwrap_err();
    let location = parse.location().unwrap();
    let source = client::application::Error::LoadState {
        path: paths().application_config_path(),
        source: nyanpasu_core::state::error::LoadError::DeserializeConfig(parse.into()),
    };
    let failure = StartupFailure::from_setup(
        &setup::Error::BuildClient {
            source: SetupError::LoadApplication { source },
        },
        &paths(),
    );
    assert_eq!(failure.kind, FailureKind::UserConfig);
    assert_eq!(
        failure.path,
        Some(paths().application_config_path().into_std_path_buf())
    );
    assert!(
        failure
            .detail
            .contains(&format!("line {}", location.line())),
        "{}",
        failure.detail
    );
    assert!(
        failure
            .detail
            .contains(&format!("column {}", location.column())),
        "{}",
        failure.detail
    );
}

#[test]
fn migration_transport_preserves_revisions_io_and_backup_availability() {
    let revisions = [
        runner::Error::NewerDocument {
            path: "application.yaml".into(),
            read: 99,
            supported: 1,
        },
        runner::Error::NewerAppliedRevision {
            path: "migration-state.yaml".into(),
            module: "application".into(),
            read: 99,
            supported: 1,
        },
        runner::Error::LoadState {
            source: store::Error::NewerSchema {
                path: "migration-state.yaml".into(),
                read: 99,
                supported: 1,
            },
        },
    ];
    for error in revisions {
        let source = Failure::from_error(&error, FailureStage::Migration, &paths(), true);
        let decoded = serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap();
        let report = Report::migration(
            &crate::utils::init::Error::ReadChildFailure { source: decoded },
            &paths(),
        );
        assert_eq!(report.failure.kind, FailureKind::NewerVersion);
        assert!(report.input().backups_dir.is_some());
    }
    for (io, expected) in [
        (io::ErrorKind::PermissionDenied, FailureKind::Permission),
        (io::ErrorKind::WouldBlock, FailureKind::Busy),
    ] {
        let error = runner::Error::ReadConfigDirectory {
            path: "config".into(),
            source: io.into(),
        };
        let source = Failure::from_error(&error, FailureStage::Backup, &paths(), false);
        let decoded = serde_json::from_slice(&serde_json::to_vec(&source).unwrap()).unwrap();
        let report = Report::migration(
            &crate::utils::init::Error::ReadChildFailure { source: decoded },
            &paths(),
        );
        assert_eq!(report.failure.kind, expected);
        assert!(report.input().backups_dir.is_none());
        assert_eq!(report.logs_dir, paths().app_data_dir());
    }
}

#[test]
fn io_wrappers_use_the_shared_classification() {
    let error: atomicwrites::Error<io::Error> =
        atomicwrites::Error::Internal(io::Error::from(io::ErrorKind::PermissionDenied));
    assert_eq!(
        StartupFailure::from_error(FailureKind::UserConfig, None, &error).kind,
        FailureKind::Permission
    );
    let error = redb::DatabaseError::DatabaseAlreadyOpen;
    assert_eq!(
        StartupFailure::from_error(FailureKind::Internal, None, &error).kind,
        FailureKind::Busy
    );
    #[cfg(windows)]
    for code in [32, 33] {
        assert_eq!(
            StartupFailure::from_error(
                FailureKind::UserConfig,
                None,
                &io::Error::from_raw_os_error(code)
            )
            .kind,
            FailureKind::Busy
        );
    }
}

#[test]
fn report_buttons_follow_kind_and_fallback_keeps_path_and_logs() {
    for (kind, containing, backups) in [
        (FailureKind::MigrationFailed, true, true),
        (FailureKind::UserConfig, true, true),
        (FailureKind::CoreRuntimeDirectory, true, false),
        (FailureKind::Internal, false, false),
    ] {
        let report = Report::new(
            StartupFailure {
                kind,
                path: Some("config/file.yaml".into()),
                detail: "full chain".into(),
            },
            &paths(),
        );
        let input = report.input();
        assert_eq!(input.containing_dir.is_some(), containing);
        assert_eq!(input.backups_dir.is_some(), backups);
        assert!(input.logs_dir.is_some());
        let mut fallback = None;
        show_with(
            &report,
            || {
                Err(Error::Write {
                    source: io::ErrorKind::BrokenPipe.into(),
                })
            },
            |text| fallback = Some(text),
        );
        let fallback = fallback.unwrap();
        assert!(fallback.contains("config/file.yaml"));
        assert!(fallback.contains(&report.logs_dir.display().to_string()));
        assert!(fallback.contains("full chain"));
    }
}

#[cfg(windows)]
#[test]
fn child_protocol_closes_stdin_before_waiting_and_rejects_nonzero_exit() {
    let report = Report::new(
        StartupFailure {
            kind: FailureKind::Internal,
            path: None,
            detail: "test".into(),
        },
        &paths(),
    );
    for code in [0, 7] {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", &format!("$inputJson = [Console]::In.ReadToEnd(); if (!$inputJson.Contains('internal')) {{ exit 9 }}; exit {code}")]);
        let result = run_child(command, &report);
        if code == 0 {
            assert!(result.is_ok());
        } else {
            assert!(matches!(result, Err(Error::ChildExited { .. })));
        }
    }
}

#[test]
fn one_shot_environment_messages_keep_actual_paths_and_localized_recovery() {
    for locale in ["en", "ja", "ko", "ru", "zh-cn", "zh-tw"] {
        for (kind, recovery) in [
            (FailureKind::Directories, None),
            (FailureKind::Logging, None),
            (FailureKind::WebView2, Some("web_view2.portable_recovery")),
            (FailureKind::WebView2, Some("web_view2.installed_recovery")),
        ] {
            let path = PathBuf::from("data/迁移/migration.log");
            let failure = StartupFailure {
                kind,
                path: Some(path.clone()),
                detail: "original cause".into(),
            };
            let message = one_shot_message(&failure, locale, recovery);
            assert!(message.contains(&path.display().to_string()));
            assert!(message.contains("original cause"));
            assert!(!message.contains("dialog.startup_failure."));
            let labels = crate::cmds::startup_failed::text(kind, locale);
            assert!(message.contains(&labels.description));
            if let Some(recovery) = recovery {
                let key = format!("dialog.startup_failure.{recovery}");
                assert!(message.contains(rust_i18n::t!(&key, locale = locale).as_ref()));
            } else {
                assert!(message.contains(&labels.recovery));
            }
        }
        let failure = StartupFailure {
            kind: FailureKind::Directories,
            path: None,
            detail: "platform has no directory".into(),
        };
        let message = one_shot_message(&failure, locale, None);
        assert!(!message.contains("dialog.startup_failure."));
        assert!(message.contains(
            rust_i18n::t!("dialog.startup_failure.unresolved_path", locale = locale).as_ref()
        ));
    }
}
