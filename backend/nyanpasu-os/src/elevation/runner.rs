//! Private launch seam; production uses the original command status calls.

use std::{io, process::ExitStatus};

pub(crate) trait CommandRunner: Send + Sync + 'static {
    fn runas(&self, command: &mut runas::Command) -> io::Result<ExitStatus>;

    #[cfg(target_os = "macos")]
    fn command(&self, command: &mut std::process::Command) -> io::Result<ExitStatus>;
}

pub(crate) struct OsCommandRunner;

impl CommandRunner for OsCommandRunner {
    fn runas(&self, command: &mut runas::Command) -> io::Result<ExitStatus> {
        command.status()
    }

    #[cfg(target_os = "macos")]
    fn command(&self, command: &mut std::process::Command) -> io::Result<ExitStatus> {
        command.status()
    }
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct CountingRunner(pub std::sync::atomic::AtomicUsize);

#[cfg(test)]
impl CommandRunner for CountingRunner {
    fn runas(&self, _: &mut runas::Command) -> io::Result<ExitStatus> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(io::Error::other("test launch intercepted"))
    }

    #[cfg(target_os = "macos")]
    fn command(&self, _: &mut std::process::Command) -> io::Result<ExitStatus> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(io::Error::other("test launch intercepted"))
    }
}
