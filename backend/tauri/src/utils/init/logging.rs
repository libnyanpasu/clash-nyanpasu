use crate::{config, utils::dirs, Config};
use anyhow::{anyhow, bail, Result};
use parking_lot::Mutex;
use std::{
    fs,
    io::IsTerminal,
    sync::{
        mpsc::{self, Sender},
        OnceLock,
    },
    thread,
};
use tracing::error;
use tracing_appender::{
    non_blocking::{NonBlocking, WorkerGuard},
    rolling::Rotation,
};
use tracing_log::log_tracer;
use tracing_subscriber::{filter, fmt, layer::SubscriberExt, reload, EnvFilter};

use super::nyanpasu::LoggingLevel;

pub type ReloadSignal = (Option<config::nyanpasu::LoggingLevel>, Option<usize>);

struct Channel(Option<Sender<ReloadSignal>>);
impl Channel {
    fn globals() -> &'static Mutex<Channel> {
        static CHANNEL: OnceLock<Mutex<Channel>> = OnceLock::new();
        CHANNEL.get_or_init(|| Mutex::new(Channel(None)))
    }
}

pub fn refresh_logger(signal: ReloadSignal) -> Result<()> {
    let channel = Channel::globals().lock();
    match &channel.0 {
        Some(sender) => {
            let _ = sender.send(signal);
            Ok(())
        }
        None => bail!("no logger channel"),
    }
}

fn get_file_appender(max_files: usize) -> Result<(NonBlocking, WorkerGuard)> {
    let log_dir = dirs::app_logs_dir().unwrap();
    let file_appender = tracing_appender::rolling::Builder::new()
        .filename_prefix("clash-nyanpasu")
        .filename_suffix("app.log")
        .rotation(Rotation::DAILY)
        .max_log_files(max_files)
        .build(log_dir)?;
    Ok(tracing_appender::non_blocking(file_appender))
}

/// initial instance global logger
pub fn init() -> Result<()> {
    let log_dir = dirs::app_logs_dir().unwrap();
    if !log_dir.exists() {
        let _ = fs::create_dir_all(&log_dir);
    }
    let (log_level, log_max_files) = { (LoggingLevel::Debug, 7) }; // This is intended to capture config loading errors
    let (filter, filter_handle) = reload::Layer::new(
        EnvFilter::builder()
            .with_default_directive(
                std::convert::Into::<filter::LevelFilter>::into(LoggingLevel::Warn).into(),
            )
            .from_env_lossy()
            .add_directive(format!("nyanpasu={}", log_level).parse().unwrap())
            .add_directive(format!("clash_nyanpasu={}", log_level).parse().unwrap()),
    );

    // register the logger
    let (appender, _guard) = get_file_appender(log_max_files)?;
    let (file_layer, file_handle) = reload::Layer::new(
        fmt::layer()
            .json()
            .with_writer(appender)
            .with_line_number(true)
            .with_file(true),
    );

    // spawn a thread to handle the reload signal
    thread::spawn(move || {
        let mut _guard = _guard; // just hold here to keep the file open
        let (sender, receiver) = mpsc::channel::<ReloadSignal>();
        {
            let mut channel = Channel::globals().lock();
            channel.0 = Some(sender);
        }
        loop {
            let signal = receiver.recv().unwrap();
            if let Some(level) = signal.0 {
                filter_handle
                    .reload(
                        EnvFilter::builder()
                            .with_default_directive(
                                std::convert::Into::<filter::LevelFilter>::into(LoggingLevel::Warn)
                                    .into(),
                            )
                            .from_env_lossy()
                            .add_directive(format!("nyanpasu={}", level).parse().unwrap())
                            .add_directive(format!("clash_nyanpasu={}", level).parse().unwrap()),
                    )
                    .unwrap(); // panic if error
            }

            if let Some(max_files) = signal.1 {
                let (appender, guard) = match get_file_appender(max_files) {
                    Ok(x) => x,
                    Err(e) => {
                        error!("failed to create file appender: {}", e);
                        continue;
                    }
                };
                _guard = guard;
                if let Err(e) = file_handle.modify(|layer| *layer.writer_mut() = appender) {
                    error!("failed to modify file appender: {}", e);
                }
            }
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

    let subscriber = tracing_subscriber::registry().with(filter).with(file_layer);
    #[cfg(debug_assertions)]
    let subscriber = subscriber.with(terminal_layer);

    log_tracer::LogTracer::init()?;
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|x| anyhow!("setup logging error: {}", x))?;
    // reload the log level
    std::thread::spawn(move || {
        let config = Config::verge();
        let log_level = config.latest().get_log_level();
        let log_max_files = config.latest().max_log_files;
        let _ = refresh_logger((Some(log_level), log_max_files));
    });
    Ok(())
}
