use crate::utils::profiling::Profilers;
use anyhow::{Result, anyhow};
use nyanpasu_config::application::LoggingLevel;
use nyanpasu_core::logs::logging::{LogRotation, ReloadSignal, app_filter, get_file_appender};
use nyanpasu_paths::PathResolver;
use std::{
    fs,
    io::IsTerminal,
    sync::mpsc::{self, Sender},
    thread,
};
use tracing::error;
use tracing_log::log_tracer;
use tracing_subscriber::{Layer as _, fmt, layer::SubscriberExt, reload};

/// initial instance global logger, returning the channel that reloads it; the
/// trace profiler, when compiled in, joins the subscriber here
pub fn init(
    profilers: &mut Profilers,
    paths: &PathResolver,
) -> Result<(Sender<ReloadSignal>, nyanpasu_jobs::LogCapture)> {
    let jobs = crate::client::jobs::capture();
    let log_dir = paths.app_logs_dir();
    if !log_dir.exists() {
        let _ = fs::create_dir_all(&log_dir);
    }
    // This is intended to capture config loading errors
    let (log_level, log_rotation) = (
        LoggingLevel::Debug,
        LogRotation {
            max_files: 7,
            max_file_size: 10,
        },
    );
    let (filter, filter_handle) = reload::Layer::new(app_filter(log_level));

    // register the logger
    let (appender, _guard) = get_file_appender(log_dir.clone().into_std_path_buf(), log_rotation)?;
    let (file_layer, file_handle) = reload::Layer::new(
        fmt::layer()
            .json()
            .with_writer(appender)
            .with_current_span(true)
            .with_line_number(true)
            .with_file(true),
    );

    // spawn a thread to handle the reload signal
    let (sender, receiver) = mpsc::channel::<ReloadSignal>();
    let paths = paths.clone();
    thread::spawn(move || {
        let mut _guard = _guard; // just hold here to keep the file open
        let mut current_rotation = log_rotation;
        while let Ok(signal) = receiver.recv() {
            if let Some(level) = signal.0 {
                filter_handle.reload(app_filter(level)).unwrap(); // panic if error
            }

            // Rebuilding the writer opens a new file, so an unchanged rotation
            // (such as the startup effect replaying the defaults) keeps the
            // current one.
            if let Some(rotation) = signal.1.filter(|r| *r != current_rotation) {
                let (appender, guard) =
                    match get_file_appender(paths.app_logs_dir().into_std_path_buf(), rotation) {
                        Ok(x) => x,
                        Err(e) => {
                            error!("failed to create file appender: {}", e);
                            continue;
                        }
                    };
                if let Err(e) = file_handle.modify(|layer| *layer.writer_mut() = appender) {
                    error!("failed to modify file appender: {}", e);
                    continue;
                }
                _guard = guard;
                current_rotation = rotation;
            }
        }
        // Every sender is gone, so no reload can come, but the guard still has
        // to outlive every log line the process writes.
        loop {
            thread::park();
        }
    });

    // if debug build, log to stdout and stderr with all levels
    #[cfg(debug_assertions)]
    let terminal_layer = fmt::Layer::new()
        .with_ansi(std::io::stdout().is_terminal())
        .compact()
        .with_target(false)
        .with_file(true)
        .with_line_number(true)
        .with_writer(std::io::stdout);

    #[cfg(debug_assertions)]
    let file_layer = file_layer.and_then(terminal_layer);

    let subscriber = tracing_subscriber::registry()
        .with(file_layer.with_filter(filter))
        .with(jobs.layer())
        .with(profilers.trace_layer(log_dir.as_std_path()));

    log_tracer::LogTracer::init()?;
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|x| anyhow!("setup logging error: {}", x))?;
    Ok((sender, jobs))
}
