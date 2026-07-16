//! Tests for the configurable logger.

use std::sync::{Arc, Mutex};

use storage_kv::logger::{LogLevel, Logger, StderrLogger};
use storage_kv::{LsmEngine, LsmOptions};

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

fn base_opts() -> LsmOptions {
    LsmOptions {
        write_buffer_size: 64,
        max_write_buffer_number: 2,
        level0_file_num_compaction_trigger: 4,
        level0_slowdown_writes_trigger: 100,
        level0_stop_writes_trigger: 200,
        target_file_size_base: 256,
        max_bytes_for_level_base: 256,
        ..Default::default()
    }
}

#[test]
fn engine_accepts_custom_logger() {
    let dir = tempfile::tempdir().unwrap();
    let capture = Arc::new(CaptureLogger::default());
    let logger: Arc<dyn Logger> = capture.clone();
    let mut opts = base_opts();
    opts.logger = Some(logger);

    let engine = LsmEngine::open(dir.path(), opts).unwrap();
    engine.put(b"a", b"1").unwrap();
    assert_eq!(engine.get(b"a").unwrap(), Some(bytes::Bytes::from_static(b"1")));

    // Normal operations do not produce errors, so the capture logger should be
    // empty (or contain only messages we explicitly sent in the test).
    let msgs = capture.messages.lock().unwrap();
    assert!(msgs.is_empty(), "unexpected log messages: {:?}", *msgs);
}

#[test]
fn default_logger_is_noop() {
    let dir = tempfile::tempdir().unwrap();
    let opts = base_opts();

    let engine = LsmEngine::open(dir.path(), opts).unwrap();
    engine.put(b"a", b"1").unwrap();
    engine.sync().unwrap();
    // A no-op logger means no output and no panic.
}

#[test]
fn stderr_logger_formats_message() {
    let logger = StderrLogger;
    // Just exercise the code path; visually inspect stderr if desired.
    logger.log(LogLevel::Info, "stderr logger test message");
}
