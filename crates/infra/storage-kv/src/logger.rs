//! Pluggable logging interface for the LSM engine.
//!
//! The engine defaults to a no-op logger so that callers who do not care about
//! internal diagnostics pay no cost.  A simple stderr logger is provided for
//! convenience; production deployments can supply their own `Logger`
//! implementation.

use std::fmt;
use std::sync::Arc;

/// Severity level for an engine log message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// A logger receives diagnostic messages from the engine and its background
/// workers.
///
/// Implementations must be `Send + Sync` because the logger is shared between
/// the foreground thread and background worker threads.  `Debug` is required so
/// that `LsmOptions` can derive `Debug`.
pub trait Logger: Send + Sync + std::fmt::Debug {
    /// Emit a log message.
    fn log(&self, level: LogLevel, message: &str);
}

/// A logger that discards every message.  This is the default.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopLogger;

impl Logger for NoopLogger {
    fn log(&self, _level: LogLevel, _message: &str) {}
}

/// A logger that writes messages to standard error in a simple fixed format.
#[derive(Debug, Clone, Copy, Default)]
pub struct StderrLogger;

impl Logger for StderrLogger {
    fn log(&self, level: LogLevel, message: &str) {
        eprintln!("[storage-kv {}] {}", level, message);
    }
}

/// Return a shared no-op logger instance.
pub fn noop_logger() -> Arc<dyn Logger> {
    Arc::new(NoopLogger)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct CaptureLogger {
        messages: Mutex<Vec<(LogLevel, String)>>,
    }

    impl Logger for CaptureLogger {
        fn log(&self, level: LogLevel, message: &str) {
            self.messages
                .lock()
                .unwrap()
                .push((level, message.to_string()));
        }
    }

    #[test]
    fn noop_logger_does_nothing() {
        let logger = NoopLogger;
        logger.log(LogLevel::Error, "should be silent");
    }

    #[test]
    fn capture_logger_records_messages() {
        let logger = CaptureLogger::default();
        logger.log(LogLevel::Warn, "hello");
        let msgs = logger.messages.lock().unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, LogLevel::Warn);
        assert_eq!(msgs[0].1, "hello");
    }

    #[test]
    fn log_level_ordering() {
        assert!(LogLevel::Debug < LogLevel::Info);
        assert!(LogLevel::Info < LogLevel::Warn);
        assert!(LogLevel::Warn < LogLevel::Error);
    }
}
