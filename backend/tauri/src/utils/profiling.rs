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
    #[cfg(feature = "dhat-heap")]
    heap: Option<dhat::Profiler>,
    #[cfg(feature = "trace-chrome")]
    trace: Option<tracing_chrome::FlushGuard>,
}

/// A new output file in `dir`, named after the local time it was opened at,
/// like the log files.
#[cfg(any(feature = "dhat-heap", feature = "trace-chrome"))]
fn output_file(dir: &Path, kind: &str) -> std::path::PathBuf {
    dir.join(format!(
        "{kind}-{}.json",
        chrono::Local::now().format("%Y-%m-%d_%H-%M-%S")
    ))
}

impl Profilers {
    /// Starts the heap profiler, as early as the app can: the Rust heap's
    /// allocations are recorded only from here on.
    #[cfg(feature = "dhat-heap")]
    pub fn start_heap(&mut self, paths: &nyanpasu_paths::PathResolver) {
        let dir = paths
            .logs_dir()
            .expect("the heap profile is written to the logs directory");
        self.heap = Some(
            dhat::Profiler::builder()
                .file_name(output_file(&dir, "dhat-heap"))
                .build(),
        );
    }

    /// Writes the heap profile out once the user quits: what is live then is
    /// what the app held while it ran, before the shutdown frees it. Resolving
    /// the stacks takes minutes on Windows, and every thread that allocates
    /// waits for it.
    pub fn finish_heap(&mut self) {
        #[cfg(feature = "dhat-heap")]
        {
            self.heap = None;
        }
    }

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
