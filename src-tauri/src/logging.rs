//! Logs to stderr and to a daily file (7 days kept). Dictated text is never
//! logged unless the user enables it for debugging.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};

/// Keep the guard alive for the whole run: dropping it flushes and stops
/// the file writer.
pub fn init(dir: &Path) -> Option<WorkerGuard> {
    let filter = EnvFilter::try_from_env("VOCALIBRE_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,whisper_rs=warn,symphonia_format_riff=warn"));
    let file = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("vocalibre")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir);
    let (layer, guard) = match file {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (
                Some(fmt::layer().with_ansi(false).with_writer(writer)),
                Some(guard),
            )
        }
        Err(_) => (None, None),
    };
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(layer)
        .init();
    whisper_rs::install_logging_hooks();
    guard
}
