//! The executable's fatal boundary, installed before creating a runtime or starting tasks.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Owns the fatal notification receiver; the hook owns only its sending half.
pub struct PanicPolicy {
    fatal: mpsc::UnboundedReceiver<()>,
    active: Arc<AtomicBool>,
}

impl PanicPolicy {
    /// Installs the neutral process policy. Before supervision is active, panic exits immediately.
    pub fn install() -> Self {
        let (notify, fatal) = mpsc::unbounded_channel();
        let active = Arc::new(AtomicBool::new(false));
        let supervising = active.clone();
        nyanpasu_panics::setup_panic_hook(move |report| {
            // Also write synchronously: startup may not have installed a subscriber,
            // and a configured log filter must not hide the reason for fatal exit.
            eprintln!(
                "fatal panic: payload={:?} location={:?}",
                report.payload, report.location
            );
            if !supervising.load(Ordering::Acquire) || notify.send(()).is_err() {
                std::process::exit(1);
            }
            // Return so the original task unwinds with its original payload.
        });
        Self { fatal, active }
    }

    /// Runs the shell to completion, asking it to shut down after any task panic.
    ///
    /// A false result means the executable must exit unsuccessfully. Join panics
    /// are fatal notifications, never application/RPC errors.
    pub async fn supervise(
        mut self,
        stop: CancellationToken,
        shell: impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    ) -> bool {
        self.active.store(true, Ordering::Release);
        let mut task = tokio::spawn(shell);
        let mut panicked = false;
        let joined = tokio::select! {
            result = &mut task => result,
            _ = self.fatal.recv() => {
                panicked = true;
                stop.cancel();
                task.await
            }
        };
        panicked |= self.fatal.try_recv().is_ok();
        self.active.store(false, Ordering::Release);
        match joined {
            Ok(Ok(())) => !panicked,
            Ok(Err(error)) => {
                eprintln!("server failed: {error:#}");
                false
            }
            Err(error) => {
                eprintln!("fatal server task: {error}");
                false
            }
        }
    }
}
