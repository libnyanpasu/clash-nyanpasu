use std::sync::Arc;

use camino::Utf8PathBuf;
use nyanpasu_config::application::ClashCore;
use nyanpasu_core_manager::{
    ControlOptions, CoreControl, CoreKind, CoreManager, CoreSpec, LocalIpcPolicy, ManagerOptions,
    native_store::{FsNativeStore, StoreOwner, legacy_kind},
};
use serde::Serialize;
use snafu::{ResultExt, Snafu};

use nyanpasu_paths::PathResolver;

use crate::client::session_end::SessionEnding;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("failed to inspect {}", path.display()))]
    InspectConfig {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to read core kind from {}", path.display()))]
    ReadCoreKind {
        path: std::path::PathBuf,
        source: nyanpasu_core_manager::native_store::StoreError,
    },
    #[snafu(display("failed to build core manager"))]
    BuildManager {
        source: nyanpasu_core_manager::Error,
    },
}

pub async fn build(
    paths: &PathResolver,
    shutdown_helper: &std::path::Path,
    session_ending: SessionEnding,
) -> Result<CoreControl, Error> {
    let runtime_root = paths.app_config_dir().join("runtime");
    let options = ManagerOptions {
        runtime_dir: Some(runtime_root.join("control")),
        local_ipc_policy: LocalIpcPolicy::Disable,
        windows_shutdown_helper: Some(shutdown_helper.to_owned()),
        ..ManagerOptions::default()
    };
    let working_dir = paths.app_data_dir().to_owned();
    let config_path = paths.application_config_path();
    let config_path = if config_path.try_exists().context(InspectConfigSnafu {
        path: config_path.as_std_path(),
    })? {
        config_path
    } else {
        paths.nyanpasu_config_path()
    };
    let native_store = Arc::new(FsNativeStore::new(
        working_dir.clone(),
        StoreOwner::current(),
        legacy_kind(config_path.as_std_path()).context(ReadCoreKindSnafu {
            path: config_path.as_std_path(),
        })?,
    ));
    let manager = CoreManager::builder(options)
        .native_store(native_store)
        .process_activity(session_ending.process_activity());

    #[cfg(target_os = "macos")]
    let manager = manager.dns_controller(Arc::new(
        nyanpasu_core_manager::dns::macos::MacosDnsController::new(
            "State:/Network/Service/nyanpasu-dns/DNS".into(),
        ),
    ));

    let manager = manager.build().await.context(BuildManagerSnafu)?;
    let source_dir = runtime_root.join("staging");

    Ok(CoreControl::spawn(
        manager,
        ControlOptions::new(source_dir, working_dir),
    ))
}

/// A failure of locating the binary a core is started from.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CoreSpecError {
    #[snafu(display("could not find the {core} core binary"))]
    FindCoreBinary {
        #[specta(type = String)]
        core: ClashCore,
        #[serde(skip)]
        source: std::io::Error,
    },
}

pub fn core_spec(
    core: &ClashCore,
    paths: &nyanpasu_paths::PathResolver,
) -> Result<CoreSpec, CoreSpecError> {
    core_spec_with(core, |core| {
        paths.find_binary_path(core.get_executable_name())
    })
}

