pub(crate) mod adapters;

use anyhow::{Result, bail};
use std::path::PathBuf;

enum StartupEnvironment {
    Native,
    Windows {
        updater_target: String,
        runtime_dir: PathBuf,
        runtime_present: bool,
        runtime_available: bool,
    },
}

#[cfg_attr(test, mockall::automock)]
trait StartupSource {
    fn snapshot(&self) -> Result<StartupEnvironment>;
}

#[derive(Debug, PartialEq, Eq)]
enum StartupSelection {
    Configured,
    FixedWebview {
        runtime_dir: PathBuf,
        updater_target: String,
    },
}

struct AppStartup<'a> {
    source: &'a dyn StartupSource,
}

impl<'a> AppStartup<'a> {
    fn new(source: &'a dyn StartupSource) -> Self {
        Self { source }
    }

    fn prepare(&self) -> Result<StartupSelection> {
        select(self.source.snapshot()?)
    }
}

fn select(environment: StartupEnvironment) -> Result<StartupSelection> {
    let StartupEnvironment::Windows {
        updater_target,
        runtime_dir,
        runtime_present,
        runtime_available,
    } = environment
    else {
        return Ok(StartupSelection::Configured);
    };
    if !runtime_present {
        return Ok(StartupSelection::Configured);
    }
    if !runtime_available {
        bail!(
            "fixed WebView2 runtime is incomplete in {}",
            runtime_dir.display()
        );
    }
    Ok(StartupSelection::FixedWebview {
        runtime_dir,
        updater_target: format!("{updater_target}-fixed-webview"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepare(environment: StartupEnvironment) -> Result<StartupSelection> {
        let mut source = MockStartupSource::new();
        source
            .expect_snapshot()
            .once()
            .return_once(|| Ok(environment));
        AppStartup::new(&source).prepare()
    }

    fn windows(arch: &str, present: bool, available: bool) -> StartupEnvironment {
        StartupEnvironment::Windows {
            updater_target: format!("windows-{arch}"),
            runtime_dir: PathBuf::from("application/WebView2"),
            runtime_present: present,
            runtime_available: available,
        }
    }

    #[test]
    fn native_startup_keeps_the_compiled_configuration() {
        assert_eq!(
            prepare(StartupEnvironment::Native).unwrap(),
            StartupSelection::Configured
        );
    }

    #[test]
    fn windows_without_bundled_runtime_uses_the_standard_target() {
        for arch in ["x86_64", "aarch64"] {
            assert_eq!(
                prepare(windows(arch, false, false)).unwrap(),
                StartupSelection::Configured
            );
        }
    }

    #[test]
    fn bundled_runtime_selects_the_fixed_target_for_each_architecture() {
        for arch in ["x86_64", "aarch64"] {
            assert_eq!(
                prepare(windows(arch, true, true)).unwrap(),
                StartupSelection::FixedWebview {
                    runtime_dir: PathBuf::from("application/WebView2"),
                    updater_target: format!("windows-{arch}-fixed-webview"),
                }
            );
        }
    }

    #[test]
    fn incomplete_runtime_is_an_error() {
        let error = prepare(windows("x86_64", true, false)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("fixed WebView2 runtime is incomplete")
        );
    }

    #[test]
    fn source_failure_is_returned_to_the_caller() {
        let mut source = MockStartupSource::new();
        source
            .expect_snapshot()
            .once()
            .return_once(|| bail!("source unavailable"));
        assert_eq!(
            AppStartup::new(&source).prepare().unwrap_err().to_string(),
            "source unavailable"
        );
    }
}
