use crate::{client::ui_effects::ports::LogRotation, utils::profiling::Profilers};
use flexi_logger::{
    Age, Cleanup, Criterion, FileSpec, Naming,
    writers::{ArcFileLogWriter, FileLogWriter, FileLogWriterHandle},
};
use nyanpasu_config::application::LoggingLevel;
use nyanpasu_paths::PathResolver;
use snafu::ResultExt as _;
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

#[derive(Debug, snafu::Snafu)]
pub(crate) enum Error {
    #[snafu(display("failed to create logging directory {}", path.display()))]
    CreateDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[snafu(display("failed to start logging thread"))]
    StartReloadThread { source: std::io::Error },
    #[snafu(display("failed to create log writer in {}", path.display()))]
    CreateWriter {
        path: PathBuf,
        source: flexi_logger::FlexiLoggerError,
    },
    #[snafu(display("failed to install log tracer"))]
    InstallLogTracer { source: log::SetLoggerError },
    #[snafu(display("failed to install tracing subscriber"))]
    InstallSubscriber {
        source: tracing::subscriber::SetGlobalDefaultError,
    },
}

#[derive(Debug)]
pub enum ReloadSignal {
    Reload(Option<LoggingLevel>, Option<LogRotation>),
    StartupFailure { detail: String, done: Sender<()> },
    Finish(Sender<()>),
}

/// Acknowledges startup failure records and pre-Tauri logging cleanup.
pub(crate) struct LoggingShutdown(pub Sender<ReloadSignal>);

impl LoggingShutdown {
    pub(crate) fn record_startup_failure(&self, detail: String) {
        let (done, completed) = mpsc::channel();
        if self
            .0
            .send(ReloadSignal::StartupFailure { detail, done })
            .is_ok()
        {
            let _ = completed.recv();
        }
    }

    /// Pre-Tauri failures have no event loop to keep responsive while flushing.
    pub(crate) fn finish_blocking(&self) {
        let (done, completed) = mpsc::channel();
        if self.0.send(ReloadSignal::Finish(done)).is_ok() {
            let _ = completed.recv();
        }
    }
}

/// Keeps the file writer alive. Fields drop in declaration order: the guard
/// flushes the non-blocking queue into the file writer before the handle shuts
/// that writer down.
struct FileAppenderGuard {
    _worker: WorkerGuard,
    direct: ArcFileLogWriter,
    _writer: FileLogWriterHandle,
}

impl FileAppenderGuard {
    /// WorkerGuard's Drop has a bounded flush. Record this fatal startup event
    /// synchronously on the reload thread so that bound cannot lose its cause.
    fn record_startup_failure(&mut self, detail: &str) -> std::io::Result<()> {
        use std::io::Write as _;
        let writer = self.direct.clone();
        let subscriber = fmt::Subscriber::builder()
            .json()
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(error = detail, "application startup failed");
        });
        self.direct.flush()
    }
}

