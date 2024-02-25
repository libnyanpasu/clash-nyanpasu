use crate::{config, utils::dirs, Config};
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::{
    fs,
    io::IsTerminal,
    sync::{Arc, OnceLock},
};
use tracing_appender::{non_blocking::WorkerGuard, rolling::Rotation};
use tracing_subscriber::{filter, fmt, layer::SubscriberExt, EnvFilter};
struct WorkerGuardHolder(Option<Box<WorkerGuard>>);
impl WorkerGuardHolder {
    fn global() -> &'static Arc<Mutex<WorkerGuardHolder>> {
        static HOLDER: OnceLock<Arc<Mutex<WorkerGuardHolder>>> = OnceLock::new();
        HOLDER.get_or_init(|| Arc::new(Mutex::new(Self(None))))
    }

    fn replace(&mut self, guard: WorkerGuard) {
        self.0 = Some(Box::new(guard))
    }

    fn clear(&mut self) {
        self.0 = None
    }
}

/// initial instance global logger
pub fn init(log_level: Option<config::logging::LoggingLevel>) -> Result<()> {
    let log_dir = dirs::app_logs_dir().unwrap();
    if !log_dir.exists() {
        let _ = fs::create_dir_all(&log_dir);
    }
    let log_level = match log_level {
        Some(level) => level,
        None => Config::verge().data().get_log_level(),
    };

    let filter = EnvFilter::builder()
        .with_default_directive(std::convert::Into::<filter::LevelFilter>::into(log_level).into())
        .from_env_lossy();

    // register the logger
    let file_appender = tracing_appender::rolling::Builder::new()
        .filename_prefix("clash-nyanpasu")
        .filename_suffix("app.log")
        .rotation(Rotation::DAILY)
        .max_log_files(7) // TODO: make this configurable, default to 7 days
        .build(&log_dir)?;
    let (appender, _guard) = tracing_appender::non_blocking(file_appender);
    WorkerGuardHolder::global().lock().replace(_guard);
    let file_layer = fmt::layer()
        .json()
        .with_writer(appender)
        .with_line_number(true)
        .with_file(true);
    // .with_target(true)
    // .with_thread_ids(true)
    // .with_thread_names(true)
    // .with_current_span(true)
    // .with_span_list(true);

    // if debug build, log to stdout and stderr with all levels
    let terminal_layer = {
        #[cfg(debug_assertions)]
        {
            Some(
                fmt::Layer::new()
                    .with_ansi(std::io::stdout().is_terminal())
                    .compact()
                    .with_target(false)
                    .with_file(true)
                    .with_line_number(true)
                    .with_writer(std::io::stdout),
            )
        }
        #[cfg(not(debug_assertions))]
        {
            None
        }
    };

    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(terminal_layer);

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|x| anyhow!("setup logging error: {}", x))?;
    Ok(())
}