fn core_spec_with(
    core: &ClashCore,
    find_binary: impl FnOnce(&nyanpasu_utils::core::CoreType) -> std::io::Result<Utf8PathBuf>,
) -> Result<CoreSpec, CoreSpecError> {
    let core_type = core.into();
    let kind = match core {
        ClashCore::ClashPremium => CoreKind::ClashPremium,
        ClashCore::ClashRs | ClashCore::ClashRsAlpha => CoreKind::ClashRust,
        ClashCore::Mihomo | ClashCore::MihomoAlpha => CoreKind::Mihomo,
        ClashCore::Meow | ClashCore::MeowAlpha => CoreKind::Meow,
    };
    let binary_path = find_binary(&core_type).context(FindCoreBinarySnafu { core: *core })?;

    Ok(CoreSpec {
        kind,
        binary_path,
        version: None,
        features: vec![],
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use nyanpasu_core_manager::{
        ConfigInput, CoreCommand, CoreCommandEnvelope, CoreState, InstanceOptions, OperationId,
        ReconcileRequest,
    };
    use nyanpasu_utils::process::{Backoff, BackoffRange, RestartPolicy, read_epoch_pid_file};

    use super::*;

    #[tokio::test]
    async fn revoking_session_restores_normal_supervision() {
        let root = tempfile::TempDir::new().unwrap();
        let paths = crate::utils::test_support::test_paths(
            root.path().join("config"),
            root.path().join("data"),
        );
        let session = crate::client::session_end::SessionEndSource::new();
        std::fs::create_dir_all(paths.app_data_dir()).unwrap();
        let control = build(&paths, &root.path().join("helper.exe"), session.subscribe())
            .await
            .unwrap();
        let mut starts = control.subscribe_logs();
        let executable = std::env::current_exe()
            .unwrap()
            .ancestors()
            .skip(1)
            .map(|directory| directory.join(format!("fake-core{}", std::env::consts::EXE_SUFFIX)))
            .find(|path| path.is_file())
            .expect(
                "build the fake with cargo build --manifest-path backend/Cargo.toml -p fake-core",
            );
        session.set_pending(true);
        let operation = control
            .submit(CoreCommandEnvelope {
                operation_id: OperationId::generate(),
                command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                    core: CoreSpec {
                        kind: CoreKind::Mihomo,
                        binary_path: Utf8PathBuf::from_path_buf(executable).unwrap(),
                        version: Some("1.18.9".into()),
                        features: vec![],
                    },
                    config: ConfigInput::inline(b"external-controller: 127.0.0.1:0\n".to_vec()),
                    options: InstanceOptions {
                        restart_policy: RestartPolicy::OnFailure { max_restarts: 2 },
                        backoff: Backoff::exponential(BackoffRange {
                            initial: Duration::ZERO,
                            max: Duration::ZERO,
                        }),
                        ..InstanceOptions::default()
                    },
                    expected_applied: None,
                })),
            })
            .unwrap();
        let first_start = tokio::time::timeout(Duration::from_secs(30), starts.recv())
            .await
            .expect("the first failed start must publish its termination record")
            .unwrap();
        let first_pid: u32 = first_start
            .message
            .rsplit_once("; pid=")
            .unwrap()
            .1
            .parse()
            .unwrap();
        let active = read_epoch_pid_file(paths.app_data_dir().join("native-store/active"))
            .await
            .expect("pending session end must prevent preparing another attempt")
            .expect("the first attempt must remain recorded while paused");
        assert_eq!(
            active.pid, first_pid,
            "the local supervisor started another attempt while session end was pending"
        );
        assert!(matches!(
            starts.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));

        session.set_pending(false);
        let error = tokio::time::timeout(Duration::from_secs(30), operation.wait())
            .await
            .expect("revocation must resume the deferred retries")
            .expect_err("the fake core exits without its required start barrier");
        assert!(error.message.contains("FAKE_CORE_READY_ADDR"), "{error}");
        let mut resumed_starts = 0;
        while let Ok(start) = starts.try_recv() {
            assert!(start.message.contains("; pid="), "{start:?}");
            resumed_starts += 1;
        }
        assert_eq!(
            resumed_starts, 2,
            "revocation must resume exactly two retries"
        );
        control.shutdown().await.unwrap();
        assert!(matches!(control.status().state, CoreState::Stopped { .. }));
    }

    #[tokio::test]
    async fn the_local_host_spawns_under_a_temp_root() {
        let root = tempfile::TempDir::new().unwrap();
        let paths = crate::utils::test_support::test_paths(
            root.path().join("config"),
            root.path().join("data"),
        );

        let control = build(
            &paths,
            &root.path().join("helper.exe"),
            crate::client::session_end::SessionEndSource::new().subscribe(),
        )
        .await
        .unwrap();

        let _ = control.status();
        assert!(!control.executor_is_closed());
    }

    #[tokio::test]
    async fn the_local_host_reads_the_typed_config_before_the_legacy_file() {
        let root = tempfile::TempDir::new().unwrap();
        let paths = crate::utils::test_support::test_paths(root.path(), root.path().join("data"));
        std::fs::write(paths.application_config_path(), "core: mihomo\n").unwrap();
        std::fs::write(paths.nyanpasu_config_path(), "invalid: [").unwrap();

        let control = build(
            &paths,
            &root.path().join("helper.exe"),
            crate::client::session_end::SessionEndSource::new().subscribe(),
        )
        .await
        .unwrap();
        assert!(!control.executor_is_closed());

        std::fs::remove_file(paths.application_config_path()).unwrap();
        assert!(
            build(
                &paths,
                &root.path().join("helper.exe"),
                crate::client::session_end::SessionEndSource::new().subscribe(),
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn core_spec_maps_every_clash_core_variant() {
        let root = tempfile::TempDir::new().unwrap();
        let variants = [
            (ClashCore::ClashPremium, CoreKind::ClashPremium),
            (ClashCore::ClashRs, CoreKind::ClashRust),
            (ClashCore::Mihomo, CoreKind::Mihomo),
            (ClashCore::MihomoAlpha, CoreKind::Mihomo),
            (ClashCore::ClashRsAlpha, CoreKind::ClashRust),
            (ClashCore::Meow, CoreKind::Meow),
            (ClashCore::MeowAlpha, CoreKind::Meow),
        ];

        for (core, expected_kind) in variants {
            let spec = core_spec_with(&core, |core_type| {
                Ok(
                    Utf8PathBuf::from_path_buf(root.path().join(core_type.get_executable_name()))
                        .unwrap(),
                )
            })
            .unwrap();
            assert_eq!(spec.kind, expected_kind);
            assert!(!spec.binary_path.as_str().is_empty());
        }
    }
}