/// Every file is named after the local time it was opened at, to the second,
/// so the names sort in write order: a new file per start, per local day and
/// per `max_file_size`. A day-only name would make a restart reopen the day's
/// first file, which then sorts as the oldest and is the first to be cleaned.
fn file_log_writer(
    log_dir: PathBuf,
    rotation: LogRotation,
) -> Result<(ArcFileLogWriter, FileLogWriterHandle), Error> {
    FileLogWriter::builder(
        FileSpec::default()
            .directory(&log_dir)
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
    .try_build_with_handle()
    .context(CreateWriterSnafu { path: log_dir })
}

fn get_file_appender(
    logs_dir: PathBuf,
    rotation: LogRotation,
) -> Result<(NonBlocking, FileAppenderGuard), Error> {
    let (writer, handle) = file_log_writer(logs_dir, rotation)?;
    let (appender, worker) = NonBlockingBuilder::default()
        .buffered_lines_limit(4096)
        .finish(writer.clone());
    Ok((
        appender,
        FileAppenderGuard {
            _worker: worker,
            direct: writer,
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
        .add_directive(
            format!("nyanpasu={level}")
                .parse()
                .expect("the static crate name and LevelFilter form a valid directive"),
        )
        .add_directive(
            format!("clash_nyanpasu={level}")
                .parse()
                .expect("the static crate name and LevelFilter form a valid directive"),
        )
}

/// initial instance global logger, returning the channel that reloads it; the
/// trace profiler, when compiled in, joins the subscriber here
pub fn init(
    profilers: &mut Profilers,
    paths: &PathResolver,
) -> Result<(Sender<ReloadSignal>, nyanpasu_jobs::LogCapture), Error> {
    let jobs = crate::client::jobs::capture();
    let log_dir = paths.app_logs_dir();
    fs::create_dir_all(&log_dir).context(CreateDirectorySnafu {
        path: log_dir.as_std_path(),
    })?;
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
    thread::Builder::new()
        .name("logging-reload".into())
        .spawn(move || {
            let mut _guard = _guard; // just hold here to keep the file open
            let mut current_rotation = log_rotation;
            while let Ok(signal) = receiver.recv() {
                let (level, rotation) = match signal {
                    ReloadSignal::Reload(level, rotation) => (level, rotation),
                    ReloadSignal::StartupFailure { detail, done } => {
                        if let Err(error) = _guard.record_startup_failure(&detail) {
                            eprintln!("failed to flush startup error to disk: {error}\n{detail}");
                        }
                        let _ = done.send(());
                        continue;
                    }
                    ReloadSignal::Finish(done) => {
                        drop(_guard);
                        let _ = done.send(());
                        return;
                    }
                };
                if let Some(level) = level {
                    filter_handle
                        .reload(app_filter(level))
                        .expect("the global subscriber outlives its logging reload thread");
                }

                // Rebuilding the writer opens a new file, so an unchanged rotation
                // (such as the startup effect replaying the defaults) keeps the
                // current one.
                if let Some(rotation) = rotation.filter(|r| *r != current_rotation) {
                    let (appender, guard) =
                        match get_file_appender(paths.app_logs_dir().into_std_path_buf(), rotation)
                        {
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
        })
        .context(StartReloadThreadSnafu)?;

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

    let installed = (|| {
        log_tracer::LogTracer::init().context(InstallLogTracerSnafu)?;
        tracing::subscriber::set_global_default(subscriber).context(InstallSubscriberSnafu)
    })();
    if let Err(error) = installed {
        LoggingShutdown(sender).finish_blocking();
        return Err(error);
    }
    Ok((sender, jobs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_logging::{FsLogFiles, LogFiles};
    use std::io::Write;

    #[test]
    fn logging_init_reports_the_unwritable_directory_before_installing_globals() {
        let directory = tempfile::tempdir().unwrap();
        let paths = PathResolver::with_base_dirs(
            directory.path().join("config").try_into().unwrap(),
            directory.path().join("data").try_into().unwrap(),
        );
        let logs = paths.app_logs_dir();
        fs::create_dir_all(logs.parent().unwrap()).unwrap();
        fs::write(&logs, b"not a directory").unwrap();
        let error = init(&mut Profilers::default(), &paths).err().unwrap();
        assert!(matches!(error, Error::CreateDirectory { path, .. } if path == logs));
    }

    #[test]
    fn startup_failure_is_on_disk_before_the_logging_guard_drops() {
        let dir = tempfile::tempdir().unwrap();
        let (_appender, mut guard) = get_file_appender(
            dir.path().into(),
            LogRotation {
                max_files: 3,
                max_file_size: 1,
            },
        )
        .unwrap();
        guard
            .record_startup_failure("setup -> application.yaml -> line 18 column 7")
            .unwrap();
        let catalog = FsLogFiles::new(dir.path().into(), "clash-nyanpasu".into())
            .catalog()
            .unwrap();
        let content = fs::read_to_string(dir.path().join(&catalog[0].name)).unwrap();
        let record: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(
            record["fields"]["error"],
            "setup -> application.yaml -> line 18 column 7"
        );
        assert_eq!(record["level"], "ERROR");
    }

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
