use anyhow::Result;
use flexi_logger::{
    Age, Cleanup, Criterion, FileSpec, Naming,
    writers::{ArcFileLogWriter, FileLogWriter, FileLogWriterHandle},
};
use nyanpasu_config::application::LoggingLevel;
use snafu::{OptionExt as _, Snafu};
use std::path::PathBuf;
use tracing_appender::non_blocking::{NonBlocking, NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::{EnvFilter, filter};

/// Why the running logger could not be reconfigured.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum LoggerError {
    #[snafu(display("the logger reload thread has stopped"))]
    ReloadThreadStopped,
}

/// How the log file is split and how many of its files are kept. The two
/// limits change together because the file writer is rebuilt from both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogRotation {
    pub max_files: usize,
    /// Split the current file once it exceeds this many MiB.
    pub max_file_size: u64,
}

/// Reconfigures the running logger.
///
/// The two `Option`s are the shape the underlying reload signal already has:
/// `None` means "leave this half alone". The plan always carries both, but the
/// port keeps the signal's own vocabulary so the adapter stays a pass-through.
#[cfg_attr(test, mockall::automock)]
pub trait LoggerRefresher: Send + Sync + 'static {
    fn refresh(
        &self,
        level: Option<LoggingLevel>,
        rotation: Option<LogRotation>,
    ) -> Result<(), LoggerError>;
}

/// The running `tracing` subscriber, through the reload channel that
/// initializing it returned.
#[derive(Debug)]
pub struct TracingLoggerRefresher {
    reload: std::sync::mpsc::Sender<ReloadSignal>,
}

impl TracingLoggerRefresher {
    pub fn new(reload: std::sync::mpsc::Sender<ReloadSignal>) -> Self {
        Self { reload }
    }
}

impl LoggerRefresher for TracingLoggerRefresher {
    fn refresh(
        &self,
        level: Option<LoggingLevel>,
        rotation: Option<LogRotation>,
    ) -> Result<(), LoggerError> {
        self.reload
            .send((level, rotation))
            .ok()
            .context(ReloadThreadStoppedSnafu)
    }
}

pub type ReloadSignal = (Option<LoggingLevel>, Option<LogRotation>);

/// Keeps the file writer alive. Fields drop in declaration order: the guard
/// flushes the non-blocking queue into the file writer before the handle shuts
/// that writer down.
pub struct FileAppenderGuard {
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

pub fn get_file_appender(
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
pub fn app_filter(level: LoggingLevel) -> EnvFilter {
    let level = filter::LevelFilter::from(level);
    EnvFilter::builder()
        .with_default_directive(
            std::convert::Into::<filter::LevelFilter>::into(LoggingLevel::Warn).into(),
        )
        .from_env_lossy()
        .add_directive(format!("nyanpasu={level}").parse().unwrap())
        .add_directive(format!("clash_nyanpasu={level}").parse().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nyanpasu_logging::{FsLogFiles, LogFiles};
    use std::{fs, io::Write};

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

    /// The refresher owns no logger of its own: it forwards to the reload channel
    /// it was handed, and says so once nothing is left to receive.
    #[test]
    fn the_logger_refresher_forwards_to_the_reload_channel_it_was_given() {
        let (reload, signals) = std::sync::mpsc::channel();
        let refresher = TracingLoggerRefresher::new(reload);
        let rotation = LogRotation {
            max_files: 3,
            max_file_size: 10,
        };

        refresher
            .refresh(Some(LoggingLevel::Info), Some(rotation))
            .unwrap();
        assert_eq!(
            signals.try_recv().unwrap(),
            (Some(LoggingLevel::Info), Some(rotation))
        );

        drop(signals);
        assert!(refresher.refresh(None, Some(rotation)).is_err());
    }
}
