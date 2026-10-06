//! Profilers that a profiling feature compiles in (see
//! docs/development/testing.md#profile-memory-and-startup). Each one finishes
//! its output file only when dropped, and the process ends inside `App::run`,
//! which never returns, so the event loop drops them.
use std::path::Path;

use tracing::Subscriber;
use tracing_subscriber::{Layer, registry::LookupSpan};

/// The profilers this build runs: none without a profiling feature.
#[derive(Default)]
pub struct Profilers {
    #[cfg(feature = "trace-chrome")]
    trace: Option<tracing_chrome::FlushGuard>,
}

/// A new output file in `dir`, named after the local time it was opened at,
/// like the log files.
#[cfg(feature = "trace-chrome")]
fn output_file(dir: &Path, kind: &str) -> std::path::PathBuf {
    dir.join(format!(
        "{kind}-{}.json",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    ))
}

impl Profilers {
    /// The layer that writes the app's spans to a Chrome trace in `log_dir`,
    /// which ui.perfetto.dev opens.
    #[cfg(feature = "trace-chrome")]
    pub fn trace_layer<S>(&mut self, log_dir: &Path) -> Option<impl Layer<S> + use<S>>
    where
        S: Subscriber + for<'span> LookupSpan<'span> + Send + Sync,
    {
        use tracing::Level;
        use tracing_subscriber::filter::Targets;

        let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
            .file(output_file(log_dir, "trace"))
            // Spans move between runtime workers, which the threaded style
            // cannot follow.
            .trace_style(tracing_chrome::TraceStyle::Async)
            .include_args(true)
            .build();
        self.trace = Some(guard);
        // Its own filter: the user's log level, unknown until the config
        // loads, does not thin the trace.
        Some(
            layer.with_filter(
                Targets::new()
                    .with_target("clash_nyanpasu", Level::INFO)
                    .with_target("nyanpasu", Level::INFO),
            ),
        )
    }

    #[cfg(not(feature = "trace-chrome"))]
    pub fn trace_layer<S>(&mut self, _log_dir: &Path) -> Option<impl Layer<S> + use<S>>
    where
        S: Subscriber + for<'span> LookupSpan<'span> + Send + Sync,
    {
        None::<tracing_subscriber::layer::Identity>
    }

    /// Writes out every profiler and stops it, on the event loop's last event.
    pub fn finish(&mut self) {
        *self = Self::default();
    }
}
