//! Desktop policy permitting the existing OS authorization dialog.

use nyanpasu_contracts::elevation::{ElevationConsent, ElevationConsentError, ElevationOperation};

pub(crate) struct InteractiveElevationConsent;

impl ElevationConsent for InteractiveElevationConsent {
    fn authorize(&self, _operation: ElevationOperation) -> Result<(), ElevationConsentError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io,
        process::{ExitStatus, Output},
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use nyanpasu_contracts::{
        core_install::{BinaryInstaller as _, CoreBinaryInstallRequest, InstallCoreBinaryError},
        elevation::{ElevationConsent as _, ElevationOperation},
        service::ServiceCommand,
    };
    use nyanpasu_os::{core_install::FsBinaryInstaller, elevation::runner::CommandRunner};

    use super::InteractiveElevationConsent;

    #[derive(Default)]
    struct Runner(AtomicUsize);

    #[async_trait::async_trait]
    impl CommandRunner for Runner {
        fn runas(&self, _: &mut runas::Command) -> io::Result<ExitStatus> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(io::Error::other("test launch intercepted"))
        }

        async fn output(&self, _: &mut tokio::process::Command) -> io::Result<Output> {
            panic!("a binary copy must not probe service status")
        }

        #[cfg(any(target_os = "linux", target_os = "macos"))]
        fn command(&self, _: &mut std::process::Command) -> io::Result<ExitStatus> {
            panic!("a binary copy uses runas")
        }
    }

    #[tokio::test]
    async fn desktop_conditional_elevation_uses_existing_consent_policy() {
        let staging = Arc::new(tempfile::tempdir().unwrap());
        let source = staging.path().join("staged-core");
        let destination = staging.path().join("installed-core");
        std::fs::write(&source, "core binary").unwrap();
        let request = CoreBinaryInstallRequest {
            target: nyanpasu_config::application::ClashCore::Mihomo,
            source,
            destination,
            staging,
            companion: None,
        };
        let runner = Arc::new(Runner::default());
        let installer = FsBinaryInstaller::new_with_runner(
            Arc::new(InteractiveElevationConsent),
            runner.clone(),
        );
        installer.install(&request).await.unwrap();
        assert_eq!(runner.0.load(Ordering::SeqCst), 0);
        std::fs::remove_file(&request.destination).unwrap();
        std::fs::create_dir(&request.destination).unwrap();
        let error = installer.install(&request).await.unwrap_err();
        assert!(
            matches!(error, InstallCoreBinaryError::StartElevatedCopy { .. }),
            "{error:?}"
        );
        assert_eq!(runner.0.load(Ordering::SeqCst), 1);
        assert_eq!(
            std::fs::read_to_string(&request.source).unwrap(),
            "core binary"
        );
    }

    #[test]
    fn interactive_policy_adds_no_confirmation() {
        for operation in [
            ElevationOperation::DnsFlush,
            ElevationOperation::CoreBinaryCopy,
            ElevationOperation::Service {
                command: ServiceCommand::Install,
            },
            ElevationOperation::Service {
                command: ServiceCommand::Update,
            },
            ElevationOperation::Service {
                command: ServiceCommand::Uninstall,
            },
            ElevationOperation::Service {
                command: ServiceCommand::Start,
            },
            ElevationOperation::Service {
                command: ServiceCommand::Stop,
            },
            ElevationOperation::Service {
                command: ServiceCommand::Restart,
            },
        ] {
            InteractiveElevationConsent.authorize(operation).unwrap();
        }
    }
}
