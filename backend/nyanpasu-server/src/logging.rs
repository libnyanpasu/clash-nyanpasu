//! The executable's reloadable file logger; its thread is explicitly joined at exit.

use std::{path::Path, sync::mpsc, thread};

use flexi_logger::{Age, Cleanup, Criterion, FileSpec, Naming, writers::FileLogWriter};
use nyanpasu_config::application::LoggingLevel;
use nyanpasu_core::client::effects::ports::{LogRotation, LoggerError, LoggerRefresher};
use nyanpasu_paths::PathResolver;
use tracing_subscriber::{Layer as _, layer::SubscriberExt as _, reload};

enum Message {
    Reload(Option<LoggingLevel>, Option<LogRotation>),
    Finish,
}

pub(crate) struct Logger(mpsc::Sender<Message>);
pub(crate) struct Logging {
    sender: mpsc::Sender<Message>,
    thread: thread::JoinHandle<()>,
}

impl LoggerRefresher for Logger {
    fn refresh(
        &self,
        level: Option<LoggingLevel>,
        rotation: Option<LogRotation>,
    ) -> Result<(), LoggerError> {
        #[cfg(feature = "panic-injection")]
        crate::panic_injection::logger_refresh(rotation.as_ref());
        self.0
            .send(Message::Reload(level, rotation))
            .map_err(|_| LoggerError::ReloadThreadStopped)
    }
}

impl Logging {
    pub(crate) fn finish(self) {
        let _ = self.sender.send(Message::Finish);
        if let Err(payload) = self.thread.join() {
            std::panic::resume_unwind(payload);
        }
    }
}

fn writer(
    directory: &Path,
    rotation: LogRotation,
) -> Result<
    (
        flexi_logger::writers::ArcFileLogWriter,
        flexi_logger::writers::FileLogWriterHandle,
    ),
    flexi_logger::FlexiLoggerError,
> {
    FileLogWriter::builder(
        FileSpec::default()
            .directory(directory)
            .basename("nyanpasu-server")
            .suffix("log"),
    )
    .append()
    .rotate(
        Criterion::AgeOrSize(Age::Day, rotation.max_file_size.saturating_mul(1024 * 1024)),
        Naming::Timestamps,
        Cleanup::KeepLogFiles(rotation.max_files),
    )
    .try_build_with_handle()
}

#[derive(Clone)]
struct Writer(flexi_logger::writers::ArcFileLogWriter);

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Writer {
    type Writer = flexi_logger::writers::ArcFileLogWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.0.clone()
    }
}

pub(crate) fn start(
    paths: &PathResolver,
    jobs: nyanpasu_jobs::LogCapture,
) -> anyhow::Result<(Logger, Logging)> {
    let directory = paths.app_logs_dir().into_std_path_buf();
    std::fs::create_dir_all(&directory)?;
    let rotation = LogRotation {
        max_files: 7,
        max_file_size: 10,
    };
    let (output, mut guard) = writer(&directory, rotation)?;
    let (file, file_handle) = reload::Layer::new(
        tracing_subscriber::fmt::layer()
            .json()
            .with_ansi(false)
            .with_writer(Writer(output)),
    );
    let (filter, filter_handle) = reload::Layer::new(tracing_subscriber::filter::LevelFilter::INFO);
    let subscriber = tracing_subscriber::registry()
        .with(file.with_filter(filter))
        .with(jobs.layer());
    tracing::subscriber::set_global_default(subscriber)?;
    let (sender, receiver) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("server-log-reload".into())
        .spawn(move || {
            let mut current = rotation;
            while let Ok(message) = receiver.recv() {
                let Message::Reload(level, rotation) = message else {
                    break;
                };
                if let Some(level) = level {
                    filter_handle
                        .reload(tracing_subscriber::filter::LevelFilter::from(level))
                        .expect("global subscriber outlives logger");
                }
                if let Some(rotation) = rotation.filter(|rotation| *rotation != current) {
                    match writer(&directory, rotation) {
                        Ok((output, next_guard)) => {
                            file_handle
                                .modify(|layer| *layer.writer_mut() = Writer(output))
                                .expect("global subscriber outlives logger");
                            guard = next_guard;
                            current = rotation;
                        }
                        Err(error) => tracing::error!(%error, "failed to reload log rotation"),
                    }
                }
            }
            drop(guard);
        })?;
    Ok((Logger(sender.clone()), Logging { sender, thread }))
}
