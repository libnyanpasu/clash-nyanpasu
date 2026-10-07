use crate::{client::ui_effects::ports::LogRotation, utils::profiling::Profilers};
use anyhow::{Result, anyhow};
use flexi_logger::{
    Age, Cleanup, Criterion, FileSpec, Naming,
    writers::{ArcFileLogWriter, FileLogWriter, FileLogWriterHandle},
};
use nyanpasu_config::application::LoggingLevel;
use nyanpasu_paths::PathResolver;
use std::{
    fs,
    io::IsTerminal,
    path::PathBuf,
    sync::mpsc::{self, Sender},
    thread,
};
use tracing::error;
use tracing_appender::non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_log::log_tracer;
use tracing_subscriber::{EnvFilter, Layer as _, filter, fmt, layer::SubscriberExt, reload};

pub type ReloadSignal = (Option<LoggingLevel>, Option<LogRotation>);

/// Keeps the file writer alive. Fields drop in declaration order: the guard
/// flushes the non-blocking queue into the file writer before the handle shuts
/// that writer down.
struct FileAppenderGuard {
    _worker: WorkerGuard,
    _writer: FileLogWriterHandle,
}

/// Every file is named after the local time it was opened at, to the second,
/// so the names sort in write order: a new file per start, per local day and
/// per `max_file_size`. A day-only name would make a restart reopen the day's
/// first file, which then sorts as the oldest and is the first to be cleaned.
fn file_log_writer(
    log_dir: PathBuf,
    rotation: LogRotation,
) -> Result<(ArcFileLogWriter, FileLogWriterHandle)> {
    Ok(FileLogWriter::builder(
        FileSpec::default()
            .directory(log_dir)
            .basename("clash-nyanpasu")
            .suffix("log"),
    )
    .append()
    .rotate(
        Criterion::AgeOrSize(Age::Day, rotation.max_file_size.saturating_mul(1024 * 1024)),
        Naming::TimestampsCustomFormat {
            current_infix: None,
            format: "%Y-%m-%d_%H-%M-%S",
        },
        Cleanup::KeepLogFiles(rotation.max_files),
    )
    .try_build_with_handle()?)
}

fn get_file_appender(
    logs_dir: PathBuf,
    rotation: LogRotation,
) -> Result<(NonBlocking, FileAppenderGuard)> {
    let (writer, handle) = file_log_writer(logs_dir, rotation)?;
    let (appender, worker) = NonBlockingBuilder::default()
        .buffered_lines_limit(4096)
        .finish(writer);
    Ok((
        appender,
        FileAppenderGuard {
            _worker: worker,
            _writer: handle,
        },
    ))
}

/// Sets the app's own crates to `level` and everything else to warn.
///
/// The directives spell the level through [`filter::LevelFilter`], which
/// `tracing` always parses back. The config spells `Silent` as `silent`, which
/// `tracing` rejects, and a directive that does not parse panics the reload
/// thread after the setting is already saved.
fn app_filter(level: LoggingLevel) -> EnvFilter {
    let level = filter::LevelFilter::from(level);
    EnvFilter::builder()
        .with_default_directive(
            std::convert::Into::<filter::LevelFilter>::into(LoggingLevel::Warn).into(),
        )
        .from_env_lossy()
        .add_directive(format!("nyanpasu={level}").parse().unwrap())
        .add_directive(format!("clash_nyanpasu={level}").parse().unwrap())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_logging::{FsLogFiles, LogFiles};
    use std::io::Write;

    /// Every level the settings offer, `silent` included, sets both of the
    /// app's own crates to the level `tracing` knows it by.
    #[test]
    fn app_filter_sets_the_app_crates_to_every_configured_level() {
        for (level, spelled) in [
            (LoggingLevel::Silent, "off"),
            (LoggingLevel::Trace, "trace"),
            (LoggingLevel::Debug, "debug"),
            (LoggingLevel::Info, "info"),
            (LoggingLevel::Warn, "warn"),
            (LoggingLevel::Error, "error"),
        ] {
            let filter = app_filter(level).to_string();
            let directives: Vec<_> = filter.split(',').collect();
            for target in ["nyanpasu", "clash_nyanpasu"] {
                let expected = format!("{target}={spelled}");
                assert!(directives.contains(&expected.as_str()), "{filter}");
            }
        }
    }

    /// The files the writer rotates into are the ones the log viewer lists,
    /// newest first, and no more of them than `max_files` are kept.
    #[test]
    fn rotated_files_are_listed_by_the_log_viewer_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let (mut writer, handle) = file_log_writer(
            dir.path().into(),
            LogRotation {
                max_files: 3,
                max_file_size: 1,
            },
        )
        .unwrap();
        let line = format!("{{\"pad\":\"{}\"}}\n", "x".repeat(1024));
        for _ in 0..(5 * 1024) {
            writer.write_all(line.as_bytes()).unwrap();
        }
        writer.write_all(b"{\"last\":true}\n").unwrap();
        writer.flush().unwrap();
        drop(handle);

        let catalog = FsLogFiles::new(dir.path().into(), "clash-nyanpasu".into())
            .catalog()
            .unwrap();
        let names: Vec<_> = catalog.iter().map(|file| file.name.as_str()).collect();
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(
            names
                .iter()
                .all(|name| name.starts_with("clash-nyanpasu_") && name.ends_with(".log")),
            "{names:?}"
        );
        let newest = fs::read_to_string(dir.path().join(names[0])).unwrap();
        assert!(newest.ends_with("{\"last\":true}\n"));
        for name in &names[1..] {
            let len = fs::metadata(dir.path().join(name)).unwrap().len();
            assert!(len <= 1024 * 1024 + line.len() as u64, "{name}: {len}");
        }
    }
}
