//! Command interception for real OS adapters; production uses OS process calls.

use std::{io, process::ExitStatus};

/// Intercepts process execution without replacing adapter policy or command construction.
#[async_trait::async_trait]
pub trait CommandRunner: Send + Sync + 'static {
    /// Runs an elevated command after the adapter has obtained consent.
    fn runas(&self, command: &mut runas::Command) -> io::Result<ExitStatus>;

    /// Collects the ordinary service status command's output.
    async fn output(
        &self,
        command: &mut tokio::process::Command,
    ) -> io::Result<std::process::Output>;

    /// Runs the authorization helper (osascript on macOS, pkexec on Linux)
    /// after consent.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn command(&self, command: &mut std::process::Command) -> io::Result<ExitStatus>;
}

pub(crate) struct OsCommandRunner;

#[async_trait::async_trait]
impl CommandRunner for OsCommandRunner {
    fn runas(&self, command: &mut runas::Command) -> io::Result<ExitStatus> {
        command.status()
    }

    async fn output(
        &self,
        command: &mut tokio::process::Command,
    ) -> io::Result<std::process::Output> {
        command.output().await
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn command(&self, command: &mut std::process::Command) -> io::Result<ExitStatus> {
        command.status()
    }
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct CountingRunner(pub std::sync::atomic::AtomicUsize);

#[cfg(test)]
#[async_trait::async_trait]
impl CommandRunner for CountingRunner {
    fn runas(&self, _: &mut runas::Command) -> io::Result<ExitStatus> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(io::Error::other("test launch intercepted"))
    }

    async fn output(&self, _: &mut tokio::process::Command) -> io::Result<std::process::Output> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(io::Error::other("test launch intercepted"))
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    fn command(&self, _: &mut std::process::Command) -> io::Result<ExitStatus> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(io::Error::other("test launch intercepted"))
    }
}
