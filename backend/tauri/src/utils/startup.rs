//! Bootstrap-owned Chrome trace output; stage clocks and serialization belong to tracing-chrome.
use anyhow::{Context, Result};
use parking_lot::Mutex;
use std::{collections::HashSet, fs, path::Path, time::SystemTime};
use tracing_chrome::{ChromeLayerBuilder, FlushGuard, TraceStyle};
use tracing_subscriber::{Layer as _, filter::filter_fn};

pub(crate) const TARGET: &str = "clash_nyanpasu::startup";

// Async export groups descendants by root ID. Separate roots keep concurrent
// stages on independent tracks rather than producing overlapping nested events.
macro_rules! span {
    ($name:literal) => {
        tracing::info_span!(target: $crate::utils::startup::TARGET, parent: None, $name)
    };
}
pub(crate) use span;

pub(crate) fn layer(
    directory: Option<&Path>,
) -> Result<(Option<super::init::logging::LogLayer>, StartupTrace)> {
    let Some(directory) = directory else {
        return Ok((None, StartupTrace::new(None)));
    };
    anyhow::ensure!(
        directory.is_absolute(),
        "NYANPASU_TRACE_DIR must be absolute"
    );
    fs::create_dir_all(directory).context("failed to create startup trace directory")?;
    let stamp = SystemTime::UNIX_EPOCH.elapsed()?.as_nanos();
    let path = directory.join(format!("startup-{}-{stamp}.json", std::process::id()));
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("failed to create startup trace {}", path.display()))?;
    let (layer, guard) = ChromeLayerBuilder::new()
        .writer(file)
        .trace_style(TraceStyle::Async)
        .include_args(true)
        .include_locations(false)
        .build();
    Ok((
        Some(
            layer
                .with_filter(filter_fn(|meta| meta.target() == TARGET))
                .boxed(),
        ),
        StartupTrace::new(Some(guard)),
    ))
}

/// A logging adapter owned by bootstrap and shared with Tauri lifecycle callbacks.
pub struct StartupTrace {
    // FlushGuard is Send but not Sync. This lock only serializes output shutdown;
    // it does not expose or share any actor's state.
    guard: Mutex<Option<FlushGuard>>,
    milestones: Mutex<HashSet<&'static str>>,
}

impl StartupTrace {
    fn new(guard: Option<FlushGuard>) -> Self {
        Self {
            guard: Mutex::new(guard),
            milestones: Mutex::new(HashSet::new()),
        }
    }

    pub fn entry(&self) {
        tracing::info!(
            name: "startup_entry",
            target: TARGET,
            parent: None,
            pid = std::process::id(),
            debug_assertions = cfg!(debug_assertions),
            os = std::env::consts::OS,
            arch = std::env::consts::ARCH,
        );
    }

    /// Later window reopenings and tray rebuilds are not startup milestones.
    pub fn milestone(&self, name: &'static str) {
        if self.milestones.lock().insert(name) {
            tracing::info!(
                name: "startup.milestone",
                target: TARGET,
                parent: None,
                milestone = %name,
            );
        }
    }

    /// Complete the file, preserving any still-open span as incomplete.
    /// Tauri and early process::exit paths must call this explicitly.
    pub fn finish(&self) {
        if let Some(guard) = self.guard.lock().take() {
            tracing::info!(name: "capture_end", target: TARGET, parent: None, finished = true);
            drop(guard);
        }
    }
}

impl Drop for StartupTrace {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(test)]
mod tests;
